#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
"""A real model provider that is down, dies mid-generation, and comes back (qualification matrix, 2026-11-14).

Needs: llama.cpp's `llama-server` (LLAMA_SERVER, default /opt/llama/src/build/bin/llama-server) and a GGUF model
(CAMPAIGN_MODEL_PATH, default /opt/models/Qwen3-1.7B-Q4_K_M.gguf), the binaries of scripts/live/campaign/live/agent/,
python3. Nothing else may listen on 127.0.0.1:11434: this script starts and kills its own llama-server there.

  down      the provider is not running when a task is sent: the task fails at once, failure class `transient`
  killed    llama-server receives SIGKILL while it generates the first turn's reply: the task fails, class
            `transient`, within 15 s of the kill, and the agent process stays up
  back      llama-server is started again: a new task on the same agent completes

Usage: python3 -I 13_provider_death.py        Evidence: $CAMPAIGN_EVIDENCE/13-provider-death.json
"""
import os
import signal
import subprocess
import sys
import time
import urllib.request

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from common import *  # noqa: F401,F403

LLAMA = os.environ.get("LLAMA_SERVER", "/opt/llama/src/build/bin/llama-server")
MODEL_PATH = os.environ.get("CAMPAIGN_MODEL_PATH", "/opt/models/Qwen3-1.7B-Q4_K_M.gguf")
PROMPT = "How is the billing service doing?"


def llama_up() -> bool:
    try:
        with urllib.request.urlopen("http://127.0.0.1:11434/health", timeout=2) as r:
            return r.status == 200
    except OSError:
        return False


def start_llama() -> subprocess.Popen:
    if llama_up():
        raise RuntimeError("something already serves 127.0.0.1:11434; stop it first")
    log = open(WORK / "13-llama-server.log", "a")
    proc = subprocess.Popen([LLAMA, "-m", MODEL_PATH, "--host", "127.0.0.1", "--port", "11434", "--alias", "qwen3:1.7b",
                             "--jinja", "-c", "4096", "-np", "1", "-t", "4"], stdout=log, stderr=log)
    if not wait_for(llama_up, secs=180, step=0.5):
        proc.kill()
        raise RuntimeError("llama-server did not become ready in 180 s")
    return proc


def task_state(port: int, task_id: str) -> dict:
    _, body = rpc(port, "GetTask", {"id": task_id}, timeout=30)
    return task_of(body.get("result", {}))


def failure(task: dict) -> str | None:
    return ((task.get("status") or {}).get("message") or {}).get("metadata", {}).get("a2a-rust.com/failure")


def wait_terminal(port: int, task_id: str, secs: float) -> tuple[dict, float]:
    t0 = time.time()
    terminal = ("TASK_STATE_COMPLETED", "TASK_STATE_FAILED", "TASK_STATE_CANCELED", "TASK_STATE_REJECTED")
    while time.time() - t0 < secs:
        task = task_state(port, task_id)
        if (task.get("status") or {}).get("state") in terminal:
            return task, time.time() - t0
        time.sleep(0.2)
    return task_state(port, task_id), time.time() - t0


def main() -> int:
    header = run_header("a real provider down, killed mid-generation, and back", os.path.basename(MODEL_PATH))
    rows = {}
    WORK.mkdir(parents=True, exist_ok=True)
    with Agent("provider-death", C_MODEL_URL=MODEL, C_MODEL="qwen3:1.7b", C_TOOLS="inventory", C_PORT_RPC=0,
               C_PORT_REST=-1, C_PORT_GRPC=-1, C_EXTRA_BODY='{"max_tokens":400}', C_MAX_TURNS=4) as agent:
        port = agent.ports["rpc"]

        # down
        t0 = time.time()
        _, body = rpc(port, "SendMessage", {"message": message(PROMPT)}, timeout=120)
        task = task_of(body.get("result", {}))
        rows["down"] = {"state": (task.get("status") or {}).get("state"), "failure": failure(task),
                        "status_text": status_text(task), "seconds": round(time.time() - t0, 2)}
        check(rows["down"]["state"] == "TASK_STATE_FAILED" and rows["down"]["failure"] == "transient",
              f"provider down at send: {rows['down']}")

        # killed
        llama = start_llama()
        _, body = rpc(port, "SendMessage", {"message": message(PROMPT), "configuration": {"returnImmediately": True}},
                      timeout=60)
        task_id = task_of(body.get("result", {})).get("id")
        # The first turn's request reaches llama-server at once; on 4 CPU threads its reply takes seconds, so
        # 1.5 s in, it is generating.
        time.sleep(1.5)
        state_at_kill = (task_state(port, task_id).get("status") or {}).get("state")
        llama.send_signal(signal.SIGKILL)
        llama.wait()
        killed_at = time.time()
        task, waited = wait_terminal(port, task_id, 60)
        rows["killed"] = {"state_at_kill": state_at_kill, "state": (task.get("status") or {}).get("state"),
                          "failure": failure(task), "status_text": status_text(task),
                          "seconds_after_kill": round(time.time() - killed_at, 2), "agent_alive": agent.alive()}
        check(state_at_kill == "TASK_STATE_WORKING" and rows["killed"]["state"] == "TASK_STATE_FAILED"
              and rows["killed"]["failure"] == "transient" and rows["killed"]["seconds_after_kill"] <= 15
              and agent.alive(), f"llama-server killed mid-generation: {rows['killed']}")

        # back
        llama = start_llama()
        try:
            t0 = time.time()
            _, body = rpc(port, "SendMessage", {"message": message(PROMPT)}, timeout=600)
            task = task_of(body.get("result", {}))
            rows["back"] = {"state": (task.get("status") or {}).get("state"), "seconds": round(time.time() - t0, 1),
                            "has_answer": artifact(task, "answer") is not None, "status_text": status_text(task)}
            check(rows["back"]["state"] == "TASK_STATE_COMPLETED", f"provider back, a new task: {rows['back']}")
        finally:
            llama.send_signal(signal.SIGTERM)
            llama.wait(timeout=30)
    save("13-provider-death.json", {"header": header, "rows": rows})
    return finish()


if __name__ == "__main__":
    sys.exit(main())
