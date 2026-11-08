# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
"""What the replica-chaos scripts share: a fleet of target processes on one
PostgreSQL database, a round-robin TCP proxy in front of them, a TCP proxy in
front of the database that can pause or cut every connection, and the
invariants checked afterwards with SQL, not by eye.

Needs: the target binary (A2A_HOSTILE_BIN), a local PostgreSQL (A2A_LIVE_PG).
"""

from __future__ import annotations

import asyncio
import json
import os
import signal
import threading
import time
import urllib.parse

from lib import PG, Target, fresh_db, port, psql

TERMINAL = ("TASK_STATE_COMPLETED", "TASK_STATE_FAILED", "TASK_STATE_CANCELED", "TASK_STATE_REJECTED")


class _Loop:
    """An asyncio loop on a thread, for the proxies."""

    def __init__(self):
        self.loop = asyncio.new_event_loop()
        self.th = threading.Thread(target=self.loop.run_forever, daemon=True)
        self.th.start()

    def run(self, coro):
        return asyncio.run_coroutine_threadsafe(coro, self.loop).result(20)

    def stop(self):
        self.loop.call_soon_threadsafe(self.loop.stop)


LOOP = _Loop()


class TcpProxy:
    """A TCP proxy to `backend` (host, port) with `pause()`, `cut()`, `resume()`.
    `backends` may be a callable returning the next (host, port) (round robin)."""

    def __init__(self, backend, listen_port: int | None = None):
        self.backend = backend
        self.port = listen_port or port()
        self.paused = asyncio.Event()
        self.conns: set = set()
        self.refusing = False
        self.accepted = 0
        self.server = LOOP.run(self._start())
        LOOP.loop.call_soon_threadsafe(self.paused.set)

    async def _start(self):
        self.paused = asyncio.Event()
        self.paused.set()
        return await asyncio.start_server(self._handle, "127.0.0.1", self.port, backlog=4096)

    async def _handle(self, r, w):
        if self.refusing:
            w.close()
            return
        self.accepted += 1
        be = self.backend() if callable(self.backend) else self.backend
        if be is None:
            w.close()
            return
        try:
            br, bw = await asyncio.open_connection(*be)
        except OSError:
            w.close()
            return
        pair = (w, bw)
        self.conns.add(pair)

        async def pipe(src, dst):
            try:
                while True:
                    d = await src.read(65536)
                    if not d:
                        break
                    await self.paused.wait()
                    dst.write(d)
                    await dst.drain()
            except (OSError, asyncio.CancelledError):
                pass
            finally:
                try:
                    dst.close()
                except OSError:
                    pass

        await asyncio.gather(pipe(r, bw), pipe(br, w))
        self.conns.discard(pair)

    def pause(self):
        LOOP.loop.call_soon_threadsafe(self.paused.clear)

    def resume(self):
        LOOP.loop.call_soon_threadsafe(self.paused.set)

    def cut(self, refuse=True):
        def _cut():
            self.refusing = refuse
            for a, b in list(self.conns):
                for x in (a, b):
                    try:
                        x.close()
                    except OSError:
                        pass
        LOOP.loop.call_soon_threadsafe(_cut)

    def allow(self):
        LOOP.loop.call_soon_threadsafe(lambda: setattr(self, "refusing", False))

    def close(self):
        self.cut(refuse=True)
        LOOP.loop.call_soon_threadsafe(self.server.close)


