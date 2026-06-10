<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Rig Agent — A2A Protocol Bridge for rig

A real [rig](https://github.com/0xPlaygrounds/rig) agent served over the A2A
protocol. Incoming A2A messages are passed to a `rig_core::agent::Agent`
(OpenAI-compatible provider), and the completion is returned as an A2A
artifact. The executor is generic over `rig_core::completion::CompletionModel`,
so swapping providers (Anthropic, Gemini, Ollama, …) only changes the client
construction in `main` — the A2A bridge is untouched.

## Architecture

```
A2A Client ──→ A2A Server (JSON-RPC)
                    │
                    ▼
              RigAgentExecutor<M>
                    │
                    ▼
              rig_core::agent::Agent<M> ──→ LLM provider
```

## Running against hosted OpenAI

```bash
export OPENAI_API_KEY=sk-...
cargo run -p rig-a2a-agent              # defaults to gpt-4o-mini
RIG_MODEL=gpt-4o cargo run -p rig-a2a-agent
```

## Running fully local — no API key

Any OpenAI-compatible server works via rig's `OPENAI_BASE_URL` support. A
verified walkthrough with [llama.cpp](https://github.com/ggml-org/llama.cpp)'s
`llama-server` and the Apache-2.0 Qwen2.5-0.5B-Instruct model (~470 MB):

```bash
# 1. Get a prebuilt llama-server (pick the latest release tag) and a model
curl -L -o llama.tar.gz \
  'https://github.com/ggml-org/llama.cpp/releases/latest/download/llama-bin-ubuntu-x64.tar.gz' \
  || echo 'grab the llama-<tag>-bin-<os>.tar.gz asset for your platform'
tar xzf llama.tar.gz
curl -L -o model.gguf \
  'https://huggingface.co/Qwen/Qwen2.5-0.5B-Instruct-GGUF/resolve/main/qwen2.5-0.5b-instruct-q4_k_m.gguf'

# 2. Serve it (OpenAI-compatible API on :11434)
./llama-*/llama-server -m model.gguf --port 11434 --alias qwen2.5-0.5b-instruct &

# 3. Point the rig agent at it
export OPENAI_API_KEY=local              # any non-empty value
export OPENAI_BASE_URL=http://127.0.0.1:11434/v1
RIG_MODEL=qwen2.5-0.5b-instruct cargo run -p rig-a2a-agent
```

(Ollama works identically — it already listens on `:11434`.)

## Talking to the agent

```bash
# Agent discovery
curl http://127.0.0.1:<port>/.well-known/agent-card.json

# Send a message (JSON-RPC binding)
curl -X POST http://127.0.0.1:<port> -H 'Content-Type: application/json' -d '{
  "jsonrpc": "2.0", "id": 1, "method": "message/send",
  "params": {"message": {"messageId": "m1", "role": "ROLE_USER",
             "parts": [{"text": "What is the capital of France?"}]}}
}'

# Full conformance suite (passes 20/20 against this agent)
cargo run -p a2a-tck -- --url http://127.0.0.1:<port> --binding jsonrpc
```

Set `A2A_BIND_ADDR=127.0.0.1:8080` for a fixed port instead of a random one.

## Failure semantics

| Condition | Task state |
|-----------|-----------|
| Completion succeeds | `TASK_STATE_COMPLETED`, artifact `rig-response` |
| Provider unreachable / errors | `TASK_STATE_FAILED` |
| Message has no text part | `TASK_STATE_FAILED` (invalid params) |

Errors are never folded into a "successful" artifact — clients can trust the
task state.

## License

Apache-2.0
