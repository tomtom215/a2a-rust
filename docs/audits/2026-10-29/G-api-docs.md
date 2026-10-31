# Audit G: API ergonomics, footguns, best practice and documentation drift

Repository: `/home/user/a2a-rust` at `8dc4331`. Every crate is at **0.15.1**, released 2026-10-26; `CITATION.cff:17-18` agrees. So "v0.15.0 public" is one patch behind.

Method: I read the code and docs and grepped them. I ran no `cargo` command, so nothing here was compiled or run. Each finding says **[read]** when I verified it by reading code or docs. It says **[inferred]** when it rests on documented behaviour of a dependency or of the Rust language that I did not run.

Severity scale:
- **H**: silently does the wrong thing, or blocks a common use case.
- **M**: a real footgun or a confusing API.
- **L**: polish.

---

## Summary table

| ID | Sev | Title |
|---|---|---|
| G-01 | H | `ClientBuilder::without_tls()` / `ClientConfig::tls` do nothing, and HTTP transports cannot take a private CA |
| G-02 | H | Config values that `build()` accepts and that later panic or hang at runtime (`with_event_queue_capacity(0)`, `with_max_connections(0 / usize::MAX)`) |
| G-03 | H | A `ServeConfig::production()` refusal makes the process exit 0 silently; `ServeReport` is not `#[must_use]` |
| G-04 | M | `build_grpc()` ignores the interface chosen from the card and dials the JSON-RPC URL |
| G-05 | M | 19 example files hand-roll the accept loop that the SDK documents as a fixed defect |
| G-06 | M | Default features: the SDK pulls in the OTLP exporter and tonic for "hello"; the server crate alone cannot push to `https://` |
| G-07 | M | `AgentCard::new` / `AgentSkill::new` produce cards that `build()` refuses; streaming is off by default |
| G-08 | M | `agent_executor!` bodies cannot use `self`, so only stateless agents fit the advertised fast path |
| G-09 | M | Too many ways to serve; `ServeConfig` does not apply to gRPC, WebSocket or `A2aRouter` |
| G-10 | M | Wire structs are not `#[non_exhaustive]`, and examples teach struct literals |
| G-11 | M | Configuration errors come back as `ServerError::InvalidParams` / `ClientError::Transport` |
| G-12 | M | `EventStream` does not implement `futures::Stream` |
| G-13 | M | No message-id generator; the docs teach a hard-coded `"m1"` or adding `uuid` |
| G-14 | M | `Telemetry`: a drop guard without `#[must_use]`; exports to localhost by default; two setup APIs |
| G-15 | L-M | Naming inconsistencies across the API |
| G-16 | L | `EventEmitter::artifact(.., None, Some(true))` has two positional `Option<bool>` arguments |
| G-17 | L | The `Arc` boilerplate in every snippet |
| G-18 | L | `RequestContext.context_id: String` but `task_id: TaskId` |
| G-19 | L | Order-dependent builder calls (`with_tenant` after `with_protocol_binding`) |
| G-20 | L | Client 30 s timeout against server 1 h executor timeout; "request timed out" says nothing actionable |
| G-21 | L | An executor returning `Ok(())` without a terminal state leaves the task non-terminal, silently |
| G-22 | L | `SendMessageResponse` has no accessors |
| G-23 | L | Missing `Debug` (`EventEmitter`, `A2aRouter`); `mutants` is a normal dependency; pub fields beside clamping setters |
| D-01 | M | Docs: `without_tls()` "Disables TLS" (2 book pages) |
| D-02 | M | Server README: `serve` has "no idle timeout" (stale) |
| D-03 | L | `ServeConfig::with_max_connections` rustdoc predicts fields that already exist |
| D-04 | M | `packaging/README.md`: "no signing key committed" (one is committed) |
| D-05 | M | `tools/a2a-cli/README.md`: exit code 2 and `--header` repeatability |
| D-06 | L | Book graceful-shutdown page: gRPC "returns the same `ServeReport`" |
| D-07 | L | Book Axum page: its hidden card advertises JSON-RPC for a REST router |
| D-08 | L | Server crate quick start step 3 is outdated |
| D-09 | L | `is_retryable` doc says "transport errors" are retryable; `Transport` is not |
| D-10 | L | The hello-agent "one screen / no feature flags" framing |
| D-11 | L | Prelude rustdoc list is out of sync with the actual re-exports |
| C-01 | M | CHANGELOG size and readability |
| C-02 | M | 0.15.1, a patch, contained behaviour changes that break existing code |
| C-03 | L | Doctest coverage: what is not compiled |

---

## Findings: API and footguns

### G-01 [H] `without_tls()` is a no-op, and JSON-RPC/REST/discovery cannot trust a private CA [read]

**Where:**
- `crates/a2a-protocol-client/src/builder/mod.rs:506-511`
- `config.rs:239-240, 429-434`
- `transport/jsonrpc.rs:139-143`
- `transport/rest/mod.rs:165-168`
- `discovery.rs:576-577`
- `tls.rs:60-108`

**Evidence:**

```rust
/// Disables TLS (plain HTTP only).
pub const fn without_tls(mut self) -> Self { self.config.tls = TlsConfig::Disabled; self }
```

