<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# MCP Agent — A2A between agents, MCP to tools

An A2A agent whose tools come from an **MCP server it discovers at startup**,
not from a catalogue compiled into it.

```
  A2A client ──JSON-RPC──→ this agent ──MCP over stdio──→ mcp-tool-server
             ←──task───────           ←──tool result─────   (child process)
                                 │
                                 └──HTTP──→ model provider
```

**A2A is how agents talk to each other. MCP is how one agent reaches its
tools.** They meet only inside this process. The A2A protocol never carries a
tool call; the MCP session never carries a task. That is the A2A project's own
recommendation — *"A2A handles inter-agent collaboration and MCP handles tool
integration"* — and this is what it looks like in Rust.

## Why this exists beside `rig-agent`

[`examples/rig-agent`](../rig-agent) is the tool-calling reference: it shows
the loop, with two tools compiled in. This example changes exactly one thing —
where the tools come from — and the value is in how little else moves.

| | `rig-agent` | `mcp-agent` |
|---|---|---|
| Tool catalogue | compiled in (`src/tools.rs`) | **discovered** via `tools/list` |
| Tool execution | a `match` on the name | `tools/call` to another process |
| Agent card skills | written by hand | **derived from what the server offered** |
| The loop | `src/agent.rs` | `src/agent.rs`, same three rules |

Diff `examples/rig-agent/src/agent.rs` against `examples/mcp-agent/src/agent.rs`.
The loop does not care where tools come from. That is the whole claim, and it
is why the loop is worth getting right once.

Nothing in this crate knows what the tools are. Point `MCP_SERVER_BIN` at a
different MCP server — someone else's, in any language — and the agent works
with that server's tools instead, with no code change.

## The two failures that must not be confused

This is the part worth copying, and `src/mcp.rs` is where it happens:

- **A tool ran and refused** — unknown service, bad argument. MCP reports it
  as a JSON-RPC error or as a result carrying `isError`. It is *information
  for the model*, which can read it and try something else, so it becomes
  `ToolOutcome::Refused` and **the A2A task stays alive**.
- **The session is gone** — the child died, the transport closed, the call
  timed out. No re-prompting fixes that, and answering anyway would hand the
  caller a guess dressed as a researched answer. **The A2A task fails.**

The model-unreachable fallback that the sibling examples use is deliberately
*not* extended to a dead MCP server. A missing model is a degraded answer a
label can warn about; missing tools mean the answer would be invented, and no
label makes that acceptable.

## Running

The agent spawns the bundled MCP server itself — there is nothing to start
first.

```bash
# Fully local. --jinja is required, or no model emits a tool call at all,
# and the model must be tool-capable: Qwen3.5-0.8B is not. See
# ../rig-agent/README.md for the measurement.
llama-server -m Qwen3-1.7B-Q4_K_M.gguf --port 11434 --alias qwen3:1.7b --jinja &

export OPENAI_API_KEY=local                     # any non-empty value
export OPENAI_BASE_URL=http://127.0.0.1:11434/v1

cargo run -p mcp-a2a-agent                      # self-driving demo
A2A_BIND_ADDR=127.0.0.1:8080 cargo run -p mcp-a2a-agent    # serve
```

With no `A2A_BIND_ADDR` the example binds an ephemeral port, drives itself
over a real `a2a-protocol-client`, prints what came back, exits non-zero if
either question failed, and reports whether the MCP leg was actually
exercised. With one, it serves until Ctrl-C.

`MCP_SERVER_BIN` points the agent at any other MCP stdio server.

### Talking to the MCP server directly

It is an ordinary MCP server, so you can drive it by hand:

```bash
printf '%s\n%s\n%s\n' \
  '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"probe","version":"0"}}}' \
  '{"jsonrpc":"2.0","method":"notifications/initialized"}' \
  '{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}' \
  | cargo run -q -p mcp-a2a-agent --bin mcp-tool-server
```

## A verified run

