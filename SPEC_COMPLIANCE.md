<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# A2A v1.0 spec-compliance traceability matrix

This document maps each normative area of the
[A2A v1.0 specification](https://a2a-protocol.org/v1.0.0/specification) to
its implementation in this repository and to the test evidence that proves
it. It is the single reference for a conformance review: every row names a
spec section, where it lives in the code, and how it is verified.

**Verification layers referenced below**

| Layer | What it proves |
|---|---|
| Unit / integration tests | Per-crate behavior (`cargo test --workspace --all-features`: 3,198 passed, 0 failed, 175 ignored across 103 test binaries, measured 2026-09-10; the ignored ones need a live PostgreSQL, SPIRE or hours of soak and run in their own CI jobs). |
| **Official TCK** (`a2aproject/a2a-tck`) | The A2A project's own conformance suite, RFC 2119-graded, run against `tck/sut`. Authoritative where it overlaps the in-repo TCK. Score and open findings: `docs/official-tck-findings.md`. |
| **In-repo TCK** (`a2a-tck`) | 22 conformance checks, run over all four bindings (JSON-RPC, REST, WebSocket, gRPC) against our own SUT, and over JSON-RPC and REST against echo agents built on the official Python, JavaScript, Go, and Java SDKs (`itk/agents/*-sdk`) — the cross-SDK client direction the official TCK does not cover. |
| **Bidirectional interop** | The official Python `a2a-sdk` **client** driving our server (`itk/interop/python_client_vs_rust.py`, 26 checks). |
| **ITK** | The upstream `a2aproject/a2a-itk` multi-hop traversal harness with this repo mounted as the `current` agent, plus the deterministic in-repo `itk/interop/itk_traversal_selftest.py`. |
| **gRPC wire fixtures** | Golden protobuf bytes serialized by the official Python SDK, decoded/re-encoded and diffed (`tck/fixtures/grpc/`). |
| **Fuzz** | libFuzzer targets over every untrusted parse surface (`fuzz/`). |
| **Hostile-peer** | Malicious-server harness (`crates/a2a-protocol-client/tests/hostile_server_tests.rs`). |

Spec section numbers use the `§` shorthand; the same numbers are cited
inline throughout the source (400+ `§` references in `crates/` — 408 on
2026-09-20, by `grep -rno "§" --include=*.rs crates | wc -l`) so a reviewer can grep
from either direction (`grep -rn "§3.4.3" crates/`).

---

## §3 — Protocol operations

| Spec area | Implementation | Evidence |
|---|---|---|
| §3.1 `SendMessage` (create/continue task) | `handler/messaging/` (`mod.rs` orchestrates; `create.rs`, `continuation.rs`) | TCK `send_message_*`; interop send checks |
| §3.1.2 Streaming send | `handler/messaging/`, `streaming/` | TCK `streaming_send_message`; interop streaming checks; ITK streaming scenarios |
| §3.1.4 `ListTasks` — order by `status.timestamp` desc, `statusTimestampAfter` filter | `handler/lifecycle/list_tasks.rs`; all four task stores (`store/`) | `list_tasks_*` unit tests; `statusTimestampAfter` store tests; ms-precision timestamps (`utc_now_iso8601`) |
| §3.4.2 Unknown `taskId` → `TaskNotFound` | `handler/messaging/continuation.rs`, `handler/lifecycle/get_task.rs` | TCK `get_unknown_task_returns_error` (portable across all official SDKs) |
| §3.4.3 `taskId`-only continuation infers `contextId` | `handler/messaging/continuation.rs` | `messaging` continuation tests |
| §3.3.4 Required-extension negotiation | `handler/capability.rs`, `interceptor.rs` | `ExtensionSupportRequired` tests; echoed `A2A-Extensions` header tests |
| §3.5.2 Resubscribe reconnection: a **non-terminal** task's stream serves the snapshot and then stays **open** until the task is terminal (§3.1.6); a terminal task is rejected | `handler/lifecycle/subscribe.rs` | `resubscribe_nonterminal_no_queue_waits_for_the_terminal_state`; other subscribe tests; interop `subscribe to terminal task rejected`; ITK resubscribe scenarios |
| §3.5.2 resumption: a client's `Last-Event-ID` replays exactly what it missed from the task's event log, bounded by `subscribe_replay_limit`. A2A requires reconnection to work, not this mechanism — `id:` / `Last-Event-ID` is the WHATWG SSE one | `streaming/sse.rs` (`id:` emission); `handler/lifecycle/subscribe.rs` (`last_event_id`, replay, the catch-up wait) | `sse_resumption_e2e.rs`; `sse_format_tests.rs`; `event_log_tests/resumption.rs`, `event_log_tests/live_resubscribe.rs` |
| Append-only task event log — the substrate that replay reads. **A mechanism, not an A2A requirement**: the specification has no event-log concept, and a store that does not implement it still conforms (the stream then starts from the snapshot) | `store/task_store/mod.rs` (`supports_event_log`, `append_event`, `last_event_seq`, `read_events`); `store/task_store/in_memory/mod.rs`; `store/event_log_sql.rs`; `store/tenant_event_log.rs` | `event_log_tests/log.rs`; `store/sqlite_store/event_log_tests.rs`; retention: `PurgeReport::orphan_rows_deleted` |
| Task cancellation (working task cancelable by default) | `handler/lifecycle/cancel_task.rs`, `executor.rs` default `cancel` | `cancel_working_task_with_default_executor_succeeds`; ITK resubscribe (cancel-after-retrieval) |

**Correction, 0.13.0.** Until this release the §3.5.2 row above read
"snapshot-then-EOF", and a test asserted it. That was the `STREAM-SUB-002`
defect written down as an expectation: §3.1.6 says the stream "MUST terminate
when the task reaches a terminal state", and a stream that ended while its task
was still `Working` terminated early. 0.13.0 removed the immediate EOF — the
reasoning is in `handler/lifecycle/subscribe.rs`, on
`resubscribe_nonterminal_no_queue_waits_for_the_terminal_state` — and this
matrix is corrected to match. A compliance matrix that documents a defect as
the compliance claim is worse than no matrix.

## §3.6 — Protocol versioning

| Spec area | Implementation | Evidence |
|---|---|---|
| §3.6 Wire version is `Major.Minor` (`"1.0"`, no patch) | `A2A_VERSION` (`lib.rs`) | version-constant tests; agent-card fixtures |
| §3.6.1 Clients send `A2A-Version` | client transports (jsonrpc/rest/websocket) | TCK sends the header on every request; client transport tests |
| §3.6.2 Absent/empty header ⇒ 0.3 ⇒ rejected by v1.0 server | `dispatch/mod.rs::validate_version_header`; jsonrpc/rest/websocket dispatchers | `jsonrpc_missing_version_header_rejected`, `rest_missing_version_header_rejected_but_card_discovery_versionless`, `ws_*` handshake tests; opt-out `accept_missing_version_header` |
| §3.6.2 Unsupported version → `VersionNotSupported` (all bindings) | dispatchers + `dispatch/grpc/helpers.rs::validated_metadata` | version-mismatch tests per binding |

## §4 — Data model

| Spec area | Implementation | Evidence |
|---|---|---|
| Task / Message / Part / Artifact / AgentCard shapes | `a2a-protocol-types/src/*.rs` | serde round-trip tests; `fuzz/json_deser`, `fuzz/jsonrpc_envelope` |
| §5.6.1 ms-precision timestamps; RFC 3339 parsing | `lib.rs` (`parse_iso8601_to_unix_millis`, `unix_millis_to_iso8601`) | parser unit tests; `fuzz/iso8601` (parse/format round-trip stability) |
| Discriminated unions (`SendMessageResponse`, `StreamResponse`) | `responses.rs`, `events.rs` | TCK wire-format checks; fuzz |

## §5 — Binding requirements & interoperability

| Spec area | Implementation | Evidence |
|---|---|---|
| §5.3 Method mapping (PascalCase RPC names; colon-suffixed REST paths) | jsonrpc/rest/websocket dispatchers; `client/transport/rest/routing.rs` | `jsonrpc_legacy_method_names_rejected`, `rest_legacy_slash_paths_not_found`; TCK against all official SDKs |
| §5.1 Error equivalence across bindings | `error.rs` (`ErrorCode`, `A2A_ERROR_DOMAIN`); `dispatch/grpc/helpers.rs`; client decoders | gRPC `ErrorInfo` tests; REST AIP-193 decode tests |
| §5.6 Protobuf ↔ ProtoJSON semantics | `proto/convert/` | golden gRPC fixtures; `fuzz/proto_convert` (differential round-trip) |

## §7 — Authentication & authorization

| Spec area | Implementation | Evidence |
|---|---|---|
| Bearer / API-key / JWT interceptors | `auth/` (`auth-jwt` feature) | `auth_jwt_e2e`, JWKS/OIDC discovery e2e tests; live-TLS JWKS test |
| JWKS parsing (remote keys) | `auth/jwt.rs::Jwks::from_json` | unit tests; `fuzz/jwks_parse` |
| §7.6.4 `TASK_STATE_AUTH_REQUIRED` is not itself an authorization (added upstream 2026-07-30, `6550d34`) | `ServerInterceptor` runs before every method regardless of task state; no server code reads `AuthRequired` for any decision (`grep -rn AuthRequired crates/a2a-protocol-server/src` returns **no lines**; the only mentions in the server crate are under `crates/a2a-protocol-server/tests/`) | `auth_required_state_tests.rs`: a continuation of an `AUTH_REQUIRED` task without credentials is rejected by the interceptor exactly as the first request was |

## §8 — Agent discovery

| Spec area | Implementation | Evidence |
|---|---|---|
| §8.3 Well-known agent card at `/.well-known/agent-card.json` | dispatchers; `client/discovery.rs` | discovery tests; card served versionless (see §3.6.2 row) |
| §8.3.2 rule 4 Client sends the selected interface's `tenant` on **every** request | `methods/mod.rs::tenant_or_default`, applied in all eleven methods; `builder/mod.rs` populates `ClientConfig::tenant` from the card | `methods::tests::card_tenant_is_sent_on_every_request` (per-method, by name); `tenant_round_trip_tests.rs` (client vs. tenant-partitioned server over JSON-RPC and REST, with a `TaskNotFound` control) |
| `AgentInterface.url` for gRPC is a target `host:port` (proto comment, upstream `cfc9d34`) | `transport/grpc.rs::normalize_endpoint`; `GrpcBareAddressScheme` | `normalize_*` unit tests; `a2a-protocol-sdk/tests/grpc_address_e2e.rs` (bare loopback → plaintext; bare target with TLS and a pinned CA; bundled roots reject an unknown CA) |
| Card body bounded (DoS) | `client/discovery.rs` (`MAX_CARD_BODY_SIZE`, read timeout) | hostile-peer: `oversized_card_body_rejected`, `slow_drip_card_body_times_out`, `short_body_under_declared_length_errors`, `immediate_reset_errors`, `valid_json_wrong_shape_rejected` |
| §13.3 Extended card requires authentication by default | `handler/lifecycle/extended_card.rs`; `ServerInterceptor::authenticates()` | extended-card auth tests; `allow_unauthenticated_extended_card` opt-out |

## §9 / §10 / §11 — Protocol bindings

| Spec area | Implementation | Evidence |
|---|---|---|
| §9 JSON-RPC binding | `dispatch/jsonrpc/` | full TCK JSON-RPC leg (our agent + 4 official SDKs) |
| §9.1 `Content-Type: application/json` (a2a+json accepted) | `dispatch/*`; `A2A_CONTENT_TYPE`, `JSON_CONTENT_TYPE` | media-type tests |
| §9.4.2 SSE frames echo request `id` | `dispatch/jsonrpc/response.rs`, `streaming/sse.rs` | SSE envelope tests |
| §10 gRPC binding (canonical `lf.a2a.v1.A2AService`) | `dispatch/grpc/` (`grpc` feature); `client/transport/grpc.rs` | grpc test leg; golden wire fixtures; ITK gRPC scenarios |
| §10.6 gRPC `google.rpc.ErrorInfo` details | `dispatch/grpc/helpers.rs` | gRPC error-detail tests |
| §11 HTTP+JSON/REST binding | `dispatch/rest/` | full TCK REST leg (our agent + 4 official SDKs) |
| §11.3 / proto `additional_bindings`: tenant as `/{tenant}/…` prefix; §11.5: `?tenant=` on GET/DELETE and body on POST | client `transport/rest/request.rs::build_uri`; server `dispatch/rest/mod.rs` (prefix → query → body, path wins, percent-decoded) | client `build_uri_*tenant*` tests; server `rest_tenant_binding_tests.rs` (7 arrival/precedence cases, each with a `TaskNotFound` control) |
| §11.6 AIP-193 error bodies | `dispatch/rest/response.rs`; client REST decoder | REST error-shape tests |

## §12 — Custom bindings

| Spec area | Implementation | Evidence |
|---|---|---|
| WebSocket binding | `dispatch/websocket.rs` (`websocket` feature) | websocket test leg; message-size cap tests |
| §5.8 URI-identified binding | `WEBSOCKET_BINDING_URI` in `a2a-protocol-types` | TCK SUT advertises the URI; `binding_for` tests cover URI, legacy name, case-folding, and a foreign URI |

## §13 — Security considerations

| Spec area | Implementation | Evidence |
|---|---|---|
| §13.3 Extended-card auth (see §8 row) | — | — |
| Push webhook SSRF defense (DNS-resolve + pin) | `push/sender.rs` (`tls-rustls`) | push-sender e2e; live-TLS rebinding test |
| Push webhook auth scheme (RFC 9110 case-insensitive) | `push/sender.rs` | push auth-scheme tests |
| Push delivery: non-retryable 4xx fail fast | `push/sender.rs` | retry-policy tests |
| Slow streaming consumer gets explicit lag error (no silent loss) | `streaming/event_queue/`, `streaming/sse.rs` | event-queue lag tests; persistence via dedicated lossless channel |
| Message-size caps (JSON-RPC/REST 4 MiB; WS frame cap) | `dispatch/mod.rs::DispatchConfig`; `dispatch/websocket.rs` | oversized-body hardening tests |

## §14 — IANA / media types

| Spec area | Implementation | Evidence |
|---|---|---|
| §14.1.1 `application/a2a+json` registered constant | `A2A_CONTENT_TYPE` | accepted-on-ingress tests |
| §4.3.3 push webhook sends `application/a2a+json` | `push/sender.rs` | `request_has_a2a_media_type_content_type` |
| §14.2.2 `A2A-Extensions` header | `handler/helpers.rs::parse_extensions_header`; `A2A_EXTENSIONS_HEADER` | extension-activation tests |

---

## Beyond A2A v1.0 — extensions and mechanisms this SDK defines

**Nothing in this section is an A2A v1.0 requirement, and no row here is a
conformance claim.** The two protocol extensions below are identified by
`a2a-rust.com` URIs precisely so that a server enabling them stays conformant
and the official TCK is unaffected — a peer that ignores the URI sees ordinary
A2A. They are recorded here for traceability, not for credit.

| Area | Implementation | Evidence |
|---|---|---|
| **Idempotency extension** — `https://a2a-rust.com/extensions/idempotency/v1`. A client-supplied key on `message/send`. `Message.extensions` is a list of URIs and cannot carry a value, so the key travels in `Message.metadata` under `a2a-rust.com/idempotency-key` while `extensions` declares the URI. Keys are scoped to the **tenant** | `a2a-protocol-types/src/idempotency.rs` (URI, `set_key`, `key_of`, `validate_key`); `handler/messaging/idempotency.rs`; `store/task_store/mod.rs` (`supports_idempotency`, `claim_idempotency_key`, `release_idempotency_key`) with all four bundled stores implementing them; `builder.rs` advertises the extension only when the store reports support; client `methods/send_message.rs` | `handler/messaging/idempotency_tests/`; `store/sqlite_store/idempotency_tests.rs`; `postgres_store_tests.rs`; `book/src/client/idempotency.md` (blocks compiled) |
| **Failure-class extension** — `https://a2a-rust.com/extensions/failure/v1`. A `FailureClass` a caller can `match` on instead of parsing English out of an error message; same metadata/extensions split, for the same reason | `a2a-protocol-types/src/failure.rs`; `executor_helpers.rs`; `handler/messaging/execute.rs` | `a2a-protocol-types/src/failure/tests.rs`; `failure_class_tests.rs` |
| **W3C Trace Context** (`traceparent` / `tracestate`) carried across an A2A hop. A **W3C** specification, not an A2A one — A2A says nothing about tracing. Reserved `trace-flags` bits are zeroed on the way out (§3.2.2.5.2, §4.3) and an oversized `tracestate` is truncated entry-wise rather than discarded (§3.3.1.5). **Not carried over the `websocket` transport**, which has no per-request header channel; §3.4 forbids the connect-time value that would be the only alternative. `InboundTracePolicy` decides whether an as-yet-unauthenticated peer may choose the `trace-id` (§7.2, §3.4) | `a2a-protocol-types/src/trace_context.rs` (parse, validate, re-emit, derive a child; mints identifiers only on a policy restart); server `handler/helpers.rs` (`InboundTracePolicy`) and `call_context.rs` (inbound header → `RequestContext`); `builder.rs` (`with_inbound_trace_policy`); client `trace_propagation.rs` (outbound, from an ambient scope); client `transport/websocket.rs` (documents the carve-out and warns once per connection) | `a2a-protocol-types/src/trace_context/tests.rs`; `a2a-protocol-client/tests/trace_propagation_e2e.rs`; `a2a-protocol-server/tests/inbound_trace_policy.rs` (all three policies end to end, and two policies in one process); `fuzz/fuzz_targets/trace_context.rs` |
| **`AgentExecutor` conformance harness** (`conformance` feature). Grades an *executor* against the protocol invariants that hold for any agent, driving it against a real event queue with no server and no ports. It is **not** a statement that a server conforms — the TCK rows above are what say that | `conformance/` (`mod.rs`, `run/checks.rs`, `report.rs`) | `conformance/tests/gaps.rs` — deliberately broken executors, each breaking one invariant and asserting the harness names that one and no other; `book/src/deployment/testing.md` (block compiled) |

---

## Known non-conformances (in the ecosystem, not here)

Running our TCK against the official SDKs surfaced upstream divergences,
skipped in CI via `a2a-tck --skip` (reported, never silent):

| SDK | Test | Divergence |
|---|---|---|
| `@a2a-js/sdk` 1.0.0 | `list_tasks_basic` | `ListTasksRequest.fromJSON({})` materializes proto3-default `status=TASK_STATE_UNSPECIFIED`; the store filters on it, so unfiltered lists return empty. |
| `@a2a-js/sdk` 1.0.0 | `a2a_media_type_accepted` | Rejects `application/a2a+json`; spec §6.1 examples use it (Python/Go SDKs accept it). |
| `a2a-java` 1.3.0.Final | `a2a_media_type_accepted` | Same `application/a2a+json` rejection, on the REST binding only. |

Our implementation accepts `application/a2a+json` and returns a populated
`ListTasks` response, matching the spec and the Python/Go reference SDKs.