`ClientConfig::tls` is never read. A grep for `config.tls` and `.tls` across `crates/a2a-protocol-client/src` finds only the builder's write and the WebSocket transport's own, separate config.

Both HTTP transports always build their client like this:

```rust
#[cfg(feature = "tls-rustls")]
let client = crate::tls::build_https_client_with_connect_timeout(
    crate::tls::default_tls_config(), connection_timeout);
```

Card discovery uses `crate::tls::build_https_client()` (`discovery.rs:577`). Both use the Mozilla `webpki_roots` only.

The public helpers `tls::tls_config_with_extra_roots` and `tls::build_https_client_with_config` return a `rustls::ClientConfig` and an `HttpsClient` respectively. No `JsonRpcTransport`, `RestTransport`, `ClientBuilder` or `CardFetchOptions` method accepts either. `tls_config_with_extra_roots` also drops a certificate it cannot add, with only a `trace_warn!`.

**Consequence:**
- `without_tls()` still dials `https://`.
- An enterprise user with an internal CA cannot reach an internal agent over JSON-RPC or HTTP+JSON at all, nor fetch its card.
- gRPC (`with_grpc_tls_config`) and WebSocket (`with_tls_config`) can, so a user only finds this out after choosing the HTTP bindings.
- The OS trust store is never consulted.

**Fix:**
- Either implement `TlsConfig` by adding `TlsConfig::Custom(Arc<rustls::ClientConfig>)` and threading it through `http_transport()` and discovery, or delete the field and the method.
- Add `ClientBuilder::with_tls_config(rustls::ClientConfig)` and `CardFetchOptions::with_tls_config`.
- Consider a `rustls-native-certs` option.
- Fix the two book rows (D-01).

---

### G-02 [H] `build()` accepts values that panic or hang at runtime [read code; tokio panic conditions inferred from tokio's docs]

**1. Event-queue capacity of zero.**

- `builder.rs:679-683`: `with_event_queue_capacity(capacity: usize)` stores any value. `build()` (`builder.rs:1059-1215`) validates the executor timeout, `max_id_length`, `max_metadata_size` and `push_delivery_timeout`, but not this.
- The value reaches `EventQueueManager::with_capacity` (`builder.rs:1465-1467`), then `broadcast::channel(capacity)` (`streaming/event_queue/mod.rs:230, 277, 309`).
- `tokio::sync::broadcast::channel` panics when capacity is 0 or greater than `usize::MAX / 2`.
- The same zero path exists per tenant: `TenantLimitsBuilder::event_queue_capacity(0)` (`tenant_config/mod.rs:232`), used at `handler/messaging/admission.rs:161-163`.

**Consequence:** `build()` returns `Ok`, and the first `SendMessage` panics inside a spawned task.

**2. Connection ceiling.**

- `serve/graceful/mod.rs:314-316`: `Semaphore::new(config.max_connections.unwrap_or(Semaphore::MAX_PERMITS))`.
- `ServeConfig::with_max_connections(usize::MAX)` makes `Semaphore::new` panic, since tokio panics when permits exceed `MAX_PERMITS`, which is `usize::MAX >> 3`.
- `with_max_connections(0)` leaves a server that never accepts and never says why.

**3. Other values accepted without a check:**
- `with_max_concurrent_streams(0)` refuses every send as "overloaded".
- `with_max_event_size(0)` rejects every event.
- An `idle_timeout` below `sse_keep_alive_interval`: `serve/graceful/config.rs:38-43` documents this as how a deployment "starts dropping idle subscribers", but nothing checks it.

**Fix:**
- Validate in `RequestHandlerBuilder::build()`, `PerTenantConfig` and `Server::with_config`, or make `with_max_connections` take `NonZeroUsize`.
- Clamp `max_connections` to `Semaphore::MAX_PERMITS`.
- Warn when `idle_timeout <= sse_keep_alive_interval`.

---

### G-03 [H] A production refusal exits 0 silently; `ServeReport` is not `#[must_use]` [read]

**Where:** `serve/graceful/mod.rs:144-165, 283-296`; `examples/hello-agent/src/main.rs:94-96`.

**Evidence:**

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct ServeReport { ... pub refused: Option<String> }
...
if let Some(why) = production::refusal(...) {
    trace_error!(reason = %why, "listener refused to start");
    return ServeReport { accepted: 0, drained: true, abandoned: 0, tasks: None, refused: Some(why) };
}
```

The flagship example discards the report:

```rust
server.serve_with_shutdown(JsonRpcDispatcher::new(handler), shutdown_signal()).await;
```

**Consequence:** copy hello-agent, add `.with_config(ServeConfig::production())`, and deploy on `0.0.0.0` without TLS. The future resolves immediately, `main` returns `Ok(())`, and the process exits 0.

The only signal is a `tracing` error, which is invisible unless a subscriber is installed; the README Quick Start installs none. A refusal reads as "drained: true". By contrast, `ShutdownReport` is `#[must_use]` (`handler/shutdown/mod.rs`); `ServeReport`, `InFlightReport` and `HaltReport` are not.

**Fix:**
- Return `Result<ServeReport, ServeRefused>` from `serve_with_shutdown`, or at least mark `ServeReport` `#[must_use = "check .refused"]`.
- Print the refusal on stderr whether or not tracing is installed.
- Make hello-agent and the book check the report.

