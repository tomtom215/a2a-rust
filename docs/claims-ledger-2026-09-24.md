<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Claims ledger, 2026-09-24

Every public claim this project makes that could be an overstatement, with
the evidence that supports it or the fact that nothing does. Written so that
no public statement — to an adopter, a foundation reviewer, or the next
release notes — rests on a claim nobody checked.

## Method

A read-only pass collected 95 candidate claims from `README.md`, the crate
READMEs, the book, `SPEC_COMPLIANCE.md`, `SECURITY.md`, `CITATION.cff`,
`docs/lf-readiness-review.md`, the GitHub release notes for v0.6.0 to
v0.13.0, and this repository's `CHANGELOG.md`. It also collected the
**published** artifacts: all 36 `.crate` files for the four crates at every
version from 0.6.0 (0.6.0, 0.7.0, 0.8.0, 0.9.0, 0.10.0, 0.11.0, 0.12.0,
0.12.1, 0.13.0), from which the README, `src/lib.rs` and `Cargo.toml` were
extracted and diffed version to version. Each claim got a proposed verdict.

Every **false** and **unsupported** verdict below was then re-checked
against the artifact itself — the published `.crate`, the source file, the
workflow — before anything was changed. The verdicts are:

- **supported** — a test, gate or recorded measurement backs it;
- **unsupported** — nothing contradicts it, and nothing backs it either;
- **false** — the artifact it describes contradicts it;
- **stale** — true when written, no longer.

Commit references are on `claude/peaceful-ptolemy-noka5b`.

## False or unsupported, and what was done

| Claim | Where | Evidence | Verdict | Fix |
|---|---|---|---|---|
| "Zero surviving mutants enforced via `cargo-mutants` CI gate" | book introduction | The PR gate is `--in-diff` (`mutants.yml`); the weekly sweep recorded 55 survivors on 2026-09-07 (`book/src/reference/mutation-history.md`) | false | Rewritten to what the gate covers |
| "The entire codebase is free of unsafe code"; "zero `unsafe` code" | book introduction | Five `build.rs` files (`crates/a2a-protocol-{types,client,server}`, `tck`, `itk`) and `benches/benches/memory_overhead.rs` use `unsafe` | false | Rewritten: no `unsafe` in library code, and where it is |
| "zero `unsafe` blocks anywhere in this crate" | `a2a-protocol-types` README, **published 0.12.0–0.13.0** | The published `.crate` files each contain `build.rs` with `unsafe { std::env::set_var("PROTOC", …) }` (checked by extracting all nine 0.12.x/0.13.0 crates) | false | Repo README fixed; published text recorded in the CHANGELOG correction |
| "no panics on any caller input or I/O failure"; "never panics on caller input" | book introduction; `client/error-handling.md` | `check_panic_paths.py` freezes `unwrap`/`expect`/`panic!`/`todo!`; it did not count `unreachable!`, and nothing measures overflow or indexing | unsupported | Gate now counts `unreachable!`; the one in library code removed; both pages rewritten to what the gate measures |
| "Exhaustive pattern matching — The compiler catches missing protocol states" | book introduction | `TaskState` and the other protocol enums are `#[non_exhaustive]` | false | Rewritten |
| "Zero-cost abstractions … with no runtime overhead" | book introduction | The executor is `Arc<dyn AgentExecutor>` returning a boxed future | false | Rewritten |
| "All defaults … are overridable via builders" | book introduction | `MAX_CARD_BODY_SIZE` (client, 2 MiB) is `pub(crate) const` with no setter | false | Rewritten, naming the exception |
| "All public types implement `Send + Sync`" | book introduction | Asserted for eight types (`reexport_tests.rs`) | unsupported | Rewritten to the eight |
| "Every A2A type with correct JSON serialization" | book introduction | Golden fixtures and the official TCK cover much; wire-shape defects have been found after release (`docs/official-tck-findings.md` §7, §8, §13) | unsupported | Rewritten |
| `A2A_VERSION` — `"1.0.0"` | `a2a-protocol-types` README, **published 0.7.0–0.13.0** | `pub const A2A_VERSION: &str = "1.0"` in each | false | CHANGELOG correction (repo README was already right) |
| `resubscribe()`, `get_authenticated_extended_card()`, `ClientBuilder::with_transport()` | `a2a-protocol-client` README, **published, every version** | No such methods in any published source | false | CHANGELOG correction (repo README was already right) |
| `signing` = "Agent card signature verification" | client and server READMEs, **published, every version** | Both crates only forward `a2a-protocol-types/signing`; neither contains `signing`-gated code | false | CHANGELOG correction (repo READMEs were already right) |
| "Complete server framework for building A2A-compliant agents" | server README, published and repo | Official TCK: 88 of 114 MUSTs pass across the three profiles (84 in the full profile), 4 fail, 22 unmeasured | unsupported | Rewritten |
| "communicating with any A2A-compliant agent" | client README and `lib.rs` | v0.3-only agents are not reached (the client README says so) | unsupported | Rewritten |
| "A complete Rust implementation" | `CITATION.cff` | as above | unsupported | Rewritten |
| "the four `build.rs` files" use `unsafe` | `README.md`; 0.12.0 release notes | There are five | false | README fixed; notes recorded |
| "`from_str/16384` at 75 %" override | `README.md` | `benchmarks.yml` excludes it outright; its comment says 75 % was tried and failed | false | Rewritten |
| Hello Agent: "35 lines, one dependency" (README), "37 lines" (book) | `README.md`, `examples/overview.md` | 28 non-blank, non-comment lines above `#[cfg(test)]`, counted; it also depends on `tokio` | false | Both rewritten with the count and its method |
| Multi-language team "proving cross-language A2A interoperability" | `README.md` | CI runs it with every worker unreachable, and says so | unsupported | Rewritten, pointing at the jobs that do measure it |
| In-repo TCK runs over "JSON-RPC and REST" only | `README.md`, `SPEC_COMPLIANCE.md` | `tck.yml` runs it over all four bindings | stale | Rewritten |
| "Published crates — Signed? Yes (by crates.io)" | `SECURITY.md` | A registry checksum is integrity, not a signature | false | Rewritten |
| "81+ E2E tests"; "~1,630 tests" | book examples overview; `deployment/testing.md` | `agent-team` prints 102; the test count has more than doubled | stale | Current figure where current; dated where historical |
| "88/88 graded MUSTs pass … the baseline is empty. This is done." | `reference/conformance-history.md` | Four MUSTs are baselined as failing since v1.0.1 | stale | Marked superseded in place |
| "No `unsafe` anywhere in the published crates"; "MSRV 1.93" | `docs/lf-readiness-review.md` (dated 2026-08-26) | See above; MSRV is 1.88 since 2026-09-09 | stale | Dated correction note at the top |
| "Official-SDK interop is now proven … 20/20 … for every one" | v0.7.0 release notes | JavaScript and Java were scored after skipping checks they fail | unsupported as worded | CHANGELOG correction |
| "MUST compatibility **100%**" | v0.8.0 release notes | True of that run; four MUSTs fail since v1.0.1 | stale | CHANGELOG correction |
| `TaskSubscription` as the operation name | server `lib.rs` | The v1.0 operation is `SubscribeToTask` | false | Fixed |
| `ServerInterceptor::after` "is called even if the handler returned an error" | server `interceptor.rs` | No method calls it on error (found while fixing N28) | false | Rewritten |

