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

* **~~Demonstrate the SDK capabilities a deployment needs.~~ Done 2026-08-11** —
  tenant isolation, authentication interceptors, rate limiting, persistent
  stores, agent-card signing, the `Metrics` hook, OpenTelemetry export and
  graceful shutdown all shipped, all covered by unit and integration tests, and
  **no example demonstrated any of them over a socket**. That is a real gap and
  not a documentation one: a reader evaluating this SDK reads the examples, and
  the examples showed an in-memory, single-tenant, unauthenticated agent.

  `examples/incident-response` now runs **sixteen** such checks as Act 5, each
  asserting the specific wrong answer it rules out. The first eight covered a
  tenant reading another's tasks, an anonymous request succeeding, a rate limit
  that accepts everything, a tampered card verifying, a task that does not
  survive a handler change, a shutdown that hangs, served requests that reach no
  recorder, and an instrumented handler exporting no datapoint; the other eight
  are in the table below. Runnable alone as
  `cargo run -p incident-response -- harden`, gated by its own step in
  `ci.yml`'s `example-surface` job, and proven able to fail by
  `scripts/prove_gates_fail.sh` (`example_hardening`, which removes the tenant
  resolver — a defect under which every request still succeeds).

  Two things were checked rather than assumed while writing it. The OTel check
  collects from a real `ManualReader` because the default global meter provider
  is a no-op, under which a handler that records nothing and one that records
  everything are indistinguishable. And the signing check tampers with
  `supported_interfaces[0].url`, not the deprecated top-level `AgentCard::url`:
  the latter is `#[serde(skip_serializing)]` since A2A v1.0 removed it, so it is
  absent from the canonical bytes and rewriting it is correctly a no-op. The
  first draft tampered there, reported a failure, and the failure was the
  check's fault rather than the SDK's.

  Three of the eight assert that something must *not* succeed, and each
  originally accepted any error as proof — so killing the agent between setup
  and assertion would have read as "correctly refused". They now require the
  error to be a server refusal (`Protocol`/`AuthRequired`/`UnexpectedStatus`)
  rather than a transport failure, and all three arms were proven to fire by
  pointing the client at a closed port: before the change all three injections
  passed, after it all three go red naming the unreached server.

  **What this entry claims, and its boundary.** "Done" means each capability
  is demonstrated over a socket by an example, with an assertion that can fail —
  not that it is bug-free. Sixteen capabilities are covered as of 2026-08-11;
  a grep of `examples/` for the eight that had none confirms each now has one.

  Eight more landed the same day, closing the list this entry previously
  carried as outstanding:

  | Capability | How it is demonstrated | What the check rules out |
  |---|---|---|
  | `ApiKeyAuthInterceptor` | Custom header, three cases | An interceptor that rejects on header *presence* rather than value |
  | `JwtAuthInterceptor` (remote JWKS) | ES256 tokens against a JWKS the example serves | A forged signature accepted, or an expired token accepted |
  | `HandlerLimits` | `max_id_length` on a caller-controlled `context_id` | An unbounded identifier a caller can make the server allocate |
  | `RetryPolicy` (client) | Fault-injecting proxy in front of a real agent | A retry layer that treats every 5xx alike and double-executes a non-idempotent `SendMessage` on an ambiguous `502` |
  | `TenantAwareSqliteTaskStore` | Two tenants, handler replaced, read back as both | Partitions that are correct in memory and share one table on disk |
  | `PostgresTaskStore` | Round-trip through two handlers | A second SQL backend assumed correct because SQLite is |
  | `init_otlp_pipeline` | Real collector socket, byte and HTTP/2-preface assertions | A pipeline that builds cleanly and exports nothing |
  | `HttpPushSender::with_tls_config` | rcgen cert, `tokio-rustls` sink, both trust stores | A sender that "supports HTTPS" without verifying certificates |

  Each was proven able to fail by removing the capability it covers. Two of the
  eight needed an external service the example cannot start; only PostgreSQL
  still does, so that check reports `[NOT RUN]` naming
  `A2A_TEST_POSTGRES_URL` rather than silently passing, and CI provides the
  service. `INCIDENT_REQUIRE_ALL=1` — set in CI — turns any `[NOT RUN]` or
  `[NOT BUILT]` into exit `4`, so a service that stops being provisioned fails
  the job instead of quietly downgrading a check to a printed line.

  Writing them found two mistakes in the checks themselves, both fixed rather
  than worked around: an "expired" JWT that expired 30 seconds ago is *correctly*
  accepted inside `JwtValidator`'s 60-second clock-skew leeway, and a proxy that
  forwards a request without its headers strips `A2A-Version` and gets
  `-32009` back. Both first reported as SDK defects; neither was.

* **~~Make every example's coverage claim measurable.~~ Done 2026-08-11** —
  all six examples now report **44 of 44 cells** and exit non-zero on a gap,
  gated by `ci.yml`'s `example-surface` job with each leg proven able to fail.
  The three that depend on an external service (LLM provider, cross-language
  workers) report that dependency's status separately, so a green matrix is
  never read as "the model works" or "four languages round-tripped".