---

### G-04 [M] `build_grpc()` ignores card selection [read]

**Where:** `crates/a2a-protocol-client/src/builder/transport_factory.rs:180-235`.

**Evidence:**

```rust
crate::call_span::Peer::new("GRPC", &self.endpoint, self.agent.clone())
...
let t = GrpcTransport::connect_with_config(&self.endpoint, grpc_config)
```

`self.endpoint` is whatever `from_card` chose, which is the card's first interface (`builder/mod.rs:171, 215-235`). `build_grpc` never consults `card_interfaces` or `preferred_binding`. It also does not validate `connection_timeout.is_zero()`, which `build()` does (`transport_factory.rs:51-55`).

**Consequence:**
- For a card listing `[JSONRPC @ :1111, GRPC @ :2222]`, `ClientBuilder::from_card(&card)?.build_grpc().await` dials gRPC at `:1111`. This is the exact bug that `with_protocol_binding`'s own rustdoc (`builder/mod.rs:400-406`) describes as fixed for the other path.
- The user must know to call `.with_protocol_binding("GRPC")` first.
- Three different construction paths exist per binding: `build()`; `build_grpc().await`; and for WebSocket, `WebSocketTransport::connect` followed by `with_custom_transport` (client README "Transport Selection"). `tools/a2a-cli/src/connect.rs:168-181` has to branch three ways.

**Fix:**
- In `build_grpc`, select the card's gRPC interface (endpoint and tenant) when the builder came from a card.
- Better: offer one async `connect()` that picks the right transport for the chosen interface, WebSocket included.

---

### G-05 [M] Examples copy the hand-rolled accept loop the SDK calls a defect [read]

**Evidence:** `listener.accept()` loops appear in 19 non-test example files (`grep -rln 'listener.accept()' examples/*/src`). Five of them swallow accept errors with `Err(_) => continue`:
- `examples/agent-team/src/infrastructure.rs:289, 337`
- `examples/genai-agent/src/main.rs:255`
- `examples/multi-lang-team/src/main.rs:331`
- `examples/rig-agent/src/main.rs:290`

`genai-agent/src/main.rs:245-271` shows the pattern:

```rust
/// Binding before building the handler lets the agent card carry the real address
fn serve(listener: tokio::net::TcpListener, dispatcher: Arc<JsonRpcDispatcher>) {
    tokio::spawn(async move { loop { let (stream, _) = match listener.accept().await { Ok(s) => s, Err(_) => continue }; ...
        hyper_util::server::conn::auto::Builder::new(...).serve_connection(io, service)
```

The SDK's own docs (`serve/mod.rs:121-124`) say:

> "Until 2026-10-18 this ran its own accept loop, which installed no hyper timer ... a peer that sent its headers a byte at a time ... held a task for as long as it liked."

`accept_retry_backoff` (`serve/mod.rs:285-292`) exists precisely so the loop does not busy-spin on EMFILE.

**Consequence:** users copy examples. The copied loop has no header-read or idle timeout, busy-spins on descriptor exhaustion, has no connection cap, and does not end in-flight tasks on shutdown.

The stated reason, learning the port before building the card, is already served by `Server::bind(addr)` or `Server::from_listener` plus `local_addr()`.

The hello-agent test helper also teaches a racy probe-bind-then-drop (`examples/hello-agent/src/main.rs:111-118`).

**Fix:** replace the loops with `Server::from_listener(listener).serve_with_shutdown(..)` and add a CI grep that forbids `listener.accept()` in `examples/*/src` outside webhook sinks.

---

### G-06 [M] Default features surprise in both directions [read]

**SDK default** (`crates/a2a-protocol-sdk/Cargo.toml:30`):

```toml
default = ["tls-rustls", "tracing", "otel", "otlp", "webhook"]
```

Server `otlp` (`crates/a2a-protocol-server/Cargo.toml` `[features]`) pulls in `opentelemetry_sdk`, `opentelemetry-otlp`, `opentelemetry-http`, `opentelemetry-appender-tracing`, `tracing-subscriber`, `async-trait`, `http`, `tonic`, `tower-service`, `hyper-rustls`, `rustls`, `webpki-roots` and `hyper-util/client-proxy`.

So the "no SDK feature flags" hello-agent (README "Hello Agent"; `examples/hello-agent/Cargo.toml:3`) compiles an OTLP exporter and tonic. I did not measure the compile cost; that part is conjecture.

**Server crate alone** (`crates/a2a-protocol-server/Cargo.toml`):

```toml
default = ["tracing", "otel"]
```

It has no `tls-rustls`. The crate's own doc (`push/sender.rs:236-241`) says the `HttpPushSender` then "is plaintext-HTTP only and fails fast on an `https://` target".

The client crate does default to TLS (`a2a-protocol-client/Cargo.toml`: `default = ["tls-rustls", "tracing", "otel", "webhook"]`).

`book/src/getting-started/installation.md:33` and the server README (`a2a-protocol-server = "0.15"`) both recommend plain `a2a-protocol-server = "0.15"` for "server only". That configuration cannot deliver push notifications to any real (`https://`) webhook.

**Fix:**
- Make server `tls-rustls` default, matching the client.
- Consider moving `otlp` out of the SDK defaults; `otel` alone already records through any installed provider. Or document the cost.

