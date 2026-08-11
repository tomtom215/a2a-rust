# Rig Agent

A real [rig](https://github.com/0xPlaygrounds/rig) agent served over the
A2A protocol: incoming A2A messages are passed to a
`rig_core::agent::Agent`, and the completion returns as an A2A artifact.
The executor is generic over `rig_core::completion::CompletionModel`, so
swapping providers (Anthropic, Gemini, Ollama, …) only changes the client
construction in `main`.

## Running

```bash
# Hosted OpenAI:
export OPENAI_API_KEY=sk-...
cargo run -p rig-a2a-agent              # defaults to gpt-4o-mini

# Fully local — any OpenAI-compatible server (llama-server, Ollama):
export OPENAI_API_KEY=local             # any non-empty value
export OPENAI_BASE_URL=http://127.0.0.1:11434/v1
RIG_MODEL=qwen3.5:0.8b cargo run -p rig-a2a-agent
```

Set `A2A_BIND_ADDR=127.0.0.1:8080` for a fixed port. The agent serves a
discovery card at `/.well-known/agent-card.json`, supports push-config
CRUD, and passes the TCK 20/20.

## Failure semantics

| Condition | Task state |
|-----------|-----------|
| Completion succeeds | `TASK_STATE_COMPLETED`, artifact `rig-response` |
| Provider unreachable / errors | `TASK_STATE_FAILED` |
| Message has no text part | `TASK_STATE_FAILED` (invalid params) |

Errors surface through the task state — they are never folded into a
"successful" artifact.

See [`examples/rig-agent/README.md`](https://github.com/tomtom215/a2a-rust/blob/main/examples/rig-agent/README.md)
for the full verified walkthrough.
