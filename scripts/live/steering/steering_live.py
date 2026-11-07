# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
"""Steering a running task across two separate server processes (ADR 0036).

Two `a2a-live-steering-agent` processes on one PostgreSQL database, as two
replicas are: a task is started on A, steering messages are sent to B, and the
log of A's executor says what it read and when. Scenarios:

  handoff   messages to B reach the executor on A, in order, `handed-off`
  notify    the same with NOTIFY on and a 3 s renewal interval: the first
            message reaches A far sooner than a poll could (and a control run
            with NOTIFY off, which is only reported)
  full      an inbox of 2 and an executor that reads late: 4 messages are
            accepted (2 in the inbox, 2 stored behind it), the 5th is a 503,
            and the executor then reads all four in order
  died      A is killed with SIGKILL: a message sent at once is accepted (its
            claim is live until its time to live) and nothing reads it, which
            is a limit and is reported as one; probes then say how long after
            the kill a message is refused instead of accepted.

Usage, from the repository root (see README.md for the build):

    python3 scripts/live/steering/steering_live.py \\
        --agent target/live/steering/debug/a2a-live-steering-agent

Exit status: 0 when every expectation held, 1 otherwise.
"""

from __future__ import annotations

import argparse
import json
import os
import pathlib
import re
import signal
import subprocess
import sys
import time
import urllib.error
import urllib.request
import uuid

HERE = pathlib.Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent / "delegation"))
import pglocal  # noqa: E402  (the local PostgreSQL helpers, no Docker)

STEERING = "https://a2a-rust.com/extensions/steering/v1"
READ = re.compile(r"\[(?P<name>\w+)\] t=(?P<t>\d+) read task=(?P<task>\S+) from=(?P<from>\S+) text=(?P<text>.*)$")
FAILED = []
DB_FULL = "steer_live_full"


def check(ok: bool, what: str) -> None:
    print(("  ok    " if ok else "  FAIL  ") + what, flush=True)
    if not ok:
        FAILED.append(what)


def rpc(port: int, method: str, params: dict, timeout: float = 30) -> dict:
    body = json.dumps({"jsonrpc": "2.0", "id": 1, "method": method, "params": params}).encode()
    req = urllib.request.Request(f"http://127.0.0.1:{port}/", data=body,
                                 headers={"content-type": "application/json", "a2a-version": "1.0"})
    opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
    try:
        with opener.open(req, timeout=timeout) as r:
            return json.loads(r.read())
    except urllib.error.HTTPError as e:
        return json.loads(e.read() or b"{}") | {"http_status": e.code}


def message(text: str, task: str | None = None, steering: bool = False) -> dict:
    m = {"messageId": uuid.uuid4().hex, "role": "ROLE_USER", "parts": [{"text": text}]}
    if task:
        m["taskId"] = task
    if steering:
        m["extensions"] = [STEERING]
    return m


class Agent:
    """One agent process, its log, and what it read."""

    def __init__(self, binary: str, name: str, port: int, db_url: str, out: pathlib.Path, **env):
        self.name, self.port = name, port
        self.log_path = out / f"{name}.log"
        self.log = open(self.log_path, "w")
        settings = {**os.environ, "NAME": name, "PORT": str(port), "DATABASE_URL": db_url,
                    **{k: str(v) for k, v in env.items()}}
        self.proc = subprocess.Popen([binary], env=settings, stderr=self.log, stdout=subprocess.DEVNULL)
        for _ in range(200):
            if "serving" in self.log_path.read_text():
                return
            time.sleep(0.1)
        raise RuntimeError(f"{name} did not start: {self.log_path.read_text()}")

    def reads(self) -> list[dict]:
        found = []
        for line in self.log_path.read_text().splitlines():
            m = READ.match(line)
            if m:
                found.append({**m.groupdict(), "t": int(m["t"])})
        return found

    def wait_reads(self, n: int, secs: float = 20) -> list[dict]:
        deadline = time.time() + secs
        while time.time() < deadline:
            reads = self.reads()
            if len(reads) >= n:
                return reads
            time.sleep(0.02)
        return self.reads()

    def kill9(self) -> None:
        self.proc.send_signal(signal.SIGKILL)
        self.proc.wait()

    def stop(self) -> None:
        if self.proc.poll() is None:
            self.proc.send_signal(signal.SIGINT)
            try:
                self.proc.wait(10)
            except subprocess.TimeoutExpired:
                self.proc.kill()
        self.log.close()