class Fleet:
    """`n` replicas named r0.. on database `db`, optionally through a DB proxy,
    and a round-robin proxy over the live ones."""

    def __init__(self, db: str, n: int, env: dict | None = None, db_proxy: TcpProxy | None = None,
                 store: str = "pg"):
        self.db = db
        self.url = fresh_db(db)
        self.env = {"STORE": store, "EXEC_TIMEOUT_SECS": "120", **(env or {})}
        if db_proxy is not None:
            u = urllib.parse.urlsplit(self.url)
            self.env["DATABASE_URL"] = f"postgres://{u.username}:{u.password}@127.0.0.1:{db_proxy.port}/{db}"
        else:
            self.env["DATABASE_URL"] = self.url
        self.replicas: list[Target | None] = []
        self.failed_to_start: list[str] = []
        self._rr = 0
        for i in range(n):
            self.start(i)
        self.rr = TcpProxy(self._next)

    def start(self, i: int):
        try:
            t = Target(f"r{i}", dict(self.env))
        except RuntimeError as e:
            self.failed_to_start.append(f"r{i}: {str(e)[-200:]}")
            t = None
        if i < len(self.replicas):
            self.replicas[i] = t
        else:
            self.replicas.append(t)
        return t

    def _next(self):
        alive = [t for t in self.replicas if t is not None and t.alive()]
        if not alive:
            return None
        self._rr += 1
        return ("127.0.0.1", alive[self._rr % len(alive)].port)

    def kill9(self, i: int):
        self.replicas[i].proc.send_signal(signal.SIGKILL)

    def sigstop(self, i: int):
        self.replicas[i].proc.send_signal(signal.SIGSTOP)

    def sigcont(self, i: int):
        self.replicas[i].proc.send_signal(signal.SIGCONT)

    def logs(self) -> str:
        return "\n".join(t.log_text() for t in self.replicas if t is not None)

    def close(self):
        self.rr.close()
        for t in self.replicas:
            if t is not None:
                try:
                    t.proc.send_signal(signal.SIGCONT)
                except OSError:
                    pass
                t.stop(sig=2, wait=10)
        LOOP.run(asyncio.sleep(0.1))


def sql(db: str, q: str) -> str:
    return psql(q, db)


def invariants(db: str, ids: list[str] | None = None) -> dict:
    """The checks that must hold for every task, each by one query."""
    term = ",".join(f"'{s}'" for s in TERMINAL)
    out: dict = {}
    out["tasks"] = int(sql(db, "select count(*) from tasks"))
    out["states"] = {k: int(v) for k, v in (ln.split("|") for ln in sql(db, "select state, count(*) from tasks group by 1").splitlines())} if out["tasks"] else {}
    out["not_terminal"] = int(sql(db, f"select count(*) from tasks where state not in ({term})"))
    ev_state = "payload->'statusUpdate'->'status'->>'state'"
    out["tasks_with_2plus_terminal_events"] = int(sql(
        db, f"select count(*) from (select task_id from task_events where {ev_state} in ({term}) group by 1 having count(*) > 1) x"))
    out["tasks_with_no_terminal_event"] = int(sql(
        db, f"select count(*) from tasks t where t.state in ({term}) and not exists (select 1 from task_events e where e.task_id = t.id and {ev_state} in ({term}))"))
    out["events_after_terminal"] = int(sql(
        db, f"select count(*) from task_events e join (select task_id, min(seq) s from task_events where {ev_state} in ({term}) group by 1) t on t.task_id = e.task_id and e.seq > t.s"))
    out["state_differs_from_terminal_event"] = int(sql(
        db, f"select count(*) from tasks t join task_events e on e.task_id = t.id and {ev_state} in ({term}) where t.state <> {ev_state}"))
    out["orphan_claims_on_terminal_tasks"] = int(sql(
        db, f"select count(*) from a2a_task_claims c join tasks t on t.id = c.task_id where t.state in ({term})"))
    out["claims_total"] = int(sql(db, "select count(*) from a2a_task_claims"))
    out["seq_gaps"] = int(sql(
        db, "select count(*) from (select task_id, max(seq) - min(seq) + 1 - count(*) g from task_events group by 1) x where g <> 0"))
    if ids is not None:
        have = {x for x in sql(db, "select id from tasks").splitlines()}
        out["sent_but_missing"] = len([i for i in ids if i not in have])
    return out


def bad(inv: dict, allow_non_terminal: bool = False) -> list[str]:
    p = []
    if inv["not_terminal"] and not allow_non_terminal:
        p.append(f"{inv['not_terminal']} task(s) not terminal")
    for k in ("tasks_with_2plus_terminal_events", "tasks_with_no_terminal_event", "events_after_terminal",
              "state_differs_from_terminal_event", "orphan_claims_on_terminal_tasks", "seq_gaps", "sent_but_missing"):
        if inv.get(k):
            p.append(f"{k}={inv[k]}")
    return p


def double_starts(logs: str) -> dict:
    """Tasks the executor started more than once, from the `exec start` log lines."""
    seen: dict = {}
    for ln in logs.splitlines():
        if "exec start " in ln:
            seen.setdefault(ln.rsplit("exec start ", 1)[1].strip(), []).append(ln.split("]")[0].strip("["))
    return {k: v for k, v in seen.items() if len(v) > 1}
