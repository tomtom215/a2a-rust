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
failed before its fix: T1 (read side), S2, S6, S7, O16 (push URL logging), S1,
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
- **`tests/swarm_scale` fails under PostgreSQL with `--include-ignored`**
  ("task … is already being processed"), identically at `d423b94`. Medium,
  pre-existing, not investigated.

**Still open from phase 1's own scope:**

- **T1, write side, is a2a-go's to fix.** This SDK writes the spec's
  `{"list":[...]}`; a2a-go v2.5.0 rejects it. The interop gate pins the
  rejection and goes red when it stops holding.
- **S3: a replica learns of another's cancel only at its executor's next
  write**, and B's `cancel()` hook releases B's resources, not A's. A push
  mechanism (PostgreSQL `LISTEN/NOTIFY`, or a periodic check) is warranted;
  session affinity avoids the problem.
- **S8 is documented, not fixed:** the gRPC and WebSocket dispatchers' `serve`
  still takes no shutdown signal.
- **gRPC `Unauthenticated` still maps to `InvalidParams`** (C16), so a gRPC
  401 does not invalidate a cached token.

**Phases 2–5 — not started.** The proposed order is in section 7: real
observability (spans, propagation, semconv metrics, one `init_telemetry`),
coordinator developer experience, signing and types hardening, then docs
checked against the code.

---

Audit date 2026-09-22, against branch `claude/pensive-allen-socw7b` at `d423b94`.
The audit itself changed no repository file; the status section below records
what the fixes that followed did.

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
| O16 | Low | Two INFO lines per request. The untrusted JSON-RPC method name is logged at INFO, a log-forging risk with the plain `fmt` format. The full webhook URL is logged at INFO (`push/sender.rs:776`), and those URLs often carry secrets. Endpoint URLs are logged at INFO on every client call. | VALIDATED except the push URL |
| O17 | Low | `ClientRequest` derives `Debug` over `extra_headers` (`client/src/interceptor.rs:50`), so `{req:?}` prints `authorization: Bearer …`. Server `CallContext` and token providers redact correctly. | VALIDATED (code) |

**What went well:** metric cardinality is bounded (`metric_label()`), and the
bearer token did not appear in server logs. W3C propagation between the Rust
coordinator and Go agents was correct in all 9 binding pairs *when opted in*
(same trace-id, tracestate preserved, Go `Extract` valid).

## 2. a2a-protocol-server

