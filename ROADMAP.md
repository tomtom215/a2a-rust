<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Roadmap

Current release: **0.7.0**. MSRV **1.93**.

## What this file is

Every item below is derived from something already committed to this
repository — a `#[deprecated]` attribute, a documented removal, a measured
gap, a workflow that does not yet do what it claims. Nothing here is an
invented milestone, and no dates are asserted, because sequencing and
priority are the maintainer's call, not a thing to infer from the tree.

Items are grouped by whether the work is *already decided* (0.8 removals that
the code and docs commit to), *decided but unscheduled*, or *open questions*
that need a maintainer decision before any work starts.

## 0.8 — removals this repository has already committed to

These are announced in the code and docs, so 0.8 is a breaking release
regardless of what else lands. Each is a `#[deprecated]` attribute or an
explicit "removal planned for 0.8" note today.

| Item | Where it is announced |
|---|---|
| Remove the `grpc-legacy-json` feature and the pre-0.7 JSON-tunnel gRPC service it serves | `crates/README.md`, `proto/README.md`, `book/src/building-agents/dispatchers.md`, `docs/adr/0009-protobuf-native-grpc.md` |
| Remove `with_event_queue_write_timeout` — a deprecated no-op; queue writes never block, and slow consumers get an explicit lag error | `crates/a2a-protocol-server/src/builder.rs`, `.../streaming/event_queue/manager.rs`, `book/src/reference/configuration.md`, `book/src/building-agents/handler.md` |
| Stop sending the legacy bare `a2a-notification-token` header; keep only the canonical `X-A2A-Notification-Token` | `CHANGELOG.md` (0.7.0 entry) |

Removing the legacy gRPC tunnel also deletes `dispatch/grpc/service.rs`,
which is the file whose thin test coverage prompted the 2026-07-31 defect
hunt. Worth sequencing so that effort is not spent twice.

## Verification debt

Work where the project's own gates do not yet measure what they claim to.
This is the category most worth clearing before any external review.

* **~~Land one complete mutation sweep.~~ Done 2026-08-07** — run 31209868659,
  all 21 shards complete, aggregated by CI: **92%**, 2168 caught / 183 missed.
  Reproduced across two different shardings, which is why the number is
  trustworthy rather than merely produced.
  Getting there took three rounds of gate fixes, because each one exposed the
  next. The 2026-07-31 diagnosis — two shards lost to the job timeout — was wrong:
  re-checked on 2026-08-06 against the 2026-07-27 run's own artifacts, the
  nine shards that *completed* also reported `Missed: 0`, while holding **200
  surviving mutants** between them. Both gates, the weekly sweep and the
  PR-blocking `--in-diff` check, were structurally incapable of failing:
  cargo-mutants wrote to `mutants.out/mutants.out/` and every reader looked in
  `mutants.out/`. Fixed 2026-08-06, along with a no-data gate so an empty
  denominator can never again be scored as 100%.
  A third defect surfaced only once the gates worked: the completeness check
  read the matrix `result`, so a sweep whose shards correctly failed on
  survivors refused to aggregate. Completeness is now a per-shard `COMPLETED`
  marker written inline when cargo-mutants returns.
  See [`book/src/reference/mutation-history.md`](book/src/reference/mutation-history.md).
* **~~Record the first real mutation score.~~ Done** — the ledger's first row
  is 2026-08-07. Keep it current: a row per completed sweep, including clean
  ones.
* **Burn down the 183 surviving mutants.** The weekly sweep now fails until
  they are killed or explicitly justified, which is the intended state, not a
  problem to suppress. No baseline file: the `--in-diff` PR gate already
  prevents new code from adding survivors, so the count can only fall. Largest
  clusters are `handler/messaging.rs` (17), `store/task_store/in_memory/eviction.rs`
  (13) and `dispatch/grpc/native.rs` (11).
* **Decide the wording of the zero-survivor rule.** `CONTRIBUTING.md` and ADR
  0006 both require "zero surviving mutants", which the tree has never met and
  which is not literally reachable — equivalent mutants cannot be killed. The
  honest form is *zero unexplained survivors*, each exception carrying an
  in-source `#[mutants::skip]` and a reason.
