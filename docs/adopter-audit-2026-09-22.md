<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Adopter audit, 2026-09-22 — all crates, and phase 1 of the fixes

Prompted by using `a2a-protocol-server` as the coordinator of a Go agentic
application: the observability it promised was not there out of the box, and
defects kept reaching releases past every review and test. Six parallel audits
looked at every crate from an adopter's seat. The findings follow as they were
reported; the status section says what has happened to them since.

## Status

**Phase 1 — fixed on `claude/pensive-allen-socw7b`**, each with a test that
failed before its fix: T1 (read side), S6, S7, O16 (push URL logging), S1,
S9, S3, C1, C2, C3, C4, C6, C8, C9, C10, C11, C15. The CHANGELOG's
`[Unreleased]` section describes each, and what it trades.

**Moved from CONJECTURED to VALIDATED by phase 1:** S3 was real on all three
stores (`tests/cross_replica_cancel/`).

**Found during phase 1, not in the tables below:**

- **Merging two correct fixes broke the REST stream-lag signal** (fixed,
  `adce975`): the server's new `google.rpc.Status` frame carries `data` as a
  `Struct` detail, and the client's new decoder kept the whole object as
  `data`, so `is_stream_lagged` read false against this repository's own
  server. Neither half's tests could see it.
- **The first shutdown fix cancelled at once** (fixed, `8161455`): a short
  call in flight at a rolling deploy was answered `Canceled`. Shutdown now lets
  work finish for `completion_grace` first.
Rows in the tables below carry a **[Fixed: …]** marker naming the commits
that fixed them. A row with no marker is open.

## Open work — ready to pick up

Each item gives the evidence behind it, where the code is, how to reproduce
it, the fix proposed, the test that has to fail before the fix, and when it is
done. VALIDATED means reproduced or read in this session, with what was run;
REPORTED means a phase-1 worker reported it and it was not re-checked. Line
numbers are at `09b2403`.

### OW1 — a2a-go cannot read this SDK's `securityRequirements` (T1, write side)

- **Severity:** Medium. A Go client cannot resolve the card of any agent built
  on this SDK that advertises security requirements.
- **Evidence:** VALIDATED. Leg 1b of `scripts/go_sdk_interop.sh` asserts the
  rejection on every run: `json: cannot unmarshal object into Go struct field
  AgentSkill.skills.securityRequirements.schemes of type
  a2a.SecuritySchemeScopes`. Upstream is unfixed even on `main`: a shallow
  clone at `522f856` (2026-09-18) still declares `type SecuritySchemeScopes
  []string` (`a2a/auth.go:80`) with no `{"list":[...]}` handling, and
  `git ls-remote --tags` lists v2.5.0 as the newest tag. A worker reported the
  upstream issue as a2aproject/a2a-go#430; it has **not** been checked, because
  this session had no API access to that repository.
- **Nothing to fix here.** This SDK already writes the normative shape: the
  proto's `map<string, StringList>`, the spec's §8.5 sample, and the Python SDK
  (a2a-sdk 1.1.5, `MessageToDict`) all agree. It reads both shapes
  (`8e218a4`).
- **When a2a-go ships a fix:**
  1. Bump the pin in `itk/agents/go-sdk/go.mod` and
     `itk/interop/go-sdk-client/go.mod`.
  2. Leg 1b of the gate goes red with "a2a-go now reads {"list":[...]}".
  3. In `scripts/go_sdk_interop.sh`, replace leg 1b's `-expect-card-rejected`
     run with the full battery plus `-expect-security` against the secured
     echo-agent.
- **Done when:** that leg passes with `-expect-security`.

### OW2 — cross-replica cancel is enforced only at the store (S3 residue)

- **Severity:** Medium, for multi-replica deployments without session
  affinity.
- **Evidence:** REPORTED by the stores worker, except (d), which a test pins.
  - (a) Replica A learns that B cancelled only at its executor's next write,
    so a silent executor keeps running, and one that ignores its token runs
    until it returns.
  - (b) A client streaming from A can still receive non-terminal frames that
    A's executor emits between B's cancel and that write; they never reach the
    store.
  - (c) B's `CancelTask` runs B's executor's `cancel()` hook
    (`handler/lifecycle/cancel_task.rs:94`), so it releases B's in-process
    resources, not A's.
  - (d) Admission is per replica: two replicas both accept a continuation of
    the same task and run two executors
    (`tests/multi_replica.rs:556`, `the_single_writer_refusal_does_not_cross_replicas`).
    Only the terminal state is protected; earlier writes are last-writer-wins.
- **Where:**
  - `handler/lifecycle/cancel_task.rs`: :57–61 cancels the local token only;
    :121–123 is the store write and its `TerminalStateConflict` mapping.
  - `handler/event_processing/background/state_machine.rs:58–63, :124`: the
    `Refused` outcome.
  - The processor's module doc, `background/processor.rs`: what A does on a
    refusal.
- **Fix options:**
  1. A running processor polls the stored state on an interval and cancels its
     executor on a foreign terminal state. This works with every store; the
     interval is the detection latency.
  2. PostgreSQL `LISTEN/NOTIFY` on cancel. Immediate, but PostgreSQL-only.
  3. For (d), a lease row with an expiry, taken at admission.
  4. Session affinity, which is a deployment choice.

  The proposal is option 1 first, and option 3 when (d) matters.
- **Failing-first test:** in `tests/cross_replica_cancel/`, give replica A an
  executor that writes nothing after `Working` and waits on its token. Cancel
  through B, then assert that A's token fires within a bound. It fails today,
  because nothing ever tells A.
- **Done when:** that test passes on the in-memory, SQLite and PostgreSQL
  stores, and `book/src/deployment/horizontal-scaling.md` states the new
  latency bound.

### OW3 — two replicas starting against a fresh PostgreSQL database can crash one

