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
**Found after phase 1, on `claude/determined-galileo-rywiyj`:**

- **N1 — `Server::serve_with_shutdown` ignored its signal at the connection
  ceiling** (Medium). It waited for a connection permit before it looked at
  the signal, so with `max_connections` set and every slot held by a stream
  that only shutdown could end, shutdown never began. VALIDATED:
  `shutdown_is_seen_at_the_connection_ceiling` in
  `tests/graceful_shutdown_tasks.rs` timed out after 20 s on the unfixed
  server with the only slot held. **[Fixed: `50753ef1`]**
- **N2 — an OAuth2 token endpoint's 429 or 5xx is classed permanent**
  (Low). `token_error` (`token_provider.rs`) returns `ClientError::Transport`
  for every non-2xx answer, so a busy identity provider fails a task as
  `Internal`, the same defect OW7 fixed for a refused connection. Not fixed:
  `UnexpectedStatus` would make a token-endpoint 401 read as the agent's, so
  the variant needs a decision. VALIDATED by reading the code.
- **N3 — the slimrpc binding maps `UNAUTHENTICATED` and `PERMISSION_DENIED`
  to `InternalError`**, not to `InvalidParams` as C16 says for it
  (`bindings/a2a-protocol-slimrpc/src/error.rs`, `rpc_code_to_error_code`'s
  wildcard). The consequence is the same: the 401 hook never fires.
  VALIDATED by the binding's unit test before its fix: `expected a 401, got
  Protocol(A2aError { code: InternalError, … })`. **[Fixed with OW5:
  `2b1bb80a`]**
- **N4 — CI does not pin cargo-mutants.** `mutants.yml` runs `cargo install
  cargo-mutants cargo-nextest --locked` with no version, so the "27.1.0" the
  documents cite is whatever was newest when they were written. It is also
  the newest today (`cargo search`, 2026-09-23), so nothing has drifted yet.
  **[Fixed on this branch: `scripts/install_cargo_mutants.sh` pins
  cargo-mutants 27.1.0 by the crates.io checksum and cargo-nextest 0.9.146,
  and both `mutants.yml` jobs install through it]**
- **N5 — the `FATAL: role "root" does not exist` lines in every PostgreSQL
  job are the service health check**, not the tests. `--health-cmd
  pg_isready` runs as the container's `root` with no `-U`, and each probe
  logs one FATAL line and still exits 0 ("accepting connections") —
  VALIDATED: three bare runs against PostgreSQL 16.13 added three lines,
  a run with `-U postgres` added none. Harmless to correctness, but at one
  line every 5 s it is what fills the job-log window the GitHub API returns,
  which is why the mutation shards' logs could not be read that way.
  **[Fixed: `d73cf871`]**
- **N6 — nightly cargo reports dead manifest entries** (Low, hygiene).
  `cargo +nightly clippy` (1.100, 2026-09-22) warns: workspace dependency
  `tonic-build` unused; `workspace.package` fields `documentation`,
  `keywords`, `categories` inherited by no crate (each crate sets its own,
  and crates.io shows them — checked for `a2a-protocol-types` and
  `a2a-protocol-server` 0.13.0); `criterion` a normal dependency of
  `a2a-benchmarks`; `a2a-protocol-sdk` unused by `a2a-book-tests` (likely a
  false positive: that crate compiles markdown). Warnings, not errors; the
  nightly job passes.
- **N7 — 0.13.0 shipped nineteen changes its notes do not mention**
  (Medium, release process; escape class 9 in action). The `v0.13.0` tag is
  on `391f0df`, the merge of #138, not on the release preparation
  (`707092f8`) that wrote the 0.13.0 section. Both published crates say they
  were built from `391f0df` (`.cargo_vcs_info.json`; the server `.crate` is
  SHA-256 `02f16ab4…42f389`), and they contain #138's work — for example
  `with_inbound_trace_policy`, `idempotency_key_max_age` and the
  `#[non_exhaustive]` marking of `RetentionPolicy` and `PurgeReport`, a
  breaking change. At that commit the CHANGELOG listed all nineteen of #138's
  entries (1 breaking, 9 added, 9 fixed) under `[Unreleased]`, and the GitHub
  release notes, extracted from the 0.13.0 section, contain none of them
  (checked by searching the whole 54,863-character body). Found because
  `cargo semver-checks` reported nothing to do for a tree whose
  `[Unreleased]` claimed a breaking change. **[Corrected in CHANGELOG and
  `STABILITY.md` §1. The process gap is closed by four `release.yml` checks
  (N10), each proven able to fail by `prove_workflow_gates_fail.py`.]**
- **N8 — the mutation gate scored uncompilable feature-gated mutants as
  caught** (Medium, gate). `mutants.yml` passed `--all-features` after `--`,
  so each mutant was built without features and, if it did not compile with
  them, failed in the test step — which cargo-mutants counts as caught.
  VALIDATED: one diff's 14 "caught" were 6 caught and 8 unviable once the flag
  moved to `cargo mutants` itself. Missed counts were never affected; caught
  counts and scores were. **[Fixed: `587418fa`]**