* **Raise coverage on the genuinely weak files.** After the 2026-07-31 pass,
  the weakest are `handler/event_processing/background/mod.rs` (54.2%),
  `serve.rs` (67.5%), and `background/push_delivery.rs` (72.8%). The first
  was on the previous shortlist and is still untouched; `serve.rs` was not
  on any list and should have been.
* **Decide whether `A2aRouter` should route `/tenants/{tenant}/…`.** The
  built-in REST dispatcher strips that prefix and threads the tenant
  through; the axum adapter registers no such routes. Verified to fail
  closed — such a request 404s rather than being served from the default
  partition — and pinned by a test, but the asymmetry between the two
  dispatchers is undocumented behaviour that a user will eventually hit.

## Release engineering and supply chain

* **Signed tags.** All ten release tags (`v0.2.0` … `v0.7.0`) are lightweight:
  no tagger, no date, no signature, despite `RELEASING.md` prescribing
  `git tag -a`. Adopting `git tag -s` needs a maintainer key and a documented
  way for adopters to obtain it — an unmade decision, not just a missing
  step. See [`RELEASING.md`](RELEASING.md).
* **PGP key for security reports.** `SECURITY.md` has none, so emailed
  vulnerability reports cannot be encrypted. GitHub Security Advisories is
  the recommended channel in the meantime.
* **Register `a2a-rust.dev`.** The domain is unregistered (NXDOMAIN), so both
  `conduct@a2a-rust.dev` and `security@a2a-rust.dev` are undeliverable. Both
  documents now point at the maintainer address instead; the dedicated
  addresses can be restored once the domain is live.

## Reporting accuracy

* **Codecov's total excludes less than `codecov.yml` says.** Verified
  2026-08-06 against Codecov's per-file report for `615d01f8`: the three `**`
  directory globs are applied, the five bare Postgres file paths are not, so
  793 permanently-uncoverable lines sit in the public denominator. That is the
  whole 93.62%-badge versus 95.75%-local gap. The entries now carry a glob
  token; **one upload is still needed to confirm the fix took**, by repeating
  the arithmetic in `docs/rust-sdk-assessment.md` §4.4.
* **Say which coverage number is meant.** `cargo llvm-cov` reports regions
  90.87%, functions 89.32% and lines 91.49% for the same workspace. A bare
  "coverage: N%" in this project is ambiguous between at least four figures.

## Conformance

Measured against the official `a2a-tck` suite: **92 of 114 MUST requirements
passing, 0 failing.** The remaining 22 are not defects in this SDK — 21 have
no test function upstream, and `CARD-EXT-002` is structurally inapplicable.
Full analysis and reproduction steps in
[`docs/official-tck-findings.md`](docs/official-tck-findings.md).

* **`SSE-001` is reported upstream** as
  [a2aproject/a2a-tck#225](https://github.com/a2aproject/a2a-tck/issues/225)
  (filed 2026-08-07). Nothing more to do here until upstream responds; if the
  fix lands, drop the `--deselect` in `.github/workflows/official-tck.yml`.
* **Track the 13 open upstream backlog items** that would move requirements
  out of `NOT TESTED` if `a2a-tck` implements them. Nothing to do here except
  re-measure when upstream moves; the ceiling is not this project's to raise.
* **WebSocket** remains a custom binding under spec §12 and is deliberately
  outside the official suite's scope. It is covered by this repository's own
  feature-gated tests.

## Open questions

Genuinely undecided — listed so they are not mistaken for oversights.

* Whether to adopt signed tags at all, or to rely solely on the SLSA build
  provenance attestations already produced for release artifacts
  (see [`PROVENANCE.md`](PROVENANCE.md)).
* Whether `0.8` should also raise MSRV, and what support window to state.
* Whether the axum adapter should reach parity with the REST dispatcher on
  tenant routing, or whether the split is intentional and should simply be
  documented as such.

## Maintaining this file

Add an item when the repository commits to it — a deprecation, a documented
removal, a measured gap. Remove it when the work lands, and say where it
landed. Do not add speculative milestones: a roadmap that lists intentions
nobody has committed to is worse than no roadmap, because it cannot be
checked against anything.
