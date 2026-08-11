<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Examples

End-to-end examples demonstrating the [a2a-rust](https://github.com/tomtom215/a2a-rust) SDK. Each example is a standalone binary crate.

## Quick start

```bash
# Run the simplest example (no external dependencies):
cargo run -p echo-agent

# Run the full SDK test suite:
cargo run -p agent-team
```

## Examples

| Example | Description | External deps | Difficulty |
|---------|-------------|--------------|------------|
| [`incident-response/`](incident-response/) | **Start here** — three-agent team: multi-turn `INPUT_REQUIRED`, delegation, streaming progress, artifacts, cooperative cancellation; runs fully local |
| [**echo-agent**](echo-agent/) | All four bindings; drives every A2A method over each and asserts the coverage matrix | None | Beginner |
| [**agent-team**](agent-team/) | 4-agent team with 100 E2E tests; the SDK's dogfood suite | None | Advanced |
| [**genai-agent**](genai-agent/) | LLM-powered agent via [genai](https://crates.io/crates/genai); all four bindings, full surface matrix | Optional model | Intermediate |
| [`rig-agent/`](rig-agent/) | **Real rig-core agent** served over A2A — hosted OpenAI or any local OpenAI-compatible server (llama-server / Ollama via `OPENAI_BASE_URL`); passes the TCK 20/20 |
| [**multi-lang-team**](multi-lang-team/) | Rust coordinator delegating to Python, JS, Go and Java agents; all four bindings, full surface matrix | Optional workers | Advanced |

## What to start with

- **New to A2A?** Start with [`echo-agent`](echo-agent/) — it serves all four
  bindings behind one handler, drives **every** A2A method over **every**
  binding, and prints the resulting coverage matrix (currently 44 of 44 cells).
  It then runs counter-tests against a second, deliberately restricted agent to
  check the refusals the spec requires. No external dependencies.

  It exits `1` if any call or counter-test fails and `2` if any matrix cell was
  never exercised, so "covers everything" is a computation rather than a
  sentence. The matrix rows come from the ratified `a2a.proto`, not from the
  example — see `a2a_protocol_types::method`.

  Until 2026-08-11 this row claimed the example demonstrated "the complete
  request lifecycle" while it drove 4 of the 11 methods over 2 of the 4
  transports, with push notifications and the extended card unadvertised, so
  seven methods were unavailable on the server it started.

- **Evaluating the SDK?** Run [`agent-team`](agent-team/) — the dogfood suite,
  100 automated end-to-end tests across all four transports, printing a
  pass/fail report and a feature table computed from the results. It exits
  non-zero if any test fails, if the feature table drifts from the tests that
  back it, or if any claimed feature area was not compiled into the build.

  Until 2026-08-11 this row read "exercises every SDK feature with 81+
  automated tests". That was not measurable: no CI job ran the binary, 15 of
  its 86 tests were failing, and its feature table printed `[x]` for every row
  regardless of outcome. All three are fixed and the suite is now gated in CI
  (`ci.yml`, job `dogfood`).

- **Integrating an LLM?** See [`genai-agent`](genai-agent/) or [`rig-agent`](rig-agent/) for patterns that bridge LLM frameworks with A2A's `AgentExecutor` trait.

- **Cross-language interop?** See [`multi-lang-team`](multi-lang-team/) for a Rust coordinator that talks to agents in 4 other languages.

## Common patterns

All examples follow the same integration pattern:

```rust
// 1. Implement AgentExecutor
struct MyExecutor;
impl AgentExecutor for MyExecutor {
    fn execute(&self, ctx: &RequestContext, queue: &dyn EventQueueWriter) -> ... {
        queue.write(StatusUpdate { state: Working }).await?;
        // ... do work ...
        queue.write(ArtifactUpdate { artifact }).await?;
        queue.write(StatusUpdate { state: Completed }).await?;
    }
}

// 2. Build a handler
let handler = RequestHandlerBuilder::new(MyExecutor)
    .with_agent_card(card)
    .build()?;

// 3. Serve via any dispatcher
let dispatcher = JsonRpcDispatcher::new(handler);
// or: RestDispatcher, A2aRouter (Axum), gRPC
```

## Prerequisites

- Rust 1.93+ (MSRV)
- `protoc` only when using `--features grpc`
- See each example's README for additional requirements

## Surface coverage

Every example serves all four bindings and drives **every** A2A method over
each, then asserts the resulting matrix is complete. Measured 2026-08-11, all
six at **44 of 44 cells**:

| Example | Cells | Also |
|---|---|---|
| `echo-agent` | 44/44 | 5 counter-tests |
| `incident-response` | 44/44 | three-act narrative demo first |
| `agent-team` | 44/44 | + 102 E2E feature tests |
| `genai-agent` | 44/44 | LLM leg reported separately |
| `rig-agent` | 44/44 | LLM leg reported separately |
| `multi-lang-team` | 44/44 | worker reachability reported per language |

The rows are not this project's list. They come from `Method::ALL`, which
`a2a-protocol-types` asserts equal to `service A2AService` in the ratified
`proto/a2a_v1/a2a.proto`, and which `scripts/check_method_denominator.py`
cross-checks against the upstream `a2aproject/a2a-tck` on every Official TCK
run. A reviewer auditing "is this 11 the real 11?" reads the proto.

Exit codes are the gate: `0` complete, `1` a call or counter-test failed, `2`
a matrix cell never ran. Gated by `ci.yml`'s `example-surface` job, each leg
proven able to fail by injection.

**What a green run does *not* mean.** Three examples depend on something CI
does not have — an LLM provider, or worker agents in four other languages.
Each says so out loud rather than letting a full matrix imply otherwise:
`genai`/`rig` print `LLM leg: NOT EXERCISED` and label every fallback answer,
and `multi-lang-team` prints each worker as `not reachable`. The protocol
surface is measured either way; the integration is not.
