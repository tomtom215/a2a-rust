<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

> **Frozen audit input, 2026-11-25, of the tree at `2058058d`.** Not maintained. Written by a delegated
> read-only audit for [`docs/program/README.md`](../README.md); its VERIFIED and INFERRED labels are the auditor's own.
> Re-check an anchor before acting on it. `UPSTREAM/` is a local clone of the named upstream repository.

# 04 — Developer-experience and API-surface audit, a2a-rust 0.16.0

Repository `/home/user/a2a-rust` at `2058058d` (`git rev-parse --short HEAD`). Read-only: `git status --short` was empty before and after. Nothing was built, tested or documented with cargo. One deviation from "offline only": `cargo tree --offline` failed (`no matching package named base64 found`), so the dependency counts in §1.9 were taken with `cargo tree --locked`, which downloaded crate sources into `~/.cargo/registry` (452 MB, outside the repo) and wrote nothing to the repo.

Every claim is labelled **VERIFIED** (I ran the command or read the cited lines) or **INFERRED** (reasoned from what I read, not run). All paths are relative to the repo root. Where docs and code disagree, the code is the authority and the disagreement is listed in §2.4.

---

## 1. Measured facts

### 1.1 Smallest agent and smallest client (VERIFIED)

| Fact | Value | Command |
|---|---|---|
| `examples/hello-agent/src/main.rs`, whole file | 200 lines | `wc -l examples/hello-agent/src/main.rs` |
| Code lines above `#[cfg(test)]` (line 102), comments and blanks excluded | **48** | `sed -n '1,101p' examples/hello-agent/src/main.rs \| grep -vcE '^\s*(//\|$)'` |
| Of those 48: telemetry start-up | 13 lines (`main.rs:61-73`) | read |
| Of those 48: refused-start check | 6 lines (`main.rs:94-98`) | read |
| Smallest client | No client-only example exists. The smallest is the README Quick Start's client half: `README.md:211-217` (build, send, print = 7 lines), plus `README.md:220-231` to stream | `grep -n 'ClientBuilder::new(url)' README.md` |
| README claim "about 50 lines of code above its tests" (`README.md:252`) | agrees with 48 | read |

The persona target "<30 lines" is not met: 48. The README Quick Start's server half (`README.md:182-208`) is about 22 code lines, but it gets there by dropping the `ServeReport` (see F-1).

### 1.2 Feature flags (VERIFIED)

Command: `for c in crates/a2a-protocol-* bindings/*; do awk '/^\[features\]/{f=1;next} /^\[/{f=0} f' $c/Cargo.toml | grep -cE '^[a-z0-9_-]+ *= *\['; done`

| Crate | Count (incl. `default`) | Default set |
|---|---|---|
| `a2a-protocol-types` | 3 | none |
| `a2a-protocol-client` | 10 | `tls-rustls`, `tracing`, `otel`, `webhook` |
| `a2a-protocol-server` | 20 | `tracing`, `otel`, `tls-rustls` |
| `a2a-protocol-sdk` | 20 | `tls-rustls`, `tracing`, `otel`, `otlp`, `webhook` |
| `a2a-protocol-discovery` | 2 | `tls-rustls` |
| `bindings/a2a-protocol-slimrpc` | 2 | `otlp` |
| `tools/a2a-cli`, `tools/a2a-codegen` | 0 | — |

Total 57 entries. Full list, one line each (from the `##` comments in each `Cargo.toml`):

**types (3):** `signing` — JWS card signing, RFC 8785 canonical JSON, x5c expiry. `audit` — audit record schema, hash chains, checkpoint verification (implies `signing`). `proto` — prost-generated `lf.a2a.v1` types and conversions.

**client (10):** `default`. `tls-rustls` — HTTPS and `wss://`, shared connector, proxy variables. `signing` — card signature verification in discovery. `tracing` — logging via `tracing`. `otel` — client spans and `rpc.client.call.duration` through the OTel API. `websocket` — WebSocket transport. `grpc` — tonic transport. `grpc-tls` — TLS for gRPC (implies `grpc`). `webhook` — `WebhookReceiver` for push notifications. `testing` — `testing::ScriptedPeer`, a hostile loopback peer.

**server (20):** `default`. `signing` — JWS plus `with_content_signer`. `tracing`. `tls-rustls` — HTTPS push delivery, `Server::with_tls`, `MutualTlsAuthInterceptor`. `sqlite` — SQLite stores. `postgres` — PostgreSQL stores (TLS always compiled in). `websocket` — WebSocket dispatcher. `grpc` — gRPC dispatcher. `grpc-tls` — TLS on the gRPC listener. `otel` — `OtelMetrics` and the traceparent bridge (API only). `otlp` — `Telemetry`, OTLP export, Langfuse preset. `otel-prometheus` — `/metrics` scrape endpoint. `otlp-json` — OTLP/HTTP with JSON bodies. `conformance` — harness that grades an `AgentExecutor`. `tool-loop` — `ToolLoop`, `Tool`, `ToolModel`, `OpenAiChatModel`. `skills` — Agent Skills in the tool loop (implies `tool-loop`). `axum` — `RestRouter`. `auth-jwt` — `JwtAuthInterceptor`. `encryption` — `EncryptedTaskStore`. `audit` — audit trail.

**sdk (20):** `default`, `signing`, `tracing`, `tls-rustls`, `webhook`, `grpc`, `grpc-tls`, `otel`, `otlp`, `otel-prometheus`, `otlp-json`, `websocket`, `sqlite`, `encryption`, `postgres`, `axum`, `tool-loop`, `skills`, `auth-jwt`, `audit` — each forwards to the crate(s) above.

**discovery (2):** `default`, `tls-rustls`. **slimrpc (2):** `default`, `otlp`.

Not forwarded by the SDK: server `conformance`, client `testing`, types `proto` (stated at `crates/a2a-protocol-sdk/README.md:91-92`).

### 1.3 Builder widths (VERIFIED)

Command: `grep -cE '^\s*pub (const |async )?fn ' <file>`

| Builder | Public methods | Where |
|---|---|---|
| `RequestHandlerBuilder` | **59** = 57 in `crates/a2a-protocol-server/src/builder.rs` + `with_authorized_tenant_resolver` (`builder/multi_tenant.rs:34`) + `with_steering` (`builder/steering.rs:39`). Includes `new`, `build`, `build_arc` | 5 `impl` blocks in `builder.rs` (lines 252, 1913, 1953, 1983) plus 5 in `builder/*.rs` |
| `ClientBuilder` | **47** = 43 in `builder/mod.rs` + `from_signed_card` (`builder/signed.rs:38`) + `build`, `build_grpc` (`builder/transport_factory.rs:165, 387`) + `with_credential_preflight` (`credential_preflight.rs:76`) | 5 `impl ClientBuilder` blocks |
| `ClientConfig` setters | 15 (`config/setters.rs`) + 3 (`config.rs`) | |
| `TelemetryBuilder` | 20 (`otel/telemetry/builder.rs:107-308`) | |
| `ServeConfig` | 12 (`serve/graceful/config.rs:206-321`) + 11 public fields | |

Verb vocabulary on `RequestHandlerBuilder` (command: `grep -nE '^\s*pub (const )?fn ' builder.rs | sed -E 's/.*fn ([a-z]+)_.*/\1/' | sort | uniq -c`): 39 `with_`, 3 `without_`, 3 `allow_`, 2 `require_`, and one each of `share_`, `refuse_`, `isolate_`, `expose_`, `declare_`, `accept_`, `authenticated_`. `ClientBuilder`: 32 `with_`, 3 `without_`, 3 `require_`, 2 `from_`, 1 `skip_`, 1 `chosen_`.

### 1.4 Facade (VERIFIED)

`crates/a2a-protocol-sdk/src/lib.rs` is 154 lines. It glob-re-exports the three crates as `types`, `client`, `server` (lines 45-61) and defines `prelude` (lines 105-154).

- Prelude named items: **70** with every feature, **68** on default features (`A2aRouter`, `RestRouter` need `axum`). Counted by hand from lines 107-153: 25 wire types, 2 errors, 13 client, 2 pool, 2 webhook, 20 server, 2 deprecated (`serve`, `serve_with_addr`), 1 macro, 1 `Telemetry`, 2 axum.
- Reachable through the globs: 123 root `pub use` names and 30 `pub mod` in the server crate, 40 and 22 in the client, 73 and 29 in types (counted by a script over each `lib.rs`; line comments stripped).
- Not in the prelude although the first three examples past hello-agent need them: `Delegation`, `FailureClass`, `CombinedDispatcher`, `TaskStore`, `Profile`, `HandlerLimits`, `ErrorCode`. `examples/deploy-agent/src/main.rs` imports `server::dispatch::CombinedDispatcher` by path; `examples/swarm` does not use the SDK crate at all (`examples/swarm/Cargo.toml` depends on the three crates separately).

### 1.5 Examples (VERIFIED)

16 directories under `examples/` (15 binaries plus the `harness` library). Command: `for e in examples/*/; do find $e -name '*.rs' | xargs cat | wc -l; wc -l < $e/src/main.rs; done`

| Example | `.rs` files | Total lines | `main.rs` |
|---|---|---|---|
| agent-team | 30 | 9194 | 500 |
| incident-response | 17 | 4180 | 289 |
| resilient-agent | 9 | 2401 | 251 |
| rig-agent | 8 | 1963 | 422 |
| mcp-bridge | 7 | 1894 | 122 |
| harness (lib) | 9 | 1818 | — |
| mcp-agent | 8 | 1631 | 425 |
| multi-lang-team | 7 | 1266 | 453 |
| genai-agent | 7 | 1232 | 487 |
| live-model | 4 | 1141 | 172 |
| deploy-agent | 4 | 1108 | 493 |
| echo-agent | 4 | 910 | 461 |
| swarm | 5 | 750 | 321 |
| tool-loop-agent | 3 | 483 | 222 |
| langfuse-agent | 2 | 327 | 206 |
| hello-agent | 1 | 200 | 200 |

