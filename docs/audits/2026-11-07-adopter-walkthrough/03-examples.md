<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# 3. The examples

Scripts: `03-build-examples.sh`, `03-examples-offline.sh`, `03-hello-claims.sh`. Raw output:
`evidence/03-examples-offline.log`, `evidence/03-hello-claims.log`, `evidence/03-build-examples-*.log`.

Built from this checkout with `RUSTFLAGS=-D warnings`, debug profile. `examples/README.md` says each
example is a `publish = false` crate that depends on the SDK by path and must be run from the
repository root; every command below was run that way.

## Dependency lines: would an adopter write these?

No example's `[dependencies]` can be copied, because each names the SDK by `path = "../../crates/..."` and
takes `tokio` from `workspace = true`. That is stated, not hidden (`examples/README.md`). The feature lists
that an adopter would have to add are present and explicit in every manifest read: `hello-agent` and
`deploy-agent` depend on `a2a-protocol-sdk` alone (deploy-agent with `postgres`), `tool-loop-agent` with
`tool-loop`, `live-model` with `tool-loop`, `grpc`, `websocket` and `signing`, and the sibling-crate examples name
`websocket`, `grpc`, `otlp`, `auth-jwt` as they use them. `hello-agent`'s tokio features
(`rt-multi-thread`, `macros`, `net`) are a superset of the README Quick Start's (`rt-multi-thread`,
`macros`), which built and ran without `net`. Not done: a copy of any example's source against the unpacked
tarballs; only the README and book programs were built that way (`02-readme-and-book.md`).

## Run

| Example | What was run | Result |
|---|---|---|
| `hello-agent` | `cargo run -p hello-agent`, its README's `curl`, the card, SIGTERM; then its documented error cases (`03-hello-claims.sh`) | **pass.** `Hello, Ada!`, `TASK_STATE_COMPLETED`; no `A2A-Version` header: `-32009 VERSION_NOT_SUPPORTED`; exit 0 on SIGTERM. One README quote was off: the README and the book page said the wrong field name is rejected with `invalid params: message must contain at least one part`; the wire carries `-32602` and `message must contain at least one part` (fixed). A browser tab (`GET /`) gets a JSON-RPC `-32009` error as HTTP 200, which is correct for a JSON-RPC-only agent and looks odd |
| `deploy-agent` | `cargo run -p deploy-agent` with no database: its README's five `curl`s, `/health`, `/ready`, the card, `/message:send`, JSON-RPC on `/`; `SIGTERM` with `DRAIN_DELAY_SECONDS=3`; six invalid configurations | **pass.** `/health` and `/ready` 200; during the drain `/ready` is 503 and `/health` 200 and the process exits 0 with `drained cleanly, exiting`; `PORT=0`, `PORT=99999`, `PORT=abc`, `AGENT_URL=ftp://x`, `AGENT_VERSION="a b"`, `TASK_RETENTION_HOURS=1.5` each exit 1 at startup. The card advertises `http://localhost:8080` (`AGENT_URL` default) as the README says. The container and PostgreSQL modes: see `06-deployment.md` |
| `echo-agent` | `cargo run -p echo-agent` (debug) | **pass.** Exit 0 in 2 s: 44 of 44 cells, five counter-tests refused |
| `tool-loop-agent`, `live-model`, `genai-a2a-agent`, `rig-a2a-agent` | built (`-D warnings`, 3 m 25 s with the CPU shared); run against a model in `05-live-model.md` | see there |
| `incident-response`, `agent-team`, `resilient-agent`, `swarm`, `mcp-agent`, `mcp-bridge`, `multi-lang-team`, `langfuse-agent` | **not run**: see "Not run" below | |

## Not run

`incident-response`, `agent-team`, `resilient-agent`, `swarm`, `mcp-agent`, `mcp-bridge`,
`multi-lang-team` and `langfuse-agent` were neither built nor run: the shared disk allowance (about 3 GB free
at that point, with each of these adding hundreds of megabytes to the target directory) and a CPU at load
average 13 to 22 from other builds made the cost of each one higher than the information it would add beyond
the CI jobs that run seven of them (`example-surface`, `dogfood`, per `examples/README.md`). `langfuse-agent` needs
a Langfuse instance, `multi-lang-team` its Python/JS/Go/Java workers, `incident-response`'s hardening needs PostgreSQL.
No claim is made about them here beyond "CI says".
