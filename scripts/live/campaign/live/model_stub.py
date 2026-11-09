#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
"""A scripted OpenAI-compatible model for provider misbehaviour, extending
scripts/live/adopter/model_stub.py (which it does not replace).

    model_stub.py PORT SCENARIO [ARG]

Needs python3 only. Every request body is appended, one JSON line, to
$MODEL_STUB_LOG (default ./model_stub.log), so a run shows what the SDK sent back.
The stream form (`"stream": true` in the request, which the agent sends when
C_DRAFTS=1) is answered as server-sent events; both forms are supported by every
scenario that makes sense in both.

  script FILE      replies from a JSON list, one per request: {"text", "tool_calls": [{"name",
                   "arguments"}], "finish_reason", "usage": {...} | "absent", "model"}; past the
                   end: text "done"
  html             HTTP 200 with an HTML body
  sse_never        stream: one chunk, then silence for good (non-stream: silence)
  sse_trickle      stream: a keep-alive comment every second for ever (no read is ever idle)
  json_trickle     non-stream: a valid body sent one byte a second
  sse_big          stream: one `data:` line of ARG bytes (default 100 MB) of content
  json_big         non-stream: one JSON body whose content is ARG bytes (default 100 MB)
  calls_many       one reply with ARG (default 10000) tool calls to a tool named `noop`
  name_big         one tool call whose name is ARG bytes (default 1 MiB)
  name_path        one tool call named "../../etc/passwd", then text
  finish           a text reply with finish_reason ARG (e.g. "weird", "tool_calls", "")
  cut_mid          the body is cut mid-JSON (non-stream) or the stream ends mid-event
  retry_after      HTTP 429 with Retry-After: ARG (seconds or an HTTP date)
  stall_chunked    chunked body: ARG bytes (default 100), then a stall of 600 s
  usage            a text reply whose usage is ARG: absent | zero | negative | huge | float | string
  models           a text reply whose `model` differs on every request (cardinality probe)
  args_big         one call to `echo` with ARG bytes (default 1 MiB) of arguments
  args_deep        one call to `echo` with arguments nested ARG (default 100000) levels deep
  args_notjson     one call whose arguments are ARG (default: not JSON) as a raw string
  err_body         HTTP 502 whose body is ARG (a canary the SDK must not hand to the caller)
"""
import json
import os
import sys
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

PORT = int(sys.argv[1])
SCENARIO = sys.argv[2]
ARG = sys.argv[3] if len(sys.argv) > 3 else None
LOG = os.environ.get("MODEL_STUB_LOG", "model_stub.log")
LOCK = threading.Lock()
COUNT = [0]
SCRIPT = json.load(open(ARG)) if SCENARIO == "script" and ARG else []


def usage_for(mode):
    base = {"prompt_tokens": 10, "completion_tokens": 5, "total_tokens": 15}
    return {
        "absent": None,
        "zero": {"prompt_tokens": 0, "completion_tokens": 0, "total_tokens": 0},
        "negative": {"prompt_tokens": -5, "completion_tokens": -7, "total_tokens": -12},
        "huge": {"prompt_tokens": 18446744073709551615, "completion_tokens": 18446744073709551615, "total_tokens": 1},
        "float": {"prompt_tokens": 1.5, "completion_tokens": 2.5, "total_tokens": 4.0},
        "string": {"prompt_tokens": "ten", "completion_tokens": "five", "total_tokens": "x"},
    }.get(mode, base)


def message(text=None, calls=()):
    m = {"role": "assistant", "content": text}
    if calls:
        m["tool_calls"] = [
            {"id": f"call_{i}", "type": "function",
             "function": {"name": c["name"], "arguments": c["arguments"] if isinstance(c["arguments"], str) else json.dumps(c["arguments"])}}
            for i, c in enumerate(calls)
        ]
    return m


def completion(msg, finish="stop", usage="default", model="stub"):
    body = {"id": "chatcmpl-stub", "object": "chat.completion", "created": 0, "model": model,
            "choices": [{"index": 0, "message": msg, "finish_reason": finish}]}
    u = usage_for("default") if usage == "default" else usage
    if u is not None:
        body["usage"] = u
    return body


