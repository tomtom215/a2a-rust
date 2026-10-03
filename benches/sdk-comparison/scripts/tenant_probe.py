# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
"""Black-box tenant-isolation probe over JSON-RPC (A2A v1.0 `tenant` request field).
Raw HTTP only; no SDK client involved. Prints one JSON line per check.
LEAK = tenant B observed or mutated tenant A's task."""
import json, sys, time, uuid, urllib.request, http.client

base = sys.argv[1].rstrip('/')
label = sys.argv[2]

def rpc(method, params, timeout=5):
    body = json.dumps({"jsonrpc": "2.0", "id": str(uuid.uuid4()), "method": method, "params": params}).encode()
    req = urllib.request.Request(base + "/", data=body, headers={"content-type": "application/json", "A2A-Version": "1.0"})
    with urllib.request.urlopen(req, timeout=timeout) as r:
        return json.loads(r.read())

def send(tenant, text, immediate=False):
    p = {"tenant": tenant, "message": {"role": "ROLE_USER", "messageId": str(uuid.uuid4()), "parts": [{"text": text}]}}
    if immediate:
        p["configuration"] = {"returnImmediately": True}
    r = rpc("SendMessage", p)
    return r["result"]["task"]["id"]

def outcome(r):
    if "error" in r:
        return "error %s %s" % (r["error"].get("code"), r["error"].get("message", "")[:60])
    res = r["result"]
    if "status" in res:
        return "task state=%s" % res["status"]["state"]
    if "tasks" in res:
        return "list n=%d" % len(res["tasks"])
    return "result " + json.dumps(res)[:80]

results = []
def check(name, r, leak_if):
    o = outcome(r)
    results.append({"server": label, "check": name, "outcome": o, "verdict": "LEAK" if leak_if(r) else "isolated"})

first = rpc("SendMessage", {"tenant": "tenant-a", "message": {"role": "ROLE_USER", "messageId": str(uuid.uuid4()), "parts": [{"text": "hello"}]}})
if "error" in first:
    # The server refuses tenants it cannot isolate: nothing is stored, so
    # there is nothing for tenant B to reach. That is the fail-closed outcome.
    print(json.dumps({"server": label, "check": "A SendMessage with a tenant", "outcome": outcome(first), "verdict": "refused"}))
    sys.exit(0)
done_id = first["result"]["task"]["id"]
# positive control: owner can read it
r = rpc("GetTask", {"tenant": "tenant-a", "id": done_id})
results.append({"server": label, "check": "control: A GetTask own task", "outcome": outcome(r),
                "verdict": "ok" if "result" in r else "BROKEN-CONTROL"})
check("B GetTask(A's task)", rpc("GetTask", {"tenant": "tenant-b", "id": done_id}), lambda r: "result" in r)
r = rpc("ListTasks", {"tenant": "tenant-b"})
check("B ListTasks sees A's task", r, lambda r: "result" in r and any(t["id"] == done_id for t in r["result"].get("tasks", [])))

wait_id = send("tenant-a", "wait:forever", immediate=True)
time.sleep(0.3)
# subscribe as B over SSE, read up to 1s
def sse(tenant, tid):
    conn = http.client.HTTPConnection(base.split("//")[1], timeout=2)
    body = json.dumps({"jsonrpc": "2.0", "id": 1, "method": "SubscribeToTask", "params": {"tenant": tenant, "id": tid}})
    conn.request("POST", "/", body, {"content-type": "application/json", "accept": "text/event-stream", "A2A-Version": "1.0"})
    resp = conn.getresponse()
    try:
        data = resp.read1(4096).decode(errors="replace")
    except Exception as e:
        data = "read-error %s" % e
    conn.close()
    return resp.status, data
st, data = sse("tenant-b", wait_id)
leak = ('"result"' in data) and (wait_id in data)
results.append({"server": label, "check": "B SubscribeToTask(A's live task)", "outcome": "http %d: %s" % (st, data.strip().replace("\n", " ")[:110]), "verdict": "LEAK" if leak else "isolated"})
r = rpc("CancelTask", {"tenant": "tenant-b", "id": wait_id})
check("B CancelTask(A's live task)", r, lambda r: "result" in r)
r2 = rpc("GetTask", {"tenant": "tenant-a", "id": wait_id})
results.append({"server": label, "check": "after B's cancel, A sees", "outcome": outcome(r2),
                "verdict": "LEAK" if "CANCELED" in outcome(r2) else "isolated"})
for x in results:
    print(json.dumps(x))
