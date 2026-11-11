<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# External review of 2026-11-09 (a2a-rust against a2a-rs): register

An independent reviewer compared this repository's `main` (`9214541e`) with
a2a-rs `main` (`505dbad1`). This file records, for each claim that bears on this
repository, whether the code supports it, what was done, and what is left.
`findings.json` is the same data for a script. Nothing here comes from the
review's word alone: each row's evidence is a file, a test or a command that
was read or run on this tree, and the review's statements about a2a-rs were
not re-derived (only a2a-rust was read).

Counts, from `findings.json` (22 rows): 4 fixed, 5 no defect in this repository, 5 not a code finding / maintainer, 7 open, 1 partly done.

## What the review got right and wrong about this repository

Right, and fixed: two workflows died on a download, not on the code (X-01, X-02),
and the class was wider than the two instances (X-03). Right, partly fixed: the
crash-consistency limitations the project already listed (X-08 to X-11). Right,
open: no current head-to-head benchmark and an unmeasured readiness bar (X-14,
X-15); the compliance gaps that need a format change (X-19).

Wrong or incomplete as stated: fuzzing is not smoke-only (X-12: nightly, 10
minutes a target, a persisted corpus); the two a2a-rs defects it found do not
apply here (X-04 to X-06), and are now pinned by tests that were each seen to
fail with the defect put in by hand.

## Register

