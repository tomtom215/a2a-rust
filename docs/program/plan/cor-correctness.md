<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# COR: correctness of the fleet and delegation paths

Read [`../README.md`](../README.md) §6 first. Every package here is a defect. The rule: **the failing test
comes first, and its failing output goes in the commit message.** No package here adds public surface
unless its section says so.

Abbreviations: `S/` is `crates/a2a-protocol-server/src/`, `C/` is `crates/a2a-protocol-client/src/`,
`T/` is `crates/a2a-protocol-types/src/`.

---

## COR-01: A store outage must not lose a task's ending (X-10)

- **Goal.** After a store outage of any length, a task that ended is terminal in the store, or its loss is
  counted and the task is failed; it is never left running with nothing to finish it.
- **Evidence.** ROADMAP *Fleet and failure correctness* item 1 and *Antipatterns* 7; `docs/handoff.md` *Now*
  ("Register rows re-opened": X-10) and *Open engineering* (the candidates: a breaker, and a direct store
  write of the failure); E05 §5 S-C.
- **Gates.** none. **Depends.** none. **Release.** `[0.16.x]`; removes a Known Limitation.
- **Read first.** `S/handler/event_processing/background/state_machine.rs` (the status update is reverted
  in memory and logged after `persisting` gives up), `S/handler/limits/mod.rs` (`store_write_retry`),
  `crates/a2a-protocol-server/tests/transient_store_errors` (the CI-sized outage cases),
  `scripts/live/README.md`.
- **Do.**
  1. Reproduce in `tests/transient_store_errors`: a store that refuses writes for longer than
     `store_write_retry` (use a short retry in the test), an executor that completes meanwhile, then the
     store returns. Assert the task is terminal. It must fail today.
  2. Design, in the pull-request description, against two cases the handoff names: (a) every event behind
     the lost one spends the whole retry again, so a queue of N drains in N times the retry; (b) the
     `Failed` status itself can be dropped. The candidates recorded are a breaker that stops retrying once
     one write has used the budget, and a durable write of the ending that outlives the retry (a pending
     terminal state retried until the store answers or the process stops). Choose one; say why not the
     other.
  3. A terminal status must never be reverted in memory and forgotten. Either it is written, or the turn
     stays owned and retrying, or the task is recorded as failed with the loss counted on
     `Metrics::on_persistence_error`.
  4. State cancellation safety for whatever holds a lock or a partial write across an await.
- **Touch.** `S/handler/event_processing/background/` (the state machine and its neighbours),
  `S/handler/limits/mod.rs` if a bound is added, the tests, `CHANGELOG.md` (*Fixed*, and remove the Known
  Limitation), `docs/audits/0.16.0-release/findings.json` (X-10).
- **Accept.** The new test fails before and passes after (both outputs in the commit message). A second
  test: N queued events during an outage drain in a time bounded by one retry budget plus N writes, not N
  budgets (assert on a paused clock, not wall time). `scripts/live/restart_live.py` (the recipe is in
  `scripts/live/README.md`) reports no unfinished task after a PostgreSQL restart longer than the retry;
  paste its summary. The incremental mutation gate on the diff has no survivor.
- **Out of scope.** A durable audit spool (X-19). Changing `store_write_retry`'s default.
- **Done (2026-11-26).** Built as a pending terminal state written again until the store rules on it,
  with the breaker beside it (`S/handler/event_processing/background/processor/pending.rs`); only an
  ending whose write gave up on an unavailable store is kept, since one the store refused has been ruled
  on (found when the first version hung two `terminal_truth` tests). Found while testing it and fixed:
  a new artifact was the one fold write not retried. Accept: the tests in `S/handler/event_processing/background/outage_tests.rs`
  fail with the fix taken out and pass with it (outputs in the commit message); the mutation gate on
  the diff, 61 mutants in three shards, left none missed or timed out. Correction to this Accept,
  measured: `scripts/live/restart_live.py`'s 4 s outage, and a 40 s one, never reach X-10. sqlx waits in `acquire`
  (30 s) through the short one, and the executor itself failed in the long one, so the run passes before
  the fix as after (both builds run, a local variant stopping PostgreSQL with `pg_ctlcluster`, since
  the script needs a Docker daemon). The unit tests are the reproduction.

