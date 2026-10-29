# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
"""What the live scripts share: the database, a replica process, JSON-RPC.

Settings, all from the environment (see README.md):

    A2A_LIVE_PG          base URL of the database server
                         (default postgres://a2a:a2a@127.0.0.1:5432)
    A2A_LIVE_CONTAINER   the Docker container PostgreSQL runs in, for psql,
                         pg_isready and restarts (default pg)
    A2A_LIVE_OUT         where logs and recordings go (default target/live)
"""

from __future__ import annotations

import json
import os
import pathlib
import subprocess
import time
import urllib.request
import uuid

ROOT = pathlib.Path(__file__).resolve().parents[2]
PG = os.environ.get("A2A_LIVE_PG", "postgres://a2a:a2a@127.0.0.1:5432")
CONTAINER = os.environ.get("A2A_LIVE_CONTAINER", "pg")
OUT = pathlib.Path(os.environ.get("A2A_LIVE_OUT", ROOT / "target" / "live"))
OUT.mkdir(parents=True, exist_ok=True)
_USER = PG.split("://", 1)[1].split(":", 1)[0]


def psql(sql: str, db: str) -> str:
    """Runs `sql` in database `db` inside the container; returns its output."""
    cmd = ["docker", "exec", "-i", CONTAINER, "psql", "-U", _USER, "-tAq", "-d", db, "-c", sql]
    return subprocess.run(cmd, capture_output=True, text=True).stdout.strip()


def fresh_db(db: str) -> str:
    """Drops and creates database `db`; returns its URL."""
    psql(f"DROP DATABASE IF EXISTS {db} WITH (FORCE)", _USER)
    psql(f"CREATE DATABASE {db}", _USER)
    return f"{PG}/{db}"


def wait_ready() -> None:
    """Waits for the server in the container to accept connections."""
    for _ in range(200):
        if subprocess.run(["docker", "exec", CONTAINER, "pg_isready", "-U", _USER],
                          capture_output=True).returncode == 0:
            return
        time.sleep(0.1)


def terminate_all(db: str) -> str:
    """Ends every backend connected to `db`, as `pg_terminate_backend` does."""
    return psql(f"SELECT count(pg_terminate_backend(pid)) FROM pg_stat_activity "
                f"WHERE datname = '{db}' AND pid <> pg_backend_pid()", _USER)


def rpc(port: int, method: str, params: dict, timeout: float = 40) -> dict:
    body = json.dumps({"jsonrpc": "2.0", "id": 1, "method": method, "params": params}).encode()
    req = urllib.request.Request(f"http://127.0.0.1:{port}/", body,
                                 {"content-type": "application/json", "A2A-Version": "1.0"})
    with urllib.request.urlopen(req, timeout=timeout) as r:
        return json.load(r)


def start(binary: str, name: str, port: int, url: str, log: pathlib.Path | None = None,
          rust_log: str | None = None) -> subprocess.Popen:
    """Starts a replica (scripts/live/replica) and waits until it answers."""
    env = {"PATH": os.environ["PATH"], "PORT": str(port), "NAME": name, "DATABASE_URL": url}
    if rust_log:
        env["RUST_LOG"] = rust_log
    sink = open(log, "w") if log else subprocess.DEVNULL
    p = subprocess.Popen([binary], env=env, stdout=sink, stderr=subprocess.STDOUT)
    for _ in range(200):
        try:
            rpc(port, "GetTask", {"id": "none"})
            break
        except Exception:
            time.sleep(0.05)
    return p


def message(text: str, task: dict | None = None) -> dict:
    m = {"messageId": str(uuid.uuid4()), "role": "ROLE_USER", "parts": [{"text": text}]}
    if task:
        m["taskId"], m["contextId"] = task["id"], task["contextId"]
    return m


def send(port: int, text: str, task: dict | None = None, now: bool = False):
    """Sends `text` (continuing `task`); `now` returns without waiting.
    Returns (task, error)."""
    r = rpc(port, "SendMessage", {"message": message(text, task),
                                  "configuration": {"returnImmediately": now}})
    return r.get("result", {}).get("task"), r.get("error")


def stored_ticks(db: str, task_id: str):
    """The stored state, the numbers of the `tick` artifact's parts, and the
    event log's positions."""
    state, data = psql(f"SELECT state, data FROM tasks WHERE id = '{task_id}'", db).split("|", 1)
    parts = [int(p["text"]) for a in json.loads(data).get("artifacts") or []
             if a["artifactId"] == "tick" for p in a["parts"]]
    seqs = [int(x) for x in psql(f"SELECT seq FROM task_events WHERE task_id = '{task_id}' "
                                 "ORDER BY seq", db).split()]
    return state, parts, seqs


def missing(found: list[int], first: int) -> list[int]:
    return sorted(set(range(first, max(found) + 1)) - set(found)) if found else []