def now_ms() -> int:
    return int(time.time() * 1000)


def start_task(agent: Agent, text: str) -> str:
    answer = rpc(agent.port, "SendMessage", {"message": message(text),
                                             "configuration": {"returnImmediately": True}})
    task = answer["result"]["task"]["id"]
    deadline = time.time() + 20
    while f"start task={task}" not in agent.log_path.read_text() and time.time() < deadline:
        time.sleep(0.02)
    return task


def steer(agent: Agent, task: str, text: str) -> dict:
    sent = now_ms()
    answer = rpc(agent.port, "SendMessage", {"message": message(text, task, steering=True)})
    delivery = ((answer.get("result", {}).get("task", {}).get("metadata") or {}).get("a2a-rust.com/steering") or {}).get("delivery")
    return {"sent": sent, "answered": now_ms(), "delivery": delivery, "answer": answer}


def task_state(agent: Agent, task: str) -> str:
    answer = rpc(agent.port, "GetTask", {"id": task})
    return answer.get("result", {}).get("status", {}).get("state", "?")


def wait_stored(db: str, count: int, secs: float = 20) -> None:
    """Waits until the steering table of `db` holds `count` rows (what the replica
    that holds the claim has not yet moved into its inbox)."""
    deadline = time.time() + secs
    while time.time() < deadline:
        if pglocal.psql("SELECT count(*) FROM a2a_task_steering", db) == str(count):
            return
        time.sleep(0.02)
    raise RuntimeError(f"{db}: the steering table never held {count} row(s)")


def wait_listening(db: str, secs: float = 20) -> None:
    """Waits until a session of `db` has issued LISTEN: the replica that runs the
    task is then told of a message by NOTIFY, not found by its next look."""
    deadline = time.time() + secs
    while time.time() < deadline:
        if pglocal.psql(f"SELECT count(*) FROM pg_stat_activity WHERE datname = '{db}' AND query ILIKE 'listen%'", db) != "0":
            return
        time.sleep(0.02)
    raise RuntimeError(f"{db}: no session ever listened")


def wait_state(agent: Agent, task: str, want: str, secs: float = 20) -> str:
    """The task's state once it is `want`, or as it stands after `secs`."""
    deadline = time.time() + secs
    state = task_state(agent, task)
    while state != want and time.time() < deadline:
        time.sleep(0.05)
        state = task_state(agent, task)
    return state


def pair(binary: str, db: str, out: pathlib.Path, **env) -> tuple[Agent, Agent]:
    url = pglocal.fresh_db(db)
    a = Agent(binary, "A", 18101, url, out, **env)
    b = Agent(binary, "B", 18102, url, out, **env)
    return a, b


def handoff(binary: str, out: pathlib.Path) -> None:
    print("handoff: messages to B reach the executor on A, in order", flush=True)
    a, b = pair(binary, "steer_live_handoff", out)
    try:
        task = start_task(a, "read")
        sent = [steer(b, task, t) for t in ("one", "two", "three", "done")]
        reads = a.wait_reads(4)
        check([s["delivery"] for s in sent] == ["handed-off"] * 4, f"B answered handed-off each time: {[s['delivery'] for s in sent]}")
        check([r["text"] for r in reads] == ["one", "two", "three", "done"], f"A's executor read them in order: {[r['text'] for r in reads]}")
        check(all(r["name"] == "A" and r["task"] == task for r in reads), "all on A, for the task")
        for s, r in zip(sent, reads):
            print(f"    '{r['text']}': sent at +0 ms, answered after {s['answered'] - s['sent']} ms, read {r['t'] - s['sent']} ms after it was sent", flush=True)
        state = wait_state(b, task, "TASK_STATE_COMPLETED")
        check(state == "TASK_STATE_COMPLETED", f"the task completed: {state}")
    finally:
        a.stop(); b.stop()


def notify(binary: str, out: pathlib.Path) -> None:
    print("notify: NOTIFY on, renewal 3 s: the first message arrives far sooner than a poll", flush=True)
    for label, flag in (("NOTIFY on", 1), ("control, NOTIFY off", 0)):
        a, b = pair(binary, f"steer_live_notify{flag}", out, STEER_NOTIFY=flag, RENEWAL_MS=3000, TTL_MS=9000)
        try:
            task = start_task(a, "read")
            if flag:
                wait_listening(f"steer_live_notify{flag}")
            first = steer(b, task, "first")
            reads = a.wait_reads(1)
            took = reads[0]["t"] - first["sent"] if reads else None
            print(f"    {label}: the first message was read {took} ms after it was sent (renewal 3000 ms)", flush=True)
            if flag == 1:
                check(took is not None and took < 1500, f"announced, not polled: {took} ms < 1500 ms")
            steer(b, task, "done")
            a.wait_reads(2)
        finally:
            a.stop(); b.stop()