### 1.6 Book (VERIFIED)

224 Markdown files under `book/src` (`find book/src -name '*.md' | wc -l`), 223 links in `SUMMARY.md`, 38,069 lines in total. `book-tests/src/lib.rs` includes 169 pages as doctests, with every feature on (`book-tests/Cargo.toml:20-38`).

### 1.7 Deprecations and `#[non_exhaustive]` (VERIFIED)

**31** `#[deprecated(` attributes in `crates/`, `bindings/`, `tools/` (a script matching lines that start with `#[deprecated`; a plain `grep -c '#\[deprecated'` gives 34 because three doc comments mention the attribute).

| Since | Count | Removal stated |
|---|---|---|
| 0.16.0 | 26 | 23 say "removed no earlier than 0.17.0"; 3 state none |
| 0.15.0 | 5 | none state one |

The eight with no removal version: `types/src/generated/security.rs:197, 212` (the `Implicit` and `Password` OAuth flow variants, deprecated because the proto marks them so; these are not removable), `client/src/config.rs:138` and `config/setters.rs:15` (`preferred_bindings`), `server/src/agent_card/hot_reload.rs:107` (`update`), `server/src/tenant_resolver.rs:258` (`new`), `server/src/otel/pipeline.rs:98, 124` (`init_otlp_pipeline*`).

The 23 with removal ≥ 0.17.0: `A2aClient::set_push_config`; `RateLimitConfig::with_window_secs`; free `serve`, `serve_with_addr`; four `TenantLimitsBuilder` bare setters; `RequestHandler::on_resubscribe`, `on_set_push_config`; six on `WebSocketDispatcher` (`dispatch/websocket.rs:351-512`); seven on `GrpcDispatcher` (`dispatch/grpc/serve_config.rs:63-164`).

`#[non_exhaustive]`: **257** in `crates/*/src` (server 124, types 97, client 33, discovery 3, sdk 0). `#[must_use`: 1,347 (server 750, types 313, client 275, discovery 9).

### 1.8 File lengths (VERIFIED)

`.file-length-baseline` lists **82** files over the 500-line limit (`grep -vcE '^\s*(#|$)' .file-length-baseline`). The 20 largest files under `crates/*/src`, `bindings/*/src`, `tools/*/src` (`find … -name '*.rs' | xargs wc -l | sort -rn | head -21`):

| Lines | File |
|---|---|
| 3446 | server/src/handler/messaging/tests.rs |
| 3322 | server/src/dispatch/websocket.rs |
| 3277 | server/src/store/task_store/in_memory/mod.rs |
| 3155 | server/src/streaming/event_queue/in_memory.rs |
| 2988 | server/src/handler/event_processing/sync_collector.rs |
| 2699 | server/src/builder.rs |
| 2595 | server/src/auth/jwt.rs |
| 2567 | client/src/transport/websocket.rs |
| 2279 | server/src/handler/event_processing/background/state_machine.rs |
| 2097 | client/src/discovery.rs |
| 2024 | client/src/retry.rs |
| 1925 | client/src/transport/grpc.rs |
| 1802 | server/src/store/tenant_sqlite_store.rs |
| 1764 | client/src/token_provider.rs |
| 1736 | types/src/error.rs |
| 1696 | server/src/store/tenant/store.rs |
| 1684 | client/src/streaming/event_stream.rs |
| 1646 | server/src/dispatch/axum_adapter.rs |
| 1612 | server/src/handler/push_config.rs |
| 1594 | client/src/builder/mod.rs |

Source total: 312,030 lines. Across the whole tree the largest file is `crates/a2a-protocol-server/tests/postgres_store_tests.rs` (3,740).

### 1.9 Compile-time cost: dependency counts (VERIFIED, with `--locked`)

Command: `cargo tree --locked -e normal --prefix none -p <crate> [flags] | sed 's/ (\*)//' | awk '{print $1" "$2}' | sort -u | grep -v '^a2a-protocol-' | wc -l` (distinct third-party crate versions, normal dependencies only).

| Crate | `--no-default-features` | default | `--all-features` |
|---|---|---|---|
| `a2a-protocol-types` | 11 | 11 | 49 |
| `a2a-protocol-client` | 51 | 73 | 130 |
| `a2a-protocol-server` | 65 | 85 | 228 |
| `a2a-protocol-sdk` | 66 | **124** | 228 |

Also: SDK default plus `postgres,auth-jwt` = 197; SDK default plus `grpc` = 129; SDK default including build dependencies = 127. `Cargo.lock` holds 512 packages (483 distinct names), which includes dev-dependencies, examples and benches.

The hello-agent build is the SDK default: 124 crates. 58 of them (124 − 66) come from the defaults `tls-rustls`, `otel`, `otlp`, `webhook`, `tracing`; the server crate alone at its defaults is 85, so the OTLP exporter the SDK turns on accounts for most of the difference (INFERRED from the two rows; I did not isolate `otlp` on its own).

### 1.10 Other counts (VERIFIED)

- Public traits by grep: 37 lines matching `pub trait ` in `crates/*/src` outside tests. `docs/handoff.md` (section "Open engineering") gives 20 reachable with every feature, from `cargo doc`; the grep includes traits in private modules.
- `Pin<Box<dyn Future` occurrences in `crates/*/src` outside tests: 429.
- `ServerError::Config(` construction sites in the server crate outside tests: 38, of which 25 are in `builder.rs`.
- `trace_warn!` / `trace_error!` sites outside tests: 224 in the server, 54 in the client.
- `Postgres*` store types a durable deployment may need: 10 plain (`PostgresTaskStore`, `PostgresPushConfigStore`, `PostgresPushOutbox`, `PostgresHaltStore`, `PostgresLeaseStore`, `PostgresTurnClaims`, `PostgresAuditStore`, `PostgresRateLimitCounter`, `PostgresStepJournal`, `PostgresDelegationJournal`) plus 2 tenant-aware (`grep -rnE 'pub struct Postgres\w+' crates/a2a-protocol-server/src`).

---

## 2. Friction log by persona

### 2.1 P1 — a Rust dev who wants an agent serving in under 30 lines

**Concepts before first success (VERIFIED, counted from `README.md:182-208` and `examples/hello-agent/src/main.rs:24-100`): 13.** `agent_executor!`, `RequestContext` (`ctx`), `EventQueueWriter` (`queue`), `EventEmitter`, `AgentCard`, `AgentInterface`, `AgentSkill` (with a mandatory tag), `RequestHandlerBuilder`, `Arc`, `Server::bind`, `JsonRpcDispatcher`, `serve_with_shutdown` + `shutdown_signal`, `ServeReport::refused`. Plus `Telemetry` in hello-agent.

