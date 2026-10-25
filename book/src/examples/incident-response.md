# Incident-Response Agent Team

Three cooperating A2A agents that triage a production incident on your
laptop — the hands-on answer to *"how is an agent different from a prompt
wrapped around an API call?"* It is the recommended first multi-agent
example; if you are new to A2A, start with the [Hello Agent](./hello-agent.md).

```bash
cargo run -p incident-response          # narrated five-act demo
```

## What it demonstrates

| Capability | Where you see it |
|---|---|
| Multi-turn tasks (`INPUT_REQUIRED`) | A vague alert parks the task with a question; the answer resumes the **same task** (`taskId` + `contextId`) |
| Agent-to-agent delegation | The triage agent calls a log-search agent and a runbook agent over real A2A client calls |
| Deterministic tool agents | The log-search agent has no LLM — an agent is a capability behind the protocol |
| Streaming progress | Repeated `Working` status updates carry progress notes the moment they happen |
| Artifacts | The final `incident-report` artifact combines assessment, log evidence, and runbook guidance |
| Cooperative cancellation | The operator calls off a parked task → `TASK_STATE_CANCELED`. The default `AgentExecutor::cancel` already records `Canceled`; triage overrides it only to drop the alert it parked (`TriageExecutor::cancel`) |
| Degraded output is labelled | An unreachable model or specialist is reported in the output (`[mechanical summary — no model reachable]`, `[verbatim runbook — no model reachable]`, `[log agent error: …]`) and the task still completes; the logs and runbook agents refuse a query naming no known service with `InvalidParams`, and triage parks an alert naming none in `INPUT_REQUIRED` |
| Every A2A method over every binding | Act 4 drives all 11 spec methods over JSON-RPC, HTTP+JSON, gRPC and WebSocket and exits non-zero if any cell never ran |
| Production hardening | Act 5: 16 asserted checks (tenancy, four authentication mechanisms, limits, retries, three task stores, signing, telemetry, HTTPS push, shutdown); `cargo run -p incident-response -- harden` runs it alone |

## Running fully local

The demo works with **no model at all** (labeled mechanical fallbacks keep
the protocol mechanics visible) and shines with a small local model —
verified with llama.cpp's `llama-server` and the Apache-2.0
Qwen3.5-0.8B model (~500 MB) on `:11434`. Ollama works
identically. See
[`examples/incident-response/README.md`](https://github.com/tomtom215/a2a-rust/blob/main/examples/incident-response/README.md)
for the verified walkthrough, the five-act demo transcript, and the
patterns worth stealing (pre-bind for agent cards, `SUBMITTED → WORKING`
before pausing, continuation IDs, cooperative cancel).

A successful demo keeps the agents serving until Ctrl+C, unless
`INCIDENT_EXIT_WHEN_DONE` is set:

```bash
curl http://127.0.0.1:9200/.well-known/agent-card.json
# 21/21 graded, 1 N/A — gated by tck.yml's tck-example-agents job.
cargo run -p a2a-tck -- --url http://127.0.0.1:9200 --binding jsonrpc
```

Each agent also serves gRPC on its port + 10 and WebSocket on its port + 20,
so the demo binds nine fixed ports (9200–9202, 9210–9212, 9220–9222) and does
not start if any is taken. `-- triage|logs|runbook` start one role;
`-- harden` runs Act 5 alone. `INCIDENT_MODEL` picks the model (default
`qwen3.5:0.8b`). Exit codes: `1` a surface call failed, `2` a method × binding
cell never ran, `3` a hardening check failed, `4` a capability went
unexercised while `INCIDENT_REQUIRE_ALL` was set.
