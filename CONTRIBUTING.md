<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Contributing to a2a-rust

Thank you for contributing! Please read this document before opening a PR.

---

## Developer Certificate of Origin (DCO)

Every contribution to this project must be certified under the
[Developer Certificate of Origin](DCO) version 1.1. The DCO is a short
statement that you wrote the contribution, or otherwise have the right to
submit it under this project's Apache-2.0 licence. It is not a copyright
assignment and it does not require signing a separate agreement — you certify
it per commit, by adding a `Signed-off-by:` line:

```sh
git commit -s -m "your message"
```

which appends:

```
Signed-off-by: Your Name <your.email@example.com>
```

The name and email must be real and must match your git author identity. Set
them once with:

```sh
git config user.name  "Your Name"
git config user.email "your.email@example.com"
```

### Fixing a missing sign-off

```sh
# The most recent commit
git commit --amend -s --no-edit && git push --force-with-lease

# Every commit on your branch
git rebase --signoff origin/main && git push --force-with-lease
```

### AI-assisted contributions

AI assistance is welcome, and this project uses it heavily — see
[`PROVENANCE.md`](PROVENANCE.md) for a full disclosure of how the existing code
was produced.

Two rules apply:

1. **You are the author.** Commit as yourself, not as the assistant. A DCO
   sign-off is an assertion by a person; a commit whose git author is a tool
   identity cannot carry a meaningful one. Credit the assistant in a trailer:

   ```
   Signed-off-by: Your Name <your.email@example.com>
   Co-Authored-By: Claude <noreply@anthropic.com>
   ```

2. **You are responsible for it.** Signing off means you have reviewed the
   code, you understand what it does, and you have the right to submit it —
   regardless of what typed it.

CI enforces both rules (`.github/workflows/dco.yml`): a pull request fails if
any non-merge commit lacks a matching sign-off, or is authored by a known
assistant identity.

---

## Coding Standards

### Every file starts with the SPDX header

```rust
// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
```

Markdown / TOML / YAML files use the appropriate comment syntax.

### 500-line maximum per file

Source files (`.rs`) should generally stay under 500 lines. If your implementation
is growing beyond this limit, consider splitting it into focused sub-modules with
a thin `mod.rs` that only re-exports. Some files exceed this guideline where
splitting would harm cohesion.

### Thin `mod.rs` files

`mod.rs` files should primarily contain `mod` declarations and `pub use`
re-exports. Shared types that are tightly coupled to a module's children
(e.g. `DispatchConfig` in `dispatch/mod.rs`) may live in the parent
`mod.rs` when splitting them out would add indirection without value.

### Lint directives on every crate root

Each crate's `lib.rs` must include:

```rust
#![deny(missing_docs)]
#![forbid(unsafe_code)]
#![warn(clippy::all, clippy::pedantic, clippy::nursery)]
#![allow(clippy::module_name_repetitions)]
```

`forbid(unsafe_code)` is a compiler-level guarantee that no `unsafe` block,
`unsafe fn`, or `unsafe impl` can ever be introduced without first removing
this attribute — which would have to appear in a PR diff and be reviewed.

### No `unwrap()` in library code

Use `?`, `map_err`, `ok_or_else`, or explicit `match`. `expect()` is also
forbidden unless the message explains an invariant that is *impossible* to
violate at runtime (documented with `// SAFETY:` style comment).

### `unsafe` blocks

Library crates are under `#![forbid(unsafe_code)]`; introducing an `unsafe`
block in one of the published crates is a compile error. The only
`unsafe`-bearing file in the repository is the counting global allocator in
`benches/benches/memory_overhead.rs`, where the `GlobalAlloc` trait cannot
be implemented safely. Any `unsafe` block in that file must be preceded by
a `// SAFETY:` comment explaining exactly why the invariants required by
the unsafe operation are upheld.

---

## Dependency Policy

Before adding a dependency:

1. Check `deny.toml` — `openssl-sys` and `reqwest` are banned.
2. Verify the license is in the allowlist (`Apache-2.0 | MIT | BSD-2-Clause | BSD-3-Clause | ISC | Unicode-DFS-2016 | Unicode-3.0 | Zlib | CC0-1.0 | CDLA-Permissive-2.0`).
3. Ask: can this be implemented in-tree in ≤ 100 lines? If yes, do that instead.
4. Use bounded version ranges: `>= x.y, < z` — no open-ended `>=`.

---

## Testing

### Writing reliable async tests

Fixed sleeps as synchronization are not accepted — they trade flakiness
under CI load for dead time on every run. In order of preference:

1. **Handshakes**: signal the state change itself (`tokio::sync::Notify`
   fired after the write, a flag flipped by the executor) and wait on the
   signal. See `NotifyOnSaveStore` / `wait_for_signalled` in
   `crates/a2a-protocol-server/tests/event_processing_tests/main.rs`.
2. **Deadline polls**: when no hook exists, poll the observable condition
   with a generous deadline (~10s) and a tight step (~2ms) — returns the
   moment the state lands, and only a genuine hang outlasts the deadline.
   See `wait_for` in `tests/stress_tests.rs`.
3. Fixed sleeps are acceptable only where the sleep itself is the test
   subject (TTL expiry uses `sleep`'s minimum-duration guarantee with the
   sleep strictly longer than the TTL), as simulated work inside test
   executors, or as outer timeout guards in `select!`.

Never assert an asynchronous counter or store state immediately after a
fixed sleep — that is the flaky-test signature this policy exists to
prevent.

### Test Categories

| Category | Location | Command |
|---|---|---|
| Unit tests | `#[cfg(test)]` modules in source files | `cargo test --workspace` |
| Integration tests | `crates/*/tests/` | included in workspace test |
| TCK conformance | `crates/a2a-protocol-types/tests/tck_wire_format.rs` | `cargo test -p a2a-protocol-types --test tck_wire_format` |
| Property-based tests | `crates/a2a-protocol-types/tests/proptest_types.rs` | `cargo test -p a2a-protocol-types --test proptest_types` |
| Corpus-based JSON tests | `crates/a2a-protocol-types/tests/corpus_json.rs` | `cargo test -p a2a-protocol-types --test corpus_json` |
| Mutation tests | `mutants.toml` (workspace root) | `cargo mutants --workspace` |
| End-to-end examples | `examples/echo-agent`, `examples/agent-team`, `examples/multi-lang-team`, `examples/rig-agent`, `examples/genai-agent`, `examples/incident-response` | `cargo run -p echo-agent` |
| Benchmarks | `crates/*/benches/` | `cargo bench` |

### Running Tests

```bash
# Run all tests
cargo test --workspace

# Run all tests including feature-gated code (sqlite, otel, axum, etc.)
cargo test --workspace --features "a2a-protocol-server/axum"

# Run tests for a specific crate
cargo test -p a2a-protocol-types
cargo test -p a2a-protocol-client
cargo test -p a2a-protocol-server
cargo test -p a2a-protocol-sdk

# Run tests with signing feature
cargo test --workspace --features a2a-protocol-sdk/signing

# Run a specific test
cargo test -p a2a-protocol-types task_state_roundtrip

# Run benchmarks
cargo bench -p a2a-protocol-types
cargo bench -p a2a-protocol-client
cargo bench -p a2a-protocol-server
```

### Mutation Testing

Mutation testing verifies that your tests actually detect code changes. A mutant
is a small, deliberate modification to the source (e.g., replacing `+` with `-`,
flipping a boolean, returning a default value). If a mutant compiles and all
tests still pass, the test suite has a gap.

```bash
# Install cargo-mutants
cargo install cargo-mutants

# Run mutation tests on all library crates
cargo mutants --workspace

# Run on a specific crate
cargo mutants -p a2a-protocol-types

# Run on a specific file
cargo mutants --file crates/a2a-protocol-types/src/task.rs

# List mutants without running (dry-run)
cargo mutants --list --workspace
```