| # | Friction | Evidence | Label |
|---|---|---|---|
| P1-1 | **A refused start is silent in the README Quick Start.** `tokio::spawn(server.serve_with_shutdown(...))` drops the `JoinHandle`, so the `#[must_use]` `ServeReport` is never read. `ServeReport` carries the refusal only as `refused: Option<String>` and the type's own doc says "a caller that drops the report exits as though it had served". The Quick Start also installs no `tracing` subscriber, so the `trace_error!("listener refused to start")` line is not printed either | `README.md:208`; `serve/graceful/mod.rs:146-152, 171`; `serve/graceful/start.rs:41-42` | VERIFIED |
| P1-2 | **Checking the refusal costs 6 lines** in hello-agent and 4 in the book's Quick Start, because `serve_with_shutdown` returns `ServeReport`, not a `Result`, and the report has no conversion | `examples/hello-agent/src/main.rs:89-98`; `book/src/getting-started/quick-start.md` (the `match report.refused` block) | VERIFIED |
| P1-3 | **Telemetry start-up is 13 of hello-agent's 48 lines**, and the same block is copied into 18 files. A two-line form exists (`Telemetry::init_where_configured()`, `otel/telemetry/mod.rs:205`) and is used twice in the whole of `examples/`, `tools/`, README and book | `grep -rln 'names a configuration Telemetry cannot use' examples tools --include=*.rs \| wc -l` → 18; `grep -rn init_where_configured examples tools README.md book/src \| wc -l` → 2 | VERIFIED |
| P1-4 | **The URL is written twice** (the card's interface and the bind address) and nothing ties them. README calls the resulting mismatch "the deployment bug whose only symptom is clients failing to call back" | `examples/hello-agent/src/main.rs:75, 83`; `README.md:275-277` | VERIFIED |
| P1-5 | **`build_arc()` exists and nobody is shown it.** `RequestHandlerBuilder::build_arc` (`builder.rs:1230`) was added because "every snippet wrapped `Arc::new(builder.build()?)`" (its doc, line 1225). Uses in `examples/`: 0. Mentions in README, book, crate READMEs: 0. `Arc::new(` near `.build()` in examples: 31 | `grep -rn 'build_arc()' examples --include=*.rs \| wc -l` → 0; `grep -rn build_arc README.md book/src crates/*/README.md \| wc -l` → 0 | VERIFIED |
| P1-6 | **Boxed-future ergonomics: the helpers exist, the examples mostly do not use them.** ADR 0008 says both helpers "are used extensively in the repo's examples". In `examples/`: `agent_executor!` 5 uses, `boxed_future(` 5, hand-written `Box::pin(async` 34, hand-written `impl AgentExecutor for` 19. The macro's `|this, ctx, queue|` arm for an agent with state exists (`executor_helpers/agent_executor_macro.rs`, "An agent with state"), and the macro cannot implement the trait for a generic type (same file, last doc line) | `grep -rn 'agent_executor!' examples --include=*.rs \| wc -l` etc.; `docs/adr/0008-agent-executor-trait-shape.md` ("Decision") | VERIFIED |
| P1-7 | **Artifacts: the examples teach the four-positional form.** `emit.artifact(id, parts, None, Some(true))` appears 19 times in examples; `complete_with*` / `text_artifact` appear on 2 lines. deploy-agent, "the smallest one you can ship", uses the positional form | `grep -rnE 'emit(ter)?\s*\.artifact\(' examples --include=*.rs \| wc -l` → 19; `executor_helpers.rs:316-321`; `executor_helpers/artifacts.rs:106, 132, 165, 192` | VERIFIED |
| P1-8 | **Error ergonomics inside an executor.** An executor returns `A2aResult<()>`. `From<ClientError> for A2aError` exists (`client/src/error/mod.rs:399`), but there is no `From` for `std::io::Error`, `serde_json::Error` or `Box<dyn Error>`, so `?` on those needs `map_err`. `A2aError::internal` appears 17 times in examples | `grep -rnE 'impl From<[^>]+> for (A2aError\|ServerError\|ClientError)' crates/*/src` | VERIFIED |
| P1-9 | **Three error types, two stringly.** `A2aError`, `ServerError`, `ClientError`. Every configuration refusal is `ServerError::Config(String)` (38 sites) or `ClientError::Config(String)`: readable, not matchable | `server/src/error/mod.rs:61`; `client/src/error/mod.rs:42` | VERIFIED |
| P1-10 | **The Quick Start dispatcher has no health endpoint.** `JsonRpcDispatcher` serves none of `/health`, `/ready`, `/load`; the book tells the reader to hand-route with `hyper` and `http_body_util`. Moving hello-agent to Kubernetes therefore means changing dispatcher to `CombinedDispatcher`, which is not in the prelude | `book/src/deployment/health-checks.md:17` and the `route` example below it; `crates/a2a-protocol-sdk/src/lib.rs:131-137` | VERIFIED |
| P1-11 | **Default compile cost: 124 crates for hello world** (§1.9), because the SDK's defaults include the OTLP exporter | §1.9 | VERIFIED |
| P1-12 | **The book's Quick Start begins with `git clone`**, not `cargo new`. The path "new project, paste, run" is the README's and `first-agent.md`'s | `book/src/getting-started/quick-start.md` (first code block) | VERIFIED |
| P1-13 | **Feature discovery is good once found.** `book/src/getting-started/installation.md:41-148` has per-crate feature tables; `scripts/check_feature_tables.py` gates them. docs.rs builds with `all-features` and `--cfg docsrs` in four crates, each with `#![cfg_attr(docsrs, feature(doc_cfg))]`. The discovery crate has no `[package.metadata.docs.rs]` | `grep -A3 'package.metadata.docs.rs' crates/*/Cargo.toml` | VERIFIED |
| P1-14 | **What fails loudly (good):** a missing `A2A-Version` header (`VERSION_NOT_SUPPORTED`); `build()` with no card, an invalid card, push declared with no sender, a zero timeout (`builder.rs:1236-1262`); `self` inside the two-argument macro form (a `compile_fail` doctest) | cited lines | VERIFIED |

### 2.2 P2 — a platform engineer deploying a durable multi-replica fleet

| # | Friction | Evidence | Label |
|---|---|---|---|
| P2-1 | **No example applies a deployment profile.** `Profile` has six variants (`profile.rs:19-205`), the book's checklist starts with it (`book/src/deployment/production.md:21-22`), and `grep -rn 'with_profile\|profile_report\|ServeConfig::production' examples --include=*.rs` returns nothing. `examples/deploy-agent` has no authenticating interceptor, no halt store, no lease store, no content signer, and does not log `profile_report()` | the grep; `examples/deploy-agent/src/main.rs` (no `with_interceptor`) | VERIFIED |
| P2-2 | **No page or example assembles the whole production handler.** Checked every file that mentions `with_halt_store` for `PostgresTaskStore` + `Profile::Production` + `with_lease_store` + `with_content_signer` together: none has all. `production.md:115-170` shows the profile with `InMemoryHaltStore` / `InMemoryLeaseStore`; `shared-stores.md` shows PostgreSQL with no profile. P2 composes them from six pages | per-file counts from a loop over `grep -rl with_halt_store book/src examples` | VERIFIED |
| P2-3 | **Up to twelve PostgreSQL store types, each with its own pool by default.** "Each store opened from a URL opens a pool of its own, of 10 connections"; five replicas at the defaults already ask for PostgreSQL's 100 connections. Sharing one pool needs `from_pool(PgPool)`, so `sqlx` becomes a direct dependency of the application at a compatible version; the server crate does not re-export it | `book/src/deployment/horizontal-scaling/shared-stores.md` ("How many connections"); `store/postgres_store/mod.rs:135, 152, 223`; `grep -rn 'pub use sqlx' crates/a2a-protocol-server/src` → none | VERIFIED |
| P2-4 | **`Profile::Production` needs the `signing` feature and a content signer**, which is not a default. "Without the feature the profile cannot be met, and `build()` says to enable it": a runtime message about a compile-time choice | `profile.rs:69-72` | VERIFIED |
| P2-5 | **Invalid configuration is caught at `build()` or at serve time, never at compile time, and the listener half is not a `Result`.** `build()` is one function of roughly 650 lines (`builder.rs:1265-1912`) returning `ServerError::Config(String)`. `ServeConfig::production()` refusals arrive in `ServeReport::refused` after `serve_with_shutdown` returns | `builder.rs:1265`; `serve/graceful/mod.rs:171, 274-278` | VERIFIED |
| P2-6 | **`profile_report()` covers the handler, not the listener.** `ProfileReport` has `profiles`, `checked`, `disabled`, `not_verifiable`, `declared` (`profile_report.rs:23-36`). It does not say whether `ServeConfig::production()` was used, whether connections are capped, or which bindings are served. A handler with no profile reports "no deployment profile: nothing was checked" (`profile_report.rs:95`) | cited lines | VERIFIED |
| P2-7 | **Warnings depend on a subscriber.** Escape hatches, card warnings and the production listener warning go through `trace_warn!` (224 warn/error sites in the server). With no subscriber installed they print nowhere; with `default-features = false` they are compiled out | `builder.rs` (e.g. the `trace_warn!(warning = %_warning, "agent card")` in `build`); `serve/graceful/start.rs:43-44`; server `Cargo.toml` `tracing` entry | VERIFIED |
| P2-8 | **A store P2 constructs reports no metrics until told to.** The builder passes its metrics to the default store only (`builder.rs:1455-1471`). "A store you construct reports to nothing until you do" (`PostgresTaskStore::with_metrics`), so `a2a.server.store_retries{operation="statement"}` and any alert built on it are silently empty | `book/src/deployment/horizontal-scaling/shared-stores.md:143`; `builder.rs:1455-1471` | VERIFIED |
| P2-9 | **Configuration is code only in the SDK; environment handling is each binary's own.** ADR 0040 §3 documents the variables in one table and renames nothing; §4 rejects a configurable gateway binary. `deploy-agent` hand-writes about 110 lines of `Config::from_env` / `from_vars`, `parse_pool_size` and a 40-line `redacted(url)`. An adopter's own crate re-writes them. The exceptions the SDK itself reads: `OTEL_*`, `LANGFUSE_*`, `SSL_CERT_FILE` / `SSL_CERT_DIR`, the proxy variables | `docs/adr/0040-first-run-configuration-and-scope.md` §3, §4; `examples/deploy-agent/src/main.rs` (the `Config`, `Vars`, `redacted` items) | VERIFIED |
| P2-10 | **Auth takes two declarations that must agree**: the interceptor and the card's security scheme. `build()` does check agreement (`builder.rs:1244-1249`), which is good, but the card side is verbose: a `HashMap::from([("bearer".to_owned(), SecurityScheme::Http(HttpAuthSecurityScheme::new("Bearer".to_owned())))])` | `book/src/deployment/production.md` (the Production example's card) | VERIFIED |
| P2-11 | **Two-step shutdown.** `serve_with_shutdown` ends tasks and drains; `RequestHandler::shutdown()` must be called afterwards to run the executor's cleanup hook. hello-agent does not call it; deploy-agent does | `serve/graceful/mod.rs:270-272`; `examples/deploy-agent/src/main.rs` (`handler.shutdown_with_timeout`) | VERIFIED |
| P2-12 | **gRPC and WebSocket listeners are separate serve shapes.** Three kept `serve_with_shutdown` signatures with different receivers and return types; `ServeConfig` reaches the others only through `with_serve_config` | `docs/adr/0052-one-serve-shape-per-listener.md` ("Per entry point") | VERIFIED |
| P2-13 | **How-to pages carry measurement history.** `shared-stores.md` mixes the recipe with dated run narratives ("On 2026-10-30 every PostgreSQL test … 147 of 151 passed"). ROADMAP "Antipatterns" items 2 and 6 name this class | `book/src/deployment/horizontal-scaling/shared-stores.md` ("Behind PgBouncer"); `ROADMAP.md` "Antipatterns" | VERIFIED |
| P2-14 | **Kubernetes material exists and is good, but lives with the example.** `examples/deploy-agent/{Dockerfile, Dockerfile.dockerignore, deployment.yaml, network.yaml}`: Deployment with 2 replicas, liveness on `/health`, readiness on `/ready`, 60 s termination grace, Service, PodDisruptionBudget, NetworkPolicy, Ingress. `deploy/kubernetes/` holds only `observability/` | `find deploy/kubernetes examples/deploy-agent -maxdepth 2 -type f`; `grep -n 'kind:\|path:' examples/deploy-agent/*.yaml` | VERIFIED |