---

### G-07 [M] Constructors produce invalid cards; streaming is opt-in [read]

**Where:**
- `crates/a2a-protocol-types/src/agent_card/builders.rs:272-291`: `AgentCard::new` sets `description: String::new()`, `skills: Vec::new()` and `capabilities: AgentCapabilities::none()`.
- `builders.rs:144-156`: `AgentSkill::new` sets `tags: Vec::new()`.
- `agent_card/mod.rs:117-160`: `validate()` refuses an empty description, an empty skills list, and a skill with no tags.
- `handler/capability.rs:41-44`: the server refuses streaming unless `capabilities.streaming == Some(true)`.

**Consequence:**
- `AgentCard::new(name, version, iface)` followed by `RequestHandlerBuilder::...build()` always fails. Every snippet needs `.with_description(..)` plus `.with_skill(AgentSkill::new(..).with_tags([..]))` before anything works.
- `validate()` returns `&'static str` and does not say which skill is at fault ("every agent skill must have at least one tag").
- With hello-agent's card, which has no `.with_streaming(true)`, `stream_message` is refused: the client from a card returns `CapabilityNotDeclared` and the server answers `UnsupportedOperation`. The README Quick Start remembers `.with_streaming(true)`; `examples/hello-agent/src/main.rs:52-61` does not.

**Fix:**
- Take the required items in the constructor, or provide `AgentCard::minimal(name, version, iface, description, skill)`.
- Name the failing index in validation errors.
- Default `streaming` to true. Every executor streams through the event queue regardless, so the SDK can always honour it.

---

### G-08 [M] `agent_executor!` bodies cannot reference `self` [inferred]

**Where:** `crates/a2a-protocol-server/src/executor_helpers.rs:125-178`.

**Evidence:** the macro emits `fn execute<'a>(&'a self, $ctx: ..., $queue: ...) { Box::pin(async move $body) }`. Here `&'a self` is written by the macro and `$body` comes from the caller.

`macro_rules!` treats `self` hygienically, so a `self.field` inside the caller's block resolves against the caller's context. rustc rejects this with E0424, noting that "a macro invocation can only access identifiers it receives from parameters".

Every use in `examples/` and `book/` is a unit struct, and no example uses `self` (`grep -A8 'agent_executor!' examples/*/src/*.rs | grep self` finds nothing). I did not compile a counter-example; that is the source of the [inferred] label.

The macro also cannot express a generic impl, `impl<M> ... for Agent<M>`.

**Consequence:** the prelude advertises the macro as "the shortest path from 'I have a struct' to 'I have an agent'" (`sdk/src/lib.rs:117-120`). The first real agent, one holding a model client or config, falls off it into the full `Pin<Box<dyn Future>>` signature.

**Fix:**
- Add a `$self:ident` binder form: `agent_executor!(MyAgent, |this, ctx, queue| async { this.model... })`.
- Or provide a blanket adapter from an `async fn` trait (`trait Agent { async fn run(&self, ..) }`) to `AgentExecutor`.

---

### G-09 [M] Too many ways to serve, with inconsistent shapes [read]

| Entry point | Signature | Where |
|---|---|---|
| `serve(addr, d)` | free fn, never returns | `serve/mod.rs:181` |
| `serve_with_addr(addr, d)` | detached spawn; no handle, no stop | `serve/mod.rs:245-253` |
| `Server::serve_with_shutdown(self, dispatcher, signal)` | `-> ServeReport` | `serve/graceful/mod.rs:267` |
| `GrpcDispatcher::serve_with_shutdown(self, listener, signal)` | `-> io::Result<ServeReport>` | `dispatch/grpc/shutdown.rs:117-121` |
| `WebSocketDispatcher::serve_with_shutdown(self: Arc<Self>, listener, signal)` | `-> ServeReport` | `dispatch/websocket/shutdown.rs:46-50` |
| `GrpcDispatcher::serve(self, addr)` | | `grpc/dispatcher.rs:215` |
| `WebSocketDispatcher::serve(self: Arc<Self>, addr)` | | `websocket.rs:403` |
| `A2aRouter::into_router()` + `axum::serve` | none of `ServeConfig` | `dispatch/axum_adapter.rs` |

`ServeConfig` applies only to `Server`. The gRPC and WebSocket dispatchers repeat `with_completion_grace`, `with_task_grace` and `with_drain_timeout` on themselves (`grpc/dispatcher.rs:181`, `websocket.rs:367`).

`A2aRouter` cannot be passed to `Server` at all. Axum users get none of the header-read timeout, idle timeout, connection cap or task-ending shutdown, and must hand-wire `finish_in_flight` (`book/src/deployment/production/graceful-shutdown.md:77-81`).

**Fix:**
- One `Server` that takes any binding (implement `Dispatcher`, or a `Listener` abstraction, for gRPC and WebSocket).
- `impl Dispatcher for A2aRouter`, or `A2aRouter::serve(server)`.
- Uniform receivers and return types.

---

### G-10 [M] Wire types are exhaustive structs with all-public fields [read]

**Evidence:** `crates/a2a-protocol-types/src/generated/agent_card.rs:74-76` declares

```rust
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentCard {
```

