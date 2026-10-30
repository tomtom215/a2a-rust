<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Running the loop

`ToolLoop` holds the model, the tools and the loop's bounds; `ToolLoopExecutor`
runs it as an A2A task. This page covers the builder, what the executor sends
the client, running the loop outside an executor, the bounds a run is held
to, and how a run that produces no answer fails.

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
| `with_recover_text_calls(bool)` | on | Whether a final reply that is exactly one JSON object naming a tool in the set (bare, in a code fence, or between `<tool_call>` tags) is run as that call rather than returned as the answer |
| `with_idempotency_secret([u8; 32])` | drawn per loop | The secret call keys are derived under; see [Idempotency keys](./tools.md#idempotency-keys) |
| `with_agent_name(name)` | none | Recorded as `gen_ai.agent.name` on the run's metrics |
| `with_meter(Meter)` | the global meter provider | Where the loop's metrics go (`otel` feature); see [Spans and metrics](./telemetry.md) |

The preamble matters more than it looks. Without one, small models tell the
caller to call a tool rather than call it, and restate a tool's numbers from
memory. The [Tool-Loop Agent](../../examples/tool-loop-agent.md) example's
preamble tells the model to call the tools itself, to follow a tool error that
suggests another tool, and to report values exactly as the tools returned
them.

The default of eight turns is four more than the two rounds a
discover-then-query catalogue needs, and small enough that a model which never
stops calling tools fails in seconds rather than at the executor timeout.

## What the client sees

`ToolLoopExecutor::new(tool_loop)` is an `AgentExecutor`. For each message it
takes the message's text parts, joined by newlines, as the prompt, and emits:

1. `Working`.
2. The `answer` artifact (`ANSWER_ARTIFACT_ID`): the model's final text.
3. The `tool-trace` artifact (`TRACE_ARTIFACT_ID`), when at least one tool
   ran: one line per call, in the order they ran, as
   `name(arguments) -> result`.
4. `Completed`.

A message with no text is refused: the run returns `InvalidParams` ("the
message has no text to answer") and the task fails.

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
for line in &answer.trace {
    println!("{line}");
}
# Ok(())
# }
```

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
| `Model(Status)` | The provider answered outside 2xx | `internal` | `the model provider answered HTTP <status>` |
| `Model(Transport)` | The request could not be sent, or the connection failed | `internal` | `could not reach the model provider` |
| `Model(InvalidResponse)` | The answer was not a chat completion | `internal` | `the model provider's answer could not be read` |
| `Model(Config)` | The client cannot work as configured | `internal` | `the model client is misconfigured` |
| `Model(Timeout)` | No complete answer within the client's timeout | `internal` | `the model provider did not answer within <timeout>` |
| `Model(ResponseTooLarge)` | The answer was over the size cap | `internal` | `the model provider's answer is larger than the <cap>-byte cap` |
| `Model(Other)` | A `ToolModel` you wrote failed | `internal` | Its message, unchanged |
| `Fatal` | A tool returned `ToolError::Fatal` | `internal` | `tool '<name>' failed: <message>` |
| `TurnLimit` | The model asked for tools on every turn | `budget-exhausted` | `the model asked for tools on all <n> turns without answering` |
| `Budget` | The caller's deadline passed or its `tokens` meter ran out | `budget-exhausted` | `the budget's deadline has passed`, or `the budget's meter "tokens" is spent` |
| `Cancelled` | The cancellation token fired | — | Nothing: the cancel path writes `Canceled` |

None of these is retried by the loop. A tool's ordinary failure
(`ToolError::Failed`) is not on the list: it goes back to the model, and the
run goes on.
