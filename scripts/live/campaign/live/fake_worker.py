#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
"""A scripted A2A JSON-RPC peer that reports what it spent as it likes.

    fake_worker.py PORT [SPENT]      SPENT: a number (default 18446744073709551615, u64::MAX) or `none`

Needs python3 only. `SendStreamingMessage` answers with a task, then a final `completed` status update whose
metadata carries the budget extension's status with `spent.tokens` = SPENT; `GetTask` returns the task with
an `answer` artifact. Used by 09_hostile_child.py: a delegating coordinator settles its budget with what the
child reports, and a child is another party's code.
"""
import json
import sys
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

PORT = int(sys.argv[1])
SPENT = sys.argv[2] if len(sys.argv) > 2 else "18446744073709551615"
LOCK = threading.Lock()
LOG = []

TASK = {"id": "fake-task-1", "contextId": "fake-ctx-1", "status": {"state": "TASK_STATE_WORKING"}}


class H(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, *a):
        pass

    def reply(self, code, body, ctype="application/json"):
        b = body if isinstance(body, bytes) else json.dumps(body).encode()
        self.send_response(code)
        self.send_header("Content-Type", ctype)
        self.send_header("Content-Length", str(len(b)))
        self.end_headers()
        self.wfile.write(b)

    def do_GET(self):
        self.reply(404, {"error": "no card"})

    def do_POST(self):
        n = int(self.headers.get("content-length") or 0)
        req = json.loads(self.rfile.read(n) or b"{}")
        method, rid = req.get("method"), req.get("id")
        with LOCK:
            LOG.append(req)
        if method in ("SendStreamingMessage", "message/stream"):
            metadata = {}
            if SPENT != "none":
                spent = json.loads(SPENT)
                metadata = {"a2a-rust.com/budget": {"meters": {"tokens": 0}, "spent": {"tokens": spent}}}
            events = [
                {"task": {**TASK, "status": {"state": "TASK_STATE_WORKING"}}},
                {"statusUpdate": {"taskId": TASK["id"], "contextId": TASK["contextId"],
                                  "status": {"state": "TASK_STATE_COMPLETED"}, "metadata": metadata}},
            ]
            self.send_response(200)
            self.send_header("Content-Type", "text/event-stream")
            self.send_header("Transfer-Encoding", "chunked")
            self.end_headers()
            for e in events:
                piece = f"data: {json.dumps({'jsonrpc': '2.0', 'id': rid, 'result': e})}\n\n".encode()
                self.wfile.write(f"{len(piece):x}\r\n".encode() + piece + b"\r\n")
                self.wfile.flush()
            self.wfile.write(b"0\r\n\r\n")
            return
        if method in ("GetTask", "tasks/get"):
            task = {**TASK, "status": {"state": "TASK_STATE_COMPLETED"},
                    "artifacts": [{"artifactId": "answer", "parts": [{"text": "a fake answer"}]}]}
            return self.reply(200, {"jsonrpc": "2.0", "id": rid, "result": task})
        self.reply(200, {"jsonrpc": "2.0", "id": rid, "error": {"code": -32601, "message": f"no method {method}"}})


if __name__ == "__main__":
    s = ThreadingHTTPServer(("127.0.0.1", PORT), H)
    s.daemon_threads = True
    print(f"fake_worker on {PORT}, spent={SPENT}", flush=True)
    s.serve_forever()