with no `#[non_exhaustive]`. The same holds for `AgentInterface` (`:16`), `AgentSkill` (`:198`), `Message` (`generated/message.rs:89`) and `Task` (`generated/task.rs:67`); `grep -B4` shows no `non_exhaustive`.

`examples/deploy-agent/src/main.rs:133-170` builds `AgentCard { url: None, name: .., ... }` as a full struct literal, and `AgentSkill { .. }` likewise. `url` is a field that is "accepted on input, never emitted" (`agent_card.rs:136-140`).

**Consequence:** any field the proto gains is a breaking change for every user who copied deploy-agent. The example also teaches setting a dead `url` field.

**Fix:** mark the wire structs `#[non_exhaustive]`, which the builders already make practical. Rewrite deploy-agent with `AgentCard::new(..)`. Deprecate the `url` field.

---

### G-11 [M] Configuration errors masquerade as protocol or transport errors [read]

**Server.** Every `RequestHandlerBuilder::build()` refusal is `ServerError::InvalidParams(String)` (`builder.rs:1059-1206`). That variant is also the JSON-RPC `-32602` request error (`error/mod.rs:30-31`), and it displays as `"invalid params: the agent card is not valid: ..."` (`error/mod.rs:89-90`).

**Client.** `build()` reports zero timeouts as `ClientError::Transport("request_timeout must be non-zero")` (`transport_factory.rs:40-55`). A missing required extension comes back as `ClientError::Protocol(A2aError{ExtensionSupportRequired})` (`:98-118`), although nothing was sent to the agent.

**Consequence:** callers cannot tell "my code is misconfigured" from "the peer refused", in a match or in metrics. The "invalid params" prefix misleads people reading logs.

The messages themselves are good: they say what to change, for example `call with_push_sender(sender), or declare pushNotifications false`.

**Fix:** add `ServerError::Config(String)` and `ClientError::Config(String)`. Both enums are `#[non_exhaustive]`, so this is additive.

---

### G-12 [M] `EventStream` is not a `futures::Stream` [read]

**Where:** `crates/a2a-protocol-client/src/streaming/event_stream.rs:462`.

**Evidence:** the type has an inherent `pub async fn next(&mut self)`. No `impl Stream` exists in the crate, and there is no `futures-core` dependency.

**Consequence:** it cannot be used with `StreamExt` (`filter_map`, `take_while`, `timeout`), `tokio_stream::StreamMap`, `select_all`, or axum `Sse` without hand-written adapters. Callers must write `while let` loops.

**Fix:** implement `futures_core::Stream`, keeping the inherent `next` for zero-import use.

---

### G-13 [M] Every message constructor requires a caller-chosen id [read]

**Where:** `crates/a2a-protocol-types/src/message.rs:30-38` (`MessageId::new(s)`; there is no generator), and `:111, 133, 146, 162`, where `Message::new`, `user`, `agent` and `user_text` all take `id`.

**Docs:**
- The README Quick Start uses `"m1"` and `"m2"`.
- The client README uses `"msg-1"`.
- `book/src/getting-started/first-agent.md:20` tells users to add `uuid = { version = "1", features = ["v4"] }`.
- The server already depends on `uuid` and generates ids internally (`EventEmitter::fail`, `executor_helpers.rs:255`).

**Consequence:** users hard-code ids, which the spec says are unique per message. Or they add a dependency purely to call one function.

**Fix:** add `MessageId::random()`, and `Message::user_text(text)` with an auto id (keeping `*_with_id` variants).

---

### G-14 [M] `Telemetry` ergonomics [read]

1. `pub struct Telemetry` (`otel/telemetry/mod.rs:121-122`) is a drop guard: "Dropping it flushes and shuts every provider down". It has no `#[must_use]` (`#[derive(Debug)]` only). `Telemetry::init()?;` or `let _ = Telemetry::init()?;` shuts export down immediately.
2. `Telemetry::init()` is `builder().build()` (`:162-164`). Without `.with_export_where_configured()`, "every signal is exported, to `localhost` when nothing names an endpoint" (`builder.rs:111-124`). The obvious one-liner therefore spams connection errors in development, and every example has to add the extra call.
3. `otel::init_otlp_pipeline` / `init_otlp_pipeline_with_endpoint` (`otel/pipeline.rs:97, 119`) remain public and undeprecated beside `Telemetry`: two ways to set up export.
4. `Drop` may block for up to 15 s, a measured figure in the doc at `:114-120`, possibly on an async worker thread.

**Fix:**
- `#[must_use = "dropping Telemetry shuts export down"]`.
- Make "export where configured" the default of `init()`.
- Deprecate `init_otlp_pipeline`.

---

### G-15 [L-M] Naming inconsistencies [read]

