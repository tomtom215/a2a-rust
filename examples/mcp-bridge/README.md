<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# A2A → MCP bridge — a remote agent, callable as a tool

Exposes a remote A2A agent as an **MCP server**, so any MCP client can call
it without knowing A2A exists.

```
  MCP client ──MCP over stdio──→ this bridge ──A2A JSON-RPC──→ remote agent
             ←──tool result─────            ←──task───────────
```

This is the reverse of [`examples/mcp-agent`](../mcp-agent), which is an
agent that *uses* MCP tools. This one makes an A2A agent *be* one.

## Why this direction is the interesting one

The other direction serves A2A agents. This one serves everyone else: an MCP
client does not need an A2A library, an A2A concept, or a line of new code to
call a remote agent — it needs a command in its server list.

```jsonc
// in any MCP client's server configuration
{ "command": "a2a-mcp-bridge", "args": ["http://agent.internal:8080"] }
```

One bridge process fronts one agent, because that is how MCP clients are
configured anyway. Three agents are three entries, not one bridge with a
routing table.

## How much of A2A survives the crossing

More than expected, which is what makes this worth building rather than
faking:

| A2A | MCP | Bridged |
|---|---|---|
| agent card `skills` | `tools/list` | ✅ one tool per skill, the agent's own descriptions |
| `Completed` | tool result | ✅ |
| `Failed`, `Rejected`, `Canceled` | tool result with `isError` | ✅ |
| long-running task | SEP-2663 task, polled with `tasks/get` | ✅ |
| task status message | MCP task status message | ✅ at poll granularity |
| `CancelTask` | `tasks/cancel` | ✅ cooperative on both sides |
| `InputRequired` | `CallToolResponse::InputRequired` | ❌ **reported, not bridged** |

MCP gained long-running tasks in the `2026-07-28` spec (SEP-2663), and that
is what makes the task half of this table possible at all — before it, a
long-running A2A task had nowhere to go but a blocked request.

### The three asymmetries, stated where they happen

**A2A skills carry no argument schema.** An MCP tool has one; an A2A skill
has a description, tags and examples, because A2A's calling convention is
fixed — you send a `Message`. So every published tool takes one string, and
the skill's description is what tells a caller's model which to use.
Inventing a schema per skill would publish a contract the agent never agreed
to.

**A2A has no skill selector.** `SendMessage` has no field naming a skill, so
every tool sends to the same agent and the choice is advisory. The chosen
tool travels in the message metadata under `a2a-mcp-bridge/skill`, which an
agent can route on if it wants and ignore if it does not. The demo's sample
agent routes on it, which is how the transcript below can prove it arrived.

**`input-required` is reported, not bridged.** MCP has a counterpart —
`CallToolResponse::InputRequired`, the multi-round-trip path — and wiring the
two together needs the bridge to hold an A2A task id across MCP calls and
translate `input_responses` into an A2A continuation message. That is real
work with a real chance of being subtly wrong, and a half-built version would
strand tasks in `input-required` with no way to answer them. A paused task
comes back as an error whose text says what the agent is waiting for, which
is honest; a caller can tell "waiting" from "broken".

## Running it

The bridge speaks MCP on its own stdin and stdout, the way MCP clients start
servers:

```bash
a2a-mcp-bridge http://127.0.0.1:8080      # or set A2A_AGENT_URL
```

To see the whole path without wiring up a client, `bridge-demo` stands up a
sample A2A agent, spawns the bridge against it as a real child process, and
acts as an MCP client:

```bash
cargo run -p a2a-mcp-bridge --bin bridge-demo
```

No model and no network — the bridge is protocol only, so this is
deterministic.

## A verified run

Verbatim from `cargo run -p a2a-mcp-bridge --bin bridge-demo` on 2026-09-19.

