<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# SWM: the swarm layer

Read [`../README.md`](../README.md) §3 and §6 first, then E03 in full: it is the inventory (§1), the glue
count (§2), the gap list (§3), the traced risks (§4) and the proposed direction (§6) this file turns into
packages.

Abbreviations: `S/` is `crates/a2a-protocol-server/src/`, `C/` is `crates/a2a-protocol-client/src/`,
`T/` is `crates/a2a-protocol-types/src/`.

## The design in one page

**What exists** is one handle, `a2a_protocol_client::delegation::Delegation`, and a set of independent
controls around it (lease, budget, lineage and halt, journal, steering, admission, balancing). Getting all
of them to agree takes about fifteen server builder calls, six steps in the executor, five client calls and
four pieces of operator code, with nothing that checks they agree (E03 §5).

**What is added** is three thin layers, each a wrapper of what exists, and one check:

| Layer | Makes correct | Wraps | Package |
|---|---|---|---|
| A, a turn-scoped delegate | one child | `RequestContext::{cancellation_token, child_lineage, budget}`, `DelegationOptions`, `Delegation::start_with`, `wait_or_hand_off` | SWM-02 |
| B, a group | N children | `Delegation`, `BudgetHandle::grant`, `steer_all`, `FailureClass::is_retryable` | SWM-04 |
| C, a recovery driver | a restart | `DelegationJournal::open`, `Delegation::reattach_with` | SWM-05 |
| a fleet profile | the configuration | `Profile` and `profile_report()` (ADR 0025) | SWM-03 |

**The line that is not crossed** (README §3 principle 3; ADR 0033 "What stays out"): the layer owns handles
and policies and chooses nothing. It does not pick which agent does a job, does not plan, does not store a
topology. Placement stays `EndpointSource` plus `BalancePolicy`. No wire change is needed for A, B or C.

**What is not built**, and the test for reopening each, is in README §7. If a package below seems to need a
registry, a scheduler or a retry engine to finish, stop: that is the signal the design went wrong.

**Order.** COR-03, COR-04 and COR-05 before SWM-02. COR-06 and COR-07 before SWM-05. A group over an
unfixed handle multiplies each defect by its width.

---

## SWM-01: Make what exists findable, and make the examples use it

- **Goal.** An adopter finds the swarm surface from the prelude and from examples that are themselves
  correct. No new type.
- **Evidence.** E03 §5 (the prelude exports none of `Delegation`, `DelegationOptions`, `Outcome`, `Lineage`,
  `Budget`, `BudgetHandle`, `FailureClass`, `HaltScope`, `Admission`, `CardCache`; the SDK forwards neither
  `testing` nor `conformance`); E03 §2 (`agent-team`, `incident-response` and `multi-lang-team` call
  `send_message`, so cancelling the coordinator reaches no child; no example uses `child_lineage`,
  `with_halt_store`, `with_admission`, `with_balancing` or `CardCache`); E04 §6 items 5 and 6.
- **Gates.** none. **Depends.** none. **Release.** `[0.16.x]` for the prelude and features (additive).
- **Read first.** `crates/a2a-protocol-sdk/src/lib.rs` (the prelude), `crates/a2a-protocol-sdk/Cargo.toml`,
  `examples/swarm/src/supervisor.rs` (the one example that uses `Delegation`),
  `book/src/client/delegation.md`.
- **Do.**
  1. Add a `swarm` module to the SDK facade that re-exports the existing types by path (a module, so a glob
     import of the prelude cannot shadow an adopter's own `Join` or `Outcome`); add to the prelude only
     `Delegation`, `DelegationOptions` and `FailureClass`.
  2. Forward `testing` and `conformance` through the SDK (E04 §6 item 6, with its acceptance check).
  3. In each of the three examples, replace the bare `send_message` delegation with `Delegation`, passing
     the turn's cancellation token. First add, to each example's own checks, an assertion that cancelling
     the coordinator's task cancels its children; it must fail before the change.
  4. Add steering's schema and vectors under `crates/a2a-protocol-types/extensions/` (E03 §7 D7: ADR 0049
     requires them for every wire extension and steering has none).
