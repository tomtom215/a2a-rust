<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Triage of the external review of 2026-11-25

The review: "A2A Rust SDK — Adversarial engineering review and implementation programme", audit cutoff
2026-11-25, with a 24-package addendum (EXT-01 to EXT-24, `a2a-rust-audit-addendum-2026-11-25.md` and
`a2a-rust-audit-backlog-2026-11-25.json`). Its author could not clone the repository or run Rust; every
claim in it is source reading, CI-log reading or document reading.

**Triaged against** `main` at `92593fc9c28f464722e264078a6b04ad9aaea2fa` (`git rev-parse origin/main`
after `git fetch origin main`), the same commit the review audited, so its line anchors apply unchanged.
Nothing in the repository was changed by the triage except this file.

**How each row was checked.** Source claims: the cited lines were opened and read, and the callers of the
cited function followed until the claimed consequence was confirmed or refuted. Where a test in the tree
already pins the behaviour, it was run (`cargo test -p a2a-protocol-client --lib -- retry::tests::non_idempotent_retried_on_503 retry::tests::only_429_503_are_safe_for_non_idempotent origin`, Rust 1.97.0:
22 passed, 0 failed). CI claims: the run and job records and full job logs for the 16 runs on the commit,
through the GitHub API. Counts: recomputed from the files (`spec-ledger.json`, `control-map.md`,
`status.json`, `06-open-items.json`) by script, never from prose. Upstream: `git clone` of
a2aproject/A2A, a2a-rs, a2a-tck and a2a-itk; crates.io; EUR-Lex. No new test was written; no defect was
reproduced end to end that was not already pinned by a test. "Confirmed" below means the code path was
read and holds, unless the row says a test ran.

Status values are the triage's: **confirmed**, **already planned** (a package in `status.json` already
requires it), **wrong** (the review misread or miscounted), **needs a decision**, **cannot check**.

## 1. Source and behaviour findings

