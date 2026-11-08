# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
"""What every hostile-campaign script shares (wave 2, area HOSTILE).

Needs: the target binary built (`scripts/live/campaign/hostile/target`, see its
Cargo.toml), Python 3 standard library only, a local PostgreSQL for the `pg`
stores (`A2A_LIVE_PG`, default postgres://postgres:postgres@127.0.0.1:5432).
Aims at listeners it starts on 127.0.0.1 only.

Settings from the environment:
    A2A_HOSTILE_BIN   the target binary (default $A2A_HOSTILE_TARGET_DIR/debug/a2a-hostile-target)
    A2A_HOSTILE_EV    where evidence is written (default: docs/audits/.../hostile/evidence)
    A2A_LIVE_PG       base URL of the database server
"""

from __future__ import annotations

import http.client as _hc
import json
import os
import pathlib
import re
import socket
import subprocess
import time
import urllib.parse
import uuid

ROOT = pathlib.Path(__file__).resolve().parents[4]
EV = pathlib.Path(os.environ.get(
    "A2A_HOSTILE_EV", ROOT / "docs/audits/2026-11-07-campaign-wave2/hostile/evidence"))
EV.mkdir(parents=True, exist_ok=True)
BIN = os.environ.get("A2A_HOSTILE_BIN") or os.path.join(
    os.environ.get("A2A_HOSTILE_TARGET_DIR", "target/hostile"), "debug/a2a-hostile-target")
PG = os.environ.get("A2A_LIVE_PG", "postgres://postgres:postgres@127.0.0.1:5432")
_U = urllib.parse.urlsplit(PG)
_PGENV = {**os.environ, "PGPASSWORD": _U.password or "", "PGCONNECT_TIMEOUT": "5"}

_next_port = [int(os.environ.get("A2A_HOSTILE_PORT_BASE", "31000"))]


def port() -> int:
    """A fresh loopback port number (the campaign uses 31000 upward)."""
    _next_port[0] += 1
    return _next_port[0]


# ── PostgreSQL ────────────────────────────────────────────────────────────────

def psql(sql: str, db: str = "postgres") -> str:
    cmd = ["psql", "-h", _U.hostname or "127.0.0.1", "-p", str(_U.port or 5432),
           "-U", _U.username or "postgres", "-d", db, "-tAq", "-c", sql]
    r = subprocess.run(cmd, capture_output=True, text=True, env=_PGENV)
    if r.returncode != 0:
        raise RuntimeError(f"psql: {r.stderr.strip()}")
    return r.stdout.strip()


def fresh_db(name: str) -> str:
    assert name.startswith("a2a_camp_"), name
    psql(f"DROP DATABASE IF EXISTS {name} WITH (FORCE)")
    psql(f"CREATE DATABASE {name}")
    return f"{PG}/{name}"


def drop_db(name: str) -> None:
    assert name.startswith("a2a_camp_"), name
    psql(f"DROP DATABASE IF EXISTS {name} WITH (FORCE)")


# ── the target process ───────────────────────────────────────────────────────

class Target:
    """The target binary with `env` added to a clean environment."""

    def __init__(self, label: str, env: dict[str, str], http_port: int | None = None,
                 grpc: bool = False):
        self.label = label
        self.port = http_port or port()
        self.grpc_port = port() if grpc else None
        e = {"PATH": os.environ["PATH"], "PORT": str(self.port), "NAME": label,
             "RUST_LOG": "warn"}
        if self.grpc_port:
            e["GRPC_PORT"] = str(self.grpc_port)
        e.update(env)
        self.log_path = EV / f"{label}.target.log"
        self.log = open(self.log_path, "w")
        self.proc = subprocess.Popen([BIN], env=e, stdout=self.log, stderr=subprocess.STDOUT)
        self.up = False
        for _ in range(400):
            if self.proc.poll() is not None:
                break
            try:
                with socket.create_connection(("127.0.0.1", self.port), timeout=0.2):
                    self.up = True
                    break
            except OSError:
                time.sleep(0.05)
        if not self.up:
            raise RuntimeError(f"{label} did not start: {self.log_path.read_text()[-2000:]}")

    @property
    def pid(self) -> int:
        return self.proc.pid

    def rss_kb(self) -> int:
        try:
            for ln in open(f"/proc/{self.pid}/status"):
                if ln.startswith("VmRSS:"):
                    return int(ln.split()[1])
        except OSError:
            pass
        return -1

    def fds(self) -> int:
        try:
            return len(os.listdir(f"/proc/{self.pid}/fd"))
        except OSError:
            return -1

    def threads(self) -> int:
        try:
            for ln in open(f"/proc/{self.pid}/status"):
                if ln.startswith("Threads:"):
                    return int(ln.split()[1])
        except OSError:
            pass
        return -1

    def alive(self) -> bool:
        return self.proc.poll() is None

    def log_text(self) -> str:
        self.log.flush()
        return self.log_path.read_text(errors="replace")

    def panics(self) -> list[str]:
        return [ln for ln in self.log_text().splitlines() if "panicked" in ln]

    def stop(self, sig: int = 2, wait: float = 15) -> int | None:
        if self.proc.poll() is None:
            self.proc.send_signal(sig)
            try:
                self.proc.wait(wait)
            except subprocess.TimeoutExpired:
                self.proc.kill()
                self.proc.wait()
        self.log.close()
        return self.proc.returncode

    def state(self) -> dict:
        return {"alive": self.alive(), "rss_kb": self.rss_kb(), "fds": self.fds(),
                "threads": self.threads(), "panics": len(self.panics())}


