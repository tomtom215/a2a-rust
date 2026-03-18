<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. -->

# Contributing to a2a-rust

Thank you for contributing! Please read this document before opening a PR.

---

## Coding Standards

### Every file starts with the SPDX header

```rust
// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.
```

Markdown / TOML / YAML files use the appropriate comment syntax.

### 500-line maximum per file

No source file (`.rs`) may exceed 500 lines. If your implementation is growing
beyond this limit, split it into focused sub-modules with a thin `mod.rs` that
only re-exports.

### Thin `mod.rs` files

`mod.rs` files should primarily contain `mod` declarations and `pub use`
re-exports. Shared types that are tightly coupled to a module's children
(e.g. `DispatchConfig` in `dispatch/mod.rs`) may live in the parent
`mod.rs` when splitting them out would add indirection without value.

### Lint directives on every crate root

Each crate's `lib.rs` must include:

```rust
#![deny(missing_docs)]
#![deny(unsafe_op_in_unsafe_fn)]
#![warn(clippy::all, clippy::pedantic, clippy::nursery)]
#![allow(clippy::module_name_repetitions)]
```

### No `unwrap()` in library code

Use `?`, `map_err`, `ok_or_else`, or explicit `match`. `expect()` is also
forbidden unless the message explains an invariant that is *impossible* to
violate at runtime (documented with `// SAFETY:` style comment).

### `unsafe` blocks

Every `unsafe` block must be preceded by a `// SAFETY:` comment explaining
exactly why the invariants required by the unsafe operation are upheld.

---

## Dependency Policy

Before adding a dependency:

1. Check `deny.toml` — `openssl-sys` and `reqwest` are banned.
2. Verify the license is in the allowlist (`Apache-2.0 | MIT | BSD-2-Clause | BSD-3-Clause | ISC | Unicode-DFS-2016 | Unicode-3.0 | Zlib | CC0-1.0 | CDLA-Permissive-2.0`).
3. Ask: can this be implemented in-tree in ≤ 100 lines? If yes, do that instead.
4. Use bounded version ranges: `>= x.y, < z` — no open-ended `>=`.

---

## Testing Guide

### Test Categories

| Category | Location | Command |
|---|---|---|
| Unit tests | `#[cfg(test)]` modules in source files | `cargo test --workspace` |
| Integration tests | `crates/*/tests/` | included in workspace test |
| TCK conformance | `crates/a2a-types/tests/tck_wire_format.rs` | `cargo test -p a2a-protocol-types --test tck_wire_format` |
| Property-based tests | `crates/a2a-types/tests/proptest_types.rs` | `cargo test -p a2a-protocol-types --test proptest_types` |
| Corpus-based JSON tests | `crates/a2a-types/tests/corpus_json.rs` | `cargo test -p a2a-protocol-types --test corpus_json` |
| Mutation tests | `mutants.toml` (workspace root) | `cargo mutants --workspace` |
| End-to-end examples | `examples/echo-agent`, `examples/agent-team` | `cargo run -p echo-agent` |
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
cargo test --workspace --features signing

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
cargo mutants --file crates/a2a-types/src/task.rs

# List mutants without running (dry-run)
cargo mutants --list --workspace
```

**Configuration** is in `mutants.toml` at the workspace root. It controls which
files are examined, which patterns are excluded (e.g., `Display`/`Debug` impls),
and timeout settings.

**Zero surviving mutants is required.** The mutation CI job
(`cargo mutants --workspace`) can be triggered manually via `workflow_dispatch`.
Nightly schedule and PR-gate triggers are currently disabled to save CI time.

When a mutant survives, the output shows the exact mutation and the file/line.
Add or strengthen tests to cover the gap, then re-run to confirm the mutant is
caught.

### Test Naming Convention

Tests follow the pattern: `{component}_{scenario}_{expected_outcome}`

Examples:
- `task_state_completed_is_terminal`
- `text_part_roundtrip_preserves_metadata`
- `jsonrpc_send_message_returns_task`

### Property-Based Tests (`proptest`)

Located in `crates/a2a-types/tests/proptest_types.rs`. These verify invariants
that must hold for all possible inputs:

- **TaskState** — round-trip, terminal classification, wire format prefix
- **Part** — serialization fidelity across text/raw/url variants
- **ID types** — Display consistency, equality contracts

### Corpus-Based JSON Tests

Located in `crates/a2a-types/tests/corpus_json.rs`. Each test deserializes a
representative JSON sample matching the A2A v1.0 wire format and verifies
`deserialize → serialize → deserialize` round-trip fidelity. Covers:

- Tasks (submitted, working, completed, failed)
- Messages (user, agent, multi-part)
- Parts (text, raw, url, data)
- Agent cards (minimal, with security)
- JSON-RPC requests and responses
- Stream events (status update, artifact update)

### Benchmarks (`criterion`)

| Benchmark | Crate | What it measures |
|---|---|---|
| `json_serde` | `a2a-protocol-types` | Serialize/deserialize AgentCard, Task, Message |
| `sse_parse` | `a2a-protocol-client` | SSE frame parsing (single, batch, fragmented) |
| `handler_bench` | `a2a-protocol-server` | Request handler throughput |

Run with `cargo bench -p a2a-protocol-types`, `cargo bench -p a2a-protocol-client`, or `cargo bench -p a2a-protocol-server`.

---

## Test Requirements

| Layer | Tool | Minimum |
|---|---|---|
| Unit | `#[test]` | Every public function |
| Async unit | `#[tokio::test]` | Every async function |
| Integration | `tests/` directory | Each crate |
| Property | `proptest` | Serde round-trips for all types |
| Mutation | `cargo-mutants` | Zero surviving mutants across all library crates |
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
cargo clippy --workspace --all-targets --features signing -- -D warnings
cargo test --workspace --features signing

# 6. With tracing feature (if changes touch tracing code)
cargo test --workspace --features tracing

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

- [ ] SPDX header on every new file
- [ ] No file exceeds 500 lines
- [ ] `cargo fmt --all` passes
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` passes
- [ ] `cargo test --workspace` passes
- [ ] `cargo doc --workspace --no-deps` passes without warnings
- [ ] New public types/functions have doc comments
- [ ] New code has tests
- [ ] `cargo mutants` shows zero surviving mutants for changed files
- [ ] `book/src/reference/pitfalls.md` updated if a non-obvious pitfall was encountered
- [ ] ADR created or updated if an architectural decision was made or revised

---

## License

By contributing, you agree that your contributions will be licensed under the Apache-2.0 license.