- **Severity:** Medium. It is pre-existing and bites only the first start on
  an empty database; a restart then succeeds, because the tables exist.
- **Evidence:** VALIDATED.
  - Two concurrent `CREATE TABLE IF NOT EXISTS` statements on a fresh
    database failed 29 times in 40 with `duplicate key value violates unique
    constraint "pg_type_typname_nsp_index"` (PostgreSQL 16.13).
  - `examples/resilient-agent`'s act 3 test, which uses `with_migrations`,
    failed with the same error when two of its tests shared one database.
- **Where** — every schema statement runs unlocked, in
  `crates/a2a-protocol-server/src/`:
  - `store/postgres_store/mod.rs:131`, `from_pool`, which `new` (:99) calls.
  - `store/pg_migration.rs:158`, `ensure_version_table` (:160). It is called
    by `run_pending` — so by `with_migrations` (`postgres_store/mod.rs:113`),
    the documented production constructor — before `run_pending` takes its
    `LOCK TABLE schema_versions`.
  - `store/tenant_postgres_store/mod.rs:103`, `from_pool`.
  - `push/postgres_config_store.rs:63` and
    `push/tenant_postgres_config_store.rs:66`, both `from_pool`.
  - `rate_limit/shared.rs:228`, `from_pool`.
  - The statements they run: `store/postgres_store/event_log.rs:28`,
    `store/postgres_store/idempotency.rs:20`, `store/tenant_event_log.rs:53`,
    `store/tenant_idempotency.rs:41`.
- **Reproduce** (needs only `psql`):

  ```bash
  export PGPASSWORD=postgres; H="-h localhost -U postgres"; hit=0
  for i in $(seq 40); do
    psql $H -qc "DROP DATABASE IF EXISTS race_probe" -c "CREATE DATABASE race_probe"
    for s in 1 2; do psql $H -d race_probe -qc \
      "CREATE TABLE IF NOT EXISTS t (id TEXT PRIMARY KEY)" 2>>race.err & done; wait
  done; grep -c "duplicate key" race.err
  ```
- **Fix:** run each constructor's schema statements in one transaction that
  first takes `SELECT pg_advisory_xact_lock($KEY)`, with a single crate-wide
  constant key so that task, push and rate-limit stores serialize against each
  other. PostgreSQL DDL is transactional, so the lock, the DDL and the
  version-table creation commit together.
- **Failing-first test:** an `#[ignore]`d PostgreSQL test that creates a fresh
  database, as `TestDb::create` does in `tests/multi_replica.rs`, and builds
  eight `PostgresTaskStore::with_migrations`, and eight `from_pool`, against
  it concurrently. It asserts every one is `Ok`. It fails today with the
  `pg_type` error.
- **Done when:** that test passes in 20 consecutive runs, and the same holds
  for the push and rate-limit stores.

### OW4 — resolved: `swarm_scale`'s replay test was broken by its own fixture

Kept here because what it took to settle is the useful part.

- **Symptom:** `fan_out::a_tail_can_recover_what_it_missed` failed at
  `d423b94` and at `09b2403`, run exactly as `docs/swarm-scale-findings.md`
  says. It printed "40 posts landed; replay returned 0 positions", against the
  doc's recorded 42.
- **Not a library regression.**
  - A bisect over `e0b9964..d423b94` landed on `ce0d782`. Its parent
    `140828f` replays 42 positions; `ce0d782` replays 0. Both were re-run with
    `--all-features` because they do not compile with default features.
  - `ce0d782` touches no library code. It gave the harness an agent card that
    does not advertise `streaming` (`tests/swarm_scale/fixtures.rs`), so the
    capability check (`handler/capability.rs:41`) refused the tail's
    `SubscribeToTask`.
  - The test's `tail()` turned that refusal into an empty result, so it read
    as a log that replayed nothing.
- **Fixed in `3c1112f`:**
  - The card advertises streaming.
  - `tail()` records a refusal, and the test asserts there was none before it
    looks at the log. With streaming removed again, it now fails with "the
    resubscribe was refused … HTTP 400 … UNSUPPORTED_OPERATION".
  - Documented run: 13 passed, replay "42 positions spanning 42, first
    Some(1) last Some(42), gaps 0". With `--all-features`: 16 passed.
- **What had been read wrongly first:**
  - Three `cost::` tests fail when the suite runs in parallel. That is
    contention between load experiments on 4 cores: they fail the same way at
    `d423b94` and pass when run as documented.
  - A worker had reported the whole suite as "failing under PostgreSQL" from
    that parallel run.
- **Open follow-up** (Low, gap in the gates): these load experiments are
  `#[ignore]`d and run in no workflow, which is how a fixture change broke one
  for weeks unnoticed. One option is a nightly job running the documented
  command.

### OW5 — gRPC status codes lose what the caller needs (C16)

- **Severity:** Medium.
- **Evidence:** VALIDATED by reading the code.
  - `crates/a2a-protocol-client/src/transport/grpc.rs:848–861`
    (`grpc_code_to_error_code`) maps `Unauthenticated` and `PermissionDenied`
    to `InvalidParams`.
  - `grpc.rs:504` maps a `Cancelled` status to `ClientError::Timeout`, which
    is retryable, so a caller's own cancel can be retried.
  - The binding does the same at `bindings/a2a-protocol-slimrpc/src/error.rs:153`.
  - Consequence: the phase-1 401 hook, `BearerAuthInterceptor::on_error`
    (`token_provider.rs:259`), matches only `UnexpectedStatus { status: 401 }`,
    so a gRPC `Unauthenticated` never invalidates a cached token.
- **Fix:** give auth failures their own mapping that the 401 hook also
  matches, either a new `ClientError` variant or `UnexpectedStatus` with 401 or
  403. Map `Cancelled` to a non-retryable error, and keep `ErrorInfo` details.