* **~~Make the examples' coverage claims measurable.~~ Done 2026-08-11** —
  `echo-agent` drove 4 of the 11 A2A methods over 2 of the 4 transports and
  `incident-response` 4 over 1, while `examples/README.md` called the first
  "the complete request lifecycle". Both now drive every method over every
  binding they serve — **44 of 44 cells each** — and exit non-zero on a gap,
  gated by `ci.yml`'s `example-surface` job.

  The denominator is deliberately not this project's: `Method::ALL` is asserted
  equal to `service A2AService` in the ratified `proto/a2a_v1/a2a.proto`, and
  `scripts/check_method_denominator.py` cross-checks both against the upstream
  `a2aproject/a2a-tck` on every Official TCK run. All three agreed on the same
  eleven methods when measured against `a2a-tck@5996b79`.

  Doing this found a real client defect — the WebSocket transport delivered its
  `stream_complete` control frame to the consumer, where it failed to
  deserialize. It only surfaces when a stream ends without a terminal task
  state, so only an agent that asks clarifying questions exposes it. Fixed with
  regression tests; see the changelog.

* **~~Run the SDK dogfood suite in CI.~~ Done 2026-08-11** — `examples/agent-team`
  holds ~5,900 lines of E2E tests and no workflow had ever executed it. It is a
  `main()`, not `#[test]`s, so `cargo test --workspace` compiled it and ran none
  of it; it appeared in the workflows only inside `cargo package --exclude`
  lists. First local run: **86 tests, 71 passed, 15 failed, exit 1**.
  Now **100 tests, 100 passing** with `--all-features`, gated by `ci.yml`'s
  `dogfood` job.

  This was the sixth gate found structurally incapable of failing, and the
  largest: the suite's "SDK FEATURES EXERCISED" table was a hardcoded list
  printed as `[x]` with no link to the results, so it stayed green through
  fifteen failing tests, and feature-gated rows were `#[cfg]`'d out of the list
  rather than reported unexercised. The table is now computed from outcomes and
  cross-checked against the suite in both directions; see
  `examples/agent-team/src/features.rs`.

  Of the 15 failures, 14 were test rot (a raw-HTTP helper that never sent
  `A2A-Version`; three tests assuming the first stream event is a status update
  rather than a `Task` snapshot; one assertion that contradicted the SDK's
  documented lag contract) and one — `GetExtendedAgentCard` — was a test whose
  agent had never been configured to support the operation, so it could not
  have passed on any commit. No SDK defect was found. That is a real result and
  worth stating plainly, but it was unknowable while nothing ran the suite.

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

* **Codecov's total excludes less than `codecov.yml` says — and the glob-token
  fix did NOT take.** Verified 2026-08-06 against Codecov's per-file report for
  `615d01f8`: the three `**` directory globs are applied, the five bare Postgres
  file paths are not, so 793 permanently-uncoverable lines sit in the public
  denominator. The entries were rewritten into glob-token form
  (`**/store/postgres_store.rs` etc.) in `0e64636` on the hypothesis that
  "the three patterns that do work here all contain a glob token".

  **Measured 2026-08-12: the hypothesis is disproven, not merely unconfirmed.**
  `git merge-base --is-ancestor 0e64636 db1da90` is true, and four uploads
  post-date the fix (`d6d28d8`, `af7a1f8`, `c008ab0`, `db1da90`), so the
  confirmation this entry was waiting on has happened. Codecov API v2, per-file
  report for `db1da9006cdf98e37ed3ea38b4a1f7817abdf429`:

  | Query | Result |
  |---|---|
  | files in report | 124 |
  | CONTROL — `tck/` files | **0** (that ignore *does* work) |
  | CONTROL — `crates/` files | **124** (query is live, not empty) |
  | TEST — postgres / `pg_migration` files | **5 — still present** |

  Both controls are load-bearing. A first attempt used the abbreviated sha, got
  HTTP 404 with an empty body, and "found no postgres files" — a vacuous pass;
  the `crates/` control is what separates "absent" from "nothing was queried".

  The five total **793 lines / 40 hits**, exactly the 793 this entry predicted:
  `postgres_config_store.rs` 124, `tenant_postgres_config_store.rs` 140,
  `pg_migration.rs` 73, `postgres_store.rs` 228, `tenant_postgres_store.rs` 228.
  Reported at `db1da90`: 35343 lines / 33290 hits = **94.19%**. With the five
  genuinely ignored: 34550 / 33250 = **96.24%** — a 2.05 point gap.

  So "contains a glob token" is *not* the discriminating property. What is
  remains **UNKNOWN** — no experiment here isolated it, and this entry will not
  name a cause it has not tested. **Do not write a third fix into `codecov.yml`
  without a way to test it before merge**; the first two were each plausible and
  each shipped without a pre-merge check that could have caught them.
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
  checking the bindings agree. ~~`BIND-EQUIV-004` is graded structurally only —
  the enforcement half needs a target configured to require credentials,
  which no job here provides.~~

  **`BIND-EQUIV-004`'s enforcement half closed 2026-08-11** — the last
  in-scope conformance claim this repository had recorded as unmeasured.
  `tck/sut` gained a `SUT_PROFILE=secured` profile that declares a bearer
  scheme on its card and enforces it with one `BearerTokenAuthInterceptor`
  above the dispatchers, and `--equivalence --auth-token` grades two sweeps
  against it: every binding must refuse an uncredentialed request, and every
  binding must serve a credentialed one. Gated in `tck.yml`.

  The acceptance sweep is not symmetry for its own sake. The first draft of the
  probe sent the JSON-RPC method as `tasks/list` where this SDK's name is
  `ListTasks`; it authenticated correctly, failed method dispatch, and reported
  a binding asymmetry that did not exist. On the rejection sweep alone, a probe
  that can never succeed looks exactly like enforcement working — the same
  shape as the gates this repo has found that could not fail. Proved by
  injection that the check goes red: run with a wrong token it exits 1.

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