def sse_events(msg, finish="stop", usage="default", model="stub"):
    """The stream form of one reply: a delta chunk per piece, then a final chunk with the reason."""
    def chunk(delta, fin=None, u=None):
        c = {"id": "chatcmpl-stub", "object": "chat.completion.chunk", "created": 0, "model": model,
             "choices": [{"index": 0, "delta": delta, "finish_reason": fin}]}
        if u:
            c["usage"] = u
        return f"data: {json.dumps(c)}\n\n".encode()

    out = [chunk({"role": "assistant"})]
    if msg.get("content"):
        out.append(chunk({"content": msg["content"]}))
    for i, call in enumerate(msg.get("tool_calls") or []):
        out.append(chunk({"tool_calls": [{"index": i, "id": call["id"], "type": "function", "function": call["function"]}]}))
    u = usage_for("default") if usage == "default" else usage
    out.append(chunk({}, finish, u))
    out.append(b"data: [DONE]\n\n")
    return out


class H(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, *a):
        pass

    def send_json(self, code, body, extra=()):
        b = body if isinstance(body, bytes) else json.dumps(body).encode()
        self.send_response(code)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(b)))
        for h, v in extra:
            self.send_header(h, v)
        self.end_headers()
        self.wfile.write(b)

    def reply(self, stream, msg, finish="stop", usage="default", model="stub"):
        if not stream:
            return self.send_json(200, completion(msg, finish, usage, model))
        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream")
        self.send_header("Transfer-Encoding", "chunked")
        self.end_headers()
        for piece in sse_events(msg, finish, usage, model):
            self.wfile.write(f"{len(piece):x}\r\n".encode() + piece + b"\r\n")
        self.wfile.write(b"0\r\n\r\n")

    def do_POST(self):
        n = int(self.headers.get("content-length") or 0)
        raw = self.rfile.read(n)
        try:
            req = json.loads(raw or b"{}")
        except ValueError:
            req = {}
        with LOCK:
            COUNT[0] += 1
            turn = COUNT[0]
            with open(LOG, "a") as f:
                f.write(json.dumps({"scenario": SCENARIO, "turn": turn, "path": self.path, "body": req}) + "\n")
        stream = bool(req.get("stream"))
        try:
            self.handle_scenario(turn, stream)
        except (BrokenPipeError, ConnectionResetError):
            pass

    def handle_scenario(self, turn, stream):
        s = SCENARIO
        if s == "script":
            item = SCRIPT[turn - 1] if turn <= len(SCRIPT) else {"text": "done"}
            usage = "default" if "usage" not in item else (None if item["usage"] == "absent" else item["usage"])
            return self.reply(stream, message(item.get("text"), item.get("tool_calls") or ()),
                              item.get("finish_reason") or ("tool_calls" if item.get("tool_calls") else "stop"),
                              usage, item.get("model", "stub"))
        if s == "html":
            return self.send_json(200, b"<html><body>Service unavailable page</body></html>")
        if s == "sse_never":
            self.send_response(200)
            self.send_header("Content-Type", "text/event-stream")
            self.send_header("Transfer-Encoding", "chunked")
            self.end_headers()
            piece = b': keep-alive\n\n'
            self.wfile.write(f"{len(piece):x}\r\n".encode() + piece + b"\r\n")
            self.wfile.flush()
            time.sleep(3600)
            return
        if s == "sse_trickle":
            # A keep-alive comment every second, for ever: no single read is idle, the turn never ends.
            self.send_response(200)
            self.send_header("Content-Type", "text/event-stream")
            self.send_header("Transfer-Encoding", "chunked")
            self.end_headers()
            while True:
                piece = b': keep-alive\n\n'
                self.wfile.write(f"{len(piece):x}\r\n".encode() + piece + b"\r\n")
                self.wfile.flush()
                time.sleep(1)
        if s == "json_trickle":
            # A non-stream body sent one byte a second.
            body = json.dumps(completion(message("slow answer"))).encode()
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            for b in body:
                self.wfile.write(bytes([b]))
                self.wfile.flush()
                time.sleep(1)
            return
        if s == "sse_big":
            size = int(ARG or 100 * 1024 * 1024)
            self.send_response(200)
            self.send_header("Content-Type", "text/event-stream")
            self.send_header("Transfer-Encoding", "chunked")
            self.end_headers()
            head = b'data: {"choices":[{"index":0,"delta":{"content":"'
            self.wfile.write(f"{len(head):x}\r\n".encode() + head + b"\r\n")
            block = b"A" * 65536
            sent = 0
            while sent < size:
                piece = block[: min(len(block), size - sent)]
                self.wfile.write(f"{len(piece):x}\r\n".encode() + piece + b"\r\n")
                sent += len(piece)
            tail = b'"},"finish_reason":null}]}\n\ndata: [DONE]\n\n'
            self.wfile.write(f"{len(tail):x}\r\n".encode() + tail + b"\r\n0\r\n\r\n")
            return
        if s == "json_big":
            size = int(ARG or 100 * 1024 * 1024)
            head = b'{"id":"x","object":"chat.completion","created":0,"model":"stub","choices":[{"index":0,"message":{"role":"assistant","content":"'
            tail = b'"},"finish_reason":"stop"}]}'
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(head) + size + len(tail)))
            self.end_headers()
            self.wfile.write(head)
            block = b"A" * 65536
            sent = 0
            while sent < size:
                piece = block[: min(len(block), size - sent)]
                self.wfile.write(piece)
                sent += len(piece)
            self.wfile.write(tail)
            return
        if s == "calls_many":
            k = int(ARG or 10000)
            return self.reply(stream, message(None, [{"name": "noop", "arguments": "{}"}] * k), "tool_calls")
        if s == "name_big":
            return self.reply(stream, message(None, [{"name": "n" * int(ARG or 1048576), "arguments": "{}"}]), "tool_calls")
        if s == "name_path":
            if turn == 1:
                return self.reply(stream, message(None, [{"name": "../../etc/passwd", "arguments": "{}"}]), "tool_calls")
            return self.reply(stream, message("done"))
        if s == "finish":
            return self.reply(stream, message("An answer."), ARG if ARG is not None else "weird")
        if s == "cut_mid":
            body = json.dumps(completion(message("An answer that is cut mid body"))).encode()
            if stream:
                self.send_response(200)
                self.send_header("Content-Type", "text/event-stream")
                self.send_header("Transfer-Encoding", "chunked")
                self.end_headers()
                piece = b'data: {"choices":[{"index":0,"delta":{"content":"par'
                self.wfile.write(f"{len(piece):x}\r\n".encode() + piece + b"\r\n")
                self.wfile.flush()
                self.close_connection = True
                return
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body[: len(body) // 2])
            self.wfile.flush()
            self.close_connection = True
            return
        if s == "retry_after":
            return self.send_json(429, {"error": {"message": "rate limited"}}, extra=[("Retry-After", ARG or "1")])
        if s == "stall_chunked":
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.send_header("Transfer-Encoding", "chunked")
            self.end_headers()
            piece = b'{"id":"x","object":"chat.completion","choices":[{"index":0,"message":{"role":"assistant","content":"' [: int(ARG or 100)]
            self.wfile.write(f"{len(piece):x}\r\n".encode() + piece + b"\r\n")
            self.wfile.flush()
            time.sleep(600)
            return
        if s == "usage":
            return self.reply(stream, message("A short answer."), "stop", usage_for(ARG or "absent"))
        if s == "models":
            return self.reply(stream, message("A short answer."), "stop", "default", f"model-variant-{turn:05d}-{os.urandom(4).hex()}")
        if s == "args_big":
            return self.reply(stream, message(None, [{"name": "echo", "arguments": '{"text":"' + "A" * int(ARG or 1048576) + '"}'}]), "tool_calls") \
                if turn == 1 else self.reply(stream, message("done"))
        if s == "args_deep":
            depth = int(ARG or 100000)
            args = '{"text":' + "[" * depth + "]" * depth + "}"
            return self.reply(stream, message(None, [{"name": "echo", "arguments": args}]), "tool_calls") \
                if turn == 1 else self.reply(stream, message("done"))
        if s == "args_notjson":
            return self.reply(stream, message(None, [{"name": "echo", "arguments": ARG or "not json at all {{{"}]), "tool_calls") \
                if turn == 1 else self.reply(stream, message("done"))
        if s == "err_body":
            return self.send_json(502, (ARG or "CANARY_PROVIDER_ERROR_BODY").encode())
        self.send_json(500, {"error": {"message": f"unknown scenario {s}"}})


def main():
    server = ThreadingHTTPServer(("127.0.0.1", PORT), H)
    server.daemon_threads = True
    print(f"model_stub {SCENARIO} on 127.0.0.1:{PORT}", flush=True)
    server.serve_forever()


if __name__ == "__main__":
    main()
