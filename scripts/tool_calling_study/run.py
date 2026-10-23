# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
"""Tool-calling runs against an OpenAI-compatible server, with no a2a-rust code
in the path: the control for docs/tool-calling-study.md.

The tools, inventory, error texts, preamble and turn bound are
examples/rig-agent's (src/tools.rs, src/agent.rs), copied, so a result here
can be set beside the agent's. Every request names its sampler and seed, and
every turn is recorded whole, so any run can be replayed and re-scored.

Usage:
  run.py --port P --label L --thinking on|off --n N --seed-base S --out FILE
         [--prompts billing,checkout] [--key-order rig|serde]

--key-order serde writes service_status's result in the SDK arm's key order
(uptimeSeconds before version, as serde_json without preserve_order does);
the default is rig-agent's. It exists for the study's key-order follow-up.

Writes FILE as JSON: the server's /props facts (chat-template hash, model
path, build), the sampler, and per run the prompt, seed, every turn's
request seed, finish reason, reasoning length, content and tool calls, the
tool results, and the final text. Scoring is score.py's job.
"""
import argparse
import hashlib
import json
import time
import urllib.error
import urllib.request

PREAMBLE = (
    "You are an operations agent with tools for checking services. "
    "Call the tools yourself to answer; never ask the user to call a tool. "
    "If a tool returns an error that suggests another tool, call that tool, then answer. "
    "Report values exactly as the tools returned them."
)
# examples/rig-agent/src/tools.rs INVENTORY, in its order.
INVENTORY = [
    ("payments-api", "degraded", "4.2.1", 1814400),
    ("checkout", "healthy", "2.8.0", 259200),
    ("search", "healthy", "1.19.3", 7776000),
]
TOOLS = [
    {"type": "function", "function": {
        "name": "list_services",
        "description": "List the name of every service in the inventory. "
                       "Call this first when the caller names a service you do not recognise.",
        "parameters": {"type": "object", "properties": {}, "additionalProperties": False}}},
    {"type": "function", "function": {
        "name": "service_status",
        "description": "Look up one service's current state, deployed version, and uptime in seconds.",
        "parameters": {"type": "object",
                       "properties": {"service": {"type": "string",
                                                  "description": "Exact service name, as returned by list_services."}},
                       "required": ["service"], "additionalProperties": False}}},
]
MAX_TURNS = 6  # examples/rig-agent/src/agent.rs MAX_TURNS
PROMPTS = {
    "billing": "How is the billing service doing?",
    "checkout": "How is the checkout service doing? Give me its version.",
}
# llama.cpp's own defaults (GET /props, build 8466395), named on every
# request so that a server started with other defaults does not change them.
SAMPLER = {"temperature": 0.8, "top_k": 40, "top_p": 0.95, "min_p": 0.05}
# How long one model turn may take, in both arms (sdk_arm.py passes it on).
TURN_TIMEOUT_SECS = 1800


KEY_ORDER = "rig"


def invoke(name, args):
    """examples/rig-agent/src/tools.rs invoke, including its error texts."""
    if name == "list_services":
        return json.dumps([s[0] for s in INVENTORY], separators=(",", ":"))
    if name == "service_status":
        wanted = args.get("service") if isinstance(args, dict) else None
        if not isinstance(wanted, str):
            return "error: 'service_status' needs a string argument 'service'"
        for service, state, version, uptime in INVENTORY:
            if service == wanted:
                # Key order as rig-agent writes it (insertion order), or as
                # the SDK arm's serde_json writes it (sorted).
                if KEY_ORDER == "serde":
                    status = {"service": service, "state": state, "uptimeSeconds": uptime,
                              "version": version}
                else:
                    status = {"service": service, "state": state, "version": version,
                              "uptimeSeconds": uptime}
                return json.dumps(status, separators=(",", ":"))
        return f"error: no service named '{wanted}'; call list_services for the inventory"
    return f"error: no tool named '{name}'"


def post(port, path, body=None):
    data = None if body is None else json.dumps(body).encode()
    req = urllib.request.Request(f"http://127.0.0.1:{port}{path}", data,
                                 {"content-type": "application/json"})
    with urllib.request.urlopen(req, timeout=TURN_TIMEOUT_SECS) as resp:
        return json.load(resp)


