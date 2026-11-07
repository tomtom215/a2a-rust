#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
"""What N idle cross-replica subscribers cost the database and a foreground send.

Two replica processes (`replica/`) share one database. Replica A parks N tasks;
replica B holds one `SubscribeToTask` stream on each, so every stream polls the
store for tasks B does not run (audit E-F25). After a warm-up long enough for
the back-off to reach its ceiling, the script measures

  * the statements-per-second the database commits while the streams idle:
    `pg_stat_database` (`xact_commit` + `xact_rollback`) over a window; each
    statement the replicas run on a pool connection is its own transaction, so
    this counts them (it is not `pg_stat_statements`, which this server does not
    preload);
  * the latency of M foreground `SendMessage` calls to B, sequential, with the
    streams still open: median, p90, p99 and the spread.

    subscribe_idle_live.py REPLICA_BIN poll|notify|control [--subs N] [--sends M]

`poll`: no log notifications (E-F25 (a) only). `notify`: both replicas built with
`A2A_LOG_NOTIFY=pool` (E-F25 (b)). `control`: no subscribers at all, for the
foreground latency the other two are compared with. A replica's pool is
`A2A_POOL_SIZE` (default 10). Prints one JSON line. Needs `psql` and the database
server of `A2A_TEST_POSTGRES_URL`.
"""

from __future__ import annotations

import argparse
import asyncio
import http.client
import json
import os
import statistics
import subprocess
import sys
import threading
import time
import uuid

ADMIN = os.environ.get("A2A_TEST_POSTGRES_URL", "postgres://postgres:postgres@localhost:5432/postgres")
BASE = ADMIN.rsplit("/", 1)[0]


def psql(sql: str, db: str = "postgres") -> str:
    url = f"{BASE}/{db}"
    out = subprocess.run(["psql", url, "-tAq", "-c", sql], capture_output=True, text=True)
    return out.stdout.strip()


def body(method: str, params: dict) -> bytes:
    return json.dumps({"jsonrpc": "2.0", "id": 1, "method": method, "params": params}).encode()


def message(text: str) -> dict:
    return {"messageId": str(uuid.uuid4()), "role": "ROLE_USER", "parts": [{"text": text}]}


def call(conn: http.client.HTTPConnection, method: str, params: dict) -> dict:
    conn.request("POST", "/", body(method, params),
                 {"content-type": "application/json", "A2A-Version": "1.0"})
    return json.load(conn.getresponse())


def start(binary: str, name: str, port: int, url: str, cpu: int, env: dict) -> subprocess.Popen:
    environ = {"PATH": os.environ["PATH"], "PORT": str(port), "NAME": name, "DATABASE_URL": url, **env}
    p = subprocess.Popen(["taskset", "-c", str(cpu), binary], env=environ,
                         stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    for _ in range(400):
        try:
            c = http.client.HTTPConnection("127.0.0.1", port, timeout=5)
            call(c, "GetTask", {"id": "none"})
            return p
        except Exception:
            time.sleep(0.05)
    raise SystemExit(f"replica {name} did not answer")


def park(port: int, n: int) -> list[str]:
    """Parks `n` tasks through replica A, 16 at a time; returns their ids."""
    ids: list[str] = []
    lock = threading.Lock()

    def work(count: int) -> None:
        c = http.client.HTTPConnection("127.0.0.1", port, timeout=60)
        for _ in range(count):
            r = call(c, "SendMessage", {"message": message("park")})
            with lock:
                ids.append(r["result"]["task"]["id"])

    threads = [threading.Thread(target=work, args=(n // 16 + (1 if i < n % 16 else 0),)) for i in range(16)]
    [t.start() for t in threads]
    [t.join() for t in threads]
    return ids


async def subscribe(port: int, ids: list[str], hold: asyncio.Event) -> int:
    """One SSE stream on each task, opened and kept open; returns how many
    answered 200 with a first frame."""
    ok = 0

    async def one(task_id: str) -> None:
        nonlocal ok
        reader, writer = await asyncio.open_connection("127.0.0.1", port)
        payload = body("SubscribeToTask", {"id": task_id})
        writer.write((f"POST / HTTP/1.1\r\nHost: 127.0.0.1\r\ncontent-type: application/json\r\n"
                      f"accept: text/event-stream\r\nA2A-Version: 1.0\r\ncontent-length: {len(payload)}\r\n\r\n"
                      ).encode() + payload)
        await writer.drain()
        head = await reader.readuntil(b"\r\n\r\n")
        if b" 200 " in head.split(b"\r\n", 1)[0]:
            await reader.readuntil(b"\n\n")  # the snapshot frame
            ok += 1
        await hold.wait()
        writer.close()

    tasks = [asyncio.create_task(one(t)) for t in ids]
    while ok < len(ids) and not all(t.done() for t in tasks):
        await asyncio.sleep(0.1)
    streams.extend(tasks)
    return ok


streams: list[asyncio.Task] = []


def commits(db: str) -> int:
    row = psql(f"SELECT xact_commit + xact_rollback FROM pg_stat_database WHERE datname = '{db}'")
    return int(row)


async def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("binary")
    ap.add_argument("config", choices=["poll", "notify", "control"])
    ap.add_argument("--subs", type=int, default=200)
    ap.add_argument("--sends", type=int, default=300)
    ap.add_argument("--warm", type=float, default=20)
    ap.add_argument("--window", type=float, default=30)
    ap.add_argument("--label", default="run")
    a = ap.parse_args()
    os.sched_setaffinity(0, {2})
    db = f"a2a_efp25_{a.label}"
    psql(f"DROP DATABASE IF EXISTS {db} WITH (FORCE)")
    psql(f"CREATE DATABASE {db}")
    url = f"{BASE}/{db}"
    env = {"A2A_LOG_NOTIFY": "pool"} if a.config == "notify" else {}
    pa = start(a.binary, "A", 18081, url, 0, env)
    pb = start(a.binary, "B", 18082, url, 1, env)
    try:
        subs = 0
        hold = asyncio.Event()
        if a.config != "control":
            ids = await asyncio.to_thread(park, 18081, a.subs)
            subs = await subscribe(18082, ids, hold)
        await asyncio.sleep(a.warm)
        before, t0 = commits(db), time.monotonic()
        await asyncio.sleep(a.window)
        per_s = (commits(db) - before) / (time.monotonic() - t0)
        conn = http.client.HTTPConnection("127.0.0.1", 18082, timeout=60)
        for _ in range(10):
            call(conn, "SendMessage", {"message": message("finish")})
        ms = []
        for _ in range(a.sends):
            t = time.perf_counter()
            r = call(conn, "SendMessage", {"message": message("finish")})
            ms.append((time.perf_counter() - t) * 1000)
            assert r["result"]["task"]["status"]["state"] == "TASK_STATE_COMPLETED", r
        ms.sort()
        q = statistics.quantiles(ms, n=100)
        print(json.dumps({
            "config": a.config, "subs": subs, "label": a.label,
            "idle_statements_per_s": round(per_s, 1),
            "send_ms": {"median": round(statistics.median(ms), 2), "p90": round(q[89], 2),
                        "p99": round(q[98], 2), "min": round(ms[0], 2), "max": round(ms[-1], 2)},
            "load1": os.getloadavg()[0],
        }), flush=True)
        hold.set()
    finally:
        for p in (pa, pb):
            p.terminate()
        for p in (pa, pb):
            p.wait()
        psql(f"DROP DATABASE IF EXISTS {db} WITH (FORCE)")


if __name__ == "__main__":
    sys.exit(asyncio.run(main()))