## Supported, and what supports it

| Claim | Evidence |
|---|---|
| Official TCK 88/114 MUSTs pass, 4 failing — the union of the three profiles; the full profile grades 88 (84 pass, 4 fail) | `tck/conformance-baseline.json`; `official-tck.yml --min-graded 88`; re-run on `main` at a2a-tck `263b9cf`, 2026-09-24 (see the handoff) |
| Four bindings, eleven methods | `tck.yml` grades the in-repo runner on all four; `--equivalence` |
| MSRV 1.88 | `Cargo.toml`; `ci.yml` matrix `[stable, "1.88"]` on three platforms |
| `cargo-semver-checks` on every pull request | `ci.yml` |
| Release artifacts match crates.io | The `SHA256SUMS` on the v0.9.0, v0.12.0 and v0.13.0 releases equal the crates.io checksums for all four crates |
| The `rig` and `genai` agents pass the in-repo TCK | `tck.yml`'s `tck-example-agents` job |

## Not verified

- Book pages beyond the introduction and the pages the vocabulary search
  hit; `docs/rust-sdk-assessment.md`; the body of
  `docs/official-tck-findings.md`.
- The README's test count (3,601 on 2026-09-20): no stored artifact backs
  it; it is re-measured in the handoff's verification of record.
- `agent-team`'s "87 with `--no-default-features`", which no CI job runs.
- The ~3,600 req/s figure in `docs/lf-readiness-review.md`, from a one-off
  run.
- Upstream acknowledgement of a2a-tck#231.

## What cannot be fixed from here

The crates.io READMEs and docs.rs pages for 0.6.0 to 0.13.0 carry the
published false claims above until the next release replaces them as the
current version, and they stay attached to those versions afterwards. The
GitHub release notes for v0.7.0, v0.8.0 and v0.12.0 carry theirs until
someone edits them; this session had no means to, and the corrections are
recorded in `CHANGELOG.md` instead, where the next release's notes will
carry them.
