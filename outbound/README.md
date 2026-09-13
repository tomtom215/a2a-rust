<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Outbound patch — zavora-ai/adk-rust

Held here so it survives the session that produced it. **Not destined for `main`.**

`adk-rust-0001-bump-a2a-protocol-types-0.12.patch` upgrades `adk-server`'s optional
`a2a-protocol-types` dependency from `^0.5` to `^0.12` and adapts the three call
sites the intervening releases broke. `adk-server` is the only external crate in
the registry that depends on any of ours, and the pin had never been bumped since
it landed in their #273 on 2026-04-12.

- **Base commit:** `9d177bfa` on `zavora-ai/adk-rust` `main` (2026-09-13).
  The patch was rebased onto it and `git apply --check`s clean there.
- **Commit:** `6e2a67d3`, authored and signed off as Tom F.
- **Contents:** 26 files, +106/-59. Three source fixes, two CHANGELOG entries,
  two example manifests, and all eighteen lockfiles that carry the crate.

Applying it, once a fork of `zavora-ai/adk-rust` exists:

```sh
git checkout -b fix/a2a-protocol-types-0.12 9d177bfa
git am adk-rust-0001-bump-a2a-protocol-types-0.12.patch
```

Verified on the 1.95.0 toolchain (their declared `rust-version`) against
`a2a-protocol-types` 0.12.0 resolved from crates.io: `cargo fmt --check` clean,
`cargo clippy --workspace --all-targets -- -D warnings` exit 0, the same for
`-p adk-server --features a2a-v1`, and `cargo test -p adk-server --features a2a-v1`
at 369 passed / 0 failed / 20 ignored across 25 binaries. The last two are the pair
their `feature-coverage` matrix runs for `{ package: adk-server, features: a2a-v1 }`.

The issue text and PR body that accompany this are **not** stored here.
