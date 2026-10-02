# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
"""Live-LLM end-to-end benchmark. Raw HTTP/SSE (no SDK client) so the measured
difference is server-side SDK overhead only.

For each of PROMPTS x ROUNDS, interleaved (direct, a2a-rs, a2a-rust) in a
rotating order to spread model-server drift evenly:
  direct : POST llama-server /v1/chat/completions stream=true
  sdk    : POST <agent>/ SendStreamingMessage "llm:<prompt>"
Records TTFT (first non-empty text delta), total time, and the reassembled
text. Correctness check: SDK text must be byte-identical to direct text
(temperature 0, seed 42, same max_tokens, parallel=1 slot so batching cannot
perturb logits)."""
import json, sys, time, uuid, http.client, statistics

LLM = ("127.0.0.1", 11434)
AGENTS = {"a2a-rs": ("127.0.0.1", 3101), "a2a-rust": ("127.0.0.1", 3102)}
PROMPTS = [
    "Name three primary colors.",
    "What is 17 times 3? Answer with the number only.",
    "Write one sentence about the ocean.",
    "List the first five prime numbers.",
    "Translate 'good morning' to French.",
]
ROUNDS = int(sys.argv[1]) if len(sys.argv) > 1 else 4
MAX_TOKENS = 64

def sse_lines(resp):
    buf = b""
    while True:
        chunk = resp.read1(65536)
        if not chunk:
            break
        buf += chunk
        while b"\n" in buf:
            line, buf = buf.split(b"\n", 1)
            yield line.decode("utf-8", "replace").strip()

def direct(prompt):
    c = http.client.HTTPConnection(*LLM, timeout=120)
    body = json.dumps({"model": "qwen", "stream": True, "max_tokens": MAX_TOKENS, "temperature": 0.0, "seed": 42,
                       "messages": [{"role": "user", "content": prompt}], "chat_template_kwargs": {"enable_thinking": False}})
    t0 = time.perf_counter(); c.request("POST", "/v1/chat/completions", body, {"content-type": "application/json"})
    r = c.getresponse(); text, ttft = "", None
    for line in sse_lines(r):
        if not line.startswith("data:"): continue
        d = line[5:].strip()
        if d == "[DONE]": break
        t = json.loads(d)["choices"][0]["delta"].get("content") or ""
        if t and ttft is None: ttft = time.perf_counter() - t0
        text += t
    return {"ttft": ttft, "total": time.perf_counter() - t0, "text": text, "state": "n/a"}

def via_agent(addr, prompt):
    c = http.client.HTTPConnection(*addr, timeout=120)
    body = json.dumps({"jsonrpc": "2.0", "id": 1, "method": "SendStreamingMessage", "params": {"message": {
        "role": "ROLE_USER", "messageId": str(uuid.uuid4()), "parts": [{"text": "llm:" + prompt}]}}})
    t0 = time.perf_counter()
    c.request("POST", "/", body, {"content-type": "application/json", "accept": "text/event-stream", "A2A-Version": "1.0"})
    r = c.getresponse(); text, ttft, state = "", None, None
    for line in sse_lines(r):
        if not line.startswith("data:"): continue
        res = json.loads(line[5:].strip()).get("result", {})
        if "artifactUpdate" in res:
            for p in res["artifactUpdate"]["artifact"].get("parts", []):
                t = p.get("text", "")
                if t and ttft is None: ttft = time.perf_counter() - t0
                text += t
        for k in ("statusUpdate", "task"):
            if k in res: state = res[k]["status"]["state"]
        if state in ("TASK_STATE_COMPLETED", "TASK_STATE_FAILED"): break
    return {"ttft": ttft, "total": time.perf_counter() - t0, "text": text, "state": state}

rows = []
order = ["direct", "a2a-rs", "a2a-rust"]
for rnd in range(ROUNDS):
    for pi, p in enumerate(PROMPTS):
        k = (rnd * len(PROMPTS) + pi) % 3
        for who in order[k:] + order[:k]:
            r = direct(p) if who == "direct" else via_agent(AGENTS[who], p)
            r.update({"who": who, "round": rnd, "prompt": pi}); rows.append(r)
json.dump(rows, open("llm_rows.json", "w"), indent=1)

ref = {(r["round"], r["prompt"]): r["text"] for r in rows if r["who"] == "direct"}
print("who        n  ttft_med_ms ttft_p90_ms total_med_ms  text==direct  completed")
for who in order:
    rs = [r for r in rows if r["who"] == who]
    tt = sorted(r["ttft"] * 1e3 for r in rs if r["ttft"]); to = sorted(r["total"] * 1e3 for r in rs)
    same = sum(1 for r in rs if r["text"] == ref[(r["round"], r["prompt"])])
    comp = sum(1 for r in rs if r["state"] in ("TASK_STATE_COMPLETED", "n/a"))
    print("%-9s %3d %11.1f %11.1f %12.1f  %5d/%-5d  %d/%d" % (who, len(rs), statistics.median(tt), tt[int(0.9 * (len(tt) - 1))], statistics.median(to), same, len(rs), comp, len(rs)))
# paired overhead vs the direct call in the same (round, prompt) cell
d = {(r["round"], r["prompt"]): r for r in rows if r["who"] == "direct"}
for who in ("a2a-rs", "a2a-rust"):
    ov = sorted((r["ttft"] - d[(r["round"], r["prompt"])]["ttft"]) * 1e3 for r in rows if r["who"] == who)
    ovt = sorted((r["total"] - d[(r["round"], r["prompt"])]["total"]) * 1e3 for r in rows if r["who"] == who)
    print("%-9s paired TTFT overhead ms: median %.2f [min %.2f, max %.2f]; total overhead median %.2f" % (who, statistics.median(ov), ov[0], ov[-1], statistics.median(ovt)))