- **Touch.** The SDK crate, the three examples, `crates/a2a-protocol-types/extensions/`,
  `book/src/reference/swarm-extensions.md`, `README.md` and `crates/a2a-protocol-sdk/README.md` feature
  tables, `CHANGELOG.md`.
- **Accept.** `python3 scripts/check_feature_tables.py` and `python3 scripts/check_readme_doctests.py` exit
  0. A doctest in the SDK crate names `a2a_protocol_sdk::swarm::Delegation`. Each example's new cancel
  assertion fails before and passes after. `cargo tree -p a2a-protocol-sdk --features testing,conformance
  -e normal | wc -l` equals the count with default features. `cargo semver-checks` reports no break.
- **Out of scope.** Any new behaviour. The example idiom ratchet (DVX-04).

## SWM-02: Layer A, a delegate scoped to the turn

- **Goal.** Inside an executor, one call starts a child that carries the turn's lineage, a grant from its
  budget and its cancellation, and returns the child's result with its artifacts.
- **Evidence.** E03 §6.2 Layer A; §2.7 (lineage and budget are carried by hand in two of five examples,
  lineage in none; cancel wiring is needed in five and present in two); headline finding 5 (`Delegated` has
  a task id and an outcome and no artifacts, so every caller makes a second `get_task`).
- **Gates.** DEC-01, DEC-09. **Depends.** COR-03, COR-04, COR-05, SWM-01. **Release.** `[0.17]`, additive.
- **Read first.** `S/request_context.rs` (`cancellation_token`, `child_lineage`, `budget`),
  `C/delegation/mod.rs`, `C/delegation/options.rs`, `C/delegation/outcome.rs`, ADR 0023 (why
  `BudgetHandle` lives in the types crate), ADR 0030 ("Why the SQL lives in the server crate": the server
  does not depend on the client), ADR 0034.
- **Do.**
  1. **Write ADR 0058 first**: the scope DEC-01 decided, what "not a runtime" means for A, B and C, and
     where layer A lives. The placement problem is real: the client crate cannot see `RequestContext` and
     the server crate does not depend on the client. Two candidates are recorded in E03 §6.2: a small trait
     in the types crate that `RequestContext` implements (as `BudgetHandle` already does), or the SDK crate,
     which is the only one that sees both. Recommend the trait: it keeps the SDK a facade and lets a
     non-server caller implement it. Record the rejected one with its reason.
  2. Extend `Delegated` with the child's final task. The struct is `#[non_exhaustive]`
     (`C/delegation/outcome.rs`), so a new public field is additive; confirm with `cargo semver-checks`.
  3. The delegate: attach `ctx.child_lineage()` always; grant from `ctx.budget()` when there is one; pass
     the turn's token as the cancel; take an optional shutdown future for hand-off. One method, taking a
     client and `MessageSendParams`, plus `_with(DelegationOptions)`.
  4. Deprecate nothing yet; SWM-04 retires `start_leased` and `start_budgeted` once the option type is the
     one way.
- **Touch.** `docs/adr/` (0058, new; and the ADR indexes), `T/` (the trait), `S/request_context.rs`,
  `C/delegation/` (a new file for the delegate; keep each file under 500 lines), the SDK `swarm` module,
  `examples/swarm/src/worker.rs` or wherever a mid-tree agent delegates, `book/src/client/delegation.md`,
  `CHANGELOG.md`.
