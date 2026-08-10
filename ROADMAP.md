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
  **The current figure is 94% (2187/125), from the 2026-08-10 sweep** — this
  bullet records the first complete sweep, not the latest one. See
  [`mutation-history.md`](book/src/reference/mutation-history.md) for the
  dated table, and the burn-down item below for the survivor clusters.
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
* **Burn down the surviving mutants.** The four largest clusters from the
  2026-08-07 sweep are done, plus `agent_card/caching.rs` at 0:

  | File | Then | Now |
  |---|---:|---:|
  | `handler/messaging.rs` | 17 | **0** |
  | `store/task_store/in_memory/eviction.rs` | 13 | 2 (equivalent, deliberate) |
  | `dispatch/grpc/native.rs` | 11 | **0** |
  | `handler/lifecycle/list_tasks.rs` | 10 | **0** |

  **51 survivors, 49 killed or designed out.** An earlier revision of this
  bullet put the unkillable share at "roughly 12%", extrapolated from
  `messaging.rs` alone, and used it to estimate a floor of ~160 killable. That
  estimate was wrong and the error is worth keeping: the rate is not a property
  of the codebase, it is a property of the *shape* of each survivor.
  Whole-method survivors (`native.rs`: ten of eleven) and never-entered blocks
  (`list_tasks.rs`: all ten) yield no equivalents at all. Only boundary
  comparisons do, and half of even those turned out to be removable by deleting
  a branch that guarded a no-op rather than by testing harder — see the ledger's
  "retired by deleting the branch" section. Do not extrapolate a floor from one
  file; measure the next one.

  ~~**~132 survivors remain** from that sweep's 183 (a2a-server 165, a2a-client
  10, a2a-types 8). The next clusters are not yet identified — the sweep that
  named the top four is now three files out of date, so start by re-running it
  (or reading the latest `mutants-summary` artifact) rather than trusting this
  list.~~

  **Superseded 2026-08-11 by measurement.** The estimate was close but it was
  an estimate; the weekly sweep of 2026-08-10 (run
  [31352927429](https://github.com/tomtom215/a2a-rust/actions/runs/31352927429),
  `041c366` on `main`) settles it: **125 survivors, 94% — 2187 caught / 125
  missed / 2 timeout / 1277 unviable.** Per crate: `a2a-server` 1225/107 (91%),
  `a2a-client` 357/10 (97%), `a2a-types` 605/8 (98%), `a2a-sdk` 0/0. All 21
  shards completed and carry a `COMPLETED` marker, and it is the first sweep to
  run against a live database — Postgres-file survivors fall 18 → 3.

  Verified by re-deriving all of it from the run's 21 raw shard artifacts with
  a second implementation of the workflow's counting rule. Every figure,
  including all four per-crate rows, reproduces exactly.

  The clusters are now identified. Largest first:
  `handler/event_processing/sync_collector.rs` 9, `dispatch/grpc/service.rs` 9,
  `streaming/event_queue/in_memory.rs` 8,
  `handler/event_processing/background/state_machine.rs` 8, `push/sender.rs` 7,
  `dispatch/websocket.rs` 7, `dispatch/axum_adapter.rs` 7, `rate_limit.rs` 6.

  Two caveats before anyone works from that list. It is measured at `041c366`,
  which is 44 commits behind `af7a1f8`; the ledger quantifies that 61 of the 125
  sit in files changed since, and 64 are in files unchanged and therefore still
  valid. And `dispatch/grpc/service.rs`'s 9 sit in the deprecated
  `grpc-legacy-json` tunnel that 0.8 deletes outright — sequence that removal
  first rather than testing code scheduled for removal.

  The weekly sweep fails until survivors are killed or explicitly
  justified, which is the intended state, not a problem to suppress. No
  baseline file: the `--in-diff` PR gate already prevents new code from adding
  survivors, so the count can only fall.

  Method that worked, in order: reproduce the file's survivor count on an
  unmodified tree first, read *why* each survives before writing anything, then
  re-measure. Report the exit code next to the counts — a cargo-mutants
  baseline failure writes empty result files and prints `caught=0 missed=0`,
  which is indistinguishable from a clean file. `scripts/preflight.sh` runs the
  CI gates locally; a live Postgres and `--run-ignored all` are required or
  every Postgres mutant survives for want of a database.
* **~~Decide whether the 500-line guideline applies to scripts.~~ Done
  2026-08-11** — it does, and `check_file_lengths.sh` now enforces it over
  `.rs`, `.sh` and `.py` rather than `.rs` alone. Widened rather than splitting
  the two long provers, because the same measurement found two more over-limit
  scripts nobody had counted (`benches/scripts/extract_benchmark_json.py` 658,
  `benches/scripts/generate_book_page.sh` 650): a ratchet catches the next one,
  a refactor catches only this one. Baseline 77 → 81 entries of 333 tracked
  sources. Both directions of the widened check were proven by injection — a
  new 501-line script exits 1, and a baseline entry naming a 201-line script
  exits 1 as stale.
* **Decide the wording of the zero-survivor rule.** Partly done.
  `CONTRIBUTING.md` no longer claims a blanket "zero surviving mutants": it now
  separates the blocking per-PR `--in-diff` gate (which a contributor is
  accountable for) from the advisory workspace sweep (pre-existing debt), and
  states the sweep's real number. ~~**Still open:** ADR 0006 carries the old
  absolute wording.~~ **Done 2026-08-11** — ADR 0006's Consequences section
  said developers "must address surviving mutants before merge" with no scope,
  which read as the whole workspace's 125. It now says "on the lines their PR
  changes", matching what the gate actually enforces, and the stale 92% / 183
  figure in its Target section is updated to 94% / 125.

  **The `#[mutants::skip]` question is closed, and closed by removal rather
  than by decision** (2026-08-09). The exception list is empty: the two
  `eviction.rs` equivalents were retired by rewriting the guard as
  `saturating_sub(...)` + `!= 0`, which keeps the O(n log n) short-circuit the
  old `>` guard existed for while removing the operator whose weakened form was
  equivalent. `--exclude-re` is gone from both the sweep and the incremental
  gate, so there are now no mutation exclusions anywhere. Measured: `eviction.rs`
  25 mutants → 17, all 17 caught (exit 0); the old pattern matches 0 of the
  crate's 2097 mutants. With nothing left to skip, taking the `mutants` crate as
  a dependency of a published crate is no longer a decision anyone is waiting on.
* **Raise coverage on the genuinely weak files.** After the 2026-07-31 pass,
  the weakest are `handler/event_processing/background/mod.rs` (54.2%),
  `serve.rs` (67.5%), and `background/push_delivery.rs` (72.8%). The first
  was on the previous shortlist and is still untouched; `serve.rs` was not
  on any list and should have been.
* **`handler/helpers.rs` is over the 500-line guideline** — 612 lines, up from
  463, crossed by the `truncate_history` extraction and its tests (2026-08-08).
  Flagged rather than silently accepted: it is a grab-bag module, so the split
  is real work (validation, call-context, history shaping, and the
  `find_task_by_context` impl are four unrelated concerns) and was not worth
  doing at the tail of that change. ~~59 of 232~~ **77 of 310** tracked `.rs`
  files already exceed the guideline (measured 2026-08-10), so this is not
  novel, but it is one more. Since 2026-08-10 the guideline is enforced as a
  ratchet by `scripts/check_file_lengths.sh`: the 77 are recorded in
  `.file-length-baseline` and the list may only shrink, so no further file can
  cross 500 lines unnoticed the way this one did.
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
  **Half of this is closed as of 2026-08-10:** `release.yml` now fails the
  release if the pushed tag is not an annotated tag, so an eleventh
  lightweight tag cannot be created through the GitHub UI without stopping
  the workflow. The signing half is unchanged and still needs the key
  decision; the check deliberately does not require a signature, because a
  gate for a key that does not exist could never fail.
* **PGP key for security reports.** `SECURITY.md` has none, so emailed
  vulnerability reports cannot be encrypted. GitHub Security Advisories is
  the recommended channel in the meantime.
* **~~Add the four missing governance files.~~ Done 2026-08-11** —
  `MAINTAINERS.md`, `.github/CODEOWNERS`, `SUPPORT.md` and `TRADEMARKS.md`.
  `GOVERNANCE.md`'s duplicated maintainer table was removed at the same time
  and now points at `MAINTAINERS.md`, so the two cannot drift.

  **These are necessary and nowhere near sufficient**, and the checklist being
  complete must not be read as progress on adoption.
  [`docs/rust-sdk-assessment.md` §7](docs/rust-sdk-assessment.md) is explicit
  that the remaining blockers are not repo-hygiene items: an official Rust SDK
  already occupies the slot, the decision mechanism is an eight-corporation TSC
  vote, the provenance disclosure has to be assessed by that body, and the
  maintainer group is one person. Adding four files moves none of those. What
  it does is stop their absence from being a distraction, and — in the case of
  `CODEOWNERS` — it documents plainly that with one maintainer the mechanism is
  inert for the maintainer's own commits.
* **Register `a2a-rust.dev`.** The domain is unregistered (NXDOMAIN — re-verified
  2026-08-11 via DoH, `Status: 3`), so both
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
* **Say which coverage number is meant.** A bare "coverage: N%" in this
  project is ambiguous between at least four figures, and the *file set*
  matters as much as the metric. Re-measured 2026-08-10 with
  `cargo llvm-cov --workspace --all-features`: lines are **90.88%** over
  everything instrumented, **93.52%** over the badge's file set, and
  **95.57%** over `codecov.yml`'s full ignore list; functions are **89.00%**.
  The four-way rollup and the commands behind it are in
  `docs/rust-sdk-assessment.md` §4.4 — quote a row from that table rather
  than a bare percentage.

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
  outside the official suite's scope. Since 2026-08-10 it is graded by the
  in-repo runner's `--binding websocket` leg (21 checks, 1 not applicable) in
  addition to this repository's feature-gated tests — the official suite's
  silence about it is expected, but ours was a real gap.
* **Cross-binding equivalence (§5.1, `BIND-EQUIV-001..004`)** is graded by the
  in-repo runner's `--equivalence` mode since 2026-08-10, across all four
  bindings. This does not move the upstream count: the official suite still
  reports those four MUSTs `NOT TESTED`, because the tests that would change
  that have to live in `a2aproject/a2a-tck` (its own `task-28`). What changed
  is that this repository no longer ships a four-binding server with nothing
  checking the bindings agree. `BIND-EQUIV-004` is graded structurally only —
  the enforcement half needs a target configured to require credentials,
  which no job here provides.

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