### 2.3 P3 — an engineer building a supervisor that delegates to 50 workers

| # | Friction | Evidence | Label |
|---|---|---|---|
| P3-1 | **There is no golden path for fan-out. The design exists and is unbuilt.** ADR 0033 ("Fan-out is a group of delegations with one join rule, one budget split and one cancel") is `Status: Proposed. Nothing here is built or run.` Its own context says "Every orchestrator on this SDK writes the same five things again, and some of them are easy to write wrongly" (a cap, a join rule, a budget split, one cancel, ordered results). No `DelegationGroup` in the client crate | `docs/adr/0033-group-delegation.md:7` and "Context"; `grep -rn 'DelegationGroup' crates/` → none | VERIFIED |
| P3-2 | **The swarm example is a benchmark, not a template.** "It is a measurement of the SDK's overhead, not of an orchestrator: the supervisor is a fixed fan-out." Its supervisor hand-rolls a `Semaphore`, a `JoinSet`, a retry-once rule and its own `Outcome` enum in 220 lines | `book/src/examples/swarm.md:5-7`; `examples/swarm/src/supervisor.rs` | VERIFIED |
| P3-3 | **The book has no "build a supervisor" chapter.** Swarm content is three pages: `client/delegation.md` (446 lines, one child), `examples/swarm.md` (166, the benchmark), `reference/swarm-extensions.md` (358, the wire contract) | `book/src/SUMMARY.md` | VERIFIED |
| P3-4 | **The budget split is a documented trap.** `BudgetHandle::grant` gives each child the smaller of what was asked and what is left, so a loop asking for the whole remainder gives the first child everything | `docs/adr/0033-group-delegation.md` "Context" item 3 | VERIFIED (as the ADR states it; I did not run it) |
| P3-5 | **Five ways to start one child.** `Delegation::start` (`delegation/mod.rs:184`), `start_with` (`options.rs:182`), `start_leased` (294), `start_budgeted` (322), `reattach_with` (241). `start_leased` and `start_budgeted` are one-line wrappers over `start_with` (`options.rs:299`, `328-333`). ADR 0033's context also names a `start_journaled` | cited lines | VERIFIED |
| P3-6 | **`Delegation` and `FailureClass` are not in the prelude**, and the swarm example needs eight direct dependencies besides the three A2A crates (`tokio`, `tokio-util`, `hyper`, `hyper-util`, `http-body-util`, `bytes`, `serde_json`, `uuid`). `tokio-util` is there for `CancellationToken`, which the server re-exports (`server/src/lib.rs:256`); `uuid` is there for message ids, which `new_message_id()` (`client/src/message_id.rs:27`, in the prelude) already makes | `examples/swarm/Cargo.toml:17-28`; `examples/swarm/src/supervisor.rs` (imports, and `uuid::Uuid::new_v4()` in `run_once`) | VERIFIED |
| P3-7 | **A bare-endpoint client silently lacks card-dependent behaviour.** `ClientBuilder::new(url)` (what `examples/swarm/src/topology.rs` uses) has no card, so `with_blocking_over_stream` "sends as it always did" and a keyed send is not retried without `with_peer_honouring_idempotency`. The card-aware path is two calls (`resolve_agent_card`, then `from_card`); there is no one-step async constructor | `client/src/builder/mod.rs:185, 235, 660-672, 960`; `client/src/methods/send_message.rs:110-120`; `grep -rnE 'pub async fn (connect\|from_url)' client/src` → transports only | VERIFIED |
| P3-8 | **Fifty clients, fifty pools unless told otherwise.** The swarm example builds one `ClientBuilder::new(url).build()` per worker and never uses `HttpPool` (in the prelude) | `examples/swarm/src/topology.rs` (`client`); `grep -n HttpPool examples/swarm/src/*.rs` → none | VERIFIED that the example does not share; the cost is INFERRED |
| P3-9 | **The example's port allocation races, against the repo's own advice.** `topology.rs:18-22` binds `127.0.0.1:0`, reads the port, drops the listener and binds again. hello-agent's test comment explains why not to: "leaves a gap in which another process can take it" | `examples/swarm/src/topology.rs:18-22`; `examples/hello-agent/src/main.rs:108-112` | VERIFIED |
| P3-10 | **Testing a supervisor: one tool, behind a feature the SDK does not forward.** `testing::ScriptedPeer` (stall, cut off, misframe, unauthorized, on each binding) is real and good. It needs `a2a-protocol-client` as a direct dependency with `testing`. There is no in-process transport, no scripted `ToolModel`, no fake clock and no record/replay in the public API | `client/src/testing/mod.rs:56-208`; `crates/a2a-protocol-sdk/README.md:91`; `grep -rniE 'MockModel\|ScriptedModel\|FakeClock\|trait Clock\|InProcess' crates/*/src` → only `ScriptedPeer` | VERIFIED |
| P3-11 | **Outcome mapping is manual.** `delegation::Outcome` is `#[non_exhaustive]` with at least `Completed`, `Failed { class, .. }`, `Canceled`, `CancelRequested`, `CancelFailed`, `Unreachable`; the example collapses them by hand, with a catch-all | `examples/swarm/src/supervisor.rs` (the `match done.outcome`); `client/src/delegation/outcome.rs:14` | VERIFIED |
| P3-12 | **A worker that wraps a model: a tool is about 22 lines each.** `Tool` has `name`, `description`, `parameters` (hand-written JSON Schema as `serde_json::Value`), `read_only`, and a boxed-future `call`. No typed-argument helper, no derive, no closure adapter, no "remote A2A agent as a tool" helper | `server/src/tool_loop/tool.rs:73-98`; `examples/tool-loop-agent/src/tools.rs:53-74`; `grep -rnE 'AgentTool\|FnTool\|tool_fn' crates/*/src` → none | VERIFIED |

### 2.4 Where docs and code disagree (code wins)

| # | Doc says | Code says | Label |
|---|---|---|---|
| D-A | `book/src/reference/api-reference.md:296` lists `set_push_config(config)` as the way to create a push config | Deprecated since 0.16.0; `create_push_config` is the method (`client/src/methods/push_config.rs:73, 103-107`) | VERIFIED |
| D-B | `server/src/executor.rs:10-11` (module doc): the framework calls `execute` for "`message/send` or `message/stream`" and `cancel` for "`tasks/cancel`" | Those are v0.3 method names; the wire methods are `SendMessage`, `SendStreamingMessage`, `CancelTask` (ADR 0053 "Context"). 15 doc-comment lines in `crates/*/src` still use the old names (`grep -rnE 'message/send\|tasks/cancel\|message/stream' crates/*/src \| grep -v test \| grep '//[/!]' \| wc -l`) | VERIFIED |
| D-C | ADR 0008: both helpers "are used extensively in the repo's examples and tests" | 5 + 5 helper uses against 34 hand-written `Box::pin(async` in examples (P1-6) | VERIFIED |
| D-D | `book/src/deployment/production.md:180`: "In 0.15 several replicas are supported only when every turn of a task is routed to one replica" | The release is 0.16.0, and turn claims across replicas are described as shipped in `docs/handoff.md` ("Cross-replica, after turn claims") | VERIFIED that the sentence names 0.15; that it is stale is INFERRED |
| D-E | ADR 0053 rule: "A duration is a `std::time::Duration`, never an integer of seconds" | Two undeprecated integer-seconds setters remain: `AuthFailureThrottleConfig::with_window_secs(u64)` (`server/src/auth/throttle.rs:131`) and `CorsConfig::with_max_age_secs(u32)` (`server/src/dispatch/cors.rs:111`). The book uses the first (`book/src/building-agents/authentication/throttling.md:44`). The rule is for new APIs, so these are inconsistencies, not violations | VERIFIED |
| D-F | ADR 0053 rule: "a client method is the wire method's name in `snake_case`" | `A2aClient::stream_message` for `SendStreamingMessage` (`client/src/methods/send_message.rs:163`); `create_push_config` for `CreateTaskPushNotificationConfig`. Pre-existing names | VERIFIED |
| D-G | `crates/a2a-protocol-sdk/src/lib.rs:6-9`: "Re-exports all three constituent crates" | The workspace has a fourth library, `a2a-protocol-discovery`, which the facade does not re-export | VERIFIED |
| D-H | Open from the 2026-11-07 walkthrough and not re-checked here: D-6 (hidden `# ` lines on crates.io READMEs), D-9 to D-13 (CLI), D-16 to D-18 | `docs/audits/2026-11-07-adopter-walkthrough/README.md` findings table | VERIFIED as listed open there |

### 2.5 Naming inconsistencies beyond ADR 0053's eight renames (VERIFIED)