**Configuration** is in `mutants.toml` at the workspace root. It controls which
files are examined, which patterns are excluded (e.g., `Display`/`Debug` impls),
and timeout settings.

**Zero surviving mutants is the standard**, enforced at two different
scopes. Know which one applies to you:

| Gate | Scope | When | Blocking? |
|---|---|---|---|
| `Mutation Testing (incremental)` | mutants in the lines your PR changed (`--in-diff`) | every pull request | **yes** |
| `Mutants Summary` | the whole workspace | weekly, and on `workflow_dispatch` | no — it reports |

**What a contributor is accountable for is the first row.** Your PR must add no
surviving mutants to the code it touches. That check is required and it works.

The workspace sweep is a different matter, and honesty about it belongs here
rather than in a footnote. **It is currently red: 183 surviving mutants,
92% caught**, measured 2026-08-07. That is pre-existing debt in code no recent
PR has touched, not a bar newcomers are being held to, and it is being burned
down rather than suppressed — there is deliberately no baseline file, because
the incremental gate above already prevents the count from growing.

The score and its history are in
[`book/src/reference/mutation-history.md`](book/src/reference/mutation-history.md),
which also records why the ledger was empty until 2026-08-07: both gates were
structurally incapable of failing, and reported `100%` over reports they were
never reading. Treat a green mutation check as meaningful only from that date
on.

