# Rig Agent

A real [rig](https://github.com/0xPlaygrounds/rig) agent served over the
A2A protocol: incoming A2A messages are passed to `RigAgent`, which the
example defines over `rig_core::completion::CompletionModel`, runs a **tool
loop**, and returns the answer as an A2A artifact. The executor is generic
over that same `CompletionModel`, so swapping providers (Anthropic, Gemini,
Ollama, …) only changes the client construction in `main`.

This is the example to read for **tool calling**.

## Tool calling, and where it sits relative to A2A

A2A has no tool concept. It carries messages, tasks and artifacts *between*
agents; what an agent does inside one task is its own business. Tool calling
lives one layer down, between `RigAgent` and its model, and the A2A server
never sees it — `src/tools.rs` imports no `a2a-protocol-*` type at all. Many
model turns, one A2A task.

That is the layering to copy: an A2A server does not become a tool runtime,
it hosts one.

The catalogue is two tools over a hard-coded inventory of three services —
`list_services` (no arguments) and `service_status` (one). Two, because
between them they cover both JSON Schema shapes a provider has to encode, and
because they compose: a model asked about a service it does not recognise has
to discover the inventory first, which is a two-round loop. One round is the
case that still works when the loop is written wrong.

Three things the loop gets right, each a bug if you copy the shape without
them:

1. **The catalogue goes on every request**, not just the first — a provider
   holds no state between turns.
2. **A tool error is a result, not a failure.** It goes back to the model as
   that call's result, so the model can recover.
3. **The loop is bounded** (`MAX_TURNS`, 6). Unbounded, a model that calls
   tools forever holds the A2A task open until the server's executor timeout,
   an hour by default.

When tools ran, the task carries a second `tool-trace` artifact naming each
call and its result, so a caller can tell a researched answer from a guessed
one.

That is not decoration. In the transcript the README quotes, the 1.7B model
reports an uptime the tool never returned — the trace line directly beneath
it shows the real figure. Grounded and *verifiable* are different properties,
and only the second is one the protocol can carry.

## Running

```bash
# Hosted OpenAI:
export OPENAI_API_KEY=sk-...
RIG_MODEL=gpt-4o-mini cargo run -p rig-a2a-agent   # RIG_MODEL defaults to qwen3:1.7b

# Fully local — any OpenAI-compatible server (llama-server, Ollama):
export OPENAI_API_KEY=local             # any non-empty value
export OPENAI_BASE_URL=http://127.0.0.1:11434/v1
RIG_MODEL=qwen3:1.7b cargo run -p rig-a2a-agent
```

Set `A2A_BIND_ADDR=127.0.0.1:8080` for a fixed port. The agent serves a
discovery card at `/.well-known/agent-card.json`, supports push-config
CRUD, and passes the in-repo TCK: 21/21 graded checks, 1 not applicable, on
the JSON-RPC binding. `tck.yml`'s `tck-example-agents` job gates that figure on
every push and pull request, with no model configured;
`cargo run -p a2a-tck -- --url <addr> --binding jsonrpc` reproduces it.

### The local model has to be tool-capable

Measured 2026-09-19 against the same `llama-server` build (llama.cpp
`b23701f`, `--jinja`), with this example's catalogue:

| Model | Result |
|---|---|
| Qwen3.5-0.8B-Q4_0 | **Cannot.** Answers in prose. Forced with `tool_choice: "required"` it still emits no tool call, and runs to `finish_reason: "length"`. |
| Qwen3-1.7B-Q4_K_M | **Can.** First request returns `finish_reason: "tool_calls"`. |

Hence the default, which differs from the sibling LLM examples': they do not
call tools, so 0.8B remains right for them. `--jinja` is load-bearing —
without it `llama-server` applies no chat template and no model emits a tool
call however capable it is.

## Failure semantics

| Condition | Task state |
|-----------|-----------|
| Answer, no tools called | `TASK_STATE_COMPLETED`, artifact `rig-response` |
| Answer after tool calls | `TASK_STATE_COMPLETED`, artifacts `rig-response` and `tool-trace` |
| A tool errored | **not** a task failure — the error returns to the model, and the trace records it |
| Model never stops calling tools | `TASK_STATE_FAILED` after `MAX_TURNS` |
| Provider unreachable / errors | `TASK_STATE_FAILED` |
| Message has no text part | `TASK_STATE_FAILED` (invalid params) |

Errors surface through the task state — they are never folded into a
"successful" artifact. The one deliberate exception is a *tool* error, which
is not an agent failure but information the model is expected to act on.

See [`examples/rig-agent/README.md`](https://github.com/tomtom215/a2a-rust/blob/main/examples/rig-agent/README.md)
for the full verified walkthrough, including both transcripts.