| Id | Claim | Status | Evidence |
|---|---|---|---|
| X-01 | ci.yml static-checks died installing mdBook: archive 'not valid gzip', so no later gate ran | CONFIRMED, FIXED | ci.yml, benchmarks.yml and docs.yml each ran `curl -sSL <url> | tar -xz` (no --fail, no retry, no checksum). Reproduced as scripts/ci/test_fetch_verified.py cases `error_page` and `flaky_504`. |
| X-02 | observability.yml failed on an HTTP 504 downloading test tooling | CONFIRMED, FIXED | deploy/observability/scripts/install_tools.sh had a checksum and `curl -fsSL` but no retry. |
| X-03 | (not in the review) other downloads of the same class | FOUND BY THE NEW GATE, FIXED | check_workflow_downloads.py on the unmodified tree: SPIRE tarball (no retry, no digest), kubeconform (no retry), two network clones (itk.yml, official-tck.yml), cargo-hack and cargo-llvm-cov installed unpinned, no CARGO_NET_RETRY in four workflows. |
| X-04 | a2a-rs maps an absent Part.content to empty text; is a2a-rust affected? | NOT AFFECTED, now pinned on the wire | crates/a2a-protocol-types/src/proto/convert/messaging.rs `TryFrom<pb::Part>` refuses an unset oneof; a unit test existed. No test drove it through a binding. |
| X-05 | a2a-rs drops typed error details on gRPC; is a2a-rust affected? | NOT AFFECTED | crates/a2a-protocol-server/src/dispatch/grpc/details.rs encodes BadRequest and Struct details; unit tests in the same file; a stream error keeps its streamLagged marker (native.rs test). |
| X-06 | a2a-rs awaits webhook delivery before publishing to live subscribers; is a2a-rust affected? | NOT AFFECTED, now measured | push_hang_live_subscriber.rs: real TCP peer that reads the webhook request and never answers; a streaming send ends in 0.23 s (executor pacing 0.2 s). With push submission run inline (block_in_place) the same test fails: 'the stream did not end while the webhook hung' (10 s guard). |
| X-07 | SSRF corpus never run against both SDKs | OPEN (evidence only) | a2a-rust's own corpus is pinned by tests: decimal/hex/octal/packed IPv4, IPv6 loopback and link-local, IPv4-mapped/compatible/NAT64, CGNAT, DNS pinning, redirects not followed, proxy answers still refused (server/src/push/sender.rs tests, tests/push_sender_*.rs). |
| X-08 | event commit and outbox insertion are not atomic (H14) | CONFIRMED, FIXED by a two-phase row, not by one transaction | ADR 0037 amendment 2026-11-11; `push::outbox::contract`, `processor_reserve_tests.rs`, `sync_collector_reserve_tests.rs`, `reaper/tests/outbox.rs`; `scripts/live/outbox_live.py`. Until now the loss window was the lane's backlog (up to `push_queue_capacity`), not one write. |
| X-09 | crash-consistency issues of the earlier review still apply | PARTLY FIXED | W2-hostile-3 reproduced live (scripts/live/finish_live.py, two replicas on PostgreSQL, owner killed with SIGKILL as the cancel returned): 75 s later the stored state was canceled, 0 events carried the ending, the claim row was still there. |
| X-10 | an outage longer than the write retry loses the event and can drop the failed status | CONFIRMED, OPEN | handoff 'Open engineering': 'Not solved: an outage longer than the retry loses the event being written'. With turn claims the reaper ends a task left working after the claim lapses; without claims nothing does. |
| X-11 | a status written with no event (E-F25) | CONFIRMED, OPEN | CHANGELOG Known Limitations; measured 2.0 s for a cancel at a 2 s ceiling. |
| X-12 | fuzzing is smoke-only in CI | NOT SUPPORTED | .github/workflows/fuzz.yml: nightly cron 41 3 * * *, 600 s per target (60 s on PRs), corpus restored and saved per target; fuzz/fuzz_targets has 18 targets including proto_convert, json_deser, webhook_url. |
| X-13 | MSRV 1.94 against 1.85 | TRADE-OFF, documented | STABILITY.md section 5 and README: 0.15.0 raised the floor from 1.88 to 1.94 for sqlx 0.9. A lower floor for the types and client crates alone is not possible in one workspace lockfile while the server needs sqlx 0.9. |
| X-14 | no current head-to-head benchmark; the history shows a2a-rs faster in places | CONFIRMED, MEASURED; a2a-rs is faster here; partly improved, gap open | benches/sdk-comparison was pinned at 0.14.1. Re-run on one host against a2a-rs's current published crates: a2a-rs serves 1.25 to 1.9x the blocking-send requests/s of this tree, a2a-rust uses 2.5 to 3x less memory; this tree was 18% to 29% slower than 0.14.1 at one connection (docs/sdk-comparison-2026-11-11.md). |
| X-15 | the production-readiness measurement matrix is incomplete | CONFIRMED, one row measured (F1); the rest open | docs/readiness-bar.md: 'No row above (R1 to R7, C1 to C5, F1 to F5) has been measured'. |
| X-16 | SLIMRPC is outside the workspace and unpublished | CONFIRMED, MAINTAINER'S DECISION | bindings/a2a-protocol-slimrpc has its own lockfile; handoff 'Publishing'. |
| X-17 | the CLI is not published | CONFIRMED, MAINTAINER'S DECISION | packaging/README.md: binaries wait on vars.PUBLISH_CLI_BINARIES. |
| X-18 | the official CLI states it sends no usage telemetry and redacts credentials; a2a-rust's CLI says neither | PARTLY TRUE, OPEN (low) | tools/a2a-cli/README.md 'Telemetry': with OTEL_EXPORTER_OTLP_ENDPOINT unset 'no exporter is built and nothing is sent'. Nothing there about credentials in output, and no test that none is printed. |
| X-19 | crash-time audit consistency, cross-replica erasure, metadata encryption and onward propagation need resolution for regulated use | CONFIRMED, OPEN | docs/compliance/controls.toml rows CRA-AI-I-2e (partial), GDPR-5-2 (partial), GDPR-17-2-19 (integrator). Crash-time audit consistency, read in the code: AuditedTaskStore::append_event (audit/task_store.rs) appends to the log and then writes the `task.event` record, and `record_event` returns nothing, so a crash between the two leaves an event in the log with no audit record; nothing checks the chain against the log's positions (AuditLog::verify checks the chain only). EncryptedTaskStore leaves ids, states, timestamps, metadata and order in the clear. |
| X-20 | neither SDK should be presented as operating at OpenAI/Anthropic/Google scale | VERIFIED | grep of README.md, ROADMAP.md, book/src, docs/*.md and the crate READMEs for hyperscale, web-scale, planet-scale, enterprise-grade, battle-tested, proven in production: no claim of that kind; book/src/deployment/horizontal-scaling.md:168 asks for measurements. |
| X-21 | single-maintainer concentration | CONFIRMED, not a code finding | MAINTAINERS.md |
| X-22 | ACTS runs of the two projects used different revisions | CONFIRMED for a2a-rs; a2a-rust is pinned | acts.yml pins a2a-itk at bcce1a15; itk.yml floats `main` (A2A_ITK_REVISION: main) for its own baseline run, which is a different gate. |

## How the crash-consistency work is split

**Built: the reaper finishes a task that is terminal while its owner is gone
(X-09).** A turn that ends releases its claim. A claim left behind a terminal
task means the owner stopped between the state reaching the store and the
ending reaching the log and the webhooks. `TurnClaimStore::ended_unreleased`
lists those claims (expired for longer than the claim time to live, on a task
in a terminal state); each look of the reaper takes one over, reads the log's
last event, appends the ending and sends it to the webhooks when the log does
not already end the task, and releases the claim. When the log does end the
task it only releases the claim: the owner got that far, and nothing provable
is missing. A claim that expired inside the margin, one this handler holds and
one that is live are left alone. No second terminal event is written in the
normal path (a live owner finishes its own task), which is what the maintainer's
open question about duplicate terminal events turned on.

Checked live, on real processes: two replicas on one PostgreSQL database, a task held
on A, `CancelTask` to B, A killed with `SIGKILL` as the cancel returned
(`scripts/live/finish_live.py`). On the tree before the change, 75 s later: state
`canceled`, no event carrying the ending, the claim row still there. On this tree: the
ending in the log once and the claim gone 44 s after the kill; 42 s with the owner in
the middle of a real model's answer (a `llama-server` slot processing at the moment of
the cancel). 21 hand mutants of the new code and SQL, all killed.

**Built: the outbox row before the event is stored (X-08).** ADR 0037's first
decision asked for the row in the event's transaction. The event log and the
outbox are written through different stores, and the log is appended before the
fold for every event but a terminal one, so a row tied to the append would queue
events the fold refuses; a transaction across the stores would be an API every
`TaskStore` and each of its decorators would implement. What was built instead
(ADR 0037, amendment 2026-11-11) is a two-phase row: the processor, and the
blocking send's collector, **reserve** an event's rows (unarmed, never claimed,
holding their webhooks' places) before the event's first durable write, then
**arm** them if the state machine announced the event or **release** them if not.
A reservation whose hold runs out belonged to a stopped replica; every replica's
worker settles it from the task store (the stored task has reached the event's
log position and the log holds no different event there: armed, else removed).
A task's ending that no queued event carried is recorded before it is logged,
and the reaper records through the outbox. Proof: the contract suite on three
outboxes (PostgreSQL live), the settlement's decision table, tests that abort a
task at a chosen point of the store write with two replicas over one outbox, 41
hand mutants of the new code (`scripts/mutants/push-outbox-h14/run.sh`), and `scripts/live/outbox_live.py` (two replicas on
PostgreSQL, one killed with `SIGKILL` at its `arm`: the event delivered once by
the survivor, in order, before the reaper's ending; at its `reserve`: the event
the task never stored not delivered). Not covered, and written down in the ADR,
the book and the CHANGELOG: a store with no event log; an outbox that is down
when an event is reserved; an executor panic's one write between its save and
its record where turn claims are off.

## Measurement (done in this change: see `docs/sdk-comparison-2026-11-11.md`)

The review's strongest fair point is that neither SDK has a reproducible
head-to-head at these commits. `docs/readiness-bar.md` lists R1 to R7, C1 to C5
and F1 to F5 as unmeasured; `benches/sdk-comparison` is pinned at 0.14.1.
Measuring them needs a quiet machine, so it follows the code changes in the
session that makes them; the results go in `docs/readiness-bar.md` with the
commit, host, build profile, command and spread, and nowhere else.