When a mutant survives, the output shows the exact mutation and the file/line.
Add or strengthen tests to cover the gap, then re-run to confirm the mutant is
caught. If you believe a mutant is genuinely *equivalent* — semantically
identical to the original, so no test can distinguish it — see
[ADR 0006](docs/adr/0006-mutation-testing.md#equivalent-mutants) before
reaching for an exemption. The bar is "no test can distinguish it", not "I
could not think of one", and the mechanism has a dependency prerequisite the
workspace does not yet carry — so raise it rather than adding it in passing.

### Test Naming Convention

Tests follow the pattern: `{component}_{scenario}_{expected_outcome}`

Examples:
- `task_state_completed_is_terminal`
- `text_part_roundtrip_preserves_metadata`
- `jsonrpc_send_message_returns_task`

### Property-Based Tests (`proptest`)

Located in `crates/a2a-protocol-types/tests/proptest_types.rs`. These verify invariants
that must hold for all possible inputs:

- **TaskState** — round-trip, terminal classification, wire format prefix
- **Part** — serialization fidelity across text/raw/url variants
- **ID types** — Display consistency, equality contracts

### Corpus-Based JSON Tests

Located in `crates/a2a-protocol-types/tests/corpus_json.rs`. Each test deserializes a
representative JSON sample matching the A2A v1.0 wire format and verifies
`deserialize → serialize → deserialize` round-trip fidelity. Covers:

- Tasks (submitted, working, completed, failed)
- Messages (user, agent, multi-part)
- Parts (text, file, data)
- Agent cards (minimal, with security)
- JSON-RPC requests and responses
- Stream events (status update, artifact update)

### Benchmarks (`criterion`)

| Benchmark | Crate | What it measures |
|---|---|---|
| `json_serde` | `a2a-protocol-types` | Serialize/deserialize AgentCard, Task, Message |
| `sse_parse` | `a2a-protocol-client` | SSE frame parsing (single, batch, fragmented) |
| `handler_bench` | `a2a-protocol-server` | Request handler throughput |
| `protocol_overhead` | `a2a-benchmarks` | JSON-RPC envelope serialization/deserialization; `protocol/payload_scaling` isolation benchmarks (64B-1MB, `to_vec` vs `SerBuffer`, `from_slice` vs `from_str`) |
| `cross_language` | `a2a-benchmarks` | Standardized workloads for cross-SDK comparison |
| `transport_throughput` | `a2a-benchmarks` | End-to-end HTTP round-trip latency |
| `concurrent_agents` | `a2a-benchmarks` | Scaling behavior under parallel load |
| `realistic_workloads` | `a2a-benchmarks` | Production-like usage patterns |
| `error_paths` | `a2a-benchmarks` | Error handling performance |
| `backpressure` | `a2a-benchmarks` | Stream throughput under varying loads |
| `data_volume` | `a2a-benchmarks` | Store performance at scale (1K–100K tasks) |
| `memory_overhead` | `a2a-benchmarks` | Heap allocation counts per operation |
| `task_lifecycle` | `a2a-benchmarks` | TaskStore and EventQueue operations |
| `enterprise_scenarios` | `a2a-benchmarks` | Multi-tenant, push config, eviction, rate limiting, CORS |
| `production_scenarios` | `a2a-benchmarks` | Full E2E production workflows and race conditions |
| `advanced_scenarios` | `a2a-benchmarks` | Tenant resolver, agent card discovery, fan-out, artifact accumulation |

Run with `cargo bench -p a2a-protocol-types`, `cargo bench -p a2a-protocol-client`, `cargo bench -p a2a-protocol-server`, or `cargo bench -p a2a-benchmarks`.

> **Note:** The `grpc` feature requires `protoc` to be installed. See the [Installation](book/src/getting-started/installation.md) page for details.

---

## Test Requirements

| Layer | Tool | Minimum |
|---|---|---|
| Unit | `#[test]` | Every public function |
| Async unit | `#[tokio::test]` | Every async function |
| Integration | `tests/` directory | Each crate |
| Property | `proptest` | Serde round-trips for all types |
| Mutation | `cargo-mutants` | Zero surviving mutants in the lines a PR changes (enforced); zero across the workspace is the standing target — see [Mutation Testing](#mutation-testing) |
| E2E | real HTTP | Client ↔ server interaction |

---

## Quality Gates

All gates must pass before merging:

```bash
# 1. Format check
cargo fmt --all -- --check

# 2. Lint (zero warnings required)
cargo clippy --workspace --all-targets -- -D warnings

# 3. All tests pass
cargo test --workspace

# 4. Documentation builds without warnings
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps

# 5. With signing feature (if changes touch signing code)
cargo clippy --workspace --all-targets --features a2a-protocol-sdk/signing -- -D warnings
cargo test --workspace --features a2a-protocol-sdk/signing

# 6. With tracing feature (if changes touch tracing code)
cargo test --workspace --features a2a-protocol-sdk/tracing

# 7. With TLS feature (if changes touch TLS code)
cargo test -p a2a-protocol-client --features tls-rustls

# 8. With axum feature (if changes touch dispatch/axum code)
cargo clippy -p a2a-protocol-server --features axum -- -D warnings
cargo test -p a2a-protocol-server --features axum

# 9. Mutation testing (zero surviving mutants in changed files)
cargo mutants --workspace
```

---

## PR Checklist

- [ ] Every commit signed off (`git commit -s`) by a human author — see [DCO](#developer-certificate-of-origin-dco)
- [ ] SPDX header on every new file
- [ ] No **new** file exceeds 500 lines, and no file you touched crosses it —
      or the PR says why splitting would harm cohesion (see
      [500-line maximum](#500-line-maximum-per-file); 46 of 139 existing
      sources already exceed it, so this is a rule for new work, not a
      claim about the tree)
- [ ] `cargo fmt --all` passes
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` passes
- [ ] `cargo test --workspace` passes
- [ ] `cargo doc --workspace --no-deps` passes without warnings
- [ ] New public types/functions have doc comments
- [ ] New code has tests
- [ ] `cargo mutants --in-diff` shows zero surviving mutants for the lines
      this PR changes (this is the blocking gate; the workspace sweep is
      pre-existing debt and not yours to clear)
- [ ] `book/src/reference/pitfalls.md` updated if a non-obvious pitfall was encountered
- [ ] ADR created or updated if an architectural decision was made or revised

---

## License

By contributing, you agree that your contributions will be licensed under the
Apache-2.0 license, and you certify the [Developer Certificate of Origin](DCO)
for each commit via its `Signed-off-by:` trailer.
