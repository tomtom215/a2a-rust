<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# The post-0.16.0 program: plan of record

Written 2026-11-25 against `main` at `2058058d` (the merge of #222; `v0.16.0` is `334fa45c`). It is the
"Phase 2" plan that `docs/handoff.md` asks for: **a plan and a target sheet for the maintainer's approval.**
Nothing in it is built. Nothing in it is in `ROADMAP.md`, which takes only committed items; a work package
moves there when the maintainer approves it.

This directory is written so that a fresh session with no memory of any earlier one can pick up one work
package and land it. Read this file, then the one workstream file your package is in. You do not need the
others.

| File | What it holds |
|---|---|
| `README.md` (this file) | What the deep dive found, the decisions the maintainer owes, the session protocol, the order of work |
| [`status.json`](status.json) | Every work package: id, workstream, dependencies, decision gates, status. The only place status lives |
| [`target-sheet.md`](target-sheet.md) | The proposed measurable targets (scale, recovery, latency, memory, cold start, cardinality) |
| [`plan/`](plan/) | One file per workstream; each work package in a fixed template |
| [`tools/plan_check.py`](tools/plan_check.py) | Checks this directory (ids, gates, cycles, cited paths) and keeps `status.json` in step with `plan/` |
| [`evidence/`](evidence/) | The seven audits this plan rests on, frozen as written on 2026-11-25. Cited below as E01 to E07 |

Evidence files: E01 [v1.1 delta](evidence/01-spec-v1.1-delta.md), E02 [peer landscape](evidence/02-peer-landscape.md),
E03 [swarm audit](evidence/03-swarm-audit.md), E04 [developer experience](evidence/04-devex-audit.md),
E05 [observability, performance, scale](evidence/05-observability-perf-scale.md),
E06 [open items](evidence/06-open-items.md) with its [machine-readable rows](evidence/06-open-items.json),
E07 [engineering system and architecture map](evidence/07-engineering-system.md).

## 1. How far to trust this

- **The audits were read-only.** No audit built, ran or benchmarked anything. Every performance number in
  them is quoted from a results file already in the repository. Every "candidate cost" in E05 §2.5 is
  labelled CONJECTURED by its author and is a hypothesis to measure.
- **Anchors are of `2058058d`.** Each `path:line` was true of that commit. Before you edit, find the symbol
  again (`grep -n`), and if it moved, correct the work package in the same pull request.
- **What was checked mechanically when this was written**, by `python3 docs/program/tools/plan_check.py .`:
  the ids, titles, dependencies and gates in `status.json` are written from the package sections and
  compared with them on every run; every dependency and gate
  resolves; the dependency graph has no cycle; the gate column of §4 agrees with the packages; every package
  has an Accept field. The script also prints every path-like token that does not resolve to a file or
  directory in the tree. Those were read one by one, which is a judgment and not a check: each is a file a
  package creates, a bare file name whose full path is given beside it, a URL route, or a path in the
  upstream repository. The evidence files were not
  re-verified line by line; these claims of theirs were re-checked by hand against the code and held:
  no `HALTED` handling in the client crate, `peer_honours_idempotency: false` in `ClientBuilder::new`,
  the single `A2A_VERSION = "1.0"`, ADR 0033's "Proposed" status, the `timeout_at` in
  `crates/a2a-protocol-server/src/streaming/event_queue/applied.rs`, the continuation that clones the first message, `Delegated` with no
  artifacts, no SQLite lease store.
- **Two audits ended early** when the session's usage limit was reached (E04 and E06). Both had written
  every section; E06's cross-references were being corrected when it stopped. Treat an E06 `partial` row as
  needing a re-check before you plan from it (E06 §2 says the same of the registers it was built from).
- **Not done by anyone:** GitHub issues and pull-request threads upstream were not read (the clone has git
  objects only), so "upstream states no release date for 1.1" means "none in the repository".

## 2. What the deep dive found

Ten findings decide the shape of the plan. Each names its evidence.

1. **The swarm primitives are real, and unique among A2A SDKs; the layer above one child does not exist.**
   Delegation with cascade cancel, leases, hop-shrinking budgets, lineage with root halt, a durable journal
   with reattach, steering, admission, a load signal, client balancing with ejection and hedging: all
   compile in every build (E03 §1). None of the six official SDKs has lineage, cascade cancel, halt,
   budgets, leases or a journal (E02 §3). But `Delegation` is the only orchestration type: a group of
   children (ADR 0033) is "Proposed. Nothing here is built", and there is no recovery driver (E03 §3 G2,
   G3). Three of the five delegating examples call `send_message` directly, so they demonstrate the gap
   the SDK exists to close (E03 §2).
2. **Five defects in the delegation path have no covering test, and a group would multiply each by N**
   (E03 §4): a handed-off child gets its whole budget again (R1); this SDK's own client retries into a halt
   and ejects halted replicas (R2); `Canceled` does not say whether a lease lapsed, an operator halted or
   someone cancelled (R3); two supervisor replicas reattach the same children and one cancels the other's
   (R4); `ClientBuilder::new` delegates with no idempotency key, so an ambiguous start cannot be recovered
   (R5). They come before any new surface.
3. **The differentiator stops at the language boundary.** Every swarm control is a private
   `a2a-rust.com/extensions/…` URI. A peer in another language receives them and ignores them (ADR 0049);
   the middleware for other SDKs is unbuilt (E02 §6). A2A 1.1 adds no lineage, cascade, budget or lease
   (E01 §3.2), so the gap stays open upstream: that is the opening for the drafts already in
   `docs/upstream/`.
4. **A2A 1.1 is larger than the handoff records, and still moving.** Beyond `SendLiveMessage`, the timeline
   and `generation`: optimistic concurrency (`if_generation_match`, error `-32010`), long-polling
   (`current_generation`), generation-based resume, client-written artifacts, a per-push-config protocol
   version, and streams that no longer close on interrupted states. Two commits after the `v1.1.0-rc` tag
   already broke the rc's JSON and relaxed a MUST (E01 §1.4, §2). The official Rust SDK has an
   eight-branch 1.1 stack (E02 §2).
5. **Three things can be done for 1.1 now with no risk from the moving target** (E01 §6): the client must
   skip an unknown stream payload instead of failing the stream; the negotiated version must be carried
   through the request path instead of validated and dropped; `generation` cannot be derived from the
   ADR 0012 `seq` and needs its own dense counter.
6. **The throughput gap has two named, countable causes.** At one connection a2a-rs serves 7,757 blocking
   sends per second at 107 µs of server CPU each; this tree serves 4,228 at 228 µs, and 0.14.1 served 6,090
   at 157 µs (`docs/sdk-comparison-2026-11-11.md`). Of 257 wake-up writes sampled, 76 come from hyper's
   per-request header timer and 50 from a `timeout_at` around a watch receiver (E05 §2.3). The regression
   gate cannot see either: it compares two in-process benchmarks at a 50% threshold (E05 §2.7).
7. **A fleet operator is missing the signals that say a fleet is unwell.** No admission queue or wait
   metric, no database pool metric, no push outbox depth or age, no event-log lag, no halt or lease
   propagation latency, no burn-rate alert; client retries, ejections and hedges are log lines; the
   telemetry-failure counter exists and nothing reads it; one client metric has an unbounded label
   (E05 §1.6, §1.7).
8. **One data-loss path is open and disclosed.** A store outage longer than `store_write_retry` (30 s) loses
   the event being written and can leave a task unfinished (register row X-10). It has no severity in its
   register and it is the first correctness item here.
9. **"What is open" has six overlapping sources and none is authoritative.** 206 rows, 190 of them open
   work, carried under ten colliding id series; fixes landed without the register changing; 22 documents
   contradict each other or the code (E06 §4, §5, §9). The four files a session is told to read first total
   about 1.38 MB.
10. **Per-change toil is concentrated in a few couplings.** 3,751 `path:line` citations in the two ledgers,
    one of which every `[Unreleased]` changelog entry breaks; the mutation test filter in five hand-synced
    copies; a 984 K changelog; a `mutants.toml` that nothing reads, guarded by a gate (E07 §A4).

### The thesis, tested

"Other stacks need several components and glue on top of their A2A SDK to run a swarm; this one has it built
in." E02 §6 gives the verdict: **true for the control of a delegation tree built on this SDK alone, false as
a general claim about fleets.** It fails on polyglot fleets (finding 3), on discovery (mDNS and a card cache;
the ecosystem is converging on catalogs), on orchestration logic (out of scope by this project's own
decision) and on distribution (the official Rust SDK is the one on a2a-protocol.org with a published CLI).
The plan below is ordered to make the claim true where it can be: correct first, then composed, then
carried across languages.

## 3. Principles this plan holds to

1. **Correct before composed.** No group, driver or new surface lands on a path with a known untested
   defect. COR precedes SWM.
2. **Subtraction first.** Before adding an API, remove a concept, a copy or a default that makes one
   necessary. E04 §6 and E07 §A4.3 are mostly subtraction and are preferred for that reason.
3. **A library, not a runtime.** The recorded decision stands: no registry, scheduler, orchestration DSL or
   mesh (`docs/handoff-history.md`, "What not to chase"; ADR 0033 "What stays out"). The swarm layer owns
   handles and policies and chooses nothing. Each package that could cross that line says how it does not.
4. **Measure with counts where the machine cannot measure time.** A session container can count wake-ups,
   instructions, allocations, statements and series, and can test fault correctness. It cannot measure
   throughput or a latency tail (the comparison records 20% variance per cell). Every performance package
   is gated on a count.
5. **One source of truth, generated views.** Status in one register; citations by symbol; lists derived.
6. **Nothing built against an unreleased specification in a default build.**
7. **The evidence bar does not drop.** Zero surviving mutants in changed lines, a failing test before a fix,
   the provers. Simplifications remove copies and couplings, never a check.

## 4. Decisions the maintainer owes

A package names its gates in `status.json`. A gated package is not started until the decision is recorded in
`docs/handoff.md`, *Decisions the maintainer made*, and its `status` here is set. "Recommended" is this
plan's advice with its reason; it is not a default a session may assume.

| Id | Decision | Recommended, and why | Gates |
|---|---|---|---|
| DEC-01 | **Scope of a first-class swarm layer.** The request of 2026-11-25 asks for swarms as the differentiator; "What not to chase" forbids a runtime. | Three client-side layers over what exists (a turn-scoped delegate, ADR 0033's group, a recovery driver), in the existing crates, with no wire change and no registry, scheduler or DSL. Accept ADR 0033. Layer C reverses ADR 0030's "not built: a recovery driver"; the live proof found three defects in the loop adopters were told to write (E03 §6.2). | SWM-02, SWM-04, SWM-05, SWM-07 |
| DEC-02 | **A2A 1.1: track it now or wait** (ROADMAP conformance item 8). | Track it. The three forward-compatible changes and a delta ledger carry no risk; types go behind an off-by-default unstable feature; live messaging, `timeline_length`, timestamps and list re-ordering wait for a non-rc tag (E01 §6). | every SPC package but SPC-01, SPC-04, SPC-12 and SPC-13; ECO-05 |
| DEC-03 | **The target sheet** ([`target-sheet.md`](target-sheet.md)). | Approve the quantities and procedures; leave each threshold PROPOSED until its first measurement. | SCL-01 to SCL-06, SCL-08, SWM-10, PRF-01 |
| DEC-04 | **Who a session commits as.** Recorded 2026-10-08: commits are authored as Tom F. with one sign-off; `dco.yml` rejects an assistant author. A session's harness asks for other trailers. | Reaffirm, in a `CLAUDE.md` at the repository root, so a session reads the rule instead of finding it in a 1,519-line handoff. FND-01 writes the file; only the maintainer can decide what it says. | FND-01, FND-11 |
| DEC-05 | **Pin the toolchain a session and CI lint with** (ROADMAP release item 8). | Pin in a file CI writes and `preflight.sh` reads (E07 §A4.3 S12). | FND-06 |
| DEC-06 | **Send the upstream drafts** (ROADMAP conformance item 7), lineage and halt, and deadline and budget, first. | Send those two. 1.1 added neither, and an extension under an upstream namespace is what makes the swarm controls reach other SDKs. | ECO-02 |
| DEC-07 | **Split `CHANGELOG.md` by release** (ROADMAP book item 4). | Split, keeping `[Unreleased]`, the newest release and an index (E07 §A4.3 S5). | FND-10 |
| DEC-08 | **Idempotency by default for a delegation** when the peer is not known to honour keys (R5). | Send the key always; a peer that ignores it is no worse off, and a peer that honours it makes an ambiguous start recoverable. Needs a look at ADR 0026 first. | COR-06 |
| DEC-09 | **Lineage attached automatically** to a delegation made inside a turn (E03 §6.4 Q6). | Yes, inside the turn-scoped delegate only; a bare `Delegation::start` stays as it is. | SWM-02 |
| DEC-10 | **An opt-in authenticated halt route** (E03 §6.4 Q8), against ADR 0049's reason for keeping halt a Rust method. | Ship it off by default, refused by `Profile::Production` without an authenticating interceptor. Every adopter otherwise writes one. | SWM-08 |
| DEC-11 | **`/health` and `/ready` on `JsonRpcDispatcher`** (E04 §6 item 8); the book documents "not served" as the contract. | Serve them. | DVX-06 |
| DEC-12 | **Publish the CLI, the SLIMRPC binding and the discovery crate** (ROADMAP developer-experience item 6; E06 §6 rows 8, 9, 21). | Publish the CLI first: the official Rust SDK has one and this project's is `publish = false`. | ECO-01 |
| DEC-13 | **A group may retry** (E03 §6.4 Q4; ADR 0033 says no). | Keep ADR 0033: the group reports which members are retryable, the caller loops. Revisit with a named adopter need. | SWM-04 |
| DEC-14 | **Loop detection** by an agent path in lineage, or depth 16 as the only bound (E03 §6.4 Q9). | Keep depth as the bound and add a width limit (SWM-06); an agent path leaks identities to every hop. | SWM-06 |

Twenty-seven decisions that predate this plan are listed, with their recorded options and what each blocks,
in E06 §6. This plan does not restate them. Three gate packages here: the evidence gate against the nightly
job (E06 §6 row 12, gates FND-09), the fleet inference quota S3 (row 16, gates nothing built here; OBS-07 is
the accounting half that needs no decision), and the spec-added field rule (row 5, to be answered with
DEC-02).

## 5. The workstreams

| Prefix | Workstream | File | Why it exists |
|---|---|---|---|
| FND | Foundation: the engineering and document system | [`plan/fnd-foundation.md`](plan/fnd-foundation.md) | Findings 9 and 10. Every later session is cheaper once these land |
| COR | Correctness of the fleet and delegation paths | [`plan/cor-correctness.md`](plan/cor-correctness.md) | Findings 2 and 8 |
| SWM | The swarm layer | [`plan/swm-swarm.md`](plan/swm-swarm.md) | Finding 1 |
| SPC | Specification: 1.0.1 remainder and 1.1 readiness | [`plan/spc-spec.md`](plan/spc-spec.md) | Findings 4 and 5 |
| PRF | Performance | [`plan/prf-performance.md`](plan/prf-performance.md) | Finding 6 |
| OBS | Observability | [`plan/obs-observability.md`](plan/obs-observability.md) | Finding 7 |
| SCL | Scale and qualification | [`plan/scl-scale.md`](plan/scl-scale.md) | The readiness bar's unmeasured rows; the target sheet |
| DVX | Developer experience | [`plan/dvx-devex.md`](plan/dvx-devex.md) | E04 |
| ECO | Ecosystem and interoperability | [`plan/eco-ecosystem.md`](plan/eco-ecosystem.md) | Finding 3 |

### Order of work

The order is by dependency, not by date. `status.json` is authoritative for dependencies; this is the
reading of it. Within a wave, packages are independent and can be separate sessions.

- **Wave 0, needs no decision, unblocks the rest.** FND-02 (the changelog citation that breaks the ledger
  gate on every entry), FND-03 (the stale claims, so no session plans from a wrong row), FND-04 (a
  build-free pre-commit command), COR-01 (X-10), COR-02 (the parked cancel), PRF-01's count harness once
  DEC-03 is answered, SPC-01, SPC-04, SPC-12, OBS-01, DVX-01, DVX-02, DVX-04, SWM-01.
- **Wave 1, the delegation defects.** COR-03 to COR-09. COR-03, COR-04 and COR-05 gate SWM-02; COR-06 and
  COR-07 gate SWM-05.
- **Wave 2, composition.** SWM-02, then SWM-04 and SWM-05, with SWM-03 beside them; SWM-09 (the book part
  and the rewritten example) closes the wave. PRF-02 to PRF-06 and OBS-02 to OBS-06 run beside it; they
  touch different modules.
- **Wave 3, proof.** SCL-01 to SCL-08, SWM-10 (the swarm at scale), PRF-07 (re-measure and publish), ECO-03
  (the refreshed comparison).
- **Wave 4, 1.1.** With DEC-02: SPC-02 and SPC-03 (the reference text and the delta ledger), SPC-05 to
  SPC-09 behind a feature, then SPC-10 when upstream tags a release.
- **Any time, each its own decision.** ECO-01, ECO-02, FND-06, FND-09, FND-10.

A release note for the planner: COR and FND packages are `[0.16.x]` material by `STABILITY.md` §3 unless
their section says "breaking". Each package states its own release line; DVX-07 is the one that is breaking by design.

## 6. Session protocol

One work package per pull request, unless a package says it pairs with another.

### Before the first edit

1. Read this file's §3 and §6, `status.json` for your package, the package's section, and the evidence
   sections it names. Read `docs/handoff.md` from *Now* to *Environment facts that still hold*.
2. Check the gates: every `gates` id of your package must be recorded as decided. If one is not, stop and
   report; do not build on the recommended answer.
3. Check the dependencies: every `deps` id must have `status: "done"`.
4. Set up the container: E07 §A2 "Step 0" is the ordered recipe (the toolchain CI uses, `git fetch
   --unshallow --tags origin`, PostgreSQL with `PGSSLMODE=disable`, `CARGO_INCREMENTAL=0
   CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0`). E07 §A3 is the list of traps earlier sessions
   fell into; read it once.
5. Re-find every anchor your package names. If one moved or the claim no longer holds, correct the package
   text first, in the same pull request, and say so in its description.

### While working

6. **Reproduce first.** For a defect, write the test that fails on the tree as it stands, run it, keep the
   failing output for the commit message. For a performance package, record the "before" count with the
   named harness before changing anything.
7. Obey the rule list in E07 §A1. The ones a session breaks most: one `Signed-off-by` and no other trailer
   (`CONTRIBUTING.md`, *AI-assisted contributions*); SPDX header on every new file; a new `.rs`, `.sh` or
   `.py` file stays under 500 lines; no `unwrap()` in library code; a new public name follows
   `CONTRIBUTING.md`, *Naming a new public item*; a new trait method has a default, a doc paragraph saying
   what not overriding it costs, and every implementation in the repository updated in the same commit.
8. Use the co-update set for your kind of change (E07 §A4.2): a bug fix, a new public API, a wire change, a
   new ADR and a new feature flag each oblige a different list of files.
9. Stay inside the package's "Out of scope". If you find another defect, add a row to
   `docs/audits/0.16.0-release/findings.json` (or the register FND-05 creates) and leave it.

### Before pushing

10. Run, in this order (E07 §A2 has each command and its pitfalls): `cargo fmt --all -- --check`; the two
    ledger checkers; `scripts/check_file_lengths.sh` after `git add`; `scripts/run_workflow_job.py
    .github/workflows/ci.yml static-checks`; clippy as CI runs it; the tests of the crates you touched with
    nextest, then with `--no-default-features`; the PostgreSQL job if you touched a store; the incremental
    mutation gate on your diff (E07 §A2 "Step 9"); `cargo semver-checks` if you touched a public item.
11. Run the package's **Accept** checks and paste each command's output into the pull-request description.
    A check you could not run is reported as not run, with the reason. It is never reported as passed.
12. Open the pull request against `main`, add the `mutation-test` label at once, and do not re-run the
    first, unlabelled mutation run.

### Closing a package

13. In the same pull request: set the package's `status` in `status.json` (`done`, with the pull request
    number in `landed`), run `python3 docs/program/tools/plan_check.py .` (after `--write` if you changed a
    package's dependencies, gates or title), add the `CHANGELOG.md` entry under `[Unreleased]` if it is user-visible, update the
    register row it closes, and rewrite `docs/handoff.md` *Now* and *First, next*.
14. If the package ends without meeting an Accept check, the status is `partial` and the section gains a
    dated "What is left" paragraph. A package is `done` only when every Accept check ran and passed.

### Stop and ask the maintainer when

- a gate is undecided, or the work would need a decision this file does not list;
- the fix needs a breaking change the package did not declare;
- an Accept check cannot be met without weakening a test, an assertion or a tolerance;
- a measurement contradicts the premise of the package (report the measurement; that is the result).

## 7. What this plan deliberately leaves out

Each was considered and has a reason. Reopening one is a decision, not a session's call.

- **A registry, a scheduler, a DAG or workflow DSL, a blackboard, work queues, restart strategies in the
  Erlang sense, a generic map-reduce** (E03 §6.5). No measurement exists above 256 workers, two hops and one
  host; SWM-10 produces it before any of these is argued.
- **Multi-region.** Undesigned; no source in the repository mentions it (E05 §3.6).
- **A proc-macro crate, typestate builders, a `from_env` configuration layer** (E04 §6, "considered and
  recommend against").
- **v0.3 protocol compatibility and interactive OAuth grants**, which five and one of the official SDKs
  have (E02 §2). Neither serves the swarm goal; each is a separate product decision.
- **Fleet-wide admission and an inference quota.** S3 needs its own decision first.
- **Splitting `tasks.data` into columns or a history table.** ADR 0047 declined the SQL history splice with
  a measurement; SCL-04 measures write amplification before anything is redesigned.
- **Building `SendLiveMessage`** before upstream defines a binding for it. No binding section mentions it
  and upstream's own ten conformance tests for it are tagged tentative (E01 §5.2).
