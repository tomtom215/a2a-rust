<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Rig Agent — A2A Protocol Bridge for rig

A real [rig](https://github.com/0xPlaygrounds/rig) agent served over the A2A
protocol. Incoming A2A messages are passed to `RigAgent`, which this example
defines over `rig_core::completion::CompletionModel` (OpenAI-compatible
provider), runs a **tool loop**, and returns the answer as an A2A artifact.
The executor is generic over that same `CompletionModel`, so swapping
providers (Anthropic, Gemini, Ollama, …) only changes the client construction
in `main` — the A2A bridge is untouched.

## Architecture

```
A2A Client ──→ A2A Server (JSON-RPC)
                    │
                    ▼
              RigAgentExecutor<M>          one A2A task
                    │
                    ▼
              RigAgent<M>
                    │  ▲                   many model turns
                    ▼  │
              tools::invoke ──→ in-memory service inventory
                    │
                    ▼
              LLM provider
```

## Tool calling, and where it sits relative to A2A

**A2A has no tool concept.** It carries messages, tasks and artifacts
*between* agents; what an agent does inside one task is its own business. So
tool calling lives one layer down, between `RigAgent` and its model, and the
A2A server never sees it — `src/tools.rs` imports no `a2a-protocol-*` type at
all. Many model turns, one A2A task.

That is the layering to copy. An A2A server does not become a tool runtime;
it hosts one.

The catalogue is two tools, in `src/tools.rs`, over a hard-coded inventory of
three services:

| Tool | Arguments | Returns |
|---|---|---|
| `list_services` | none | every service name |
| `service_status` | `{"service": "<name>"}` | state, version, uptime |

Two, because between them they cover both JSON Schema shapes a provider has
to encode, and because they *compose*: a model asked about a service it does
not recognise has to discover the inventory before it can query it, which is
a two-round loop. A single-tool catalogue only ever demonstrates one round,
and one round is the case that still works when the loop is written wrong.

Three things the loop in `src/agent.rs` gets right, each of which is a bug if
you copy the shape without them:

1. **The catalogue goes on every request**, not just the first. A provider
   holds no state between turns.
2. **A tool error is a result, not a failure.** `tools::invoke` errors are
   handed back to the model as that call's result. An unknown service is
   something the model recovers from; a failed A2A task is not.
3. **The loop is bounded** (`agent::MAX_TURNS`, 6). Unbounded, a model that
   calls tools forever holds the A2A task open until the server's executor
   timeout — an hour by default.

When tools ran, the task carries a second `tool-trace` artifact naming each
call and its result, so a caller can tell a researched answer from a guessed
one.

## Running against hosted OpenAI

```bash
export OPENAI_API_KEY=sk-...
RIG_MODEL=gpt-4o-mini cargo run -p rig-a2a-agent   # RIG_MODEL defaults to qwen3:1.7b
RIG_MODEL=gpt-4o cargo run -p rig-a2a-agent
```

## Running fully local — no API key

Any OpenAI-compatible server works via rig's `OPENAI_BASE_URL` support.

### Pick a model that can actually call tools

**Measured, not assumed.** Both runs below used the same
`llama-server` build (llama.cpp `b23701f`, `--jinja`) on 2026-09-19, with the
catalogue above:

| Model | Result |
|---|---|
| Qwen3.5-0.8B-Q4_0 | **Cannot.** Answered in prose asking for the service name. Forced with `tool_choice: "required"` it still emitted no tool call, and ran to `finish_reason: "length"` after 7,400+ tokens. |
| Qwen3-1.7B-Q4_K_M | **Can.** First request came back `finish_reason: "tool_calls"` with `service_status({"service": "checkout"})`. |

So the walkthrough below uses **Qwen3-1.7B** (Apache-2.0, ~1.2 GB). 0.8B is
still fine for the plain-completion path, and the conformance run needs no
model at all — but it will not demonstrate a single tool call, which is easy
to mistake for a broken loop.

`--jinja` is load-bearing: without it `llama-server` does not apply the
model's chat template, and no model emits tool calls no matter how capable.

A verified walkthrough with [llama.cpp](https://github.com/ggml-org/llama.cpp)'s
`llama-server`:

```bash
# 1. Build llama-server, and fetch the model
#
# From source rather than a release asset: llama.cpp's prebuilt tarballs carry
# the release tag in the filename, so there is no stable
# `releases/latest/download/<name>` URL to give you. An earlier revision of
# this file offered one and it 404s. Building takes about three minutes on
# four cores and needs only cmake and a C++ compiler.
git clone --depth 1 https://github.com/ggml-org/llama.cpp
cmake -S llama.cpp -B llama.cpp/build -DLLAMA_CURL=OFF
cmake --build llama.cpp/build --target llama-server -j"$(nproc)"

curl -L -o model.gguf \
  'https://huggingface.co/ggml-org/Qwen3-1.7B-GGUF/resolve/main/Qwen3-1.7B-Q4_K_M.gguf'

# 2. Serve it (OpenAI-compatible API on :11434)
#
# --jinja applies the model's chat template, which is what carries the
# tool-call format. Without it the model never emits one.
./llama.cpp/build/bin/llama-server -m model.gguf --port 11434 --alias qwen3:1.7b \
  --jinja \
  --chat-template-kwargs '{"enable_thinking":false}' &   # direct answers, no thinking preamble

# 3. Point the rig agent at it
export OPENAI_API_KEY=local              # any non-empty value
export OPENAI_BASE_URL=http://127.0.0.1:11434/v1
RIG_MODEL=qwen3:1.7b A2A_BIND_ADDR=127.0.0.1:8080 cargo run -p rig-a2a-agent
```

### What a tool-calling turn looks like

Both transcripts below are verbatim from the run described above — a debug
build, Qwen3-1.7B on four CPU cores, ~5 s each. The *prose* varies between
runs, as any model's does; the artifacts and the trace lines do not.

One round, because the service exists:

```console
$ curl -sS -X POST http://127.0.0.1:8080 -H 'Content-Type: application/json' \
    -H 'A2A-Version: 1.0' -d '{"jsonrpc":"2.0","id":1,"method":"SendMessage",
      "params":{"message":{"messageId":"m1","role":"ROLE_USER",
      "parts":[{"text":"How is the checkout service doing? Give me its version."}]}}}'

state: TASK_STATE_COMPLETED
[ rig-response ]
The checkout service is in a healthy state. Its current version is 2.8.0, and
it has been running for 7200 seconds (2 hours).
[ tool-trace ]
service_status({"service":"checkout"}) -> {"service":"checkout","state":"healthy","uptimeSeconds":259200,"version":"2.8.0"}
```

**Read those two artifacts against each other.** The tool returned
`uptimeSeconds: 259200` — three days. The model reported 7200 seconds, a
number it invented, and converted it correctly to a wrong answer. The version
and the state it took from the tool are right; the arithmetic is not.

That is not a defect in this example, and it is left in rather than cropped
out: it is what a 1.7B model does, and it is exactly why the trace artifact
exists. Without it the caller sees one confident paragraph and has no way to
check it. With it, the discrepancy is one line away. A grounded answer and a
verifiable one are different properties, and only the second is something the
protocol can carry.

Two rounds, because the service does not exist — the tool error is handed
back and the model recovers from it:

```console
$ ... "parts":[{"text":"How is the billing service doing?"}] ...

state: TASK_STATE_COMPLETED
[ rig-response ]
The services in the inventory are: payments-api, checkout, search.

Which service would you like to check the status of?
[ tool-trace ]
service_status({"service":"billing"}) -> error: no service named 'billing'; call list_services for the inventory
list_services({}) -> ["payments-api","checkout","search"]
```

This one is why the error text names the recovery tool: the model read the
hint and followed it.

### vLLM

vLLM serves the same OpenAI-compatible API, from the unquantized weights
rather than a GGUF. Use it when you want throughput or a GPU; llama.cpp is the
lighter option for a laptop CPU.

```bash
pip install vllm
vllm serve Qwen/Qwen3-1.7B --port 11434 --served-model-name qwen3:1.7b
```

`--served-model-name` matters: the examples send the model name in the request
body, and vLLM rejects a name it was not started with. Setting it to the same
string llama.cpp's `--alias` uses means the same env var works against either
server, which is the point of both being OpenAI-compatible.

(Ollama works identically — it already listens on `:11434`.)

## Talking to the agent

```bash
# Agent discovery
curl http://127.0.0.1:<port>/.well-known/agent-card.json

# Send a message (JSON-RPC binding)
curl -X POST http://127.0.0.1:<port> -H 'Content-Type: application/json' -H 'A2A-Version: 1.0' -d '{
  "jsonrpc": "2.0", "id": 1, "method": "SendMessage",
  "params": {"message": {"messageId": "m1", "role": "ROLE_USER",
             "parts": [{"text": "What is the capital of France?"}]}}
}'

# Full conformance suite — 21/21 graded, 1 N/A on this binding, gated by
# tck.yml's tck-example-agents job. The command below reproduces it locally.
cargo run -p a2a-tck -- --url http://127.0.0.1:<port> --binding jsonrpc
```

Set `A2A_BIND_ADDR=127.0.0.1:8080` for a fixed port instead of a random one.

## Failure semantics

| Condition | Task state |
|-----------|-----------|
| Answer, no tools called | `TASK_STATE_COMPLETED`, artifact `rig-response` |
| Answer after tool calls | `TASK_STATE_COMPLETED`, artifacts `rig-response` **and** `tool-trace` |
| A tool errored | **not a task failure** — the error goes back to the model as that call's result, and the trace records it |
| Model never stops calling tools | `TASK_STATE_FAILED` after `MAX_TURNS` |
| Provider unreachable / errors | `TASK_STATE_FAILED` |
| Message has no text part | `TASK_STATE_FAILED` (invalid params) |

Errors are never folded into a "successful" artifact — clients can trust the
task state. The one deliberate exception is a *tool* error, which is not an
agent failure: it is information the model is expected to act on, and the
`tool-trace` artifact is where a caller sees that it happened.

## License

Apache-2.0