- **Accept.**
  - A test over three hops: the grandchild's task carries `root` equal to the first task and `depth` 2,
    with no lineage code in the executors.
  - A test: cancelling the root's task ends the grandchild within the bound the existing cascade test uses
    (`crates/a2a-protocol-client/tests/delegation_tests.rs`), again with no cancel code in the executors.
  - A test: the grandchild's deadline is inside the parent's (as
    `crates/a2a-protocol-client/tests/delegation_budget_tests.rs` asserts by hand today).
  - The result carries the child's artifacts with no second request (assert the peer saw one `GetTask`
    fewer than the hand-written form, or none).
  - `cargo semver-checks` reports no break. No file crosses 500 lines.
- **Out of scope.** Groups. Automatic token exchange (SWM-12).

## SWM-03: A fleet profile that checks the configuration agrees

- **Goal.** A handler built for a fleet refuses to start when its swarm controls cannot work together, and
  says what is missing.
- **Evidence.** E03 §5 ("no type that checks they agree": a lease granted to a server built without a lease
  store is silently ignored, `S/handler/lease.rs`); E04 §6 item 4 (the profile system is the SDK's answer to
  "is this configuration fit", and no example runs it).
- **Gates.** none. **Depends.** COR-09 (so SQLite fleets can satisfy it). **Release.** `[0.17]`, additive.
- **Read first.** ADR 0025 and its amendments, `S/profile.rs`, `S/profile_report.rs`,
  `S/profile_shared_tests.rs`, `S/builder/regulated.rs` (how `RegulatedData` adds declarations).
- **Do.** Reuse the profile mechanism; add no second validator. Either extend `DurableProduction` or add one
  profile (name it by the naming rule; the ADR amendment records which and why). It checks, from what the
  builder already holds: delegation controls declared on the card have their stores (lease, halt); the
  stores are shared when the task store hands out claims (ADR 0025 amendment F4 already refuses a
  process-local one); admission is set when a load signal is reported; a replica name is set. Each refusal
  names the builder call that fixes it. `profile_report()` lists the swarm controls and their stores.
- **Touch.** `S/profile.rs`, `S/profile_report.rs`, tests, ADR 0025 (amendment),
  `book/src/deployment/production.md`, `CHANGELOG.md`.
- **Accept.** One test per refusal, each asserting the message names the missing call. A fully configured
  handler on SQLite and one on PostgreSQL build. `examples/deploy-agent` prints the report (DVX-03 wires
  it).