- **Failing-first test:** extend `tests/bearer_token_invalidation.rs` with a
  gRPC stub that answers `Unauthenticated`, and assert that the next call
  carries a new token. It fails today.
- **Done when:** that test passes, and the same mapping is applied in the
  slimrpc binding.

### OW6 — the gRPC and WebSocket dispatchers take no shutdown signal (S8)

- **Severity:** Medium.
- **Evidence:** VALIDATED by reading the code.
  - `dispatch/grpc/dispatcher.rs:171`: `serve(addr)` has no signal
    parameter.
  - `dispatch/websocket.rs:229`: `serve` has none either, and at :230 it
    discards the server's error with `let _ =`.
  - `1c0af5d` documented how to stop them by hand, with `finish_in_flight`.
- **Fix:** add `serve_with_shutdown(addr, signal)` to both, running the same
  sequence as `Server::serve_with_shutdown`: stop accepting, call
  `finish_in_flight`, then drain. Return the server's error instead of
  dropping it.
- **Failing-first test:** port `tests/graceful_shutdown_tasks.rs`'s delegation
  test to both dispatchers. It cannot compile today, because there is no signal
  to pass.
- **Done when:** both dispatchers pass it, and the book's production chapter
  drops the manual recipe.

### OW7 — OAuth2 token-endpoint connection failures are classed as permanent

- **Severity:** Low.
- **Evidence:** VALIDATED by reading the code.
  - `crates/a2a-protocol-client/src/token_provider.rs:498` maps a failed
    request to the token endpoint, a refused connection included, to
    `ClientError::Transport`, which is not retryable.
  - Through `From<ClientError> for A2aError`, a task that fails on it is
    therefore classed `Internal`, not `Transient`.
- **Fix:** map connection errors and timeouts at :498 the way the transports
  do: `HttpClient` for a connection error, `Timeout` for a timeout.
- **Failing-first test:** point `OAuth2ClientCredentials` at a closed port and
  assert `err.is_retryable()`.

### OW8 — terminal-state gate follow-ups (phase-1 `c597a56`, `4874074`)

- **Severity:** Low. Everything here is REPORTED by the stores worker and not
  reproduced.
  - A streaming client's final frame can wait up to the queue's write timeout
    (5 s) behind push deliveries of earlier events in the background
    processor.
  - The final frame is appended to the event log only after the store has
    ruled on it (`handler/event_processing/background/mod.rs:224`), so a crash
    between the two leaves the log without the final event.
  - Custom `TaskStore`s get no terminal protection unless they call
    `store::refuses_write` (`store/terminal.rs:80`) inside their own writes.
    That is documented, and nothing enforces it.
- **Next step:** reproduce the first two, each with a test, before choosing a
  fix.

### OW9 — client stream follow-ups (phase-1 `9ed2bcb`, `f9f907c`, `848466a`)

- **Severity:** Low.
  - The gRPC keepalive settings (`transport/grpc.rs:311–313`) are
    CONJECTURED: no test reads them back. `tests/` has a socket2 read-back for
    the HTTP connector that can be copied.
  - gRPC and WebSocket events pass through the SSE parser's
    `max_event_size` (16 MiB by default) while gRPC's own cap,
    `max_decoding_message_size` (`grpc.rs:440`), is 32 MiB. REPORTED.
  - C20 is unchanged: the frame queue drops its oldest frames beyond 4,096
    (`streaming/sse_parser/parser.rs:90`), and an `id:` containing NUL clears
    the stored id.

### OW10 — resolved: ADR 0007 said flat security scopes are rejected

`docs/adr/0007-axum-integration-and-tck.md:35` described the pre-`8e218a4`
behaviour. The ADR now carries a dated amendment at its end rather than an
edited line, since ADRs are records.

### OW11 — the coordinator end-to-end test phase 1 promised and did not build

- **Severity:** Medium, as a gap in the gates.
- **Where it was promised:** section 7, phase 1: "a coordinator end-to-end
  test against a2a-go". What phase 1 built instead is
  `scripts/go_sdk_interop.sh`, which drives each direction on its own: an
  a2a-go client against this server, and this client against an a2a-go server.
  Nothing runs the application's actual shape, a Go client calling a Rust
  coordinator that delegates to a Go worker, so trace, cancel and
  stream-forwarding across both hops are unexercised.
- **Proposal:** build it with phase 3's delegation helper, which the
  coordinator would exercise, and add it as a third leg of the same script.
  Pass criteria:
  - The Go client's stream ends `completed`, with the Go worker's artifact
    forwarded.
  - A cancel through the coordinator reaches the worker, which logs it.
  - The worker sees the Go client's trace-id.

### OW12 — every other open finding, by phase

Section 7 gives the order. Every row below the Status section that has no
**[Fixed …]** marker belongs to exactly one of these:

| Phase | Findings |
|---|---|
| 2 — observability | O1–O15; O16 apart from the push URL; O17 |
| 3 — coordinator developer experience | S4, S5, S10, S11, C7, C18, K2, and OW11. C4 (`From<ClientError>`), listed in section 7's phase 3, was done in phase 1 |
| 4 — signing and types | T2–T7, K1 |
| 5 — docs checked against code | C5, T8, S12, S13, K3, K4, and C19's README overstatement |
| Open work above | T1 → OW1, S2 → OW13, S8 → OW6, C16 → OW5, C20 → OW9 |
| Unscheduled | S14, S15, S16, C12, C13, C14, C17, C19 (retry behaviour), C21, T9, T10, T11, T12 |

The unscheduled rows are real and mostly Low. C12 (agent-card URLs used
unchecked — SSRF, and bearer tokens sent over a downgraded scheme) and C14
(one slow WebSocket consumer blocks the socket) are the two Medium ones worth
scheduling first.

### OW13 — a2a-go's client loses JSON-RPC streaming pre-stream errors (S2)