- Three auth interceptor names: `ApiKeyAuthInterceptor`, `BearerTokenAuthInterceptor` (server), `BearerAuthInterceptor` (client), all in one prelude (`sdk/src/lib.rs:117-119, 131-133`).
- `with_config` is a setter on `Server` (`serve/graceful/mod.rs:230`) and a constructor elsewhere (`InMemoryTaskStore::with_config`, used at `builder.rs:1471`). ADR 0053 lists this and leaves it.
- `_arc` twins: `with_task_store` / `with_task_store_arc` (`builder.rs:318, 328`), `with_push_config_store` / `_arc` (362, 371), `ToolSet::add` / `add_arc` (`tool_loop/tool.rs:149, 159`), `build` / `build_arc`.
- `PostgresTaskStore::with_migrations(url)` is an async constructor named like a setter (`store/postgres_store/mod.rs:166`), beside `new`, `new_with_pool_size`, `from_pool`.
- `Telemetry::install_globally(bool)` is a builder setter without `with_` (`otel/telemetry/builder.rs:308`).
- `A2aRouter` and `RestRouter` are the same type under two names, both in the prelude (`sdk/src/lib.rs:153`).

---

## 3. API-surface coherence

### 3.1 Duplicated ways to do one thing (VERIFIED)

| Thing | Ways | Evidence |
|---|---|---|
| Implement an executor | hand-written `impl` with `Box::pin`; `boxed_future`; `agent_executor!` in four arms (with or without `this`, with or without `cancel`) | `executor.rs:79`; `executor_helpers.rs:110`; `executor_helpers/agent_executor_macro.rs:93` |
| Emit an artifact | `artifact(id, parts, Option<bool>, Option<bool>)`, `artifact_chunk(.., ArtifactChunk)`, `text_artifact`, `complete_with`, `complete_with_text` | `executor_helpers.rs:316`; `executor_helpers/artifacts.rs:106-192` |
| Get an `Arc<RequestHandler>` | `Arc::new(b.build()?)`, `b.build_arc()?` | `builder.rs:1230, 1265` |
| Start telemetry | `Telemetry::init()`, `init_where_configured()`, `builder()…build()`, deprecated `init_otlp_pipeline*` | `otel/telemetry/mod.rs:175, 192, 205`; `otel/pipeline.rs:98, 124` |
| Start one delegation | five entry points (P3-5) | `delegation/mod.rs:184`; `options.rs:182-322` |
| Serve | three kept `serve_with_shutdown` shapes plus seven deprecated entry points | ADR 0052 |
| Build a client | `new(url)`, `from_card`, `from_card_preferring`, `from_signed_card`; then `build()` or `build_grpc().await` | `client/src/builder/mod.rs:185, 235, 279`; `signed.rs:38`; `transport_factory.rs:165, 387` |
| Tenant resolver | `with_tenant_resolver`, `with_authorized_tenant_resolver` | `builder.rs:894`; `builder/multi_tenant.rs:34` |

### 3.2 Leaky internals in the public API

- **Third-party types in signatures (VERIFIED).** `sqlx::Error` and `sqlx::PgPool` (`store/postgres_store/mod.rs:135, 223`), `hyper::Error` in both error enums (`server/src/error/mod.rs:47`, `client/src/error/mod.rs:22`), `rustls::ServerConfig` (`serve/graceful/tls.rs:38`), `rustls::ClientConfig` (`tool_loop/openai.rs:137`), `tokio_util::sync::CancellationToken` as a public field of `RequestContext` (`request_context.rs:65`; re-exported at `lib.rs:256`), `tracing::Level` (`otel/telemetry/builder.rs:159`), the OTel SDK provider types (`otel/telemetry/mod.rs:224-236`). A major bump of any of these is a breaking change of this SDK. Only `CancellationToken` is re-exported, so the others need a matching direct dependency.
- **`RequestContext` has seven public fields and about fifteen accessors (VERIFIED).** `message`, `task_id`, `context_id`, `stored_task`, `metadata`, `cancellation_token`, `call_context` are fields (`request_context.rs:43-77`); `inbox()`, `budget()`, `lineage()`, `approval()`, `tenant()` and others are methods. It is `#[non_exhaustive]`, so adding is safe, but a reader must learn which is which. Its `with_*` builders (lines 157-292) are the handler's construction API, public because tests build contexts.
- **Span plumbing at the crate root (VERIFIED).** `BindingSpan`, `CustomBinding`, `StreamEnd`, `StreamOutcome` (`server/src/lib.rs:241`) are for someone writing a custom binding, and sit beside `RequestHandler` in the root namespace.
- **`#[doc(hidden)] pub use a2a_protocol_types as __types`** (`server/src/lib.rs:192-193`) exists for the macro. Acceptable; noted.
- **`selection::{is_compatible, canonical_binding, same_binding}`** are free `pub fn` in `client/src/builder/selection.rs:75-97`. Whether the module is public was not checked (INFERRED as possibly reachable).

### 3.3 Overly wide builders

`RequestHandlerBuilder` has 59 methods in one flat namespace (§1.3). They fall into about nine concerns: stores (6), push (2), provenance (3), profiles and escape hatches (about 9), cross-replica (halt, lease, turn claims, replica name: 9), cards (4), limits (5), metrics and tracing (7), tenancy and caller isolation (7). rustdoc lists them alphabetically, so P1 sees `accept_unisolated_tenants` and `allow_process_local_stores` before `with_agent_card`. No grouping type exists except `HandlerLimits` and `PerTenantConfig`.

### 3.4 What could be typestate or sealed

- **Typestate is not worth it here (INFERRED, a judgement).** "A handler must have a card or say `without_agent_card()`" is the one check that is always wrong to forget, and it could be `RequestHandlerBuilder::new(executor, card)`. That is a signature change needing a deprecation cycle, and the present `build()` error is immediate and names the fix. The profile checks depend on combinations of six profiles and cannot be types without an explosion. Recommendation: do not add typestate; make the failure cheaper to see instead (§6, items 1 and 7).
- **Sealing (VERIFIED as open, not assessed trait by trait).** `fuzzing_sealed.rs` exists in the server crate. Extension traits that adopters implement (`AgentExecutor`, `TaskStore`, `PushSender`, `Tool`, `ToolModel`, `Metrics`, interceptors) must stay open. Whether any trait meant only for the SDK is implementable from outside was not audited.

### 3.5 Invalid configuration caught only at run time, and whether `profile_report()` shows it

| Configuration | Caught | In `profile_report()`? |
|---|---|---|
| No card; invalid card; push declared with no sender; auth scheme mismatch; zero timeouts; zero queue capacity; turn-claim timing | `build()` → `ServerError::Config(String)` (`builder.rs:1236-1300`) | No: the report is about profiles |
| Profile requirements (authenticator, halt, lease, signer, shared stores, tenant limits, declarations) | `build()` (`builder.rs`, the `check_*` calls) | Yes: `checked`, `disabled`, `not_verifiable`, `declared` |
| Plaintext beyond loopback; an undeclared binding (under `ServeConfig::production()`); `max_connections == Some(0)` | serve time → `ServeReport::refused` (`serve/graceful/start.rs:29-47`) | **No** |
| Unbounded connections under production | serve time, a `trace_warn!` only (`start.rs:43-44`) | **No** |
| Card URL does not match where the server listens (non-production) | **Never** (P1-4) | No |
| A supplied store without metrics | **Never** (P2-8) | No |
| A client built for a bare endpoint with card-dependent options set (`with_blocking_over_stream`) | **Never**: silently sends the ordinary way (`client/src/builder/mod.rs:668-670`) | n/a |
| `Profile::Production` without the `signing` feature | `build()`, at run time, for a compile-time fact | Yes, as a refusal |
| No profile at all | Never refused; the report says "nothing was checked" | Yes, as that sentence |

### 3.6 `#[must_use]`

Coverage is close to complete (VERIFIED by a script over `crates/{server,client,types}/src`: of 636 public by-value `self -> Self` methods, 12 lack a function-level `#[must_use]`, and on inspection most of the 12 are covered by a type-level attribute or sit after another attribute the script did not look past). Genuine candidates, each to confirm by reading: `rate_limit/identity.rs:45` (`with_header`), `rpc_span/mod.rs:270` (`with_http`), `provenance.rs:209` and `client/src/content.rs:93` (`requiring_signatures`), `client/src/declared.rs:55` (`with_extended_agent_card`). `ServeReport` is `#[must_use]` with a message (`serve/graceful/mod.rs:152`). No meaningful gap. The real hole is that `tokio::spawn` discards it (P1-1), which no attribute fixes.

### 3.7 Async patterns

Consistent (VERIFIED): every public trait with async methods uses the manual `Pin<Box<dyn Future + Send + 'a>>` shape (`AgentExecutor`, `TaskStore`, `Tool`, `ToolModel`, `TokenProvider` at `client/src/token_provider.rs:147`, and others; 429 occurrences). Inherent methods are `async fn`. `EventStream` implements `futures_core::Stream` (`client/src/streaming/event_stream/poll.rs:166`) and has an inherent `next`. One oddity: ADR 0008 rejects `async-trait` in the public API, while the server's `otlp` feature depends on it (`dep:async-trait` in the `otlp` list of `crates/a2a-protocol-server/Cargo.toml`); that is an internal use for the OTel exporter traits (INFERRED, not read).

---

## 4. Tooling gaps

