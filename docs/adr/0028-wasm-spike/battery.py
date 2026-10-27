#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
"""Drives the handler Worker (`wrangler dev --local`, default http://127.0.0.1:8787/)
and asserts what the real `RequestHandler` does inside workerd.

Exit status is the number of failed assertions. Standard library only.
usage: battery.py [BASE_URL]
"""
import json, sys, time, uuid, threading, urllib.request

# No proxies: the Worker is on loopback, and a sandbox's proxy variables would otherwise intercept.
OPEN = urllib.request.build_opener(urllib.request.ProxyHandler({})).open
BASE = sys.argv[1] if len(sys.argv) > 1 else "http://127.0.0.1:8787/"
failures = []


def call(method, params, query="", timeout=20):
    body = json.dumps({"jsonrpc": "2.0", "id": 1, "method": method, "params": params}).encode()
    req = urllib.request.Request(BASE + query, body, {"content-type": "application/json"})
    t0 = time.monotonic()
    with OPEN(req, timeout=timeout) as r:
        raw = r.read().decode()
    return raw, time.monotonic() - t0


def rpc(method, params, query="", timeout=20):
    raw, took = call(method, params, query, timeout)
    return json.loads(raw), took


def send(text, query="", immediate=False):
    params = {"message": {"messageId": str(uuid.uuid4()), "role": "ROLE_USER", "parts": [{"text": text}]}}
    if immediate:
        params["configuration"] = {"returnImmediately": True}
    return rpc("SendMessage", params, query)


def task_of(resp):
    r = resp.get("result", {})
    return r.get("task", r)


def check(name, ok, detail=""):
    print(f"{'PASS' if ok else 'FAIL'}  {name}" + (f"  [{detail}]" if detail else ""))
    if not ok:
        failures.append(name)


# 1. a blocking send runs the executor and returns the finished task
r, took = send("hello")
t = task_of(r)
art = (t.get("artifacts") or [{}])[0].get("parts", [{}])[0].get("text")
check("blocking SendMessage completes with the executor's artifact",
      t.get("status", {}).get("state") == "TASK_STATE_COMPLETED" and art == "Hello, hello!", f"{took*1000:.0f} ms")

# 2. state survives into a second request
g, _ = rpc("GetTask", {"id": t["id"]})
check("GetTask in a later request returns the same completed task",
      task_of(g).get("id") == t["id"] and task_of(g).get("status", {}).get("state") == "TASK_STATE_COMPLETED")

# 3. a timer the executor awaits fires, repeatedly, across requests (the wasmtimer failure)
took_all = []
for _ in range(3):
    r, took = send("slow")
    took_all.append(took)
    if task_of(r).get("status", {}).get("state") != "TASK_STATE_COMPLETED":
        break
check("executor timer fires on 3 consecutive requests (each >= 80 ms)",
      len(took_all) == 3 and all(x >= 0.08 for x in took_all), ", ".join(f"{x*1000:.0f} ms" for x in took_all))

# 4. the handler's own timeout fires and fails the task
r, took = send("hang")
check("executor_timeout (400 ms) fails a hung executor, in under 2 s",
      task_of(r).get("status", {}).get("state") == "TASK_STATE_FAILED" and took < 2.0, f"{took*1000:.0f} ms")

# 5. cancel
r, _ = send("until-cancelled", immediate=True)
tid = task_of(r)["id"]
time.sleep(0.3)
c, _ = rpc("CancelTask", {"id": tid})
time.sleep(0.3)
g, _ = rpc("GetTask", {"id": tid})
check("CancelTask ends a running executor; GetTask then reports CANCELED",
      task_of(c).get("status", {}).get("state") == "TASK_STATE_CANCELED"
      and task_of(g).get("status", {}).get("state") == "TASK_STATE_CANCELED")

# 6. streaming
raw, took = call("SendStreamingMessage", {"message": {"messageId": str(uuid.uuid4()), "role": "ROLE_USER",
                                                       "parts": [{"text": "slow"}]}})
frames = [json.loads(l[len("data: "):]) for l in raw.split("\n") if l.startswith("data: ")]
kinds = [next(iter(f["result"])) for f in frames if "result" in f]
last_state = frames[-1]["result"].get("statusUpdate", {}).get("status", {}).get("state") if frames else None
check("SendStreamingMessage streams task, working, artifact, completed as SSE",
      kinds == ["task", "statusUpdate", "artifactUpdate", "statusUpdate"] and last_state == "TASK_STATE_COMPLETED",
      f"{kinds}")

# 7. concurrency
results = []
def one(i):
    r, _ = send("slow" if i % 3 == 0 else f"n{i}")
    results.append(task_of(r))
threads = [threading.Thread(target=one, args=(i,)) for i in range(30)]
[x.start() for x in threads]; [x.join() for x in threads]
check("30 concurrent sends all complete, with 30 distinct task ids",
      len(results) == 30 and all(x.get("status", {}).get("state") == "TASK_STATE_COMPLETED" for x in results)
      and len({x["id"] for x in results}) == 30)

# 8. error path
e, _ = rpc("Nope", {})
check("an unknown method is a JSON-RPC error", "error" in e)

# 9. work that outlives its response needs ctx.wait_until
def settles(query):
    r, _ = send("slow", query=query, immediate=True)
    tid = task_of(r)["id"]
    state = None
    for _ in range(20):
        time.sleep(0.1)
        state = task_of(rpc("GetTask", {"id": tid})[0]).get("status", {}).get("state")
        if state == "TASK_STATE_COMPLETED":
            break
    return state
def idle():
    return OPEN(BASE + "idle", timeout=20).read().decode() == "true"

# An executor left running by an earlier case would keep `wait_idle` waiting and
# let this case pass for the wrong reason, so it only counts from an idle handler.
if idle():
    with_hold = settles("?hold=1")
    check("non-blocking send whose executor needs a timer completes with ctx.wait_until(wait_idle)",
          with_hold == "TASK_STATE_COMPLETED", with_hold)
else:
    check("handler idle before the wait_until case (needed for it to mean anything)", False,
          "an earlier case left an executor running; run `hold_probe.py hold1` on a fresh isolate")
without = settles("?hold=0")
check("HAZARD (expected): the same send without wait_until never completes",
      without != "TASK_STATE_COMPLETED", without)

# 10. content signing, in-isolate
req = urllib.request.Request(BASE + "signed-check")
text = OPEN(req, timeout=20).read().decode()
check("ES256 signed artifact verifies; a tampered copy is rejected",
      "genuine_verifies=true" in text and "tampered_rejected=true" in text and "sig_in_metadata=true" in text, text)

print(f"\n{len(failures)} failed" if failures else "\nall passed")
sys.exit(len(failures))