- **Severity:** Medium for a Go client calling this server over JSON-RPC.
  REST and gRPC are unaffected: a Go client gets a typed `TaskNotFound` on
  both, and the interop gate checks that.
- **What happened:** phase 1 first "fixed" S2 in `0a076e1` by sending the
  error as one SSE `event: error` frame, which a2a-go reads. The official
  conformance kit then failed on PR #141 with a REGRESSION on STREAM-SUB-003
  and STREAM-SUB-004 over JSON-RPC.
  - The TCK's JSON-RPC client (`tck/transport/jsonrpc_client.py`,
    `_call_streaming`) reads any `text/event-stream` answer as a successful
    stream.
  - a2a-go's client (`a2aclient/jsonrpc.go`, `sendStreamingRequest` and
    `parseSSEStream`) reads only SSE `data:` lines. Given a non-200 status, it
    returns an untyped "unexpected HTTP status" error and discards the body.
  - So no single response satisfies both, and a non-200 status would also
    break this crate's own client, released versions included, which turn a
    non-2xx streaming answer into `UnexpectedStatus`.
  - This repository treats the official suite as authoritative where the two
    overlap (`docs/official-tck-findings.md`), so the server sends the plain
    JSON 200 again. With that, the official TCK run locally reports "failures
    exactly match the baseline; no regressions".
- **What stays from phase 1:** this crate's client reads both shapes
  (`crates/a2a-protocol-client/tests/jsonrpc_stream_refusal_tests.rs`), so it
  works against a2a-go's server as well as this one.
- **Pinned:** `itk/interop/go-sdk-client`'s `expectLostByGo` passes only
  while a2a-go shows an empty stream with a nil error over JSON-RPC. It goes
  red, saying to restore the strict check, once a2a-go reports
  `TaskNotFound`.
- **Next step:** report upstream, asking a2a-go's JSON-RPC client to parse a
  non-SSE `application/json` body as a JSON-RPC response, as the TCK's client
  and the Python SDK's do.
- **For an adopter today:** a Go client calling a Rust coordinator should use
  HTTP+JSON or gRPC if it needs typed errors from a stream that fails to open.

## How the evidence was produced

Six independent audits ran in parallel: observability, adopter developer
experience, Go SDK interop, documented claims against code, client
behaviour, and types/sdk/slimrpc behaviour. Several of them built real
programs:

- a coordinator that depends only on `a2a-protocol-sdk`, run against two
  workers built with a2a-go v2.5.0;
- a Go client and a Go server built with a2a-go v2.5.0 (the latest v2
  release, per `go list -m -versions`), run against this repository's crates
  over JSON-RPC, HTTP+JSON and gRPC;
- raw-TCP stub servers that stall, cut off or mis-frame streams to probe the
  client;
- an OpenTelemetry `ManualReader` and a JSON log subscriber wrapped around a
  real `JsonRpcDispatcher`;
- the RFC 8785 test vectors, plus 1M random doubles, fed through the signing
  canonicalizer.

**How to read the labels.**

- **VALIDATED** means a program was run, or a grep/`cargo tree` result is
  quoted.
- **CONJECTURED** means the finding comes from reading the code only.
- **[re-checked]** means the lead re-ran the check themselves rather than
  taking the report's word for it.

The auditors' probe programs lived in a session scratchpad and are gone. What
they proved is kept in-repo as tests: every phase-1 fix carries one that failed
first, and `scripts/go_sdk_interop.sh` is the interop harness made permanent.

---

## 1. Observability (all crates)

