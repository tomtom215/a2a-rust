# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
"""A local PostgreSQL for delegation_live.py: psql, a fresh database, counts.

Unlike scripts/live/common.py this needs no Docker: it talks to the cluster
Debian installs (`pg_ctlcluster 16 main start`). Settings, from the environment:

    A2A_LIVE_PG   base URL (default postgres://postgres:postgres@127.0.0.1:5432)

The `postgres` role's password may need setting once:
    su postgres -c "psql -c \\"ALTER USER postgres PASSWORD 'postgres'\\""
"""

from __future__ import annotations

import os
import subprocess
import time
import urllib.parse

PG = os.environ.get("A2A_LIVE_PG", "postgres://postgres:postgres@127.0.0.1:5432")
_U = urllib.parse.urlsplit(PG)
USER, PASSWORD = _U.username or "postgres", _U.password or ""
HOST, PORT = _U.hostname or "127.0.0.1", str(_U.port or 5432)
_ENV = {**os.environ, "PGPASSWORD": PASSWORD, "PGCONNECT_TIMEOUT": "5"}


def psql(sql: str, db: str = "postgres") -> str:
    """Runs `sql` in `db`; returns its output, rows split by '|'. Raises on error."""
    cmd = ["psql", "-h", HOST, "-p", PORT, "-U", USER, "-d", db, "-tAq", "-c", sql]
    r = subprocess.run(cmd, capture_output=True, text=True, env=_ENV)
    if r.returncode != 0:
        raise RuntimeError(f"psql failed: {r.stderr.strip()}")
    return r.stdout.strip()


def ensure_running() -> str:
    """Starts the local cluster if it is down; returns `psql --version`'s server version."""
    ready = ["pg_isready", "-h", HOST, "-p", PORT, "-U", USER]
    if subprocess.run(ready, capture_output=True).returncode != 0:
        subprocess.run(["pg_ctlcluster", "16", "main", "start"], capture_output=True)
        for _ in range(100):
            if subprocess.run(ready, capture_output=True).returncode == 0:
                break
            time.sleep(0.2)
    return psql("SHOW server_version")


def fresh_db(db: str) -> str:
    """Drops and creates database `db`; returns its URL."""
    psql(f"DROP DATABASE IF EXISTS {db} WITH (FORCE)")
    psql(f"CREATE DATABASE {db}")
    return f"{PG}/{db}"


def drop_db(db: str) -> None:
    psql(f"DROP DATABASE IF EXISTS {db} WITH (FORCE)")


def _counts(sql: str, db: str) -> dict[str, int]:
    out = {}
    for line in psql(sql, db).splitlines():
        key, n = line.rsplit("|", 1)
        out[key] = int(n)
    return out


def snapshot(db: str) -> dict:
    """What the journal and the worker's task table say now.

    journal: records by outcome ('<open>' while unsettled); tasks: by state;
    pairs: 'outcome/state' for each record joined to its task."""
    return {
        "journal": _counts("SELECT coalesce(outcome, '<open>'), count(*) FROM a2a_delegations "
                           "GROUP BY 1 ORDER BY 1", db),
        "no_task_id": int(psql("SELECT count(*) FROM a2a_delegations WHERE task_id IS NULL", db)),
        "tasks": _counts("SELECT state, count(*) FROM tasks GROUP BY 1 ORDER BY 1", db),
        "pairs": _counts("SELECT coalesce(d.outcome, '<open>') || '/' || t.state, count(*) "
                         "FROM a2a_delegations d JOIN tasks t ON t.id = d.task_id GROUP BY 1 ORDER BY 1", db),
        "connections": int(psql(f"SELECT count(*) FROM pg_stat_activity WHERE datname = '{db}' "
                          "AND pid <> pg_backend_pid()", db)),
    }