- **N9 — cargo-mutants never generates `Ok(...)` for a `*Result` alias**
  (Medium, gate). It recognises a `Result` only when the type's last path
  segment is spelled `Result` (cargo-mutants 27.1.0, `src/fnvalue.rs:77`), so
  for `-> ClientResult<T>` and `-> A2aResult<T>` it emits `ClientResult::new()`
  and similar, which never compile. VALIDATED with `cargo mutants --list`:
  1,000 such mutants over 184 functions (types 40 over 7, client 505 over 82,
  server 455 over 95), none of them viable — so "replace this function's body
  with `Ok(default)`", the mutation that shows a function's effects are
  tested at all, has never been run against those 184 functions. Not fixed:
  the in-repo fix is spelling those return types through an alias named
  `Result` (types are unchanged, so it is not an API change), which will
  surface mutants no run has ever graded; measure how many survive before
  deciding.
  **Measured 2026-09-23** on `main` with the return types rewritten to an
  alias named `Result` (a scratch tree, not committed), `--all-features`
  given to cargo-mutants, CI's test filter and live PostgreSQL: 274 mutants
  over the 184 functions. types 11 (9 caught, 2 unviable, 0 missed); client
  144 (34 caught, 107 unviable, 3 missed); server 119 (84 caught, 33
  unviable, 2 missed); 0 timeouts. The five survivors, each now resolved on
  this branch and each checked by applying it by hand:
  - client `GrpcTransport::to_json` → `Ok(null)`: no client-crate test made a
    successful gRPC call (the success path was covered from the server and
    SDK crates, whose tests a client mutant never runs). Killed by
    `tests/grpc_unary_success_tests.rs`.
  - client `WebSocketTransport::check_open` → `Ok(())`: killed by
    `a_call_on_a_dropped_websocket_is_refused_as_final` (see N18).
  - client `check_endpoint_reachable` → `Ok(())`: equivalent under
    `--all-features`, where the function's body *is* `Ok(())`; its no-TLS
    branch had no test and now has one, which the CI
    `--no-default-features` job runs.
  - server `PostgresTaskStore::push_artifact` → `Ok(None)` and `Ok(Some(0))`:
    both make `save_artifact_delta` fall back to `save`, which stores the
    same bytes and moves the task to the front of `list`. Only the
    appended-parts delta was checked for list order; killed by
    `artifact_push_preserves_list_position`.

  The 142 unviable are mostly `Ok(Default::default())` for a type with no
  `Default`, which no tool can build; 85 functions had no other body
  replacement. Each was reviewed by reading (2026-09-23): for 79 a test in
  the function's own crate asserts something a trivial body would break —
  the crate matters, because cargo-mutants runs only the mutated crate's
  tests. The other 6 now have one: `A2aClient::from_card` (its test asserted
  only a timeout a default client also has), `OAuth2ClientCredentials::
  from_oidc_issuer` (untested anywhere), `GrpcTransport::connect` and
  `GrpcTransport::parse_params` (covered only from the SDK crate; the client
  crate's gRPC stub now answers only the id it is asked for), and the timeout
  arguments of `RestTransport::with_timeout` and
  `WebSocketTransport::connect_with_timeout` (no test bounded the elapsed
  time, so a dropped value fell back to the 30 s default unnoticed; the REST
  test was run against exactly that and failed at 30.0 s). The review also
  noted that `connection_timeout` in the JSON-RPC and REST transports is still
  checked by no test that measures it. **[Resolved on this branch; the
  maintainer chose a patched cargo-mutants in CI, which makes these
  mutants part of every run.]**

- **N10 — no release was ever checked to be its own release preparation,
  and eight of seventeen tags were not** (Medium, release
  process; escape class 9). `scripts/check_release_tree.py` asks, for a tag,
  whether any file packaged into the four crates changed after the release's
  own CHANGELOG section was last edited. VALIDATED by its `history` mode on
  2026-09-23: `prep` fails `v0.2.0`, `v0.6.0`, `v0.7.0`, `v0.8.0`, `v0.9.0`,
  `v0.10.0`, `v0.12.0` and `v0.13.0`; `unreleased` fails `v0.3.0` and
  `v0.13.0` (19 entries — N7's count, re-derived from the tagged file);
  `cadence` fails `v0.13.0` (a second breaking minor in September 2026); its
  `vcs` mode confirms from the crates.io downloads that all four 0.13.0
  crates were built from `391f0df` (server SHA-256 `02f16ab4…42f389`, as N7
  records). What the late changes were varies: `v0.10.0`'s is one lint fix
  across twelve files, `v0.12.0`'s are mostly `#[cfg(test)]` additions with
  some library lines — whether any of them changed behaviour was not
  established, and it does not need to be for the gate to be right, since
  the notes were not re-read against them. **[Gated: the four checks run in
  `release.yml`; `RELEASING.md` says what they ask of the process.]**

- **N12 — signing canonicalized integers beyond 2^53 with all their digits**
  (Medium, signing interop; not in the tables). RFC 8785 §3.2.2.3 makes
  every JSON number an IEEE 754 double, so `9007199254740993` canonicalizes
  as `9007199254740992`, as V8's `JSON.stringify(JSON.parse(…))` renders it;
  `canonicalize` wrote the literal, so the canonical bytes of any card whose
  metadata carried such an integer disagreed with every conforming
  implementation. VALIDATED by `rfc8785_integers_are_doubles` failing on
  unchanged code (`left: "9007199254740993"`, `right: "9007199254740992"`).
  A unit test, `canonicalize_integers_exact`, had pinned the deviation by
  asserting `u64::MAX`'s exact digits. **[Fixed with T2.]**
- **N13 — a peer that goes away mid-call read differently on each binding**
  (Medium, client; found by E6's scripted peer). The same cut — the stream or
  connection ending before a final event — was a retryable `Http` error over
  JSON-RPC and HTTP+JSON, a non-retryable `Transport` over WebSocket and a
  non-retryable `Protocol(InternalError)` over gRPC. A WebSocket handshake
  refused with 401 was `Transport` too, so `BearerAuthInterceptor` never
  dropped the token (the gap OW5 closed for gRPC). VALIDATED:
  `tests/scripted_peer_tests.rs` failed three of five tests, five wrong
  outcomes, on the unfixed transports — e.g. `WebSocket: expected a
  retryable error, got Some(Err(Transport("WebSocket connection closed")))`
  and `Grpc: … Protocol(A2aError { code: InternalError, message: "protocol
  error: missing grpc-status trailer, …" })`. **[Fixed on this branch; see
  the CHANGELOG's first breaking entry.]**
- **N14 — under parallel load a sequential post to a private channel was
  refused as "already being processed"** (severity unknown; CONJECTURED).
  `swarm_scale cost::a_channel_gets_slower_as_it_ages`, run by
  `--run-ignored all` alongside the rest of the server suite on 4 cores,
  failed with `task … is already being processed; wait for it to reach
  input-required or a terminal state before sending again` on a post the
  test makes only after the previous one returned. OW4 attributes these
  experiments' parallel failures to contention; a refusal of a *sequential*
  post is a different claim — that a blocking send can answer before the
  task is released for the next — and is not established either way. CI
  excludes `binary(swarm_scale)` from mutation runs, so nothing runs it under
  load. Next step: reproduce with the test alone and a CPU hog.
- **N11 — cargo-mutants makes no viable body replacement for a function
  returning `Pin<Box<dyn Future<…>>>`** (Medium, gate; wider than N9). It
  offers `Pin::new()`, `Pin::from_iter(…)`, `Pin::new(Box::new(Default::default()))`
  and `Pin::from(Box::new(Default::default()))`, none of which compiles, so
  "replace the body" has never been graded for 162 functions: 144 in the
  server (every `TaskStore`, `PushConfigStore`, `ServerInterceptor` and
  event-queue implementation) and 18 in the client. VALIDATED with
  `cargo mutants --list --all-features` on `main` (648 such mutants); the
  cause is `type_replacements` in cargo-mutants 27.1.0's `src/fnvalue.rs`,
  which has no case for `Pin` or `dyn Future`. Statement-level mutants inside
  those bodies are still generated and graded. Spelling a return type as
  `Result` does not reach these: the alias sits inside `Output = …`.
  **[Fixed on this branch: `scripts/install_cargo_mutants.sh` builds
  cargo-mutants 27.1.0 from its checksum-pinned crate with
  `scripts/cargo-mutants/27.1.0-result-aliases-and-boxed-futures.patch`, which
  replaces a boxed future's body with one yielding each replacement of its
  output and skips a replacement equal to the body; `mutants.yml` runs it.
  Measured with the patch on `main`'s tree plus this branch's earlier commits
  (`--re 'Box::pin\(async move'`, `--all-features`, CI's test filter, live
  PostgreSQL): client 23 mutants, 18 caught, 5 unviable, 0 missed; server 229,
  142 caught, 50 unviable, 37 missed. Of the 37, 10 are equivalent: four
  interceptors' `after` and `on_shutdown`, whose bodies already are the
  replacement; two `close`s whose effect is nothing; and three trait defaults.
  The patched build no longer generates 8 of them. Two remain, and each is
  expected to survive: `CancelOnFirstWrite::close`, which forwards to a no-op,
  and `TaskStore::earliest_event_seq`'s `Ok(None)`, whose body opens with `let
  _ = task_id;`. 8 were the push-config `count`s, which this branch's
  `PushConfigStore` count tests, committed while the measurement ran, already
  kill. The other 19 were untested: `TaskStore`'s defaults for `append_event`,
  `release_idempotency_key` and `earliest_event_seq`, and
  `release_idempotency_key` or `earliest_event_seq` on the in-memory, SQLite
  and PostgreSQL tenant stores and the PostgreSQL store. Each now has a test.
  Re-run with the patched build over the 41 mutants of those functions
  (`swarm_scale` left out of the filter: it failed the unmutated baseline on
  this host, on `main` too): 40 caught, 1 missed — the equivalent
  `Ok(None)`.]**
- **N15 — the book taught code that does not compile, and prose the code
  contradicts** (Medium, docs; escape class 1). Found by compiling the 127
  `ignore`d blocks. Five could not compile as shown; each was confirmed by
  compiling the original with only its missing context added, under nightly
  rustdoc with the error code pinned (a wrong-code control fails): E0382 (a
  value sent twice, `client/builder.md`), E0004 (`match` on the
  `#[non_exhaustive]` `SendMessageResponse`, `client/sending-messages.md`),
  E0277 (`Arc<RateLimitInterceptor>` as a `ServerInterceptor`,
  `building-agents/interceptors.md`), E0614 (`&*writer`,
  `deployment/testing.md`), E0283 (`ClientBuilder::new("…".into())`).
  Prose the code contradicts, each checked against the source: `cancel`'s
  default refuses (it has cancelled since 0.7; `executor.rs:96`); the
  builder picks the transport from the URL (it defaults to JSON-RPC;
  `transport_factory.rs`); CORS is on by default (off until `with_cors`);
  `sqlx::PgConnection` is not `Send + Sync` (a `compile_fail` block asserting
  it compiled). Two examples taught weaker security than the SDK practises: a
  body-size check on `size_hint()` alone, which the REST dispatcher's own
  comment calls a memory-amplification DoS, and a token check by `HashSet`
  lookup. VALIDATED as above. **[Fixed on this branch: every block but
  `slimrpc`'s three compiles; `check_book_code.sh` holds the rest at zero.]**
- **N16 — types a caller builds with no constructor, and a public field of a
  type the SDK does not export** (Low, API; next to K2). `TaskQueryParams`,
  which `get_task` takes, and `Task`, which a custom store or a test
  builds, have no `new`; both need a full struct literal, so adding a field
  breaks every caller (neither is `#[non_exhaustive]`, so that is also the
  only way). `RequestContext::cancellation_token` is a
  `tokio_util::sync::CancellationToken`, which no crate here re-exports, so an
  executor that stores or creates one needs its own `tokio-util` dependency
  at a compatible version. VALIDATED while converting the book: the pages
  now build both with literals, and `book-tests` depends on `tokio-util` for
  the third. **[Fixed on `claude/peaceful-ptolemy-noka5b`: `Task::new`,
  `TaskQueryParams::new` with `with_history_length` and `with_tenant`, and
  `a2a_protocol_server::CancellationToken` (so `a2a_protocol_sdk::server::`
  too); `book-tests` dropped its `tokio-util` dependency. Neither struct was
  made `#[non_exhaustive]`, which would break every literal now; that is a
  choice for a breaking release]**
- **N17 — `deny.toml` allows a licence no dependency carries** (Low,
  hygiene). `cargo deny check` on `main` passes with a warning that the
  `Unicode-DFS-2016` allowance matches no crate. An allowance with nothing
  behind it widens the policy for a future dependency without anyone
  deciding to. VALIDATED (`cargo deny check`, exit 0 with the warning). The
  slimrpc binding's own policy had the same shape: `CDLA-Permissive-2.0`,
  which the root tree needs for `webpki-roots`, matched nothing there.
  **[Fixed on this branch: both allowances removed, and both policies set
  `unused-allowed-license = "deny"`. Probed: re-adding `Unicode-DFS-2016` to
  the root policy fails `cargo deny check licenses` with
  `license-not-encountered`, exit 4. The deny jobs are marketplace actions
  `prove_gates_fail.sh` cannot inject into (`ci_gate_audit.sh` says why), so
  the probe is the evidence.]**
- **N18 — `WebSocketTransport` never reconnects** (Low, client; found
  while fixing N13). Once its socket drops, `closed` stays set and every
  later call fails at once with a non-retryable `Transport("WebSocket
  connection closed")`. N13 made the drop itself retryable, as it is on every
  other binding, but on this one only a new transport can act on that: a
  caller retrying on the same client gets one futile attempt. VALIDATED with
  a scripted peer that cuts the connection: the in-flight `send_message`
  failed `HttpClient("WebSocket connection closed")` with one connection
  made, and the next call on the same client failed `Transport(…)`. (The
  retry policy did not re-send the first call, since `SendMessage` is not
  idempotent unless the peer honours idempotency keys.) The fix is a lazy
  reconnect from the stored URL and config, bounded like the first connect;
  E3's circuit breaker would sit in front of it. Until then
  `a_call_on_a_dropped_websocket_is_refused_as_final` pins the refusal as
  non-retryable, so a retry loop stops instead of spinning. **[Fixed on
  `claude/peaceful-ptolemy-noka5b`: the transport keeps its endpoint and
  configuration and replaces a dead connection on the next call — one
  reconnect for concurrent callers, bounded by `connect_timeout`; requests
  and streams hold the connection they started on, so a reconnect cannot
  silence one (N22). `a_transport_reconnects_after_its_server_restarts`
  (server stopped and restarted on the same port) and
  `a_call_after_a_dropped_websocket_reconnects` (which replaces the test
  that pinned the refusal) both fail against the previous transport]**
- **N19 — the WebSocket dispatcher's documentation claims the v0.3 method
  aliases** (Low, docs; found while adding spans to it). The
  `process_ws_message` doc comment and `book/src/building-agents/dispatchers.md`
  said it routes "the v0.3 `method/verb` aliases"; of those it routes only
  `message/stream`, and `ws_legacy_method_names_rejected` asserts that
  `message/send`, `tasks/list` and `tasks/get` are refused with `-32601`.
  VALIDATED by reading the dispatch match and the test. **[Fixed on this
  branch: both documents now say what the code does; the behaviour is
  unchanged]**
- **N20 — the two HTTP+JSON dispatchers answer the same error with different
  statuses** (Medium, server wire; found while mapping each binding's status
  for `error.type`). The axum adapter answered `ServerError::Overloaded` with
  `503` and `PayloadTooLarge` with `413`; `RestDispatcher` sent both through
  `to_a2a_error()` and answered `500` and `400` — though its own body-limit
  check answers `413`. A client whose retry policy keys on `503` retried an
  overloaded axum server and gave up on an overloaded `RestDispatcher`.
  VALIDATED by reading both mappings; the existing test
  `server_error_payload_too_large_maps_to_400` pinned the `400`. **[Fixed on
  this branch: one `ServerError::http_status` serves both, the pinned test
  now expects `413`, and `server_error_overloaded_maps_to_503` is new. The
  body's `status` field still says `INTERNAL` for an overload, from the A2A
  code; an AIP-193 `UNAVAILABLE` there is left for a wire change of its own]**
- **N21 — a blocking send answers `input-required` before the task stops being
  in flight** (Medium, server behaviour; found when a mutation baseline
  failed). `collect_events` returns as soon as the task reaches an interrupted
  or terminal state (`sync_collector.rs`, the `break` after
  `is_interrupted()`), without waiting for the executor's spawned future,
  which is what removes the task's cancellation token. Admission refuses a
  send while that token is live (`reject_in_flight_send`). So a client that
  answers an `input-required` at once can be refused with "task … is already
  being processed; wait for it to reach input-required or a terminal state" —
  the state its previous response reported. VALIDATED by
  `swarm_scale::cost::a_channel_gets_slower_as_it_ages`, which posts
  sequentially to one task: it passes 10 of 10 on an idle machine and fails 5
  of 5 on `main` (`638ff9a0`) under four busy loops on four cores, with that
  error; this branch's trees fail identically, 20 of 20 each. The harness's
  `settle` loop absorbs the first occurrence and nothing absorbs the rest. The
  mutation baseline runs `swarm_scale`, and on this 4-core host on 2026-09-24
  the server suite as that baseline runs it failed 3 of 3 times on `main`: two
  to four `swarm_scale::cost` and `fan_in` tests on this refusal (`fan_in`
  reports a single agent's own posts refused 40% of the time), and
  `fan_out::every_tail_sees_every_post` timing out at 45 s. The same command
  had passed on this host hours earlier, so how often it bites depends on the
  host; a busy CI runner may see it. The fix is a design choice — have the
  blocking response wait, bounded, for the executor to return, or let
  admission accept a continuation of a task whose recorded state is already
  interrupted — and is left for the maintainer. **[Fixed on
  `claude/peaceful-ptolemy-noka5b`: the maintainer chose the admission side.
  The executor's writer marks its turn *parked* when the latest state it
  writes is `input-required` or `auth-required` — before the event reaches
  the queue, so no reader sees the state first — and admission waits, up to
  `executor_drain_timeout`, for a parked turn's executor to finish rather
  than refusing. A send into a task whose executor has not parked it is
  still refused at once, and past the bound the refusal is unchanged, so two
  executors never run for one task. `an_immediate_answer_to_input_required_is_admitted`
  and `a_streaming_answer_to_input_required_is_admitted` fail on `main`
  (`0b7e87c`) with the refusal above and pass with the fix. Their executor
  lingers 300 ms after parking, which reproduces the race on an idle host;
  the reproduction above needed four busy loops]**
- **N22 — a WebSocket stream goes silent when its client is dropped**
  (Medium, client behaviour; found when `prove_gates_fail.sh` graded
  `cargo test --workspace --all-features` PRE-BROKEN on this branch). Dropping
  a `WebSocketTransport` aborts its reader task (`impl Drop for Inner`), but
  the `EventStream` it returned holds a `PendingGuard`, whose map holds the
  stream's sender. With the reader gone and the sender alive, the stream
  receives nothing more and never ends; only the idle bound (5 min by
  default) reports a `Timeout`. On the HTTP and gRPC bindings a stream
  outlives its client. `scripted_peer_tests`, which drops the client after
  the first event, failed on WebSocket when the abort beat the reader to the
  peer's close: VALIDATED by timestamps on the reader's polls (polled
  `Pending` 0.02 ms before the peer closed, never polled again) and by
  failure counts under eight busy loops on four cores — 8 of 640 runs before
  the fix, 0 of 300 after. The regression test
  `a_stream_outlives_the_transport_that_opened_it` fails on `main`
  (`638ff9a0`) and passes here. **[Fixed: the stream holds the connection
  too]**
- **N23 — the agent card's poll watcher can miss the first change** (Low,
  server behaviour; found when `Test (stable, macos-latest)` failed
  `poll_watcher_detects_change` on this branch, in code it does not touch).
  `spawn_poll_watcher` read the file's baseline mtime inside the spawned
  task, through `spawn_blocking`. A rewrite that landed before that read
  became the baseline, so it was never seen as a change and never loaded;
  the test's 10 s deadline could not help, because the watcher was not
  slow but blind. VALIDATED: a test that rewrites the file before the
  watcher's task first runs fails on the old code every time and passes
  with the fix. **[Fixed: the baseline is read in `spawn_poll_watcher`]**
- **N24 — the published manifests admitted dependency versions under
  RustSec advisories** (Medium, supply chain; reported by an adopter on
  0.12.1 as "`cargo update -p a2a-protocol-client` does not advance rustls").
  `cargo deny` reads this repository's lockfile; a consumer's cargo keeps
  whatever its own lockfile holds as long as our requirement admits it, so
  a fix that moved only our lockfile never reached them. On `main`
  (`0b7e87c`) the normal and build dependencies of the four published
  crates admitted affected, published versions under seven advisories:
  `rustls` `>=0.23, <0.24` (RUSTSEC-2024-0336, RUSTSEC-2024-0399,
  RUSTSEC-2026-0285), `ring` `0.17` (RUSTSEC-2025-0009), `bytes` `1`
  (RUSTSEC-2026-0007), `sqlx` `0.8` (RUSTSEC-2024-0363), `time` `0.3`
  (RUSTSEC-2026-0009) and `tokio` `>=1.38, <2` (RUSTSEC-2025-0023,
  unsound) — thirteen requirement/advisory pairs. VALIDATED by
  `scripts/check_advisory_floors.py` against the RustSec database at
  `66105a54` and the crates.io version lists, 2026-09-24: exit 1 with those
  thirteen on `main`, exit 0 with the new floors. **[Fixed: floors raised to
  the patched versions, each within the 1.88 MSRV; both lockfiles already
  satisfied them. The script is a CI gate, and `prove_gates_fail.sh`
  injects the 0.13.0 rustls requirement back]**
- **N25 — an idle `SubscribeToTask` over SSE never ended** (Medium, server
  behaviour; found by the drop-path audit, then reproduced). The bound
  `subscribe_max_idle` (300 s) was measured inside the future the reattach
  hook returns, and that future lived only inside one `read()` call. The SSE
  writer races `read()` against its keep-alive timer (30 s) and drops the
  loser, so every keep-alive discarded the hook's future and the next
  `read()` started the bound again. A subscription to a task parked at
  `input-required` held its connection and polled the store every
  `subscribe_reattach_interval` for as long as the client stayed. VALIDATED:
  `the_idle_bound_survives_a_read_cancelled_by_a_keep_alive`, which cancels
  `read()` every 30 ms against a 100 ms bound, fails on `main` at its 5 s
  limit and passes with the fix; `resubscribe_gives_up_after_the_idle_bound`
  never cancelled a read, which is why it passed. **[Fixed: the reader keeps
  the pending hook future, so `read()` is cancel-safe and the bound runs
  from the first close. Held in a `Mutex` to keep `InMemoryQueueReader`
  `Sync`]**
- **N26 — a send dropped mid-commit wedged its task** (High, server
  behaviour; found by the drop-path audit, then reproduced). hyper drops a
  request's future when its client goes away. Between claiming an
  idempotency key and spawning the executor the send path holds the queue
  lease, the cancellation token and the key, and only an `Err` released
  them. A drop — a client timing out during a slow store write — released
  nothing: every later continuation of that task was refused as "already
  being processed" for the life of the process, the queue counted against
  `max_concurrent_queues` permanently, and a keyed retry waited on a task
  that would never exist. VALIDATED: `a_send_dropped_mid_commit_releases_what_it_took`
  holds the history write open, aborts the send, and fails on `main` with
  "a dropped send left its cancellation token registered".
  **[Fixed: `CommitGuard` releases exactly what the send took — the lease if
  it was taken, the token only if the entry is still this send's turn, the
  key — when the commit future is dropped, and is disarmed when the commit
  returns. `a_send_dropped_while_waiting_leaves_the_running_turn_cancelable`
  fails when the guard removes any token under the id, and
  `a_send_dropped_mid_commit_releases_its_idempotency_key` fails with the
  guard disabled. Residual: if the drop lands after the task row is
  written — possible only while an inline push config is being registered,
  the one await between that write and the spawn — the row stays as written
  with no executor]**
- **N27 — a blocking send whose client went away left its task `working`
  for good** (High, server behaviour; found by the drop-path audit, then
  reproduced). A blocking `SendMessage` has no background processor: the
  collector running in the request's future is the only thing persisting
  the executor's events. hyper drops that future when the client goes away
  — a client timeout on a slow model call is enough — after which the
  executor's writes fail for want of a reader, its own failure report fails
  the same way, and the stored task keeps the last state persisted before
  the drop. No push notification for the rest of the task is sent either.
  VALIDATED: `a_blocking_send_dropped_mid_work_still_records_the_outcome`
  drops a blocking send 50 ms into a 200 ms task and fails on `main` with
  the store at `working`. **[Fixed: the collection runs on a task of its
  own on the handler's background tracker, holding owned clones of the
  seven handler fields it uses (`SyncCollector`), in the call's span and
  tenant; the request awaits it, so a dropped request stops only the
  waiting]**
- **N28 — an interceptor's failing `after` hook orphaned the task it ran
  after** (Medium, server behaviour; found by the drop-path audit, then
  reproduced). The send path ran `run_after` once the executor was spawned
  but before the response path attached anything to persist its events; an
  `after` error — or a client dropping the request while `after` awaited —
  dropped the committed send there. The caller got an error for a task
  that ran, and the store kept it at `submitted`. VALIDATED:
  `a_failing_after_hook_does_not_orphan_the_running_task` fails on `main`
  with the store at `submitted`. Found beside it: `ServerInterceptor::after`
  was documented as "called even if the handler returned an error"; no
  method calls it on an error, and the book's interceptor chapter already
  said so. **[Fixed: `after` runs once the send's response exists; the
  trait documentation now says what every method does. Whether `after`
  *should* also run on errors is a design question left open]**
- **N29 — one unread WebSocket stream stalled every call on its socket**
  (Medium, client behaviour; found by the drop-path audit, then
  reproduced). `WebSocketTransport` has one reader task per socket, and it
  awaited room in a stream's bounded channel (64 frames) before reading the
  next frame. A caller that opened a stream and then made any other call
  before reading it — `GetTask`, `CancelTask` — got no answer once the agent
  had sent more than 64 events: the answer sat unread behind them until the
  call timed out. The comment above the send said a stalled consumer
  "blocks only this send". VALIDATED:
  `an_unread_stream_does_not_stall_a_unary_call_on_the_same_socket` (300
  events, unread) times out its `ListTasks` after 10 s on `main`.
  **[Fixed: the reader never waits on a stream. A WebSocket multiplexes
  calls with no flow control, so an unread stream's frames are buffered or
  shed; the buffer stays 64, and what overflows it ends that stream with a
  `stream_lagged` error — the server's own treatment of a lagging reader,
  and one the caller can resubscribe from. Trade: a consumer that reads, but
  more slowly than the agent writes, used to be backpressured without loss
  (at the cost of every other call on the socket) and is now told it
  lagged]**
- **N30 — a WebSocket peer that stopped reading mid-stream was never
  closed, and kept its connection slot** (Medium, server behaviour; found
  by the drop-path audit, then reproduced). `with_idle_timeout` documents
  that it closes "a client that has stopped reading its socket". Against
  one being streamed to, three waits defeated it: the stream's send blocked
  on the full socket while holding the sink lock; the keepalive waited for
  that lock to send its ping, so the idle check never ran again; and once
  the read loop ended, the closing handshake took the same lock. VALIDATED:
  `a_peer_that_stops_reading_mid_stream_is_closed_by_the_idle_bound`
  (`tests/websocket_slow_reader.rs`; `max_connections(1)`, a 1 s idle
  bound, a peer with a 4 KiB receive buffer and a 12 MB stream) fails on
  `main`: a second client is not served. **[Fixed: the ping never waits for
  the lock or past the budget; request tasks are cancelled when the peer is
  gone (closed, errored or idle — not on shutdown, which lets them finish);
  the closing handshake is bounded at 1 s. Each part was removed in turn
  and the test failed each time. Behaviour change: a request in flight when
  its peer's connection ends is dropped, as an HTTP request is when its
  client goes away — safe now that N26–N28 are fixed]**
- **N31 — a cancelled gRPC stream kept its subscription while the task was
  quiet** (Low, server behaviour; found by the drop-path audit, then
  reproduced). The task forwarding a queue reader into a gRPC response
  noticed the client going away only when its next send failed; on a task
  that emitted nothing more, the reader and its place on the task's queue
  stayed for as long as the task was quiet (bounded at `subscribe_max_idle`
  for a parked subscription only since N25). VALIDATED:
  `a_cancelled_stream_releases_its_reader_while_the_task_is_quiet` fails on
  `main`: a write after the stream was dropped still found a reader.
  **[Fixed: the forwarder also waits on the channel closing. On WebSocket
  the same wait is covered by N30's cancellation; the SSE writer notices at
  its next keep-alive, which is bounded]**
- **N32 — the card poll watcher could skip a fixed card after a failed
  parse** (Low, server behaviour; found by the drop-path audit, then
  reproduced). The watcher recorded a file's mtime whether or not the
  reload worked. On a filesystem with coarse timestamps (one to two
  seconds on HFS+, FAT, some NFS), a poll that caught a half-written card
  and a final write inside the same granule left the old card in place
  until the next edit — N23's family, one step later. VALIDATED:
  `a_card_that_failed_to_parse_is_retried_at_the_same_mtime` stamps both
  writes with one mtime and fails on `main` at its 10 s deadline. **[Fixed:
  the recorded mtime advances only on a successful reload, and a file that
  stays broken is logged once per mtime rather than at every poll]**
- **Examined and left, from the same audit** (CONJECTURED, not reproduced):
  a queue write dropped between persisting and broadcasting an event — only
  the executor timeout firing inside a terminal event's verdict wait can do
  it — leaves live subscribers one event short; the blocking path's push
  job reads webhook configs after the response, so a config deleted at
  once can still receive that send's events, which were raised while it
  was registered; the SLIMRPC binding's unicast bridge and multicast
  fan-out outlive a dropped consumer while the agent is quiet, as N31 did;
  `CleanupGuard`'s release is not on the shutdown tracker (unreachable
  under the release profile's `panic = "abort"`); a WebSocket client
  request registered in the instant after a connection drop waits its
  timeout instead of failing at once.

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

**[Fixed: `f7956d10`]** Reproduced first: 48 constructors against a fresh
database, 2 failed on round 0 (`pg_class_relname_nsp_index` — the `pg_class`
variant of the same race; a raw `psql` probe also produced it once in 40
alongside 28 `pg_type` failures); 20 consecutive passing runs after.

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

**[Fixed: `2b1bb80a`, gRPC transport and slimrpc binding]** `ErrorInfo`
reasons are consulted for `CANCELLED` too; carrying `ErrorInfo` metadata as
`data` is not done on any binding and stays out of scope.

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

**[Fixed: `e5ee1518`]** `serve_with_shutdown(listener, signal)` on both,
returning `ServeReport`; the book's manual recipe is gone. Mutation testing
grades less of it than the count suggests: of 30 mutants over the change, 20
are unviable because cargo-mutants' only mutants for the two
`serve_with_shutdown` bodies replace the whole function with
`Default::default()` for types that have none. Those bodies are covered by
behaviour, not by mutants — cancellation before return, `Canceled` on the
wire, drained and abandoned counts with one and two stubborn clients, the
gRPC port refusing peers once accepting stops. One survivor
(`OpenConnections::count -> 1`) was killed by making the stubborn case two
clients, checked by applying the mutant by hand.

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

**[Fixed: `af60cece`]** For OIDC discovery too. A 429 or 5xx answer from the
token endpoint is still permanent: N2.

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
| O1 | Critical | **[Fixed on this branch: every binding — JSON-RPC over HTTP and WebSocket, HTTP+JSON through `RestDispatcher` and the axum router, gRPC — runs each call in one `SERVER` span, and the executor's span carries `a2a.task.id` and `a2a.context.id`, which is what the book claims. The gate `crates/a2a-protocol-sdk/tests/observability_e2e/` asserts the span tree on JSON-RPC, HTTP+JSON and gRPC; the WebSocket and axum spans are exercised by tests that check their metrics, not their spans]** **No spans exist in any crate.** The book claims "Task and context identifiers are on the spans … events from inside an executor inherit that context"; this is false. | VALIDATED [re-checked]: a grep for `span!\|info_span\|#[instrument]\|.instrument(\|Span::current` over `crates/` returns 0 hits. `book/src/deployment/observability.md:266-268` |
| O2 | Critical | **[Fixed on this branch with the `otel` feature and a `tracing-opentelemetry` layer: the downstream `traceparent` names the recorded `SERVER` span; the gate asserts, per binding, that the id the executor sees was exported. Without a recording layer the id is still minted, as ADR 0013 option 3 decides — there is no recorded span to name]** **The server sends downstream a span id it never records.** `fresh_span_id` (`server/src/handler/helpers.rs:169`) creates a child span id and stores it in `CallContext`, and that id is what goes downstream. No span with that id is ever exported, so every Go agent's trace points at a parent the backend never sees. A user who adds `tracing-opentelemetry` cannot repair this. | VALIDATED: inbound `00f067aa0ba902b7` went downstream as `0508261a5e764cd6`. [re-checked the call sites] |
| O3 | High | `Cargo.toml` feature `otel` says "native OTLP export of traces and metrics". `opentelemetry-otlp` is built with only `metrics`, and there is no `TracerProvider`. The 0.13.0 sweep fixed three documents and missed this one, which docs.rs publishes. | VALIDATED [re-checked]: `server/Cargo.toml:54,109`, `otel/pipeline.rs:54` |
| O4 | High | **[Fixed on this branch: the four spawn sites run in `INTERNAL` child spans (`a2a.execute`, `a2a.process_events`, `a2a.deliver_push`, `a2a.sse`); the gate asserts every one has a recorded parent. Its first version found the SSE writer spawned after the call's span had closed, a root span per stream; fixed with `ServerSpan::run_with`, and probed by moving it back out]** The executor and background work run in `tokio::spawn` with no `.instrument(...)`. This happens at 4 sites: `execute.rs:106`, `background/mod.rs:62`, `sync_collector.rs:439`, `streaming/sse.rs:264`. A user's own outer span (e.g. an axum `TraceLayer`) is not the parent either. | CONJECTURED (code); the no-span result is VALIDATED |
| O5 | High | **[Fixed on this branch: `rpc.server.call.duration` with the conventions' buckets and attributes (verified upstream 2026-09-23, ADR 0013); `a2a.server.latency` takes the same buckets and is deprecated; the unit strings in `otel/mod.rs` now match what is emitted]** The latency histogram records seconds but keeps the SDK's millisecond-sized default buckets `[0,5,10,25,…]`, so everything under 5 s lands in one bucket. Names are not semconv: `a2a.server.latency` / `method`, where semconv has `rpc.server.duration` / `rpc.method` / `rpc.system`. The unit strings in the doc (`otel/mod.rs:65`) are wrong. | VALIDATED (ManualReader) |
| O6 | High | Streaming latency measures only stream setup. A 1.5 s stream recorded 0.0006 s. There is no stream-duration, events-per-stream or active-stream metric. | VALIDATED |
| O7 | High | If `OtelMetricsBuilder::build()` runs before the global MeterProvider is installed, every metric is silently a no-op for the life of the process. There is no warning. | VALIDATED |
| O8 | High | The client has no spans, no metrics hook and no retry counter. `CallInterceptor::after` is skipped when the transport errors (`client/src/methods/send_message.rs:91-100`), so an interceptor cannot time a call or count its errors. | VALIDATED |
| O9 | High | Outbound `traceparent` is opt-in twice: `TracePropagationInterceptor` **and** `CurrentTrace::scope(ctx.trace_context())` around each call. Neither is in the prelude, and the scope is not inherited across `tokio::spawn`. With the interceptor but no scope, nothing is sent. | VALIDATED |
| O10 | Medium | **[Fixed on this branch for `serve`, `serve_with_addr` and `Server::serve_with_shutdown` (`serve/connections.rs`), and the double count fixed (`pool_counters_count_each_connection_once`). The gRPC and WebSocket dispatchers' listeners still report nothing]** The `pool.{active,idle,created,closed}` metrics are advertised (README.md:77, observability.md:147-150) but `on_connection_pool_stats` has no production caller. If it were called, cumulative totals passed to `Counter::add` would double-count. | VALIDATED |
| O11 | Medium | **[Partly fixed on this branch: malformed JSON, an unknown method, an HTTP+JSON request refused on its body or parameters, and a call whose peer went away are now recorded by `rpc.server.call.duration`. Still open: executor failure or timeout (the call succeeds with a failed task — a task-outcome metric, E5), tenant-resolution failure, and the push delivery a config-store read error aborts]** Some failures produce no metric: malformed JSON, unknown method, executor failure or timeout, tenant-resolution failure (conjectured), and push delivery aborted by a config-store read error (`background/push_delivery/mod.rs:72-74`). There is no task-outcome metric. | VALIDATED except as marked |
| O12 | Medium | OTLP setup covers only part of the `OTEL_*` configuration. It is gRPC only and ignores `OTEL_EXPORTER_OTLP_PROTOCOL`. The `service_name` argument overrides `OTEL_SERVICE_NAME`, and `service.version` is never set. There is no log bridge and no Prometheus option. Graceful shutdown doesn't flush the meter provider, which loses up to 60 s of metrics. `tracing` is off by default in the server and the sdk. | CONJECTURED (code) |
| O13 | Medium | **[Fixed on this branch: `tracing` is a default feature of the client, server and SDK (maintainer's decision, ADR 0013), so a default build of any of them reports these paths to whatever subscriber is installed. Escape class 3's other half — nothing checks that a failure surfaces somewhere — is still open]** Many failure paths report only through `trace_*!`, which compiles to nothing without the non-default `tracing` feature. Examples: the WebSocket traceparent drop, which the book says "warns once per connection", and skipped webhooks. | VALIDATED |
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
| S8 | Medium | **[Fixed: `e5ee1518` — was open work OW6]** Graceful shutdown covers only JSON-RPC and REST. `GrpcDispatcher::serve` and `WebSocketDispatcher::serve` take no shutdown signal, and the gRPC background serve discards its error with `let _ =`. | VALIDATED (code) |
| S9 | Medium | **[Fixed: `3f6f7d3`]** README.md:60 says `shutdown()` reports a queue it had to force-destroy. The field is "always 0" (`handler/shutdown/mod.rs:30`), and every queue is destroyed unconditionally. | VALIDATED |
| S10 | Medium | `EventEmitter::status(state)` can't carry a progress message. `RequestContext.task_id` is a `TaskId` but `context_id` is a `String`. | VALIDATED (compile) |
| S11 | Medium | The README's one-line `serve()` is the unhardened path: no connection cap, no header or idle timeout, no shutdown. There is no top-level `max_concurrent_tasks` (per-tenant only). | CONJECTURED (code, but the crate's own docs agree) |
| S12 | Medium | **[Fixed on this branch: feature tables gated by `check_feature_tables.py`; every defaults table on the configuration page, the executor timeout included, gated by `tests/book_defaults.rs`, which reported five wrong or missing rows on the unfixed page]** `book/src/reference/configuration.md:17` gives the executor-timeout default as None; the code sets 1 h. The server README's `signing` row says "verification", but the crate does no signing. The feature table omits grpc-tls, auth-jwt, tls-rustls and conformance. | VALIDATED |
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
| C5 | High | **[Fixed: the README is a doctest now]** **The client README (its crates.io page) documents APIs that don't exist**: `resubscribe()`, `get_authenticated_extended_card()`, `ClientBuilder::with_transport()`. It says "10 variants" (there are 11), has a non-exhaustive `match` that won't compile, and gives the wrong description for the `signing` row. | VALIDATED [re-checked] |
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
| C16 | Medium | **[Fixed: `2b1bb80a` — was open work OW5]** gRPC `Unauthenticated`/`PermissionDenied` map to `InvalidParams`. `ErrorInfo` details are dropped. A mid-stream `Cancelled` becomes a retryable `Timeout`. slimrpc does the same at `error.rs:153`. | VALIDATED (code and tests) |
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
| T2 | High | **[Fixed: `signing` enables `float_roundtrip`; RFC 8785 vectors gate it]** **Signing: serde_json lacks `float_roundtrip`**, so floats in a card are off by one ULP before canonicalization. The RFC 8785 §3.2.4 example gives `333333333.33333325`, 5 of 24 Appendix-B vectors fail after parsing, and 29.7% of random exponent-form doubles parse wrong. | VALIDATED [re-checked: the feature is absent from every manifest] |
| T3 | High | **Signing: verification canonicalizes the re-serialized struct, not the received JSON** (`signing.rs:70-71`). Any unknown field, the legacy `url`, a missing `skills`, `null` capabilities, snake_case aliases or the v0.3 scheme form makes a valid peer signature fail. Empty defaults (`"skills":[]`) are added to the canonical bytes. There is no cross-SDK signing test. | VALIDATED [re-checked the code path] |
| T4 | Medium | **[Ties fixed with T2; `crit` and the `-32603` mapping are open]** The ES number formatter gets exact ties wrong (`1424953923781206.3` vs `.2`). `crit` headers go unchecked (RFC 7515 §4.1.11). A bad signature surfaces as `-32603 Internal`. | VALIDATED |
| T5 | Medium | One unknown enum value fails the whole payload: `TASK_STATE_PAUSED` fails the `Task`, `ROLE_SYSTEM` the `Message`, and an unknown or extra key in `StreamResponse` fails the event. A newer peer can break stream consumers. | VALIDATED |
| T6 | Medium | Values accepted over JSON can't be converted to proto (non-base64 `raw`, integers above 2^53, non-RFC3339 timestamps), so GetTask over gRPC or slimrpc returns INTERNAL. `has_valid_timestamp` accepts `"garbage T garbage garbage"`. | VALIDATED |
| T7 | Medium | JSON requires `contextId` on `Task`; proto doesn't. Large numbers in metadata are silently rounded, and `1e400` rejects the whole message. | VALIDATED |
| T8 | Medium | **[README half fixed: it is a doctest now; the book pages' `protocol_version: "1.0.0"` literals are open]** The types README says `A2A_VERSION = "1.0.0"`; the code has `"1.0"`. Its `Message` literal won't compile, its `match` is non-exhaustive, and `proto` is undocumented. `first-agent.md` and `concepts/agent-cards.md` teach `protocol_version: "1.0.0"` with a 16-field literal instead of the existing builders. | VALIDATED [re-checked README] |
| T9 | Low | `parse_iso8601_to_unix_millis` rolls invalid dates over (`2026-02-31` becomes Mar 3) and accepts non-ISO forms. It feeds ListTasks `statusTimestampAfter`. | VALIDATED |
| T10 | Low | Lossy round-trips through proto and JSON. These matter because signing re-serializes. | VALIDATED |
| T11 | Low | Semver: core structs have all-public fields and aren't `#[non_exhaustive]`, which is inconsistent with `AgentCapabilities`. `TaskState::ALL: [Self; 9]` exposes the variant count. | VALIDATED |
| T12 | Low | `Part::data(Null)` is rejected by Go. `Part::file` with neither bytes nor uri silently produces `raw("")`. Message ids are mandatory and there is no generator. | Mixed |

## 5. a2a-protocol-sdk and a2a-protocol-slimrpc

| # | Sev | Finding | Evidence |
|---|---|---|---|
| K1 | Medium | **[Fixed on this branch: the SDK takes the client and server with `default-features = false` and forwards its own defaults; `cargo tree -p a2a-protocol-sdk --no-default-features` has no rustls, hyper-rustls or webpki-roots]** `default-features = false` on the sdk does not remove TLS. The sdk's client and server dependencies don't set it, so rustls still comes in, and the manifest comment says otherwise. | VALIDATED (`cargo tree`) [re-checked manifest] |
| K2 | Medium | The prelude lacks what a server or coordinator needs: `Server`/`ServeConfig`, `FailureClass`, `CurrentTrace`, `TracePropagationInterceptor`, the caching resolver and `ErrorCode`. The shipped coordinator examples depend on 7 crates, not the sdk alone. | VALIDATED (compile) |
| K3 | Low | **[Feature table fixed and gated; the forwarding of `conformance` and `proto` is open]** The sdk README feature table omits `auth-jwt` and misattributes `tls-rustls`/`grpc-tls`. `conformance` and a bare `proto` are not forwarded. The crate root has no server+client example, and the macro docs use `a2a_protocol_server::` paths. | VALIDATED |
| K4 | Low | slimrpc: the docs say "no change to any of those crates" was needed, which the README contradicts. It names a nonexistent `A2aClientBuilder`. It inherits T6 (INTERNAL on unconvertible tasks). It builds, and 57 tests pass. | VALIDATED |

## 6. Why these got past review and tests

Each of these gaps is tied to at least one defect that escaped:

1. **Written claims are never checked against code.** Crate READMEs (the
   crates.io pages) are not compiled; 130 of the book's 206 Rust blocks are
   `ignore`; Cargo feature docs and defaults tables are unchecked. This let
   through O1, O3, O10, C5, T8, S9 and S12.
   **[Gated: the four crate READMEs compile as doctests
   (`check_readme_doctests.py` guards the include); every feature table is
   checked against its manifest (`check_feature_tables.py`); the
   configuration page's defaults tables against the structs' `Default`
   (`tests/book_defaults.rs`); and 127 of the book's 130 `ignore` blocks now
   compile, which found N15 — the other 3 are `slimrpc`, outside the
   workspace. Still unchecked: prose that makes a claim no code block
   exercises.]**
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
   **[Gated: E6's `ScriptedPeer` drives every binding through stall,
   cut-off, mis-frame and 401 in `tests/scripted_peer_tests.rs`; its first
   run found N13.]**
7. **Signing has no external test vectors.** RFC 8785 Appendix B is not in
   the tests. This let through T2, T3 and T4.
   **[Gated: `crates/a2a-protocol-types/tests/rfc8785_vectors.rs` carries
   Appendix B, the §3.2.3 sort sample and the §3.2.4 bytes, plus V8-sourced
   tie-rule edges; they found T2, the T4 tie and N12. T3 needs cross-SDK
   signed cards, not vectors, and is open.]**
8. **New parsers of peer input aren't required to have fuzz targets.**
   `check_fuzz_matrix.py` only checks that existing targets run. The
   traceparent panic shipped in 0.13.0 this way; JWT, REST query and
   X-Forwarded-For are still unfuzzed.
   **[Targets added: `jwt_token`, `rest_route`, `forwarded_for`, and for
   parsers the audit did not name, `webhook_url` and `page_token`; each ran
   its 60-second smoke clean. `fuzz/README.md` now keeps the inventory of
   every peer-input parser and its target, which is the requirement in
   written form; nothing yet fails when a new parser is added without a
   row. The client's REST error bodies and `Retry-After` are listed there as
   not yet covered.]**
9. **Release policy isn't checked by machine.** 0.12.0 (09-10) and 0.13.0
   (09-20) were both breaking, and `STABILITY.md` allows one breaking minor
   release per month. The `PurgeReport` rename skipped deprecation.
   **[Gated at release time: N10. Deprecate-first is still unchecked.]**

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

## 8. Bar-raiser candidates (proposed 2026-09-23; none started)

Needs from running agents, swarms and coordinators in production that no
finding above covers. Each gives the need, the evidence that it is missing
today (the command or file read, at `fa2e901`), a proposed shape, and the test
that must fail first. They are candidates, not commitments: `ROADMAP.md`
takes an item only when work on it starts. Ranked by what they change for an
operator, highest first.

### E1 — readiness that turns false when shutdown begins

- **Need.** In a rolling deploy the load balancer must stop routing to a
  replica before it starts cancelling work, or new requests land on a
  replica that is about to refuse or cancel them.
- **Missing.** REST answers `/ready` with a constant
  (`dispatch/rest/mod.rs:116`, `health_response()`); no dispatcher consults
  the handler's shutdown state. Overlaps O14.
- **Shape.** A readiness answer derived from the handler: 503 once
  shutdown has started or the store stops answering; `/health` (liveness)
  unchanged. The same on every dispatcher, and `grpc.health.v1` for gRPC.
- **Failing first.** Start `serve_with_shutdown`, fire the signal with a
  task held open, and assert `/ready` is 503 while `/health` is 200.

### E2 — deadline propagation across delegation hops

- **Need.** A coordinator with 30 s left should not delegate work that will
  run for five minutes, and a worker should stop when its caller's budget is
  spent. Budgets are how swarms stay bounded.
- **Missing.** `grep -rn -i 'grpc-timeout\|a2a-deadline'` over `crates/`
  returns nothing: no binding reads an inbound deadline, the executor cannot
  see one, and the client sends none.
- **Shape.** `RequestContext::deadline()` from `grpc-timeout` on gRPC (the
  standard header, which tonic already parses); for HTTP bindings a declared
  A2A extension header, since the specification defines none. The client
  derives the outbound deadline from the remaining budget. Needs a spec
  check before the HTTP half is built.
- **Failing first.** A worker behind a gRPC call with a 200 ms deadline
  observes `ctx.deadline()` as `Some` within that bound.

### E3 — per-peer circuit breaking in the client

- **Need.** In a swarm one dead worker should cost its callers one fast
  failure each, not a full retry schedule each: retries against a peer that
  is down multiply load exactly when it can least take it.
- **Missing.** No circuit-breaker state in `crates/a2a-protocol-client`
  (`grep -rn -i circuit` finds only a test message about short-circuiting a
  backoff).
- **Shape.** An opt-in `CallInterceptor`/transport wrapper keyed by
  endpoint: open after N consecutive retryable failures, half-open after a
  cool-down, with its state visible to metrics.
- **Failing first.** Against a closed port, the (N+1)th call fails without a
  connection attempt, measured by counting accepts on a listener.

### E4 — a retry budget with an overall deadline (C19)

- **Need.** "At most 10 s for this call, retries included" is what a caller
  can reason about; a per-attempt timeout times an attempt count is not.
- **Missing.** No overall bound in `retry.rs` (`grep -n -i
  'max_elapsed\|total_timeout\|overall'` returns nothing).
- **Failing first.** A policy with a 1 s overall budget against a server
  that answers 503 slowly returns within 1 s plus one attempt's timeout.

### E5 — stuck-task and outcome signals

- **Need.** The questions an on-call engineer asks of an agent fleet are
  "how many tasks are stuck, and how old is the oldest?" and "what fraction
  of tasks failed, by class?". Neither is answerable from today's catalogue.
- **Missing.** The eleven instruments in `otel/mod.rs` count requests,
  responses, errors, latency, queues, pool and push; none records task
  outcome or task age. Overlaps O11.
- **Shape.** `a2a.server.task.outcome` counter by terminal state and failure
  class; `a2a.server.task.oldest_non_terminal_age` gauge from a periodic
  store query (opt-in, since it is a query).
- **Failing first.** A `ManualReader` sees no outcome instrument after a
  task completes.

### E6 — a scripted hostile peer, published for adopters

- **Need.** Every adopter who writes a coordinator needs to test it against
  a worker that stalls, cuts a stream, sends a malformed frame or answers
  401. Escape class 6 (section 6) is this repository's own version of the
  same gap.
- **[Built: `a2a_protocol_client::testing::ScriptedPeer`, feature
  `testing`, on all four bindings; `tests/scripted_peer_tests.rs` runs every
  script against every binding. The C1 stall tests were not ported onto it;
  they still run as they were.]**
- **Missing.** The raw-TCP stubs exist only inside individual test files
  (`crates/a2a-protocol-client/tests/stream_liveness_tests.rs`,
  `hostile_server_tests.rs`), one per test.
- **Shape.** A `testing` feature exposing a scripted peer
  (`ScriptedPeer::new().stall_after(1).on(Binding::Rest)`), used by this
  repository's own tests first so it is exercised before it is published.
- **Failing first.** The existing stall and cut-off tests, ported to it,
  still fail on the pre-C1 client.

### E7 — `a2a doctor`: one command that says what is wrong with an agent

- **Need.** The first thing anyone does with a misbehaving agent is check
  its card, reach each advertised interface, confirm the protocol version,
  and try one call on each binding. Today that is a manual sequence.
- **Missing.** `tools/a2a-cli` has `card`, `send`, `stream` and `task`
  (`src/cli.rs:132-154`); nothing checks a card against its own interfaces.
- **Shape.** `a2a doctor URL`: fetch and validate the card; for each
  interface, connect, send one message, report latency and the error class of
  any failure; exit non-zero on any finding, so it can gate a deploy.
- **Failing first.** Against an agent whose card advertises a gRPC
  interface on a closed port, `doctor` exits non-zero naming that interface.