# ── HTTP ─────────────────────────────────────────────────────────────────────

BASE_HEADERS = {"content-type": "application/json", "a2a-version": "1.0"}


def http(port_: int, method: str, path: str, body: bytes | str | None = None,
         headers: dict | None = None, timeout: float = 20, raw_headers=None):
    """One request; returns (status, headers dict, body bytes). `raw_headers` is a
    list of (name, value) pairs sent as given (to repeat a header)."""
    h = _hc.HTTPConnection("127.0.0.1", port_, timeout=timeout)
    try:
        h.putrequest(method, path, skip_host=False, skip_accept_encoding=True)
        hs = list(raw_headers) if raw_headers is not None else list((headers or BASE_HEADERS).items())
        if isinstance(body, str):
            body = body.encode()
        if body is not None and not any(k.lower() == "content-length" for k, _ in hs):
            hs.append(("content-length", str(len(body))))
        for k, v in hs:
            h.putheader(k, v)
        try:
            h.endheaders(body if body is not None else None)
        except (BrokenPipeError, ConnectionResetError):
            # The server answered and closed while the body was still going up
            # (a limit hit): read what it said.
            pass
        r = h.getresponse()
        data = r.read()
        return r.status, {k.lower(): v for k, v in r.getheaders()}, data
    finally:
        h.close()


def rpc(port_: int, method: str, params, token: str | None = None, extra: dict | None = None,
        path: str = "/", timeout: float = 20, id_=1):
    hs = dict(BASE_HEADERS)
    if token:
        hs["authorization"] = f"Bearer {token}"
    if extra:
        hs.update(extra)
    body = json.dumps({"jsonrpc": "2.0", "id": id_, "method": method, "params": params})
    st, hd, data = http(port_, "POST", path, body, hs, timeout)
    try:
        return st, json.loads(data)
    except ValueError:
        return st, data


def rest(port_: int, method: str, path: str, body=None, token: str | None = None,
         extra: dict | None = None, timeout: float = 20):
    hs = dict(BASE_HEADERS)
    if token:
        hs["authorization"] = f"Bearer {token}"
    if extra:
        hs.update(extra)
    b = None if body is None else (body if isinstance(body, (str, bytes)) else json.dumps(body))
    st, hd, data = http(port_, method, path, b, hs, timeout)
    try:
        return st, json.loads(data) if data else None
    except ValueError:
        return st, data


def msg(text: str, task: dict | None = None, ctx: str | None = None, mid: str | None = None,
        extensions: list[str] | None = None) -> dict:
    m = {"messageId": mid or str(uuid.uuid4()), "role": "ROLE_USER", "parts": [{"text": text}]}
    if task:
        m["taskId"], m["contextId"] = task["id"], task["contextId"]
    if ctx:
        m["contextId"] = ctx
    if extensions:
        m["extensions"] = extensions
    return m


def send(port_: int, text: str, token: str | None = None, now: bool = False,
         extra: dict | None = None, **kw):
    """SendMessage; returns (http status, result-or-error dict)."""
    st, r = rpc(port_, "SendMessage", {"message": msg(text, **kw),
                "configuration": {"returnImmediately": now}}, token=token, extra=extra)
    return st, r


def task_of(r) -> dict | None:
    if isinstance(r, dict):
        return (r.get("result") or {}).get("task")
    return None


# ── evidence ─────────────────────────────────────────────────────────────────

class Recorder:
    """Appends one JSON line per attack to EV/<name>.jsonl and keeps the tally."""

    def __init__(self, name: str):
        self.path = EV / f"{name}.jsonl"
        self.f = open(self.path, "w")
        self.tally = {"tried": 0, "held": 0, "deviation": 0}

    def rec(self, attack: str, verdict: str, detail) -> None:
        """verdict: 'held' (the SDK refused as documented) or 'DEVIATION'."""
        self.tally["tried"] += 1
        self.tally["held" if verdict == "held" else "deviation"] += 1
        line = {"attack": attack, "verdict": verdict, "detail": detail}
        self.f.write(json.dumps(line, default=str) + "\n")
        self.f.flush()
        flag = "  " if verdict == "held" else "!!"
        print(f"{flag} {verdict:9} {attack}: {str(detail)[:160]}")

    def close(self) -> None:
        self.f.write(json.dumps({"tally": self.tally}) + "\n")
        self.f.close()
        print("tally", self.tally)


LEAK = re.compile(r"((?i:sqlx|postgres|sqlite|pg_catalog|sqlstate|date/time field|"
                  r"invalid byte sequence|invalid input syntax|syntax error at|"
                  r"/home/|/proc/|panicked|called `Result|RUST_BACKTRACE)|ERROR:  |\.rs:\d+|thread ')")


def leaks(text) -> list[str]:
    """Words in a response that name the database, the OS or the source."""
    s = text if isinstance(text, str) else json.dumps(text, default=str)
    return sorted({m.group(0).lower() for m in LEAK.finditer(s)})
