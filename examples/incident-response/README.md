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

Two further acts answer the questions that follow from that one — *does this
SDK serve the whole protocol?* and *can I deploy it?*

| | Where this demo shows it |
|---|---|
| Every A2A method over every binding | Act 4 drives all 11 spec methods across JSON-RPC, HTTP+JSON, gRPC and WebSocket and **exits non-zero if any cell never ran**. The method list is derived from the ratified `a2a.proto`, not from a list this repository maintains |
| Calls that must be *refused* | Act 4's counter-tests: push notifications, streaming and the extended card against an agent advertising none of them |
| Multi-tenancy, five kinds of auth and limits, three task stores, signing, telemetry, retries, HTTPS push and shutdown | Act 5's **16 checks** — see [Act 5](#act-5--production-hardening) |

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
cargo run -p incident-response          # narrated five-act demo
cargo run -p incident-response -- harden   # Act 5 on its own
```

Exit codes: `1` a surface call failed, `2` a method × binding cell never ran,
`3` a hardening check failed, `4` a capability went unexercised while
`INCIDENT_REQUIRE_ALL` was set.

The demo works with **no model at all** (agents label their output as
mechanical/verbatim fallbacks so the protocol mechanics stay visible), and
shines with a small local model — verified with llama.cpp's `llama-server`
and the Apache-2.0 Qwen3.5-0.8B model (~500 MB):

```bash
curl -L -o model.gguf \
  'https://huggingface.co/ggml-org/Qwen3.5-0.8B-GGUF/resolve/main/Qwen3.5-0.8B-Q4_0.gguf'
llama-server -m model.gguf --port 11434 --alias qwen3.5:0.8b \
  --chat-template-kwargs '{"enable_thinking":false}' &   # direct answers, no thinking preamble
cargo run -p incident-response          # INCIDENT_MODEL defaults to qwen3.5:0.8b
```

### vLLM

vLLM serves the same OpenAI-compatible API, from the unquantized weights
rather than a GGUF. Use it when you want throughput or a GPU; llama.cpp is the
lighter option for a laptop CPU.

```bash
pip install vllm
vllm serve Qwen/Qwen3.5-0.8B --port 11434 --served-model-name qwen3.5:0.8b
```

`--served-model-name` matters: the examples send the model name in the request
body, and vLLM rejects a name it was not started with. Setting it to the same
string llama.cpp's `--alias` uses means the same env var works against either
server, which is the point of both being OpenAI-compatible.

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

ACT 4 — every A2A method over every binding, counted
  --- JSON-RPC ---            (11/11) … and the same for HTTP+JSON, gRPC, WebSocket
  --- counter-tests (calls that must be refused) ---

ACT 5 — production hardening: tenancy, auth, limits, durability, telemetry
  [ok] Tenant isolation …     2 tenants isolated; a cross-tenant params.tenant was refused
  [ok] Client retry …         2 x 503 ridden out; 502 on a non-idempotent method not retried
  [ok] OTLP pipeline …        2150 bytes of HTTP/2 exported to the configured collector
  …
  16 passed, 0 failed, 0 not compiled, 0 not run
```

The incident report combines the LLM's assessment with the raw log
evidence and runbook guidance — every claim in it is traceable to an
agent that produced it.

## Act 5 — production hardening

Acts 1–4 cover what an agent is and what the protocol requires. Neither asks
the question an operator asks first: *what happens when this is exposed to more
than one caller?* Act 5 exercises the SDK capabilities that answer it, over a
real socket, and **asserts** each one rather than narrating it:

| Capability | The wrong answer it rules out |
|---|---|
| `TenantAwareInMemoryTaskStore` + `HeaderTenantResolver` | One tenant reading another's tasks — including by *naming* the other tenant in `params.tenant`, which is the v0.6.0 regression this check was written for |
| `BearerTokenAuthInterceptor` (+ the client's `AuthInterceptor`) | An unauthenticated request succeeding — and, in the other direction, a server that refuses everything |
| `ApiKeyAuthInterceptor` | An interceptor that rejects on the header being *present* rather than on its value |
| `JwtAuthInterceptor` against a served JWKS | A forged ES256 signature verifying, or an expired token still working |
| `RateLimitInterceptor` | A limit that accepts everything, or one that accepts nothing |
| `HandlerLimits` | An unbounded caller-supplied `context_id` the server allocates |
| `RetryPolicy` (client) | A retry layer that retries a `502` on `SendMessage` — an ambiguous failure a gateway may already have forwarded, so the work can run twice |
| `sign_agent_card` / `verify_agent_card` | A rewritten interface URL still verifying, which would let a signed card redirect callers to an impostor |
| `SqliteTaskStore` | A task that does not survive being read back through a *different* handler over the same file |
| `TenantAwareSqliteTaskStore` | Partitions that look right in memory and share one table on disk, leaking the moment the process restarts |
| `PostgresTaskStore` | A second SQL backend assumed correct because the first one is |
| `RequestHandler::shutdown` | A shutdown that hangs instead of draining |
| The `Metrics` hook | Served requests that reach no recorder |
| `OtelMetrics` | An instrumented handler that exports no `a2a.server.requests` datapoint — checked by collecting from a real `ManualReader`, because the default global meter provider is a no-op and would make any wiring look correct |
| `init_otlp_pipeline` | A pipeline that builds cleanly and never reaches the collector — checked by pointing it at a socket this example owns and requiring HTTP/2 bytes |
| `HttpPushSender::with_tls_config` | A sender that speaks HTTPS without verifying certificates — the same self-signed cert must be accepted with a trust store naming it and rejected with the default one |

Every check names the specific wrong answer in its failure message, and every
one was proven able to fail by removing the capability it covers.
`scripts/prove_gates_fail.sh` injects the tenant-resolver removal to keep the
act's CI step honest.

**Nothing here passes by not running.** A capability compiled out prints
`[NOT BUILT]` with the feature it needs; one whose external service is absent
prints `[NOT RUN]` with what to set. Only PostgreSQL is in the second category
— set `A2A_TEST_POSTGRES_URL` to exercise it:

```bash
A2A_TEST_POSTGRES_URL=postgres://user:pass@localhost:5432/db \
  cargo run -p incident-response -- harden
```

Set `INCIDENT_REQUIRE_ALL=1` (as CI does) to make either state exit `4` instead
of passing quietly, so a service that stops being provisioned fails the run
rather than downgrading a check to a printed line.

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
- **Resolve the tenant server-side**: a `TenantResolver` derives it from
  trusted request context, and a client that names a different one is
  rejected. Trusting `params.tenant` alone is only safe behind a gateway that
  has already authenticated the caller.
- **Demonstrations should assert**: a demo that only prints what happened
  cannot fail, so it tells a reader nothing. Every Act 4 and Act 5 check names
  the specific wrong answer it rules out, and the process exits non-zero when
  it sees one.

## License

Apache-2.0
