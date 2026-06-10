# Incident-Response Agent Team

Three cooperating A2A agents that triage a production incident on your
laptop — the hands-on answer to *"how is an agent different from a prompt
wrapped around an API call?"* This is the recommended first example.

```bash
cargo run -p incident-response          # narrated three-act demo
```

## What it demonstrates

| Capability | Where you see it |
|---|---|
| Multi-turn tasks (`INPUT_REQUIRED`) | A vague alert parks the task with a question; the answer resumes the **same task** (`taskId` + `contextId`) |
| Agent-to-agent delegation | The triage agent calls a log-search agent and a runbook agent over real A2A client calls |
| Deterministic tool agents | The log-search agent has no LLM — an agent is a capability behind the protocol |
| Streaming progress | Repeated `Working` status updates carry progress notes the moment they happen |
| Artifacts | The final `incident-report` artifact combines assessment, log evidence, and runbook guidance |
| Cooperative cancellation | The operator calls off a parked task → `TASK_STATE_CANCELED` (see the `AgentExecutor::cancel` override) |
| Honest failure states | Provider errors and textless messages end in `TASK_STATE_FAILED`, never a fake success |

## Running fully local

The demo works with **no model at all** (labeled mechanical fallbacks keep
the protocol mechanics visible) and shines with a small local model —
verified with llama.cpp's `llama-server` and the Apache-2.0
Qwen2.5-0.5B-Instruct model (~470 MB) on `:11434`. Ollama works
identically. See
[`examples/incident-response/README.md`](https://github.com/tomtom215/a2a-rust/blob/main/examples/incident-response/README.md)
for the verified walkthrough, the three-act demo transcript, and the
patterns worth stealing (pre-bind for agent cards, `SUBMITTED → WORKING`
before pausing, continuation IDs, cooperative cancel).

The agents keep serving after the demo:

```bash
curl http://127.0.0.1:9200/.well-known/agent-card.json
cargo run -p a2a-tck -- --url http://127.0.0.1:9200 --binding jsonrpc   # 20/20
```
