#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
"""The push outbox's reservation (audit H14), with a replica that is really killed.

Two replicas share one PostgreSQL database, push configs and outbox. A webhook
receiver is a plain HTTP server here. A task is sent to replica A, which runs
the `park` executor (a working status, two artifacts, input-required). A is
built to send itself SIGKILL at its Nth outbox call (`A2A_KILL_AT`):

  arm      after the event is stored, before its rows are armed. The webhook must
           still be told, once, by replica B, which settles the reservation from
           the task store after its hold (about 63 s here).
  reserve  after the rows are reserved, before the event is stored. The webhook
           must NOT be told: the task never had the event.

The kill is the 2nd call, so the first event (the working status) is delivered
by A, and the second (the `early` artifact) is the one in the window.

Usage: outbox_live.py REPLICA LABEL arm|reserve [WAIT_SECONDS]
Exit status: 0 when the receiver saw what the case requires and the outbox is
empty at the end; 1 otherwise. Needs PostgreSQL as in README.md (set
A2A_LIVE_LOCAL=1 to use a local `psql` and no container)."""
import json
import sys
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

import common

BIN, LABEL, KILL_AT = sys.argv[1], sys.argv[2], sys.argv[3]
WAIT = float(sys.argv[4]) if len(sys.argv) > 4 else 150.0
assert KILL_AT in ("arm", "reserve"), KILL_AT
DB, PORT_A, PORT_B, HOOK = "a2a_outbox", 7880, 7881, 7890

SEEN: list[dict] = []
LOCK = threading.Lock()
T0 = time.time()


def kind(body: dict) -> str:
    """What one delivery carried, as `status:<STATE>` or `artifact:<id>`."""
    update = body.get("statusUpdate") or body.get("status_update")
    if update:
        return "status:" + str(update["status"]["state"])
    update = body.get("artifactUpdate") or body.get("artifact_update")
    if update:
        a = update["artifact"]
        return "artifact:" + str(a.get("artifactId") or a.get("artifact_id"))
    return "other:" + ",".join(sorted(body))


class Hook(BaseHTTPRequestHandler):
    def do_POST(self):
        raw = self.rfile.read(int(self.headers.get("content-length", 0)))
        try:
            body = json.loads(raw)
        except ValueError:
            body = {}
        with LOCK:
            SEEN.append({"at": time.time() - T0, "kind": kind(body),
                         "delivery_id": self.headers.get("A2A-Delivery-Id")})
        self.send_response(200)
        self.end_headers()

    def log_message(self, *args):
        pass


def seen_kinds() -> list[str]:
    with LOCK:
        return [s["kind"] for s in SEEN]


server = ThreadingHTTPServer(("127.0.0.1", HOOK), Hook)
threading.Thread(target=server.serve_forever, daemon=True).start()

common.wait_ready()
url = common.fresh_db(DB)
env = {"A2A_OUTBOX": "1"}
b = common.start(BIN, "B", PORT_B, url, common.OUT / f"outbox-{LABEL}-b.log", "warn", env)
a = common.start(BIN, "A", PORT_A, url, common.OUT / f"outbox-{LABEL}-a.log", "warn",
                 dict(env, A2A_KILL_AT=KILL_AT, A2A_KILL_NTH="2"))
try:
    r = common.rpc(PORT_A, "SendMessage", {
        "message": common.message("park"),
        "configuration": {"returnImmediately": True,
                          "taskPushNotificationConfig": {"url": f"http://127.0.0.1:{HOOK}/hook", "token": "tok"}}})
    task = r.get("result", {}).get("task")
    assert task, r
    tid = task["id"]
    for _ in range(200):
        if a.poll() is not None:
            break
        time.sleep(0.1)
    assert a.poll() is not None, "replica A never reached its kill point"
    killed_at = time.time() - T0
    print(f"{LABEL} ({KILL_AT}): A died (exit {a.returncode}) at {killed_at:.1f} s; "
          f"receiver so far: {seen_kinds()}; outbox rows: "
          f"{common.psql('SELECT count(*) || chr(32) || count(*) FILTER (WHERE NOT armed) FROM a2a_push_outbox', DB)} (rows, unarmed)")

    def rows() -> int:
        return int(common.psql("SELECT count(*) FROM a2a_push_outbox", DB) or 0)

    deadline = time.time() + WAIT
    while time.time() < deadline:
        kinds = seen_kinds()
        if KILL_AT == "arm" and any(k.startswith("artifact:") for k in kinds) and rows() == 0:
            break
        if KILL_AT == "reserve" and rows() == 0 and time.time() - T0 > killed_at + 70:
            break
        time.sleep(1)
    time.sleep(5)  # a duplicate, or a wrong delivery, would land here
    with LOCK:
        final = list(SEEN)
    # At least once: a delivery whose owner died before it was removed is sent
    # again under the same delivery id. What must not happen is one event under
    # two ids, or an event the task never stored.
    first = {}
    for s in final:
        first.setdefault(s["delivery_id"], s)
    distinct = list(first.values())
    artifacts = [s for s in distinct if s["kind"].startswith("artifact:")]
    working = [s for s in distinct if s["kind"] == "status:TASK_STATE_WORKING"]
    retries = len(final) - len(distinct)
    settled = common.psql("SELECT count(*) FROM a2a_push_outbox", DB)
    order = [s["kind"] for s in sorted(final, key=lambda s: s["at"])]
    print(f"{LABEL} ({KILL_AT}): receiver saw {[(s['kind'], round(s['at'], 1)) for s in final]}; "
          f"{retries} retry(ies) under an id already seen; outbox rows left {settled}")
    ok = len(working) == 1 and settled == "0" and None not in first
    if KILL_AT == "arm":
        ok = ok and len(artifacts) == 1 and artifacts[0]["at"] > killed_at
        ok = ok and order.index("artifact:A-early") < order.index("status:TASK_STATE_FAILED")
        print(f"{LABEL}: the artifact in the window was delivered once, "
              f"{artifacts[0]['at'] - killed_at:.0f} s after the kill, by the survivor, before the ending"
              if ok else f"{LABEL}: FAILED to deliver it once, in order")
    else:
        ok = ok and len(artifacts) == 0
        print(f"{LABEL}: no delivery for the event the task never stored" if ok
              else f"{LABEL}: FAILED, a delivery or a row for an event that was not stored")
    sys.exit(0 if ok else 1)
finally:
    for p in (a, b):
        if p.poll() is None:
            p.terminate()
            p.wait(timeout=20)
    server.shutdown()
