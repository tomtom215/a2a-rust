# Tool-Loop Agent

An agent that calls tools, with no tool loop of its own. [Rig Agent](./rig-agent.md)
and [MCP Agent](./mcp-agent.md) each write their loop over rig-core. This one
writes none: `ToolLoopExecutor` runs the SDK's `ToolLoop` over
`OpenAiChatModel`, and the agent is two tools (`list_services`,
`service_status`), a preamble and an agent card. Its only dependency is
`a2a-protocol-sdk` with the `tool-loop` feature (see
[ADR 0022](https://github.com/tomtom215/a2a-rust/blob/main/docs/adr/0022-tool-calling-loop.md)).

A run serves the agent over JSON-RPC on an ephemeral loopback port, asks it one
question with the A2A client, prints the `answer` and `tool-trace` artifacts,
and exits non-zero unless the task completed.

```bash
TOOL_LOOP_BASE_URL=http://127.0.0.1:8080/v1 \
  cargo run -p tool-loop-agent -- "How is the billing service doing?"
```

The model is any OpenAI-compatible chat endpoint: llama.cpp's `llama-server`,
vLLM, Ollama or a hosted API.

| Variable | Default |
|---|---|
| `TOOL_LOOP_BASE_URL` | `http://127.0.0.1:8080/v1` |
| `TOOL_LOOP_MODEL` | `qwen3:1.7b` |
| `TOOL_LOOP_API_KEY` | none; sent as `Authorization: Bearer` when set |
| `TOOL_LOOP_THINKING` | unset: each request carries `"chat_template_kwargs": {"enable_thinking": false}`; `1` sends nothing extra |
| `TOOL_LOOP_MODEL_TIMEOUT_SECS` | 120, `OpenAiChatModel`'s default: how long one model turn may take |
| `TOOL_LOOP_EXTRA_BODY` | none: a JSON object merged into every request (sampler, seed) |

## What the default question exercises

With no question on the command line it asks "How is the billing service
doing?". The inventory has no `billing`, so the model has to read the tool's
error, call `list_services`, and answer: a two-round loop. A model that answers
without calling a tool, or that stops after the first error, shows up in the
`tool-trace` artifact.

`cargo test -p tool-loop-agent` needs no model. It plays the billing run with a
scripted `ToolModel` through the same server, client and tools.

## Why there are two artifacts

The `answer` is what the model said. The `tool-trace` is what the tools
returned. They are separate because a fluent answer can disagree with its own
trace. In the recorded runs (a 1.7-billion-parameter model on four shared CPU
cores), six of ten answers to the checkout question gave an uptime the tool had
not returned, and the trace line beneath each showed the real number. Counts
that small say what those runs did, not a rate; the point is that the trace is
there to be read.

## Why the client streams

A blocking `SendMessage` moves no bytes until the run is over, and the server
closes a connection idle for 75 seconds (`DEFAULT_IDLE_TIMEOUT`). A model turn on
a small local model can outlast that, so the blocking send fails in the client
with `client error (SendRequest)`. The stream does not: the server sends a
keep-alive after 30 seconds without an event
(`DispatchConfig::sse_keep_alive_interval`). The first version of this example
sent a blocking request, and 3 of its 15 runs failed that way. A caller of any
agent whose turn can outlast the idle timeout should stream, or poll a
`return_immediately` send. See [Streaming Responses](../client/streaming.md).

## Where next

- [Dogfooding Against a Real Model](../deployment/dogfooding-real-model.md) and
  the [Live Model](./live-model.md) example run this same loop on every binding,
  with provenance, budgets, a halt and an approval quorum.
- [API Quick Reference](../reference/api-reference.md) lists `ToolLoop`,
  `ToolLoopExecutor` and `OpenAiChatModel`.
- The example's
  [README](https://github.com/tomtom215/a2a-rust/blob/main/examples/tool-loop-agent/README.md)
  has the recorded runs (model, build, host, ten runs of each prompt) and one
  transcript of each.