def one_run(port, thinking, prompt, seed, max_tokens):
    messages = [{"role": "system", "content": PREAMBLE}, {"role": "user", "content": prompt}]
    turns, trace = [], []
    for turn in range(MAX_TURNS):
        turn_seed = seed * 8 + turn
        # cache_prompt off: with it on, a turn reuses whatever cached prefix
        # an earlier run left in a slot, and the same seed then gave a
        # different run (1 of 4 replays identical, 2026-10-20).
        body = {"messages": messages, "tools": TOOLS, "seed": turn_seed, "max_tokens": max_tokens,
                "cache_prompt": False,
                "chat_template_kwargs": {"enable_thinking": thinking == "on"}, **SAMPLER}
        try:
            choice = post(port, "/v1/chat/completions", body)["choices"][0]
        except urllib.error.HTTPError as e:
            # The server refused its own model's output, e.g. llama.cpp's
            # "Failed to parse tool call arguments as JSON": the run ends
            # there and is the server's failure, recorded as such.
            turns.append({"seed": turn_seed, "http_error": e.code,
                          "body": e.read()[:500].decode("utf-8", "replace")})
            return turns, trace, None
        msg = choice["message"]
        calls = msg.get("tool_calls") or []
        # llama-server draws each call id at random, and the id goes back in
        # the next request's messages, so the same seed gave a different run
        # (1 of 8 replays identical, every turn equal but the ids). Replaced
        # by one fixed by position; the model sees the same id either way.
        for i, call in enumerate(calls):
            call["id"] = f"call-{turn}-{i}"
        turns.append({"seed": turn_seed, "finish_reason": choice.get("finish_reason"),
                      "reasoning_chars": len(msg.get("reasoning_content") or ""),
                      "content": msg.get("content") or "", "tool_calls": calls})
        if not calls:
            return turns, trace, msg.get("content") or ""
        messages.append({"role": "assistant", "content": msg.get("content") or "", "tool_calls": calls})
        for call in calls:
            raw = call["function"]["arguments"]
            try:
                args = json.loads(raw) if isinstance(raw, str) else raw
            except json.JSONDecodeError:
                args = None
            result = invoke(call["function"]["name"], args)
            trace.append({"name": call["function"]["name"], "arguments": args, "result": result})
            messages.append({"role": "tool", "tool_call_id": call.get("id", ""), "content": result})
    return turns, trace, None  # turn bound reached


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--port", type=int, required=True)
    ap.add_argument("--label", required=True)
    ap.add_argument("--thinking", choices=("on", "off"), required=True)
    ap.add_argument("--n", type=int, required=True)
    ap.add_argument("--seed-base", type=int, required=True)
    ap.add_argument("--max-tokens", type=int, default=4096)
    ap.add_argument("--prompts", default="billing,checkout")
    ap.add_argument("--key-order", choices=("rig", "serde"), default="rig")
    ap.add_argument("--out", required=True)
    a = ap.parse_args()
    global KEY_ORDER
    KEY_ORDER = a.key_order
    props = post(a.port, "/props")
    with open(__file__, "rb") as f:
        harness = hashlib.sha256(f.read()).hexdigest()
    out = {
        "label": a.label, "thinking": a.thinking, "n": a.n, "seed_base": a.seed_base,
        "max_tokens": a.max_tokens, "sampler": SAMPLER, "max_turns": MAX_TURNS,
        "key_order": a.key_order,
        "harness_sha256": harness, "turn_timeout_secs": TURN_TIMEOUT_SECS,
        "server": {"model_path": props.get("model_path"), "build_info": props.get("build_info"),
                   "chat_template_sha256": hashlib.sha256(props.get("chat_template", "").encode()).hexdigest()},
        "runs": [],
    }
    for k, kind in enumerate(a.prompts.split(",")):
        for i in range(a.n):
            seed = a.seed_base + 1000 * k + i
            t0 = time.monotonic()
            turns, trace, final = one_run(a.port, a.thinking, PROMPTS[kind], seed, a.max_tokens)
            out["runs"].append({"kind": kind, "seed": seed, "turns": turns, "trace": trace,
                                "final": final, "secs": round(time.monotonic() - t0, 1)})
            with open(a.out, "w") as f:  # rewritten per run: a killed run keeps what it had
                json.dump(out, f, indent=1)


if __name__ == "__main__":
    main()
