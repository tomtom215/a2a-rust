# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
"""Shared helpers for the live-model campaign drivers (scripts/live/campaign/live/).

Needs: python3 (standard library only); the two binaries built from
scripts/live/campaign/live/agent/ (see its Cargo.toml for the command), found in
$CAMPAIGN_TARGET (the cargo target's `debug` directory); for the real-model
drivers a llama-server on 127.0.0.1:11434 (see run_notes in the README).
Proves nothing by itself: it starts agents, speaks the three bindings and
records what came back.
"""

from __future__ import annotations

import json
import os
import pathlib
import re
import signal
import subprocess
import sys
import threading
import time
import uuid
from http.client import HTTPConnection

HERE = pathlib.Path(__file__).resolve().parent
REPO = HERE.parents[3]
TARGET = pathlib.Path(os.environ.get("CAMPAIGN_TARGET", REPO / "target/live/campaign/debug"))
AGENT = TARGET / "a2a-campaign-agent"
CLIENT = TARGET / "a2a-campaign-client"
EVID = pathlib.Path(os.environ.get("CAMPAIGN_EVIDENCE", REPO / "docs/audits/2026-11-07-campaign-wave2/live/evidence"))
WORK = pathlib.Path(os.environ.get("CAMPAIGN_WORK", "/tmp/a2a-campaign-live"))
MODEL = os.environ.get("CAMPAIGN_MODEL_URL", "http://127.0.0.1:11434/v1")
STEERING_URI = "https://a2a-rust.com/extensions/steering/v1"
BUDGET_URI = "https://a2a-rust.com/extensions/budget/v1"
RESULTS: list[tuple[bool, str]] = []
LAST: dict = {"bytes": 0}  # the raw size of the last http() response body


def check(ok: bool, what: str) -> bool:
    RESULTS.append((ok, what))
    print(("  ok    " if ok else "  FAIL  ") + what, flush=True)
    return ok


def note(text: str) -> None:
    print(f"  note  {text}", flush=True)


def finish() -> int:
    bad = [w for ok, w in RESULTS if not ok]
    print(f"\n{len(RESULTS) - len(bad)} of {len(RESULTS)} expectations held", flush=True)
    return 1 if bad else 0


def load() -> str:
    try:
        return open("/proc/loadavg").read().split()[0]
    except OSError:
        return "?"


def save(name: str, data) -> pathlib.Path:
    """Writes `data` (text, bytes or JSON-able) under the evidence directory."""
    EVID.mkdir(parents=True, exist_ok=True)
    path = EVID / name
    if isinstance(data, bytes):
        path.write_bytes(data)
    elif isinstance(data, str):
        path.write_text(data)
    else:
        path.write_text(json.dumps(data, indent=2, sort_keys=True, default=str) + "\n")
    return path


class Agent:
    """One a2a-campaign-agent process. `env` is the C_* settings (see its main.rs)."""

    def __init__(self, name: str, **env):
        WORK.mkdir(parents=True, exist_ok=True)
        self.name = name
        self.out_path = WORK / f"{name}.stdout"
        self.err_path = WORK / f"{name}.stderr"
        self.tool_log = WORK / f"{name}.tools"
        self.tool_log.write_text("")
        settings = {**os.environ, "C_TOOL_LOG": str(self.tool_log), "NO_PROXY": "*", "no_proxy": "*",
                    **{k: str(v) for k, v in env.items()}}
        settings.pop("HTTPS_PROXY", None); settings.pop("HTTP_PROXY", None)
        settings.pop("https_proxy", None); settings.pop("http_proxy", None)
        self.out = open(self.out_path, "w")
        self.err = open(self.err_path, "w")
        self.proc = subprocess.Popen([str(AGENT)], env=settings, stdout=self.out, stderr=self.err)
        self.ports: dict[str, int] = {}
        deadline = time.time() + 30
        while time.time() < deadline:
            m = re.search(r"^READY(.*)$", self.out_path.read_text(), re.M)
            if m:
                self.ports = {k: int(v) for k, v in re.findall(r"(\w+)=(\d+)", m.group(1))}
                return
            if self.proc.poll() is not None:
                break
            time.sleep(0.1)
        raise RuntimeError(f"{name} did not start: {self.err_path.read_text()[-2000:]}")

    def rss_kb(self) -> int:
        try:
            for line in open(f"/proc/{self.proc.pid}/status"):
                if line.startswith("VmRSS:"):
                    return int(line.split()[1])
        except OSError:
            pass
        return -1

    def alive(self) -> bool:
        return self.proc.poll() is None

    def tools(self) -> list[str]:
        return [l for l in self.tool_log.read_text().splitlines() if l.strip()]

    def stdout(self) -> str:
        return self.out_path.read_text()

    def stderr(self) -> str:
        return self.err_path.read_text()

    def stop(self) -> None:
        if self.proc.poll() is None:
            self.proc.send_signal(signal.SIGINT)
            try:
                self.proc.wait(timeout=20)
            except subprocess.TimeoutExpired:
                self.proc.kill()
                self.proc.wait()
        self.out.close()
        self.err.close()

    def __enter__(self):
        return self

    def __exit__(self, *a):
        self.stop()


