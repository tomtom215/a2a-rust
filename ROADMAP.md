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

## 0.8 — removals this repository committed to — **DONE 2026-08-13**

All three landed in the 0.8.0 preparation commit, and all four crates were
reconciled to 0.8.0 at the same time. Kept here as the record of what was
promised against what shipped, rather than deleted.

| Item | Status |
|---|---|
| Remove the `grpc-legacy-json` feature and the pre-0.7 JSON-tunnel gRPC service it serves | **Done** — feature dropped from both `a2a-protocol-server` and `a2a-protocol-sdk`; `dispatch/grpc/service.rs`, the `a2a.v1` protos, the build-script step, the JSON codec helpers, `into_legacy_service`, the `Legacy*` re-exports and the coexistence test all deleted |
| Remove `with_event_queue_write_timeout` — a deprecated no-op | **Done** — that setter and `EventQueueManager::with_write_timeout` both removed, with the builder field, its `build()` plumbing, its `Debug` field and both tests |
| Stop sending the legacy bare `a2a-notification-token` header | **Done** — two sites, not one: `HttpPushSender` no longer *sends* it (the row's actual wording), and it is dropped from the default CORS `allow_headers`. Canonical `x-a2a-notification-token` retained, and the `push_sender_https_e2e` assertion inverted to pin the header's absence rather than deleted |

`dispatch/grpc/service.rs` is gone. Deleting it was expected to retire the 9
surviving mutants the 2026-08-10 sweep recorded against it — the reason the
file was worth sequencing ahead of any mutation work on it. **The 2026-08-13
re-run (run 31681284244, at `6ebf821`) measures 0 survivors in that file**, so
those 9 had already been killed by tests landed in between and the deletion
retires none of them. Kept visible rather than quietly corrected: the estimate
was sound reasoning over a stale number, and the number moved.

### Mutants no test can kill — proved, not assumed

ADR 0006 sets the target at zero surviving mutants "with the single
documented exception" of mutants that no test can kill, and requires that an
equivalence claim be *proved* rather than asserted. Three came up while
burning down the 2026-08-13 sweep. They are recorded here rather than skipped
in source, because the `#[mutants::skip]` attribute needs the `mutants` crate
as a **runtime** dependency of published crates — a supply-chain decision the
ADR says must be raised on its own terms, not settled inside a test PR.
Decision taken 2026-08-14: keep them documented, add no dependency.

| Mutant | Why no test can kill it |
|---|---|
| `tenant_config.rs` — `TenantLimits::builder` → `Default::default()` | The body *is* `TenantLimitsBuilder::default()`. In a function returning `TenantLimitsBuilder`, `Default::default()` resolves to `<TenantLimitsBuilder as Default>::default()` — the same call. |
| `tenant_config.rs` — `PerTenantConfig::builder` → `Default::default()` | Identical argument for `PerTenantConfigBuilder`. |
| `streaming/sse.rs` — `SseBodyWriter::close` → `()` | The body is `drop(self)` and the receiver is `self` by value. With or without the explicit `drop`, `self` is dropped when the function returns, and nothing follows it. |
| `auth/jwt.rs` — `build_jwks_client` → `Default::default()` | Not an equivalence: the mutated variant is `#[cfg(not(feature = "tls-rustls"))]` and the sweep builds `--all-features`, so it is not compiled at all. See below. |

The first three are equivalences of *form*: the mutated expression compiles to
the same observable behaviour, so a test written against it could not fail.

**The fourth is a different category, and ADR 0006 does not yet name it.**
`build_jwks_client` has two `#[cfg]` variants — one for `tls-rustls`, one for
its absence. cargo-mutants parses the source, sees both, and generates a
mutant for each; the sweep then builds with `--all-features`, which *enables*
`tls-rustls`, so the `#[cfg(not(...))]` variant is never compiled. Mutating a
body that is not in the binary changes nothing, the suite passes, and the
result is reported as `MISSED` rather than `unviable`. It is not a test gap
and not a semantic equivalence: it is a mutant in code the build excludes.

Proved in two directions rather than argued:

1. Line 790 at `7469fd5` — the commit the sweep measured — is inside the
   `#[cfg(not(feature = "tls-rustls"))]` variant. Verified with
   `git show 7469fd5:crates/a2a-protocol-server/src/auth/jwt.rs`.
2. The *other* variant's return type does not implement `Default`: a
   compile probe of `let _: JwksHttpClient = Default::default();` under
   `--all-features` fails with `the trait bound Client<HttpsConnector<
   HttpConnector>, Full<Bytes>>: Default is not satisfied`. So a mutant on
   the TLS variant would be `unviable`, not `MISSED` — the reported one has
   to be the cfg-excluded variant.

Worth knowing generally: any `#[cfg]`-gated function that is not part of the
sweep's feature set will produce mutants of this kind, and they will look
exactly like test gaps in the report.

**Everything else in the sweep was killable.** Of the 57 survivors measured at
`7469fd5`, **53 fell to tests**; these four are the remainder.

**Confirmed by measurement, not by the claim.** Run
[31814679306](https://github.com/tomtom215/a2a-rust/actions/runs/31814679306) at
`1d51be0`, full CI configuration, 21/21 shards with `COMPLETED` markers:
**2262 caught / 4 missed / 1 timeout / 1293 unviable — 99%** (99.82% exact). The
four `missed.txt` lines across all 21 artifacts are exactly the four above and
nothing else. Every count was re-derived from the raw artifacts rather than read
off the summary, and the arithmetic closes: 2262 + 4 + 1 + 1293 = 3560, which is
the total number of entries in the shards' `mutants.json` files.

**Three of those 53 were claimed before they were true, and the confirming
sweep caught all three.** Recorded here because the burn-down's credibility
rests on the claims being checkable, and these were not:

* `state_machine.rs:143` was reported killed in `8e3f321`. The test written
  for it exercised the parts-cap branch, which `return`s at line 112 — the
  mutant lives in the store-save-failure revert further down, and the test
  never reached it. It passed, on a different code path. The underlying error
  was skipping mutation verification for that file and asserting a kill from a
  green test, which is the one thing this whole exercise shows does not follow.
* `rest/mod.rs:255` was called run-to-run variance in `11f4456` when it
  appeared in the `6ebf821` sweep and not the `7469fd5` one. It had never been
  killed. The REST binding accepts two cancel spellings, and three existing
  tests exercise the colon form (`/tasks/{id}:cancel`) while the slash form had
  no coverage at all. A survivor that disappears with no test change is a
  question, not noise.
* `manager.rs:385` — `replace EventQueueManager::destroy with ()` was reported
  resolved in `90bf065`, which said the three timeouts were "bounded in
  `1d51be0` — re-verified as 26 caught / 0 timeouts". The only run matching that
  "26 caught" printed:

  ```text
  TIMEOUT  crates/…/manager.rs:385:9: replace EventQueueManager::destroy with ()
  76 mutants tested in 22m: 26 caught, 49 unviable, 1 timeouts
  ```

  and it finished 2026-08-13 17:51 UTC — **21 hours before `1d51be0` was
  authored**, so it could not have verified anything about it. There was also no
  local-versus-CI disagreement to reconcile: the CI sweep and this run said the
  same thing, and "0 missed" was read off the summary as "0 timeouts". The
  *diagnosis* was wrong as well as the reading: `destroy` is what closes a
  task's event queue (the blocking send path calls it from
  `CleanupGuard::drop`), so a no-op `destroy` hangs every test that drains a
  task stream to EOF — applying the mutation by hand stops tests returning
  across `dispatch::grpc::native`, `handler::messaging`, `event_processing_tests`,
  `handler_tests`, `audit_tests` and `auth_jwt_e2e` — while the tests that catch
  it head-on (`manager_destroy_removes_queue`,
  `active_count_decrements_on_destroy`) fail in milliseconds and never get to
  report, because the process never exits. Bounding the one unbounded receive in
  `1d51be0` could not have fixed that, and did not.

The first two are fixed in `2607280` and verified (96 mutants, 42 caught, 0
missed). The third is fixed at the harness rather than in a test: a 45s
per-test kill in `.config/nextest.toml` terminates the hangs and names them, so
the run exits non-zero and the mutant is scored. Verified under the sweep's own
configuration — `cargo mutants -p a2a-protocol-server --test-tool=nextest
--profile=mutants -f …/manager.rs -F 'destroy with \(\)' -- --all-features
--run-ignored all` against a live PostgreSQL 16.13 — **1 mutant tested, 1
caught, empty `timeout.txt`**. Then the same command again with the file moved
out of the tree, as a control:

```text
without  TIMEOUT  …manager.rs:385:9: replace EventQueueManager::destroy with ()
                  in 8s build + 334s test
         1 mutant tested in 8m: 1 timeouts
with     1 mutant tested in 3m: 1 caught
```

Same tree, same command, one file the difference — and no `.rs` file differs
from the sweep at `1d51be0` that reported the same mutant as `TIMEOUT`
(`git diff 1d51be0 HEAD -- '*.rs' 'Cargo.*'` is empty). The claim is measured in
both directions rather than inferred from the fixed one, which is the whole
lesson of the two bullets above it.

The general rule this produced: **a passing test is not evidence that a mutant
died — only a mutation run is.** The third adds the corollary that was still
missed after the first two: **and a mutation run is only evidence if the outcome
column is read.** `0 missed` and `0 timeouts` are different statements, and the
summary line prints both.

### Residue left behind deliberately

The two public `write_timeout` setters are gone, but the value is still
threaded through `new_in_memory_queue_with_options` into an
`#[allow(dead_code)]` field on `InMemoryQueueWriter`, and
`DEFAULT_WRITE_TIMEOUT` is still `pub`. Removing those changes a public
constructor's arity and deletes an exported constant, neither of which was on
the announced list — an unadvertised API break riding along with an advertised
one is exactly what a deprecation schedule exists to prevent. Left for a later
release, recorded here so it is a decision rather than an oversight.

## 0.8.x — the SLIMRPC binding and the send-path cliff — **2026-08-15**

Two pieces of work, on `claude/v0.8.0-transport-perf-3wq3w8`. Recorded here in
the shape a fresh session needs: what is settled, what it cost to learn, and
what is genuinely next.

### The 1.5 ms `message/send` — cause found, fixed, 91% off

The standing hypothesis was cross-thread scheduling on 4-core runners. It is
**disproved**: the same send on a single-worker runtime does not close the gap,
and cutting an executor event changes it by 2%.

The cause was `InMemoryTaskStore`. It caps at `max_capacity` (10,000); once full
it is over the cap on *every* subsequent write, and each write cloned every
`TaskId` into a `Vec`, sorted it, and removed one task. A blocking send performs
several saves, so one request paid that O(n log n) sweep several times.

A cliff, not a slope, and it never recovered — measured with
`cargo run --release -p a2a-benchmarks --example send_probe`:

| Sends so far | Before | After |
|---|---|---|
| 10,000 (at the cap) | 65.2 µs | 62.9 µs |
| 11,000 (past it) | **2.4 ms** | **67.3 µs** |

`transport/jsonrpc/send/single_message` improves 91% (2.18 ms → 191 µs,
p < 0.05). Control: `get_task` on a missing id — one round trip, no write — is
unchanged at −0.25% (p = 0.76). Scheduling turned out to be real but
second-order, worth ~50 µs of the remaining 191, entirely masked by the
eviction cost.

### The SLIMRPC binding — `bindings/a2a-protocol-slimrpc`

All eleven spec methods plus multicast, deliberately outside the workspace with
its own `Cargo.lock` (`agntcy-slim-rpc` brings 379 transitive dependencies
including `aws-lc-sys`; `a2a-protocol-types` has 12). 65 tests across ten
topologies — in-process, multicast group, one node over TCP, that node with
verified TLS, mutual TLS, two peered nodes, a node in its own OS process, and
three suites against a real SPIRE deployment (identity, federation, rotation).
The crate README carries a security posture table separating what is *verified
by a test* from what is merely *available*.

**One change to a published crate was needed**, and it is the finding worth
carrying forward: `Transport::send_streaming_request` must return an
`EventStream`, and every `EventStream` constructor was `pub(crate)`. A
third-party binding could implement the unary half of the trait and not the
streaming half. The trait is `pub`, its parameters are `pub`, its return type is
`pub`, and it was still unimplementable from outside.
`EventStream::from_event_channel` closes that — purely additive.
`docs/rust-sdk-assessment.md` §4.1.1 previously concluded no change was needed,
verified by reading declarations; the correction is recorded there.

### Things that cost time, so they need not cost it twice

* **SLIM names carry a fourth instance component** (`org/ns/agent/NULL_COMPONENT`).
  Keying on the full rendering filed every multicast response under a name no
  invited member matched: all agents answered, all were reported as timeouts.
* **A client must announce its own name to the node.** `Channel` sets a route
  outwards only, so without it nothing can route an agent's reply back and every
  call fails its session handshake. Invisible in-process.
* **`SpireIdentityManager` must be built once and cloned** for provider and
  verifier — it holds an MLS signature key, and two managers carry two different
  ones. **And each app needs its own SPIFFE ID**: two apps sharing one cannot
  complete an MLS handshake. Both present as a session that never completes
  rather than as an authentication error.
* **Federation is bundles-before-entries.** An entry naming `-federatesWith` is
  rejected outright unless that trust domain's bundle is already imported.

### Next, in the order I would take them

1. **Soak.** The one thing none of this covers: sustained traffic over hours.
   It would settle rotation-under-load, RSS and queue growth, and the
   store-eviction path at steady state in one job — and it is the class of bug
   the current suite structurally cannot reach, since every test starts from an
   empty store.
2. **Split `handler/messaging.rs`** (2,395 lines, still the worst file, still
   holding the hot path and the destroy/`CleanupGuard` coupling).
3. **A 30-line `hello-agent`**, and a deployment example. Both ends of the
   funnel are still missing; the smallest example remains 736 LOC.

Known limits of the SLIMRPC work, stated rather than implied: federation is by
manual bundle exchange rather than a bundle endpoint; rotation covers JWT-SVIDs
(what SLIM's app identity uses), not X.509 SVIDs or the node's own TLS
certificate under a live connection; everything runs on one machine, so real
network loss, latency and NAT are untested; and static-token identity is
supported via `with_identity` but has no test.

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
  **The current figure is 97% (2254/63), from the 2026-08-13 sweep** (run
  31681284244, all 21 shards complete, 21/21 `COMPLETED` markers verified
  against the artifacts) — this bullet records the first complete sweep, not
  the latest one. See
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
  valid.

  **Superseded 2026-08-13 — the sweep was re-run, and the arithmetic this
  paragraph used to carry was wrong.** It predicted "116 at most rather than
  125" on the reasoning that `dispatch/grpc/service.rs`'s 9 survivors sat in
  the `grpc-legacy-json` tunnel and would retire when 0.8 deleted the file.
  Run 31681284244 at `6ebf821` measures **63 survivors, 97%** — and
  `service.rs` holds **0** of them. Those 9 had already been killed by tests
  landed between `041c366` and `6ebf821`, so 0.8's deletion retires none of
  them; the improvement came from test work, not from removing code. The
  estimate was labelled "arithmetic on a stale measurement, not a new one",
  and the measurement duly disagreed with it in both directions: the total is
  lower than predicted, and the mechanism was not the one assumed.

  Also now zero, having been the two largest clusters: `handler/messaging.rs`
  (was 17) and `store/task_store/in_memory/eviction.rs` (was 13).
  `dispatch/grpc/native.rs` (was 11) is zero as well — worth noting because it
  is the only gRPC surface after 0.8.

  **Five of the 63 were in files 0.8 touches, and are killed on the release
  branch.** None sat in deleted code, so all five would otherwise have
  shipped: four in `streaming/event_queue/manager.rs` (`with_capacity`,
  the `>=` concurrency-limit guard, `raw_subscribe`, `subscribe_with_snapshot`)
  and one in `dispatch/grpc/dispatcher.rs` — `replace GrpcDispatcher::serve
  with Ok(())`, which is the one worth naming: no test in the crate called
  `serve` at all, so a server that never bound was indistinguishable from a
  working one, and only the out-of-crate TCK run covered it.

  Confirmed first by a targeted `cargo mutants --file` over both files rather
  than by the tests passing, which proves nothing about a mutant: 76 mutants,
  26 caught, 49 unviable, 0 missed. That run carried two stated limits — it
  used `--features grpc` rather than CI's `--all-features --run-ignored all`
  (the `#[ignore]`d Postgres suite needs a live database), and it covered two
  files rather than the workspace.

  **Both limits are now closed by a full sweep of the release branch.** Run
  [31742334862](https://github.com/tomtom215/a2a-rust/actions/runs/31742334862)
  at `7469fd5`, the complete CI configuration — `--all-features --run-ignored
  all`, live PostgreSQL service, 21 shards: **97%, 2210 caught / 57 missed /
  2 timeout / 1289 unviable**, with 21/21 `COMPLETED` markers and a
  `missed.txt` line count matching the summary. `manager.rs` and
  `dispatcher.rs` are **0**, so the narrower run was not misleading.
  `dispatch/grpc/service.rs` is absent from the report entirely, 0.8 having
  deleted it, and `dispatch/grpc/native.rs` remains 0.

  **Only five of the six-survivor drop from 63 is this branch's doing.** Two
  mutants changed classification in files the branch does not modify —
  `rest/mod.rs:255 delete match arm ("POST", ["tasks", id, "cancel"])` went
  from `MISSED` to caught, and `sse.rs:102 send_event` from `TIMEOUT` to
  caught. Both sit on async paths where the harness is timing-sensitive, so
  this is run-to-run variance, not an improvement to claim. Counted honestly,
  the branch removes five and the measured total happens to be 57.

  The remaining `TIMEOUT` on `replace EventQueueManager::destroy with ()` is
  pre-existing — recorded by run 31681284244 too, as `manager.rs:403:9`
  against `main`'s line numbering versus `385:9` on the branch, the
  `with_write_timeout` removal having shifted the file. It survived a third
  sweep — run 31814679306 at `1d51be0` reports it as the run's only `TIMEOUT`,
  in shard 12/12, **a shard that exited 0**, because the workflow fails on
  `missed` and not on `timeout`. **Closed 2026-08-14** by the 45s per-test kill
  in `.config/nextest.toml`; it now reports caught. See the third bullet under
  "claimed before they were true" above for what it actually was, and why the
  fix is not a test.

  **The 57 is measured at `7469fd5` and is already behind.** All nine
  `a2a-protocol-types` survivors it reports — 5 in `error.rs`, 2 in
  `days_from_civil`, 2 in `proto/convert` — are addressed in `df6f023` and
  `05272fa`, which post-date the sweep. The `error.rs` five are confirmed
  dead by a targeted run (70 mutants, 54 caught, 16 unviable, 0 missed); the
  other four await theirs. ~~**The figure is not 48 until a sweep says so**~~ —
  the same rule that this file's "116 at most" line broke once already.

  **A sweep has now said so, and it is not 48 either.** Run
  [31814679306](https://github.com/tomtom215/a2a-rust/actions/runs/31814679306)
  at `1d51be0`, full CI configuration, 21/21 shards with `COMPLETED` markers:
  **2262 caught / 4 missed / 1 timeout / 1293 unviable, 99%** (99.82% exact),
  3560 mutants total. `a2a-protocol-types` and `a2a-protocol-client` are both
  **100% with zero survivors and zero timeouts** — so the nine types survivors
  really were dead, but the count was 57 → 4, not 57 → 48. Estimating the
  endpoint would have been wrong by an order of magnitude in the other
  direction this time; the rule holds regardless of which way the error runs.

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
* **`mutants.toml` is not read by cargo-mutants, and never has been.** Found
  2026-08-14 while chasing the `destroy` timeout above. cargo-mutants 27.1.0
  discovers `.cargo/mutants.toml`; this repository's file is at the root.
  Individual keys were already documented as "silently ignored" — `test_tool`,
  `profile`, `exclude_re` each carry a note saying so — but the cause was never
  per-key. Two independent proofs, both in the file's own banner: `cargo mutants
  -p a2a-protocol-types --all-features --list` lists 155 mutants under
  `src/proto/`, the exact path `exclude_globs` claims to exclude, and the count
  is identical under `--no-config`; and `cargo mutants --config mutants.toml`
  aborts with `unknown field 'cap_timeout'`, so a discovered file would fail
  every sweep rather than configure it.

  What that means in practice, all measured the same day: the per-mutant budget
  is cargo-mutants' default **5.0x baseline with no cap** (`baseline 65s test` →
  `Auto-set test timeout to 328s`; 328 > the 300 `cap_timeout` claims, which
  settles it independently of the multiplier), and **generated protobuf code is
  mutated**, its mutants inside every score this project has published. Only the
  `mutants-incremental` job passes `--timeout 300` on the command line, so the
  PR gate is capped and the full sweep is not.

  **Deliberately not fixed for 0.8.** Activating it changes what the sweep
  measures — 155 fewer mutants in `a2a-protocol-types`, the only crate with a
  `src/proto/`, and a budget cut from ~328s to 195s that is close to the ~120s
  which previously reported trivial mutants as TIMEOUT. That needs a sweep to
  land on, not a
  tail-of-release edit, and it would make the number non-comparable with every
  row of the mutation history until re-measured. The one thing `cap_timeout`
  existed to prevent — a hung mutant burning the job timeout — is now handled
  better a layer down, by the 45s per-test kill in `.config/nextest.toml`, which
  bounds the hang *and* names the test.

  When it is done: move the file to `.cargo/mutants.toml`, delete `cap_timeout`
  first or nothing runs, re-measure with `--list` before and after so the scope
  change is a number rather than an assumption, and record the new baseline as
  its own row. Worth pairing with a check that the config is actually loaded —
  the repository already treats "is this gate pointed at what it claims to
  cover?" as a separate question from "can this gate fail?"
  (`scripts/check_mutation_scope.sh`), and this is the same question again.
* **The blocking send path's post-executor drain has no bound.** Noted while
  proving out the `destroy` mutant, and left as an observation rather than a
  change. `SyncCollector::collect` breaks on a terminal or interrupted state, or
  on the reader returning `None`; that `None` requires the event queue to close,
  which requires `EventQueueManager::destroy`, which `CleanupGuard::drop` spawns
  as a task. In normal operation the executor is bounded by `executor_timeout`
  and a well-behaved executor reaches a terminal state, so the loop exits on the
  state check without needing EOF. The gap is narrow: an executor that completes
  without a terminal state, and a `destroy` that never runs, waits forever — and
  the only realistic way the spawn does not run is runtime shutdown, when the
  process is going away regardless. Not changed at the tail of a release, since
  bounding it means deciding what a blocking `message/send` returns when the
  drain gives up, which is a protocol answer and not a local one.
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

* **All 39 CI gates re-run locally at `6ebf821` on 2026-08-12 — 39 of 39 pass.**
  `scripts/preflight.sh --full`, with a live PostgreSQL so the 16 `#[ignore]`d
  `postgres_store_tests` actually execute rather than being skipped into a
  green. Notable timings: workspace clippy 233s, `cargo test --workspace` 233s,
  `agent-team --release --all-features` 343s.

  The first pass reported 38 pass / 1 fail, and the failure was **an artefact of
  the harness, not a defect**: `examples/incident-response` binds ports
  **9200, 9201 and 9202**, and a `@a2a-js/sdk` experiment running in the same
  session was holding 9200 and 9201, so the demo died with
  `Os { code: 98, AddrInUse }`. Re-run with the ports free it exits 0 with
  "15 passed, 0 failed, 0 not compiled, 1 not run" — the one `[NOT RUN]` being
  the PostgreSQL check, which that invocation does not set
  `A2A_TEST_POSTGRES_URL` for and which the separate `harden` gate does cover.

  Recorded because it is defect class 4 — a measurement artefact that reads
  exactly like a real failure in a summary line. **Anything run locally
  alongside this suite must avoid 9200-9202**, along with the TCK's
  9994-9999 and 9897-9899.

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
