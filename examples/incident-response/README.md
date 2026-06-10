<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Incident-Response Agent Team

Three cooperating A2A agents that triage a production incident on your
laptop — the hands-on answer to *"how is an agent different from a prompt
wrapped around an API call?"*

A wrapped prompt answers once and forgets. An agent holds a **task**:

| What a task can do | Where this demo shows it |
|---|---|
| Pause and ask the caller for missing input, then **resume the same task** | Act 1 → Act 2: a vague alert parks in `TASK_STATE_INPUT_REQUIRED`; the operator's answer (same `taskId` + `contextId`) resumes it |
| Gather evidence with **tools** | The triage agent queries a deterministic log-search agent — no LLM, exact semantics |
| **Delegate to other agents** over the same protocol it serves | Triage → logs agent and runbook agent are real A2A client calls; swap any of them for an agent in another language or from another vendor |
| **Stream progress** while it works | `message/stream` delivers `Working` status updates ("querying log agent", "synthesizing incident report") the moment they happen |
| Produce **artifacts**, not just chat | The final `incident-report` artifact contains the assessment, the log evidence, and the runbook guidance |
| Be **cancelled** | Act 3: the operator calls off a parked task → `TASK_STATE_CANCELED`. Cancellation is cooperative — see `TriageExecutor::cancel` |
| End in an **honest terminal state** | LLM/provider failures and textless messages end in `TASK_STATE_FAILED`, never a fake success |

## The team

```
                 ┌──────────────────────────┐
 operator ──A2A──►  triage  :9200  (LLM)    │  orchestrates, asks, reports
                 └────┬────────────┬────────┘
                 A2A  │            │  A2A
        ┌─────────────▼──┐   ┌─────▼──────────────┐
        │ logs  :9201    │   │ runbook  :9202     │
        │ deterministic  │   │ LLM-summarized,    │
        │ log search     │   │ verbatim fallback  │
        └────────────────┘   └────────────────────┘
```

The logs agent is deliberately **not** LLM-backed: an A2A agent is a
capability behind the protocol. Tools with exact semantics stay
deterministic; the judgment lives in the agents that call them.

## Run it

```bash
cargo run -p incident-response          # narrated three-act demo
```

The demo works with **no model at all** (agents label their output as
mechanical/verbatim fallbacks so the protocol mechanics stay visible), and
shines with a small local model — verified with llama.cpp's `llama-server`
and the Apache-2.0 Qwen2.5-0.5B-Instruct model (~470 MB):

```bash
curl -L -o model.gguf \
  'https://huggingface.co/Qwen/Qwen2.5-0.5B-Instruct-GGUF/resolve/main/qwen2.5-0.5b-instruct-q4_k_m.gguf'
llama-server -m model.gguf --port 11434 --alias qwen2.5-0.5b-instruct &
cargo run -p incident-response          # INCIDENT_MODEL defaults to qwen2.5-0.5b-instruct
```

(Ollama on `:11434` works identically; hosted providers work by setting
`INCIDENT_MODEL` to e.g. `gpt-4o-mini` plus the provider's API key.)

The agents keep serving after the demo — probe them:

```bash
curl http://127.0.0.1:9200/.well-known/agent-card.json
cargo run -p a2a-tck -- --url http://127.0.0.1:9200 --binding jsonrpc   # 20/20
```

Or run each role as its own process (that's the point of a wire protocol):

```bash
cargo run -p incident-response -- logs     &   # :9201
cargo run -p incident-response -- runbook  &   # :9202
cargo run -p incident-response -- triage       # :9200
```

## What the demo prints

```
ACT 1 — vague alert: the task pauses and asks for missing input
  ⇢ task 8444... [TASK_STATE_SUBMITTED]
  ⇢ status: TASK_STATE_WORKING — reading alert
  ⇢ status: TASK_STATE_INPUT_REQUIRED — Which service is affected? I have runbooks for: payments-api, checkout, search

ACT 2 — the operator answers on the same task; the agents collaborate
  ⇢ status: TASK_STATE_WORKING — triaging 'payments-api': querying log agent
  ⇢ status: TASK_STATE_WORKING — querying runbook agent
  ⇢ status: TASK_STATE_WORKING — synthesizing incident report
  ⇢ artifact 'incident-report' (2963 chars)
  ⇢ status: TASK_STATE_COMPLETED

ACT 3 — tasks are cancellable: the operator calls off a parked task
  → cancel_task(...) ⇒ TASK_STATE_CANCELED
```

The incident report combines the LLM's assessment with the raw log
evidence and runbook guidance — every claim in it is traceable to an
agent that produced it.

## Patterns worth stealing

- **Pre-bind, then build**: bind the listener first so the agent card can
  advertise the real address.
- **`SUBMITTED → WORKING` before any interrupted state**: the A2A state
  machine requires announcing work before pausing for input.
- **Continuations need `taskId` + `contextId`**: both ride on the follow-up
  message.
- **Status messages narrate; artifacts deliver**: repeated `Working`
  updates carry progress notes, the artifact carries the deliverable.
- **Cooperative cancellation**: override `AgentExecutor::cancel` to release
  task state; the default reports tasks as not cancelable.
- **Degrade loudly**: when a specialist or the model is unreachable, the
  output says so (`[log agent unavailable: …]`, `[verbatim runbook — no
  model reachable]`) instead of pretending.

## License

Apache-2.0
