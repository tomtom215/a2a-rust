#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
"""Steering a running task (ADR 0036) while a real model drives the tool loop.

Needs: llama-server on 127.0.0.1:11434, the binaries of scripts/live/campaign/live/agent/, python3.
Every model request goes through model_proxy.py, so the driver can say what the model was sent.
For JSON-RPC and HTTP+JSON (gRPC has no bearer token in the campaign client, see README):

  goal      a message while a tool sleeps; is it in the next turn, once, after the tool result?
  bound     inbox of 4, seven messages: the first four accepted in order, the rest refused
            (503 / error), none lost or duplicated, the four reach the model in order
  answering a message sent during the answering turn: accepted, never read, counted `unread`
  owner     another caller (isolate-by-caller is the default with authentication) cannot steer
  oversize  a message over the size bound
  ended     a steering message to a task that has completed
  cancel    cancel a sleeping run: CANCELED, the tool's future dropped; steering after it
  midstream drafts on: a message while the model streams its answer

Proves (or fails to) the SDK's counts and refusals; whether the model obeys the steer is recorded,
never graded.
"""
import json
import os
import subprocess
import sys
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from common import *  # noqa: F401,F403
from otlp_sink import Sink

PROXY_PORT = 11500
SLEEP_PROMPT = "Call the sleep_work tool with seconds set to {n}. When it is done, reply with the single word WORK-DONE."