def http(port: int, method: str, path: str, body=None, headers=None, timeout: float = 600):
    """One request; returns (status, headers, parsed-or-raw body)."""
    conn = HTTPConnection("127.0.0.1", port, timeout=timeout)
    h = {"a2a-version": "1.0", **(headers or {})}
    data = None
    if body is not None:
        data = body if isinstance(body, bytes) else json.dumps(body).encode()
        h.setdefault("content-type", "application/json")
    conn.request(method, path, body=data, headers=h)
    r = conn.getresponse()
    raw = r.read()
    conn.close()
    LAST["bytes"] = len(raw)
    try:
        parsed = json.loads(raw)
    except ValueError:
        parsed = raw
    return r.status, dict(r.getheaders()), parsed


def rpc(port: int, method: str, params: dict, token: str | None = None, timeout: float = 600):
    h = {"authorization": f"Bearer {token}"} if token else {}
    status, headers, body = http(port, "POST", "/", {"jsonrpc": "2.0", "id": 1, "method": method, "params": params}, h, timeout)
    return status, body


def rest(port: int, method: str, path: str, body=None, token: str | None = None, timeout: float = 600):
    h = {"authorization": f"Bearer {token}"} if token else {}
    status, headers, parsed = http(port, method, path, body, h, timeout)
    return status, parsed


def message(text: str, task: str | None = None, steering: bool = False, budget_tokens: int | None = None,
            budget_ms: int | None = None) -> dict:
    m: dict = {"messageId": uuid.uuid4().hex, "role": "ROLE_USER", "parts": [{"text": text}]}
    if task:
        m["taskId"] = task
    ext = []
    if steering:
        ext.append(STEERING_URI)
    if budget_tokens is not None or budget_ms is not None:
        ext.append(BUDGET_URI)
        b: dict = {}
        if budget_tokens is not None:
            b["meters"] = {"tokens": budget_tokens}
        if budget_ms is not None:
            b["remainingMs"] = budget_ms
        m["metadata"] = {"a2a-rust.com/budget": b}
    if ext:
        m["extensions"] = ext
    return m


def client(binding: str, port: int, *args: str, timeout: float = 900) -> list[dict]:
    """Runs a2a-campaign-client; returns every JSON line it printed."""
    env = {**os.environ, "NO_PROXY": "*", "no_proxy": "*"}
    for k in ("HTTPS_PROXY", "HTTP_PROXY", "https_proxy", "http_proxy"):
        env.pop(k, None)
    p = subprocess.run([str(CLIENT), binding, f"http://127.0.0.1:{port}", *args], env=env,
                       capture_output=True, text=True, timeout=timeout)
    lines = []
    for l in p.stdout.splitlines():
        try:
            lines.append(json.loads(l))
        except ValueError:
            lines.append({"raw": l})
    if p.stderr.strip():
        lines.append({"stderr": p.stderr[-1500:]})
    return lines