| Tool | State | Evidence | Label |
|---|---|---|---|
| Scaffolding | **None.** No `cargo-generate.toml` anywhere; the CLI has no `new` or `init` | `find . -name cargo-generate.toml`; `tools/a2a-cli/src/cli.rs:224-244` | VERIFIED |
| CLI commands | `card` (fetch; `sign`; `verify`), `audit verify`, `send`, `stream`, `task {get, cancel, list, subscribe}`. No push-config commands, no `doctor`, no shell completions | `tools/a2a-cli/src/cli.rs:224-244, 264-303, 344-364` | VERIFIED |
| CLI distribution | `publish = false`; binary name `a2a`; `packaging/{homebrew,winget}` exist; publishing is ROADMAP `[decision]` X-17 | `tools/a2a-cli/Cargo.toml:10-13, 20`; `ROADMAP.md` "Developer experience and defaults" item 6 | VERIFIED |
| `a2a doctor` | Proposed as E7 in the 2026-09-22 audit, not built | `docs/adopter-audit-2026-09-22.md:1320-1331`; `grep -rn doctor tools/a2a-cli/src ROADMAP.md` → none | VERIFIED |
| Inspector / debug UI | None in the repo. What stands in: three Grafana dashboards, `Telemetry::describe()` and `notices()` (`otel/telemetry/mod.rs:288, 297`), the CLI's JSON-lines stream | `deploy/observability/grafana` | VERIFIED |
| Executor test kit | `conformance::check(executor)`, `check_with`, `check_with_context`, `Report::assert_pass()` (feature `conformance`, server crate only) | `server/src/conformance/mod.rs:70-90`; `report.rs:113` | VERIFIED |
| Client test kit | `testing::ScriptedPeer` (feature `testing`, client crate only) | `client/src/testing/mod.rs` | VERIFIED |
| In-process server + client | No helper. The pattern is `Server::bind("127.0.0.1:0")`, `local_addr()`, build the card, `tokio::spawn(serve_with_shutdown(.., pending()))`: 12 lines in hello-agent's test (`main.rs:113-126`), re-written per example | `examples/hello-agent/src/main.rs:113-126`; `examples/swarm/src/topology.rs` | VERIFIED |
| Mock model, fake clock, record/replay | None public | grep in P3-10 | VERIFIED |
| Proc-macros or derives | None. One declarative macro, `agent_executor!`. No derive for skills or tools | `grep -rn 'proc-macro' --include=Cargo.toml .` → only a comment in the CLI manifest | VERIFIED |
| Typed message and part helpers | Present and good: `Message::user_text`, `agent_text`, `text()`, `texts()`; `Part::text`, `raw`, `url`, `file_bytes`, `file_uri`; `Task::text()`; `SendMessageResponse::text()`; `new_message_id()` | `types/src/message.rs:133-296, 472-566`; `types/src/task.rs:416`; `types/src/responses.rs:87` | VERIFIED |
| OpenAPI / JSON Schema output | None. No `schemars`, no OpenAPI generator. Tool parameter schemas are hand-written `serde_json::Value` | `grep -rniE 'schemars\|JsonSchema\|openapi' crates/*/Cargo.toml tools/*/Cargo.toml` → none | VERIFIED |
| MCP bridging | Examples only: `examples/mcp-agent` (agent calls MCP tools) and `examples/mcp-bridge` (A2A agent exposed as an MCP server, on `rmcp` 3.4, 1,894 lines). No library helper | `examples/mcp-bridge/Cargo.toml:33-36` | VERIFIED |
| docs.rs feature annotations | On for types, client, server, sdk (`all-features = true`, `--cfg docsrs`, `feature(doc_cfg)`). Missing for discovery | §2.1 P1-13 | VERIFIED |
| IDE discoverability | Prelude of 68-70 names helps. Against it: 59 builder methods in one list (§3.3), root namespaces of 123 and 40 names, two names for one router | §1.4 | VERIFIED counts; effect INFERRED |
| Mechanical gates on docs | Strong: README and 169 book pages compile as doctests; `check_feature_tables.py`, `check_doc_versions.py`, `check_book_examples.py`, `check_api_reference.py`, `check_example_servers.py` and others in `scripts/` | `ls scripts`; `book-tests/src/lib.rs` | VERIFIED |

---

## 5. Operability as developer experience

**What P2 gets (VERIFIED):**
- First-run diagnostics: `Telemetry` start-up summary and `describe()`; `deploy-agent` logs the redacted database URL, pool sizes and the retention setting; a `WARN` when `DATABASE_URL` is used as the fallback.
- `RequestHandler::profile_report()` with a `Display` impl (`profile_report.rs:92-111`).
- `/health`, `/ready`, `/load` on `RestDispatcher`, `CombinedDispatcher`, `RestRouter`; readiness reaches the task store with a constant-cost statement and turns to `draining` on shutdown (`book/src/deployment/health-checks.md`).
- Config validation at `build()` with messages that name the fix.
- A two-stage `Dockerfile` taking `PACKAGE` as a build argument, a signed published image, `deployment.yaml` and `network.yaml` (P2-14).
- Observability stack: `deploy/observability/` (compose files, Prometheus rules, Alertmanager, Grafana, Loki, Tempo, collector) and `deploy/kubernetes/observability/` (PrometheusRule, dashboards ConfigMap, kustomization).
- Book: Operations Runbooks (216 lines), Troubleshooting, Alerts and runbooks, Running More Than One Replica (five sub-pages), Configuration Reference (467 lines).

**What P2 still has to invent:**

| # | Gap | Evidence | Label |
|---|---|---|---|
| O-1 | The production handler itself: PostgreSQL stores + JWT interceptor + halt, lease, outbox + signer + profiles, in one program | P2-1, P2-2 | VERIFIED |
| O-2 | Environment parsing, URL redaction, pool sizing for their own crate (about 150 lines in deploy-agent, not reusable: it is an example binary) | P2-9 | VERIFIED |
| O-3 | A start-up line that says what the listener checked: `profile_report()` omits it | §3.5 | VERIFIED |
| O-4 | Health endpoints if they serve JSON-RPC alone, gRPC or WebSocket on their own listener | `health-checks.md:17-18` | VERIFIED |
| O-5 | Manifests for the fuller shape: no Secret for a JWT issuer or signing key, no HorizontalPodAutoscaler (the `/load` signal exists), no migration Job (schema is created at start under an advisory lock) | `grep -n 'kind:' examples/deploy-agent/*.yaml` → Deployment, Service, PodDisruptionBudget, NetworkPolicy, Ingress | VERIFIED for what is present; that the rest is needed is INFERRED |
| O-6 | A preflight that runs before traffic: `a2a doctor` is unbuilt (§4) | §4 | VERIFIED |
| O-7 | Metrics from their own stores (P2-8) | P2-8 | VERIFIED |
| O-8 | Not verified by anyone: `docker build` of the image, the manifests against a schema, a pod running | `docs/audits/2026-11-07-adopter-walkthrough/README.md` "Not verified" | VERIFIED as stated there |

---

## 6. Ranked improvements

Ordered by value to the three personas against cost and risk. Each prefers removing a concept or using what already ships over adding surface. "Compatibility" is against `STABILITY.md` §2-§3 (deprecate first; removal no earlier than the next minor; at most one breaking minor per month; `cargo-semver-checks` in CI). The repo's convention for verification is a `scripts/check_*.py` gate with a `--self-test`, proven to fail by `scripts/prove_gates_fail.sh`; each item names one.

### 1. Make a refused start impossible to drop: `ServeReport::into_result`

- **Problem.** P1-1, P1-2. The README Quick Start loses the refusal; hello-agent spends 6 lines on it.
- **Change.** One method, no new entry point (ADR 0052 keeps the three serve shapes):
  ```rust
  impl ServeReport {
      /// `Err` when the server refused to start, carrying `refused`.
      pub fn into_result(self) -> std::io::Result<Self>;
  }
  ```
  Then `server.serve_with_shutdown(d, shutdown_signal()).await.into_result()?;`. In the README Quick Start, keep the `JoinHandle` and check it, or await the server and run the client from a second task.
- **Files.** `crates/a2a-protocol-server/src/serve/graceful/mod.rs`; `README.md:204-208`; `book/src/getting-started/quick-start.md`; `examples/hello-agent/src/main.rs:89-98`; `book/src/deployment/production.md` (the Production example).
- **Verify.** A unit test that a report with `refused: Some(..)` gives `Err` whose message contains the reason. A gate in `scripts/check_readme_doctests.py` (or a new check) that fails on `tokio::spawn(` whose argument contains `serve_with_shutdown(` and whose result is not bound, in `README.md`, `crates/*/README.md` and `book/src/getting-started/`.
- **Compatibility.** Additive. `ServeReport` is `#[non_exhaustive]`; adding a method is not a break.
- **Over-engineering risk.** Low. Do not add `Server::run`.

### 2. Build ADR 0033's `DelegationGroup`, then make the swarm example use it

- **Problem.** P3-1 to P3-5, P3-11. The design is written and accepted as needed; every supervisor re-implements it.
- **Change.** As ADR 0033 "Decision" specifies, in `a2a_protocol_client::delegation`. A sketch consistent with it and with ADR 0053 naming:
  ```rust
  pub struct DelegationGroup { /* … */ }
  impl DelegationGroup {
      pub fn new(max_in_flight: std::num::NonZeroUsize) -> Self;   // required cap, no default
      pub fn with_join(self, join: Join) -> Self;
      pub fn with_budget(self, parent: &BudgetHandle, split: BudgetSplit) -> Self;
      pub fn add(&mut self, client: Arc<A2aClient>, params: MessageSendParams);
      pub fn add_with(&mut self, client: Arc<A2aClient>, params: MessageSendParams, options: DelegationOptions);
      pub async fn run<F: Future<Output = ()>>(self, cancel: F) -> Vec<MemberOutcome>;
  }
  #[non_exhaustive] pub enum Join { All, FirstOk, Quorum(usize), FailFast }
  #[non_exhaustive] pub enum BudgetSplit { Equal, Fixed(/* … */), Weighted(/* … */) }
  #[non_exhaustive] pub enum MemberOutcome { Settled(Delegated), NotStarted(ClientError) }
  pub fn retryable(outcomes: &[MemberOutcome]) -> impl Iterator<Item = usize> + '_;
  ```
  Re-export `Delegation`, `DelegationGroup`, `Join` and `FailureClass` from the SDK prelude. Add one book page, "Building a supervisor", under Client Usage, whose program is a 50-worker supervisor in under 60 lines.