| ID | Claim | Status | Evidence at 92593fc9 | Severity |
|---|---|---|---|---|
| R-COR-01 | A terminal status write that outlasts `store_write_retry` is lost and the task left unfinished | already planned | `state_machine.rs:198-218` reverts and returns `StoreFailed`; `processor/reconcile.rs:118-134` reads back, `processor.rs:293-301` unconfirms and leaves `ended` unset; `processor.rs:125-141` `close_out` returns when the stored task is not terminal; nothing re-queues the write. Register X-10; COR-01. The review's added Accept checks (kill at each persistence boundary, recovery on another replica, no stale overwrite) are **not** in COR-01's Accept | High |
| R-COR-02 | Cancelling a parked task writes the status and no event | already planned | `cancel_task.rs:155-160` `save_status_delta`, no log append. E-F25; COR-02 | Medium |
| R-COR-03 | A hand-off continuation re-presents the original budget | already planned | `continuation.rs:134` clones the message; server `handler/budget.rs:42` opens an account from the message at receipt. COR-03 | High (budgeted fleets) |
| R-COR-05 | The client retries into a halt and ejects the halted replica | already planned; **binding-dependent** | Server: `Halted` is HTTP 503 (`error/mod.rs:218`), gRPC `UNAVAILABLE` without `RetryInfo` (`:284`, `dispatch/grpc/helpers.rs:148`), JSON-RPC 200 with `-32603` + `ErrorInfo HALTED` (`dispatch/jsonrpc/response.rs:151-155`, `error/mod.rs:358`). Client: 503 retryable (`retry.rs:214`), retry-safe for non-idempotent sends (`:332`), origin `Failure` (`origin.rs:72-74`); test `what_says_the_origin_failed` asserts 503 is `Failure` and passed. gRPC → `HttpClient` (`transport/grpc.rs:597-613`): retryable for idempotent sends, origin `Failure`. JSON-RPC: not retried, origin `Success`. So REST and gRPC, not JSON-RPC. COR-05 step 1 asks for exactly this table | High (emergency stop) |
| R-ADV-01 | A continuation retry carries a new logical identity | confirmed; **not planned** | `continuation.rs:133-140` new `MessageId` and fresh idempotency key per call; `:111-125` calls it once per loop iteration and retries on every `Err`. Deliberate (`:131-132`). Whether a peer then executes twice was **not reproduced**. COR-06 covers the start, not continuations | Medium (High if reproduced) |
| R-ADV-02 | A 503 is treated as proof of non-execution | confirmed by a passing test; **needs a decision** | `retry.rs:315-340`; test `non_idempotent_retried_on_503` (`retry.rs:1785-`) asserts a bare 503 (`retry_after: None`) re-sends `SendMessage`, and passed. The gRPC client already treats `UNAVAILABLE` without `RetryInfo` as possibly-after-execution (`grpc.rs:597-601`); HTTP does not. COR-05's Accept (`cor-correctness.md:140-141`) requires the current behaviour. See decision D-1 | Medium |
| R-COR-06/07 | No atomic ownership claim on journal records; drop outside a runtime only logs | already planned | `delegation_journal.rs:140-190` (`begin`/`progress`/`settle`/`open(owner)`/`purge_settled`, no claim or fence); `drop.rs:17-22`. The review omits that the record stays unsettled and reattachable. COR-06, COR-07 | High (multi-supervisor) |
| R-ADV-03 | Production profiles accept declarations unverified; a custom store is taken as durable | confirmed, disclosed in rustdoc; **needs a decision** | `profile.rs:155-156` ("A store of your own is taken to be durable unless it says otherwise"), `:188-190` ("as declared, not verified"). See decision D-2 | Medium |
| R-OBS | Unbounded `server.address` on the client call metric | already planned | `call_metric.rs:130-133`; OBS-01 step 3 and its hostile arm | Medium |
| N-1 (triage) | `error.type` / `rpc.status_code` on the same metric take a peer-chosen JSON-RPC code (`call_span.rs:281`, `e.code.as_i32().to_string()`) | confirmed path; cardinality **not measured** | Belongs in OBS-01's hostile arm | Low |
| N-2 (triage) | A continuation's save can overwrite an event another replica's turn folded; "only a compare-and-set write would refuse the stale save" | confirmed as disclosed; **in no package** | `spec-ledger.json` §3.5.2 PARTIAL note, final paragraph; `tests/stale_write_position` covers the position half only. `grep -rn 'compare-and-set\|stale save' docs/program ROADMAP.md docs/audits/0.16.0-release/findings.json` finds nothing | Medium |
| R-COR-09 | No SQLite lease store | already planned | Only `store/lease/postgres.rs` and the in-memory store in `store/lease/mod.rs` implement `LeaseStore`. COR-09 | Medium |

## 2. Governance, CI and evidence claims