| # | Sev | Finding | Evidence |
|---|---|---|---|
| O1 | Critical | **No spans exist in any crate.** The book claims "Task and context identifiers are on the spans … events from inside an executor inherit that context"; this is false. | VALIDATED [re-checked]: a grep for `span!\|info_span\|#[instrument]\|.instrument(\|Span::current` over `crates/` returns 0 hits. `book/src/deployment/observability.md:266-268` |
| O2 | Critical | **The server sends downstream a span id it never records.** `fresh_span_id` (`server/src/handler/helpers.rs:169`) creates a child span id and stores it in `CallContext`, and that id is what goes downstream. No span with that id is ever exported, so every Go agent's trace points at a parent the backend never sees. A user who adds `tracing-opentelemetry` cannot repair this. | VALIDATED: inbound `00f067aa0ba902b7` went downstream as `0508261a5e764cd6`. [re-checked the call sites] |
| O3 | High | `Cargo.toml` feature `otel` says "native OTLP export of traces and metrics". `opentelemetry-otlp` is built with only `metrics`, and there is no `TracerProvider`. The 0.13.0 sweep fixed three documents and missed this one, which docs.rs publishes. | VALIDATED [re-checked]: `server/Cargo.toml:54,109`, `otel/pipeline.rs:54` |
| O4 | High | The executor and background work run in `tokio::spawn` with no `.instrument(...)`. This happens at 4 sites: `execute.rs:106`, `background/mod.rs:62`, `sync_collector.rs:439`, `streaming/sse.rs:264`. A user's own outer span (e.g. an axum `TraceLayer`) is not the parent either. | CONJECTURED (code); the no-span result is VALIDATED |
| O5 | High | The latency histogram records seconds but keeps the SDK's millisecond-sized default buckets `[0,5,10,25,…]`, so everything under 5 s lands in one bucket. Names are not semconv: `a2a.server.latency` / `method`, where semconv has `rpc.server.duration` / `rpc.method` / `rpc.system`. The unit strings in the doc (`otel/mod.rs:65`) are wrong. | VALIDATED (ManualReader) |
| O6 | High | Streaming latency measures only stream setup. A 1.5 s stream recorded 0.0006 s. There is no stream-duration, events-per-stream or active-stream metric. | VALIDATED |
| O7 | High | If `OtelMetricsBuilder::build()` runs before the global MeterProvider is installed, every metric is silently a no-op for the life of the process. There is no warning. | VALIDATED |
| O8 | High | The client has no spans, no metrics hook and no retry counter. `CallInterceptor::after` is skipped when the transport errors (`client/src/methods/send_message.rs:91-100`), so an interceptor cannot time a call or count its errors. | VALIDATED |
| O9 | High | Outbound `traceparent` is opt-in twice: `TracePropagationInterceptor` **and** `CurrentTrace::scope(ctx.trace_context())` around each call. Neither is in the prelude, and the scope is not inherited across `tokio::spawn`. With the interceptor but no scope, nothing is sent. | VALIDATED |
| O10 | Medium | The `pool.{active,idle,created,closed}` metrics are advertised (README.md:77, observability.md:147-150) but `on_connection_pool_stats` has no production caller. If it were called, cumulative totals passed to `Counter::add` would double-count. | VALIDATED |
| O11 | Medium | Some failures produce no metric: malformed JSON, unknown method, executor failure or timeout, tenant-resolution failure (conjectured), and push delivery aborted by a config-store read error (`background/push_delivery/mod.rs:72-74`). There is no task-outcome metric. | VALIDATED except as marked |
| O12 | Medium | OTLP setup covers only part of the `OTEL_*` configuration. It is gRPC only and ignores `OTEL_EXPORTER_OTLP_PROTOCOL`. The `service_name` argument overrides `OTEL_SERVICE_NAME`, and `service.version` is never set. There is no log bridge and no Prometheus option. Graceful shutdown doesn't flush the meter provider, which loses up to 60 s of metrics. `tracing` is off by default in the server and the sdk. | CONJECTURED (code) |
| O13 | Medium | Many failure paths report only through `trace_*!`, which compiles to nothing without the non-default `tracing` feature. Examples: the WebSocket traceparent drop, which the book says "warns once per connection", and skipped webhooks. | VALIDATED |
| O14 | Medium | Health endpoints are inconsistent. axum `/ready` checks the store. REST `/ready` is a constant. JSON-RPC has `/health` and `/ready` (per the devx audit's live run). gRPC has no `grpc.health.v1`. | Mixed; the two audits disagreed on JSON-RPC, and the live run was taken as authoritative |
| O15 | Medium | Push webhooks carry no `traceparent` (`push/sender.rs:851-906`). WebSocket drops it by design. Only JSON-RPC propagation is tested end to end. | CONJECTURED except JSON-RPC |
| O16 | Low | **[Push URL logging fixed: `9ee3cc3`; the rest is open]** Two INFO lines per request. The untrusted JSON-RPC method name is logged at INFO, a log-forging risk with the plain `fmt` format. The full webhook URL is logged at INFO (`push/sender.rs:776`), and those URLs often carry secrets. Endpoint URLs are logged at INFO on every client call. | VALIDATED except the push URL |
| O17 | Low | `ClientRequest` derives `Debug` over `extra_headers` (`client/src/interceptor.rs:50`), so `{req:?}` prints `authorization: Bearer …`. Server `CallContext` and token providers redact correctly. | VALIDATED (code) |

**What went well:** metric cardinality is bounded (`metric_label()`), and the
bearer token did not appear in server logs. W3C propagation between the Rust
coordinator and Go agents was correct in all 9 binding pairs *when opted in*
(same trace-id, tracestate preserved, Go `Extract` valid).

## 2. a2a-protocol-server

| # | Sev | Finding | Evidence |
|---|---|---|---|
| S1 | High | **[Fixed: `3f6f7d3`, `8161455`, `09b2403`]** **The documented graceful shutdown leaves downstream work running.** SIGINT during a streamed delegation followed the documented order. The 15 s socket drain (`serve/graceful/mod.rs:117`) ran before any task was cancelled. `handler.shutdown()` then cancels tokens but doesn't wait for executors. Result: exit after 16 s with `abandoned: 1`, no terminal event upstream, and no cancel sent to either Go task. | VALIDATED (live, a2a-go workers) [re-checked the constant] |
| S2 | High | **[Reverted on the server — a2a-go's to fix: OW13]** **Over JSON-RPC, a streaming call's pre-stream error is sent as plain `application/json` 200.** a2a-go's client only reads `data:` lines, so it sees `events=0 err=<nil>`. Go clients silently lose "task not found" on `SendStreamingMessage` and `SubscribeToTask`. REST and gRPC are fine. | VALIDATED (Go client) |
| S3 | High | **[Fixed: `c597a56`, `4874074` — residual gaps are open work OW2]** **Possible cross-replica cancel race.** CancelTask on replica B writes Canceled. Replica A's background processor still holds its in-memory `last_task`, and Postgres `save_status_delta` runs an unconditional `UPDATE … WHERE id = $4` (`store/postgres_store/store_impl.rs:241-246`). The client is told Canceled and the task ends Completed. Separately, `tests/multi_replica.rs:530-548` shows two replicas both accepting a continuation of the same task, which `horizontal-scaling.md` does not mention. | Unconditional UPDATE VALIDATED [re-checked]; race CONJECTURED |
| S4 | High | **`agent_executor!` can't be used by an executor that has state** (it hides `self`, `E0424`). Every coordinator has to write out the full `Pin<Box<dyn Future…>>` signature. | VALIDATED (compile) |
| S5 | High | **There are no delegation helpers.** Forwarding a downstream stream into the upstream queue, rewriting ids, passing cancellation downstream and merging fan-out streams all have to be hand-written: 110 of the 230 lines in the auditor's coordinator. The executor's `queue` is borrowed for `'a`, so spawned fan-out tasks can't write to it, which forces an mpsc relay. No book chapter covers delegation. | VALIDATED |
| S6 | Medium | **[Fixed: `32ae44b`]** Over REST, a mid-stream error is sent as `event: error` with a bare `{code,message}`. a2a-go's REST stream parser doesn't recognize it, so a Go client gets "unknown stream response type". | CONJECTURED (both sources) |
| S7 | Medium | **[Fixed: `9ee3cc3`]** The push token header and content type differ from a2a-go. Rust sends `x-a2a-notification-token` / `application/a2a+json`; Go uses `A2A-Notification-Token` / `application/json`. Each side's webhook rejects the other's pushes. The comment at `push/sender.rs:898-903` says official receivers use the X- name, which is not true of a2a-go 2.5.0. | VALIDATED (same webhook) |
| S8 | Medium | **[Documented only: `1c0af5d` — open work OW6]** Graceful shutdown covers only JSON-RPC and REST. `GrpcDispatcher::serve` and `WebSocketDispatcher::serve` take no shutdown signal, and the gRPC background serve discards its error with `let _ =`. | VALIDATED (code) |
| S9 | Medium | **[Fixed: `3f6f7d3`]** README.md:60 says `shutdown()` reports a queue it had to force-destroy. The field is "always 0" (`handler/shutdown/mod.rs:30`), and every queue is destroyed unconditionally. | VALIDATED |
| S10 | Medium | `EventEmitter::status(state)` can't carry a progress message. `RequestContext.task_id` is a `TaskId` but `context_id` is a `String`. | VALIDATED (compile) |
| S11 | Medium | The README's one-line `serve()` is the unhardened path: no connection cap, no header or idle timeout, no shutdown. There is no top-level `max_concurrent_tasks` (per-tenant only). | CONJECTURED (code, but the crate's own docs agree) |
| S12 | Medium | `book/src/reference/configuration.md:17` gives the executor-timeout default as None; the code sets 1 h. The server README's `signing` row says "verification", but the crate does no signing. The feature table omits grpc-tls, auth-jwt, tls-rustls and conformance. | VALIDATED |
| S13 | Low | The README says rate limiting is "per-caller". Without auth or `trusted_proxy_hops`, every caller shares the `"anonymous"` bucket (`rate_limit/identity.rs:45`). | VALIDATED |
| S14 | Low | A missing or `0.3` `A2A-Version` header gets `-32009` with `"id":null` even though the request id was known. There is no v0.3 compatibility layer (a2a-go ships `a2acompat/a2av0`). | VALIDATED |
| S15 | Low | Tasks in flight at a crash or shutdown stay non-terminal in the durable store, and there is no recovery path. | CONJECTURED |
| S16 | Low | Task statuses carry no `timestamp`; Go always sets one. | VALIDATED (Go client) |

## 3. a2a-protocol-client

| # | Sev | Finding | Evidence |
|---|---|---|---|
| C1 | High | **[Fixed: `9ed2bcb`, `f9f907c`]** **A stream that goes silent after its first event hangs forever.** There is no idle or per-event timeout, no TCP keepalive and no HTTP/2 ping (`streaming/event_stream.rs:332-341`, `tls.rs:126-138`). | VALIDATED (stub: still pending at 8 s with 1 s timeouts) |
| C2 | High | **[Fixed: `efd6be0`]** **A stream ending with no terminal event returns `None` like normal completion.** A partial final frame is silently dropped. There is no resume: the client parses `id:` but drops it, and `subscribe_to_task` can't send `Last-Event-ID`, although the server supports resumption. | VALIDATED |
| C3 | High | **[Fixed: `4377528`]** **OAuth2 refresh failures run one after another under one lock.** A failed refresh caches nothing, so each queued caller runs its own full-timeout refresh (`token_provider.rs:538`). With 1 s timeouts, 5 callers failed at 1, 2, 3, 4 and 5 s; at the 30 s default with 100 callers, that is about 50 minutes. | VALIDATED |
| C4 | High | **[Fixed: `5ac7e9a`]** **`ClientError` doesn't convert to `A2aError`**, so `?` in an executor fails (`E0277`). Only the reverse conversion exists (`error/mod.rs:184`). Every call site has to convert to a string, which loses whether it was a timeout, a transient failure or a protocol error. | VALIDATED [re-checked] |
| C5 | High | **The client README (its crates.io page) documents APIs that don't exist**: `resubscribe()`, `get_authenticated_extended_card()`, `ClientBuilder::with_transport()`. It says "10 variants" (there are 11), has a non-exhaustive `match` that won't compile, and gives the wrong description for the `signing` row. | VALIDATED [re-checked] |
| C6 | Medium | **[Fixed: `46791be`]** The first-event timeout reuses `stream_connect_timeout` (30 s). A Go agent that flushes headers and then thinks longer than 30 s is cut off (`jsonrpc.rs:401`, `rest/streaming.rs:87`). gRPC has the same problem. | VALIDATED (stub) |
| C7 | Medium | The blocking `send_message` has a 30 s `request_timeout`, too short for delegation, and retry is off by default. Both shipped coordinators wrap calls in their own timeouts. | CONJECTURED (code) |
| C8 | Medium | **[Fixed: `85c5a6c`, `adce975`]** REST streaming errors aren't decoded, although REST unary errors are. `subscribe_to_task` 404 gives `UnexpectedStatus` where `get_task` gives `TaskNotFound`. Go's in-stream AIP-193 `{"error":…}` frames become `Serialization("unknown variant error")`. | VALIDATED (stub and Go server) |
| C9 | Medium | **[Fixed: `b22ae03`]** Deleting a push config on a Go server reports failure though it succeeded: over JSON-RPC Go returns no `result`, over REST it returns an empty 200. gRPC works. Go is the non-compliant side, but the Rust client should tolerate it. | VALIDATED (Go server) |
| C10 | Medium | **[Fixed: `38f24c7`]** Interface selection ignores `protocolVersion`, so it chose a Go agent's `/v03` endpoint over `/v1.0`. A lowercase `"jsonrpc"` binding makes `build()` fail (the selector ignores case, the factory doesn't). `from_card` falls back to `.first()` and errors instead of trying the next interface. | VALIDATED |
| C11 | Medium | **[Fixed: `848466a`]** The SSE parser truncates an endless line silently and never errors: 50 MiB with no newline gave 0 errors and 0 frames, holding up to 32 MiB. `max_event_size` can't be set from `ClientConfig`. | VALIDATED |
| C12 | Medium | Agent-card interface URLs are used as given: no same-origin check and no https→http downgrade guard, so an SSRF risk and a bearer-token leak risk. | CONJECTURED (code) |
| C13 | Medium | `HTTPS_PROXY`/`NO_PROXY` are ignored, and only the bundled webpki roots are trusted (no system roots). | VALIDATED (grep) |
| C14 | Medium | WebSocket: one slow stream consumer blocks routing for every request on the socket (conjectured). Pretty-printed JSON frames are corrupted by `data:` wrapping (validated). | Mixed |
| C15 | Medium | **[Fixed: `e03d8d7`, `739a304`]** Token cache: `expires_in ≤ 30` means every call hits the token endpoint. A downstream 401 never invalidates the cached token. | VALIDATED |
| C16 | Medium | **[Open work OW5]** gRPC `Unauthenticated`/`PermissionDenied` map to `InvalidParams`. `ErrorInfo` details are dropped. A mid-stream `Cancelled` becomes a retryable `Timeout`. slimrpc does the same at `error.rs:153`. | VALIDATED (code and tests) |
| C17 | Medium | `CachingCardResolver`: a network call on every `resolve()`, no TTL, a stampede under concurrency, no stale-on-error, a new HTTPS client per fetch, no redirect following, a timeout reported as a non-retryable `Transport` error, and an error body up to 2 MiB kept untruncated. | VALIDATED |
| C18 | Medium | `A2aClient` isn't `Clone`, `EventStream` has no `futures::Stream` implementation, `cancel_task` won't accept a `TaskId`, and there is no per-call header API. | VALIDATED (compile) |
| C19 | Low | `Retry-After` is capped by `max_backoff`, HTTP-date values are ignored, and there is no overall deadline across attempts. The README overstates which sends are retried: without an idempotency key, only 429 and 503. | VALIDATED |
| C20 | Low | The SSE frame queue drops the oldest frames beyond 4096 per chunk. `data: a\ndata:\n\n` yields `"a"` where the spec says `"a\n"`. An id containing NUL clears the stored id. | VALIDATED |
| C21 | Low | The default `accepted_output_modes` is injected into every send. `HttpClient(String)` throws away the error chain. | VALIDATED (code) |