## COR-02: Cancelling a parked task announces it (E-F25, X-11)

- **Goal.** `CancelTask` of a task with no live queue writes an event, so a subscriber on another replica
  sees the cancel at once instead of at its next poll.
- **Evidence.** ROADMAP *Fleet* item 2; `docs/handoff.md` *Now* (E-F25: `save_status_delta` and no event,
  in `S/handler/lifecycle/cancel_task.rs`); ADR 0057 names the step; E05 §5 S-G.
- **Gates.** none. **Depends.** none. **Release.** `[0.16.x]`.
- **Read first.** `S/handler/lifecycle/cancel_task.rs`, ADR 0057,
  `crates/a2a-protocol-server/tests/log_wake_e2e.rs`.
- **Do.** Reproduce in `crates/a2a-protocol-server/tests/log_wake_e2e.rs`: two handlers on one store, a parked task, a subscriber on the
  second, a cancel on the first; assert the subscriber's stream ends with `canceled` within one second with
  the log wake on. Then append the status event to the log in the same write as the status.
- **Touch.** `S/handler/lifecycle/cancel_task.rs`, the test, ADR 0057 (the sentence ROADMAP *The book's
  structure* item 2 says the code has outgrown), `CHANGELOG.md`.
- **Accept.** The test fails before, passes after. `tests/statement_count.rs` constants are unchanged, or
  the change is stated with the new count. Audit records for the cancel are unchanged (name the test that
  says so).
- **Out of scope.** Parked-task expiry (ADR 0056).
- **Done (2026-11-26).** The cancel announces the ending through the claim reaper's sequence
  (`S/handler/event_processing/ending.rs`) when no turn runs the task: no live claim, or, without a
  claim store, the task interrupted when the cancel came. Accept: `crates/a2a-protocol-server/tests/log_wake_e2e.rs`
  `a_cancel_of_a_parked_task_ends_a_long_wait_on_the_other_replica` fails without the change and ends
  the stream 253 ms after the cancel with it (8 s ceiling, PostgreSQL 16); `statement_count` passes
  with its constants unchanged (2 of 2, run directly); the audit record of a cancel is unchanged
  (`tests/audit_trail.rs` `a_cancel_request_names_who_asked`, passed in the all-features run).

## COR-03: A handed-off child does not get its budget again (E03 R1)

- **Goal.** After a hand-off continuation, the child's deadline and meters are what the parent has left, not
  what the parent first granted.
- **Evidence.** E03 §4 R1. `C/delegation/continuation.rs` `continuation()` clones the first message, which
  carries the budget extension with `remainingMs` computed at grant time; `S/handler/budget.rs` starts a new
  account from the message at receipt. A child can outlive its parent's deadline by up to three times the
  grant.
- **Gates.** none. **Depends.** none. **Release.** `[0.16.x]`.
- **Read first.** ADR 0023 (*Scope*: "a continuing message brings its own budget"), ADR 0054,
  `C/delegation/options.rs`, `C/delegation/continuation.rs`,
  `crates/a2a-protocol-client/tests/delegation_budget_tests.rs`.
- **Do.**
  1. Reproduce in `crates/a2a-protocol-client/tests/delegation_budget_tests.rs`: a budgeted delegation whose child hands off once; assert
     the continuation's `remainingMs` is no greater than the grant minus the time already spent, and that
     the meters the first turn reported are charged.
  2. Recompute the budget on the continuation from the live `ChildGrant` (what was reserved, less what the
     handed-off turn reported and the time elapsed). If the handed-off turn reported no spend, charge the
     elapsed time and say in the doc comment that meters cannot be known.
- **Touch.** `C/delegation/continuation.rs`, `C/delegation/mod.rs` (where the grant is settled), tests,
  ADR 0023 (a dated amendment), `CHANGELOG.md` (*Fixed*).
- **Accept.** The test fails before, passes after. A property the test states in words: over k hand-offs,
  the sum of deadlines given to the child never exceeds the parent's grant.