- **Files.** New `crates/a2a-protocol-client/src/delegation/group.rs` (+ tests); `delegation/mod.rs`; `crates/a2a-protocol-sdk/src/lib.rs` (prelude); `examples/swarm/src/supervisor.rs` (replace `Semaphore` + `JoinSet`); new `book/src/client/supervisor.md`; `book/src/SUMMARY.md`; `book-tests/src/lib.rs`; ADR 0033 status.
- **Verify.** Tests against `testing::ScriptedPeer` and loopback agents: the cap is never exceeded; `FirstOk` cancels and awaits the rest; equal split over N members never reserves more than the parent had; results are in input order; dropping `run` cancels every started member. `examples/swarm` keeps its exit-code contract (`fanout`, `faults`, `cancel` accounting). A line-count ratchet: `examples/swarm/src/supervisor.rs` shrinks (set the bound when built).
- **Compatibility.** Additive. Prelude additions are additive but can shadow a user's own `Join` under a glob import; `Delegation*` names are unlikely to collide, `Join` might. Consider exporting `Join` only by path.
- **Over-engineering risk.** Medium. ADR 0033 "What stays out" already fences it: no DAGs, no planning, no placement, no retry of settled members. Hold to that. Do not also keep adding `start_*` variants (see item 9).

### 3. Bring hello-agent to 30 code lines or fewer, using only what ships plus item 1

- **Problem.** §1.1: 48 lines against a "<30" expectation. 19 of the 48 are telemetry error handling and the refusal check.
- **Change.** No new API beyond item 1. In `examples/hello-agent/src/main.rs`: `main` returns `Result<(), Box<dyn std::error::Error>>`; telemetry becomes `let telemetry = Telemetry::builder().with_default_service_name("hello-agent").with_export_where_configured().build()?; telemetry.install_subscriber();`; the handler becomes `RequestHandlerBuilder::new(HelloAgent).with_agent_card(card(url)).build_arc()?`; the serve call ends in `.into_result()?`. The friendlier "OTEL_* environment" message moves to `deploy-agent`, where exit codes matter.
- **Files.** `examples/hello-agent/src/main.rs`, its README, `book/src/examples/hello-agent.md`, `book/src/getting-started/quick-start.md`, `README.md:252` ("about 50 lines").
- **Verify.** A check script (extend `scripts/check_book_examples.py` or add `check_hello_agent_size.py`): `sed -n '1,/^#\[cfg(test)\]/p' examples/hello-agent/src/main.rs | grep -vcE '^\s*(//|$)'` ≤ 30, with a self-test. The existing rule "one dependency, no feature flags" (file header, lines 17-22) stays. hello-agent's own tests still pass.
- **Compatibility.** None; an example.
- **Over-engineering risk.** None. This is subtraction.

### 4. One production reference: deploy-agent applies profiles and prints its report; one book page assembles everything

- **Problem.** P2-1, P2-2, O-1, O-3. The profile system is the SDK's answer to "is this configuration fit for production", and no example runs it.
- **Change.** (a) `examples/deploy-agent`: always log `handler.profile_report()` at start. With no database, apply `Profile::LocalDevelopment`. With `A2A_DATABASE_URL` set and an authenticator configured, apply `Profile::DurableProduction` and add `PostgresPushOutbox`. Read the authenticator from the environment under ADR 0040's naming rule (`A2A_` prefix), for example `A2A_JWT_ISSUER` + `A2A_JWT_AUDIENCE` behind the `auth-jwt` feature; with a database and no authenticator, log at `WARN` that no profile is applied and why. (b) New book page "A production handler, whole" under Testing & Deployment: one compiled program with `PostgresTaskStore`, `PostgresPushConfigStore`, `PostgresPushOutbox`, `PostgresHaltStore`, `PostgresLeaseStore`, `JwtAuthInterceptor`, `ContentSigner`, `Profile::Production` + `DurableProduction`, `ServeConfig::production().with_tls_terminated_in_front()`, one shared pool through `from_pool`, `with_metrics` on each store. (c) Add the listener's checks to the report:
  ```rust
  impl ServeConfig {
      /// What a listener under this configuration refuses and warns about, as text for a start-up log.
      pub fn describe(&self) -> String;
  }
  ```
  Prefer this to widening `ProfileReport`, which is the handler's.
- **Files.** `examples/deploy-agent/src/main.rs`, `Cargo.toml`, `README.md`, `deployment.yaml` (the new variables and a Secret reference), `src/tests.rs`; new `book/src/deployment/production/whole.md`; `book/src/SUMMARY.md`; `book-tests`; `book/src/reference/configuration.md` (the variable table ADR 0040 §3 requires); `serve/graceful/config.rs`.
- **Verify.** deploy-agent tests: with a database and no authenticator, the log names the missing authenticator; with both, `profile_report()` lists `DurableProduction`. A gate: `grep -c 'with_profile' examples/deploy-agent/src/main.rs` ≥ 1 and the new book page contains all of `PostgresTaskStore`, `Profile::Production`, `with_halt_store`, `with_lease_store`, `with_content_signer` (a `check_*.py` with a self-test). The page compiles in `book-tests`.
- **Compatibility.** `ServeConfig::describe` is additive. deploy-agent's behaviour with only a database is unchanged except for one more log line; ADR 0040 "Consequences" sets the precedent for that.
- **Over-engineering risk.** Medium on (a): do not turn deploy-agent into the gateway binary ADR 0040 §4 rejected. It stays a trivial agent; only the wrapper grows. If the JWT variables feel like scope creep, do (b) and the report line alone.

### 5. Make the examples teach the short forms the SDK already has, and hold it with a ratchet

- **Problem.** P1-5, P1-6, P1-7, P3-6, P3-9, D-C. The ergonomic API exists and the examples, which adopters copy, bypass it.
- **Change.** No API. Mechanical edits across `examples/`: `Arc::new(b.build()?)` → `b.build_arc()?`; `emit.artifact(id, vec![Part::text(t)], None, Some(true))` → `emit.text_artifact(id, t)` or `complete_with_text`; hand-written `impl AgentExecutor` on a non-generic struct → `agent_executor!(T, |this, ctx, queue| async { … })`; `Message::user(uuid…, vec![Part::text(x)])` → `Message::user_text(new_message_id(), x)`; `tokio_util::sync::CancellationToken` → the server's re-export; probe-drop-rebind → bind once and keep the `Server` (as `hello-agent/src/main.rs:113-126`). Correct ADR 0008's "used extensively" sentence or make it true.
- **Files.** `examples/*/src/**/*.rs` (start with `swarm`, `deploy-agent`, `tool-loop-agent`, `incident-response`); `docs/adr/0008-agent-executor-trait-shape.md`; a new baseline file beside `.file-length-baseline`.
- **Verify.** `scripts/check_example_idioms.py` with a shrink-only baseline (the mechanism `.file-length-baseline` and `.book-ignore-baseline` already use): counts of `Box::pin(async` (34 today), `emit\w*\.artifact\(` (19), `Arc::new(` within four lines of `.build()` (31), `drop(probe)` after a `:0` bind (3) may only go down. Self-test with planted regressions.
- **Compatibility.** None.
- **Over-engineering risk.** Low. Risk is churn in `agent-team` (9,194 lines); exempt it in the first pass through the baseline.

### 6. Forward `testing` and `conformance` through the SDK

- **Problem.** P3-10, K3 (open since 2026-09-22). The two test kits need the client and server crates as direct dependencies, which breaks the "one dependency" story at the moment an adopter writes a test.
- **Change.** In `crates/a2a-protocol-sdk/Cargo.toml`: `testing = ["a2a-protocol-client/testing"]`, `conformance = ["a2a-protocol-server/conformance"]`. Both add no dependencies (their manifests say so). Leave `proto` unforwarded unless asked.
- **Files.** `crates/a2a-protocol-sdk/Cargo.toml`, `README.md:76-92`; `book/src/getting-started/installation.md:115-148`; `book/src/deployment/testing/conformance.md`.
- **Verify.** `scripts/check_feature_tables.py` already compares tables to manifests; it must pass with the two new rows. A doctest in the SDK crate using `a2a_protocol_sdk::client::testing::ScriptedPeer` and `a2a_protocol_sdk::server::conformance::check` under those features. `cargo tree -p a2a-protocol-sdk --features testing,conformance -e normal` count equals the default count (124).
- **Compatibility.** Additive.
- **Over-engineering risk.** None.

### 7. A test-kit helper for "serve this executor on loopback and give me a client"

- **Problem.** §4: the 12-line bind / card / spawn pattern is re-written in every example and will be in every adopter's test; the swarm copy is racy (P3-9).
- **Change.** In the server crate's `conformance` module (already the dev-dependency surface; no new feature), one function that removes the pattern rather than adding a framework:
  ```rust
  /// Binds 127.0.0.1:0, builds `card(url)` with the bound address, serves `executor`
  /// over JSON-RPC and HTTP+JSON until the returned guard is dropped.
  pub async fn serve_on_loopback(
      executor: impl AgentExecutor,
      card: impl FnOnce(&str) -> AgentCard,
  ) -> std::io::Result<LoopbackAgent>;

  pub struct LoopbackAgent { /* … */ }
  impl LoopbackAgent {
      pub fn url(&self) -> &str;
      pub fn handler(&self) -> &Arc<RequestHandler>;
      pub async fn shutdown(self) -> ServeReport;
  }
  ```
  The client stays the caller's (`ClientBuilder::new(agent.url())`), so the server crate still does not depend on the client (README "siblings").