**What went well:** dropping an `EventStream` aborts its reader promptly;
`next()` is cancel-safe; chunk and CRLF framing is correct; retry of
non-idempotent sends is correctly limited; body size limits are enforced.

## 4. a2a-protocol-types

| # | Sev | Finding | Evidence |
|---|---|---|---|
| T1 | High | **[Read side fixed: `8e218a4`; write side is open work OW1]** **Agent cards with security requirements can't be exchanged with a2a-go in either direction.** Rust writes `{"o":{"list":["s"]}}` (proto/spec shape); Go writes `{"o":["s"]}`, and each side fails to parse the other. Rust matches the spec, but in practice the reader must accept both shapes. | VALIDATED (both directions) |
| T2 | High | **Signing: serde_json lacks `float_roundtrip`**, so floats in a card are off by one ULP before canonicalization. The RFC 8785 §3.2.4 example gives `333333333.33333325`, 5 of 24 Appendix-B vectors fail after parsing, and 29.7% of random exponent-form doubles parse wrong. | VALIDATED [re-checked: the feature is absent from every manifest] |
| T3 | High | **Signing: verification canonicalizes the re-serialized struct, not the received JSON** (`signing.rs:70-71`). Any unknown field, the legacy `url`, a missing `skills`, `null` capabilities, snake_case aliases or the v0.3 scheme form makes a valid peer signature fail. Empty defaults (`"skills":[]`) are added to the canonical bytes. There is no cross-SDK signing test. | VALIDATED [re-checked the code path] |
| T4 | Medium | The ES number formatter gets exact ties wrong (`1424953923781206.3` vs `.2`). `crit` headers go unchecked (RFC 7515 §4.1.11). A bad signature surfaces as `-32603 Internal`. | VALIDATED |
| T5 | Medium | One unknown enum value fails the whole payload: `TASK_STATE_PAUSED` fails the `Task`, `ROLE_SYSTEM` the `Message`, and an unknown or extra key in `StreamResponse` fails the event. A newer peer can break stream consumers. | VALIDATED |
| T6 | Medium | Values accepted over JSON can't be converted to proto (non-base64 `raw`, integers above 2^53, non-RFC3339 timestamps), so GetTask over gRPC or slimrpc returns INTERNAL. `has_valid_timestamp` accepts `"garbage T garbage garbage"`. | VALIDATED |
| T7 | Medium | JSON requires `contextId` on `Task`; proto doesn't. Large numbers in metadata are silently rounded, and `1e400` rejects the whole message. | VALIDATED |
| T8 | Medium | The types README says `A2A_VERSION = "1.0.0"`; the code has `"1.0"`. Its `Message` literal won't compile, its `match` is non-exhaustive, and `proto` is undocumented. `first-agent.md` and `concepts/agent-cards.md` teach `protocol_version: "1.0.0"` with a 16-field literal instead of the existing builders. | VALIDATED [re-checked README] |
| T9 | Low | `parse_iso8601_to_unix_millis` rolls invalid dates over (`2026-02-31` becomes Mar 3) and accepts non-ISO forms. It feeds ListTasks `statusTimestampAfter`. | VALIDATED |
| T10 | Low | Lossy round-trips through proto and JSON. These matter because signing re-serializes. | VALIDATED |
| T11 | Low | Semver: core structs have all-public fields and aren't `#[non_exhaustive]`, which is inconsistent with `AgentCapabilities`. `TaskState::ALL: [Self; 9]` exposes the variant count. | VALIDATED |
| T12 | Low | `Part::data(Null)` is rejected by Go. `Part::file` with neither bytes nor uri silently produces `raw("")`. Message ids are mandatory and there is no generator. | Mixed |