- **Out of scope.** Budgets across replicas of the parent.
- **Done (2026-11-26).** `C/delegation/child_budget.rs` keeps the live grant and what each handed-back
  turn reported; a continuation carries the grant's time less the time since it was made and each meter
  less the reported spend, no continuation is sent once that time is gone, and the settlement charges the
  sum over the turns (it charged the last turn's alone, refunding the rest). A meter a turn did not report
  is carried as granted and keeps its whole reservation. The tests are in the unit harness
  (`C/delegation/continuation_tests.rs`, `C/delegation/child_budget_tests.rs`), not `tests/delegation_budget_tests.rs`
  as *Do* said: its scripted transport and paused clock drive a hand-off, which the integration file's
  peers do not. Accept: `a_continuation_carries_what_is_left_and_every_turn_is_charged` fails before the
  change (the continuation carried the whole grant, 56 999 ms, where 55 999 ms were left) and passes
  after; the property is `no_continuation_reaches_past_the_grant` (three hand-offs, each continuation's
  time plus the time gone equals the grant) with `no_continuation_is_sent_after_the_grant_runs_out`
  (3 sends where 2 are due, without the check). Mutation gate on the client diff: 16 mutants, 12 caught,
  4 unviable, none missed. The same change fixed register X2-1 (a retried continuation's identity).

## COR-04: `Canceled` says who stopped the child (E03 R3)

- **Goal.** A parent can tell a lapsed lease from an operator's halt from a third party's `CancelTask`.
- **Evidence.** E03 §4 R3 (`S/handler/lease/watch.rs` stops the turn with the call a halt uses;
  `S/handler/messaging/terminal.rs` writes `Canceled` for all three; `C/delegation/outcome.rs` maps the
  state to `Outcome::Canceled`, "whoever asked for it"). SWM-04's `FailFast` and retry helper need it.
- **Gates.** none. **Depends.** none. **Release.** `[0.17]` (a new metadata key on the wire and a new public
  enum; additive).
- **Read first.** `T/failure.rs` and `T/failure/` (how `FailureClass` rides the failed status as metadata
  under an extension URI), ADR 0049 (schemas and vectors for every wire extension), ADR 0034.
- **Do.**
  1. Reproduce: three tests, one per cause, each asserting a machine-readable cause on the final status.
  2. Carry the cause in the style of `FailureClass`: a `#[non_exhaustive]` enum in the types crate (lease
     lapsed, halted, cancel requested, executor timeout if it ends `Canceled`), written by `finish_run`
     from what set `turn.halted`. Reuse the failure-class extension's metadata if its schema allows a
     canceled status; a new URI only if it does not, with schema and vectors under
     `crates/a2a-protocol-types/extensions/`.
  3. `Outcome::Canceled` exposes it. An absent cause (a peer in another SDK) is `None`, never guessed.
- **Touch.** `T/failure.rs`, `S/handler/lease/watch.rs`, `S/handler/halt.rs`,
  `S/handler/messaging/terminal.rs`, `C/delegation/outcome.rs`,
  `book/src/reference/swarm-extensions.md`, golden vectors, `CHANGELOG.md`.
- **Accept.** The three tests fail before, pass after. The golden-vector test for the extension passes.
  `cargo semver-checks` reports no break. A peer that sends `canceled` with no cause yields `None` (a test).
- **Out of scope.** A cause on `Failed` (it has `FailureClass`).
- **Done (2026-11-26).** A new extension, cancel/v1 (`T/cancel_cause.rs`, schema and vectors in
  `crates/a2a-protocol-types/extensions/cancel-v1/`): the failure extension's schema could not carry it,
  since its carrier is the failed status and a reader takes an unknown token as `internal`. The server
  stamps the cause on every `Canceled` its writer passes on (`S/handler/cancel_cause.rs`), from marks on
  the turn: the lease watch marks a lapse before it stops the turn as a halt does, and `CancelTask` marks
  the running turn and its own writer's. The executor timeout ends `Failed`, class `budget-exhausted`
  (`S/handler/messaging/execute.rs`), so it has no cancel cause. The URI is not declared on the agent card:
  adding it would refuse a signed card built for 0.16 (`S/builder.rs` `advertise`), the hand-off marker's
  precedent. Accept: `a_cancel_task_says_a_caller_asked` (running and parked), `a_halt_says_so_on_the_canceled_status`
  and `a_lapsed_lease_says_so_on_the_canceled_status` fail with the stamp removed (cause `None`), and the
  lease test reads `Halted` with only the lapse mark removed; `extension_vectors` runs the new vectors
  and `scripts/check_extension_vectors.py` passes; `cargo semver-checks` on the four crates: no semver update
  required; `Outcome::cancel_cause` is `None` for a bare `canceled` (`C/delegation/outcome.rs` test).

## COR-05: The client does not retry into a halt (E03 R2)

- **Goal.** A `HALTED` refusal is final for the caller and neutral for endpoint ejection, on every binding.
- **Evidence.** E03 §4 R2. ADR 0049 decision 4 requires of any hop: "stop on a `HALTED` refusal: a child's
  refusal is not a fault to retry". `grep -rn HALTED crates/a2a-protocol-client/src` prints nothing.
  `C/retry.rs` treats a 503 as retryable; `C/origin.rs` counts 502 to 504 toward ejection.
- **Gates.** none. **Depends.** none. **Release.** `[0.16.x]` as a specification-of-our-own correction; say
  so in the changelog.
- **Read first.** ADR 0049; `S/handler/halt.rs` (what a halted server answers on each binding: status, code
  and `ErrorInfo` reason); `crates/a2a-protocol-server/tests/http_json_error_parity.rs`; `C/retry.rs`,
  `C/origin.rs`, `C/error/`.
- **Do.**
  1. First trace, per binding (JSON-RPC, HTTP+JSON, gRPC, WebSocket), which `ClientError` a halt produces
     today. E03 did not trace this; the table goes in the pull request.
  2. Reproduce per binding: a server halted for the caller's root; a client with a retry policy and a
     pooled, ejecting transport. Assert one attempt, a distinguishable error, and no ejection.
  3. Recognise the halt by its machine-readable reason, not by status code alone (a 503 from a broken
     replica must stay retryable).
- **Touch.** `C/retry.rs`, `C/origin.rs`, `C/error/`, `C/delegation/outcome.rs` if a halt surfaces there,
  tests under `crates/a2a-protocol-client/tests/`, `CHANGELOG.md`.
- **Accept.** The per-binding tests fail before, pass after. A 503 with `Retry-After` and no halt reason
  is still retried (a test); a bare 503 is not, for a non-idempotent call (decision D-1, 2026-11-26: it
  read "a plain 503 with no halt reason is still retried"). The incremental mutation gate has no survivor
  in `C/retry.rs` and `C/origin.rs`.
- **Out of scope.** Middleware for other SDKs (ECO-04).
- **Done (2026-11-26).** The trace, per binding, of what a halt produced before the change (read, then
  confirmed by the test): JSON-RPC and WebSocket, `Protocol(InternalError)` with the `HALTED` detail in
  `data`, not retried; HTTP+JSON, `UnexpectedStatus { 503 }` (the reader found no A2A code in the
  standard reason and dropped the body), retryable; gRPC, `HttpClient("gRPC unavailable: …")`,
  retryable. Now all four give `Protocol` with the detail (`T/halt.rs`, `C/transport/grpc.rs`,
  `C/transport/rest/request.rs`), read by `ClientError::is_halted`; a protocol error is not retried and
  counts as an answer for ejection (`C/origin.rs`, test `a_halt_is_an_answer_not_a_failure`). D-1 is in
  `C/retry.rs` `safe_to_retry_non_idempotent`. Accept: `crates/a2a-protocol-sdk/tests/binding_equivalence/halt.rs`
  fails before the change on HTTP+JSON (`UnexpectedStatus { status: 503, … }`) and, with only the gRPC
  fix taken out, on gRPC (`HttpClient("gRPC unavailable: halted: stop everything")`); it passes after on
  all four, one attempt each. `non_idempotent_retried_on_503_only_with_retry_after` and
  `only_429_and_503_with_retry_after_are_safe_for_non_idempotent` replace the two tests that asserted
  the old rule.

## COR-06: An ambiguous delegation start is recoverable (E03 R5)

- **Goal.** A delegation whose first send failed without an answer can be sent again without starting a
  second child.
- **Evidence.** E03 §4 R5. `C/builder/mod.rs` sets `peer_honours_idempotency: false` in `ClientBuilder::new`
  and true only from a card or by `with_peer_honouring_idempotency`. Every delegating example builds with
  `ClientBuilder::new`. The journal then holds an open record with no task id and no key.
- **Gates.** DEC-08. **Depends.** none. **Release.** `[0.17]` if the default changes (a behaviour change on
  the wire).
- **Read first.** ADR 0026 (why keys are random and when they are sent), `T/idempotency.rs`,
  `C/delegation/options.rs`, `C/delegation/journal.rs`,
  `crates/a2a-protocol-client/tests/delegation_journal/records.rs`.
- **Do.** Per the decision. If "send always": `Delegation::start*` attaches a key whatever the client
  believes of the peer, the journal records it, and the retry rule for a keyed send stays as it is (retried
  only against a peer known to honour the key). Then add the recovery the book describes: resend under the
  record's key.
- **Touch.** `C/delegation/options.rs`, `C/delegation/journal.rs`, the journal tests,
  `book/src/client/delegation.md`, `CHANGELOG.md`.
- **Accept.** A test: the first send's response is dropped by a `ScriptedPeer`; the recovery resends; the
  agent ran one task. It fails before the change.
- **Out of scope.** Deriving a child key from the parent's (E03 §6.4 Q7; needs its own ADR against
  ADR 0026).
- **Done (2026-11-26).** DEC-08 as amended by the maintainer on 2026-11-26: send always, and resend
  once without the key on the typed refusal. The section's *Held* note (DEC-08's reason did not hold for
  this SDK's server, whose `TaskStore` default refuses keys) led to the amendment. `C/delegation/keys.rs`
  keys every start, and takes a key the start added off on `idempotency::refuses_keys`; a caller's key is
  never taken off. The server's refusal carries the typed detail (`S/handler/messaging/idempotency.rs`).
  `DelegationOptions::resending` sends a record's message again under its key, with no unkeyed
  fallback: a refusal then says the agent cannot deduplicate. A record whose start fell back still names
  the key (the journal has no way to change it); a resend under it is refused, which says so. Accept:
  `crates/a2a-protocol-client/tests/delegation_journal/resend.rs` loses the first response at a TCP relay,
  resends from the record, and the agent ran one task; with the start's key taken away (the behaviour
  before, for a client not known to honour keys) it fails at "the record has the key". The
  `delegation::keys` tests fail the same way. `crates/a2a-protocol-client/tests/delegation_journal/records.rs`
  `an_unkeyed_message_to_an_agent_not_known_to_honour_keys_is_recorded_with_a_fresh_one` replaces the test
  that asserted the old rule.