- **Files.** `crates/a2a-protocol-server/src/conformance/` (new `loopback.rs`); `book/src/deployment/testing/unit-and-integration.md`; `examples/hello-agent/src/main.rs` tests; `examples/swarm/src/topology.rs`.
- **Verify.** Its own test: two agents started concurrently get different ports and both answer; dropping the guard closes the listener. `grep -c 'drop(probe)' examples -r` → 0.
- **Compatibility.** Additive, behind `conformance`.
- **Over-engineering risk.** Medium. Keep it to this one function. A scripted `ToolModel`, a fake clock and record/replay are each a separate decision; I would not start them without a named adopter need. A scripted model is the most defensible of the three, because `ToolModel` is a public trait and every tool-loop test needs one.

### 8. Serve `/health` and `/ready` from `JsonRpcDispatcher`

- **Problem.** P1-10, O-4. The first dispatcher an adopter meets cannot be probed, and the book's remedy is 15 lines of `hyper`.
- **Change.** `JsonRpcDispatcher` answers `GET /health`, `GET /ready` and (with `with_load_reporting`) `GET /load` exactly as `RestDispatcher` does (`dispatch/rest/mod.rs:162-166`). JSON-RPC is `POST` on the base path, so the paths do not collide. Alternative with no behaviour change: put `CombinedDispatcher` in the prelude and use it in hello-agent; that forces the card to declare two interfaces, so it is the worse option for a first agent.
- **Files.** `crates/a2a-protocol-server/src/dispatch/jsonrpc/mod.rs`; `book/src/deployment/health-checks.md:12-40`; tests beside the REST ones.
- **Verify.** A dispatcher test: `GET /health` → 200 `{"status":"ok"}`; `GET /ready` → 503 `draining` after `begin_draining()`. The in-repo TCK still passes on JSON-RPC.
- **Compatibility.** A behaviour change on the wire (a path that answered an error now answers 200). Not a Rust API break. It needs a `CHANGELOG` entry and, since `health-checks.md` documents "not served" as the contract, a maintainer decision.
- **Over-engineering risk.** Low.

### 9. Stop the duplication from growing, and retire what the deprecations allow

- **Problem.** §3.1, §2.5, D-A, D-B, D-E.
- **Change.** (a) Do ROADMAP item `[0.17]` 4: remove the 23 items whose notes say "removed no earlier than 0.17.0", and give the 6 removable ones without a stated version a version (the two proto-mirrored enum variants stay). Add the gate ADR 0053 "Consequences" says is missing: every `#[deprecated]` outside `generated/` names a removal version. (b) Deprecate `Delegation::start_leased` and `start_budgeted` in favour of `start_with(.., DelegationOptions)` when item 2 lands, so the group has one option type. (c) Add `AuthFailureThrottleConfig::with_window(Duration)` and `CorsConfig::with_max_age(Duration)` and deprecate the `_secs` forms, as ADR 0053 did for `RateLimitConfig`. (d) Fix D-A and D-B in the docs.
- **Files.** Those listed in §1.7; `client/src/delegation/options.rs:294-340`; `server/src/auth/throttle.rs:131`; `server/src/dispatch/cors.rs:111`; `book/src/reference/api-reference.md:296`; `server/src/executor.rs:10-11` and the other 13 doc lines; `book/src/building-agents/authentication/throttling.md:44`; `book/src/reference/upgrading/` (a 0.16 → 0.17 page).
- **Verify.** `cargo-semver-checks` reports exactly the removals listed in `### Breaking Changes`. New gate `scripts/check_deprecations.py`: each `#[deprecated(` has `since` and "removed no earlier than X.Y.0", with an allow-list for `generated/`. `grep -rnE 'message/send|tasks/cancel|message/stream' crates/*/src | grep '//[/!]'` → 0 outside upgrade notes.
- **Compatibility.** (a) is a breaking minor, allowed by `STABILITY.md` §3.1 from 0.17.0, one per calendar month. (b) and (c) are deprecations, non-breaking now.
- **Over-engineering risk.** Low; this is subtraction. Do not attempt ADR 0053's rejected option 2 (normalising every builder verb).

### 10. Group the handler builder's documentation, not its API

- **Problem.** §3.3: 59 methods, alphabetical in rustdoc.
- **Change.** No signature changes. Split the one large `impl RequestHandlerBuilder` (`builder.rs:252-1265`) into several `impl` blocks, each with a doc comment heading ("Stores", "Agent card", "Across replicas", "Tenants and callers", "Escape hatches", …). rustdoc renders each block's methods together under its comment. The crate already does this for `multi_tenant.rs` and `steering.rs`. As a side effect `builder.rs` (2,699 lines) can leave `.file-length-baseline`.
- **Files.** `crates/a2a-protocol-server/src/builder.rs` and new files under `builder/`; `.file-length-baseline`.
- **Verify.** `cargo-semver-checks` reports nothing. `scripts/check_file_lengths.sh` passes with `builder.rs` removed from the baseline. `grep -c 'pub (const )?fn' ` over `builder.rs` + `builder/*.rs` still totals 59.
- **Compatibility.** None.
- **Over-engineering risk.** None. The alternative, sub-builders such as `.replicas(|r| r.with_halt_store(..))`, adds a second way to set everything and should not be done.

### 11. `a2a doctor URL` (E7), as the preflight P2 lacks

- **Problem.** O-6; §4. Proposed 2026-09-22 with a shape and a failing-first test, never built.
- **Change.** As E7 specifies: fetch and validate the card; for each declared interface connect and send one message; report latency and the error class; non-zero exit on any finding. Add `GET /health` and `/ready` when present.
- **Files.** `tools/a2a-cli/src/cli.rs`, `commands.rs`; `tools/a2a-cli/README.md`; `book/src/reference/cli.md`.
- **Verify.** The test E7 names: an agent whose card advertises gRPC on a closed port makes `doctor` exit non-zero naming that interface. Against `hello-agent`, exit 0.
- **Compatibility.** Additive; the CLI is unpublished.
- **Over-engineering risk.** Low if it stays read-mostly. It sends one real message, so it needs `--no-send` for production agents.

### 12. Smaller items, each additive

| Item | Problem | Change | Verify |
|---|---|---|---|
| Card helper for the common auth scheme | P2-10 | `AgentCard::with_bearer_auth(self, scheme_name: impl Into<String>) -> Self` in `types/src/agent_card/builders.rs`, also adding the matching `SecurityRequirement` | The Production example in `production.md` loses its `HashMap::from` block and still passes `build()` |
| docs.rs metadata for discovery | P1-13 | Add `[package.metadata.docs.rs]` to `crates/a2a-protocol-discovery/Cargo.toml` | `grep -c 'metadata.docs.rs' crates/*/Cargo.toml` → 5 |
| Supplied stores inherit metrics | P2-8 | In `build()`, when the supplied store has none, hand it the handler's `MetricsHandle` through a new defaulted `TaskStore` method, or document the call in the page from item 4 | A test that a supplied `SqliteTaskStore` records `a2a.server.store_retries` without an explicit `with_metrics` |
| Facade doc | D-G | Say "three of the four library crates"; name `a2a-protocol-discovery` | read |

### Things I considered and recommend against

- **A proc-macro crate (`#[agent]`, `#[tool]`, derive for skills).** It would add `syn`/`quote` to every build (the CLI manifest already notes their cost), a fifth published crate, and a second way to write an executor. ADR 0002's dependency philosophy and ADR 0008's "no third-party macro crate in the public API" both point away from it. A closure-based `Tool` adapter with typed arguments via `serde` would cover most of P3-12 without a proc-macro; propose it only if tool-loop adoption shows the need.
- **Typestate builders** (§3.4).
- **A config-file or `from_env` layer in the SDK.** ADR 0040 decided against a gateway binary and against renaming variables. Item 4's reference program is the cheaper answer.
- **Turning `otlp` off by default in the SDK** to cut the 124-crate default. It was a deliberate decision (2026-10-14, recorded in `crates/a2a-protocol-sdk/Cargo.toml`), and it changes behaviour for every adopter. If compile time becomes a reported complaint, measure `cargo build --timings` first.
- **`ServerError::Config` as a typed enum.** 38 sites, a breaking change, and the consumers are humans reading a start-up log.

---

## 7. What was not checked

- Nothing was compiled or run, so every proposed signature is a sketch and every "the example would shrink to N lines" is an estimate.
- `docs/adr/0025-production-profile.md` and `crates/a2a-protocol-client/src/client.rs` were not read in full; profile behaviour is taken from `profile.rs` and the book, client behaviour from the builder and method modules.
- `crates/*/README.md` were sampled (the SDK's feature section, the server's `serve()` row), not read end to end. `examples/README.md` (137 non-empty lines) was not read.
- Example `main.rs` files read in full: hello-agent, deploy-agent, swarm (`supervisor.rs`, `topology.rs`), tool-loop-agent (`tools.rs` in part). The other twelve were measured, not read.
- The `#[must_use]` scan is a line-based heuristic.
- The `cargo tree` counts are normal dependencies on this host's target (Linux); other targets differ.
- Whether any pre-2026-11-04 entry in `docs/handoff.md` "Open engineering" is still open (the section itself says to re-check).