| # | Sev | Finding | Evidence |
|---|---|---|---|
| S1 | High | **The documented graceful shutdown leaves downstream work running.** SIGINT during a streamed delegation followed the documented order. The 15 s socket drain (`serve/graceful/mod.rs:117`) ran before any task was cancelled. `handler.shutdown()` then cancels tokens but doesn't wait for executors. Result: exit after 16 s with `abandoned: 1`, no terminal event upstream, and no cancel sent to either Go task. | VALIDATED (live, a2a-go workers) [re-checked the constant] |
| S2 | High | **Over JSON-RPC, a streaming call's pre-stream error is sent as plain `application/json` 200.** a2a-go's client only reads `data:` lines, so it sees `events=0 err=<nil>`. Go clients silently lose "task not found" on `SendStreamingMessage` and `SubscribeToTask`. REST and gRPC are fine. | VALIDATED (Go client) |
| S3 | High | **Possible cross-replica cancel race.** CancelTask on replica B writes Canceled. Replica A's background processor still holds its in-memory `last_task`, and Postgres `save_status_delta` runs an unconditional `UPDATE … WHERE id = $4` (`store/postgres_store/store_impl.rs:241-246`). The client is told Canceled and the task ends Completed. Separately, `tests/multi_replica.rs:530-548` shows two replicas both accepting a continuation of the same task, which `horizontal-scaling.md` does not mention. | Unconditional UPDATE VALIDATED [re-checked]; race CONJECTURED |
| S4 | High | **`agent_executor!` can't be used by an executor that has state** (it hides `self`, `E0424`). Every coordinator has to write out the full `Pin<Box<dyn Future…>>` signature. | VALIDATED (compile) |
| S5 | High | **There are no delegation helpers.** Forwarding a downstream stream into the upstream queue, rewriting ids, passing cancellation downstream and merging fan-out streams all have to be hand-written: 110 of the 230 lines in the auditor's coordinator. The executor's `queue` is borrowed for `'a`, so spawned fan-out tasks can't write to it, which forces an mpsc relay. No book chapter covers delegation. | VALIDATED |
| S6 | Medium | Over REST, a mid-stream error is sent as `event: error` with a bare `{code,message}`. a2a-go's REST stream parser doesn't recognize it, so a Go client gets "unknown stream response type". | CONJECTURED (both sources) |
| S7 | Medium | The push token header and content type differ from a2a-go. Rust sends `x-a2a-notification-token` / `application/a2a+json`; Go uses `A2A-Notification-Token` / `application/json`. Each side's webhook rejects the other's pushes. The comment at `push/sender.rs:898-903` says official receivers use the X- name, which is not true of a2a-go 2.5.0. | VALIDATED (same webhook) |
| S8 | Medium | Graceful shutdown covers only JSON-RPC and REST. `GrpcDispatcher::serve` and `WebSocketDispatcher::serve` take no shutdown signal, and the gRPC background serve discards its error with `let _ =`. | VALIDATED (code) |
| S9 | Medium | README.md:60 says `shutdown()` reports a queue it had to force-destroy. The field is "always 0" (`handler/shutdown/mod.rs:30`), and every queue is destroyed unconditionally. | VALIDATED |
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
| C1 | High | **A stream that goes silent after its first event hangs forever.** There is no idle or per-event timeout, no TCP keepalive and no HTTP/2 ping (`streaming/event_stream.rs:332-341`, `tls.rs:126-138`). | VALIDATED (stub: still pending at 8 s with 1 s timeouts) |
| C2 | High | **A stream ending with no terminal event returns `None` like normal completion.** A partial final frame is silently dropped. There is no resume: the client parses `id:` but drops it, and `subscribe_to_task` can't send `Last-Event-ID`, although the server supports resumption. | VALIDATED |
| C3 | High | **OAuth2 refresh failures run one after another under one lock.** A failed refresh caches nothing, so each queued caller runs its own full-timeout refresh (`token_provider.rs:538`). With 1 s timeouts, 5 callers failed at 1, 2, 3, 4 and 5 s; at the 30 s default with 100 callers, that is about 50 minutes. | VALIDATED |
| C4 | High | **`ClientError` doesn't convert to `A2aError`**, so `?` in an executor fails (`E0277`). Only the reverse conversion exists (`error/mod.rs:184`). Every call site has to convert to a string, which loses whether it was a timeout, a transient failure or a protocol error. | VALIDATED [re-checked] |
| C5 | High | **The client README (its crates.io page) documents APIs that don't exist**: `resubscribe()`, `get_authenticated_extended_card()`, `ClientBuilder::with_transport()`. It says "10 variants" (there are 11), has a non-exhaustive `match` that won't compile, and gives the wrong description for the `signing` row. | VALIDATED [re-checked] |
| C6 | Medium | The first-event timeout reuses `stream_connect_timeout` (30 s). A Go agent that flushes headers and then thinks longer than 30 s is cut off (`jsonrpc.rs:401`, `rest/streaming.rs:87`). gRPC has the same problem. | VALIDATED (stub) |
| C7 | Medium | The blocking `send_message` has a 30 s `request_timeout`, too short for delegation, and retry is off by default. Both shipped coordinators wrap calls in their own timeouts. | CONJECTURED (code) |
| C8 | Medium | REST streaming errors aren't decoded, although REST unary errors are. `subscribe_to_task` 404 gives `UnexpectedStatus` where `get_task` gives `TaskNotFound`. Go's in-stream AIP-193 `{"error":…}` frames become `Serialization("unknown variant error")`. | VALIDATED (stub and Go server) |
| C9 | Medium | Deleting a push config on a Go server reports failure though it succeeded: over JSON-RPC Go returns no `result`, over REST it returns an empty 200. gRPC works. Go is the non-compliant side, but the Rust client should tolerate it. | VALIDATED (Go server) |
| C10 | Medium | Interface selection ignores `protocolVersion`, so it chose a Go agent's `/v03` endpoint over `/v1.0`. A lowercase `"jsonrpc"` binding makes `build()` fail (the selector ignores case, the factory doesn't). `from_card` falls back to `.first()` and errors instead of trying the next interface. | VALIDATED |
| C11 | Medium | The SSE parser truncates an endless line silently and never errors: 50 MiB with no newline gave 0 errors and 0 frames, holding up to 32 MiB. `max_event_size` can't be set from `ClientConfig`. | VALIDATED |
| C12 | Medium | Agent-card interface URLs are used as given: no same-origin check and no https→http downgrade guard, so an SSRF risk and a bearer-token leak risk. | CONJECTURED (code) |
| C13 | Medium | `HTTPS_PROXY`/`NO_PROXY` are ignored, and only the bundled webpki roots are trusted (no system roots). | VALIDATED (grep) |
| C14 | Medium | WebSocket: one slow stream consumer blocks routing for every request on the socket (conjectured). Pretty-printed JSON frames are corrupted by `data:` wrapping (validated). | Mixed |
| C15 | Medium | Token cache: `expires_in ≤ 30` means every call hits the token endpoint. A downstream 401 never invalidates the cached token. | VALIDATED |
| C16 | Medium | gRPC `Unauthenticated`/`PermissionDenied` map to `InvalidParams`. `ErrorInfo` details are dropped. A mid-stream `Cancelled` becomes a retryable `Timeout`. slimrpc does the same at `error.rs:153`. | VALIDATED (code and tests) |
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
| T1 | High | **Agent cards with security requirements can't be exchanged with a2a-go in either direction.** Rust writes `{"o":{"list":["s"]}}` (proto/spec shape); Go writes `{"o":["s"]}`, and each side fails to parse the other. Rust matches the spec, but in practice the reader must accept both shapes. | VALIDATED (both directions) |
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