- **Out of scope.** Checking the client side (a profile is the handler's).

## SWM-04: Layer B, `DelegationGroup` as ADR 0033 specifies

- **Goal.** A supervisor fans out to N children under one cap, one join rule, one budget split and one
  cancel, with results in input order, without writing a semaphore and a join set.
- **Evidence.** ADR 0033 (the design; "Proposed. Nothing here is built"); E03 §2.1 (about 276 of 750 lines
  of `examples/swarm` are plumbing; `supervisor.rs` lines 46 to 150 and 171 to 203 are what the group
  replaces); §2.7 (fan-out with a cap and ordered fan-in in four of five examples); E04 §6 item 2 (an API
  sketch consistent with ADR 0033 and the naming rule).
- **Gates.** DEC-01, DEC-13. **Depends.** SWM-02, COR-04. **Release.** `[0.17]`, additive.
- **Read first.** ADR 0033 whole, including "What stays out" and "Open"; `C/delegation/steer.rs`
  (`steer_all`, the existing fan-out with per-child results in order); `T/budget.rs` and `T/budget/`
  (`BudgetHandle::grant`, `ChildGrant`); `examples/swarm/src/supervisor.rs`.
- **Do.**
  1. Accept ADR 0033 (status line, dated), with three additions E03 §6.2 found: a member's target may be a
     balanced client, so "try another replica" is the existing balancer and not group logic; a member's
     result carries artifacts; the cancel cause (COR-04) is what lets `FailFast` and the retry helper tell
     a lapse from a halt.
  2. Implement in `C/delegation/group.rs` (split when it nears 500 lines). The cap is required, with no
     default. `Join` is a `#[non_exhaustive]` enum (`All`, `FirstOk`, `Quorum`, `FailFast`); state in the
     ADR what `Quorum(k)` reports once k successes are unreachable (recommend: end at once, cancel the
     rest, report `Unreachable` with what settled). The budget share is taken at member start through
     `BudgetHandle::grant`; do not add a second budget mechanism. Lease renewals are staggered.
  3. A helper that yields the indices of members the caller may retry, from `FailureClass` and the cancel
     cause. The group itself does not retry (DEC-13).
  4. Dropping the future that `run` returns cancels every started member; state the cancellation safety in
     the doc comment.
  5. Deprecate `Delegation::start_leased` and `start_budgeted` in favour of `start_with` (E04 §6 item 9b),
     by the deprecation rule in `CONTRIBUTING.md`.
- **Touch.** `C/delegation/group.rs` (new) and its tests, `C/delegation/mod.rs`, the SDK `swarm` module,
  ADR 0033, `examples/swarm/src/supervisor.rs`, `book/src/client/delegation.md`, `CHANGELOG.md`.
- **Accept.**
  - Against `testing::ScriptedPeer` and loopback agents: in-flight never exceeds the cap (count at the
    peer); `FirstOk` cancels and awaits the rest; `Quorum(k)` ends as the ADR states when k is unreachable;
    an equal split over N never reserves more than the parent had (a property test over N and the grant);
    results are in input order; dropping `run` cancels every started member.
  - `examples/swarm` keeps its exit-code contract on `fanout`, `faults` and `cancel` (run each; paste the
    summary lines), and `wc -l examples/swarm/src/supervisor.rs` is lower than before by at least the lines
    E03 §2.1 attributes to fan-out plumbing (state both counts).
  - The incremental mutation gate has no survivor in `group.rs`.
- **Out of scope.** Durable groups (a `group` column or record changes the journal contract; ADR 0033 defers
  it). Retry inside the group. `SlimRpcMulticast` as a member transport (E03 §6.4 Q10; record the question
  in the ADR, build nothing).

## SWM-05: Layer C, a recovery driver

- **Goal.** A supervisor that restarts reattaches every child it had, renews their leases at once, and
  loses none because one record was bad.
- **Evidence.** E03 §6.2 Layer C; §2.2 (`scripts/live/delegation/src/bin/supervisor.rs` is the only code
  composing journal, lease, reattach and hand-off; about half of its 372 lines is plumbing); ADR 0030's
  amendment of 2026-11-04 (the live proof found three defects in that loop: two in the loop the book tells
  adopters to write, one in the SDK).
- **Gates.** DEC-01. **Depends.** COR-06, COR-07, SWM-02. **Release.** `[0.17]`, additive.
- **Read first.** ADR 0030 whole; `book/src/client/delegation.md` (the recovery loop it documents);
  `scripts/live/delegation/README.md` and `src/bin/supervisor.rs`; `C/delegation/resume.rs`,
  `C/delegation/journal.rs`.
- **Do.**
  1. Amend ADR 0030: the "not built: a recovery driver" decision is reversed, and why (the defects).
  2. The driver does exactly what the book's loop says and nothing more: claim the owner's open records
     (COR-07); reattach every one before waiting on any; renew leases at once; never drop held handles
     because one reattach failed; report records with no task id (recoverable by key since COR-06, else
     reported). It returns the handles, or a group built from them; it decides nothing about what to do
     with a child.
  3. The journal record needs the lease's time to live to renew at once (E03 §6.2: the URL is there, the
     ttl is not). Add it with the COR-07 migration if that has not shipped, else a second migration.
  4. Rewrite the live supervisor on the driver.
- **Touch.** `C/delegation/` (a new file), `T/delegation_journal.rs`, both SQL journals if the record
  grows, ADR 0030, `scripts/live/delegation/src/bin/supervisor.rs`, `book/src/client/delegation.md`,
  `CHANGELOG.md`.
