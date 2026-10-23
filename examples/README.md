<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Examples

End-to-end examples for the [a2a-rust](https://github.com/tomtom215/a2a-rust)
SDK. Each is a `publish = false` crate in this workspace that depends on the
SDK by path, so run it from the repository root. [`harness/`](harness/) is
not an example: it is the shared library that computes the coverage matrix.

## Start here

```bash
cargo run -p hello-agent   # the smallest agent: one file, JSON-RPC on 127.0.0.1:3000
cargo run -p echo-agent    # every A2A method over every binding, then exits
cargo run -p agent-team    # the SDK's dogfood suite: 100 end-to-end tests
```

`cargo test --workspace` runs the SDK's own test suite; the commands above run
examples.

## The examples, smallest first

The package name is not always the directory name, so each row gives the
command. "Exits" means a default run checks itself and exits with a status;
"serves" means it runs until Ctrl+C.

| Example | What it shows | Run | Needs | Default run |
|---|---|---|---|---|
| [hello-agent](hello-agent/) | The smallest complete agent: an executor, a card and a JSON-RPC listener in one file | `cargo run -p hello-agent` | Nothing | Serves |
| [deploy-agent](deploy-agent/) | The same agent made deployable: `PORT`/`AGENT_URL` configuration, `/healthz` and `/readyz`, graceful shutdown, a Dockerfile and a Kubernetes manifest | `cargo run -p deploy-agent` | Docker or Kubernetes, optionally | Serves |
| [echo-agent](echo-agent/) | All four bindings behind one handler; drives every method over each and prints the coverage matrix, then runs counter-tests against an agent that declares no optional capabilities | `cargo run -p echo-agent` | Nothing | Exits |
| [incident-response](incident-response/) | Three agents handling an incident: multi-turn `INPUT_REQUIRED`, delegation, streaming progress, artifacts, cooperative cancellation; then the surface matrix and sixteen production-hardening checks (tenancy, four authentication mechanisms, limits, retries, three task stores, signing, telemetry, HTTPS push, shutdown) | `cargo run -p incident-response` | A model and PostgreSQL, optionally | Serves after the demo; `INCIDENT_EXIT_WHEN_DONE=1` exits |
| [agent-team](agent-team/) | A team of agents over JSON-RPC, REST and gRPC, plus one serving all four bindings: 100 end-to-end tests, with a feature table computed from the results; the SDK's dogfood suite | `cargo run -p agent-team` | Nothing | Exits |
| [resilient-agent](resilient-agent/) | Durability across a handler restart (SQLite stores reopened by a fresh handler, including a task cut off mid-stream); failure injection with the numbers the SDK reports; two replicas over in-memory and shared PostgreSQL stores. Each check states what the SDK does **not** do: no executor retry or resume, no push redelivery, no cross-replica events | `cargo run -p resilient-agent` | PostgreSQL, optionally | Exits |
| [genai-agent](genai-agent/) | An LLM-backed agent through [genai](https://crates.io/crates/genai); all four bindings and the surface matrix | `cargo run -p genai-a2a-agent` | A model, optionally | Exits; `A2A_BIND_ADDR` serves |
| [rig-agent](rig-agent/) | A rig-core agent with tool calling: a bounded tool loop over a two-tool catalogue, showing where tool calling sits relative to A2A. Passes the in-repo TCK (21 of 21 graded, 1 not applicable; gated by `tck.yml`) | `cargo run -p rig-a2a-agent` | A model, optionally | Exits; `A2A_BIND_ADDR` serves |
| [tool-loop-agent](tool-loop-agent/) | Tool calling from the SDK alone: `ToolLoopExecutor` over `OpenAiChatModel` (feature `tool-loop`, ADR 0022), two service-inventory tools, asked one question through the A2A client; prints the answer and the `tool-trace` artifact | `cargo run -p tool-loop-agent -- "a question"` | A model (`TOOL_LOOP_BASE_URL`) | Exits |
| [live-model](live-model/) | The SDK end to end against a real model: the tool loop streamed and signature-verified on all four bindings, provenance refusal, idempotency, budget exhaustion, delegation under a budget, halt and an approval quorum of two, each reported pass, fail or not exercised. The README records runs with Qwen3-1.7B and Qwen3.5-2B on llama-server | `cargo run --release -p live-model` | A model (`LIVE_BASE_URL`) | Exits |
| [mcp-agent](mcp-agent/) | Tools over MCP: spawns an MCP server, discovers its tools with `tools/list`, and derives its card's skills from the answer. Ships the server too | `cargo build -p mcp-a2a-agent && cargo run -p mcp-a2a-agent --bin mcp-a2a-agent` | A model, optionally | Exits; `A2A_BIND_ADDR` serves |
| [mcp-bridge](mcp-bridge/) | The reverse: an A2A agent callable as an MCP tool. One MCP tool per skill, A2A tasks mapped onto MCP tasks; progress, cancellation and `isError` all cross | `cargo build -p a2a-mcp-bridge && cargo run -p a2a-mcp-bridge --bin bridge-demo` | Nothing | Exits |
| [langfuse-agent](langfuse-agent/) | Two agents in one Langfuse trace, exported by the SDK's own `Telemetry` with no tracing code in either agent | `cargo run -p langfuse-agent -- "a question"` | A self-hosted Langfuse | Exits |
| [multi-lang-team](multi-lang-team/) | A Rust coordinator delegating to Python, JavaScript, Go, Java and Rust workers; all four bindings and the surface matrix | `cargo run -p multi-lang-team` | The worker agents, optionally | Exits; `A2A_BIND_ADDR` serves |
| [swarm](swarm/) | A coordinator, supervisors and workers, every hop a real A2A call: fan-out throughput, retry by `FailureClass`, and cancellation, with a control arm showing that A2A cascades no cancellation on its own | `cargo run --release -p swarm` | A model, optionally | Exits |

`mcp-agent` and `mcp-bridge` each spawn their second binary from the build
directory, and `cargo run --bin` builds only the binary it runs, hence the
`cargo build` first.

**Telemetry.** Every binary here but two starts the SDK's `Telemetry` from the
standard `OTEL_*` environment, as `langfuse-agent` does: set
`OTEL_EXPORTER_OTLP_ENDPOINT` to a collector and its traces, metrics and logs
go there (`OTEL_SERVICE_NAME` overrides the default service name, the
binary's); leave it unset and nothing is built, so a plain `cargo run` needs
no collector. The two: `hello-agent`, whose point is that the prelude alone
says hello, and `mcp-agent`'s `tool_server`, an MCP server that runs no A2A
code. So do `tck/sut`, the ITK agent in `itk/` and the `a2a` CLI.

## Which one to read

- **Tool calling:** [`tool-loop-agent`](tool-loop-agent/), with the SDK's
  own loop (`server::tool_loop`, feature `tool-loop`), or
  [`rig-agent`](rig-agent/), which writes the same loop out over rig-core.
  A2A has no tool concept, so the loop lives between the executor and its
  model and the server never sees it. Both show the bounded loop, tool errors
  returned to the model rather than failing the task, and a `tool-trace`
  artifact that makes the calls visible to the caller.
- **Tools from an MCP server:** [`mcp-agent`](mcp-agent/), the same loop with
  the catalogue discovered at startup. Diff its `src/agent.rs` against
  `rig-agent`'s: only the tool source moves.
- **An MCP client reaching an A2A agent:** [`mcp-bridge`](mcp-bridge/).
- **Cross-language interop:** [`multi-lang-team`](multi-lang-team/).
- **Orchestrating many agents:** [`swarm`](swarm/).

## The pattern every agent follows

[`hello-agent/src/main.rs`](hello-agent/src/main.rs) is the pattern in about
fifty lines, and is compiled and tested in CI:

1. An executor, written with `agent_executor!` and an `EventEmitter`: a
   `Working` status, an artifact, a `Completed` status.
2. A handler: `RequestHandlerBuilder::new(executor).with_agent_card(card).build()`,
   wrapped in an `Arc`.
3. A dispatcher for each binding the agent serves (`JsonRpcDispatcher`,
   `RestDispatcher`, the gRPC and WebSocket dispatchers, or `A2aRouter` for
   Axum), handed to `serve`, or to a `Server` with a `ServeConfig` in
   production.

## Prerequisites

- Rust 1.94 or later (the MSRV).
- No `protoc`: the build scripts use a vendored binary on Linux and macOS
  (x86_64 and aarch64) and Windows x86_64. Set `PROTOC` to use your own.
- Anything else an example needs is in its README.

## Surface coverage

Six examples serve all four bindings, drive **every** A2A method over each,
and assert that the matrix is complete. Measured 2026-08-11, all six at **44
of 44 cells**:

| Example | Cells | Also |
|---|---|---|
| `echo-agent` | 44/44 | 5 counter-tests |
| `incident-response` | 44/44 | the narrative demo first, then 16 hardening checks |
| `agent-team` | 44/44 | 100 end-to-end tests |
| `genai-agent` | 44/44 | the LLM leg reported separately |
| `rig-agent` | 44/44 | the LLM leg reported separately |
| `multi-lang-team` | 44/44 | worker reachability reported per language |

The rows are not this project's list. They come from `Method::ALL`, which
`a2a-protocol-types` asserts equal to `service A2AService` in the ratified
`proto/a2a_v1/a2a.proto`, and which `scripts/check_method_denominator.py`
cross-checks against the upstream `a2aproject/a2a-tck` on every Official TCK
run. A reviewer auditing "is this 11 the real 11?" reads the proto.

Exit codes are the gate: `0` complete, `1` a call or counter-test failed, `2`
a matrix cell never ran, and in `incident-response` `3` a hardening check
failed and `4` a capability went unexercised. `ci.yml`'s `example-surface`
job runs five of the six, each leg proven able to fail by injection, and
`resilient-agent` and `swarm` beside them; its `dogfood` job runs
`agent-team`. `hello-agent`,
`deploy-agent`, `mcp-agent`, `mcp-bridge` and `langfuse-agent` are covered by
their tests under `cargo test --workspace`.

### Beyond the protocol

`incident-response` adds an Act 5 that leaves the method × binding matrix
behind and exercises what a deployment needs: **16 checks** covering tenant
isolation, four authentication mechanisms (bearer, API key, JWT via JWKS, and
the client-side credential store), rate limiting, handler limits, client
retries, three task stores (in-memory, SQLite, PostgreSQL, plus the
tenant-partitioned variants), agent-card signing, the `Metrics` hook,
OpenTelemetry both in-process and over OTLP, HTTPS push delivery and graceful
shutdown. Each asserts the specific wrong answer it rules out, and each was
proven able to fail by removing the capability it covers. Run them alone with
`cargo run -p incident-response -- harden`.

A check that needs something the example cannot start reports `[NOT RUN]`,
naming what to set, rather than passing quietly. Only PostgreSQL is in that
category, and `INCIDENT_REQUIRE_ALL=1` (set in CI, which provides the
service) makes it an error.

**What a green run does *not* mean.** Several examples depend on something
CI does not have, and say so rather than letting a full matrix imply
otherwise. `genai-agent` and `rig-agent` print `LLM leg: NOT EXERCISED` and
label every fallback answer; `multi-lang-team` prints each worker it cannot
reach as `not reachable`; `swarm` reports its model scenario as skipped. CI
runs `mcp-agent`'s and `langfuse-agent`'s tests but never a model or a
Langfuse instance. The protocol surface is measured either way; the
integration is not.

### History

Until 2026-08-11 this index said `echo-agent` demonstrated "the complete
request lifecycle" while it drove 4 of the 11 methods over 2 of the 4
bindings, and that `agent-team` exercised "every SDK feature with 81+
automated tests" while no CI job ran it, 15 of its 86 tests failed, and its
feature table printed `[x]` for every row. Both are now computed and gated,
and until then no example demonstrated the hardening checks above over a
socket.