| ID | Claim | Status | Evidence |
|---|---|---|---|
| GOV-01 | `main` unprotected; ruleset `protect-main` disabled | confirmed | Branch API `protected: false`; ruleset 13934282 `enforcement: disabled` (rules `deletion`, `non_fast_forward`). Already recorded: `docs/handoff.md:208`, `:615`; register G1, BL-033. A repository setting: the maintainer's act |
| GOV-02 | The upstream ITK traversal did not run on the commit | confirmed, and stronger | Run 45663680816: in-repo self-test success, baseline job skipped. `itk.yml:102` runs it on `workflow_dispatch` only, `:103` `continue-on-error: true`; `itk-nightly.yml:132-134` exits 0 with failures; both track `A2A_ITK_REVISION: main` while ACTS is pinned to `bcce1a15` (upstream main is `604239c6`) |
| — | Tag `v0.16.0` signed, at `334fa45c` | confirmed | `verification.verified: true`, SSH signature |
| — | 16 runs on the commit, all green | confirmed; names partly wrong | Names are "TCK" (not "Internal TCK"), "ITK (upstream current-mount)", "Deploy Documentation" |
| — | CI run 45663680630, 24 jobs | wrong id | CI is 45663680911 (24 jobs, all success); 45663680630 is Official TCK |
| — | TCK full profile MUST 87/0/5/7/16 of 115; SHOULD 9/0/1/2/2; MAY 4/0/0/0/0; minimal 65/0; extension 1/0; pin `5e6f37c2` | confirmed | Job 139379952638 log lines 838, 1066-1068, 1234, 1301, 1358-1407. Pin is upstream HEAD |
| — | ACTS conformant on JSON-RPC, gRPC, REST | confirmed | Job 139379951391 log lines 2454, 2476, 2498 |
| — | Go interop 58 and 42 passed | confirmed | Job 139379952246 lines 672, 720 |
| — | "8 examples passed" | wrong | 8 is the check tally inside `resilient-agent` (job 139379952795 line 801); the job runs 3 examples; the swarm example's `llm` scenario was skipped (`SWARM_LLM_URL` unset) |
| — | Default workspace tests 3,378 passed, 16 skipped | wrong — **and `docs/handoff.md:92` carries the same error** | Job 139379952585 line 8555: default `--workspace` 7,973 passed, 271 skipped. 3,378/16 is the server-only `--features auth-jwt` step (lines 66569, 69970). All-features 8,000/271 and no-default 7,968/271 confirmed |
| — | Soak: 1,268,217 requests, 0 failures, 16.3 B/request; two replicas 67,575 pairs, 0 misses | confirmed | Job 139379951511 lines 1001, 1018, 1098 |
| — | Fuzz is smoke-length | confirmed | `fuzz.yml:105-114`: 60 s per target on push and dispatch, 600 s on schedule; 18 targets |
| — | Third-party actions should be pinned to SHAs | already done | 206 of 206 non-local `uses:` pinned to 40-hex SHAs; every workflow has top-level `permissions:` |
| — | 23 commits, 75 files, no `.rs` since `v0.16.0` | confirmed | 16 commits + 7 merges |

## 3. Dependencies and supply chain

| ID | Claim | Status | Evidence |
|---|---|---|---|
| — | Open PRs #223, #224, #225, #227 with the listed bumps | confirmed | All open, unmerged; every version matches |
| D-0543 | RUSTSEC-2026-0543 exception | confirmed | `deny.toml:79`: `chrono`, unmaintained, examples only via rmcp/schemars; removal condition, no date |
| D-0459 | Re-evaluate RUSTSEC-2026-0459 against sqlx 0.9.1 | confirmed; **actionable now** | `deny.toml:80`: "sqlx-sqlite 0.9.0, the latest (checked 2026-11-14)". sqlx 0.9.1 (published 2026-11-18) admits `libsqlite3-sys <0.41`; the advisory is patched `>=0.39`. #223's lockfile keeps `libsqlite3-sys 0.37.0`, so merging it alone does not remove the exception; a fresh resolution picks 0.40.0 |
| D-0592 | Consumers can still resolve a vulnerable `url` | confirmed | No published crate depends on `url` directly; `sqlx-core` and `sqlx-sqlite` require `url = "2.2.2"`. A downstream project on the server crate with `sqlite,postgres` accepted `cargo update -p url --precise 2.5.8` and `--precise 2.2.2`. No `Url::origin()` call in sqlx-core/-sqlite/-postgres 0.9.0 (grep), so exposure is resolution, not a known call path |
| N-3 (triage) | `scripts/check_advisory_floors.py` checks direct requirements only | confirmed; **in no package** | `requirements()` (`:105`) reads the four manifests' `dependencies`/`build-dependencies`; transitive advisories (the `url` case above) are invisible to it while CI is green |
| — | Waivers expire with an owner | not present | Neither `deny.toml` exception has a date or owner |
| — | MSRV 1.94; stable 1.100.0 | confirmed | `Cargo.toml:39`; `channel-rust-stable.toml` "1.100.0 (0ef1bc386 2026-11-09)" |

