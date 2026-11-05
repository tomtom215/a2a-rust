#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
"""Live proof of the turn hand-off on a worker's graceful shutdown (ADR 0054, audit D-21).

Two workers (`a2a-live-worker`, JSON-RPC, one `PostgresTaskStore` database) behind a
TCP proxy that sends each new connection to the first worker that accepts it (A, then
B): a stand-in for a load balancer during a rolling deploy, where the stopping replica
stops accepting first. A supervisor (`a2a-live-supervisor start N`) delegates N `hold`
children through the proxy, so all of them land on A. At --signal-at seconds A gets
SIGINT, its shutdown signal.

Scenarios:
  h  RESUMABLE=1 on both workers: A hands every running turn back; each delegation
     continues its child (`HandOffPolicy`), which the proxy now sends to B.
  x  control, RESUMABLE=0: A ends the turns failed/transient, as before ADR 0054.

    python3 scripts/live/delegation/handoff_live.py --worker BIN --supervisor BIN \\
        --scenarios h,x --runs 3 --children 5 --hold-secs 20 --signal-at 8 --out DIR
"""

from __future__ import annotations

import argparse
import asyncio
import hashlib
import json
import os
import random
import re
import signal
import socket
import subprocess
import sys
import threading
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import pglocal  # noqa: E402

EXPECTATIONS = """\
Written 2026-11-05 before any run of this driver. Per run: 5 children, hold 20 s,
SIGINT to worker A 8 s after the supervisor starts; A's completion window is the
default 5 s, so A stops the turns about 13 s in.

h (RESUMABLE=1):
1. The supervisor exits 0; its 5 OUTCOME lines are all `completed`; SUMMARY says so.
2. Worker A: 5 `start ... rerun=false` lines, 5 `token-fired`, 0 `cancel-hook`,
   0 `completed`. Nothing on A completed and nothing was cancelled there.
3. Worker B: 5 `start ... rerun=true` lines (the stored task carried the mark), then
   5 `completed`.
4. Database afterwards: 5 tasks `TASK_STATE_COMPLETED`, 0 in any other state; 5
   journal records `completed`, 0 open.
5. The supervisor ends about 13 s + 1 s (first continuation wait) + 20 s = 34 s after
   it started, within 10 s either way.

x (control, RESUMABLE=0):
1. The 5 OUTCOME lines are all `failed` (transient), the supervisor exits 0.
2. Worker A: 5 `token-fired` and 5 `cancel-hook` (the shutdown runs the hook before it
   writes the transient failure); worker B: 0 `start` lines.
3. Database: 5 tasks `TASK_STATE_FAILED`.

What would make me wrong: the continuation reaching A (still draining) and being
refused past the policy's three tries; B refusing the send because A's claim is not
yet released; the hand-off status not reaching the supervisor before A's stream
closes, so the delegation resumes instead (then, with the default ResumePolicy, it
would follow a task nobody runs until the reaper's window, and the run would time
out).
"""


def free_port() -> int:
    with socket.socket() as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


class Failover:
    """Per connection: the first backend that accepts, in order."""

    def __init__(self, port: int, backends: list[int]):
        self.port, self.backends = port, backends
        self.loop = asyncio.new_event_loop()
        self.routed: dict[int, int] = {b: 0 for b in backends}
        threading.Thread(target=self._run, daemon=True).start()

    def _run(self) -> None:
        asyncio.set_event_loop(self.loop)
        server = self.loop.run_until_complete(
            asyncio.start_server(self._client, "127.0.0.1", self.port))
        self.loop.run_until_complete(server.serve_forever())

    async def _client(self, reader, writer) -> None:
        for b in self.backends:
            try:
                br, bw = await asyncio.open_connection("127.0.0.1", b)
            except OSError:
                continue
            self.routed[b] += 1
            await asyncio.gather(self._pipe(reader, bw), self._pipe(br, writer))
            return
        writer.close()

    @staticmethod
    async def _pipe(r, w) -> None:
        try:
            while data := await r.read(65536):
                w.write(data)
                await w.drain()
        except (ConnectionError, OSError):
            pass
        finally:
            try:
                w.close()
            except OSError:
                pass