## 5. a2a-protocol-sdk and a2a-protocol-slimrpc

| # | Sev | Finding | Evidence |
|---|---|---|---|
| K1 | Medium | `default-features = false` on the sdk does not remove TLS. The sdk's client and server dependencies don't set it, so rustls still comes in, and the manifest comment says otherwise. | VALIDATED (`cargo tree`) [re-checked manifest] |
| K2 | Medium | The prelude lacks what a server or coordinator needs: `Server`/`ServeConfig`, `FailureClass`, `CurrentTrace`, `TracePropagationInterceptor`, the caching resolver and `ErrorCode`. The shipped coordinator examples depend on 7 crates, not the sdk alone. | VALIDATED (compile) |
| K3 | Low | The sdk README feature table omits `auth-jwt` and misattributes `tls-rustls`/`grpc-tls`. `conformance` and a bare `proto` are not forwarded. The crate root has no server+client example, and the macro docs use `a2a_protocol_server::` paths. | VALIDATED |
| K4 | Low | slimrpc: the docs say "no change to any of those crates" was needed, which the README contradicts. It names a nonexistent `A2aClientBuilder`. It inherits T6 (INTERNAL on unconvertible tasks). It builds, and 57 tests pass. | VALIDATED |

## 6. Why these got past review and tests

