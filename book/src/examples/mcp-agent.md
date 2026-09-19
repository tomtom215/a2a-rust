# MCP Agent

An A2A agent whose tools come from an **MCP server it discovers at startup**,
rather than from a catalogue compiled into it.

```text
  A2A client ──JSON-RPC──→ this agent ──MCP over stdio──→ mcp-tool-server
             ←──task───────           ←──tool result─────   (child process)
                                 │
                                 └──HTTP──→ model provider
```

A2A is how agents talk to each other. MCP is how one agent reaches its tools.
They meet only inside this process: the A2A protocol never carries a tool
call, and the MCP session never carries a task. That is the A2A project's own
recommendation — *"A2A handles inter-agent collaboration and MCP handles tool
integration"* — in Rust.

## Why it exists beside the Rig Agent

[Rig Agent](./rig-agent.md) is the tool-calling reference, with two tools
compiled in. This example changes exactly one thing, and the value is in how
little else moves:

| | `rig-agent` | `mcp-agent` |
|---|---|---|
| Tool catalogue | compiled in | discovered via `tools/list` |
| Tool execution | a `match` on the name | `tools/call` to another process |
| Agent card skills | written by hand | derived from what the server offered |
| The loop | `src/agent.rs` | `src/agent.rs`, same three rules |

Diffing the two `agent.rs` files is the point: the loop does not care where
tools come from, which is why it is worth getting right once. Nothing in the
crate knows what the tools are, so `MCP_SERVER_BIN` repoints it at any other
MCP stdio server — someone else's, in any language — with no code change.

## The two failures that must not be confused

- **A tool ran and refused** — unknown argument, unknown record. MCP reports
  it as a JSON-RPC error or as a result carrying `isError`. It is information
  *for the model*, so it returns as a tool result and the A2A task stays
  alive.
- **The session is gone** — the child died, the transport closed. No
  re-prompting fixes that, and answering anyway would hand the caller a guess
  dressed as a researched answer. The A2A task fails.

The model-unreachable fallback the sibling examples use is deliberately not
extended to a dead MCP server: a missing model is a degraded answer a label
can warn about, while missing tools mean the answer would be invented.

## Running

The agent spawns the bundled MCP server itself; there is nothing to start
first.

```bash
llama-server -m Qwen3-1.7B-Q4_K_M.gguf --port 11434 --alias qwen3:1.7b --jinja &

export OPENAI_API_KEY=local
export OPENAI_BASE_URL=http://127.0.0.1:11434/v1

cargo run -p mcp-a2a-agent                                 # self-driving demo
A2A_BIND_ADDR=127.0.0.1:8080 cargo run -p mcp-a2a-agent    # serve
```

`--jinja` is required or no model emits a tool call, and the model must be
tool-capable — see [Rig Agent](./rig-agent.md#the-local-model-has-to-be-tool-capable)
for the measurement.

## What a run shows

**The tool count is the model's choice, not a property of your code.** Two
runs of the same binary, same model, same question, with nothing changed
between them: one took a single `service_status` call, the other listed the
inventory first and took two. That is the argument for `MAX_TURNS` being a
hard bound rather than a tidy default — you are not the one deciding how many
calls a turn makes.

**The recovery path works end to end.** The MCP server refused an unknown
service; the refusal crossed back as a tool *result* rather than a transport
error; the model read the hint the server had written into the message, called
the tool it named, and answered. The A2A task never failed.

With no model reachable the demo still exits 0 — the A2A half genuinely
works — but it prints `MCP leg: NOT EXERCISED` and says every answer above it
is the labelled mechanical fallback. Otherwise a green run with no provider
would look exactly like a green run that exercised MCP.

## Tests

Fourteen, in two groups. Ten drive the bridge over an in-process
`tokio::io::duplex` pipe — real MCP framing, no process — and cover the
branches: a tool refusing, a session dying mid-call, a model that never stops
calling. Four spawn the **shipped** `mcp-tool-server` binary the way a real
MCP client does, which is what proves its schemas reach the wire from the
`JsonSchema` derive and that its `main` writes nothing but MCP to stdout. A
stray `println!` there would desynchronize every frame, and no in-process test
would notice.

They need neither a model nor a network, and run under `cargo test
--workspace`. No separate CI job gates the live-model demo.

## Failure semantics

| Condition | Task state |
|-----------|-----------|
| Answer, no tools called | `TASK_STATE_COMPLETED`, artifact `mcp-response` |
| Answer after tool calls | `TASK_STATE_COMPLETED`, artifacts `mcp-response` and `tool-trace` |
| An MCP tool refused | **not** a task failure — the refusal returns to the model, and the trace records it |
| The MCP session broke | `TASK_STATE_FAILED` |
| Model never stops calling tools | `TASK_STATE_FAILED` after `MAX_TURNS` |
| Model provider unreachable | `TASK_STATE_FAILED` when serving; a labelled mechanical reply in the demo |
| Message has no text part | `TASK_STATE_FAILED` (invalid params) |
| MCP server unreachable at startup | the process refuses to start |

The last row is deliberate: an agent that cannot reach its tools should not
serve an agent card advertising them.

See [`examples/mcp-agent/README.md`](https://github.com/tomtom215/a2a-rust/blob/main/examples/mcp-agent/README.md)
for the full walkthrough and the verbatim transcript.