- **Accept.**
  - A test: a supervisor with K journaled, leased children is dropped without cancelling (the detach path),
    a new one runs the driver, and all K complete; no child's lease lapsed in between (assert at the peer).
  - A test: one record is unreadable; the other K minus 1 are still reattached and the bad one is reported.
  - The live proof (`scripts/live/delegation/README.md`, arms `a,c,c2,cx,al`) passes three runs with
    `supervisor.rs` rewritten; state its line count before and after.
- **Out of scope.** Restart strategies. Deciding whether a recovered child should be retried.

## SWM-06: A width limit on fan-out

- **Goal.** Depth 16 with unbounded width bounds nothing; a job has a bound on children per task that the
  server enforces.
- **Evidence.** E03 §3 G21 (no `max_children`, `max_fan_out` or similar exists) and G20 (a loop between
  agents is bounded only by depth, and only if every hop carries lineage); E03 §6.4 Q11.
- **Gates.** DEC-14. **Depends.** SWM-02. **Release.** `[0.17]`.
- **Read first.** ADR 0034 (lineage carries `root`, `depth`, `parentTaskId`; decision 4, lineage is a
  caller's claim and is never an authorization), `T/lineage.rs` (`MAX_LINEAGE_DEPTH`),
  `S/handler/lineage.rs`.
- **Do.** A server-side bound on tasks admitted per `(tenant, parentTaskId)`, off by default, set in
  `HandlerLimits`, refused before the executor runs with a failure class a parent can read. It needs a
  count by parent: say in the pull request what it costs each store (an index on the lineage column, or a
  counter) and measure the statement count. A client-side cap already exists in the group (SWM-04); this is
  the bound that holds when the parent is not this SDK.
- **Touch.** `S/handler/limits/mod.rs`, `S/handler/lineage.rs`, the stores, tests, ADR 0034 (amendment),
  `scripts/inert_bounds_allowlist.txt` if a sibling does not read the new bound (gate 45 of
  `static-checks`), `CHANGELOG.md`.
- **Accept.** A test: with the limit at N, child N plus 1 under one parent is refused with the documented
  class before its executor runs; children of another parent are not. `python3
  scripts/check_inert_bounds.py` exits 0. `tests/statement_count.rs` constants are unchanged with the limit
  off.
- **Out of scope.** An agent path in lineage. A per-root total (needs a stored root index ADR 0034 does not
  have; record it).

## SWM-07: Forward a child's progress into the parent's stream

- **Goal.** A parent can relay a child's status and artifact events to its own caller without hand-writing
  the forwarding and id rewriting.
- **Evidence.** E03 §3 G35; `docs/adopter-audit-2026-09-22.md` finding S5 ("110 of the 230 lines").
- **Gates.** DEC-01. **Depends.** SWM-02. **Release.** `[0.17]`, additive.
- **Read first.** `C/delegation/mod.rs` (`next_event`), `S/executor_helpers.rs` and `S/executor_helpers/`
  (the emit helpers an executor writes through), the adopter audit's S5 for the exact glue.
- **Do.** One helper that takes a child's event and writes it to the parent's queue with the parent's task
  and context ids, artifact ids namespaced by child so two children cannot collide, and the child's
  terminal state **not** forwarded as the parent's (the parent decides its own ending). No merging policy,
  no reducer: the caller still drives `next_event`. Like layer A it sees both sides, so it lives where
  ADR 0058 put layer A.
- **Touch.** Where ADR 0058 says; `book/src/client/delegation.md`; one example.
- **Accept.** A test with two children emitting the same artifact id: the parent's stream holds both,
  distinguishable, in arrival order, and no terminal event until the parent emits its own. The example that
  hand-wrote the forwarding loses those lines (state the count).
- **Out of scope.** Merging fan-out streams by policy; summarising progress.

## SWM-08: An opt-in halt route

- **Goal.** An operator can halt and resume a root over HTTP without every adopter writing the route.
- **Evidence.** E03 §5 ("the operator controls are methods on `RequestHandler` with no route"), §6.4 Q8;
  ADR 0049 (why halt is not on the wire: a wire halt is something any caller could send).
- **Gates.** DEC-10. **Depends.** none. **Release.** `[0.17]`, additive, off by default.
- **Read first.** ADR 0019, ADR 0024, ADR 0049, `S/handler/halt.rs` (`halt`, `resume`, `HaltScope`),
  `S/dispatch/rest/mod.rs` (how `/health`, `/ready` and `/load` are served), ADR 0025.
- **Do.** A route enabled by one builder call, served outside the A2A method space, that requires an
  authenticated caller and a caller-supplied authorization check (a closure over `CallContext`), and
  refuses to be enabled under `Profile::Production` without an authenticating interceptor. Every halt and
  resume is an audit record (it already is for the method; confirm and name the test).
- **Touch.** `S/dispatch/rest/`, `S/builder.rs` or a file under `S/builder/`, `S/profile.rs`, tests,
  ADR 0049 (amendment), `book/src/deployment/oversight.md`, `tools/a2a-cli` (an `a2a halt` command is a
  follow-up, not this package).
- **Accept.** Tests: unauthenticated is 401; authenticated and not authorized is 403; authorized halts the
  root and a second replica on the same halt store stops its turn; the route answers 404 when not enabled;
  `Profile::Production` without an authenticator refuses to build with it on.
- **Out of scope.** A halt that crosses agents with no shared store (it travels as cancels, hop by hop;
  ADR 0049 "What is not built").

## SWM-09: One book part, and the swarm example on the new layers

- **Goal.** A reader goes from one agent to a supervised, recoverable fleet in one ordered part, and the
  example is the proof.
- **Evidence.** E03 §5 (the material is in seven places; "there is no 'running a swarm' chapter that orders
  them"); E04 §2 persona P3; E02 §4 ("a self-proving cluster sample").
- **Gates.** none. **Depends.** SWM-02, SWM-03, SWM-04, SWM-05. **Release.** documentation.
- **Read first.** `book/src/SUMMARY.md`; the seven pages E03 §5 lists; `scripts/check_book_examples.py`,
  `scripts/check_book_code.sh` (the ignore ratchet), `book-tests/`.
- **Do.** A part "Running a swarm" that orders: one child (layer A), many (the group), surviving a restart
  (the driver), stopping it (halt, cancel causes), bounding it (budget, lease, width), seeing it (OBS-05),
  configuring a fleet (SWM-03), and "what this SDK does not do" (README §7, so the claim is honest on the
  page). Each page's program is compiled by `book-tests`. The supervisor page's program is a 50-worker
  supervisor; give its line count. Move pages, do not duplicate them.
- **Touch.** `book/src/`, `book/src/SUMMARY.md`, `book-tests/`, `examples/swarm/`, `examples/README.md`,
  `docs/swarm-orchestration.md` (a dated note that the part supersedes its "Next, in order").
- **Accept.** `mdbook build book` and `python3 scripts/gen_sitemap.py --check` exit 0; `cargo test -p
  book-tests` passes; `./scripts/check_book_code.sh` shows no new ignored block; `cargo run -p swarm`
  (with the arguments its README gives) exits 0 on every mode.
- **Out of scope.** New features.

## SWM-10: The swarm, measured past what has been measured

- **Goal.** Numbers for width, depth, hosts and a supervisor kill, so the next design argument starts from
  a measurement.
- **Evidence.** E03 §3 G12 and §6.5 (nothing above 256 workers, deeper than two hops, or across hosts was
  measured; a supervisor kill run "is not done"); `docs/swarm-orchestration.md` *Next, in order* item 3;
  target sheet S-4, S-5, F-6.
- **Gates.** DEC-03. **Depends.** SWM-04, SWM-05, OBS-03. **Release.** a measurement and a document.
- **Read first.** `examples/swarm/README.md`, `crates/a2a-protocol-server/tests/swarm_scale/`,
  `scripts/live/delegation/README.md`, `docs/swarm-scale-findings.md` (how a finding is written here).
- **Do.** With the harnesses that exist, extended only as needed: width 256, 1,024 and 4,096 children under
  one supervisor; depth 2, 4 and 8; cancel and halt propagation time from root to last leaf at each;
  `kill -9` of the supervisor mid-run and recovery by the driver; leased children at 10^4 with renewals per
  second counted (target S-4). State for each run the host, the store, and whether the container could
  measure it; a count or a bound is a result, a tail latency from the container is not.
- **Touch.** `examples/swarm/`, `tests/swarm_scale/`, a new dated findings document under `docs/`,
  `docs/readiness-bar.md` (`measured-limits`), `docs/program/target-sheet.md`.
- **Accept.** Each row has its command, three runs and the range. Zero orphaned children after the kill
  run, by a query the document shows. The findings document says what would falsify its conclusions.
- **Out of scope.** Building anything the measurements suggest. That is the next plan.

## SWM-11: Endpoint selection that reads the server's load signal

- **Goal.** A balanced client prefers the replica that says it has room.
- **Evidence.** E03 §3 G9 and §7 D4 (the server emits `endpoint-load-metrics`, `S/handler/load.rs`; the
  client selects by its own in-flight count and does not read the header; ADR 0050's stated reason, "no
  server sends it yet", is no longer true).
- **Gates.** none. **Depends.** none. **Release.** `[0.17]`, additive, opt-in.
- **Read first.** ADR 0046, ADR 0050 (decision 2 and "Not built"), `C/balance.rs` and `C/balance/`,
  `C/pool/`.
- **Do.** A `BalancePolicy` option that weights by the last reported utilization per endpoint, with a stated
  staleness bound after which the endpoint falls back to the in-flight count. A client reading a header is
  not a scheduler: nothing here assigns work to a named agent.
- **Touch.** `C/balance/`, tests, ADR 0050 (amendment correcting D4), `book/src/client/load-balancers.md`.
- **Accept.** A test with two loopback replicas, one reporting saturated: over N sends, the share to the
  saturated one is below a stated bound; with the signal stale it returns to even. No change with the
  option off (the existing balancing tests pass unmodified).
- **Out of scope.** ORCA beyond the header ADR 0046 emits; selection by `AgentSkill`.

## SWM-12: A delegation carries the caller's principal

- **Goal.** The token exchange that already exists is one option on a delegation, not a client per
  principal the adopter builds and caches.
- **Evidence.** E03 §3 G29 ("nothing connects it to `Delegation`: one `Arc<A2aClient>` per handle"); ADR
  0021; E02 §3 (RFC 8693 actor chains are unique to this SDK among the A2A SDKs).
- **Gates.** none. **Depends.** SWM-02, COR-11. **Release.** `[0.17]`, additive.
- **Read first.** ADR 0021, `C/token_provider.rs` and `C/token_provider/`, `C/interceptor.rs`,
  `S/call_context.rs` (`actors`).
- **Do.** A per-call credential on `DelegationOptions` (a token provider used for this delegation only), so
  the turn-scoped delegate can exchange the inbound caller's token for a narrower one for the child.
  Nothing is exchanged unless a provider is configured.
- **Touch.** `C/delegation/options.rs`, `C/token_provider/`, the turn-scoped delegate, tests,
  `book/src/client/delegation.md`.
- **Accept.** A two-hop test: the child's server reads an `act` chain naming the parent, and the token's
  audience is the child. With no provider the request is byte-identical to today's (a golden test).
- **Out of scope.** A token cache policy beyond what the provider already has.