def full(binary: str, out: pathlib.Path) -> None:
    print("full: an inbox of 2, an executor that reads late: 4 accepted, the 5th is a 503", flush=True)
    a, b = pair(binary, DB_FULL, out, STEER_CAP=2, LATE_SECS=4, RENEWAL_MS=200)
    try:
        task = start_task(a, "readlate")
        answers = []
        for i in range(1, 5):
            answers.append(steer(b, task, f"m{i}"))
            if i == 2:
                wait_stored(DB_FULL, 0)  # A has moved the first two into its inbox: room in the store
        fifth = steer(b, task, "m5")
        check([x["delivery"] for x in answers] == ["handed-off"] * 4, f"four accepted: {[x['delivery'] for x in answers]}")
        error = fifth["answer"].get("error", {})
        check(error.get("code") == -32603 and "steering inbox" in error.get("message", ""),
              f"the fifth is refused as a full inbox: {error.get('message', fifth['answer'])}")
        check(fifth["answer"].get("error", {}).get("data", [{}])[0].get("retryDelay") == "1s", "with RetryInfo 1s")
        reads = a.wait_reads(4, secs=30)
        check([r["text"] for r in reads] == ["m1", "m2", "m3", "m4"], f"the executor read all four, in order: {[r['text'] for r in reads]}")
        steer(b, task, "done")
        a.wait_reads(5)
    finally:
        a.stop(); b.stop()


def died(binary: str, out: pathlib.Path) -> None:
    print("died: A is killed with SIGKILL while it runs the task", flush=True)
    a, b = pair(binary, "steer_live_died", out, TTL_MS=3000, RENEWAL_MS=500)
    try:
        task = start_task(a, "read")
        check(wait_state(a, task, "TASK_STATE_WORKING") == "TASK_STATE_WORKING", "the task is working on A when A is killed")
        a.kill9()
        early = steer(b, task, "sent just after the kill")
        print(f"    LIMIT: a message sent {early['answered'] - early['sent']} ms after the kill was answered {early['delivery']!r}: "
              "the dead replica's claim is live until its time to live, and nothing reads what is handed to it", flush=True)
        check(early["delivery"] == "handed-off", "the claim of a replica that has just died is still live: accepted")
        killed = early["sent"]
        deadline = time.time() + 40
        late = steer(b, task, "probe")
        probes = 1
        while late["delivery"] is not None and time.time() < deadline:
            time.sleep(0.5)
            late = steer(b, task, "probe")
            probes += 1
        error = late["answer"].get("error", {})
        print(f"    after {late['sent'] - killed} ms and {probes} probe(s) a message was refused: "
              f"{error.get('message', late['answer'])!r}; the task is {task_state(b, task)}", flush=True)
        check(late["delivery"] is None and bool(error), "once the claim lapsed a message is refused, not delivered")
        check(not any(r["text"] in ("probe",) or r["text"].startswith("sent") for r in b.reads()), "B's executor read none of them: they were for A")
    finally:
        a.stop(); b.stop()


SCENARIOS = {"handoff": handoff, "notify": notify, "full": full, "died": died}


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--agent", required=True, help="the a2a-live-steering-agent binary")
    ap.add_argument("--scenarios", default=",".join(SCENARIOS))
    ap.add_argument("--out", default=os.environ.get("A2A_LIVE_OUT", "target/live/steering-run"))
    args = ap.parse_args()
    out = pathlib.Path(args.out)
    out.mkdir(parents=True, exist_ok=True)
    print(f"PostgreSQL {pglocal.ensure_running()}, logs in {out}", flush=True)
    for name in args.scenarios.split(","):
        SCENARIOS[name](args.agent, out)
    for db in ("handoff", "notify1", "notify0", "full", "died"):
        pglocal.drop_db(f"steer_live_{db}")
    print(f"\n{'FAIL' if FAILED else 'PASS'}: {len(FAILED)} expectation(s) failed", flush=True)
    for what in FAILED:
        print("  -", what)
    return 1 if FAILED else 0


if __name__ == "__main__":
    sys.exit(main())