## COR-07: Journal records have an owner that is enforced (E03 R4)

- **Goal.** Two replicas of one supervisor do not both reattach a child, and one dropping its handle does
  not cancel a child the other follows.
- **Evidence.** E03 §4 R4. `T/delegation_journal.rs`: `open(owner)` lists; nothing claims or fences. ADR
  0030: the owner column exists "but does not enforce". ADR 0030, "not run under concurrent replicas".
- **Gates.** none. **Depends.** none. **Release.** `[0.17]` (new defaulted trait methods and a schema
  change in both SQL journals).
- **Read first.** ADR 0030; `S/store/delegation_journal/postgres.rs` and `S/store/delegation_journal/sqlite.rs`; the turn-claim design
  it should copy rather than reinvent (`S/store/claim/`, a time-to-live claim with a fencing token); E07
  §B5 (the two schema paths and the next migration version).
- **Do.**
  1. Reproduce with two journal handles on one PostgreSQL: both `open` the same owner and reattach; one
     drops; assert the child is still running. It must fail today.
  2. Add a claim on a record: who holds it, until when, with a fence that a settle or a cancel must
     present. Defaulted trait methods; `CONTRIBUTING.md` *Extending a public trait* applies in full (doc
     paragraph on the cost of the default, every implementation updated, a gate where one is expressible).
  3. Migration on both databases through the versioned runner; the upgrade test covers a 0.16 journal.