```console
1. Sample A2A agent listening on http://127.0.0.1:35831
2. Spawned the bridge: target/debug/a2a-mcp-bridge
a2a-mcp-bridge: 'Sample Reporting Agent' at http://127.0.0.1:35831, 2 tool(s): slow_report, always_fails
3. MCP session up, tasks extension declared

Tools the bridge published from the agent card:
  slow_report — Produce a service report. Takes a few hundred milliseconds. (A2A skill 'slow_report' on remote agent 'Sample Reporting Agent'; one call is one agent task)
  always_fails — A skill that always fails, so an error has something to cross. (A2A skill 'always_fails' on remote agent 'Sample Reporting Agent'; one call is one agent task)

--- calling 'slow_report' (completes after a few polls)
    materialized as MCP task 9da79eca-a880-4fb0-a044-28f0ff230c60
    poll 1: working — contacting the A2A agent
    poll 2: working — Submitted
    poll 3: working — Submitted
    poll 4: working — Submitted
    poll 5: working — Working: writing up
    poll 6: working — Working: writing up
    poll 7: working — Working: writing up
    settled after 8 poll(s)
    isError: false
      Report for 'how are the payment services?': 3 services checked, 1 degraded (payments-api). [answered via A2A skill 'slow_report']

--- calling 'always_fails' (comes back as an MCP error result)
    materialized as MCP task 991b72d0-f7fd-446e-aac7-92587e0a965b
    poll 1: working — contacting the A2A agent
    poll 2: working — Submitted
    poll 3: working — Submitted
    poll 4: working — Submitted
    settled after 5 poll(s)
    isError: true
      [-32603] this skill always fails, so the bridge has an error to map

Demo complete. A2A tasks crossed to MCP as tasks, and back as results.
```

Three things that transcript settles.

**The skill hint arrives.** `[answered via A2A skill 'slow_report']` is the
sample agent echoing back the metadata the bridge sent. Without that line the
"advisory hint" above would be a claim about code nobody ran.

**A2A progress reaches the MCP caller.** `Working: writing up` is the A2A
agent's own task status message, crossing two protocols to land in
`tasks/get`.

**And it arrives coarsened.** The agent emits three steps — `gathering`,
`correlating`, `writing up` — 120 ms apart. The caller saw only the last of
them, because the bridge polls A2A every 250 ms. That is the cost of polling
rather than streaming, stated in `src/bridge.rs` and visible here rather than
discovered later.

### Why it polls A2A instead of streaming it

A2A has `SendStreamingMessage`, and it would give finer progress. Two reasons
against. An agent card may not advertise streaming at all, and a bridge that
worked only against streaming agents would be a bridge with a footnote. And
MCP's own task model *is* polling — the client drives `tasks/get` at its own
interval — so poll-to-poll has one clock instead of translating between two.

## Tests

```bash
cargo test -p a2a-mcp-bridge
```

Fifteen, in two layers:

- **The mapping is pure**, so its awkward cases are asserted on values: a
  skill id that is not a legal tool name, two that collide (refused, naming
  both — publishing either would send one skill's calls to the other), a card
  with no skills, and each task state's rendering including the
  `input-required` gap.
- **The bridge needs a real agent**, so those tests start one on localhost and
  connect a real MCP client over an in-process pipe. Every frame in both
  directions is genuinely serialized. They cover discovery, a call returning
  the agent's artifact, the skill hint arriving, a failed A2A task becoming an
  `isError` result *without* killing the session, unknown tools and missing
  arguments, and both refusals-to-start.

The child-process spawn is what `bridge-demo` exercises; the transcript above
is its output. No CI job runs the demo — the fifteen tests run under
`cargo test --workspace`, which CI does run, and they need no network beyond
localhost.

## Failure semantics

| Condition | Result |
|-----------|--------|
| A2A task completes | tool result with the artifact text |
| A2A task fails, is rejected or cancelled | tool result with `isError` and the agent's reason |
| A2A task pauses on `input-required` | `isError`, saying what the agent is waiting for |
| A2A agent unreachable at startup | the bridge refuses to start |
| Agent card advertises no skills | the bridge refuses to start |
| Two skill ids collide as tool names | the bridge refuses to start, naming both |
| A2A task never settles | cancelled after 5 minutes, reported as an error |
| Non-text artifact parts | dropped, with a line saying how many |

Refusing to start is deliberate in three of those rows. An MCP server that
came up with an empty or wrong tool list is indistinguishable, to its caller,
from an agent that has nothing to offer.

## License

Apache-2.0
