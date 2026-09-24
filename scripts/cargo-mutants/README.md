<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# The cargo-mutants patch

`27.1.0-result-aliases-and-boxed-futures.patch` modifies cargo-mutants 27.1.0,
the mutation-testing tool this repository's `mutants.yml` runs. It is applied
by `scripts/install_cargo_mutants.sh` to the crates.io release tarball, which
is pinned by checksum. This file is the record an auditor needs: what the
patch changes and why, what it is licensed under, and the commands that prove
the tool CI runs is exactly this patch on exactly that release.

## Decision

**Kept in this repository; not proposed upstream.** Decided by the maintainer,
2026-09-24. A later cargo-mutants release is adopted only by re-deriving the
patch against it and re-running every proof below.

## Licence

The patch is **MIT**, the licence of cargo-mutants. Its context and removed
lines are cargo-mutants' own, Copyright 2021-2024 Martin Pool, as
`src/fnvalue.rs` states; its added lines are Copyright 2026 Tom F. and are
licensed MIT as well, so the patched tool — the only thing built from it — is
under one licence. It is not part of any published a2a-rust crate, and it is
listed in `NOTICE` as third-party material. Checked in the release tarball:
`Cargo.toml` declares `license = "MIT"`, and `LICENSE` reads "MIT License,
Copyright (c) 2021 Martin Pool".

The header lines at the top of the patch are ignored by `patch`, which starts
at the first `---` line; the apply proof below is run on the file as committed.

## What it changes, and why

Two gaps in stock 27.1.0's generation of "replace the function body with a
value" mutants, both measured on this repository's code:

1. **`Result` aliases.** Stock cargo-mutants recognises a `Result` only when
   the type's last path segment is spelled `Result` (`src/fnvalue.rs`), so for
   `-> ClientResult<T>` or `-> A2aResult<T>` it emits replacements such as
   `ClientResult::new()` that never compile. Measured 2026-09-23: 1,000 such
   unviable mutants over 184 functions, so "replace the body with
   `Ok(default)`" had never been graded for any of them (audit N9).
2. **Boxed futures.** Object-safe async trait methods return
   `Pin<Box<dyn Future<Output = T> + Send + 'a>>` — `TaskStore`,
   `ServerInterceptor`, `AgentExecutor` and every other extension point here.
   Stock 27.1.0 has no case for that type at all: 162 functions got no body
   replacement (audit N11).

The patch adds both cases to `src/fnvalue.rs` (a `Pin<Box<dyn Future>>`
yields `Box::pin(async move { <each replacement of T> })`; a type whose last
segment ends in `Result` yields `Ok(<each replacement>)`), and teaches
`src/visit.rs` that a boxed future already yielding its own replacement —
`Box::pin(async { Ok(()) })`, the entire body of a default `after` hook — is
the function unchanged rather than a mutant, where the stock text comparison
missed it for differing only in `async move` or `()`.

## Proof, re-run 2026-09-24 on the committed files

| What | Command | Result |
|---|---|---|
| The upstream tarball is the pinned one | `sha256sum cargo-mutants-27.1.0.crate` | `07072e7bcdeb425d5e5fdbfd9f15a2c749e23cb2edf5ef40aee5876760ae1cf9`, equal to `CARGO_MUTANTS_SHA256` in the installer |
| The patch applies exactly, header included | `patch -d cargo-mutants-27.1.0 -p1 --forward --fuzz=0 --no-backup-if-mismatch < 27.1.0-…patch` | exit 0; files `src/fnvalue.rs`, `src/visit.rs` |
| cargo-mutants' own tests pass without the patch | `cargo test --locked --bin cargo-mutants -- fnvalue visit` on the stock source | 74 passed, 0 failed |
| …and with it | the same, on the patched source | 78 passed, 0 failed |
| The four tests the patch adds | the difference between the two runs | `fnvalue::test::result_alias_named_like_result`, `fnvalue::test::pinned_boxed_future_yields_each_replacement`, `fnvalue::test::pin_of_something_else_is_not_a_boxed_future`, `visit::test::no_boxed_future_mutants_equivalent_to_source` |
| The installer builds and verifies it | `scripts/install_cargo_mutants.sh` | exit 0 in 4 min 40 s; prints `…/cargo-mutants carries the patch`; `cargo mutants --version` prints `cargo-mutants 27.1.0` |
| The binary CI uses is patched | the self-test the installer runs as its last step, in both `mutants.yml` jobs too (`mutants.yml` lines 320 and 969 run the installer) | lists `Ok(..)` for a `Result` alias and `Box::pin(async move{false})` for a boxed future, and no mutant of a body that already yields its replacement. That a stock 27.1.0 binary fails these checks was not re-run here; the stock unit-test run above lacks the four tests that exercise them |

The installed binary's own hash is not a proof of anything portable: it
depends on the toolchain that built it (rustc 1.98.1 here, SHA-256
`809f2abe…155f22`), and the build is not reproducible across toolchains.
The self-test is the check that holds on any toolchain, which is why CI runs
it rather than comparing a hash.

## Patch identity

SHA-256 of the committed patch, header included:
`478de84eead3876617a84e882b5ddb4ece4efd9c13afe139c9a850f7bc206c8f`
(`sha256sum scripts/cargo-mutants/27.1.0-result-aliases-and-boxed-futures.patch`).
A change to the file changes this value; update it here in the same commit.