def count(path: Path, pattern: str) -> int:
    return sum(1 for line in path.read_text().splitlines() if re.search(pattern, line))


def one_run(args, scenario: str, run: int, out: Path) -> dict:
    tag = f"{scenario}{run}"
    db = f"a2a_handoff_{tag}_{random.randrange(16**6):06x}"
    url = pglocal.fresh_db(db)
    pa, pb, pp = free_port(), free_port(), free_port()
    base = {"PATH": os.environ["PATH"], "DATABASE_URL": url,
            "HOLD_SECS": str(args.hold_secs), "RESUMABLE": "1" if scenario == "h" else "0"}
    logs = {k: out / f"{tag}-{k}.log" for k in ("a", "b", "sup")}
    a = subprocess.Popen([args.worker], env={**base, "PORT": str(pa), "NAME": "A"},
                         stderr=logs["a"].open("w"), stdout=subprocess.DEVNULL)
    b = subprocess.Popen([args.worker], env={**base, "PORT": str(pb), "NAME": "B"},
                         stderr=logs["b"].open("w"), stdout=subprocess.DEVNULL)
    time.sleep(1.5)
    proxy = Failover(pp, [pa, pb])
    t0 = time.monotonic()
    sup = subprocess.Popen([args.supervisor, "start", str(args.children)],
                           env={"PATH": os.environ["PATH"], "DATABASE_URL": url,
                                "WORKER_URL": f"http://127.0.0.1:{pp}", "A2A_OWNER": f"sup-{tag}"},
                           stdout=logs["sup"].open("w"), stderr=subprocess.STDOUT)
    time.sleep(args.signal_at)
    a.send_signal(signal.SIGINT)
    try:
        code = sup.wait(timeout=args.timeout)
    except subprocess.TimeoutExpired:
        sup.kill()
        code = "timeout"
    took = round(time.monotonic() - t0, 1)
    snap = pglocal.snapshot(db)
    for p in (a, b):
        if p.poll() is None:
            p.send_signal(signal.SIGINT)
            try:
                p.wait(timeout=15)
            except subprocess.TimeoutExpired:
                p.kill()
    pglocal.drop_db(db)
    outcomes = re.findall(r"OUTCOME .* outcome=(\S+)", logs["sup"].read_text())
    return {
        "run": tag, "supervisor_exit": code, "seconds": took,
        "outcomes": {o: outcomes.count(o) for o in sorted(set(outcomes))},
        "a_start": count(logs["a"], r"start task=.*rerun=false"),
        "a_token_fired": count(logs["a"], "token-fired"),
        "a_cancel_hook": count(logs["a"], "cancel-hook"),
        "a_completed": count(logs["a"], r"\scompleted task="),
        "b_start_rerun": count(logs["b"], r"start task=.*rerun=true"),
        "b_start_other": count(logs["b"], r"start task=.*rerun=false"),
        "b_completed": count(logs["b"], r"\scompleted task="),
        "proxy_connections": {"A": proxy.routed[pa], "B": proxy.routed[pb]},
        **{k: v for k, v in snap.items() if k != "connections"},
    }


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--worker", required=True)
    ap.add_argument("--supervisor", required=True)
    ap.add_argument("--scenarios", default="h,x")
    ap.add_argument("--runs", type=int, default=3)
    ap.add_argument("--children", type=int, default=5)
    ap.add_argument("--hold-secs", type=int, default=20)
    ap.add_argument("--signal-at", type=float, default=8)
    ap.add_argument("--timeout", type=float, default=150)
    ap.add_argument("--out", required=True)
    args = ap.parse_args()
    out = Path(args.out)
    out.mkdir(parents=True, exist_ok=True)
    print(f"expectations sha256 {hashlib.sha256(EXPECTATIONS.encode()).hexdigest()}", flush=True)
    print(f"server {pglocal.ensure_running()}", flush=True)
    results = []
    with (out / "results.jsonl").open("w") as f:
        for scenario in args.scenarios.split(","):
            for run in range(1, args.runs + 1):
                r = one_run(args, scenario, run, out)
                results.append(r)
                f.write(json.dumps(r) + "\n")
                print(json.dumps(r), flush=True)
    return 0


if __name__ == "__main__":
    sys.exit(main())
