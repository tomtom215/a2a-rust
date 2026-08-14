<!-- SPDX-License-Identifier: Apache-2.0 -->

## What this changes

<!-- What does this PR do, and why? Link any related issue. -->

## How it was verified

<!-- Which checks did you run locally? Paste relevant output if useful. -->

## Checklist

- [ ] **Every commit is signed off** (`git commit -s`) by a human git author —
      see [DCO](../DCO) and [CONTRIBUTING](../CONTRIBUTING.md#developer-certificate-of-origin-dco)
- [ ] SPDX header on every new file
- [ ] No file exceeds 500 lines
- [ ] `cargo fmt --all` passes
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` passes
- [ ] `cargo test --workspace` passes
- [ ] `cargo doc --workspace --no-deps` passes without warnings
- [ ] New public types/functions have doc comments
- [ ] New code has tests
- [ ] `cargo mutants --test-tool=nextest -- --all-features` shows zero surviving
      mutants for changed files (`--test-tool=nextest` is required — see
      CONTRIBUTING.md)
- [ ] `CHANGELOG.md` updated if the change is user-visible
- [ ] ADR created or updated if an architectural decision was made or revised
