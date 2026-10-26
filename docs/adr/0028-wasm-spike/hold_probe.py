# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
"""One request, or a short sequence, against the handler Worker, for use on a fresh isolate.

usage: hold_probe.py STEP...   where STEP is hold1 (non-blocking send, ctx.wait_until(wait_idle)),
hold0 (the same without it), hang, cancel or stream. Prints the task state the send reaches.
Standard library only.
"""
import json, sys, time, uuid, urllib.request
OPEN = urllib.request.build_opener(urllib.request.ProxyHandler({})).open
BASE="http://127.0.0.1:8787/"
def rpc(method, params, query=""):
    b=json.dumps({"jsonrpc":"2.0","id":1,"method":method,"params":params}).encode()
    return json.loads(OPEN(urllib.request.Request(BASE+query,b,{"content-type":"application/json"}),timeout=20).read())
def send(text, query="", immediate=False):
    p={"message":{"messageId":str(uuid.uuid4()),"role":"ROLE_USER","parts":[{"text":text}]}}
    if immediate: p["configuration"]={"returnImmediately":True}
    return rpc("SendMessage",p,query)
def state(r): r=r.get("result",{}); return r.get("task",r).get("status",{}).get("state")
def settle(query):
    r=send("slow",query,True); tid=r["result"]["task"]["id"]
    for _ in range(20):
        time.sleep(0.1); s=state(rpc("GetTask",{"id":tid}))
        if s=="TASK_STATE_COMPLETED": return s
    return s
for step in sys.argv[1:]:
    if step=="hang": print("hang ->", state(send("hang")))
    elif step=="hold1": print("hold=1 ->", settle("?hold=1"))
    elif step=="hold0": print("hold=0 ->", settle("?hold=0"))
    elif step=="stream":
        b=json.dumps({"jsonrpc":"2.0","id":1,"method":"SendStreamingMessage","params":{"message":{"messageId":str(uuid.uuid4()),"role":"ROLE_USER","parts":[{"text":"slow"}]}}}).encode()
        OPEN(urllib.request.Request(BASE,b,{"content-type":"application/json"}),timeout=20).read(); print("stream done")
    elif step=="cancel":
        r=send("until-cancelled",immediate=True); tid=r["result"]["task"]["id"]; time.sleep(.3); rpc("CancelTask",{"id":tid}); print("cancelled")
