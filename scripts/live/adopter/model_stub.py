#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
"""A scripted OpenAI-compatible model for the failures a real small model only sometimes produces.

    model_stub.py [BASE_PORT]       (default 3500; each scenario has its own port)

    BASE+1  a tool call whose arguments are truncated JSON, then (after the error comes back) an answer
    BASE+2  a call to a tool that does not exist, then an answer
    BASE+3  tool arguments that are a JSON array, not an object, then an answer
    BASE+4  an answer cut off by the token limit (finish_reason "length")
    BASE+5  HTTP 500 whose body carries something that looks like a secret
    BASE+6  HTTP 429 with Retry-After: 2
    BASE+7  HTTP 200 with a body that is not JSON
    BASE+8  a 6 MiB answer
    BASE+9  the same tool call forever (the loop's turn bound must end it)
    BASE+10 an empty reply: no content, no tool calls
    BASE+11 a tool call written as JSON text in `content` (the page says the loop runs it)
    BASE+12 never answers (a half-open peer)

Every request body is appended to `$MODEL_STUB_LOG` (default ./model_stub.log), so a run shows
what the SDK actually sent back to the model after the bad turn. Needs python3 only.
"""
import json
import os
import sys
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

BASE = int(sys.argv[1]) if len(sys.argv) > 1 else 3500
LOG = os.environ.get("MODEL_STUB_LOG", "model_stub.log")
LOCK = threading.Lock()
COUNT = {}


def completion(msg, finish="stop"):
    return {"id": "x", "object": "chat.completion", "created": 0, "model": "stub",
            "choices": [{"index": 0, "message": msg, "finish_reason": finish}],
            "usage": {"prompt_tokens": 10, "completion_tokens": 5, "total_tokens": 15}}


def call(name, args, cid="call_1"):
    return {"role": "assistant", "content": None,
            "tool_calls": [{"id": cid, "type": "function", "function": {"name": name, "arguments": args}}]}


def make(port):
    k = port - BASE

    class H(BaseHTTPRequestHandler):
        protocol_version = "HTTP/1.1"

        def log_message(self, *a):
            pass

        def send(self, code, body, ctype="application/json", extra=()):
            b = body if isinstance(body, bytes) else json.dumps(body).encode()
            self.send_response(code)
            self.send_header("Content-Type", ctype)
            self.send_header("Content-Length", str(len(b)))
            for h, v in extra:
                self.send_header(h, v)
            self.end_headers()
            self.wfile.write(b)

        def do_POST(self):
            n = int(self.headers.get("content-length") or 0)
            body = self.rfile.read(n)
            with LOCK:
                COUNT[k] = COUNT.get(k, 0) + 1
                turn = COUNT[k]
                with open(LOG, "a") as f:
                    f.write(json.dumps({"scenario": k, "turn": turn, "path": self.path, "body": json.loads(body or b"{}")}) + "\n")
            has_tool_result = b'"role":"tool"' in body or b'"role": "tool"' in body
            if k == 1:
                self.send(200, completion(call("list_services", '{"service": ') if not has_tool_result else {"role": "assistant", "content": "I could not call the tool correctly."}, "tool_calls" if not has_tool_result else "stop"))
            elif k == 2:
                self.send(200, completion(call("launch_missiles", "{}") if not has_tool_result else {"role": "assistant", "content": "That tool does not exist; the services are payments-api, checkout and search."}, "tool_calls" if not has_tool_result else "stop"))
            elif k == 3:
                self.send(200, completion(call("service_status", '["checkout"]') if not has_tool_result else {"role": "assistant", "content": "The arguments were wrong."}, "tool_calls" if not has_tool_result else "stop"))
            elif k == 4:
                self.send(200, completion({"role": "assistant", "content": "Monitoring matters because it lets you see failures before your customers do, and because"}, "length"))
            elif k == 5:
                self.send(500, {"error": {"message": "upstream failure: Authorization: Bearer sk-live-SECRET-1234 rejected at db-internal.corp:5432", "type": "server_error"}})
            elif k == 6:
                self.send(429, {"error": {"message": "rate limited"}}, extra=[("Retry-After", "2")])
            elif k == 7:
                self.send(200, b"<html>not json</html>", ctype="text/html")
            elif k == 8:
                self.send(200, completion({"role": "assistant", "content": "a" * (6 * 1024 * 1024)}))
            elif k == 9:
                self.send(200, completion(call("list_services", "{}", cid=f"call_{turn}"), "tool_calls"))
            elif k == 10:
                self.send(200, completion({"role": "assistant", "content": ""}))
            elif k == 11:
                if has_tool_result:
                    self.send(200, completion({"role": "assistant", "content": "The services are payments-api, checkout and search."}))
                else:
                    self.send(200, completion({"role": "assistant", "content": json.dumps({"name": "list_services", "arguments": {}})}))
            elif k == 12:
                time.sleep(300)

    return H


def main():
    for i in range(1, 13):
        s = ThreadingHTTPServer(("127.0.0.1", BASE + i), make(BASE + i))
        s.daemon_threads = True
        threading.Thread(target=s.serve_forever, daemon=True).start()
    print(f"model_stub on 127.0.0.1:{BASE + 1}..{BASE + 12}", flush=True)
    while True:
        time.sleep(3600)


if __name__ == "__main__":
    main()