- **`with_config`.** It is a setter on `Server` (`serve/graceful/mod.rs:223`, `self -> Self`). It is a constructor on `JsonRpcDispatcher`, `RestDispatcher`, `CombinedDispatcher` and `A2aRouter`: `with_config(handler, config)` (`dispatch/jsonrpc/mod.rs:64`, `rest/mod.rs:71`, `combined.rs:76`, `axum_adapter.rs:145`). It is a constructor again on the stores (`store/task_store/in_memory/mod.rs:616`).
- **Auth interceptors.** The client has `BearerAuthInterceptor` and `AuthInterceptor`; the server has `BearerTokenAuthInterceptor`. The interceptor traits are `CallInterceptor` (client) and `ServerInterceptor` (server).
- **Method names.** The spec's `SubscribeToTask` is `subscribe_to_task` on the client but `RequestHandler::on_resubscribe` on the server. The spec's `CreateTaskPushNotificationConfig` is `set_push_config`.
- **Builder flag vocabulary.** It mixes `with_x(bool)` (`with_trace_propagation`, `with_agent_span_conventions`), `without_x()`, `require_x()`, `allow_x()`, `skip_x()` and bare verbs (`isolate_tasks_by_caller`, `share_tasks_across_callers`, `authenticated_upstream`, `accept_unisolated_tenants`) (`builder.rs:508-1004`).
- **Tenant limits.** `TenantLimitsBuilder` setters have no `with_` (`max_concurrent_tasks(n)`, `tenant_config/mod.rs:216-240`), while `TenantLimits::with_event_queue_capacity(Option<usize>)` (`:190`) is a second, differently styled way to build the same struct.
- **`A2aRouter`.** The name implies "all A2A", but it serves HTTP+JSON only. `examples/deploy-agent/src/main.rs:466-470` records that the maintainer's own example shipped advertising JSONRPC for it.
- **Durations.** `RateLimitConfig::window_secs: u64` uses seconds as an integer where every other config uses `Duration`.

---

### G-16 [L] `EventEmitter::artifact(id, parts, append: Option<bool>, last_chunk: Option<bool>)` [read]

**Where:** `executor_helpers.rs:330-347`.

Every hello snippet reads `emit.artifact("greeting", vec![..], None, Some(true))`. Hello also needs three calls: Working, the artifact, Completed.

**Fix:** add `emit.text_artifact(id, text)`, `emit.complete_with_text(text)`, and an `ArtifactChunk { append, last }` or builder in place of the two `Option<bool>` arguments.

The type's own rustdoc example is `rust,ignore` (`executor_helpers.rs:192`), so docs.rs shows an uncompiled example for the most-used helper.

---

### G-17 [L] `Arc` boilerplate [read]

`RequestHandlerBuilder::build()` returns `RequestHandler`, but every dispatcher takes `Arc<RequestHandler>` (`dispatch/jsonrpc/mod.rs:58`, etc.). `WebSocketDispatcher::serve*` additionally takes `self: Arc<Self>`. Every snippet in the README and book therefore wraps `Arc::new(... .build()?)`.

**Fix:** accept `impl Into<Arc<RequestHandler>>`, or add `build_arc()`.

---

### G-18 [L] `RequestContext.context_id: String` but `task_id: TaskId` [read]

**Where:** `request_context.rs:45-48`.

`EventEmitter` has to rewrap it every time: `ContextId::new(self.ctx.context_id.clone())` (`executor_helpers.rs:227` and others). This is inconsistent with the newtype policy the types crate states (`message.rs:27-28`).

---

### G-19 [L] Order-dependent builder calls [read]

`ClientBuilder::with_protocol_binding` re-resolves the tenant from the card. Its rustdoc says "call `with_tenant` *after* this to override it" (`builder/mod.rs:412-427`). Called in the other order, the user's tenant is silently replaced.

**Fix:** record an explicit tenant and let it win regardless of order.

---

### G-20 [L] Default timeouts disagree, and the timeout error is not actionable [read]

- Client `with_timeout` defaults to 30 s (`config.rs:298`).
- Server executor timeout defaults to 1 h (`builder.rs:72`).
- A blocking `SendMessage` to an LLM agent that takes 40 s therefore fails client-side with `ClientError::Timeout("request timed out")` (`transport/jsonrpc.rs:250`), while the task keeps running on the server.

The message names neither the limit nor the remedies: `with_timeout`, `with_return_immediately` or `stream_message`.

---

### G-21 [L] An executor returning `Ok(())` without a terminal state [read]

`book/src/building-agents/executor.md:31-33` documents:

> "Returning `Ok(())` without a terminal or interrupted state invents none: the task stays in the last state written."

A forgotten `emit.status(Completed)` leaves the task `WORKING` forever. A blocking client gets a non-terminal task after `executor_drain_timeout`, which is 5 s.

I did not find a dedicated warning for this case. That absence is unverified; it could be logged somewhere I did not read.

**Fix:** emit a `warn!` naming the task, or add an opt-in `HandlerLimits::complete_on_ok`.

---

### G-22 [L] `SendMessageResponse` has no accessors [read]

**Where:** `generated/responses.rs:35-43`.

The enum is `#[non_exhaustive]` and has no `impl` block apart from proto conversion. Every caller writes `if let SendMessageResponse::Task(t) = r { .. } else { .. }` plus a wildcard arm.

**Fix:** add `as_task()`, `into_task()` and `text()`.

---

### G-23 [L] Assorted best-practice items [read]