Verbatim from `cargo run -p mcp-a2a-agent` on 2026-09-19 — a debug build,
Qwen3-1.7B-Q4_K_M under llama.cpp `b23701f`, four CPU cores. The prose varies
between runs, as any model's does; so, as it turns out, does the number of
tool calls.

```console
MCP tools discovered: list_services, service_status

Demo — driving the agent over A2A at http://127.0.0.1:36845

--- How is the checkout service doing? Give me its version.
    state: Completed
    [mcp-response]
      The checkout service is currently healthy. Its version is 2.8.0.
    [tool-trace]
      service_status({"service":"checkout"}) -> {"service":"checkout","state":"healthy","uptimeSeconds":259200,"version":"2.8.0"}

--- How is the billing service doing?
    state: Completed
    [mcp-response]
      The services in the inventory are: payments-api, checkout, and search.
      The billing service is not listed among them. Please check the service
      name again.
    [tool-trace]
      service_status({"service":"billing"}) -> error: no service named 'billing'; call list_services for the inventory
      list_services({}) -> ["payments-api","checkout","search"]

Demo complete: 2 of 2 answers used MCP tools, both over A2A.
```

Two things in that trace are worth reading twice.

**The tool count is the model's choice, not a property of your code.** The
first question took one MCP round trip here. An earlier run of this same
binary, same model, same question took *two* — the model listed the inventory
before querying it, though it had been given a name it could use directly.
Nothing in the code changed between the runs. That is the argument for
`MAX_TURNS` being a hard bound rather than a tidy default: you are not the one
deciding how many calls a turn makes.

**The recovery path works end to end.** The MCP server refused an unknown
service; the refusal crossed back as a tool *result* rather than a transport
error; the model read the hint the server had written into the message, called
the tool it named, and answered. Every layer behaved, and the A2A task never
failed.

### With no model reachable

The demo still exits 0, because the A2A half genuinely works — but it says
what it did not do:

```text
A2A leg: EXERCISED — both questions round-tripped as tasks.
MCP leg: NOT EXERCISED — no model was reachable, so the agent
  never called a tool and every answer above is the labelled
  mechanical fallback. Point OPENAI_BASE_URL at a tool-capable
  model to exercise it; see README.md.
```

Without that, a green run with no provider would look exactly like a green
run that exercised MCP, and inferring the second from the first is the
substitution this repository keeps removing.

## Tests

```bash
cargo test -p mcp-a2a-agent
```

Fourteen tests, in two groups, and they cover different things:

- **`src/tests.rs`** — ten tests over an in-process `tokio::io::duplex` pipe
  against a purpose-built server. Every request is genuinely serialized,
  framed and parsed as MCP. This is where the branches live: a tool refusing,
  a session dying mid-call, a model that never stops calling tools.
- **`tests/child_process.rs`** — four tests that spawn the **shipped**
  `mcp-tool-server` binary the way a real MCP client does. This is what proves
  the binary in `target/` works, that its schemas reach the wire from the
  `JsonSchema` derive, and that its `main` writes nothing but MCP to stdout —
  a stray `println!` there would desynchronize every frame, and no in-process
  test would notice.

No separate CI job gates the live-model demo; the fourteen tests run under
`cargo test --workspace`, which CI does run, and they need neither a model nor
a network.

## Failure semantics

| Condition | Task state |
|-----------|-----------|
| Answer, no tools called | `TASK_STATE_COMPLETED`, artifact `mcp-response` |
| Answer after tool calls | `TASK_STATE_COMPLETED`, artifacts `mcp-response` **and** `tool-trace` |
| An MCP tool refused | **not a task failure** — the refusal returns to the model, and the trace records it |
| The MCP session broke | `TASK_STATE_FAILED` |
| Model never stops calling tools | `TASK_STATE_FAILED` after `MAX_TURNS` |
| Model provider unreachable | `TASK_STATE_FAILED` when serving; a labelled mechanical reply in the demo |
| Message has no text part | `TASK_STATE_FAILED` (invalid params) |
| MCP server unreachable **at startup** | the process refuses to start |

That last row is deliberate. An agent that cannot reach its tools should not
serve an agent card advertising them.

## License

Apache-2.0
