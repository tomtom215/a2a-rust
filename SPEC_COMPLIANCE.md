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
| Unit / integration tests | Per-crate behavior (`cargo test --workspace --all-features`, 4000+ tests). |
| **TCK** (`a2a-tck`) | 22 conformance checks × {JSON-RPC, REST}, run against our `echo-agent` **and** against echo agents built on the official Python, JavaScript, Go, and Java SDKs (`itk/agents/*-sdk`). |
| **Bidirectional interop** | The official Python `a2a-sdk` **client** driving our server (`itk/interop/python_client_vs_rust.py`, 26 checks). |
| **ITK** | The upstream `a2aproject/a2a-itk` multi-hop traversal harness with this repo mounted as the `current` agent, plus the deterministic in-repo `itk/interop/itk_traversal_selftest.py`. |
| **gRPC wire fixtures** | Golden protobuf bytes serialized by the official Python SDK, decoded/re-encoded and diffed (`tck/fixtures/grpc/`). |
| **Fuzz** | libFuzzer targets over every untrusted parse surface (`fuzz/`). |
| **Hostile-peer** | Malicious-server harness (`crates/a2a-protocol-client/tests/hostile_server_tests.rs`). |

Spec section numbers use the `§` shorthand; the same numbers are cited
inline throughout the source (215+ `§` references) so a reviewer can grep
from either direction (`grep -rn "§3.4.3" crates/`).

---

## §3 — Protocol operations

| Spec area | Implementation | Evidence |
|---|---|---|
| §3.1 `SendMessage` (create/continue task) | `handler/messaging.rs` | TCK `send_message_*`; interop send checks |
| §3.1.2 Streaming send | `handler/messaging.rs`, `streaming/` | TCK `streaming_send_message`; interop streaming checks; ITK streaming scenarios |
| §3.1.4 `ListTasks` — order by `status.timestamp` desc, `statusTimestampAfter` filter | `handler/lifecycle/list_tasks.rs`; all four task stores (`store/`) | `list_tasks_*` unit tests; `statusTimestampAfter` store tests; ms-precision timestamps (`utc_now_iso8601`) |
| §3.4.2 Unknown `taskId` → `TaskNotFound` | `handler/messaging.rs`, `handler/lifecycle/get_task.rs` | TCK `get_unknown_task_returns_error` (portable across all official SDKs) |
| §3.4.3 `taskId`-only continuation infers `contextId` | `handler/messaging.rs` | `messaging` continuation tests |
| §3.3.4 Required-extension negotiation | `handler/capability.rs`, `interceptor.rs` | `ExtensionSupportRequired` tests; echoed `A2A-Extensions` header tests |
| §3.5.2 Resubscribe reconnection (snapshot-then-EOF; terminal-task rejection) | `handler/lifecycle/subscribe.rs` | subscribe tests; interop `subscribe to terminal task rejected`; ITK resubscribe scenarios |
| Task cancellation (working task cancelable by default) | `handler/lifecycle/cancel_task.rs`, `executor.rs` default `cancel` | `cancel_working_task_with_default_executor_succeeds`; ITK resubscribe (cancel-after-retrieval) |

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

## §8 — Agent discovery

| Spec area | Implementation | Evidence |
|---|---|---|
| §8.3 Well-known agent card at `/.well-known/agent-card.json` | dispatchers; `client/discovery.rs` | discovery tests; card served versionless (see §3.6.2 row) |
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
| §11.6 AIP-193 error bodies | `dispatch/rest/response.rs`; client REST decoder | REST error-shape tests |

## §12 — Custom bindings

| Spec area | Implementation | Evidence |
|---|---|---|
| WebSocket binding (`"WEBSOCKET"`) | `dispatch/websocket.rs` (`websocket` feature) | websocket test leg; message-size cap tests |

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
| §14.2.2 `A2A-Extensions` header | `handler/helpers.rs::parse_extensions_header`; `A2A_EXTENSIONS_HEADER` | extension-activation tests |

---

## Known non-conformances (in the ecosystem, not here)

Running our TCK against the official SDKs surfaced upstream divergences,
skipped in CI via `a2a-tck --skip` (reported, never silent):

| SDK | Test | Divergence |
|---|---|---|
| `@a2a-js/sdk` 1.0.0 | `list_tasks_basic` | `ListTasksRequest.fromJSON({})` materializes proto3-default `status=TASK_STATE_UNSPECIFIED`; the store filters on it, so unfiltered lists return empty. |
| `@a2a-js/sdk` 1.0.0 | `a2a_media_type_accepted` | Rejects `application/a2a+json`; spec §6.1 examples use it (Python/Go SDKs accept it). |
| `a2a-java` 1.0.0.CR1 | `a2a_media_type_accepted` | Same `application/a2a+json` rejection. |

Our implementation accepts `application/a2a+json` and returns a populated
`ListTasks` response, matching the spec and the Python/Go reference SDKs.