## 4. Specification, upstream and comparison

| Claim | Status | Evidence |
|---|---|---|
| No final A2A v1.1.0 tag; `v1.1.0-rc` at `78e1d6a1` (2026-10-29); `dev-1.1` newer | confirmed | `git ls-remote --tags`; `dev-1.1` HEAD `3d92e711` (2026-11-24) |
| Upstream #2426 adds task/artifact/event timestamps, deprecates the status timestamp and its filter, relaxes ordering | confirmed | Commit `3d92e711`; the ordering change (MUST status-timestamp → SHOULD `createdAt`, MAY `updatedAt`) is in `docs/specification.md`, the fields in `specification/a2a.proto` |
| Ledger: 510 entries, 455 TESTED / 32 APPLICATION / 19 NOT_APPLICABLE / 4 PARTIAL; the four sections | confirmed | `spec-ledger.json`, counted by script |
| a2a-rs @ `74ebd92c`: MSRV 1.85, optional native-tls, in-memory task store only, no WebSocket, five-platform CLI | confirmed | Its `Cargo.toml:35`, `a2a-server/src/task_store/inmemory.rs:75`, `release-binaries.yml` |
| a2a-rs fixed message-only stream cancellation on 2026-11-25 | confirmed | `74ebd92` (#588), with `f61f4a7` (#554) and `9832b2e` (#556). Whether this SDK has the same defect is **not checked** |
| Historical figures (7,757 / 107 µs; 4,228 / 228 µs; 0.14.1 6,090 / 157 µs) | confirmed, with a flaw the plan shares | 6,090 is `sdk-comparison-2026-11-11.md:86` (table 2, CPU not reported); 157 µs is `:194` (table 4, where 0.14.1 served 5,379). The pair mixes two tables; `docs/program/README.md:97-99` pairs them the same way |
| Target sheet values | confirmed, one term wrong | "Recovery ≤45 s for a 15 s lease" is a turn-claim TTL (`target-sheet.md:43`) |
| Programme: 93 packages, 1 done; per-workstream counts | confirmed but FND wrong | FND has 12 (11 todo + FND-01 done); the review's per-workstream figures sum to 92 |
| 206 open-items rows | confirmed | partial 86, open 78, decision 13, stale-claim 12, maintainer-action 7, waiting-upstream 6, disclosed-accepted 3, not-verifiable 1 |

## 5. Regulatory

| Claim | Status | Evidence |
|---|---|---|
| Regulation (EU) 2026/1744 in force 27 July 2026 | confirmed | EUR-Lex: OJ L 24.7.2026, "third day following" publication |
| AI Act dates 2 Dec 2026 (new Art. 5 prohibitions), 2 Dec 2027 (Annex III), 2 Aug 2028 (Annex I) | confirmed | Consolidated text 2026-07-27, Art. 113(a), (c). The 2 Dec 2026 Art. 50(2) transition is Art. 111(4) |
| CRA reporting from 11 Sep 2026, general application 11 Dec 2027 | confirmed | Reg. 2024/2847 Art. 71(2) (Art. 14 from 11 Sep 2026; Chapter IV from 11 Jun 2026) |
| Control map 88 rows: 15 / 4 / 29 / 40 / 0 | confirmed | `control-map.md`, five Status tables |
| GDPR rows 22: 2 / 7 / 13; ten erasure limitations | confirmed | `control-map.md:90-120`; `book/src/compliance/gdpr.md` "What erasure does not reach" |

## 6. The 24 proposed packages against the plan

Each EXT package was read clause by clause against the package sections it names and every other section
that overlaps (`docs/program/plan/*.md`), `README.md` §3, §4 and §7, and the target sheet. Mechanical
checks: every dependency id resolves; the EXT graph and the combined 117-node graph are acyclic; the
addendum's order is a valid topological order.

| EXT | Result | Anchors |
|---|---|---|
| 01 | Settings change is the maintainer's act (README §4 "Acts that stay the maintainer's"); DCO and provenance already built (`release.yml:416-531`, `scripts/verify_tag_signature.sh`). New: negative tests in a disposable repository | FND-09, FND-11; register G1, BL-033 |
| 02 | Amends FND-09: the manifest exists (`scripts/release_evidence.py`, `scripts/ci_evidence.py`). New: a skipped mandatory job blocks; waivers expire with owner; upstream refs and test counts recorded; negative fixtures. Blocking needs E06 §6 row 12 decided | FND-09 |
| 03 | Amends COR-01/COR-02/COR-10. New: transactional state/event/outbox contract, fault matrix at each write, outage plus kill, recovery on another replica, bounded buffers | COR-01, COR-02, COR-10 |
| 04 | Amends COR-06 for starts (key always sent, DEC-08). New: request fingerprint and conflict rejection, explicit ambiguity result, persist-before-send ordering, continuation identity (R-ADV-01). Its 503 clause conflicts with COR-05's Accept: decision D-1 | COR-05, COR-06, SWM-14 |
| 05 | Claim/fence is COR-07; conservation is COR-03/SWM-04. A durable cross-replica budget account is the open S3 decision (README:184, :328) | COR-03, COR-07, SWM-04; D-3 |
| 06 | Authenticated halt route is SWM-08. Halt reaching parked and recovered tasks is out of scope of COR-08 and in no package: new. Approval is digest-bound (approval-v1); actor/tenant/policy/expiry/execution binding is new | SWM-08, COR-08 |
| 07 | Profiles and refusals exist (`profile.rs`); SWM-03 adds fleet. New: declared / configured / contract-tested / operationally-verified split; detecting a store that misreports; readiness showing degraded controls | SWM-03, DVX-03; D-2 |
| 08 | RFC traceability is COR-11; token exchange SWM-12; tenant scoping SWM-15. New: threat model, workload identity, per-binding cross-tenant suite, rotation/replay/confused-deputy tests | COR-11, SWM-12, SWM-15 |
| 09 | New. Sandbox execution crosses README §3 principle 3 ("A library, not a runtime") | — |
| 10 | New | — |
| 11 | New; primitives exist (`erase_caller`, `erase_subject`, holds) | register BL-046, X-19 |
| 12 | New; explicitly out of scope of COR-01 ("A durable audit spool (X-19)") | register A3, X-19 |
| 13 | Amends SPC-12/SPC-13 (it names COR-10, which is unrelated). A release gate on real ITK peers needs E06 §6 row 12 | SPC-12, SPC-13 |
| 14 | Amends SPC-10 (it names SPC-13, unrelated). New: mixed-version fleets, replay equivalence, downgrade, rollback | SPC-10 |
| 15 | Fan-out/partial success/recovery are SWM-04/SWM-05. Durable groups deferred by ADR 0033. "Dependency outcomes", "compensation", "queue saturation" cross README §7 (DAG, workflow, work queues) | D-3 |
| 16 | Conflicts whole: README §7 "Fleet-wide admission and an inference quota. S3 needs its own decision first" | D-3 |
| 17 | 60 replicas and multi-host faults are SCL-03/SCL-08. Multi-region conflicts with README §7. New: zone loss, failover, restore, RTO/RPO | SCL-03, SCL-08; D-3 |
| 18 | Pairing and repetitions are PRF-07. "Meet approved target sheet" conflicts with DEC-03 (thresholds stay PROPOSED). New: profiles, open-loop traffic, slow readers, long soak | PRF-07; D-4 |
| 19 | Near-duplicate of OBS-01..08. New: budget and lease-age signals, SLOs by profile, incident drills | OBS-01..08 |
| 20 | Reference, doctor, book are DVX-03/DVX-09/SWM-09. "Deterministic test agents" is a separate decision under DVX-05 | DVX-03, DVX-05, DVX-09 |
| 21 | Much exists (FND-06, SBOM, SLSA, `check_advisory_floors.py`, CRA page). New: per-update qualification, minimal-versions build, waiver expiry, per-feature licences — and N-3 (transitive floors) | FND-06, FND-09 |
| 22 | Contracts, middleware, catalog are ECO-04..07. "Policy-filtered selection" conflicts with README §3.3 and ECO-05; "contribute upstream" is the maintainer's act | ECO-04..07 |
| 23 | Near-duplicate of ECO-03 (it names ECO-01, unrelated). New: packaging/MSRV/diagnosis columns; port a2a-rs #588 as a regression scenario | ECO-03, PRF-07 |
| 24 | Register dedup is FND-05; fail-before/pass-after is README §6. New: module-level independent review inventory, interleaving tests, long fuzz campaigns | FND-05 |

The review's G0–G5 gates are a second taxonomy across the `release` field (`0.16.x` 25, `0.17` 31,
`none` 35, unstated 2) and `docs/readiness-bar.md`; see decision D-5.