- **Touch.** `T/delegation_journal.rs` and `T/delegation_journal/`, `C/delegation/journal.rs`,
  `C/delegation/resume.rs`, `C/delegation/drop.rs`, both SQL journals, their tests, ADR 0030 (amendment),
  `book/src/client/delegation.md`, `CHANGELOG.md`.
- **Accept.** The two-holder test fails before, passes after, on memory, SQLite and PostgreSQL. A holder
  whose claim lapsed cannot settle (a test). The upgrade test passes from a 0.16.0 schema.
- **Out of scope.** Durable groups (ADR 0033 defers them).

## COR-08: A parked child of a dead parent is stopped by its lease (E03 R8)

- **Goal.** A leased task that is parked (`input-required`) when its lease lapses ends, instead of staying
  parked until an opt-in expiry or for ever.
- **Evidence.** E03 §4 R8 (`S/handler/lease/watch.rs` removes the watch entry when the turn parks and keeps
  the lease; nothing checks the deadline of a task with no running turn; root halts skip parked tasks too).
- **Gates.** none. **Depends.** COR-04 (the cause to write). **Release.** `[0.17]`.
- **Read first.** ADR 0020, ADR 0056 (parked tasks may expire: deleted, not cancelled, and why a
  store-level purge cannot write the event), `S/handler/lease/`, the claim reaper under
  `S/handler/claims/reaper/`.
