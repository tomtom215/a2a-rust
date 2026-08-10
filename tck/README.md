<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# A2A Technology Compatibility Kit (TCK)

Standalone conformance test runner that validates any A2A v1.0 server implementation.

## Overview

- Tests wire format interoperability across all A2A implementations
- Language-agnostic: tests servers written in Rust, Python, Go, JS, Java, or any language
- Validates all 11 A2A methods over all four bindings — JSON-RPC (§9), gRPC
  (§10), HTTP+JSON (§11), and the §12 WebSocket custom binding
- Grades §5.1 cross-binding equivalence (`BIND-EQUIV-001..004`), which no
  single-binding run can
- Checks ProtoJSON naming conventions, discriminated union serialization, security scheme formatting

### Why it does not use this repository's own client

The TCK depends on none of the `a2a-protocol-*` crates, and compiles the
canonical `a2a.proto` itself (see `build.rs`) rather than importing the
generated types. A conformance kit that drives the implementation it grades
shares that implementation's reading of the specification, and a symmetric
misreading passes on both sides of every assertion. Every request here is
built from the wire format up.

`scripts/check_proto_copies.sh` keeps `tck/proto` byte-identical to the other
vendored copies, so independence does not become drift.

## Usage

`--url` is always the agent's **HTTP origin**, whichever binding is being
graded: §5 serves the agent card over HTTP no matter what carries the RPCs.
For `websocket` and `grpc`, the listener's address is read from the card's
`WEBSOCKET` / `GRPC` interface — the same path a real client takes.

```bash
# JSON-RPC (§9) — the default
cargo run -p a2a-tck -- --url http://localhost:3000 --binding jsonrpc

# HTTP+JSON / REST (§11)
cargo run -p a2a-tck -- --url http://localhost:3000 --binding rest

# WebSocket (§12 custom binding)
cargo run -p a2a-tck -- --url http://localhost:3000 --binding websocket

# gRPC (§10)
cargo run -p a2a-tck -- --url http://localhost:3000 --binding grpc

# Cross-binding equivalence (§5.1) — drives every binding the card advertises
cargo run -p a2a-tck -- --url http://localhost:3000 --equivalence
```

For an agent that serves a listener without advertising it, name the endpoint
explicitly with `--ws-url ws://host:port` or `--grpc-url host:port`. Passing
either on a binding that will not dial it is a configuration error rather than
a silently ignored flag.

## Exit Codes

| Code | Meaning |
|------|---------|
| 0 | All tests passed |
| 1 | One or more tests failed, or a `--skip`ped test now passes |
| 2 | Configuration error, an undiscoverable endpoint, or a run that graded nothing |

A run that grades zero checks exits 2. Every check being skipped or ruled not
applicable is not a pass — it is a run that verified nothing, and reporting it
green is the failure mode `--min-graded` exists to catch in
`tck/scripts/check_conformance.py`.

## What It Tests

| Area | Details |
|------|---------|
| Methods | All 11 A2A v1.0 method request/response formats |
| Naming | ProtoJSON naming conventions (`SCREAMING_SNAKE_CASE` with type prefix) |
| Security | `SecurityRequirement` / `StringList` wrapper format |
| Unions | Discriminated union serialization (`SendMessageResponse`, `StreamResponse`) |
| Discovery | Agent card discovery (`/.well-known/agent-card.json`) |
| Errors | Error responses for invalid requests, and §5.4's per-binding error mapping |
| Equivalence | §5.1: identical operations, equivalent results, consistent error mapping, shared auth schemes |

### Not applicable is reported, never passed

A check that cannot apply to a binding — a JSON-RPC envelope assertion against
§11, an `application/a2a+json` Content-Type against a §12 text frame — is
reported `N/A` with its reason and excluded from the score. It is not counted
as a pass. Until 2026-08-10 one such check returned `Ok(())` for two bindings
it never ran against, so those runs reported 22/22 while 21 checks ran.

### gRPC has its own assertions, not a JSON adapter

The §10 checks are separate function bodies. Converting protobuf responses to
ProtoJSON and reusing the JSON assertions would be less code and would produce
checks that cannot fail: `task_state_values` asserts the state is one of the
nine `TASK_STATE_*` strings, and over gRPC that string only exists after this
crate's own enum-to-name mapping runs. The assertion would be about the
converter, not the server. Each gRPC check asserts what is observable at that
binding instead. Check *names* are shared across bindings so runs stay
comparable.

## Cross-binding equivalence (§5.1)

`--equivalence` grades four MUSTs that are statements about the relation
between bindings, each trivially satisfied by any one of them:

| ID | Requirement |
|---|---|
| `BIND-EQUIV-001` | All supported protocols provide the same set of operations |
| `BIND-EQUIV-002` | All bindings return semantically equivalent results for the same request |
| `BIND-EQUIV-003` | All bindings map errors consistently, using §5.4's per-binding codes |
| `BIND-EQUIV-004` | All bindings support the same authentication schemes declared in the AgentCard |

`BIND-EQUIV-004` is graded **structurally only**, and the run says so: the card
declares security once at card level and no interface may override it. Proving
each binding *enforces* those schemes identically needs a target configured to
require credentials.

Requirement texts are quoted from `a2aproject/a2a-tck@5996b79` (2026-06-29),
`tck/requirements/interop.py`. Note that upstream's backlog ticket `task-28`
summarises `BIND-EQUIV-004` as "Streaming equivalence" while `interop.py` — the
file the suite loads — defines it as the authentication requirement above; this
runner follows `interop.py`.

## Targets in this repository

| Target | Bindings served | Used by |
|---|---|---|
| `examples/echo-agent` | JSON-RPC, HTTP+JSON, WebSocket (with `A2A_WS_BIND_ADDR`) | `tck-self-test` |
| `tck/sut` | all four (`SUT_GRPC_HOST`, `SUT_WS_HOST`) | `tck-all-bindings`, equivalence, and the official suite |

The example stays an example: it serves three bindings and does not carry the
official suite's `messageId`-keyed behaviour contract. The SUT carries both.

## Integration Test Kit (ITK)

The TCK is used by the ITK (`itk/`) to cross-check agents written in Python, Go, JS, and Java.
See `itk/README.md` for Docker Compose orchestration.

## License

Apache-2.0
