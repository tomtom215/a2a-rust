# a2a-rust: Implementation Plan & Roadmap

**Protocol Version:** A2A 0.3.0
**Target Rust Version:** 1.93.x (stable)
**License:** Apache-2.0
**Status:** Pre-implementation — planning complete

---

## Table of Contents

1. [Goals and Non-Goals](#1-goals-and-non-goals)
2. [Dependency Philosophy](#2-dependency-philosophy)
3. [Workspace & Crate Structure](#3-workspace--crate-structure)
4. [Architecture Overview](#4-architecture-overview)
5. [Complete File Inventory](#5-complete-file-inventory)
6. [Implementation Phases](#6-implementation-phases)
   - [Phase 0 — Project Foundation](#phase-0--project-foundation)
   - [Phase 1 — Protocol Types (`a2a-types`)](#phase-1--protocol-types-a2a-types)
   - [Phase 2 — HTTP Client (`a2a-client`)](#phase-2--http-client-a2a-client)
   - [Phase 3 — Server Framework (`a2a-server`)](#phase-3--server-framework-a2a-server)
   - [Phase 4 — SSE Streaming](#phase-4--sse-streaming)
   - [Phase 5 — Push Notifications](#phase-5--push-notifications)
   - [Phase 6 — Umbrella Crate & Examples](#phase-6--umbrella-crate--examples)
   - [Phase 7 — Quality & Release Preparation](#phase-7--quality--release-preparation)
7. [Testing Strategy](#7-testing-strategy)
8. [Quality Gates](#8-quality-gates)
9. [Coding Standards](#9-coding-standards)
10. [Protocol Reference Summary](#10-protocol-reference-summary)

---

## 1. Goals and Non-Goals

### Goals

- **Full spec compliance** — every method, type, error code, and transport variant defined in A2A 0.3.0.
- **Enterprise-grade** — production-ready error handling, no panics, no `unwrap()` at boundaries, structured logging support.
- **Minimal footprint** — zero mandatory deps beyond `serde`/`serde_json`; optional features gate all I/O.
- **Modern Rust idioms** — async/await, `async fn` in traits (stable since 1.75), `impl Trait`, `LazyLock`, Edition 2021.
- **Transport abstraction** — pluggable HTTP backends; the protocol core carries no HTTP dep.
- **Strict modularity** — 500-line file cap, single-responsibility per module, thin `mod.rs` files.
- **Complete test coverage** — four-layer testing strategy; E2E tests are non-negotiable.
- **Zero `unsafe`** — unless crossing true FFI or raw pointer boundaries, with mandatory `// SAFETY:` comments.

### Non-Goals

- gRPC binding in the initial release (tracked in Phase 8+, separate crate `a2a-grpc`).
- WebSocket transport (not in the 0.3.0 spec).
- Built-in persistence (TaskStore and PushConfigStore ship as in-memory defaults only; users plug in their own).
- Opinionated web framework integration (Axum, Actix adapters are examples, not core).

---

## 2. Dependency Philosophy

Every dependency is a maintenance liability and a supply chain risk. The following rules are enforced by `deny.toml`:

### Mandatory Runtime Dependencies

| Crate | Version | Justification | Features |
|---|---|---|---|
| `serde` | `>=1.0.200, <2` | JSON-RPC protocol is JSON-only — unavoidable | `derive` |
| `serde_json` | `>=1.0.115, <2` | JSON serialization for all wire types | default |
| `tokio` | `>=1.38, <2` | Async runtime; all I/O is async | `rt,net,io-util,sync,time` |
| `hyper` | `>=1.4, <2` | Raw HTTP/1.1+2 for client and server | `client,server,http1,http2` |
| `http-body-util` | `>=0.1, <0.2` | Hyper 1.x body combinator | default |
| `hyper-util` | `>=0.1.6, <0.2` | Connection pooling, graceful shutdown | `client,client-legacy,http1,http2,tokio` |
| `uuid` | `>=1.8, <2` | Task/Message/Artifact ID generation | `v4` |

### Optional Dependencies (feature-gated)

| Crate | Feature Flag | Justification |
|---|---|---|
| `tracing` | `tracing` | Structured logging; zero cost when disabled |
| `rustls` | `tls-rustls` | TLS for HTTPS without OpenSSL system dep |
| `tokio-rustls` | `tls-rustls` | Tokio-integrated TLS |
| `webpki-roots` | `tls-rustls` | Mozilla root certificates |

### Dev/Test Only

| Crate | Purpose |
|---|---|
| `tokio` (full) | Integration test runtime |
| `proptest` | Property-based testing |
| `wiremock` | HTTP mock server for client tests |
| `criterion` | Microbenchmarks |

### Explicitly Excluded

- `reqwest` — too many transitive deps (native-tls, cookie jar, etc.)
- `axum`/`actix-web` — framework lock-in; users choose their own
- `anyhow`/`thiserror` — we define our own error types (see `a2a-types/src/error.rs`)
- `openssl-sys` — prefer `rustls` for zero system deps
- `log` — prefer `tracing` (feature-gated); no mandatory logging dep

---

## 3. Workspace & Crate Structure

```
a2a-rust/
├── Cargo.toml                  # workspace manifest
├── Cargo.lock
├── LICENSE                     # Apache-2.0
├── README.md
├── LESSONS.md                  # Pitfall catalog (grows during development)
├── CONTRIBUTING.md
├── rust-toolchain.toml         # channel = "stable", components = [rustfmt, clippy]
├── deny.toml                   # cargo-deny: licenses, advisories, duplicates
├── clippy.toml                 # pedantic + nursery overrides
│
├── docs/
│   ├── implementation/
│   │   ├── plan.md             # THIS DOCUMENT
│   │   └── type-mapping.md     # Spec types → Rust types with serde annotations
│   └── adr/
│       ├── 0001-workspace-crate-structure.md
│       ├── 0002-dependency-philosophy.md
│       ├── 0003-async-runtime-strategy.md
│       ├── 0004-transport-abstraction.md
│       └── 0005-sse-streaming-design.md
│
├── crates/
│   ├── a2a-types/              # All protocol types — serde only, no I/O
│   ├── a2a-client/             # HTTP client (hyper-backed)
│   ├── a2a-server/             # Server framework (hyper-backed)
│   └── a2a-sdk/                # Convenience umbrella re-export crate
│
└── examples/
    ├── echo-agent/             # Minimal server implementation
    ├── client-demo/            # Client usage walkthrough
    └── streaming-agent/        # SSE streaming demonstration
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
│  JSON-RPC dispatcher | REST dispatcher       │
│  (pure HTTP plumbing, no protocol logic)     │
└───────────────────┬─────────────────────────┘
                    │
┌───────────────────▼─────────────────────────┐
│               hyper 1.x                      │
│  (raw HTTP/1.1 + HTTP/2, TLS optional)       │
└─────────────────────────────────────────────┘
```

### Server 3-Layer Architecture (mirrors Go SDK)

**Layer 1 — User implements `AgentExecutor`:**
```rust
pub trait AgentExecutor: Send + Sync + 'static {
    async fn execute(
        &self,
        ctx: &RequestContext,
        queue: &dyn EventQueueWriter,
    ) -> Result<(), A2aError>;

    async fn cancel(
        &self,
        ctx: &RequestContext,
        queue: &dyn EventQueueWriter,
    ) -> Result<(), A2aError>;
}
```

**Layer 2 — Framework provides `RequestHandler`:**
```rust
pub struct RequestHandler<E: AgentExecutor> { ... }
impl<E: AgentExecutor> RequestHandler<E> {
    pub fn builder(executor: E) -> RequestHandlerBuilder<E> { ... }
    pub async fn on_send_message(&self, params: MessageSendParams) -> A2aResult<SendMessageResponse>;
    pub async fn on_send_message_stream(&self, params: MessageSendParams) -> A2aResult<EventStream>;
    pub async fn on_get_task(&self, params: TaskQueryParams) -> A2aResult<Task>;
    pub async fn on_list_tasks(&self, params: ListTasksParams) -> A2aResult<TaskListResponse>;
    pub async fn on_cancel_task(&self, params: TaskIdParams) -> A2aResult<Task>;
    pub async fn on_resubscribe(&self, params: TaskIdParams) -> A2aResult<EventStream>;
    // push notification config methods...
    // agent/authenticatedExtendedCard...
}
```

**Layer 3 — Transport adapters wire hyper to `RequestHandler`:**
```rust
// JSON-RPC transport (HTTP POST to single endpoint)
pub struct JsonRpcHandler<E: AgentExecutor> { ... }

// REST transport (RESTful HTTP verbs + paths)
pub struct RestHandler<E: AgentExecutor> { ... }
```

### Client Architecture

```rust
pub struct A2aClient { ... }

impl A2aClient {
    pub async fn from_card(card: &AgentCard) -> A2aResult<Self>;
    pub fn builder() -> ClientBuilder;

    pub async fn send_message(&self, params: MessageSendParams) -> A2aResult<SendMessageResponse>;
    pub async fn stream_message(&self, params: MessageSendParams) -> A2aResult<EventStream>;
    pub async fn get_task(&self, params: TaskQueryParams) -> A2aResult<Task>;
    pub async fn list_tasks(&self, params: ListTasksParams) -> A2aResult<TaskListResponse>;
    pub async fn cancel_task(&self, id: TaskId) -> A2aResult<Task>;
    pub async fn resubscribe(&self, id: TaskId) -> A2aResult<EventStream>;
    // push notification config methods...
}
```

---

## 5. Complete File Inventory

Every file is listed with its single responsibility and estimated line count. No file exceeds 500 lines.

### `crates/a2a-types/`

```
Cargo.toml                          [~40 lines]  serde + serde_json only
src/
  lib.rs                            [~60 lines]  module declarations + pub use re-exports
  error.rs                          [~120 lines] A2aError enum, A2aResult<T>, error codes
  task.rs                           [~180 lines] Task, TaskStatus, TaskState, TaskId, TaskVersion
  message.rs                        [~200 lines] Message, MessageRole, Part, TextPart, FilePart, DataPart, FileContent
  artifact.rs                       [~100 lines] Artifact, ArtifactId
  agent_card.rs                     [~280 lines] AgentCard, AgentCapabilities, AgentSkill, AgentProvider, AgentInterface, TransportProtocol
  security.rs                       [~220 lines] SecurityScheme variants, OAuthFlows, SecurityRequirements, NamedSecuritySchemes
  events.rs                         [~150 lines] TaskStatusUpdateEvent, TaskArtifactUpdateEvent, StreamResponse (union)
  jsonrpc.rs                        [~160 lines] JSONRPCRequest, JSONRPCSuccessResponse, JSONRPCErrorResponse, JSONRPCError, JsonRpcId
  params.rs                         [~200 lines] MessageSendParams, SendMessageConfiguration, TaskQueryParams, TaskIdParams, ListTasksParams, GetPushConfigParams, DeletePushConfigParams
  push.rs                           [~130 lines] PushNotificationConfig, PushNotificationAuthInfo, TaskPushNotificationConfig
  extensions.rs                     [~100 lines] AgentExtension, ExtensionUri, AgentCardSignature
  responses.rs                      [~120 lines] SendMessageResponse (Task|Message union), TaskListResponse, AuthenticatedExtendedCardResponse
```

### `crates/a2a-client/`

```
Cargo.toml                          [~50 lines]  a2a-types + hyper + tokio + uuid
src/
  lib.rs                            [~50 lines]  module declarations + pub use re-exports
  error.rs                          [~100 lines] ClientError, ClientResult<T>; From<A2aError>
  config.rs                         [~120 lines] ClientConfig (preferred transports, output modes, TLS settings)
  client.rs                         [~280 lines] A2aClient struct, constructor, all method impls (delegates to transport)
  builder.rs                        [~180 lines] ClientBuilder with method chaining; validates config
  discovery.rs                      [~160 lines] resolve_agent_card(); fetch /.well-known/agent-card.json; parse + validate
  interceptor.rs                    [~100 lines] CallInterceptor trait (before/after); InterceptorChain
  auth.rs                           [~180 lines] AuthInterceptor, CredentialsStore trait, InMemoryCredentialsStore, SessionId
  transport/
    mod.rs                          [~40 lines]  Transport trait definition; re-exports
    jsonrpc.rs                      [~350 lines] HTTP JSON-RPC transport impl; build request, parse response, error mapping
    rest.rs                         [~380 lines] HTTP REST transport impl; path building, verb mapping, response parsing
  methods/
    mod.rs                          [~30 lines]  re-exports only
    send_message.rs                 [~120 lines] send_message() + stream_message() client methods
    tasks.rs                        [~200 lines] get_task(), list_tasks(), cancel_task(), resubscribe()
    push_config.rs                  [~180 lines] set/get/list/delete push notification config
    extended_card.rs                [~80 lines]  agent/authenticatedExtendedCard call
  streaming/
    mod.rs                          [~30 lines]  re-exports
    sse_parser.rs                   [~220 lines] raw SSE line parser; frame accumulator; handles keep-alive comments
    event_stream.rs                 [~200 lines] EventStream type (AsyncIterator); reads SSE frames; deserializes events
```

### `crates/a2a-server/`

```
Cargo.toml                          [~55 lines]  a2a-types + hyper + tokio + uuid
src/
  lib.rs                            [~60 lines]  module declarations + pub use re-exports
  error.rs                          [~100 lines] ServerError, ServerResult<T>
  executor.rs                       [~120 lines] AgentExecutor trait; RequestContext struct
  handler.rs                        [~350 lines] RequestHandler<E>; all on_* method impls
  builder.rs                        [~200 lines] RequestHandlerBuilder with fluent API; validates required fields
  request_context.rs                [~130 lines] RequestContext fields + constructors; RelatedTasks loader
  call_context.rs                   [~100 lines] CallContext (user, extensions, method name)
  interceptor.rs                    [~120 lines] CallInterceptor + RequestContextInterceptor traits; InterceptorChain
  dispatch/
    mod.rs                          [~35 lines]  re-exports
    jsonrpc.rs                      [~420 lines] parse JSON-RPC body; route by method name; serialize response; error mapping
    rest.rs                         [~450 lines] parse REST path + verb; route to handler method; serialize response
  agent_card/
    mod.rs                          [~40 lines]  re-exports; CORS header constants
    static_handler.rs               [~80 lines]  StaticAgentCardHandler; serves a fixed AgentCard
    dynamic_handler.rs              [~100 lines] DynamicAgentCardHandler; AgentCardProducer trait; calls producer per request
  streaming/
    mod.rs                          [~35 lines]  re-exports
    sse.rs                          [~200 lines] SseResponseBuilder; write SSE frames; keep-alive ticker; chunked encoding
    event_queue.rs                  [~280 lines] EventQueueWriter trait; EventQueueReader trait; InMemoryEventQueue; EventQueueManager
  push/
    mod.rs                          [~35 lines]  re-exports
    sender.rs                       [~220 lines] PushSender trait; HttpPushSender impl (hyper-backed); retry + timeout
    config_store.rs                 [~200 lines] PushConfigStore trait; InMemoryPushConfigStore impl
  store/
    mod.rs                          [~30 lines]  re-exports
    task_store.rs                   [~280 lines] TaskStore trait; InMemoryTaskStore impl (DashMap); snapshot + versioning
```

### `crates/a2a-sdk/`

```
Cargo.toml                          [~40 lines]  re-exports all workspace crates
src/
  lib.rs                            [~80 lines]  #![doc = "..."] + pub use a2a_types::*; pub use a2a_client::*; pub use a2a_server::*
```

### `examples/`

```
echo-agent/
  Cargo.toml                        [~20 lines]
  src/main.rs                       [~180 lines] minimal echo agent: returns input as text artifact

client-demo/
  Cargo.toml                        [~20 lines]
  src/main.rs                       [~200 lines] discover card, send message, poll task, print result

streaming-agent/
  Cargo.toml                        [~20 lines]
  src/main.rs                       [~250 lines] streaming agent + streaming client; shows SSE round-trip
```

---

## 6. Implementation Phases

### Phase 0 — Project Foundation

**Deliverables:** Compilable workspace, CI passing green, all tooling configured.

| Task | File(s) | Notes |
|---|---|---|
| Workspace `Cargo.toml` | `Cargo.toml` | `[workspace]` with members array; `[profile.release]` with `panic="abort"`, `lto=true`, `opt-level=3`, `codegen-units=1`, `strip=true` |
| Rust toolchain pin | `rust-toolchain.toml` | `channel = "stable"`, components = `["rustfmt", "clippy"]` |
| `.gitignore` | `.gitignore` | `/target`, `Cargo.lock` (for libs), `.env` |
| `deny.toml` | `deny.toml` | Apache-2.0 license list; advisory DB; no git sources; bounded versions |
| `clippy.toml` | `clippy.toml` | Deny: `clippy::all`, `clippy::pedantic`, `clippy::nursery`; allow: `module_name_repetitions` |
| CI pipeline | `.github/workflows/ci.yml` | Steps: `fmt --check`, `clippy -D warnings`, `test --all-targets`, `doc --no-deps`, `cargo-deny` |
| README skeleton | `README.md` | Project overview, quick-start placeholder, status badge |
| LESSONS skeleton | `LESSONS.md` | Headers for pitfall categories; fill as dev proceeds |
| Empty crate stubs | Each `crates/*/Cargo.toml` + `src/lib.rs` | Compile-clean empty crates before any implementation |

**Validation:** `cargo build --workspace` exits 0. `cargo test --workspace` exits 0 (no tests yet). CI runs green.

---

### Phase 1 — Protocol Types (`a2a-types`)

**Deliverables:** Complete, serialization-correct Rust types for every A2A 0.3.0 schema.

**Order of implementation (dependency-driven):**

#### 1a. `error.rs` — Error types first (everything depends on these)

```rust
// All A2A protocol error codes as typed variants
pub enum ErrorCode {
    // JSON-RPC standard
    ParseError       = -32700,
    InvalidRequest   = -32600,
    MethodNotFound   = -32601,
    InvalidParams    = -32602,
    InternalError    = -32603,
    // A2A-specific (-32000 to -32099)
    TaskNotFound                    = -32001,
    TaskNotCancelable               = -32002,
    PushNotificationNotSupported    = -32003,
    UnsupportedOperation            = -32004,
    ContentTypeNotSupported         = -32005,
    VersionNotSupported             = -32006,
    ExtensionSupportRequired        = -32007,
    InvalidMessage                  = -32008,
    AuthenticationFailed            = -32009,
    AuthorizationFailed             = -32010,
    UnsupportedMode                 = -32011,
}

pub struct A2aError {
    pub code: ErrorCode,
    pub message: String,
    pub data: Option<serde_json::Value>,
}

pub type A2aResult<T> = Result<T, A2aError>;
```

`A2aError` implements: `std::error::Error`, `Display`, `Debug`, `Clone`.
Constructor methods: `A2aError::task_not_found(id)`, `A2aError::internal(msg)`, etc.

#### 1b. `jsonrpc.rs` — JSON-RPC envelope types

```rust
pub type JsonRpcId = Option<serde_json::Value>; // string | number | null

pub struct JsonRpcRequest {
    pub jsonrpc: JsonRpcVersion,   // always "2.0"
    pub id: JsonRpcId,
    pub method: String,
    pub params: Option<serde_json::Value>,
}

#[serde(untagged)]
pub enum JsonRpcResponse<T> {
    Success(JsonRpcSuccessResponse<T>),
    Error(JsonRpcErrorResponse),
}

pub struct JsonRpcSuccessResponse<T> {
    pub jsonrpc: JsonRpcVersion,
    pub id: JsonRpcId,
    pub result: T,
}

pub struct JsonRpcErrorResponse {
    pub jsonrpc: JsonRpcVersion,
    pub id: JsonRpcId,
    pub error: JsonRpcError,
}

pub struct JsonRpcError {
    pub code: i64,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}
```

#### 1c. `task.rs` — Task, TaskStatus, TaskState

```rust
// Newtype for compile-time ID distinctness
pub struct TaskId(String);
pub struct TaskVersion(u64);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TaskState {
    Submitted,
    Working,
    InputRequired,
    AuthRequired,
    Completed,
    Failed,
    Canceled,
    Rejected,
    Unknown,
}

impl TaskState {
    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Canceled | Self::Rejected)
    }
}

pub struct Task {
    pub id: TaskId,
    pub context_id: String,
    pub status: TaskStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub history: Option<Vec<Message>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub artifacts: Option<Vec<Artifact>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
    pub kind: TaskKind, // always "task"
}
```

#### 1d. `message.rs` — Message, Part variants

Serde discriminated union using `#[serde(tag = "kind", rename_all = "lowercase")]`:

```rust
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Part {
    Text(TextPart),
    File(FilePart),
    Data(DataPart),
}

pub struct TextPart {
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
}

pub struct FilePart {
    pub file: FileContent,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
}

// Untagged: deserializes as Bytes if `bytes` key present, Uri if `uri` key present
#[serde(untagged)]
pub enum FileContent {
    Bytes(FileWithBytes),
    Uri(FileWithUri),
}
```

#### 1e. `artifact.rs`, `agent_card.rs`, `security.rs`, `events.rs`, `params.rs`, `push.rs`, `extensions.rs`, `responses.rs`

Each implements the corresponding spec types (see `docs/implementation/type-mapping.md` for full field-by-field mapping).

**Key serde patterns used:**
- `#[serde(rename_all = "camelCase")]` — all structs (spec uses camelCase)
- `#[serde(skip_serializing_if = "Option::is_none")]` — all optional fields
- `#[serde(tag = "kind")]` — discriminated unions (Part, StreamResponse)
- `#[serde(untagged)]` — ambiguous unions (FileContent, JsonRpcResponse, SendMessageResponse)
- `#[serde(rename = "...")]` — where Rust field name cannot match spec name

**Validation:** `cargo test -p a2a-types` passes. Round-trip JSON tests for all types. Corpus of canonical JSON samples from the spec tested for exact deserialization.

---

### Phase 2 — HTTP Client (`a2a-client`)

**Deliverables:** Working client that can communicate with any A2A-compliant agent over JSON-RPC.

**Order of implementation:**

#### 2a. `error.rs` + `config.rs`

```rust
pub enum ClientError {
    Http(hyper::Error),
    Serialization(serde_json::Error),
    Protocol(A2aError),
    Transport(String),
    InvalidEndpoint(String),
    AuthRequired { task_id: TaskId },
}
pub type ClientResult<T> = Result<T, ClientError>;

pub struct ClientConfig {
    pub preferred_transports: Vec<TransportProtocol>,
    pub accepted_output_modes: Vec<String>,
    pub history_length: Option<u32>,
    pub return_immediately: bool,
    pub request_timeout: Duration,
    pub tls: TlsConfig,
}
```

#### 2b. `transport/mod.rs` — Transport trait

```rust
pub trait Transport: Send + Sync + 'static {
    async fn send_request(
        &self,
        method: &str,
        params: serde_json::Value,
    ) -> ClientResult<serde_json::Value>;

    async fn send_streaming_request(
        &self,
        method: &str,
        params: serde_json::Value,
    ) -> ClientResult<Box<dyn EventStreamSource>>;
}
```

#### 2c. `transport/jsonrpc.rs` — JSON-RPC over HTTP

- Build `JsonRpcRequest` with generated ID
- POST to agent's `url` (from AgentCard)
- Parse `JsonRpcResponse<serde_json::Value>`
- Map `JsonRpcError` → `ClientError::Protocol(A2aError)`
- Streaming: request with `Accept: text/event-stream`, return SSE reader

#### 2d. `streaming/sse_parser.rs` + `streaming/event_stream.rs`

SSE parser is a state machine operating on raw bytes:
- Accumulates lines into frames
- Parses `data:`, `event:`, `id:`, `retry:` fields
- Handles `:` keep-alive comments
- Returns `SseFrame { event_type, data, id, retry }`

`EventStream` wraps the SSE parser, deserializes `StreamResponse` from each data field:
```rust
pub struct EventStream {
    inner: SseFrameStream,
}

impl EventStream {
    pub async fn next(&mut self) -> Option<ClientResult<StreamResponse>>;
}
```

#### 2e. `discovery.rs`

```rust
pub async fn resolve_agent_card(base_url: &str) -> ClientResult<AgentCard>;
pub async fn resolve_agent_card_with_path(url: &str, path: &str) -> ClientResult<AgentCard>;
```

Fetches `/.well-known/agent-card.json`, deserializes `AgentCard`, validates `protocolVersion`.

#### 2f. `interceptor.rs` + `auth.rs`

```rust
pub trait CallInterceptor: Send + Sync + 'static {
    async fn before(&self, req: &mut ClientRequest) -> ClientResult<()>;
    async fn after(&self, resp: &ClientResponse) -> ClientResult<()>;
}

pub struct AuthInterceptor { ... }
impl CallInterceptor for AuthInterceptor { ... }

pub trait CredentialsStore: Send + Sync + 'static {
    fn get(&self, session: &SessionId, scheme: &str) -> Option<String>;
    fn set(&self, session: SessionId, scheme: &str, credential: String);
    fn remove(&self, session: &SessionId, scheme: &str);
}
pub struct InMemoryCredentialsStore(RwLock<HashMap<SessionId, HashMap<String, String>>>);
```

#### 2g. `builder.rs` + `client.rs`

`ClientBuilder` validates: at least one transport protocol, valid base URL, no conflicting TLS options.

`A2aClient` holds a `Transport` impl + `InterceptorChain` + `ClientConfig`. Each method:
1. Builds `MessageSendParams` / `TaskQueryParams` etc.
2. Runs `interceptor.before()`
3. Calls `transport.send_request()` or `transport.send_streaming_request()`
4. Deserializes result
5. Runs `interceptor.after()`
6. Returns typed result

#### 2h. `methods/` — All client methods

Each file implements 1–3 closely related RPC calls:

| File | Methods |
|---|---|
| `send_message.rs` | `send_message()`, `stream_message()` |
| `tasks.rs` | `get_task()`, `list_tasks()`, `cancel_task()`, `resubscribe()` |
| `push_config.rs` | `set_push_config()`, `get_push_config()`, `list_push_configs()`, `delete_push_config()` |
| `extended_card.rs` | `get_authenticated_extended_card()` |

**Validation:** `cargo test -p a2a-client` with `wiremock` mocking. All 11 RPC methods exercised. Auth flow tested. SSE streaming tested with mock SSE server.

---

### Phase 3 — Server Framework (`a2a-server`)

**Deliverables:** A working `AgentExecutor`-based server that can be driven by any hyper HTTP server.

**Order of implementation:**

#### 3a. `error.rs` + `executor.rs`

```rust
pub trait AgentExecutor: Send + Sync + 'static {
    async fn execute(
        &self,
        ctx: &RequestContext,
        queue: &dyn EventQueueWriter,
    ) -> A2aResult<()>;

    async fn cancel(
        &self,
        ctx: &RequestContext,
        queue: &dyn EventQueueWriter,
    ) -> A2aResult<()>;
}
```

#### 3b. `request_context.rs`

```rust
pub struct RequestContext {
    pub message: Message,
    pub task_id: TaskId,
    pub context_id: String,
    pub stored_task: Option<Task>,
    pub metadata: Option<serde_json::Value>,
}
```

Constructed by `RequestHandler` before calling `AgentExecutor::execute`. The `stored_task` field carries the previous task snapshot for continuation requests.

#### 3c. `store/task_store.rs`

```rust
pub trait TaskStore: Send + Sync + 'static {
    async fn save(&self, task: Task) -> A2aResult<()>;
    async fn get(&self, id: &TaskId) -> A2aResult<Option<Task>>;
    async fn list(&self, params: &ListTasksParams) -> A2aResult<TaskListResponse>;
    async fn delete(&self, id: &TaskId) -> A2aResult<()>;
}

// Default impl; no external dep; suitable for single-process deployments
pub struct InMemoryTaskStore {
    tasks: RwLock<HashMap<TaskId, Task>>,
}
```

#### 3d. `streaming/event_queue.rs`

```rust
pub trait EventQueueWriter: Send + Sync {
    async fn write(&self, event: StreamResponse) -> A2aResult<()>;
    async fn close(&self) -> A2aResult<()>;
}

pub trait EventQueueReader: Send {
    async fn read(&mut self) -> Option<A2aResult<StreamResponse>>;
}

// Backed by tokio::sync::mpsc channel
pub struct InMemoryEventQueue { ... }
```

#### 3e. `handler.rs` + `builder.rs`

`RequestHandlerBuilder` fluent API:
```rust
RequestHandlerBuilder::new(executor)
    .with_task_store(custom_store)
    .with_push_sender(http_push_sender)
    .with_push_config_store(custom_store)
    .with_interceptor(logging_interceptor)
    .with_agent_card(card)
    .build()  // → RequestHandler<E>
```

`RequestHandler::on_send_message` flow:
1. Generate `TaskId` (uuid v4), `ContextId` (uuid v4 if not provided)
2. Create initial `Task { state: Submitted }`
3. Save to `TaskStore`
4. Create `InMemoryEventQueue`
5. Spawn `tokio::task` → calls `executor.execute(ctx, &queue)`
6. If `return_immediately=true`: return `Task { state: Submitted }`
7. Else: poll queue for first terminal event, return final `Task`

#### 3f. `dispatch/jsonrpc.rs` + `dispatch/rest.rs`

JSON-RPC dispatcher:
- Parse `JsonRpcRequest` from HTTP body bytes
- Match `method` string to handler method
- Call appropriate `RequestHandler` method
- Serialize response as `JsonRpcResponse<T>`
- Streaming: return SSE response for `message/stream` and `tasks/resubscribe`

REST dispatcher:
- Parse HTTP method + path pattern
- Extract path parameters (`{taskId}`, etc.)
- Call appropriate `RequestHandler` method
- Return JSON response body

Both dispatchers map `A2aError` → `JsonRpcErrorResponse` with correct error codes.

#### 3g. `agent_card/`

```rust
// Static: card provided at construction time
pub struct StaticAgentCardHandler { card_json: Bytes }

// Dynamic: card generated per-request
pub trait AgentCardProducer: Send + Sync + 'static {
    async fn produce(&self, req: &CardRequest) -> A2aResult<AgentCard>;
}
pub struct DynamicAgentCardHandler<P: AgentCardProducer> { producer: P }
```

Both handlers:
- Respond to `GET /.well-known/agent-card.json`
- Set `Content-Type: application/json`
- Set CORS header `Access-Control-Allow-Origin: *`

#### 3h. `interceptor.rs` + `call_context.rs`

Server-side interceptors run before/after each RPC call, with access to `CallContext` (caller identity, activated extensions, method name).

**Validation:** Integration tests spin up a real hyper server, send JSON-RPC requests, verify responses. `AgentExecutor` implemented as a test double. All 11 methods tested via JSON-RPC and REST dispatchers.

---

### Phase 4 — SSE Streaming

**Deliverables:** Fully functional bidirectional SSE: client consumes events, server produces them.

This phase integrates streaming support added in stubs during Phases 2 and 3.

| Task | Location | Notes |
|---|---|---|
| Server SSE writer | `a2a-server/src/streaming/sse.rs` | Chunked transfer; keep-alive `:` comments; `final: true` detection closes stream |
| Client SSE parser | `a2a-client/src/streaming/sse_parser.rs` | Handles fragmented chunks; reconnect hint via `retry:` field |
| `message/stream` end-to-end | tests | Client calls `stream_message()`, server emits `TaskStatusUpdateEvent` series, client receives all |
| `tasks/resubscribe` | both | Client reconnects after disconnect; server replays pending events |
| EventQueueManager | `a2a-server/src/streaming/event_queue.rs` | `get_or_create(task_id)` + `destroy(task_id)` lifecycle |

**Keep-alive design:** Server sends `: keep-alive\n\n` every 30s (configurable). Client's SSE parser ignores comment lines per spec.

**Backpressure:** `InMemoryEventQueue` uses bounded `tokio::sync::mpsc` channel (default: 64 events). If the queue fills, `EventQueueWriter::write` returns `A2aError::Internal` and the executor is expected to yield.

---

### Phase 5 — Push Notifications

**Deliverables:** Agent can deliver push notifications to client-registered webhooks.

| Task | Location | Notes |
|---|---|---|
| `PushConfigStore` trait | `a2a-server/src/push/config_store.rs` | CRUD for `TaskPushNotificationConfig`; keyed by `(TaskId, config_id)` |
| `InMemoryPushConfigStore` | same file | `RwLock<HashMap<(TaskId, String), PushNotificationConfig>>` |
| `PushSender` trait | `a2a-server/src/push/sender.rs` | `async fn send(&self, url: &str, event: &StreamResponse, config: &PushNotificationConfig)` |
| `HttpPushSender` | same file | hyper POST with `Content-Type: application/json`; 30s timeout; honor auth config; optional `A2A-Notification-Token` header |
| Server integration | `handler.rs` | After each `EventQueueWriter::write`, if push configs exist for `task_id`, call `PushSender::send` |
| Push config RPC methods | `handler.rs` | `on_set_push_config`, `on_get_push_config`, `on_list_push_configs`, `on_delete_push_config` |

**Security:** `HttpPushSender` inspects `PushNotificationAuthInfo.schemes`:
- `"bearer"` → `Authorization: Bearer {credentials}`
- `"basic"` → `Authorization: Basic {base64(credentials)}`
- Token verification header sent when `PushNotificationConfig.token` is set

---

### Phase 6 — Umbrella Crate & Examples

**Deliverables:** `a2a-sdk` re-export crate; three working examples.

#### `crates/a2a-sdk/src/lib.rs`

```rust
//! # a2a-sdk
//!
//! All-in-one re-export of `a2a-types`, `a2a-client`, and `a2a-server`.
//! For lower compile times, depend on individual crates directly.

pub use a2a_types::*;
pub use a2a_client::{A2aClient, ClientBuilder, ClientConfig, ClientError};
pub use a2a_server::{AgentExecutor, RequestHandler, RequestHandlerBuilder, ServerError};
```

#### `examples/echo-agent/src/main.rs`

Minimal server that echoes each message as a text artifact. Demonstrates:
- `AgentExecutor` implementation
- `RequestHandlerBuilder` construction
- Starting a hyper server
- Serving the AgentCard at `/.well-known/agent-card.json`

#### `examples/client-demo/src/main.rs`

Client that:
1. Discovers the echo agent's card
2. Sends a message
3. Polls until task is terminal
4. Prints the artifact text

#### `examples/streaming-agent/src/main.rs`

Agent that emits multiple `TaskStatusUpdateEvent` and `TaskArtifactUpdateEvent` during processing. Client connects with `stream_message()` and prints each event as it arrives.

---

### Phase 7 — Quality & Release Preparation

**Deliverables:** All quality gates passing, documentation complete, crates publishable.

| Task | Notes |
|---|---|
| Property-based tests | `proptest` for `TaskState` transitions, `Part` round-trip, ID generation uniqueness |
| Corpus-based JSON tests | Deserialize every sample from official spec; verify round-trip |
| E2E integration tests | Echo agent + client test in the same process; streaming test; push notification test with mock webhook |
| Benchmark suite | `criterion`: JSON serialization throughput; SSE parse throughput; server handler throughput |
| `cargo doc --no-deps` | Zero warnings; all public items documented; examples compile |
| `LESSONS.md` | Document every non-obvious pitfall discovered during implementation |
| ADR finalization | Each ADR updated with implementation outcomes |
| `CONTRIBUTING.md` | Testing guide, coding standards, PR checklist |
| Publish dry-run | `cargo publish --dry-run -p a2a-types`, etc. — verify no missing files |
| Version alignment | All crates at `0.1.0`; workspace `[package.version]` |

---

## 7. Testing Strategy

### Four Layers (non-negotiable)

| Layer | Tool | Location | Purpose |
|---|---|---|---|
| **Unit** | `#[test]` in-source | Each `src/*.rs` file | Pure logic: serde round-trips, state machine transitions, error construction, ID uniqueness |
| **Property-based** | `proptest` | `src/*.rs` | Invariants: serialization idempotence, terminal state irreversibility, error code stability |
| **Integration** | `#[tokio::test]` in `tests/` | Each crate's `tests/` dir | Module boundaries: client ↔ mock server, server handler ↔ mock executor |
| **E2E** | `#[tokio::test]` | `tests/e2e/` | Full round-trip: real client + real server in-process; all transports; streaming; push |

### Test Naming Convention

`{component}_{scenario}_{expected_outcome}`

Examples:
- `task_state_completed_is_terminal`
- `text_part_roundtrip_preserves_metadata`
- `client_send_message_returns_task_on_success`
- `server_jsonrpc_unknown_method_returns_method_not_found`
- `sse_parser_keep_alive_comment_is_ignored`
- `push_sender_bearer_auth_sets_authorization_header`

### Test Organization

Each source file's tests live in its own `#[cfg(test)]` block. Section banners:
```rust
// ---------------------------------------------------------------------------
// serialization tests
// ---------------------------------------------------------------------------
```

Integration and E2E tests live in `crates/{crate}/tests/`.

### Non-Negotiable Rules

1. `cargo test --workspace` must pass before any commit to `main`.
2. E2E tests MUST be written for every transport path (JSON-RPC + REST), streaming, push notifications.
3. No test may use `unwrap()` without a comment explaining why it cannot fail.
4. All async tests use `#[tokio::test]` (not `tokio::block_on` in non-async tests).

---

## 8. Quality Gates

All gates must pass before tagging a release. CI enforces all of them.

```bash
# Formatting (zero diffs allowed)
cargo fmt --all -- --check

# Linting (zero warnings allowed)
cargo clippy --workspace --all-targets -- -D warnings

# Tests (all must pass)
cargo test --workspace --all-targets

# Documentation (zero warnings)
cargo doc --workspace --no-deps

# MSRV check (Rust 1.93.x)
rustup run 1.93 cargo check --workspace

# Supply chain (license + advisory + duplicate check)
cargo deny check

# Publish dry-run
cargo publish --dry-run -p a2a-types
cargo publish --dry-run -p a2a-client
cargo publish --dry-run -p a2a-server
cargo publish --dry-run -p a2a-sdk
```

### Release Profile Requirements

```toml
[profile.release]
panic        = "abort"
lto          = true
opt-level    = 3
codegen-units = 1
strip        = true
```

---

## 9. Coding Standards

Derived from `quack-rs` and applied to this project.

### File-Level

- **SPDX header on every file:**
  ```
  // SPDX-License-Identifier: Apache-2.0
  // Copyright 2026 Tom F.
  ```
- **500-line maximum.** When a file approaches 400 lines, extract a submodule.
- **Thin `mod.rs` files** (35–70 lines): module declarations + `pub use` re-exports only. No logic.

### Error Handling

- `unwrap()` and `expect()` are **forbidden** in library code.
- `unwrap()` in tests requires a comment: `// SAFETY: test data is hardcoded valid`.
- `?` operator is the standard propagation mechanism.
- `panic!()` is forbidden except in `unreachable!()` for provably exhaustive matches.

### Async

- `async fn` in traits (stable Rust 1.75+) — no `Pin<Box<dyn Future>>` boxing.
- All I/O-bound functions are async. No `std::thread::sleep` or blocking calls in async context.
- `tokio::task::spawn_blocking` for CPU-bound work that cannot be made async.

### Unsafe

- `unsafe` blocks are **forbidden** unless crossing true FFI or `unsafe` hyper internals.
- Every `unsafe` block requires a `// SAFETY:` comment explaining the upheld invariants.
- `#![deny(unsafe_op_in_unsafe_fn)]` in every crate.

### Documentation

- `#![warn(missing_docs)]` in every crate.
- Module-level docs: purpose → key types → usage example.
- Public struct/enum docs: what it represents, which spec section defines it.
- Cross-references: `[TypeName]`, `[method_name()]`.

### Imports

- Group: `std` → external crates → `crate` / `super`.
- No wildcard imports except in `prelude` modules and test `use super::*`.

---

## 10. Protocol Reference Summary

A condensed quick-reference for implementation use.

### All RPC Methods

| Method | Transport | Params | Returns |
|---|---|---|---|
| `message/send` | JSON-RPC POST | `MessageSendParams` | `Task \| Message` |
| `message/stream` | JSON-RPC POST → SSE | `MessageSendParams` | `EventStream` |
| `tasks/get` | JSON-RPC POST | `TaskQueryParams` | `Task` |
| `tasks/list` | JSON-RPC POST | `ListTasksParams` | `TaskListResponse` |
| `tasks/cancel` | JSON-RPC POST | `TaskIdParams` | `Task` |
| `tasks/resubscribe` | JSON-RPC POST → SSE | `TaskIdParams` | `EventStream` |
| `tasks/pushNotificationConfig/set` | JSON-RPC POST | `TaskPushNotificationConfig` | `TaskPushNotificationConfig` |
| `tasks/pushNotificationConfig/get` | JSON-RPC POST | `GetPushConfigParams` | `TaskPushNotificationConfig` |
| `tasks/pushNotificationConfig/list` | JSON-RPC POST | `TaskIdParams` | `Vec<TaskPushNotificationConfig>` |
| `tasks/pushNotificationConfig/delete` | JSON-RPC POST | `DeletePushConfigParams` | `null` |
| `agent/authenticatedExtendedCard` | JSON-RPC POST | `AuthenticatedExtendedCardParams` | `AuthenticatedExtendedCardResponse` |

### SSE Event Types

| `kind` value | Rust type | When emitted |
|---|---|---|
| `"task"` | `Task` | When `message/send` returns a full task immediately |
| `"message"` | `Message` | When agent response is a single message, not a task |
| `"status-update"` | `TaskStatusUpdateEvent` | On every task state transition during streaming |
| `"artifact-update"` | `TaskArtifactUpdateEvent` | When an artifact is ready or appended |

### Terminal Task States

`completed`, `failed`, `canceled`, `rejected` — no further state transitions possible.

### Error Code Quick-Reference

| Code | Name | When to use |
|---|---|---|
| -32700 | ParseError | Malformed JSON body |
| -32600 | InvalidRequest | Missing `jsonrpc`/`method`/`id` fields |
| -32601 | MethodNotFound | Unknown method name |
| -32602 | InvalidParams | Params don't match expected schema |
| -32603 | InternalError | Unexpected server error |
| -32001 | TaskNotFound | `tasks/get` or `tasks/cancel` with unknown ID |
| -32002 | TaskNotCancelable | Cancel requested for terminal task |
| -32003 | PushNotificationNotSupported | Client requests push; server `capabilities.pushNotifications: false` |
| -32004 | UnsupportedOperation | Method exists but not implemented by this agent |
| -32005 | ContentTypeNotSupported | Requested MIME type not in `defaultOutputModes` |
| -32006 | VersionNotSupported | Client/server `A2A-Version` header mismatch |
| -32007 | ExtensionSupportRequired | `AgentExtension.required: true` and client lacks it |
| -32008 | InvalidMessage | Message structure violates protocol constraints |
| -32009 | AuthenticationFailed | Credentials missing or invalid |
| -32010 | AuthorizationFailed | Credentials valid but insufficient permissions |
| -32011 | UnsupportedMode | Requested transport/streaming mode unsupported |

### AgentCard Well-Known URI

```
GET https://{host}/.well-known/agent-card.json
Response: application/json
CORS: Access-Control-Allow-Origin: *
```

### HTTP Headers

| Header | Direction | Purpose |
|---|---|---|
| `A2A-Version` | Both | Protocol version negotiation |
| `A2A-Extensions` | Both | Comma-separated declared extension URIs |
| `Authorization` | Client→Server | Bearer/Basic auth |
| `A2A-Notification-Token` | Server→Client (push) | Shared secret for webhook verification |
| `Content-Type: application/json` | Both | Standard JSON-RPC |
| `Content-Type: text/event-stream` | Server→Client | SSE streaming response |
| `Cache-Control: no-cache` | Server→Client (SSE) | Prevent SSE caching |

---

*Document version: 1.0 — initial plan*
*Last updated: 2026-03-15*
*Author: Tom F.*