- **Do.** Reproduce: a leased child parks; the parent stops renewing; assert the child ends within three
  lease lifetimes with the lease-lapsed cause. Then let the component that already sweeps on a timer (the
  reaper, on the replica that can take the claim) end a parked task whose lease lapsed, through the
  handler, so the event, the push and the audit record are written. Do not add a second sweeper.
- **Touch.** `S/handler/lease/`, `S/handler/claims/reaper/`, tests, ADR 0020 (amendment), `CHANGELOG.md`.
- **Accept.** The test fails before, passes after, on the in-memory and PostgreSQL lease stores. The
  statement count of an idle replica per sweep is stated before and after.
- **Out of scope.** Root halts reaching parked tasks (record as a register row with the same shape).

## COR-09: A SQLite lease store

- **Goal.** A one-host fleet on SQLite can lease; today the lease store is memory or PostgreSQL only.
- **Evidence.** E03 summary and §1 (halt: three stores; lease: two; journal: three).
  `ls crates/a2a-protocol-server/src/store/lease` shows `S/store/lease/mod.rs`, `S/store/lease/postgres.rs`, `S/store/lease/tests.rs`.
- **Gates.** none. **Depends.** none. **Release.** `[0.17]`, additive behind `sqlite`.
- **Read first.** `S/store/lease/postgres.rs`, the halt store's SQLite implementation as the model, the
  store conformance suite under `S/conformance/`, E07 §B10.
- **Do.** Implement against the same contract tests the PostgreSQL store passes; deadlines from one clock
  source, stated. Add it to `Profile` checks that today accept a process-local lease store.
- **Touch.** `S/store/lease/` (a new `S/store/lease/sqlite.rs`), `S/store/mod.rs`, the contract tests,
  `book/src/deployment/horizontal-scaling/`, `scripts/check_sql_driver_isolation.sh` if it lists modules,
  `CHANGELOG.md`.
- **Accept.** The lease contract tests pass on all three stores. `scripts/check_sql_driver_isolation.sh`
  exits 0. `python3 scripts/check_feature_tables.py` exits 0.
- **Out of scope.** Lease groups (ADR 0044; needs SCL-06's measurement first).
- **Done (2026-11-26).** `S/store/lease/sqlite.rs`, `SqliteLeaseStore`. Clock: the callers', which on one
  host is one clock; a renewal saturates as the in-memory store's does (SQLite turns an overflowing sum into
  a float). Accept: the lease tests pass on all three stores (12 with PostgreSQL 16, among them
  `sqlite_keeps_the_contract` and `postgres_keeps_the_contract_on_the_database_clock`);
  `scripts/check_sql_driver_isolation.sh` and `scripts/check_feature_tables.py` exit 0. The durable and
  shared-store profiles accept it over a file and refuse it in memory (`S/builder/durable_sqlite_tests.rs`).