- **Missing `Debug`.** `EventEmitter` (`executor_helpers.rs:204`) and `A2aRouter` (`axum_adapter.rs:128`) implement no `Debug`, while the other public builders do.
- **`mutants` as a normal dependency.** It is a regular dependency of `a2a-protocol-server` (`Cargo.toml:140`), so every downstream user compiles it.
- **Pub fields beside clamping setters.** `ServeConfig` and `PushRetryPolicy` are `#[non_exhaustive]` with `pub` fields and validating setters. Direct field writes bypass the setters' clamping. `PushRetryPolicy::with_max_attempts` maps 0 to 1, while `p.max_attempts = 0` is possible; it is re-clamped at use, `push/sender.rs:523`.
- **`ServeConfig::production`.** It is both a `pub bool` field and an associated constructor (`config.rs:126, 160-172`).
- **Public `hyper` dependency.** `hyper` types appear in public API (`Dispatcher::dispatch(hyper::Request<Incoming>)`, `ServerError::Http(hyper::Error)`), so a hyper 2 upgrade will be a breaking change for this crate. Acceptable, but undocumented in `STABILITY.md` (I did not verify the latter).

---

## Findings: documentation drift

### D-01 [M] `without_tls()` "Disable TLS (plain HTTP only)" [read]

**Where:** `book/src/client/builder/reference.md:30`, `book/src/reference/configuration.md:199` and the rustdoc at `builder/mod.rs:506`. All three describe a method that does nothing (G-01).

### D-02 [M] The server README is stale on `serve` [read]

`crates/a2a-protocol-server/README.md:61-63`:

> "`serve` is the shortest path, not the production one: it serves every connection with hyper's defaults, with no connection cap, no idle timeout ..."

The code contradicts this: `serve` runs on `Server` under `ServeConfig::default()`, with a 30 s header-read timeout and a 75 s idle timeout (`serve/mod.rs:112-124, 181-190`). `book/src/getting-started/first-agent.md:132-138` also states the right values. This README is the crate's crates.io page.

### D-03 [L] `ServeConfig::with_max_connections` rustdoc predicts existing fields [read]

`serve/graceful/config.rs:186-192`:

> "a header-read timeout and an idle timeout are the obvious next fields"

Both fields already exist in the same struct (`:110-125`).

### D-04 [M] `packaging/README.md` says no signing key is committed [read]

`packaging/README.md:20-22`:

> "release.yml refuses any `v*` tag not signed by a key committed under `.github/release-signers/`, and none is committed"

`.github/release-signers/allowed_signers` contains `tomf@tomtomtech.net namespaces="git" ssh-ed25519 AAAAC3...`. Step 1 of "Turning the drafts into releases" is therefore already done, and 0.15.0 and 0.15.1 were released.

### D-05 [M] `tools/a2a-cli/README.md` exit codes and `--header` [read]

**Exit codes.** The README's table says code `2` means "Usage error ... Reported by `clap`; no network is touched". But `tools/a2a-cli/src/main.rs:109-118` also exits 2 on an unusable `OTEL_*` value (`std::process::exit(2)`), and the CHANGELOG [Unreleased] confirms that every binary does this. `src/error.rs:8-9, 42-49` ("Everything here is therefore exit 1") does not mention this path either.

**`--header`.** The README says "Extra header on every request; repeatable". The values are collected into `HashMap<String, String>` (`connect.rs:80-86`; `HeaderInterceptor.headers`, `:44-58`). Repeating the same name, for example two `--header Cookie=...`, silently keeps only the last one. Names are also case-sensitive keys, so `X-A` and `x-a` both get sent.

**Fix:** document exit 2 for the telemetry configuration path, or give it a distinct code. Use a `Vec<(String, String)>` and append rather than insert.

### D-06 [L] Book graceful-shutdown page on gRPC [read]

`book/src/deployment/production/graceful-shutdown.md:82-84` says the gRPC and WebSocket `serve_with_shutdown` "returns the same `ServeReport`". The gRPC one returns `io::Result<ServeReport>` (`dispatch/grpc/shutdown.rs:117-121`).

### D-07 [L] Book Axum page models a misconfiguration [read]

`book/src/building-agents/dispatchers/axum.md:23` has a hidden line, `AgentCard::new(.., AgentInterface::jsonrpc(..))`, used with `A2aRouter`, which serves HTTP+JSON. The rustdoc counterpart correctly uses `AgentInterface::rest` (`axum_adapter.rs:16`). The page compiles, but a reader who expands the hidden lines copies the misconfiguration.

### D-08 [L] Server crate quick start is outdated [read]

`crates/a2a-protocol-server/src/lib.rs:13-15`, step 3:

> "Wire [`JsonRpcDispatcher`] or [`RestDispatcher`] into your hyper server."

That predates `serve` and `Server`, which are what users should be pointed at.

### D-09 [L] `is_retryable` doc contradicts its match [read]

`crates/a2a-protocol-client/src/retry.rs:150-151` lists "HTTP connection/transport errors" as retryable, but `ClientError::Transport(_) => false` (`:176`).

### D-10 [L] The hello-agent framing [read]

The README ("Hello Agent") and `examples/hello-agent/README.md` call it "one screen ... no SDK feature flags".

Measured: `main.rs` is 99 lines above `#[cfg(test)]`, 62 of them non-blank and non-comment. `main()` is 39 code lines, about 15 of them `Telemetry` setup that the README's "What it demonstrates" omits.

It relies on SDK default features, `otlp` among them (G-06), so "no feature flags" holds only nominally.