def task_of(result) -> dict:
    """The task inside a JSON-RPC `result` (v1.0 wraps it as {"task": ...} or not)."""
    if isinstance(result, dict):
        return result.get("task", result)
    return {}


def artifact_text(task: dict, artifact_id: str) -> str | None:
    for a in task.get("artifacts") or []:
        if a.get("artifactId") == artifact_id or a.get("id") == artifact_id:
            return "".join(p.get("text", "") for p in a.get("parts", []))
    return None


def artifact(task: dict, artifact_id: str) -> dict | None:
    for a in task.get("artifacts") or []:
        if a.get("artifactId") == artifact_id or a.get("id") == artifact_id:
            return a
    return None


def status_text(task: dict) -> str:
    m = (task.get("status") or {}).get("message") or {}
    return "".join(p.get("text", "") for p in m.get("parts", []))


def wait_for(pred, secs: float = 60, step: float = 0.2):
    deadline = time.time() + secs
    while time.time() < deadline:
        v = pred()
        if v:
            return v
        time.sleep(step)
    return pred()


def run_header(title: str, model_file: str | None = None) -> dict:
    """The state every recorded run names: model, server, load."""
    info = {"title": title, "load_at_start": load(), "time": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())}
    if model_file:
        info["model_file"] = model_file
    print(f"== {title} (load {info['load_at_start']})", flush=True)
    return info


def stream_task_id(ev: dict) -> str | None:
    if "result" in ev and isinstance(ev["result"], dict):
        ev = ev["result"]
    if isinstance(ev.get("task"), dict):
        return ev["task"].get("id")
    for k in ("statusUpdate", "artifactUpdate"):
        if isinstance(ev.get(k), dict):
            return ev[k].get("taskId")
    return None


class Stream:
    """A streaming send (SendStreamingMessage over JSON-RPC, POST /message:stream over HTTP+JSON),
    read on a thread: `.events` fills as they arrive, `.task_id` is set at the first, `.done`
    is set when the stream ends, `.error` holds a transport error or a non-200 reply."""

    def __init__(self, binding: str, port: int, text: str, token: str | None = None, budget_tokens: int | None = None,
                 budget_ms: int | None = None):
        self.events: list[dict] = []
        self.task_id: str | None = None
        self.done = threading.Event()
        self.error = None
        self.t0 = time.time()
        self.times: list[float] = []
        msg = message(text, budget_tokens=budget_tokens, budget_ms=budget_ms)
        if binding == "jsonrpc":
            path, body = "/", {"jsonrpc": "2.0", "id": 1, "method": "SendStreamingMessage", "params": {"message": msg}}
        else:
            path, body = "/message:stream", {"message": msg}
        headers = {"a2a-version": "1.0", "content-type": "application/json", "accept": "text/event-stream"}
        if token:
            headers["authorization"] = f"Bearer {token}"
        self.thread = threading.Thread(target=self._run, args=(port, path, body, headers), daemon=True)
        self.thread.start()

    def _run(self, port, path, body, headers):
        try:
            conn = HTTPConnection("127.0.0.1", port, timeout=1800)
            conn.request("POST", path, body=json.dumps(body).encode(), headers=headers)
            r = conn.getresponse()
            if r.status != 200:
                self.error = (r.status, r.read()[:2000])
                return
            buf = b""
            while True:
                line = r.readline()
                if not line:
                    break
                buf += line
                if line in (b"\n", b"\r\n"):
                    for l in buf.splitlines():
                        if l.startswith(b"data:"):
                            try:
                                ev = json.loads(l[5:].strip())
                            except ValueError:
                                continue
                            self.events.append(ev)
                            self.times.append(time.time() - self.t0)
                            if self.task_id is None:
                                self.task_id = stream_task_id(ev)
                    buf = b""
        except Exception as e:  # noqa: BLE001
            self.error = repr(e)
        finally:
            self.done.set()

    def wait_task(self, secs: float = 60) -> str | None:
        wait_for(lambda: self.task_id or self.done.is_set(), secs)
        return self.task_id