Each of these gaps is tied to at least one defect that escaped:

1. **Written claims are never checked against code.** Crate READMEs (the
   crates.io pages) are not compiled; 130 of the book's 206 Rust blocks are
   `ignore`; Cargo feature docs and defaults tables are unchecked. This let
   through O1, O3, O10, C5, T8, S9 and S12.
2. **The observability check only looks at one side.**
   `check_otel_metrics_coverage.py` confirms the exporter overrides every
   callback. Nothing confirms that a real server run produces each
   advertised instrument, or any span tree. This let through O1, O2, O5, O6,
   O7 and O10.
3. **Default builds have no `tracing`, and nothing checks that failures
   surface somewhere.** This let through O13 and skipped webhooks.
4. **No scenario tests with several actors or replicas, and none written
   the way an adopter would use the SDK.** No test builds a coordinator that
   delegates. This let through S1, S3, S4, S5, C4 and last month's
   context-lockout and tenant fixes.
5. **Cross-SDK interop covers one direction and one role only.** CI runs
   a2a-go as a *server* driven by the in-repo TCK, which deliberately does
   not use `a2a-protocol-client` (`tck/Cargo.toml:19-23`). That contradicts
   `docs/official-tck-findings.md:11-15`. CI never runs a Go client, a gRPC
   leg against Go, a card with security requirements, or push delivery. This
   let through T1, S2, S7, C8 and C9.
6. **Nothing tests hostile or stalled peers.** No stub server stalls, cuts
   off or mis-frames a stream. This let through C1, C2, C6 and C11.
7. **Signing has no external test vectors.** RFC 8785 Appendix B is not in
   the tests. This let through T2, T3 and T4.
8. **New parsers of peer input aren't required to have fuzz targets.**
   `check_fuzz_matrix.py` only checks that existing targets run. The
   traceparent panic shipped in 0.13.0 this way; JWT, REST query and
   X-Forwarded-For are still unfuzzed.
9. **Release policy isn't checked by machine.** 0.12.0 (09-10) and 0.13.0
   (09-20) were both breaking, and `STABILITY.md` allows one breaking minor
   release per month. The `PurgeReport` rename skipped deprecation.

**Feature matrix: no defect.** Each feature of every crate compiles alone,
and CI's `cargo hack --each-feature` already covers that.

## 7. Proposed order of work (not started; awaiting a decision)

Each phase is complete and verified before the next starts, and every fix
ships with a regression test.

1. **Correctness and interop that the Go app can hit today:** T1, S2, C1,
   C2 (idle timeout and missing-terminal detection), C3, C4, C8, C9, S7, C10,
   S1, and S3 (confirm with a two-replica test first). Add gates 4–6 in the
   same change: a Go client and a gRPC leg in CI, stall/cut-off stub tests,
   and a coordinator end-to-end test against a2a-go.
2. **Real observability:** server spans per RPC, executor, store, push and
   stream, with semconv attributes. Parent them on the extracted remote
   context via `tracing-opentelemetry`, and propagate the *real* span id.
   Also: `.instrument` on every spawn; client spans and metrics; automatic
   propagation; seconds buckets and semconv names; stream metrics; rejected-
   request metrics; one `init_telemetry()` covering the `OTEL_*` variables,
   traces, metrics and logs, with a flush guard wired into graceful
   shutdown; and health checks on all dispatchers. The gate is an
   `InMemorySpanExporter` + `ManualReader` end-to-end test that asserts the
   span tree across a JSON-RPC, REST and gRPC hop and every catalogued
   instrument.
3. **Coordinator developer experience:** a delegation helper (forward a
   downstream stream, rewrite ids, propagate cancel and trace), an
   `agent_executor!` form that works with state, `From<ClientError>`, a
   progress message on `status`, `EventStream: Stream`, a `Clone` client,
   prelude additions, and a book chapter on delegation.
4. **Signing and types hardening:** T2–T7 and K1.
5. **Truth in the docs:** compile the crate READMEs as doctests, generate
   the defaults tables, fix every claim listed above, and correct
   `docs/official-tck-findings.md`.

Phases 1 and 2 include breaking changes (the `ClientError` conversion is
additive; the metric renames, span-id semantics and the client timeout
split are not).