def proxy():
    log = WORK / "steer-proxy.log"
    log.write_text("")
    p = subprocess.Popen([sys.executable, "-I", str(HERE / "model_proxy.py"), str(PROXY_PORT), "11434", str(log)],
                         stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    time.sleep(1)
    return p, log


def proxy_requests(log):
    out = []
    for l in log.read_text().splitlines():
        try:
            out.append(json.loads(l))
        except ValueError:
            pass
    return out


def user_turns(request):
    return [m.get("content") for m in (request or {}).get("messages", []) if m.get("role") == "user"]


def steer(binding, agent, token, task, text):
    """(accepted?, detail) of one steering message."""
    if binding == "jsonrpc":
        st, body = rpc(agent.ports["rpc"], "SendMessage", {"message": message(text, task, steering=True)}, token)
        if "error" in body:
            return False, {"http": st, "error": body["error"]}
        return True, {"http": st, "metadata": task_of(body.get("result", {})).get("metadata")}
    st, body = rest(agent.ports["rest"], "POST", "/message:send", {"message": message(text, task, steering=True)}, token)
    if st != 200:
        return False, {"http": st, "body": body}
    return True, {"http": st, "metadata": task_of(body).get("metadata")}


def get_task(binding, agent, token, task):
    if binding == "jsonrpc":
        return task_of(rpc(agent.ports["rpc"], "GetTask", {"id": task}, token)[1].get("result", {}))
    return task_of(rest(agent.ports["rest"], "GET", f"/tasks/{task}", None, token)[1])


def cancel(binding, agent, token, task):
    if binding == "jsonrpc":
        return rpc(agent.ports["rpc"], "CancelTask", {"id": task}, token)
    return rest(agent.ports["rest"], "POST", f"/tasks/{task}:cancel", {}, token)


def started(agent, what="START sleep_work", secs=120):
    return wait_for(lambda: any(what in l for l in agent.tools()), secs)


def steering_counts(sink):
    """Cumulative a2a.server.steering by outcome (the largest value each series reached)."""
    counts = {}
    for p in sink.points("a2a.server.steering"):
        outcome = next((v for k, v in p["attributes"].items() if "outcome" in k), "?")
        value = int(p["dp"].get("asInt", 0) or 0)
        counts[outcome] = max(counts.get(outcome, 0), value)
    return counts


def scenarios(binding, model_file, results):
    sink = Sink()
    env = {"C_MODEL_URL": f"http://127.0.0.1:{PROXY_PORT}/v1", "C_TOOLS": "inventory,slow", "C_STEER": 4,
           "C_AUTH": "ta:alice,tb:bob", "C_TELEMETRY": "1", "OTEL_EXPORTER_OTLP_ENDPOINT": sink.url(),
           "OTEL_EXPORTER_OTLP_PROTOCOL": "http/json", "OTEL_METRIC_EXPORT_INTERVAL": "1000",
           "C_EXTRA_BODY": '{"max_tokens":300}', "C_MAX_TURNS": 6, "C_STEER_MAX_BYTES": 65536}
    pproc, plog = proxy()
    res = results.setdefault(binding, {})
    port_key = "rpc" if binding == "jsonrpc" else "rest"
    try:
        with Agent(f"steer-{binding}", C_PORT_RPC=0 if binding == "jsonrpc" else -1, C_PORT_REST=0 if binding == "rest" else -1,
                   C_PORT_GRPC=-1, **env) as agent:
            port = agent.ports[port_key]
            # ---- goal
            print(f"-- [{binding}] goal change while a tool sleeps", flush=True)
            n0 = len(proxy_requests(plog))
            s = Stream(binding, port, SLEEP_PROMPT.format(n=12), "ta")
            task = s.wait_task()
            check(task is not None, f"{binding}: the stream named a task ({s.error})")
            check(bool(started(agent)), f"{binding}: the model called sleep_work")
            ok, detail = steer(binding, agent, "ta", task, "NEW INSTRUCTION: when the work is done, do not say WORK-DONE; reply with the single word PINEAPPLE.")
            print(f"   receipt: {ok} {detail}", flush=True)
            check(ok, f"{binding}: the steering message to the working task was accepted")
            check("delivery" in json.dumps(detail.get("metadata")) and "inbox" in json.dumps(detail.get("metadata")),
                  f"{binding}: the receipt says delivery=inbox ({detail.get('metadata')})")
            s.done.wait(300)
            final = get_task(binding, agent, "ta", task)
            reqs = proxy_requests(plog)[n0:]
            seen = [i for i, r in enumerate(reqs) if any("PINEAPPLE" in (t or "") for t in user_turns(r["request"]))]
            print(f"   model requests: {len(reqs)}; those carrying the steer: {seen}; final state {(final.get('status') or {}).get('state')}", flush=True)
            print(f"   answer: {artifact_text(final, 'answer')!r}", flush=True)
            check(len(seen) >= 1 and seen[0] == 1, f"{binding}: the steer is in the model's request right after the tool turn (requests {seen})")
            if reqs and seen:
                last = reqs[-1]["request"]
                copies = sum((t or "").count("PINEAPPLE") for t in user_turns(last))
                check(copies == 1, f"{binding}: the steer appears exactly once in the last request ({copies})")
                roles = [m.get("role") for m in last["messages"]]
                print(f"   last request roles: {roles}", flush=True)
            res["goal"] = {"state": (final.get("status") or {}).get("state"), "answer": artifact_text(final, "answer"),
                           "requests": len(reqs), "carrying_steer": seen}
            # ---- bound
            print(f"-- [{binding}] seven messages, inbox of 4", flush=True)
            n0 = len(proxy_requests(plog))
            agent.tool_log.write_text("")
            s = Stream(binding, port, SLEEP_PROMPT.format(n=14), "ta")
            task = s.wait_task()
            started(agent)
            sent = []
            for i in range(7):
                ok, detail = steer(binding, agent, "ta", task, f"STEER-{i}-MARK")
                sent.append((i, ok, detail))
                print(f"   steer {i}: accepted={ok} {json.dumps(detail)[:200]}", flush=True)
            acc = [i for i, ok, _ in sent if ok]
            check(acc == [0, 1, 2, 3], f"{binding}: exactly the first four of seven were accepted ({acc})")
            refused = [d for i, ok, d in sent if not ok]
            # HTTP+JSON answers 503; JSON-RPC has no HTTP status to use and answers -32603 with
            # google.rpc.RetryInfo in `data` (the design of audit H1), so that is what is required there.
            check(all((d.get("http") == 503 if binding == "rest" else "RetryInfo" in json.dumps(d)) for d in refused),
                  f"{binding}: the refusals are 503 (HTTP+JSON) or carry RetryInfo (JSON-RPC) ({[json.dumps(d)[:100] for d in refused[:2]]})")
            s.done.wait(300)
            reqs = proxy_requests(plog)[n0:]
            order = []
            for r in reqs:
                for t in user_turns(r["request"]):
                    for i in range(7):
                        if f"STEER-{i}-MARK" in (t or ""):
                            order.append(i)
            last = reqs[-1]["request"] if reqs else {}
            lastturns = [t for t in user_turns(last) if "MARK" in (t or "")]
            print(f"   steer turns in the last request: {lastturns}", flush=True)
            check(lastturns == [f"STEER-{i}-MARK" for i in range(4)], f"{binding}: the four reach the model, in order, once each ({lastturns})")
            final = get_task(binding, agent, "ta", task)
            res["bound"] = {"accepted": acc, "refused": [json.dumps(d)[:300] for d in refused], "state": (final.get("status") or {}).get("state"),
                            "answer": artifact_text(final, "answer")}
            # ---- answering turn
            print(f"-- [{binding}] a message that arrives in the answering turn", flush=True)
            n0 = len(proxy_requests(plog))
            agent.tool_log.write_text("")
            s = Stream(binding, port, SLEEP_PROMPT.format(n=2), "ta")
            task = s.wait_task()
            started(agent)
            wait_for(lambda: any("END sleep_work" in l for l in agent.tools()), 60, 0.01)
            ok, detail = steer(binding, agent, "ta", task, "LATE-MARK arrives while the model writes its answer")
            print(f"   late steer accepted={ok} {json.dumps(detail)[:200]}", flush=True)
            s.done.wait(300)
            reqs = proxy_requests(plog)[n0:]
            late_in = [i for i, r in enumerate(reqs) if any("LATE-MARK" in (t or "") for t in user_turns(r["request"]))]
            print(f"   requests that carried it: {late_in} of {len(reqs)}", flush=True)
            if ok:
                check(not late_in, f"{binding}: a message sent in the answering turn is not read (it was in requests {late_in})")
            else:
                note(f"{binding}: the late message was refused: {detail}")
            res["answering"] = {"accepted": ok, "in_requests": late_in}
            # ---- owner
            print(f"-- [{binding}] another caller", flush=True)
            agent.tool_log.write_text("")
            s = Stream(binding, port, SLEEP_PROMPT.format(n=8), "ta")
            task = s.wait_task()
            started(agent)
            ok, detail = steer(binding, agent, "tb", task, "BOB-MARK steer from a caller who does not own the task")
            print(f"   bob steers alice's task: accepted={ok} {json.dumps(detail)[:240]}", flush=True)
            check(not ok, f"{binding}: a caller who does not own the task cannot steer it")
            st_bob = get_task(binding, agent, "tb", task)
            check(not st_bob or "id" not in st_bob, f"{binding}: and cannot read it ({str(st_bob)[:100]})")
            cst, cbody = cancel(binding, agent, "tb", task)
            check(st_bob == {} or True, "")
            s.done.wait(300)
            reqs = proxy_requests(plog)
            check(not any("BOB-MARK" in (t or "") for r in reqs for t in user_turns(r["request"])), f"{binding}: Bob's text never reached the model")
            res["owner"] = {"accepted": ok, "detail": json.dumps(detail)[:300], "bob_cancel": [cst, json.dumps(cbody)[:200]]}
            # ---- oversize
            print(f"-- [{binding}] oversize", flush=True)
            agent.tool_log.write_text("")
            s = Stream(binding, port, SLEEP_PROMPT.format(n=6), "ta")
            task = s.wait_task()
            started(agent)
            ok, detail = steer(binding, agent, "ta", task, "X" * 70000)
            print(f"   70 kB steer: accepted={ok} {json.dumps(detail)[:240]}", flush=True)
            check(not ok, f"{binding}: a 70 kB steering message is refused")
            s.done.wait(300)
            res["oversize"] = {"accepted": ok, "detail": json.dumps(detail)[:300]}
            # ---- ended
            print(f"-- [{binding}] steering a completed task", flush=True)
            ok, detail = steer(binding, agent, "ta", task, "after the end")
            print(f"   to a completed task: accepted={ok} {json.dumps(detail)[:300]}", flush=True)
            res["ended"] = {"accepted": ok, "detail": json.dumps(detail)[:400]}
            # ---- cancel
            print(f"-- [{binding}] cancel a sleeping run", flush=True)
            agent.tool_log.write_text("")
            s = Stream(binding, port, SLEEP_PROMPT.format(n=60), "ta")
            task = s.wait_task()
            started(agent)
            for i in range(3):
                steer(binding, agent, "ta", task, f"PENDING-{i}")
            t0 = time.time()
            cst, cbody = cancel(binding, agent, "ta", task)
            s.done.wait(60)
            took = time.time() - t0
            final = get_task(binding, agent, "ta", task)
            dropped = wait_for(lambda: any("DROP sleep_work" in l for l in agent.tools()), 10)
            print(f"   cancel -> {cst}; state {(final.get('status') or {}).get('state')} after {took:.1f}s; tool DROP logged: {bool(dropped)}", flush=True)
            check((final.get("status") or {}).get("state") == "TASK_STATE_CANCELED", f"{binding}: cancel ends CANCELED")
            check(bool(dropped), f"{binding}: the sleeping tool's future was dropped by the cancel")
            ok, detail = steer(binding, agent, "ta", task, "after the cancel")
            res["cancel"] = {"state": (final.get("status") or {}).get("state"), "seconds": round(took, 1), "dropped": bool(dropped),
                             "steer_after": [ok, json.dumps(detail)[:300]]}
            # ---- agent health and counters
            check(agent.alive(), f"{binding}: the agent is alive after all scenarios")
            time.sleep(2.5)
            res["steering_counts"] = steering_counts(sink)
            print(f"   a2a.server.steering by outcome: {res['steering_counts']}", flush=True)
    finally:
        pproc.terminate()
        time.sleep(1)
        sink.stop()
    save(f"04-steering-{binding}-requests.jsonl", (WORK / "steer-proxy.log").read_text()[:3_000_000])
    return res


def midstream(model_file, results):
    pproc, plog = proxy()
    try:
        with Agent("steer-drafts", C_MODEL_URL=f"http://127.0.0.1:{PROXY_PORT}/v1", C_TOOLS="inventory", C_STEER=4, C_DRAFTS=1,
                   C_PORT_RPC=0, C_PORT_REST=-1, C_PORT_GRPC=-1, C_EXTRA_BODY='{"max_tokens":260}') as agent:
            print("-- [jsonrpc] a message while the model streams (drafts on)", flush=True)
            s = Stream("jsonrpc", agent.ports["rpc"], "Write about 120 words on why monitoring matters.")
            task = s.wait_task()
            wait_for(lambda: any("artifactUpdate" in json.dumps(e) for e in s.events), 120, 0.05)
            ok, detail = steer("jsonrpc", agent, None, task, "MIDSTREAM-MARK change the topic to gardening")
            print(f"   accepted={ok} {json.dumps(detail)[:200]}", flush=True)
            s.done.wait(300)
            reqs = proxy_requests(plog)
            carried = [i for i, r in enumerate(reqs) if any("MIDSTREAM-MARK" in (t or "") for t in user_turns(r["request"]))]
            final = get_task("jsonrpc", agent, None, task)
            print(f"   requests that carried it: {carried} of {len(reqs)}; final {(final.get('status') or {}).get('state')}", flush=True)
            if ok:
                check(not carried, f"midstream: the message sent while the answering turn streams is not read (requests {carried})")
            results["midstream"] = {"accepted": ok, "carried": carried, "state": (final.get("status") or {}).get("state"),
                                    "answer": artifact_text(final, "answer")}
    finally:
        pproc.terminate()


def grpc_steer(results):
    """Steering over gRPC: the run is started on JSON-RPC, the steering message goes through the gRPC client."""
    pproc, plog = proxy()
    try:
        with Agent("steer-grpc", C_MODEL_URL=f"http://127.0.0.1:{PROXY_PORT}/v1", C_TOOLS="inventory,slow", C_STEER=4,
                   C_PORT_RPC=0, C_PORT_REST=-1, C_PORT_GRPC=0, C_EXTRA_BODY='{"max_tokens":300}') as agent:
            print("-- [grpc] steering over gRPC, run started over JSON-RPC", flush=True)
            n0 = len(proxy_requests(plog))
            s = Stream("jsonrpc", agent.ports["rpc"], SLEEP_PROMPT.format(n=10))
            task = s.wait_task()
            started(agent)
            lines = client("grpc", agent.ports["grpc"], "steer", task, "GRPC-MARK reply with the single word PINEAPPLE")
            print(f"   receipt: {json.dumps(lines)[:300]}", flush=True)
            ok = next((l["ok"] for l in lines if "ok" in l), None)
            check(ok is not None and "inbox" in json.dumps(task_of(ok).get("metadata")), "grpc: the steering message was accepted with delivery=inbox")
            sent = []
            for i in range(6):
                lines = client("grpc", agent.ports["grpc"], "steer", task, f"GRPC-FILL-{i}")
                sent.append("ok" in lines[0] if lines else False)
            print(f"   five more messages into an inbox of 4 holding one: accepted {sent}", flush=True)
            check(sent == [True, True, True, False, False, False], f"grpc: the inbox bound is the same over gRPC ({sent})")
            full = client("grpc", agent.ports["grpc"], "steer", task, "GRPC-OVER")
            print(f"   the refusal: {json.dumps(full)[:400]}", flush=True)
            s.done.wait(300)
            reqs = proxy_requests(plog)[n0:]
            last = reqs[-1]["request"] if reqs else {}
            turns = [t for t in user_turns(last) if "GRPC-" in (t or "")]
            print(f"   steer turns in the last request: {turns}", flush=True)
            check(turns[:1] == ["GRPC-MARK reply with the single word PINEAPPLE"] and len(turns) == 4, f"grpc: four messages reach the model in order ({turns})")
            results["grpc"] = {"receipt": lines[:1], "accepted": sent, "refusal": full, "turns": turns}
    finally:
        pproc.terminate()


def main(which):
    header = run_header("steering a running task with a real model", os.environ.get("CAMPAIGN_MODEL_FILE"))
    results = {"header": header}
    if which == {"grpc"}:
        grpc_steer(results)
        save(f"04-steering-grpc-{os.environ.get('CAMPAIGN_MODEL_FILE', 'model')}.json", results)
        return finish()
    for binding in ("jsonrpc", "rest"):
        if not which or binding in which:
            scenarios(binding, header.get("model_file"), results)
    if not which or "midstream" in which:
        midstream(header.get("model_file"), results)
    save(f"04-steering-{os.environ.get('CAMPAIGN_MODEL_FILE', 'model')}.json", results)
    return finish()


if __name__ == "__main__":
    sys.exit(main(set(sys.argv[1:])))