Concepts a newcomer needs: `agent_executor!`, `EventEmitter`, `TaskState`, `Part`, `AgentCard`, `AgentInterface`, `AgentSkill` (with its tag rule), `RequestHandlerBuilder`, `Arc`, `Server::bind`, `serve_with_shutdown`, `JsonRpcDispatcher`, `shutdown_signal` and `Telemetry`: 14 in all. The URL `127.0.0.1:3000` is also written twice, once for the card and once for the bind, with nothing tying them together.

The README Quick Start is shorter: 40 code lines.

### D-11 [L] Prelude rustdoc list is out of sync [read]

`crates/a2a-protocol-sdk/src/lib.rs:66-81` omits much of what the prelude exports: `AgentSkill`, `AgentCapabilities`, `TaskStatusUpdateEvent`, the auth and rate-limit interceptors, `Server`, `serve*`, `shutdown_signal`, `Telemetry` and `A2aRouter`. The SDK README's "Prelude Contents" is shorter still.

`ServeConfig` and `CombinedDispatcher` are not in the prelude, although `Server` is, so the production path needs extra imports.

---

## CHANGELOG and versioning

### C-01 [M] Size and readability [read]

`CHANGELOG.md` is **754,592 bytes and 11,763 lines**. Measured per section:

| Section | Bytes | Words | Bullets | Words per bullet | Subsections |
|---|---|---|---|---|---|
| `[Unreleased]` | 22.6 KB | 3,507 | 38 | ~92 | Added, Behaviour Changes, Fixed, Internal; no "Breaking Changes" |
| `[0.15.1]` (patch) | 20.2 KB | 2,912 | 24 | 121 | — |
| `[0.15.0]` | **289 KB** | **40,316** | 415 | — | Breaking Changes alone is lines 656-985 |

Entries read as engineering logs rather than user notes. The [Unreleased] preamble cites llama.cpp build numbers and Grafana versions, and entries carry measurements such as "390 ± 13 against 443 ± 13".

"Internal" sections are included in the user-facing file.

An upgrade guide does exist (`book/src/reference/upgrading.md`, plus `0.14-to-0.15/`, about 68 KB). `book-tests/tests/upgrading_guide.rs` enforces a section for every breaking boundary.

**Fix:** a one-screen "Highlights / Breaking / Action required" block at the top of each release. Move "Internal" and the measurements to `docs/`.

### C-02 [M] 0.15.1, a patch, broke existing code [read]

The `CHANGELOG.md` [0.15.1] "Behaviour Changes" (`:468-502`) and the book (`upgrading.md:52-60`) both say:

> "`install_subscriber` now always installs a subscriber, so a `tracing_subscriber` `.init()` after it panics"

They also say `HTTP_PROXY` is now applied to OTLP export, which reroutes in-cluster export through a proxy unless `NO_PROXY` is set.

`STABILITY.md`, as quoted at `upgrading.md:66-70`, says "A minor release may contain breaking changes; a patch release may not." A change that makes previously working code panic does not meet that bar, even though no signature changed.

### C-03 [L] Doctest coverage: what is not compiled [read]

What is compiled:
- `book-tests` registers 209 of 210 `book/src/**/*.md` pages. Only `SUMMARY.md` is left out.
- Across the book's Rust blocks: 159 `rust`, 139 `rust,no_run` (compiled, not run) and 4 `rust,ignore`, all in `bindings/slimrpc.md`. Nothing is tagged `text` that ought to be Rust.
- The root README and all four crate READMEs are compiled as doctests (`include_str!` in each `lib.rs`, and `book-tests/src/lib.rs`).

Not compiled:
- `examples/hello-agent/README.md`: 2 Rust blocks.
- `examples/genai-agent/README.md`: 1 block, which is pseudo-code with no body and would not compile.
- 14 `rust,ignore` rustdoc blocks in `crates/*/src`, among them the main examples of `EventEmitter` (`executor_helpers.rs:192`), `RequestContext`, `EventStream`, `RateLimitInterceptor` and the `A2aRouter` composability block.

### Versions, MSRV and docs.rs [read]

Consistent:
- All crates and `tools/a2a-cli` are at 0.15.1; `CITATION.cff` is 0.15.1 / 2026-10-26.
- Book dependency snippets use `"0.15"`. Older versions appear only in historical upgrade pages.
- MSRV is 1.94 in `[workspace.package]`, the README badge, `installation.md` and CI (`ci.yml:619`, `release.yml:340-342`).
- `Duration::from_mins` (`client/config/stream_defaults.rs:39`) is fine at 1.94.
- docs.rs uses `all-features = true` with `--cfg docsrs` on all four crates.

---

## What is good (for balance)

- `RequestHandlerBuilder::build()` refusals are specific and say what to change.
- The book is unusually well tested.
- Default values in `book/src/reference/configuration.md` match code. I checked `HandlerLimits`, `TaskStoreConfig`, `DispatchConfig`, `GrpcConfig`, `PushRetryPolicy`, `RateLimitConfig`, the executor and queue constants, and the client builder defaults.
- Documented env vars all exist in code or are read by the OpenTelemetry SDK.
- An automated scan of `Type::method` references across all book pages found no missing API, apart from historical names in the upgrade pages.