## COR-10: The 0.17 fleet rows the register already holds

- **Goal.** Each of ROADMAP *Fleet and failure correctness* items 7 to 11 and 13 is closed or re-stated
  against HEAD.
- **Evidence.** ROADMAP lines 82 to 98; E06 §2 ("treat every `partial` triage row as needing a re-check").
- **Gates.** none per row unless the row says so. **Depends.** FND-03. **Release.** `[0.17]`.
- **Do.** One pull request per item. For each: re-check at HEAD first (E06 found five of nine sampled rows
  already fixed); write the failing test; fix. The items: a finished task whose owner died completes sooner
  than three claim lifetimes (X-09); steering's dead-replica window with tests over gRPC and WebSocket
  (ADR 0036); hand-off for a crashed replica, a handler without claims and a reattached delegation
  (ADR 0054); parked-task expiry's two races (ADR 0056); the push outbox cases ADR 0037 lists as not
  covered; the WebSocket and client liveness rows.
- **Touch.** Per item.
- **Accept.** Per item: its test fails before and passes after, and its register row carries the commit.
- **Out of scope.** Item 12 (load and admission across a fleet; S3 needs a decision).

## COR-11: The security rows with a mechanical test

- **Goal.** The high-severity RFC 8725 rows are tested, and the actor chain is checked against a published
  weakness.
- **Evidence.** `docs/handoff.md` *Waiting on the maintainer* ("What the RFC ledger left open": four
  high-severity rows implemented and untested; three GAP rows); E02 §5 (an RFC 8693 chain-splicing weakness
  disclosed 2026-02-26, "worth checking against" `S/auth/actors.rs`); E06 `BL-035`.
- **Gates.** none. **Depends.** none. **Release.** `[0.16.x]` for tests; a fix that tightens a verifier is a
  behaviour change and is labelled.
- **Read first.** `docs/compliance/rfc-ledger.md` (the open rows), `S/auth/` (the JWT validator and the
  actor chain), the rule that a security row is `TESTED` only with a recorded mutant a test of the row
  killed.
- **Do.**
  1. For each of the four rows: a test, then a hand mutant of the check that the test kills
     (`python3 -I scripts/hand_mutate.py --help` for the harness), recorded as the ledger requires.
  2. Read the disclosed weakness from its primary source, state in the pull request whether the server's
     reading of the `act` chain is exposed, and add a test either way.
  3. The three GAP rows: fix, or record a `DEVIATION` with its reason.
- **Touch.** `S/auth/`, `docs/compliance/rfc-ledger.json` and `.md`, tests.
- **Accept.** `python3 scripts/check_rfc_ledger.py` exits 0 with the four rows `TESTED`. Each recorded
  mutant is reproduced by the harness in CI's form.
- **Out of scope.** RFC 7517, 7518, 8785, 6750, 9068, 6749 ledgers (not started; a separate package when
  asked for).

## COR-12: Close CI1

- **Goal.** The one open release-gate row is root-caused or closed with evidence.
- **Evidence.** ROADMAP *Fleet* item 3; `docs/handoff.md` (270 targeted macOS runs reproduced nothing; the
  failure message now says late or lost).
- **Gates.** none. **Depends.** none. **Release.** internal.
- **Do.** Count green macOS legs on `main` since `7acfbe35` from the Actions history (state the query). If
  it recurred, take the new message as the lead. If not, close the row with the count and the date.
- **Touch.** `docs/audits/2026-11-13-adversarial-review/findings.json`.
- **Accept.** The row's status is `FIXED` with a cause and a test, or closed with the count and the query
  that produced it.
- **Out of scope.** Rewriting the discovery tests.
- **Done (2026-11-26).** Closed without a cause: CI1 did not recur. 12 `.github/workflows/ci.yml` push runs on `main` since
  `7ac5b74e` (the first holding `7acfbe35`); their macOS test legs ran 22 times, 21 green; the red one failed
  the CI2 test, not a discovery test. The query and the run ids are in the row
  (`docs/audits/2026-11-13-adversarial-review/findings.json`, `CI1`, status `CLOSED`).
