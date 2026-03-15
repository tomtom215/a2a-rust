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

`mod.rs` files contain only `mod` declarations and `pub use` re-exports.
No logic, no structs, no impls.

### Lint directives on every crate root

Each crate's `lib.rs` must include:

```rust
#![warn(missing_docs)]
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
2. Verify the license is in the allowlist (`Apache-2.0 | MIT | BSD-*`).
3. Ask: can this be implemented in-tree in ≤ 100 lines? If yes, do that instead.
4. Use bounded version ranges: `>= x.y, < z` — no open-ended `>=`.

---

## Test Requirements

| Layer | Tool | Minimum |
|---|---|---|
| Unit | `#[test]` | Every public function |
| Async unit | `#[tokio::test]` | Every async function |
| Integration | `tests/` directory | Each crate |
| Property | `proptest` | Serde round-trips for all types |
| E2E | real HTTP, `wiremock` | Client ↔ server interaction |

---

## PR Checklist

- [ ] SPDX header on every new file
- [ ] No file exceeds 500 lines
- [ ] `cargo fmt --all` passes
- [ ] `cargo clippy --all-targets -- -D warnings` passes
- [ ] `cargo test --workspace` passes
- [ ] `cargo doc --workspace --no-deps` passes without warnings
- [ ] `LESSONS.md` updated if a non-obvious pitfall was encountered
- [ ] ADR created or updated if an architectural decision was made or revised
