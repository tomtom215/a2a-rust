# a2a-rust: Implementation Plan & Roadmap

**Protocol Version:** A2A v1.0.0
**Target Rust Version:** 1.93.x (stable)
**License:** Apache-2.0
**Status:** Core implementation complete — hardening & polish phase

---

## Table of Contents

1. [Goals and Non-Goals](#1-goals-and-non-goals)
2. [Dependency Philosophy](#2-dependency-philosophy)
3. [Workspace & Crate Structure](#3-workspace--crate-structure)
4. [Architecture Overview](#4-architecture-overview)
5. [Complete File Inventory](#5-complete-file-inventory)
6. [Implementation Phases](#6-implementation-phases)
   - [Phase 0 — Project Foundation](#phase-0--project-foundation) ✅
   - [Phase 1 — Protocol Types (`a2a-types`)](#phase-1--protocol-types-a2a-types) ✅
   - [Phase 2 — HTTP Client (`a2a-client`)](#phase-2--http-client-a2a-client) ✅
   - [Phase 3 — Server Framework (`a2a-server`)](#phase-3--server-framework-a2a-server) ✅
   - [Phase 4 — v1.0 Protocol Upgrade](#phase-4--v10-protocol-upgrade) ✅
   - [Phase 5 — Server Tests & Bug Fixes](#phase-5--server-tests--bug-fixes) ✅
   - [Phase 6 — Umbrella Crate & Examples](#phase-6--umbrella-crate--examples) ✅
   - [Phase 7 — v1.0 Spec Gaps & Hardening](#phase-7--v10-spec-gaps--hardening) 🔲
   - [Phase 8 — Quality & Release Preparation](#phase-8--quality--release-preparation) 🔲
7. [Testing Strategy](#7-testing-strategy)
8. [Quality Gates](#8-quality-gates)
9. [Coding Standards](#9-coding-standards)
10. [Protocol Reference Summary](#10-protocol-reference-summary)

---

## 1. Goals and Non-Goals

### Goals

- **Full spec compliance** — every method, type, error code, and transport variant defined in A2A v1.0.0.
- **Enterprise-grade** — production-ready error handling, no panics, no `unwrap()` at boundaries.
- **Minimal footprint** — zero mandatory deps beyond `serde`/`serde_json`; optional features gate all I/O.
- **Modern Rust idioms** — async/await, Edition 2021, `Pin<Box<dyn Future>>` for object-safe async traits.
- **Transport abstraction** — pluggable HTTP backends; the protocol core carries no HTTP dep.
- **Strict modularity** — 500-line file cap, single-responsibility per module, thin `mod.rs` files.
- **Complete test coverage** — unit tests, integration tests with real TCP servers, end-to-end examples.
- **Zero `unsafe`** — unless crossing true FFI or raw pointer boundaries, with mandatory `// SAFETY:` comments.

### Non-Goals

- gRPC binding in the initial release (tracked post-v1.0, separate crate `a2a-grpc`).
- WebSocket transport (not in the v1.0 spec).
- Built-in persistence (`TaskStore` and `PushConfigStore` ship as in-memory defaults only; users plug in their own).
- Opinionated web framework integration (Axum, Actix adapters are examples, not core).

---

## 2. Dependency Philosophy

Every dependency is a maintenance liability and a supply chain risk. The following rules are enforced by `deny.toml`:

### Mandatory Runtime Dependencies

| Crate | Version | Justification | Features |
|---|---|---|---|
| `serde` | `>=1.0.200, <2` | JSON-RPC protocol is JSON-only — unavoidable | `derive` |
| `serde_json` | `>=1.0.115, <2` | JSON serialization for all wire types | default |
| `tokio` | `>=1.38, <2` | Async runtime; all I/O is async | `rt,net,io-util,sync,time,macros` |
| `hyper` | `>=1.4, <2` | Raw HTTP/1.1+2 for client and server | `client,server,http1,http2` |
| `http-body-util` | `>=0.1, <0.2` | Hyper 1.x body combinator | default |
| `hyper-util` | `>=0.1.6, <0.2` | Connection pooling, graceful shutdown | `client,client-legacy,http1,http2,tokio` |
| `uuid` | `>=1.8, <2` | Task/Message/Artifact ID generation | `v4` |
| `bytes` | `1` | Zero-copy byte buffer (used by hyper/SSE) | default |

### Optional Dependencies (feature-gated, not yet implemented)

| Crate | Feature Flag | Justification |
|---|---|---|
| `tracing` | `tracing` | Structured logging; zero cost when disabled |
| `rustls` | `tls-rustls` | TLS for HTTPS without OpenSSL system dep |
| `tokio-rustls` | `tls-rustls` | Tokio-integrated TLS |
| `webpki-roots` | `tls-rustls` | Mozilla root certificates |

### Dev/Test Only

| Crate | Purpose |
|---|---|
| `tokio` (full features) | Integration test runtime |
| `hyper-util` (server features) | Test server infrastructure |

### Explicitly Excluded

- `reqwest` — too many transitive deps (native-tls, cookie jar, etc.)
- `axum`/`actix-web` — framework lock-in; users choose their own
- `anyhow`/`thiserror` — we define our own error types (see `a2a-types/src/error.rs`)
- `openssl-sys` — prefer `rustls` for zero system deps
- `wiremock` — tests use real TCP servers instead of mocking

---

## 3. Workspace & Crate Structure

```
a2a-rust/
├── Cargo.toml                  # workspace manifest
├── Cargo.lock
├── LICENSE                     # Apache-2.0
├── README.md
├── LESSONS.md                  # Pitfall catalog
├── CONTRIBUTING.md
├── rust-toolchain.toml         # channel = "stable", components = [rustfmt, clippy]
├── deny.toml                   # cargo-deny: licenses, advisories, duplicates
├── clippy.toml                 # pedantic + nursery overrides
│
├── docs/
│   └── implementation/
│       ├── plan.md             # THIS DOCUMENT
│       └── type-mapping.md     # Spec types → Rust types with serde annotations
│
├── crates/
│   ├── a2a-types/              # All protocol types — serde only, no I/O
│   ├── a2a-client/             # HTTP client (hyper-backed)
│   ├── a2a-server/             # Server framework (hyper-backed)
│   └── a2a-sdk/                # Convenience umbrella re-export crate + prelude
│
└── examples/
    └── echo-agent/             # Full-stack demo (server + client, sync + streaming)
```

### Crate Dependency Graph

```
a2a-types  ←─────────────────────────── (no a2a-* deps)
     ↑
a2a-client  (depends on a2a-types)
     ↑
a2a-server  (depends on a2a-types)
     ↑
a2a-sdk     (re-exports a2a-types + a2a-client + a2a-server)
```

`a2a-client` and `a2a-server` are **siblings** — neither depends on the other.

### Why Four Crates?

| Crate | Audience | Compile Weight |
|---|---|---|
| `a2a-types` | Shared by client, server, and downstream type derivers | Minimal |
| `a2a-client` | Agent orchestrators, test harnesses | +hyper + tokio |
| `a2a-server` | Agent implementors | +hyper + tokio |
| `a2a-sdk` | Quick-start users who want everything | All of the above |

A downstream crate that only implements an agent server does not pay for the client's dep tree, and vice versa.

---

## 4. Architecture Overview

### Protocol Layers

```
┌─────────────────────────────────────────────┐
│               User Code                      │
│  (implements AgentExecutor or uses Client)   │
└───────────────────┬─────────────────────────┘
                    │
┌───────────────────▼─────────────────────────┐
│            a2a-server / a2a-client           │
│  RequestHandler | AgentExecutor | Client     │
│  (protocol logic, dispatch, SSE, push)       │
└───────────────────┬─────────────────────────┘
                    │
┌───────────────────▼─────────────────────────┐
│            Transport Layer                   │
│  JsonRpcDispatcher | RestDispatcher          │
│  JsonRpcTransport  | RestTransport           │
│  (pure HTTP plumbing, no protocol logic)     │
└───────────────────┬─────────────────────────┘
                    │
┌───────────────────▼─────────────────────────┐
│               hyper 1.x                      │
│  (raw HTTP/1.1 + HTTP/2, TLS optional)       │
└─────────────────────────────────────────────┘
```

### Server 3-Layer Architecture

**Layer 1 — User implements `AgentExecutor`:**

All trait methods use `Pin<Box<dyn Future>>` for object safety (dyn-dispatch via `Arc<dyn AgentExecutor>`):

```rust
pub trait AgentExecutor: Send + Sync + 'static {
    fn execute<'a>(
        &'a self,
        ctx: &'a RequestContext,
        queue: &'a dyn EventQueueWriter,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>>;

    fn cancel<'a>(
        &'a self,
        ctx: &'a RequestContext,
        queue: &'a dyn EventQueueWriter,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>>;
    // Default cancel() returns TaskNotCancelable error.
}
```

**Layer 2 — Framework provides `RequestHandler`:**

```rust
pub struct RequestHandler<E: AgentExecutor> { ... }
impl<E: AgentExecutor> RequestHandler<E> {
    pub async fn on_send_message(&self, params: MessageSendParams, streaming: bool) -> ServerResult<SendMessageResult>;
    pub async fn on_get_task(&self, params: TaskQueryParams) -> ServerResult<Task>;
    pub async fn on_list_tasks(&self, params: ListTasksParams) -> ServerResult<TaskListResponse>;
    pub async fn on_cancel_task(&self, params: CancelTaskParams) -> ServerResult<Task>;
    pub async fn on_resubscribe(&self, params: TaskIdParams) -> ServerResult<InMemoryQueueReader>;
    pub async fn on_set_push_config(&self, config: TaskPushNotificationConfig) -> ServerResult<TaskPushNotificationConfig>;
    pub async fn on_get_push_config(&self, params: GetPushConfigParams) -> ServerResult<TaskPushNotificationConfig>;
    pub async fn on_list_push_configs(&self, task_id: &str) -> ServerResult<Vec<TaskPushNotificationConfig>>;
    pub async fn on_delete_push_config(&self, params: DeletePushConfigParams) -> ServerResult<()>;
    pub async fn on_get_extended_agent_card(&self) -> ServerResult<AgentCard>;
}
```

**Layer 3 — Transport dispatchers wire hyper to `RequestHandler`:**

```rust
// JSON-RPC 2.0: routes PascalCase method names (SendMessage, GetTask, etc.)
pub struct JsonRpcDispatcher<E: AgentExecutor> { ... }

// REST: routes HTTP verb + path (/message:send, /tasks/{id}, etc.)
pub struct RestDispatcher<E: AgentExecutor> { ... }
```

### Client Architecture

```rust
pub struct A2aClient { ... }

impl A2aClient {
    pub fn from_card(card: &AgentCard) -> ClientResult<Self>;

    pub async fn send_message(&self, params: MessageSendParams) -> ClientResult<SendMessageResponse>;
    pub async fn stream_message(&self, params: MessageSendParams) -> ClientResult<EventStream>;
    pub async fn get_task(&self, params: TaskQueryParams) -> ClientResult<Task>;
    pub async fn list_tasks(&self, params: ListTasksParams) -> ClientResult<TaskListResponse>;
    pub async fn cancel_task(&self, params: CancelTaskParams) -> ClientResult<Task>;
    pub async fn resubscribe(&self, params: TaskIdParams) -> ClientResult<EventStream>;
    // push notification config methods...
    // get_authenticated_extended_card()...
}
```

---

## 5. Complete File Inventory

Every file listed with its responsibility and actual line count. No source file exceeds 500 lines.

### `crates/a2a-types/` (2,903 lines)

```
Cargo.toml                          [~25 lines]  serde + serde_json only
src/
  lib.rs                            [68 lines]   module declarations + pub use re-exports
  error.rs                          [276 lines]  A2aError, ErrorCode enum, A2aResult<T>
  task.rs                           [333 lines]  Task, TaskStatus, TaskState (SCREAMING_SNAKE_CASE), TaskId, ContextId, TaskVersion
  message.rs                        [308 lines]  Message, MessageRole, Part, PartContent (untagged oneof: Text/Raw/Url/Data)
  artifact.rs                       [139 lines]  Artifact, ArtifactId
  agent_card.rs                     [265 lines]  AgentCard, AgentCapabilities, AgentSkill, AgentProvider, AgentInterface
  security.rs                       [340 lines]  SecurityScheme variants (OAuth2, HTTP, ApiKey, OIDC, MutualTLS), OAuthFlows
  events.rs                         [181 lines]  TaskStatusUpdateEvent, TaskArtifactUpdateEvent, StreamResponse (untagged union)
  jsonrpc.rs                        [321 lines]  JsonRpcRequest, JsonRpcSuccessResponse<T>, JsonRpcErrorResponse, JsonRpcId
  params.rs                         [287 lines]  MessageSendParams, SendMessageConfiguration, TaskQueryParams, ListTasksParams, etc.
  push.rs                           [116 lines]  TaskPushNotificationConfig, AuthenticationInfo
  extensions.rs                     [105 lines]  AgentExtension, AgentCardSignature
  responses.rs                      [164 lines]  SendMessageResponse (Task|Message union), TaskListResponse
```

### `crates/a2a-client/` (3,408 lines)

```
Cargo.toml                          [~35 lines]  a2a-types + hyper + tokio + uuid
src/
  lib.rs                            [127 lines]  module declarations + pub use re-exports + doc examples
  error.rs                          [140 lines]  ClientError (Http, Serialization, Protocol, Transport, etc.), ClientResult<T>
  config.rs                         [153 lines]  ClientConfig, TlsConfig; transport binding constants (JSONRPC, REST, GRPC)
  client.rs                         [132 lines]  A2aClient struct, from_card(), config()
  builder.rs                        [285 lines]  ClientBuilder: endpoint, timeout, protocol binding, interceptors, TLS, build()
  discovery.rs                      [157 lines]  resolve_agent_card(): fetch /.well-known/agent.json; parse + validate
  interceptor.rs                    [287 lines]  CallInterceptor trait, InterceptorChain, ClientRequest, ClientResponse
  auth.rs                           [282 lines]  AuthInterceptor, CredentialsStore trait, InMemoryCredentialsStore, SessionId
  transport/
    mod.rs                          [74 lines]   Transport trait definition; re-exports
    jsonrpc.rs                      [324 lines]  JSON-RPC over HTTP: build request, parse response, SSE streaming, body reader
    rest.rs                         [472 lines]  REST over HTTP: route mapping, path params, verb mapping, streaming
  methods/
    mod.rs                          [13 lines]   re-exports
    send_message.rs                 [72 lines]   send_message() + stream_message()
    tasks.rs                        [147 lines]  get_task(), list_tasks(), cancel_task(), resubscribe()
    push_config.rs                  [164 lines]  set/get/list/delete push notification config
    extended_card.rs                [51 lines]   get_authenticated_extended_card()
  streaming/
    mod.rs                          [14 lines]   re-exports
    sse_parser.rs                   [269 lines]  SSE line parser; frame accumulator; handles keep-alive comments
    event_stream.rs                 [245 lines]  EventStream: reads SSE frames; deserializes JsonRpcResponse<StreamResponse>
```

### `crates/a2a-server/` (2,585 lines)

```
Cargo.toml                          [~38 lines]  a2a-types + hyper + tokio + uuid + bytes
src/
  lib.rs                            [67 lines]   module declarations + pub use re-exports
  error.rs                          [135 lines]  ServerError, ServerResult<T>, to_a2a_error() conversion
  executor.rs                       [68 lines]   AgentExecutor trait (Pin<Box<dyn Future>> for object safety)
  handler.rs                        [449 lines]  RequestHandler<E>: on_send_message, collect_events, find_task_by_context
  builder.rs                        [126 lines]  RequestHandlerBuilder: executor, stores, push, interceptors, agent card
  request_context.rs                [61 lines]   RequestContext: message, task_id, context_id, stored_task, metadata
  call_context.rs                   [50 lines]   CallContext: method name for interceptor use
  interceptor.rs                    [111 lines]  ServerInterceptor trait, ServerInterceptorChain (before/after hooks)
  dispatch/
    mod.rs                          [10 lines]   re-exports
    jsonrpc.rs                      [227 lines]  JSON-RPC 2.0 dispatcher: route PascalCase methods, serialize responses
    rest.rs                         [322 lines]  REST dispatcher: route HTTP verb + path, colon-suffixed actions
  agent_card/
    mod.rs                          [13 lines]   re-exports; CORS_ALLOW_ALL constant
    static_handler.rs               [50 lines]   StaticAgentCardHandler: serves pre-serialized AgentCard
    dynamic_handler.rs              [75 lines]   DynamicAgentCardHandler, AgentCardProducer trait
  streaming/
    mod.rs                          [12 lines]   re-exports
    sse.rs                          [192 lines]  build_sse_response (wraps events in JSON-RPC envelopes), SseBodyWriter, keep-alive
    event_queue.rs                  [173 lines]  EventQueueWriter/Reader traits, InMemoryQueue (mpsc), EventQueueManager
  push/
    mod.rs                          [10 lines]   re-exports
    sender.rs                       [131 lines]  PushSender trait, HttpPushSender impl
    config_store.rs                 [141 lines]  PushConfigStore trait, InMemoryPushConfigStore
  store/
    mod.rs                          [8 lines]    re-exports
    task_store.rs                   [154 lines]  TaskStore trait, InMemoryTaskStore (with list filtering)
```

### `crates/a2a-sdk/` (83 lines)

```
Cargo.toml                          [~24 lines]  re-exports all workspace crates
src/
  lib.rs                            [83 lines]   types/client/server modules + prelude with common re-exports
```

### `examples/` (415 lines)

```
echo-agent/
  Cargo.toml                        [~24 lines]
  src/main.rs                       [415 lines]  Full-stack demo: EchoExecutor, JSON-RPC + REST servers,
                                                 5 demos (sync/streaming × JSON-RPC/REST + GetTask)
```

### Integration Tests (1,702 lines)

```
crates/a2a-server/tests/
  handler_tests.rs                  [704 lines]  20 tests: EchoExecutor, FailingExecutor, CancelableExecutor,
                                                 send/get/list/cancel/resubscribe/push config CRUD
  dispatch_tests.rs                 [855 lines]  21 tests: real TCP server, JSON-RPC + REST dispatch,
                                                 streaming SSE, agent card serving, error responses
  streaming_tests.rs                [143 lines]  7 tests: event queue lifecycle, SSE frame formatting
```

### Project Totals

| Component | Lines |
|---|---|
| a2a-types | 2,903 |
| a2a-client | 3,408 |
| a2a-server | 2,585 |
| a2a-sdk | 83 |
| examples | 415 |
| integration tests | 1,702 |
| **Total** | **~11,100** |
| **Tests** | **157 (50 types + 51+8 client + 48 server)** |

---

## 6. Implementation Phases

### Phase 0 — Project Foundation ✅ COMPLETE

**Deliverables:** Compilable workspace, tooling configured.

| Task | Status |
|---|---|
| Workspace `Cargo.toml` with `[profile.release]` | ✅ |
| `rust-toolchain.toml` pinned to stable | ✅ |
| `deny.toml`, `clippy.toml` | ✅ |
| README, LESSONS, CONTRIBUTING | ✅ |
| Empty crate stubs | ✅ |

---

### Phase 1 — Protocol Types (`a2a-types`) ✅ COMPLETE

**Deliverables:** Complete, serialization-correct Rust types for every A2A v1.0.0 schema. 50 unit tests passing.

All types implemented with v1.0.0 wire format:
- **Enums**: `SCREAMING_SNAKE_CASE` serialization (e.g., `TASK_STATE_COMPLETED`, `ROLE_USER`)
- **Methods**: `PascalCase` (e.g., `SendMessage`, `GetTask`)
- **JSON fields**: `camelCase` via `#[serde(rename_all = "camelCase")]`
- **Oneof unions**: Untagged serde (`#[serde(untagged)]`) for `Part`, `StreamResponse`, `SendMessageResponse`
- **Newtype IDs**: `TaskId`, `ContextId`, `MessageId`, `ArtifactId` for type safety

Key serde patterns:
- `#[serde(skip_serializing_if = "Option::is_none")]` on all optional fields
- `#[serde(rename = "...")]` for explicit per-variant enum names
- `#[serde(untagged)]` for protocol oneof unions

---

### Phase 2 — HTTP Client (`a2a-client`) ✅ COMPLETE

**Deliverables:** Working client supporting JSON-RPC and REST transports. 51 unit tests + 8 doc-tests passing.

Implemented:
- `ClientBuilder` with fluent API (endpoint, timeout, protocol binding, interceptors, TLS toggle)
- `JsonRpcTransport`: HTTP POST with JSON-RPC envelopes, SSE streaming
- `RestTransport`: HTTP verb + path routing, path parameter extraction
- `EventStream`: async SSE parser that deserializes `JsonRpcResponse<StreamResponse>` frames
- `SseParser`: raw byte-level SSE frame accumulator with keep-alive comment handling
- `AuthInterceptor` + `InMemoryCredentialsStore` for bearer/basic auth
- `resolve_agent_card()`: fetch `/.well-known/agent.json`
- All 11 RPC methods implemented as `async` methods on `A2aClient`

---

### Phase 3 — Server Framework (`a2a-server`) ✅ COMPLETE

**Deliverables:** Full `AgentExecutor`-based server framework with both JSON-RPC and REST dispatchers, SSE streaming, push notification support, and in-memory stores.

Implemented:
- `AgentExecutor` trait (object-safe with `Pin<Box<dyn Future>>`)
- `RequestHandler` with all protocol methods
- `RequestHandlerBuilder` with fluent API
- `JsonRpcDispatcher` and `RestDispatcher` for hyper
- `EventQueueManager` with `InMemoryQueueWriter`/`InMemoryQueueReader` (tokio mpsc)
- `build_sse_response()` producing SSE with JSON-RPC envelope wrapping
- `TaskStore` trait + `InMemoryTaskStore`
- `PushConfigStore` trait + `InMemoryPushConfigStore`
- `PushSender` trait + `HttpPushSender`
- `ServerInterceptor` + `ServerInterceptorChain`
- `StaticAgentCardHandler` + `DynamicAgentCardHandler`

---

### Phase 4 — v1.0 Protocol Upgrade ✅ COMPLETE

**Deliverables:** Full upgrade from A2A v0.3.0 to v1.0.0 wire format across all three crates.

This phase was not in the original plan but was required when the A2A spec was updated.

Key changes:
- `TaskState` enum: `kebab-case` → `SCREAMING_SNAKE_CASE` (e.g., `"TASK_STATE_COMPLETED"`)
- `MessageRole` enum: `lowercase` → `SCREAMING_SNAKE_CASE` (e.g., `"ROLE_USER"`)
- `Part` type: tagged `kind` discriminator → untagged `PartContent` oneof (`text`/`raw`/`url`/`data` fields)
- `StreamResponse`: tagged `kind` → untagged oneof (discriminated by field presence)
- `AgentCard`: flat `url`/`preferred_transport` → `supported_interfaces: Vec<AgentInterface>`
- `ContextId` newtype added (was plain `String`)
- `AgentCapabilities.state_transition_history` removed
- `AgentCapabilities.extended_agent_card` added
- Agent card path: `/.well-known/agent-card.json` → `/.well-known/agent.json`
- JSON-RPC method names: `snake/case` → `PascalCase` (e.g., `message/send` → `SendMessage`)
- `TaskStatus.message` changed from `Option<Message>` to optional embedded message
- `Task.kind` field removed (v1.0 uses untagged unions)

---

### Phase 5 — Server Tests & Bug Fixes ✅ COMPLETE

**Deliverables:** 48 integration tests for a2a-server. Critical event queue lifecycle bug fixed.

Tests organized across three files:
- `handler_tests.rs` (20 tests): send message, get/list/cancel tasks, resubscribe, push config CRUD, error propagation, executor failure handling
- `dispatch_tests.rs` (21 tests): real TCP servers with JSON-RPC and REST dispatch, streaming SSE responses, agent card serving, method-not-found errors
- `streaming_tests.rs` (7 tests): event queue write/read, manager lifecycle, SSE frame formatting

**Bug fixed:** `on_send_message` spawned executor task retained a writer reference through the `EventQueueManager`, preventing the mpsc channel from closing. Non-streaming sends would hang forever waiting for `collect_events`. Fix: spawned task owns the writer `Arc` directly and calls `event_queue_mgr.destroy()` on completion.

**Bug fixed (Phase 6):** `build_sse_response` was serializing raw `StreamResponse` JSON in SSE data frames, but the client's `EventStream` expected `JsonRpcResponse<StreamResponse>` envelopes. Fix: server now wraps each SSE event in a `JsonRpcSuccessResponse` envelope.

---

### Phase 6 — Umbrella Crate & Examples ✅ COMPLETE

**Deliverables:** `a2a-sdk` prelude module; working end-to-end echo-agent example.

#### `a2a-sdk` Enhancements

- Added `prelude` module with curated re-exports of the most commonly used types:
  - Wire types: `Task`, `TaskState`, `Message`, `Part`, `Artifact`, `StreamResponse`, `AgentCard`, etc.
  - ID newtypes: `TaskId`, `ContextId`, `MessageId`, `ArtifactId`
  - Params/responses: `MessageSendParams`, `SendMessageResponse`, `TaskListResponse`, etc.
  - Client: `A2aClient`, `ClientBuilder`, `EventStream`
  - Server: `AgentExecutor`, `RequestHandler`, `RequestHandlerBuilder`, dispatchers
  - Errors: `A2aError`, `A2aResult`, `ClientError`, `ServerError`
- Updated description from v0.3.0 to v1.0

#### `examples/echo-agent`

Single binary demonstrating the full A2A stack:
1. `EchoExecutor` implementing `AgentExecutor` (Working → Artifact → Completed)
2. Server startup with **both** JSON-RPC and REST dispatchers on separate ports
3. **5 demos** exercised end-to-end:
   - Synchronous `SendMessage` via JSON-RPC
   - Streaming `SendStreamingMessage` via JSON-RPC
   - Synchronous `SendMessage` via REST
   - Streaming `SendStreamingMessage` via REST
   - `GetTask` retrieval of a previously created task

All demos complete successfully, validating the full client-server pipeline across both transport bindings.

---

### Phase 7 — v1.0 Spec Gaps & Hardening 🔲 NOT STARTED

**Deliverables:** Close remaining gaps against the A2A v1.0.0 specification. Additional test coverage for edge cases.

#### 7a. REST Query Parameter Parsing for ListTasks

The REST dispatcher's `handle_list_tasks` currently ignores query parameters. Implement parsing of:
- `context_id` — filter tasks by conversation context
- `status` — filter by task state
- `page_size` / `page_token` — pagination
- `status_timestamp_after` — timestamp filtering
- `include_artifacts` — control artifact inclusion

Location: `crates/a2a-server/src/dispatch/rest.rs` → `handle_list_tasks`

#### 7b. Tenant-Prefixed REST Routes

The v1.0 spec supports optional tenant-prefixed routes (e.g., `/tenants/{tenant}/tasks/{id}`). Currently not implemented.

Location: `crates/a2a-server/src/dispatch/rest.rs` — add optional prefix matching

#### 7c. SubscribeToTask as GET in REST

The REST dispatcher currently only handles `POST /tasks/{id}:subscribe`. The v1.0 spec also allows `GET /tasks/{id}:subscribe` for SSE.

Location: `crates/a2a-server/src/dispatch/rest.rs` — add GET handler for subscribe routes

#### 7d. Additional Server Tests

| Test area | Description |
|---|---|
| Interceptor chain | Test before/after hooks, rejection (interceptor returns error stops processing) |
| Task continuation | Same `context_id` in second request finds previous task via `stored_task` |
| Concurrent send + cancel | Race condition between executor running and cancel request arriving |
| Push notification delivery | `PushSender` called during `collect_events` when push configs exist |
| REST query parsing | Verify `ListTasks` filters work with URL query parameters |
| Error edge cases | Malformed JSON bodies, missing required fields, oversized payloads |

#### 7e. Client-Side Improvements

- REST `ListTasks` should pass query parameters in URL (not body) for GET requests
- Better error messages when server returns unexpected response shapes
- Connection keep-alive verification

---

### Phase 8 — Quality & Release Preparation 🔲 NOT STARTED

**Deliverables:** All quality gates passing, documentation complete, crates publishable.

| Task | Status | Notes |
|---|---|---|
| Property-based tests (`proptest`) | 🔲 | `TaskState` transitions, `Part` round-trip, ID uniqueness |
| Corpus-based JSON tests | 🔲 | Deserialize official spec samples; verify round-trip fidelity |
| Benchmark suite (`criterion`) | 🔲 | JSON serialization, SSE parse, handler throughput |
| `cargo doc --no-deps -D warnings` | ✅ | Zero warnings; all public items documented |
| `LESSONS.md` finalization | 🔲 | Document all non-obvious pitfalls discovered |
| `CONTRIBUTING.md` update | 🔲 | Testing guide, PR checklist |
| Publish dry-run | 🔲 | `cargo publish --dry-run` for each crate |
| Version alignment | 🔲 | All crates at `0.1.0` with consistent metadata |
| `tracing` feature flag | 🔲 | Optional structured logging |
| TLS support | 🔲 | `tls-rustls` feature for HTTPS |
| CI pipeline hardening | 🔲 | Enforce all quality gates in GitHub Actions |

---

## 7. Testing Strategy

### Current Test Coverage

| Crate | Unit Tests | Integration Tests | Doc-Tests | Total |
|---|---|---|---|---|
| `a2a-types` | 50 | — | — | 50 |
| `a2a-client` | 51 | — | 8 | 59 |
| `a2a-server` | — | 48 | — | 48 |
| **Total** | **101** | **48** | **8** | **157** |

### Test Patterns

| Pattern | Description |
|---|---|
| Test executors | `EchoExecutor` (happy path), `FailingExecutor` (error path), `CancelableExecutor` (cancel support) |
| Real TCP servers | Integration tests start actual hyper servers on random ports — no mocking |
| SSE round-trip | Client and server tested with real SSE streaming over TCP |
| Helper constructors | `make_message()`, `make_send_params()`, `minimal_agent_card()` for test brevity |

### Test Organization

- Unit tests: `#[cfg(test)]` modules inside each source file
- Integration tests: `crates/a2a-server/tests/` directory
- End-to-end validation: `examples/echo-agent` (runs all transport paths)

### Test Naming Convention

`{component}_{scenario}_{expected_outcome}`

Examples:
- `task_state_completed_is_terminal`
- `text_part_roundtrip_preserves_metadata`
- `jsonrpc_send_message_returns_task`
- `rest_get_task_returns_task`
- `sse_write_event_format`
- `queue_destroy_allows_recreation`

---

## 8. Quality Gates

All gates must pass before tagging a release. Currently enforced manually; CI hardening is a Phase 8 task.

```bash
# Formatting (zero diffs allowed)
cargo fmt --all -- --check

# Linting (zero warnings allowed)
cargo clippy --workspace --all-targets

# Tests (all must pass)
cargo test --workspace

# Documentation (zero warnings)
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps

# End-to-end smoke test
cargo run -p echo-agent
```

**Current status:** All gates passing ✅

---

## 9. Coding Standards

### File-Level

- **SPDX header on every file:**
  ```
  // SPDX-License-Identifier: Apache-2.0
  // Copyright 2026 Tom F.
  ```
- **500-line maximum.** When a file approaches 400 lines, extract a submodule.
- **Thin `mod.rs` files** (8–15 lines): module declarations + `pub use` re-exports only. No logic.

### Error Handling

- `unwrap()` and `expect()` are **forbidden** in library code.
- `unwrap()` in tests/examples is acceptable with clear context.
- `?` operator is the standard propagation mechanism.
- `panic!()` forbidden except in `unreachable!()` for provably exhaustive matches.

### Async Trait Pattern

All async traits use `Pin<Box<dyn Future>>` return types for object safety:

```rust
fn method<'a>(
    &'a self,
    arg: &'a Type,
) -> Pin<Box<dyn Future<Output = Result<T>> + Send + 'a>>;
```

This pattern applies to: `AgentExecutor`, `TaskStore`, `PushConfigStore`, `PushSender`, `Transport`, `CallInterceptor`, `ServerInterceptor`, `EventQueueWriter`, `EventQueueReader`.

### Serde Conventions

| Convention | Applied To |
|---|---|
| `#[serde(rename_all = "camelCase")]` | All structs |
| `#[serde(rename = "SCREAMING_SNAKE")]` | Enum variants (v1.0 wire format) |
| `#[serde(skip_serializing_if = "Option::is_none")]` | All optional fields |
| `#[serde(untagged)]` | Oneof unions (Part, StreamResponse, SendMessageResponse) |

### Unsafe

- `unsafe` blocks are **forbidden** unless crossing true FFI boundaries.
- Every `unsafe` block requires a `// SAFETY:` comment explaining the upheld invariants.
- `#![deny(unsafe_op_in_unsafe_fn)]` in every crate.

### Documentation

- `#![warn(missing_docs)]` in every crate.
- Module-level docs: purpose → key types → usage example.
- Public struct/enum docs: what it represents, which spec section defines it.

---

## 10. Protocol Reference Summary

A condensed quick-reference for implementation use (updated for v1.0.0).

### All RPC Methods (v1.0.0 PascalCase names)

| Method | Transport | Params | Returns |
|---|---|---|---|
| `SendMessage` | JSON-RPC POST | `MessageSendParams` | `Task \| Message` |
| `SendStreamingMessage` | JSON-RPC POST → SSE | `MessageSendParams` | SSE `StreamResponse` events |
| `GetTask` | JSON-RPC POST | `TaskQueryParams` | `Task` |
| `ListTasks` | JSON-RPC POST | `ListTasksParams` | `TaskListResponse` |
| `CancelTask` | JSON-RPC POST | `CancelTaskParams` | `Task` |
| `SubscribeToTask` | JSON-RPC POST → SSE | `TaskIdParams` | SSE `StreamResponse` events |
| `CreateTaskPushNotificationConfig` | JSON-RPC POST | `TaskPushNotificationConfig` | `TaskPushNotificationConfig` |
| `GetTaskPushNotificationConfig` | JSON-RPC POST | `GetPushConfigParams` | `TaskPushNotificationConfig` |
| `ListTaskPushNotificationConfigs` | JSON-RPC POST | `TaskIdParams` | `Vec<TaskPushNotificationConfig>` |
| `DeleteTaskPushNotificationConfig` | JSON-RPC POST | `DeletePushConfigParams` | `{}` |
| `GetExtendedAgentCard` | JSON-RPC POST | — | `AgentCard` |

### REST Route Table

| Method | Path | Handler |
|---|---|---|
| `POST` | `/message:send` | `SendMessage` |
| `POST` | `/message:stream` | `SendStreamingMessage` |
| `GET` | `/tasks/{id}` | `GetTask` |
| `GET` | `/tasks` | `ListTasks` |
| `POST` | `/tasks/{id}:cancel` | `CancelTask` |
| `POST` | `/tasks/{id}:subscribe` | `SubscribeToTask` |
| `POST` | `/tasks/{taskId}/pushNotificationConfigs` | `CreateTaskPushNotificationConfig` |
| `GET` | `/tasks/{taskId}/pushNotificationConfigs/{id}` | `GetTaskPushNotificationConfig` |
| `GET` | `/tasks/{taskId}/pushNotificationConfigs` | `ListTaskPushNotificationConfigs` |
| `DELETE` | `/tasks/{taskId}/pushNotificationConfigs/{id}` | `DeleteTaskPushNotificationConfig` |
| `GET` | `/extendedAgentCard` | `GetExtendedAgentCard` |
| `GET` | `/.well-known/agent.json` | Agent card discovery |

### SSE Event Format

Each SSE event wraps a `StreamResponse` in a JSON-RPC success response envelope:

```
event: message
data: {"jsonrpc":"2.0","id":null,"result":{...StreamResponse...}}
```

`StreamResponse` is an untagged union discriminated by field presence:

| Discriminating field | Rust variant | When emitted |
|---|---|---|
| `status` + `taskId` (no `artifact`) | `StatusUpdate(TaskStatusUpdateEvent)` | On every task state transition |
| `artifact` + `taskId` | `ArtifactUpdate(TaskArtifactUpdateEvent)` | When an artifact is ready or appended |
| `id` + `contextId` + `status` | `Task(Task)` | Full task snapshot |
| `role` + `parts` | `Message(Message)` | Agent response as a direct message |

### Terminal Task States

`Completed`, `Failed`, `Canceled`, `Rejected` — serialized as `TASK_STATE_COMPLETED`, etc. No further state transitions possible.

### Error Code Quick-Reference

| Code | Name | When to use |
|---|---|---|
| -32700 | ParseError | Malformed JSON body |
| -32600 | InvalidRequest | Missing `jsonrpc`/`method`/`id` fields |
| -32601 | MethodNotFound | Unknown method name |
| -32602 | InvalidParams | Params don't match expected schema |
| -32603 | InternalError | Unexpected server error |
| -32001 | TaskNotFound | `GetTask` or `CancelTask` with unknown ID |
| -32002 | TaskNotCancelable | Cancel requested for terminal task |
| -32003 | PushNotificationNotSupported | Push requested; agent doesn't support it |
| -32004 | UnsupportedOperation | Method exists but not implemented |
| -32005 | ContentTypeNotSupported | Requested MIME type unsupported |
| -32006 | InvalidAgentResponse | Agent returned invalid response shape |
| -32007 | ExtendedAgentCardNotConfigured | No extended card configured |
| -32008 | ExtensionSupportRequired | Required extension not declared by client |
| -32009 | VersionNotSupported | Protocol version mismatch |

### AgentCard Well-Known URI

```
GET https://{host}/.well-known/agent.json
Response: application/json
CORS: Access-Control-Allow-Origin: *
```

### Key v1.0.0 Wire Format Differences from v0.3.0

| Aspect | v0.3.0 | v1.0.0 |
|---|---|---|
| Enum serialization | `kebab-case` / `lowercase` | `SCREAMING_SNAKE_CASE` |
| Method names | `snake/case` | `PascalCase` |
| Part discrimination | `kind` tag field | Untagged by field presence |
| StreamResponse | `kind` tag field | Untagged by field presence |
| Agent card URL | `url` field on AgentCard | `supported_interfaces[].url` |
| Transport binding | `preferred_transport` enum | `protocol_binding` string |
| Agent card path | `/.well-known/agent-card.json` | `/.well-known/agent.json` |
| Context ID | plain `String` | `ContextId` newtype |
| Capabilities | `state_transition_history` | `extended_agent_card` (replaces it) |

---

*Document version: 2.0 — post-implementation update*
*Last updated: 2026-03-15*
*Author: Tom F.*
