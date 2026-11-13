<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Running the loop

`ToolLoop` holds the model, the tools and the loop's bounds; `ToolLoopExecutor`
runs it as an A2A task. This page covers the builder, what the executor sends
the client, running the loop outside an executor, the bounds a run is held
to, and how a run that produces no answer fails.

> **Cargo feature.** Every block on this page needs the `tool-loop` feature: `a2a-protocol-sdk = { version = "0.16", features = ["tool-loop"] }`, or `a2a-protocol-server = { version = "0.16", features = ["tool-loop"] }` on its own.

## The builder

```rust,no_run
# use a2a_protocol_sdk::server::tool_loop::{OpenAiChatModel, ToolLoop, ToolSet};
# fn main() -> Result<(), Box<dyn std::error::Error>> {
# let model = OpenAiChatModel::new("http://127.0.0.1:8080/v1", "qwen3:1.7b")?;
# let tools = ToolSet::new();
let tool_loop = ToolLoop::new(model, tools)
    .with_preamble("You are an operations agent. Call the tools yourself to answer.")
    .with_max_turns(6)
    .with_agent_name("ops-agent");
# Ok(())
# }
```

| Method | Default | What it is for |
|---|---|---|
| `ToolLoop::new(model, tools)` | — | A loop over a `ToolModel` and a `ToolSet` |
| `with_preamble(text)` | none | The system preamble every turn is sent with |
| `with_max_turns(n)` | 8 (`DEFAULT_MAX_TURNS`) | How many model turns a run may take. `0` is taken as `1`: a run asks the model at least once |
| `with_max_calls_per_turn(n)` | 64 (`DEFAULT_MAX_CALLS_PER_TURN`) | How many tool calls one model reply may make. The first `n` run; the rest are dropped, with a line in the trace saying so. `0` is taken as `1` |
| `with_recover_text_calls(bool)` | on | Whether a final reply that is exactly one JSON object naming a tool in the set (bare, in a code fence, or between `<tool_call>` tags) is run as that call rather than returned as the answer |
| `with_text_call_recovery(TextCallRecovery)` | `All` | Which tools a reply written as a call may run: `All` (as `with_recover_text_calls(true)`), `ReadOnly` (only a tool that declares `read_only()`), `Off` (never: a requested JSON example stays an example) |
| `with_trace_content(TraceContent)` | `Full` | What the caller's `tool-trace` artifact holds: `Full` (`name(arguments) -> result`, truncated, **not redacted**) or `Receipt` (`name [call id]: ran`, `failed` or `not run again`: no arguments, no result; the model still sees both) |
| `with_argument_check(ArgumentCheck)` | `Off` | Whether a call's arguments are checked against the tool's JSON Schema before the policy is asked and the tool runs: `Off` (an object is enough, as before), `Checked` (a violation is answered to the model as an error it can correct, and the tool does not run; a keyword the loop does not implement is ignored) or `Strict` (as `Checked`, and a tool whose schema uses a keyword the loop cannot check is refused by name). Checked: `type`, `enum`, `const`, `properties`, `required`, `additionalProperties`, `items`, `minItems`, `maxItems`, `uniqueItems`, `minLength`, `maxLength`, `minimum`, `maximum`, `exclusiveMinimum`, `exclusiveMaximum`, `minProperties`, `maxProperties`, `anyOf`, `oneOf`, `allOf`, `not`. Not implemented: `$ref`, `pattern`, `patternProperties`, `multipleOf`, `if`/`then`/`else`, `contains`, `prefixItems`, `propertyNames`, `dependentRequired`, `unevaluated*`, a list-form `items`. A tool that needs one of those validates it itself |
| `with_policy(ToolPolicy)` | none | Asked before **every** call runs (native, recovered from text, repeated after a retry) with the tool, the arguments and the request's tenant and authenticated caller, and able to refuse it; the reason is the call's result as the model sees it. With the `audit` feature, `AuditedPolicy::new(log)` (optionally `.wrapping(your_policy)`) writes a `tool.action` record (tool, call id, verdict, a digest of the arguments, caller, task) to the audit log *before* an allowed call runs and does not run a call it cannot record ([Audit trail](../../deployment/audit.md#required-mode)) |
| `with_journal(Arc<dyn StepJournal>)` | none | Keeps each run's progress, so a run begun again under the same key (the task and message it answers) **resumes** instead of starting over, and a call a policy says needs approval can wait for it ([Resuming a run, and approving a call](#resuming-a-run-and-approving-a-call)) |
| `with_pin(name, value)` | none | Adds a pin to every checkpoint (a policy version, a skill bundle digest): a run whose checkpoint carries other pins is refused on resume |
| `with_idempotency_secret([u8; 32])` | random per loop | The secret call keys are derived under. Set it, to the same value on every replica, if a journaled call may be in flight when a replica dies |
| `ToolLoop::strict(model, tools)` | — | `new` with `TextCallRecovery::Off`, `TraceContent::Receipt` and `ArgumentCheck::Strict`: for a deployment that takes a tool call to be an action with consequences |
| `with_idempotency_secret([u8; 32])` | drawn per loop | The secret call keys are derived under; see [Idempotency keys](./tools.md#idempotency-keys) |
| `with_agent_name(name)` | none | Recorded as `gen_ai.agent.name` on the run's metrics |
| `with_meter(&Meter)` | the global meter provider | Where the loop's metrics go (`otel` feature); see [Spans and metrics](./telemetry.md) |

The preamble matters more than it looks. Without one, small models tell the
caller to call a tool rather than call it, and restate a tool's numbers from
memory. The [Tool-Loop Agent](../../examples/tool-loop-agent.md) example's
preamble tells the model to call the tools itself, to follow a tool error that
suggests another tool, and to report values exactly as the tools returned
them.

The default of eight turns is four more than the two rounds a
discover-then-query catalogue needs, and small enough that a model which never
stops calling tools fails in seconds rather than at the executor timeout.

## Resuming a run, and approving a call

A run's conversation, its record of calls already answered and its trace live in
memory. A replica that dies mid-run loses them, and the next attempt asks the
model afresh and runs every tool it chooses again. With a journal installed that
changes:

```rust,no_run
use std::sync::Arc;
use a2a_protocol_server::tool_loop::{InMemoryStepJournal, ToolLoop, ToolModel, ToolSet};
# fn build(model: impl ToolModel, tools: ToolSet) -> ToolLoop {
ToolLoop::new(model, tools)
    .with_journal(Arc::new(InMemoryStepJournal::new()))
    // The same on every replica, so a retried call has the same key.
    .with_idempotency_secret([7; 32])
    // What a resumed run must still be running under.
    .with_pin("policy", "2026-11-12")
# }
```

`InMemoryStepJournal` is for tests and for a deployment that keeps a run on one
replica. `SqliteStepJournal` (`sqlite`) and `PostgresStepJournal` (`postgres`)
keep checkpoints across a restart, the second for every replica sharing the
database.

* **Resume points.** A checkpoint is written after each model reply, before each
  call and after each call. A run begun again under the same key restores the
  conversation, does not ask the model for what it answered, and does not run a
  call it already ran.
* **Pins.** A checkpoint records the model's name and digests of the preamble, the
  tool set (names, descriptions, schemas, read-only flags) and the loop's modes,
  plus your `with_pin` labels. A run resumed under different pins fails with
  `LoopError::PinMismatch`, naming which, rather than continuing under a model or a
  tool set it did not begin with.
* **Receipts, written first.** A call's receipt (tool, call id, a digest of the
  arguments, never the arguments) is written *before* the call runs. A call whose
  receipt still says `Started` on resume may have run. It is re-run only when the
  idempotency secret was set explicitly, so its key is the same on every replica and
  your backend can deduplicate; otherwise the run fails with
  `LoopError::Indeterminate`. **The journal makes a repeat rare and visible. It does
  not make an external effect exactly-once: that is the backend's idempotency.**
* **Fail closed.** A checkpoint the journal cannot write stops the run before the
  call it was about to record (`LoopError::Journal`).
* **Fencing.** Each checkpoint carries a revision and a write is accepted only as the
  successor of the one stored, so a replica that lost its claim cannot overwrite a
  newer run's progress.

Not here: compensation (undoing a call that ran), and retry policy beyond the model
retry.

### A call that needs a person's approval

A `ToolPolicy` that returns `PolicyDecision::RequireApproval("what the person is
shown")` stops that call. The run parks in its journal, and the task goes
`input-required` with an `ApprovalRequest` whose digest is `call_digest(tool,
arguments, tenant, requester, task)`: the call as an action, in canonical form. The
decision that releases it is bound to that digest. It releases that call, from that
requester, in that tenant and task, and not the same call from another requester nor
another call by the same one.

```text
alice asks            -> policy: RequireApproval -> task input-required, digest D
bob decides (digest D)-> the gate checks id, digest and that bob may approve
                      -> the run resumes from the journal, makes the call it
                         parked on (read from the journal, not from the decision),
                         runs the policy again, and answers
```

The loop checks the digest again itself (it does not rely on the gate alone), refuses
a decision from another tenant or task, and asks the policy again on resume: an
approval does not outrank a later `Deny`. A refusal is a result the model sees ("this
call was not approved by …") and the call never runs. A loop with no journal has
nowhere to wait: it refuses such a call, as for `Deny`.

Once the digest matches and the policy has been asked again, the loop tells the policy
who approved, through `ToolPolicy::approved(request, approver)`, immediately before the
call runs; an `Err` there stops it. It is the only point that sees the effective
decision, so a policy that must record or check an approved action does it there.
`AuditedPolicy` writes its durable `approved` record in it.

It needs an `ApprovalGate` on the handler (`with_approval_gate`): without one, a
decision in a message is only a claim and `ctx.approval()` is always `None`. With tasks
isolated by caller, give the approvers a review grant
(`ApprovalGate::with_review_grants()`, [Human oversight](../../deployment/oversight.md)).

## What the client sees

`ToolLoopExecutor::new(tool_loop)` is an `AgentExecutor`. For each message it
takes the message's text parts, joined by newlines, as the prompt, and emits:

1. `Working`.
2. The `answer` artifact (`ANSWER_ARTIFACT_ID`): the model's final text.
3. The `tool-trace` artifact (`TRACE_ARTIFACT_ID`), when at least one tool
   ran: one line per call, in the order they ran, as
   `name(arguments) -> result`. A line is at most 16 KiB
   (`TRACE_LINE_LIMIT`); a longer one is cut and ends `… [N more bytes]`. The
   model is given the whole result either way.
4. `Completed`.

When a run **fails** after a tool ran, the same artifact is sent first, then
`Failed` (it was not before 0.16.0, and an empty final reply used to complete
the task with it). A caller that cannot see which calls ran retries with new
idempotency keys and runs them again.

A message with no text is refused: the run returns `InvalidParams` ("the
message has no text to answer") and the task fails.

**A reply cut off at the model's output limit still completes the task, and
says so.** When the provider reports `finish_reason: "length"` (a small
`max_tokens`, a full context window), the text is a prefix of an answer and
may end mid-sentence. The `answer` artifact's `metadata` and the `Completed`
status message then both carry `{"a2a-rust.com/finish-reason": "length"}`
(`FINISH_REASON_METADATA_KEY`), and the message reads "The answer was cut off
at the model's output limit and may be incomplete." An answer that ended by
itself has neither. Before 0.16.0 nothing marked the difference. **A final
reply with no text fails the task** (below), because an empty answer is not
one: a reasoning model whose output limit goes on thinking sends exactly that.

The trace is what lets a caller tell an answer the tools backed from one the
model made up. In the example's recorded runs, a fluent answer sometimes gave
a number the tool had not returned, and the trace line beneath it showed the
real one; [Why there are two artifacts](../../examples/tool-loop-agent.md#why-there-are-two-artifacts)
has the details.

A run can take several model turns, and a blocking `SendMessage` moves no
bytes until it ends. A client of a tool-loop agent should stream, or send with
`returnImmediately` and poll; the example's
[Why the client streams](../../examples/tool-loop-agent.md#why-the-client-streams)
shows the failure a blocking send hit.

### Watching the text arrive

The `answer` artifact comes at the end of the run, whole. To watch a long
answer arrive, turn on drafts:

```text
ToolLoopExecutor::new(tool_loop).with_drafts()
```

Each model turn's text then streams, as the model sends it, to an artifact
of its own, `draft-1`, `draft-2` and so on (`draft_artifact_id(turn)`), sent
before the `answer` artifact. The first event creates a draft, the rest
append to it, and the last carries `lastChunk`, when the next turn starts or
the run ends. The loop learns that a turn's text is the answer only when the
turn ends, which is why the text does not stream into `answer` itself: a
turn that goes on to call tools would already have written there. A client
that reads only `answer` and `tool-trace` sees nothing change.

Pieces are coalesced: an append carries at least 256 bytes (`FLUSH_BYTES`),
or whatever arrived once 250 ms (`FLUSH_AFTER`) have passed, so an answer is
tens of appends rather than one per token. Drafts need a model that streams
(`ToolModel::complete_streaming`); `OpenAiChatModel` does, and with a model
that does not, no draft is sent. They are off by default, because a client
that reads every artifact would now see them.

## Running it outside an executor

The loop does not need A2A. `run` answers one prompt:

```rust,no_run
use a2a_protocol_sdk::server::CancellationToken;
use a2a_protocol_sdk::server::tool_loop::{OpenAiChatModel, ToolLoop, ToolSet};

# #[tokio::main]
# async fn main() -> Result<(), Box<dyn std::error::Error>> {
let model = OpenAiChatModel::new("http://127.0.0.1:8080/v1", "qwen3:1.7b")?;
let tool_loop = ToolLoop::new(model, ToolSet::new()).with_max_turns(4);

let answer = tool_loop.run("Say hello.", &CancellationToken::new()).await?;
println!("{}", answer.text);
if answer.truncated() {
    println!("(cut off at the output limit)");
}
for line in &answer.trace {
    println!("{line}");
}
# Ok(())
# }
```

`Answer::finish_reason` is the final reply's own, as the provider spelled it,
when it said; `truncated()` is `finish_reason == Some("length")`
(`FINISH_REASON_LENGTH`). A `ToolModel` of your own reports a cut-off reply by
returning `ModelTurn::with_finish_reason("length")`. `Answer::text` is never
blank: a final reply with no text is `Err(LoopError::EmptyAnswer)` here, as it
fails the task under the executor.

| Method | Budget | Idempotency scope |
|---|---|---|
| `run(prompt, cancel)` | none | random, per run |
| `run_within(prompt, cancel, budget)` | the given `Option<&BudgetHandle>` | random, per run |
| `run_scoped(scope, prompt, cancel, budget)` | the given one | `scope`: give different work different scopes, since the same call in the same scope has the same key |

`ToolLoopExecutor` calls `run_scoped` with the task and message ids as the
scope and the caller's budget from `RequestContext::budget()`.

## Bounds on a run

**Turns.** A run that is still calling tools after `max_turns` turns ends with
`LoopError::TurnLimit`.

**The caller's budget** ([ADR 0023](https://github.com/tomtom215/a2a-rust/blob/main/docs/adr/0023-delegation-budget.md)).
When the caller's message carried a budget, no turn starts once its deadline
has passed or its `tokens` meter is empty, and each reply's reported usage,
input plus output tokens, is charged to that meter; the run ends with
`LoopError::Budget` once it is spent. A provider that reports no usage charges
nothing. How a parent hands a budget to a child is on
[Delegating to Another Agent](../../client/delegation.md#a-parents-budget-deadlines-and-meters).

**Cancellation.** The token is checked before each turn and each call, and a
turn or call in progress is abandoned when it fires. Under
`ToolLoopExecutor` a cancelled run writes nothing; the cancel path writes
`Canceled`.

**The model client's own bounds.** Each turn is bounded by the client's
timeout and response-size cap ([Model clients](./models.md#openaichatmodel)).

**The executor timeout.** The handler's `with_executor_timeout` still applies
around the whole run (one hour by default; see
[Executor timeout](../executor/patterns.md#executor-timeout)).

## How a run fails

A run that ends without an answer fails the task with the failure-class
extension, so a caller can branch on the class. The status message says what
happened in words; it never carries the provider's response body or URL,
which can echo the prompt, an account detail or an internal address. Those go
to the operator's log.

| `LoopError` | Cause | Failure class | The caller is told |
|---|---|---|---|
| `Model(Status)` | The provider answered outside 2xx | `transient` for 429, 502, 503 and 504; `internal` otherwise | `the model provider answered HTTP <status>` |
| `Model(RateLimited)` | The provider answered 429 (sent again first when `with_model_retry` is set) | `transient` | `the model provider answered HTTP 429` |
| `Model(Transport)` | The request could not be sent, or the connection failed | `transient` | `could not reach the model provider` |
| `Model(InvalidResponse)` | The answer was not a chat completion | `internal` | `the model provider's answer could not be read` |
| `Model(Config)` | The client cannot work as configured | `internal` | `the model client is misconfigured` |
| `Model(Timeout)` | No complete answer within the client's timeout | `transient` | `the model provider did not answer within <timeout>` |
| `Model(ResponseTooLarge)` | The answer was over the size cap | `internal` | `the model provider's answer is larger than the <cap>-byte cap` |
| `Model(Other)` | A `ToolModel` you wrote failed | `internal` | Its message, unchanged |
| `Fatal` | A tool returned `ToolError::Fatal` | `internal` | `tool '<name>' failed: <message>` |
| `TurnLimit` | The model asked for tools on every turn | `budget-exhausted` | `the model asked for tools on all <n> turns without answering` |
| `Budget` | The caller's deadline passed or its `tokens` meter ran out | `budget-exhausted` | `the budget's deadline has passed`, or `the budget's meter "tokens" is spent` |
| `EmptyAnswer` | The final reply had no text, with `finish_reason` `length` | `budget-exhausted` | `the model stopped at its output limit before it wrote any text` |
| `EmptyAnswer` | The final reply had no text, with `finish_reason` `content_filter` | `policy-refusal` | `the model provider withheld the answer (content filter)` |
| `EmptyAnswer` | The final reply had no text, any other reason or none | `internal` | `the model returned an empty answer` |
| `Cancelled` | The cancellation token fired | — | Nothing: the cancel path writes `Canceled` |

None of these is retried by the loop, except a `Model(RateLimited)` turn when
`with_model_retry` is set. A tool's ordinary failure
(`ToolError::Failed`) is not on the list: it goes back to the model, and the
run goes on.
