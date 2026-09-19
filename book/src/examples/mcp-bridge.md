# A2A → MCP Bridge

Exposes a remote A2A agent as an **MCP server**, so any MCP client can call
it without knowing A2A exists.

```text
  MCP client ──MCP over stdio──→ this bridge ──A2A JSON-RPC──→ remote agent
             ←──tool result─────            ←──task───────────
```

This is the reverse of [MCP Agent](./mcp-agent.md), which is an agent that
*uses* MCP tools. This one makes an A2A agent *be* one — and it is the
direction that reaches further, because an MCP client needs no A2A library,
no A2A concept and no new code to call a remote agent. It needs a command in
its server list:

```jsonc
{ "command": "a2a-mcp-bridge", "args": ["http://agent.internal:8080"] }
```

One bridge process fronts one agent, because that is how MCP clients are
configured anyway.

## How much of A2A survives the crossing

| A2A | MCP | Bridged |
|---|---|---|
| agent card `skills` | `tools/list` | ✅ one tool per skill, the agent's own descriptions |
| `Completed` | tool result | ✅ |
| `Failed`, `Rejected`, `Canceled` | tool result with `isError` | ✅ |
| long-running task | SEP-2663 task, polled with `tasks/get` | ✅ |
| task status message | MCP task status message | ✅ at poll granularity |
| `CancelTask` | `tasks/cancel` | ✅ cooperative on both sides |
| `InputRequired` | `CallToolResponse::InputRequired` | ❌ reported, not bridged |

MCP gained long-running tasks in the `2026-07-28` specification (SEP-2663),
and that is what makes the task rows possible at all. Before it, a
long-running A2A task had nowhere to go but a blocked request.

## Three asymmetries worth knowing before you copy this

**A2A skills carry no argument schema.** An MCP tool has one; an A2A skill
has a description, tags and examples, because A2A's calling convention is
fixed — you send a `Message`. Every published tool therefore takes one
string, and the skill's description is what tells a caller's model which to
use. Inventing a schema per skill would publish a contract the agent never
agreed to.

**A2A has no skill selector.** `SendMessage` has no field naming a skill, so
every tool sends to the same agent and the choice is advisory. The chosen
tool travels in the message metadata under `a2a-mcp-bridge/skill`, which an
agent may route on or ignore.

**`input-required` is reported, not bridged.** MCP has a counterpart, and
wiring it up needs the bridge to hold an A2A task id across MCP calls and
translate `input_responses` into an A2A continuation message — real work with
a real chance of being subtly wrong, where a half-built version would strand
tasks with no way to answer them. A paused task comes back as an error whose
text says what the agent is waiting for, so a caller can tell "waiting" from
"broken".

## Running

```bash
a2a-mcp-bridge http://127.0.0.1:8080      # or set A2A_AGENT_URL

# or, to see the whole path at once:
cargo run -p a2a-mcp-bridge --bin bridge-demo
```

`bridge-demo` stands up a sample A2A agent, spawns the bridge against it as a
real child process, and acts as an MCP client. No model and no network — the
bridge is protocol only, so the demo is deterministic.

## What the demo run shows

The verbatim transcript is in the example's README. Three things it settles:

- **The skill hint arrives.** The sample agent echoes back the metadata the
  bridge sent, so the "advisory hint" above is demonstrated rather than
  asserted.
- **A2A progress reaches the MCP caller.** The agent's own task status
  message crosses two protocols and lands in `tasks/get`.
- **It arrives coarsened.** The agent emits three steps 120 ms apart; the
  caller sees the last one, because the bridge polls A2A every 250 ms. That
  is the stated cost of polling rather than streaming, visible in the output
  rather than discovered later.

Why polling: an agent card may not advertise streaming at all, and MCP's own
task model is polling, so poll-to-poll keeps one clock instead of translating
between two.

## Tests

Fifteen, in two layers. The mapping is pure, so its awkward cases are
asserted on values — a skill id that is not a legal tool name, two that
collide (refused, naming both), a card with no skills, and each task state's
rendering including the `input-required` gap. The bridge needs a real agent,
so those tests start one on localhost and connect a real MCP client over an
in-process pipe, covering discovery, a returned artifact, the skill hint
arriving, a failed A2A task becoming an `isError` result without killing the
session, and both refusals-to-start.

They need no model and no network beyond localhost, and run under `cargo test
--workspace`. The child-process spawn is what `bridge-demo` exercises; no CI
job runs the demo.

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

Refusing to start is deliberate in three of those rows: an MCP server that
came up with an empty or wrong tool list is indistinguishable, to its caller,
from an agent with nothing to offer.

See [`examples/mcp-bridge/README.md`](https://github.com/tomtom215/a2a-rust/blob/main/examples/mcp-bridge/README.md)
for the full walkthrough and the verbatim transcript.
