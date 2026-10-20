<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# tool-loop-agent

An agent with tool calling from the SDK alone (ADR 0022). `rig-agent` and
`mcp-agent` write their own tool loop over rig-core; this one writes none.
`ToolLoopExecutor` runs the SDK's `ToolLoop` over `OpenAiChatModel`, and
the agent is two tools (`list_services`, `service_status`, the same
inventory as `rig-agent`), a preamble and a card. One dependency for the
agent: `a2a-protocol-sdk` with `features = ["tool-loop"]`.

A run serves the agent over JSON-RPC on an ephemeral loopback port, asks it
one question with the A2A client (streaming), prints the `answer` and
`tool-trace` artifacts, and exits non-zero unless the task completed.

```bash
TOOL_LOOP_BASE_URL=http://127.0.0.1:8080/v1 cargo run -p tool-loop-agent -- \
    "How is the billing service doing?"
```

| Variable | Default |
|---|---|
| `TOOL_LOOP_BASE_URL` | `http://127.0.0.1:8080/v1` (llama-server's default port) |
| `TOOL_LOOP_MODEL` | `qwen3:1.7b` |
| `TOOL_LOOP_API_KEY` | none; sent as `Authorization: Bearer` when set |
| `TOOL_LOOP_THINKING` | unset: every request carries `"chat_template_kwargs": {"enable_thinking": false}` (`OpenAiChatModel::with_extra_body`); `1` sends nothing extra |
| `TOOL_LOOP_EXTRA_BODY` | none: a JSON object merged into every request after the line above, for the sampler and seed (`docs/tool-calling-study.md` drives the agent this way) |

With no question on the command line it asks "How is the billing service
doing?": the inventory has no `billing`, so the model has to read the tool's
error, call `list_services`, and answer — a two-round loop.

`cargo test -p tool-loop-agent` needs no model: it plays the billing run
with a scripted `ToolModel` through the same server, client and tools.

## Live run, 2026-10-20

| | |
|---|---|
| Server | llama.cpp `llama-server` build b11751 (`8466395`), OpenAI-compatible API at `http://127.0.0.1:11435/v1`, context 8192 |
| Model | `qwen3:1.7b` (`qwen3-1.7b-q4_k_m.gguf`, Qwen3-1.7B Q4_K_M) |
| Library | `server::tool_loop` as of commit `d2bd157`; this example as committed with this file |
| Host | four shared CPU cores, with another model server benchmarking on the same machine |

```bash
TOOL_LOOP_BASE_URL=http://127.0.0.1:11435/v1 TOOL_LOOP_MODEL=qwen3:1.7b \
    cargo run -p tool-loop-agent -- "How is the billing service doing?"
TOOL_LOOP_BASE_URL=http://127.0.0.1:11435/v1 TOOL_LOOP_MODEL=qwen3:1.7b \
    cargo run -p tool-loop-agent -- "How is the checkout service doing? Give me its version."
```

Ten runs of each, alternating, thinking off (the default above):

| Prompt | Completed | Tool calls | Answer |
|---|---|---|---|
| billing (unknown name: two rounds) | 10 of 10 | 10 of 10 called `service_status("billing")`, read the error, then called `list_services` | 10 of 10 named the three services and said billing is not among them |
| checkout (one call) | 10 of 10 | 10 of 10 called `service_status("checkout")` once | 10 of 10 gave the state and version 2.8.0 correctly; the uptime was right (259200 s, about 72 h) in 3, a number the tool never returned (7200 s) in 6, and 259200 s converted wrongly to 7.77 h in 1 |

No run needed a call recovered from text, none hit the turn bound, and the
provider failed no turn. A run took 14 to 104 s.

One transcript of each, verbatim:

```console
state: TASK_STATE_COMPLETED
[ answer ]
The services in the inventory are: payments-api, checkout, search. The billing service is not listed among them.
[ tool-trace ]
service_status({"service": "billing"}) -> error: no service named 'billing'; call list_services for the inventory
list_services({}) -> ["payments-api","checkout","search"]
```

```console
state: TASK_STATE_COMPLETED
[ answer ]
The checkout service is in a healthy state. Its current version is 2.8.0, and it has been running for 7200 seconds (2 hours).
[ tool-trace ]
service_status({"service": "checkout"}) -> {"service":"checkout","state":"healthy","uptimeSeconds":259200,"version":"2.8.0"}
```

The second is why the trace is an artifact. The answer is fluent and its
uptime is invented; the line beneath it shows what the tool returned. Six of
ten checkout answers did this, against the preamble's instruction to report
values exactly as returned; `rig-agent`'s README recorded none in twenty on
the same model and build with the same preamble, through rig-core's request.
Counts this small say what these runs did, not a rate.

### Why the client streams

The first version of this example sent the question with a blocking
`SendMessage`. Fifteen runs completed that way before it changed: 12
answered, and 3 failed in the client with `client error (SendRequest)`. A
blocking send moves no bytes until the run is over, and the server closes a
connection idle for 75 s (`ServeConfig`'s `DEFAULT_IDLE_TIMEOUT`). Against a
stand-in model that answers after 80 s, the blocking send failed at exactly
75.0 s and the streamed one completed at 80 s: the stream carries a
keep-alive every 30 s. Six of the twenty streamed runs above took longer
than 75 s. A caller of any agent whose turn can outlast the idle timeout —
any agent on a small local model — should stream, or poll a
`return_immediately` send.
