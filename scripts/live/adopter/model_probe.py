#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
"""One chat completion against a local llama-server, timed, with thinking on or off.

    model_probe.py [--think] [--max N] [--url URL] [PROMPT]

Prints the finish reason, the content, whether reasoning text came back, the
token counts and the server's own decode rate. Default: thinking off
(`chat_template_kwargs.enable_thinking=false`), 64 tokens, port 11434.
Needs python3 only; run it before the Rust examples to see what the box does.
"""
import argparse
import json
import time
import urllib.request

ap = argparse.ArgumentParser()
ap.add_argument("prompt", nargs="?", default="What is 17*23? Answer with the number.")
ap.add_argument("--think", action="store_true")
ap.add_argument("--max", type=int, default=64)
ap.add_argument("--url", default="http://127.0.0.1:11434")
ap.add_argument("--timeout", type=int, default=900)
a = ap.parse_args()

body = {"model": "x", "max_tokens": a.max, "messages": [{"role": "user", "content": a.prompt}]}
if not a.think:
    body["chat_template_kwargs"] = {"enable_thinking": False}
req = urllib.request.Request(a.url + "/v1/chat/completions", json.dumps(body).encode(), {"content-type": "application/json"})
opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
t = time.time()
d = json.load(opener.open(req, timeout=a.timeout))
dt = time.time() - t
m = d["choices"][0]
print("thinking:", "on" if a.think else "off", "| max_tokens:", a.max, "| wall: %.1f s" % dt)
print("finish:", m["finish_reason"])
print("content:", repr(m["message"].get("content"))[:300])
print("reasoning returned:", bool(m["message"].get("reasoning_content")), len(m["message"].get("reasoning_content") or ""), "chars")
print("usage:", d["usage"])
print("server decode rate:", d.get("timings", {}).get("predicted_per_second"), "tok/s; prompt rate:", d.get("timings", {}).get("prompt_per_second"))