## 7. Stale claims in this repository found by the triage

- `docs/handoff.md:92`: "the default-features run: 3,378 tests passed, 16 skipped" — the CI default
  workspace run is 7,973 / 271; 3,378 / 16 is the server-only step.
- `docs/program/README.md:97-99` (and `plan/prf-performance.md`): "0.14.1 served 6,090 at 157 µs"
  pairs figures from two tables of `docs/sdk-comparison-2026-11-11.md` (`:86`, `:194`).
- `deny.toml:65-68,80`: "sqlx-sqlite 0.9.0, the latest (checked 2026-11-14)" — sqlx 0.9.1 exists and
  meets the stated removal condition.

These belong to FND-03.

## 8. Decisions this triage needs

- **D-1 (503 for a non-idempotent send).** COR-05's Accept keeps any 503 retry-safe; the review and the
  gRPC client's own reasoning say a 503 without a typed refusal may follow execution. Options: (a) keep;
  (b) align HTTP with gRPC: a 503 is retry-safe for a non-idempotent send only with `Retry-After` (this
  server's overload refusal carries one, `error/mod.rs` `OVERLOAD_RETRY_AFTER`; its halt does not), else
  ambiguous; (c) only a typed `ErrorInfo`/`RetryInfo` refusal is retry-safe. Recommended: (b), amending
  COR-05's Accept to "a 503 with `Retry-After` and no halt reason is still retried". Sends that carry an
  idempotency key a peer honours stay retryable either way.
- **D-2 (custom store durability default).** Options: (a) keep "durable unless it says otherwise", add a
  store contract-test kit and report the store's durability as declared in `profile_report`; (b) flip the
  default to process-local (breaking for every custom store; 0.17). Recommended: (a).
- **D-3 (scope).** EXT-05's durable distributed budget, EXT-16 entirely, EXT-15's DAG/compensation/queues,
  EXT-17's multi-region, EXT-22's policy-filtered selection cross README §7 or §3.3. Recommended: keep the
  exclusions and adopt only the non-conflicting clauses; S3 stays its own open decision.
- **D-4 (EXT-18 "meet approved target sheet").** Recommended: drop the clause; DEC-03 stands.
- **D-5 (G0–G5).** Recommended: do not add a second gate taxonomy; carry EXT-02's blocking rules into
  FND-09 and the readiness bar.
- **D-6 (new packages).** Whether to add packages for the items in no package: N-2 (compare-and-set on
  the stale save), N-3 (transitive advisory floors), R-ADV-01 (continuation identity, or fold into
  COR-06), the parked/recovered-task halt (EXT-06), and EXT-09, -10, -11, -12.
- **Already open, needed by EXT-02 and EXT-13:** E06 §6 row 12, the nightly evidence gate.
- **The maintainer's acts:** enabling the `protect-main` ruleset and a `v*` tag ruleset (GOV-01).
