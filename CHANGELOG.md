<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

Several replicas can now share one PostgreSQL database (or several processes
one SQLite file) with no session affinity: each turn of a task claims it in
the database, so two replicas never run one task at once, a continuation
starts from everything the last turn stored wherever it ran, a cancel reaches
the executor on another replica within about a second, a replica that dies
mid-turn hands its tasks over after the claim's time to live, and a replica
that stalled past it writes nothing more. Found missing, with the other
entries here, by running the repository's own binaries as replicas behind a
load balancer, against a real model (llama.cpp `llama-server` b11913 with
Qwen3.5-2B and Gemma 4 E2B) and a real observability stack (OpenTelemetry
Collector 0.163.2, Prometheus 3.15.1, Tempo 2.9.0, Loki 3.7.9, Grafana
13.2.4), 2026-10-28.

### Added

- **`a2a-protocol-discovery`, a new crate: agents announced and found on
  the local network with mDNS / DNS-SD** (audit B-24; ADR 0029, proposed).
  `Announcer::announce(&Advertisement::new(name, port))` announces
  `_a2a._tcp.local.` with TXT `path=` and `scheme=`; `discover(window)` and
  `Browser` find agents, and `DiscoveredAgent::fetch_card` fetches the card
  through the client's card discovery, which can require signatures.
  Announcements are unauthenticated and card URLs leave out loopback and
  IPv6 link-local addresses. One link only; not a registry. Not published
  yet (`publish = false`). Tested over real multicast
  (`tests/mdns_round_trip.rs`) and live with two agent processes; a book page
  under Agent Cards & Discovery.
- **`A2aError::with_retry_after`** (types) and
  **`RequestHandler::admits_sends`** (server): a temporary failure that
  names a retry delay, which says the request was refused before any work
  so even a send may be repeated, and whether a handler still takes sends.
  Both serve the shutdown change under *Behaviour Changes*.
- **The client trusts a private CA, a pinned self-signed certificate, or
  presents a client certificate, on the HTTP bindings** (client;
  `ClientBuilder::with_tls_config`, `tls::tls_config_from_pem`,
  `TlsConfig::Custom`). Until now the JSON-RPC and HTTP+JSON transports
  trusted Mozilla's roots and nothing else whatever the builder was given, so
  an agent behind an internal CA could not be called over either (audit
  B-01, G-01; `tests/private_ca.rs`).
- **The client reads `SSL_CERT_FILE` and `SSL_CERT_DIR`** (client), as curl,
  OpenSSL and Go do: the certificates they name are trusted beside Mozilla's
  roots on every HTTP call the client makes (both HTTP bindings, card
  discovery, JWKS, the token provider). A container's CA bundle, or a
  TLS-inspecting proxy's root, needs no code.
- **The client goes through `HTTPS_PROXY`, `HTTP_PROXY`, `ALL_PROXY` and
  `NO_PROXY`** (client; upper or lower case, curl's `NO_PROXY` rules), on the
  same paths: `CONNECT` for `https://`, a forward request with
  `Proxy-Authorization` for `http://`. Only the OTLP exporter read them
  before, so in a network whose only way out is a proxy, telemetry was
  exported and every agent call failed (audit B-02;
  `tests/proxy_and_cert_env.rs`). The WebSocket and gRPC transports go
  through the same proxy as a `CONNECT` tunnel, their TLS and handshake
  inside it, a `ws://`/`wss://` URL matched as its `http://`/`https://`
  equivalent (`tests/websocket_through_proxy.rs`, `ws://` and `wss://`;
  the SDK's `tests/grpc_through_proxy.rs`; each shown failing with its part
  reverted). Run live through Squid 6 with the CLI on both bindings
  (`TCP_TUNNEL/200` in its access log). Without `tls-rustls` they connect
  directly.
- **`TaskStore::health()`** (server; provided method): what readiness asks.
  The SQL stores answer with one statement whose cost does not grow with the
  table (`SELECT pg_is_in_recovery()`, `SELECT 1`), and a PostgreSQL standby
  is not ready. The default asks `count()`, so a custom store still compiles.
- **Tasks a dead replica left running are ended** (server, the PostgreSQL
  and SQLite stores; `handler::claims::reaper`, `TurnClaimStore::abandoned`):
  every handler with turn claims looks, once per claim time to live, for
  tasks `submitted` or `working` with no live claim and no write for a time
  to live, takes each one's claim, and ends it `failed` with failure class
  `transient` and a message saying the replica running it stopped; its
  webhooks are sent the event, and a subscriber on another replica sees it
  at its next poll. Run live (three replicas behind HAProxy on PostgreSQL
  17): 70 s after the replica running a task was killed with `SIGKILL`, the
  task still answered `working`, and nothing would have ended it (audit
  E-F1). Tested on all four SQL stores (`handler/claims/reaper_tests.rs`;
  two mutants of it, each caught).
- **`ClientConfig::send_timeout` and `ClientBuilder::with_send_timeout`**
  (client; also `GrpcTransportConfig` and `WebSocketTransportConfig`, and
  the CLI's `--send-timeout`): a blocking `SendMessage` waits up to ten
  minutes for the agent's turn, where every other call keeps its 30 s
  request timeout, and running out says what to do (stream, or
  `return_immediately` and follow the task). Run live against an agent
  calling a local model: a 60 s turn failed at 30 s with `request timed
  out`, the CLI exited 1, the task went on running, and the caller never
  learned its id (audit D-01; `tests/blocking_send_timeout.rs`).
- **`TelemetryBuilder::with_span_level`** (server; default `INFO`, the level
  of every span this SDK makes): see *Fixed*.
- **`ServeConfig::with_drain_delay`, `RequestHandler::readiness`,
  `begin_draining` and `is_draining`** (server): on its shutdown signal a
  `Server` marks the handler draining, so `/ready` answers `503 draining` at
  once, and goes on accepting for the delay (zero by default) while a
  Kubernetes Service's endpoints and the proxies in front learn the pod is
  leaving; then it stops as before. A server that stopped accepting at once
  refused the requests still routed to it on every rollout (audit A5).
  `finish_in_flight` begins draining too, for an axum server.
- **`ServeConfig::with_allowed_hosts` and `without_host_check`** (server):
  see the loopback `Host` check under *Behaviour Changes*.

- **`PostgresTaskStore::pool_options()`** (server): the pool options `new`
  uses (a ping only for a connection idle a second or more), for a caller
  who builds the pool for `from_pool`. Its docs say which stores may share
  such a pool: the PostgreSQL task stores, not the push-config, rate-limit
  or audit stores.
- **The `TaskStore` trait states what a write retried by the handler must
  keep** ("Writes the handler makes again"): `save`, `save_status_delta`,
  `save_artifact_delta` and `append_event` leave the store as one call would
  when called twice, and "not now" is only for a failure a later call may
  get past. It was documented only on `HandlerLimits::store_write_retry`,
  where a custom store's author would not look. `tests/retried_statements.rs`
  now runs those writes twice on all six stores in this crate, the in-memory
  and SQLite ones in every test run.

- **`Metrics::on_store_retry(operation, cause)`** and the OpenTelemetry
  counter `a2a.server.store_retries` (server): every store write a turn makes
  again because the store said "not now" (`cause = "unavailable"`, under
  `store_write_retry`), and every PostgreSQL statement run again on a new
  connection because its first lost one (`operation = "statement"`,
  `cause = "connection_lost"`), reported through the store's `with_metrics`.
  Their log lines are now throttled to one per ten seconds per kind: a
  failover met every statement in flight and logged a warning for each. The
  trait method has a default body, so existing implementations still compile;
  the turn claims a PostgreSQL store hands out report through its metrics too.

- **`HandlerLimits::store_write_retry`** (server; default 30 s): how long a
  turn's event consumer goes on writing one event while the task store
  answers that it is unavailable for now (*Fixed*).
- **Turn claims: one replica at a time per task** (server; `store::claim`).
  The PostgreSQL and SQLite stores, plain and tenant-aware, keep a table,
  `a2a_task_claims`, beside their tasks, and the handler takes a row in it
  for each turn: a send claims its task once it knows it (after an idempotent
  replay is answered), waits for a claim another turn holds for up to
  `executor_drain_timeout` and is then refused as a send to a running task is
  on its own replica, reads a continued task again under the claim, and
  releases it when the turn's executor and its event consumer have both
  ended. Every task write and event-log append a turn makes is fenced by the
  claim's token in the same statement (on PostgreSQL locking the claim row
  `FOR KEY SHARE`, so a write that waited through a takeover is refused too),
  so a replica whose claim was taken over is refused (`ClaimLost`) and its
  executor cancelled. The owner renews the claims it holds, by token, every
  second; a `CancelTask` on another replica marks the claim and the owner's
  next renewal fires the executor's token, with or without a halt store; a
  dead replica's claims expire after 15 s and the next send takes the task
  over; a replica that shuts down releases its claims at once. A send that
  waits for another replica's turn gives up its idempotency key while it
  waits, and an approval toward a quorum is counted under the claim, so
  approvals on two replicas at once are both counted. `build` refuses
  `with_turn_claims` over a store with claims of its own. New: `store::claim` (`TurnClaimStore`, `HeldClaim`, `ClaimLost`,
  `CLAIM_LOST_MARKER`, `current_fence`, `InMemoryTurnClaims`,
  `PostgresTurnClaims`, `SqliteTurnClaims`), the provided method
  `TaskStore::turn_claims` (default `None`: no claims, as before), and the
  builder's `with_turn_claims`, `without_turn_claims`,
  `with_turn_claim_timing` and `with_replica_name`, with
  `DEFAULT_TURN_CLAIM_TTL`, `DEFAULT_TURN_CLAIM_RENEWAL` and
  `MIN_TURN_CLAIM_TTL`. PostgreSQL migration 8 and SQLite migration 9 create
  the table; `from_pool` does too. Tests: `tests/turn_claims/` (each case on
  SQLite and PostgreSQL), `tests/replica_processes.rs` (replicas as separate
  processes, one killed mid-turn: taken over 2.82 s after the kill with a 3 s
  time to live), `tests/multi_replica.rs`
  `the_single_writer_refusal_crosses_replicas`, and
  `tests/soak_multi_replica.rs`, which now checks that every claim is
  released by the end of a run. An independent review of the first version
  found six defects (a renewal racing a new claim cancelled it; renewal by
  owner kept a forgotten claim alive; the PostgreSQL fence read a stale
  snapshot; the quorum count, and a waiting send's key, were outside the
  claim; foreign claims over a fenced store refused every write); each is
  fixed here with a test that fails without the fix.
- **`deploy/observability/docker-compose.yml`**: the whole stack in one
  command, a Collector receiving all three signals and sending metrics to
  Prometheus (through its OTLP receiver, so the dashboards and rules work
  unchanged), traces to Tempo and logs to Loki, and Grafana provisioned with
  the three dashboards and Tempo and Loki data sources linked both ways (a
  span to its logs, a log line to its trace). Before, the Collector
  configuration sent traces and logs only to its `debug` exporter, and the
  README's addresses named services nothing defined.
- **Rolling upgrades from 0.15 need no affinity and no ordering**
  (PostgreSQL; `store::claim::upgrade`). Every write this release makes marks
  itself with a transaction-local setting; triggers on `tasks`, `task_events`,
  `tenant_tasks` and `tenant_task_events` refuse an unmarked write (an
  earlier release's) to a task a newer replica holds a claim on, except a
  cancel, which wins and stops the newer replica's executor; an unmarked
  write that leaves a task `submitted` or `working` claims it until 5 minutes
  after its last write (`a2a.legacy_claim_ttl`), so newer replicas wait for
  that turn, and one that parks or ends the task releases it. Both sides take
  a per-task advisory lock before they look. Installed by every PostgreSQL
  store at startup and by PostgreSQL migration 9. Tested against a real
  0.15.0 replica process (4 of 4 cases; 1 of 4 with the triggers disabled)
  and in `tests/turn_claims/upgrade.rs`, which replays 0.15.0's statements
  and checks that every write this release makes is marked (each of five
  mutants of the guard fails at least one case). Its cost to a fleet that has
  finished upgrading was not distinguishable from noise (334 ± 31 pairs/s
  with the triggers, 344 ± 41 without, three runs each). SQLite gets none of this:
  processes sharing one file are on one host and are replaced together.
- **`deploy/observability/docker-compose.prod.yml`**: the stack closed down
  for a shared host (a Grafana login from `GRAFANA_ADMIN_PASSWORD`, only
  Grafana and the Collector's OTLP ports published, a memory limit on the
  Collector). Both Collector configurations now run `memory_limiter` first in
  every pipeline and answer health checks on 13133.
- **`deploy-agent` takes `DATABASE_URL`**: tasks and push configs in
  PostgreSQL, which is what its Kubernetes manifest's two replicas need (see
  *Fixed*).
- **The book, restructured** (78 pages to 210): every page over about 450
  lines split into a landing page and focused sub-pages at the same paths,
  so links and anchors from outside the book keep working (the longest
  hand-written page was `upgrading.md` at 2,585 lines; now 436, apart from
  the gate-checked API quick reference). New pages: a complete observability
  chapter (metrics with every label value and bucket, spans, trace context,
  logs, Prometheus ingest paths, every shipped alert and recording rule, a
  local-stack walkthrough), the tool loop, the `a2a` command-line client,
  conformance testing, containers, health checks, and the example harness, so
  every example has a page. 38 diagrams, rendered from Mermaid sources in
  `book/diagrams/` by `scripts/render_book_diagrams.sh` in a light and a dark
  variant that follow the reader's theme. Contributor material (CI, mutation
  and conformance histories, dogfooding) moved to a *Project & Quality* part.
  Corrected on the way, each against the source: the task state machine
  (it drew only some legal transitions), an invalid transition's error on a
  blocking send (`InvalidParams`, not `InvalidStateTransition`), the
  observability page's alerting section (it named three of eight shipped
  alerts and claimed one on `store_retries` that does not exist), the
  hello-agent and quick-start code, and the WebSocket authentication refusal.
- **A book link gate** (`scripts/check_book_links.py`): pull requests now
  build the book with the deployed mdBook and check every internal link,
  `#anchor` and image, which nothing did (`docs.yml` built it only after
  merge). `benchmarks.yml` runs it before its bot push.

### Breaking Changes

- **A `JwtValidator` checks `iss` and `aud` by default** (server;
  `auth::jwt`; audit SEC-06). One that names no issuer, or no audience,
  refuses every token, and the interceptor logs why when it is built, unless
  `allow_any_issuer()` or `allow_any_audience()` (new) opts out. Before,
  both checks were skipped when unset, so a key set that signs for more than
  one issuer or application let any of its tokens in (another tenant of a
  multi-tenant identity provider; a token minted for another API).
  `from_oidc_issuer` now refuses a discovery document whose `issuer` is not
  the one named (OpenID Connect Discovery 1.0 §4.3; a trailing `/` ignored)
  and binds that issuer to a validator that names none. A JWKS or discovery
  URL must be `https://`, or `http://` to a loopback host; a plaintext one
  is refused (every token, with the reason logged, for `from_jwks_url`).
  `JwtAuthInterceptor::new` is no longer `const`. Tests in `auth/jwt.rs` and
  `tests/auth_jwt_e2e.rs`, each shown failing with its part reverted.
- **The WebSocket listener refuses every browser by default** (server;
  `WebSocketDispatcher`; audit SEC-03). An upgrade that carries an `Origin`
  header is refused with `403` unless `with_allowed_origins` lists that
  origin; one without an `Origin`, as a client that is not a browser sends
  (this SDK's included), is accepted as before. WebSocket is outside CORS,
  so the old default let any page a user visited open a connection with the
  user's ambient credentials and read every answer. `allow_any_origin()`
  restores the old behaviour. `tests/websocket_tls_origin.rs`
  (`by_default_a_browser_origin_is_refused`, shown failing with the old
  default) and `dispatch/websocket/origin.rs`.
- **`ServeReport` is `#[must_use]`** (server): a server that refused to start
  returns at once with `refused` set, and a caller that dropped the report,
  as `hello-agent` did, exited `0` as though it had served (audit G-03).
- **`tls::HttpsClient` is a struct, not a type alias** (client), with the
  `request` method the transports use; it routes through the proxy the
  environment names. **`TlsConfig` is `#[non_exhaustive]`** and has a
  `Custom` variant; `ClientConfig::with_tls`, `ClientBuilder::without_tls`
  and `Server::with_config` are no longer `const fn`.

### Behaviour Changes

- **A tool-loop run whose model provider is briefly away ends failure class
  `transient`, not `internal`** (server; `tool-loop`): HTTP 429, 502, 503
  or 504 from the provider, no answer within the timeout, or no connection.
  Seen live: `llama-server` answers `503` for minutes while it loads a
  model, and every task ended `internal`, which tells a caller not to
  retry. Any other failure stays `internal` (`tests/tool_loop/executor.rs`,
  `tool_loop/loop_error.rs`).
- **Shutdown no longer ends running tasks `canceled`, and no longer admits
  sends it will stop** (server; audit E-F2). Once `finish_in_flight`,
  `cancel_in_flight` or a `shutdown` method begins (so from the end of
  `ServeConfig`'s drain delay on `Server`), a `SendMessage` or
  `SendStreamingMessage` is refused before any work: HTTP `503` with
  `Retry-After`, gRPC `UNAVAILABLE`, a `RetryInfo` detail on every binding.
  Before, it was admitted with its token already cancelled and ended
  `Canceled` a moment later, on every rolling deploy. A task the shutdown
  stops without a terminal state of its own ends `Failed` with failure class
  `transient` and a message saying it may be sent again; its `cancel` hook
  still runs, and a `Canceled` the hook writes is written as that failure.
  Nobody cancelled such a task, and a client does not repeat a cancelled
  one. A halt still ends tasks `Canceled`; an executor that writes its own
  terminal state keeps it. `tests/shutdown_in_flight_tests.rs`,
  `tests/graceful_shutdown_tasks.rs`, `tests/graceful_shutdown_bindings/`,
  and end to end on all four bindings (`a2a-protocol-client`
  `tests/send_refused_while_shutting_down.rs`, `a2a-protocol-sdk`
  `tests/send_refused_while_shutting_down.rs`), each shown failing with its
  part of the change reverted.
- **An error that is both temporary and names a delay is `503` and gRPC
  `UNAVAILABLE`** (server), where the rate-limit rule made it `429` and
  `RESOURCE_EXHAUSTED`. No error before this release was both.
- **The gRPC client reads `UNAVAILABLE` with a `RetryInfo` detail as a
  refusal a send may be repeated on** (client): `UnexpectedStatus { status:
  503, retry_after }`, as the HTTP bindings give it. A bare `UNAVAILABLE`,
  which a proxy or a dropped connection also produces, stays the transport
  error it was.
- **`with_timeout` no longer bounds a blocking `SendMessage`** (client;
  likewise `GrpcTransportConfig`'s `timeout` and
  `WebSocketTransportConfig::with_request_timeout`): a send that waits for
  the turn to end is bounded by `send_timeout` (ten minutes by default)
  instead, so a client that set a short `with_timeout` to fail fast on a
  stalled agent now waits up to ten minutes on such a send. Set
  `ClientBuilder::with_send_timeout` for the old bound (`build_grpc` passes
  it on); a `WebSocketTransport` given to `with_custom_transport` takes its
  own, `WebSocketTransportConfig::with_send_timeout`.
  `tests/scripted_peer_tests.rs` now sets both.
- **A server listening on loopback refuses a request whose `Host` is
  another DNS name** (server; `Server`, so `serve`, `serve_with_addr` and
  `Server::bind`), with `403` and a body naming
  `ServeConfig::with_allowed_hosts`. IP literals, names of one label
  (`localhost`, a Compose service name) and names under `.localhost` are
  answered. This is the DNS-rebinding defence: a web page re-points its own
  name at `127.0.0.1` and the browser then lets it read the answers of an
  agent on the user's machine. Found live against `hello-agent`, whose
  `ListTasks` answered `Host: evil.example` (audit SEC-02;
  `tests/browser_reachable_loopback.rs`). A listener on another address is
  not checked. `ServeConfig::without_host_check()` turns it off.
- **A request body with no `Content-Type` is refused** (server; JSON-RPC,
  HTTP+JSON, the axum router) as `ContentTypeNotSupportedError`, like one in
  a type A2A does not define. A browser sends a cross-origin `fetch` with a
  byte-array body and no type without a CORS preflight, so any web page
  could run `SendMessage`, `CancelTask` or a push-config call on an agent on
  `localhost`; run live against `hello-agent` (audit SEC-01). A request with
  no body (a bodiless cancel) needs no type.
- **`build()` refuses push configs in memory beside a shared task store
  when the card declares push notifications** (server): with a SQL task store
  and no `with_push_config_store`, a config registered on one replica was
  never seen by the replica running the task, and none survived a restart
  (audit E-F4). The error names both remedies.
- **An overloaded server's refusal carries a `RetryInfo` detail of one
  second** (server; `ServerError::Overloaded`: the stream, connection,
  per-tenant and idempotency caps), and HTTP+JSON's `503` a `Retry-After`.
  Over JSON-RPC and WebSocket the answer is HTTP `200`, so without the
  detail a client's retry policy read a full server as a permanent failure
  (audit H1).
- **An empty credential never authenticates** (server;
  `ApiKeyAuthInterceptor`, `BearerTokenAuthInterceptor`, keys added at
  runtime). `ApiKeyAuthInterceptor::new([""])`, an unset variable read as
  the empty string, accepted an empty `x-api-key:` from anyone (audit
  SEC-05).
- **SSE responses carry `X-Accel-Buffering: no` and
  `Cache-Control: no-cache, no-transform`, and no `Transfer-Encoding`**
  (server): the headers nginx and other proxies read to leave a stream
  unbuffered and untransformed (audit B-06). Run live through nginx 1.29 with
  its default proxy configuration, the stream was not held either way (first
  event after 0.01 s, with the header and with nginx told to ignore it), so
  this is for proxies configured to buffer or compress, not a fix of an
  observed stall. `Transfer-Encoding` is hyper's to set, and illegal in
  HTTP/2.
- **`deploy/observability` publishes Grafana on host port 3001**, not 3000,
  where `hello-agent` and a self-hosted Langfuse listen: with the stack up,
  the quick start's agent exited at once with `AddrInUse` (audit A20, run
  live).
- **The repository's `.dockerignore` keeps `.env` files, keys, `.git` and the
  built book out of the build context** (audit A12): `COPY . .` put them in
  every build stage and its cache, and any commit invalidated the source
  layers.
- **This workspace's release builds unwind on a panic** (`[profile.release]`
  no longer sets `panic = "abort"`): the handler catches an executor's panic
  and marks the task `Failed`, which an abort skipped, killing the process and
  every stream it served, in every binary built here, the `deploy-agent`
  image included (audit A4). An application's own crate keeps its own
  profile.
- **The client's pooled connections are dropped after 50 s idle**, not 90 s
  (client): below the 60 s of common load balancers and the 75 s of this
  crate's server, so a connection the peer has closed is not reused for a
  send (audit B-07).

- **A PostgreSQL statement timeout or cancel (`57014`) and an idle-session
  timeout (`57P05`) are now temporary failures** (server): HTTP `503`, gRPC
  `UNAVAILABLE`, where they were internal errors (`500`, `INTERNAL`). Both
  end the statement without applying it, so the same request may succeed
  again.
- **The PostgreSQL task stores' own pools no longer ping a connection before
  every statement** (server; `PostgresTaskStore::new`/`with_migrations`,
  `TenantAwarePostgresTaskStore::new`). They ping one idle for a second or
  more; a statement that finds its connection gone (`57P01`, `57P02`,
  `57P05`, class `08`, or the socket) runs once more on a connection pinged
  first, if a second run leaves the database as one would, and every other
  statement is pinged first as before. A pool passed to `from_pool` keeps its
  own settings. `a2a_claim_turn` answers `'claimed'` to a call whose own
  claim already carries its token, which is what such a second run is.
- **A send to a task another replica is running now waits, then is refused**
  (server, with the PostgreSQL or SQLite stores). It used to be admitted, and
  both replicas ran the task. Each turn now costs two more statements against
  the store (the claim and its release), and a handler holding claims renews
  them once a second; `without_turn_claims()` opts out for a store one replica
  uses. Measured with `tests/soak_multi_replica.rs` (two replicas, PostgreSQL
  17 on the same 4-vCPU host, release build, three 30 s runs each way,
  through a proxy adding latency): the claim costs one or two round
  trips, 12% fewer send-and-read pairs per second at a 1.0 ms
  round trip (390 ± 13 against 443 ± 13), 8% at 2.8 ms, 8% at 11.2 ms
  (book: Running More Than One Replica, *What it costs*, with partitions and
  a database restart mid-turn).
- **On PostgreSQL, a 0.15 replica's write to a task a newer replica is
  running is refused** (SQLSTATE `A2A01`), and a newer replica waits for a
  task a 0.15 replica is running, so a fleet can be replaced one replica at a
  time with no session affinity (*Added*, the upgrade guard). Its client gets
  an internal error for that send, where before the two replicas both ran
  the task. SQL of your own that writes the task tables is treated as an old
  replica's unless it runs `SET LOCAL a2a.claims = 'on'` first.
- **Log lines on standard error are coloured only on a terminal**, and never
  with `NO_COLOR` set (`Telemetry::install_subscriber`). Every line in a file,
  a pipe, or a Docker or Kubernetes log carried ANSI escape codes.
- **Every binary in this repository refuses an `OTEL_*` value it cannot use
  with one line and exit status 2**, instead of a panic with a backtrace and
  `SIGABRT` (status 134) in the release profile.

### Fixed

- **The PostgreSQL halt store broke behind a transaction-mode pooler**
  (server; `PostgresHaltStore`). Its `LISTEN` ran on a lent server
  connection and stayed there when the pooler lent it again, so a later
  notification arrived mid-statement and sqlx failed it (`unexpected
  message: NotificationResponse`); through PgBouncer 1.26.1 the halt itself
  could not be written. New constructors: `from_pool_listening_at(pool,
  url)` listens on a connection of its own straight to PostgreSQL, and
  `from_pool_without_listener(pool)` listens to nothing (`signals` is
  `None`; handlers learn of other replicas' halts at their next poll).
  `from_pool` is unchanged, for a pool that reaches PostgreSQL directly.
  Tested directly and through PgBouncer (`store/halt/tests.rs`; the new
  constructors pass through it, `from_pool` fails as described).
- **The PostgreSQL stores could not use TLS** (server, `postgres`): sqlx was
  built with no TLS backend, so `sslmode=require` or `verify-full`, which
  hosted PostgreSQL documents, refused to start ("SQLx was built without TLS
  support enabled"), and the default `prefer` connected in plaintext. Run
  live against a PostgreSQL 17 with `ssl=on`: `require`, `verify-ca`,
  `verify-full` and `prefer` now connect over TLS 1.3, `disable` does not,
  and a host the certificate does not name is refused
  (`tests/postgres_tls.rs`, which needs no database).
- **`Telemetry` exported every DEBUG and TRACE event of every crate as a
  span event** (server): the span layer had no level filter, so h2's,
  hyper's and sqlx's events inside an exported span went to the collector,
  and `RUST_LOG`, which filters only what is printed, could not stop them.
  Spans and their events are now exported at `INFO` and above
  (`with_span_level`), and each exporting layer tells `tracing` its most
  verbose level, so the callsites below it are disabled process-wide
  (audit C-01; `tests/telemetry_export/span_level.rs`, which fails with the
  filter removed).
- **Each request wrote two or three `INFO` lines** (server and client:
  "dispatching REST request", "handling get task", "sending JSON-RPC
  request", …), and at `INFO` each was also an OTLP log record (audit C-11,
  seen live). They are `DEBUG` now; startup and shutdown lines stay `INFO`.
- **An application's own `INFO` lines were not printed by default**
  (`Telemetry::install_subscriber`): with `RUST_LOG` unset only warnings and
  the telemetry line showed, so `deploy-agent` started without saying where
  it listened or which database it opened (seen live). The crate the default
  service name names (`deploy-agent` as `deploy_agent`) is printed at `INFO`,
  that crate only.
- **`/ready` waited as long as the database's pool did** (server): with
  PostgreSQL paused, the probe waited out sqlx's 30 s acquire timeout (run
  live), so a balancer with a longer probe timeout kept routing to the
  replica. It answers `503 timeout` after `handler::READINESS_TIMEOUT` (2 s).
- **A failed SQLite migration left its exclusive transaction open** on the
  pooled connection (server; `store::migration`), holding the file against
  every other writer: every `?` after the raw `BEGIN EXCLUSIVE` returned
  with it open. It is a `Transaction` now, rolled back on error and on drop
  (audit D-14; a test with a migration that fails).
- **`scripts/preflight.sh` refused to run** ("gate drift detected"): its
  default tier named `cargo test --workspace --all-features`, which
  `ci.yml` replaced with nextest and a doctest step. It names those now.
- **`/ready` on the HTTP+JSON dispatcher, and so on `CombinedDispatcher`,
  was a constant `200`** (server), the same answer as `/health`, though the
  README and the book said it probes the task store; only the axum router's
  did. A replica whose database was gone stayed in its load balancer's
  rotation. It answers `RequestHandler::readiness()` now, with the axum
  router's body (`tests/rest_readiness.rs`, which fails with the constant
  restored).
- **`deploy-agent`, "the smallest agent you can actually ship", served
  through `axum::serve`** (example): no header-read or idle timeout, so a
  client sending a byte a minute held a connection forever, an unbounded
  drain, and a bind on IPv4 only (audit A6, A7, B-16). It runs on the SDK's
  `Server` and `CombinedDispatcher` now (both HTTP bindings, `/health` and
  `/ready`, `DRAIN_DELAY_SECONDS`, `[::]` with an IPv4 fallback), validates
  `PORT` and `AGENT_URL`, logs through `tracing` which database it opens
  (without the password), and fails with one line naming what could not
  start. Its manifest gained a startup probe (the port opens only once the
  database answers, and liveness killed a slow first start before its error
  was logged: audit A3), `maxSurge: 1` / `maxUnavailable: 0`, a
  PodDisruptionBudget, node and zone spreading, `seccompProfile:
  RuntimeDefault` and no service-account token (audit A8, A9), the drain
  delay, and a 60 s grace period. `/healthz` and `/readyz` are gone; the
  probes use `/health` and `/ready`.
- **Readiness scanned every task** (server, `/ready`, `task_store_health`):
  it ran `SELECT COUNT(*)`, which slowed as the table grew until every
  replica's probe timed out at once (audit A2, E-F5). It runs
  `TaskStore::health()` now.
- **A client's connect, DNS and TLS failures said only
  `client error (Connect)`** (client): every HTTP call now reports the
  error's whole cause chain (`invalid peer certificate: UnknownIssuer`,
  `Connection refused`), seen live against a private-CA agent (audit B-05).
- **Values `build()` and `Server` accepted, then panicked or hung on**
  (server; audit G-02): `with_event_queue_capacity(0)` panicked on the first
  event (`build()` refuses it; a per-tenant capacity is clamped to 1),
  `with_max_connections(usize::MAX)` panicked in `Semaphore::new` (clamped),
  and `with_max_connections(0)` served nothing, forever (refused, in
  `ServeReport::refused`).
- **`ClientBuilder::without_tls()` did nothing** (client; audit B-17): it
  now refuses an `https://` endpoint when the client is built.

- **A turn's claim fenced writes to any task, not only its own** (server;
  SQLite and PostgreSQL stores). A write carrying a turn's token passed the
  fence while that token held a claim on any task, so a write to another
  task made inside a turn would have been fenced by the wrong claim. No
  handler path makes one (every request opens its own fence scope); the
  fence now names the task written (`c.task_id = …`), and a refused write is
  told apart from a terminal-state refusal by the same test. A test writes
  to two tasks under one claim on SQLite and PostgreSQL: the claimed task
  lands, the other is refused as `ClaimLost`, and its log is not appended
  to; with the fence unscoped, both stores let the second write through.

- **A PostgreSQL database with a short `statement_timeout` stopped replicas
  starting** (server). Creating the schema, and each migration, ran under the
  database's timeout, so at 100 ms the first DDL statement failed (`57014`)
  and the store could not be built; a replica panicked at startup in a live
  run. The schema transaction now lifts the timeout for itself
  (`SET LOCAL statement_timeout = 0`, `store::pg_schema::lock`). A test
  starts the store, and runs every migration, while another session holds
  the schema lock for longer than a 100 ms timeout, as a second replica
  starting at the same moment does; it fails with `57014` without the fix.
  `lock_timeout` is untouched.

- **A turn whose executor emitted more than its queue held during a store
  outage failed, and its task was left unfinished** (server). While the
  event consumer retried one write (`store_write_retry`, 30 s), the queue
  filled, and the executor's next write gave up after a fixed five seconds;
  the turn failed, and its `Failed` status, meeting the same full queue, was
  dropped, so the task stayed `Submitted` with part of its output, for good.
  Reproduced with an in-memory store unavailable for ten seconds and an
  executor appending 1,100 parts to a queue of capacity 1: the task ended
  `Submitted` with 1,025 parts. The queue's wait is now at least
  `store_write_retry`; the same run completes with all 1,100. A blocking
  send was not affected.

- **A turn's event was lost when one write of it to the task store failed**
  (server): the event consumer logged the failure and went on, and the
  event was in neither the log nor the task. Under a 200 ms
  `statement_timeout`, another session holding the task row for 2 s lost
  three artifact parts from the task and four positions from the log (3
  live runs of 3); now each write is made again while the store says "not
  now", for up to `store_write_retry`, and none is lost (3 of 3).
  `tests/transient_store_errors` holds the row the same way, and ends
  every backend mid-turn.
- **A send made 45 round trips to PostgreSQL**, counting its read back, and
  on a slow link those dominated. A new task's send and read now make 8
  statements, a continuation's 11, each followed by the ping sqlx makes as
  the connection returns to the pool and none preceded by one unless the
  connection sat idle a second (*Behaviour Changes*): a context the send
  minted is not looked up; a send naming a task reads that task, not the
  context's newest (a count and a page); a new task's event numbering starts
  at 0 without reading the log; the turn reuses the row it read under its
  claim; and a new task, or a continuation read under its claim, is saved
  whole, so neither the save nor the event consumer reads the row back.
  Without a claim a continuation still appends its message to the stored
  history, which another replica's turn may have written since. Measured
  with `tests/soak_multi_replica.rs` (as above, claims on): 334 ± 31 to
  532 ± 11 send-and-read pairs per second direct, 67 ± 1 to 166 ± 1 at a
  2.6–2.8 ms round trip, 21 ± 1 to 54 ± 0 at 11.1–11.2 ms.
- **`examples/deploy-agent/deployment.yaml` ran two replicas on in-memory
  stores**: through a round-robin balancer, every second `GetTask` answered
  `TaskNotFound` (measured with two processes behind HAProxy). The manifest
  now takes `DATABASE_URL` from a Secret, not optional, and the process warns
  at startup when it runs without one. The README's claim that two replicas
  were safe because the agent is stateless was wrong and is gone.
- **Four model-backed examples could not finish against a current reasoning
  model**: `genai-agent`, `rig-agent`, `mcp-agent` and `incident-response`
  sent no output cap and left the model's reasoning on, and Qwen3.5-2B on four
  CPU cores generated over 5,000 tokens for one answer, past every client's
  timeout, while llama-server went on generating for callers that had gone.
  Each now caps output at 1,024 tokens and asks a local server not to reason
  (`<PREFIX>_MAX_TOKENS`, `<PREFIX>_THINKING=1`), as `tool-loop-agent` and
  `live-model` already did.
- **`task_id` and `method` were logged with their `Debug` form**, quoted, in
  `GetTask`, `CancelTask`, `SubscribeToTask`, the JSON-RPC dispatcher and the
  client's delegation: in Loki the attribute read `"\"…\""`, so a query for
  the id matched nothing.
- **A readiness probe was a trace of its own** (server,
  `RequestHandler::task_store_health`): its store read opened a database span
  with no request around it, so a probe every 5 s per replica (the shipped
  manifest's) filled the trace backend with `count <database>` traces. It now
  records no span; the store's duration metric is still recorded.
- **Dashboards** (`deploy/observability`): the ratio panels drew a
  0–10,000% axis while every call succeeded, and a flat duration drew seven
  identical ticks; ratios now start at 0 with a 10% soft maximum and
  durations at 0. A call with no method (an unknown route, a request that is
  not JSON-RPC) showed as a series named `Value` in the per-method panels and
  is now `(unrouted)`. Found by looking at the panels after
  `check_dashboards.py` had passed them.
- **Export warnings named the wrong fix, or none, for most TLS and proxy
  failures** (`Telemetry`). Measured against a real Collector with TLS and a
  real Squid: the hints for a certificate issued for another host and an
  expired certificate never fired (they matched rustls's error names, which
  its messages do not contain); a self-signed certificate marked CA, a
  collector that wants a client certificate, one that does not trust the one
  given, an `https://` endpoint on a plaintext receiver (and the reverse), a
  proxy that refuses a tunnel and a proxy that wants a login all had no hint;
  and a 403 from a proxy was blamed on the collector's credentials. Each now
  says what it was and what to change, tested on the failures' own text.
- **`NO_PROXY=[::1]` exempted nothing**: a bracketed IPv6 address, the form a
  URL writes it in, is now read as the bare one (`::1` and ranges such as
  `fd00::/8` already worked).
- **`PgMigrationRunner` cut every migration at each `;`**, including one
  inside a string literal or a function body, which then failed. A
  migration is now sent whole, in the simple protocol, inside its
  transaction.
- **`echo-agent`'s server mode, `tck/sut`, the ITK agent and `mcp-bridge` did
  not flush telemetry on `SIGTERM`**: each ran until killed. They now return
  on `shutdown_signal()`, which drops their `Telemetry`.

### Internal

- OSV GHSA-qwgh-2vcv-g2f7 (`block-buffer` < 0.12.1, a caught panic leaves
  the cursor stored in the block invalid) is waived until 2027-01-29 in the
  `osv-scanner.toml` beside the four lockfiles that lock 0.10.4 (root,
  binding, `fuzz/`, `benches/sdk-comparison/harness/agent-a2a-rust/`). No
  0.10.x release has a fix, and 0.10.4 keeps its cursor in a separate field
  the callback never sees. The advisory's PoC ported to 0.10.4 runs clean
  under Miri; on 0.11.0 it trips the crate's invariant check. osv-scanner
  2.7.0 with `osv.yml`'s lockfile arguments: four findings before, "No
  issues found" after.
- `scripts/check_pg_retry.py`, a CI step: every statement on a PostgreSQL
  task store's pool runs inside `pg_retry::retry` or `checked`, since those
  pools no longer ping before each statement; a PostgreSQL module on a pool
  of its own is listed with the reason. It found the migration runner's
  version read and `BEGIN` outside both, now inside `retry`.
- `otel_pipeline_default_endpoint` asserted that the final flush failed
  against the default endpoint, so it failed wherever a collector listened on
  `localhost:4317`; it now runs in a child whose `OTEL_EXPORTER_OTLP_ENDPOINT`
  names a stand-in collector, and asserts the export arrives there.
  `swarm_scale` kept its turn shape in two statics shared by tests running at
  once, so one test's 1,024-event turns overran another's replay; the shape
  now belongs to each deployment.
- **CI runs the test matrix under cargo-nextest** (`--profile ci`: each test
  in its own process, a hung test killed and named at two minutes, no
  retries, a JUnit report uploaded), with the doctests, which nextest does not
  run, in their own steps under default, all and no default features. The
  nextest version is read from the mutation gate's install script. The whole
  workspace with every feature passed under it locally: 5,851 tests.
- **The `deploy-agent` image caches its dependencies** (cargo-chef 0.1.78):
  after a change to the source only, the dependency layer was reused and the
  build took 221 s instead of 428 s (measured in a sandbox build; the rest is
  the workspace's own crates under fat LTO).
- **The multi-replica soak's rate-limit check holds at any request rate.**
  The sweep runs every 1,000th count, so through a slow link (11 ms per round
  trip) more windows passed between sweeps than the fixed 32-row ceiling
  allowed, and every such run failed though the sweep ran. Below the rate the
  ceiling assumes, the run must now show a sweep and stay within what one
  sweep interval allows at the measured rate; with the sweep removed, it still
  fails.
- **A turn-claim test raced the previous turn's claim release** and failed
  when the release had not run yet; it retries as a replica does.

## [0.15.1] - 2026-10-26

Telemetry export works in more deployments and says what it is doing: proxies,
gzip, a console exporter and a Prometheus endpoint; a warning when an export
is lost, with the transport's cause and, where the failure names the fix, a
hint; and `shutdown_signal()`, so a pod stopped by `SIGTERM` flushes. A diff of
the public items against `v0.15.0` finds only additions: `shutdown_signal`,
`OtlpCompression`, five `TelemetryBuilder` methods, three `Telemetry` methods
and the off-by-default `otel-prometheus` feature. Two behaviours change, under
*Behaviour Changes*; the proxy one affects any pod that sets `HTTP_PROXY`.

### Added

- **OTLP export works from more setups, and says what it is doing.** Each item
  was found by running the repository's `hello-agent` against a stand-in
  collector (2026-10-26) and was silent or refused before.
  - **Proxies**: `HTTP_PROXY`, `HTTPS_PROXY`, `ALL_PROXY` and `NO_PROXY`, in
    either case, as curl reads them. An `https://` collector is reached
    through a `CONNECT` tunnel with TLS inside it (its certificate is still
    checked); a plain `http://` one over OTLP/HTTP as a forward-proxy request
    with `Proxy-Authorization` from the proxy URL's login; OTLP/gRPC is
    tunnelled for both. Only `http://` proxies are spoken: any other kind is
    ignored with one warning and the export goes direct. New
    `TelemetryBuilder::with_proxy` (does not consult `NO_PROXY`) and
    `without_proxy`. Before, a collector only the proxy could resolve received
    nothing and the process printed nothing.
  - **Compression**: `OTEL_EXPORTER_OTLP_COMPRESSION=gzip` (and per signal) on
    both transports, `none` accepted, `zstd` and anything else an error naming
    the variable. The first traces export of `hello-agent` went from 6,031 to
    1,607 bytes and the first metrics export from 2,484 to 1,080 (body bytes
    on the wire, one run each, 2026-10-26). New `TelemetryBuilder::with_compression`
    and `OtlpCompression`. Before, the value was ignored: `opentelemetry-otlp`
    warns when it was not built for a compression, and `Telemetry` had no
    subscriber when it built the exporters.
  - **A console exporter**: `OTEL_TRACES_EXPORTER`, `OTEL_METRICS_EXPORTER` and
    `OTEL_LOGS_EXPORTER` take a comma list of `otlp`, `console` (and the
    deprecated `logging`) and `none`; `console` was refused. One line per span,
    metric data point and log record on standard output, no collector and, for a
    console-only process, no export thread.
  - **A Prometheus endpoint**: with the new, off-by-default `otel-prometheus`
    feature (server and SDK crates), `OTEL_METRICS_EXPORTER=prometheus` serves
    `/metrics` on `OTEL_EXPORTER_PROMETHEUS_HOST` (`localhost`) and `_PORT`
    (9464) for a Prometheus that scrapes, under the names the book's Prometheus
    table gives. Plain HTTP, unauthenticated. Adds `opentelemetry-prometheus` and
    `prometheus` to the lockfile. `Telemetry::prometheus_addr` reports where.
  - **Resource attributes** `host.name`, `host.arch`, `os.type`, `process.pid`
    and `process.executable.name`, each only when `OTEL_RESOURCE_ATTRIBUTES` does
    not name it; `TelemetryBuilder::without_resource_detection` omits them.
    `host.name` is `HOSTNAME`, `COMPUTERNAME` or the kernel's hostname file
    (this crate forbids `unsafe`, which `gethostname(2)` needs), so a macOS
    process that does not export `HOSTNAME` has none.
  - **`OTEL_PROPAGATORS`**: `tracecontext`, `baggage` and `none` are honoured; a
    name this SDK does not have (`b3`, `jaeger`, …) is reported by
    `Telemetry::notices` and a warning, not an error.
  - **`Telemetry::describe`** (where each signal goes, with any login in a URL
    removed) and **`Telemetry::notices`**.
  - OTLP/HTTP endpoints with no scheme are refused with what to write; the
    message was "invalid format".
  - **Why an export failed.** `opentelemetry-otlp` 0.33.1 reports every
    OTLP/HTTP transport failure as `HTTP export failed: network error`, so a
    refused connection, an unknown CA, a gRPC-only collector and a missing proxy
    read the same. The loss warning now appends the transport's own error
    (`cause: client error (Connect): invalid peer certificate: UnknownIssuer`)
    and, only where the failure itself names the fix, a `Hint`: an unknown
    issuer (`OTEL_EXPORTER_OTLP_CERTIFICATE`; the exporter trusts Mozilla's
    roots, never the operating system's), a certificate for another name or an
    expired one, a `401` or `403` (the headers), a `404` over OTLP/HTTP (the
    paths), and a failure with no HTTP status to port 4317 over HTTP or 4318
    over gRPC (the other transport's usual port). A bare refused connection
    gets none.
  - **A base endpoint that already ends in `/v1/traces`** (or `/v1/metrics`,
    `/v1/logs`) posts to a doubled path and a `404`; a startup notice now says
    what it would post to and what to give instead.
  - **`OTEL_RESOURCE_ATTRIBUTES` is percent-decoded**, as the specification
    requires for `,` and `=` in a value (`prod%2Cwest` is `prod,west`);
    `opentelemetry_sdk` 0.33.1 kept it literal. An entry that is not valid
    percent-encoding is kept as written and reported at startup.
  - **`TelemetryBuilder::without_startup_summary`** keeps `install_subscriber`'s
    startup line off while warnings and errors still print. The `a2a` CLI uses
    it, so a one-shot command prints nothing extra.
  Book: [Observability](book/src/deployment/observability.md), "Seeing it with
  no collector", "When something is wrong", "Proxies, compression and private
  CAs", "Prometheus: scrape instead of push" and "What the resource says", and a
  [troubleshooting](book/src/deployment/troubleshooting.md) entry.
- **`shutdown_signal()`**, a future that resolves on `SIGTERM` or `SIGINT`
  (Ctrl-C elsewhere than Unix), for `Server::serve_with_shutdown`. A server
  that waits on `tokio::signal::ctrl_c()` alone is ended by the `SIGTERM` a
  container runtime sends, without returning from `main`, so no `Telemetry`
  guard is dropped: measured on `hello-agent`, the collector received no spans
  and no metrics. With it the server drains, `main` returns and the guard
  flushes. The flush after `SIGTERM` took 0.1 s to a collector that answers,
  1.8 s to a port that refuses and 15.0 s to one that accepts and never
  answers (each provider waits up to 5 s; `OTEL_EXPORTER_OTLP_TIMEOUT=2000`
  made that 4.1 s; one run each), so a pod's `terminationGracePeriodSeconds`
  has to include it. Exported from the server crate and the SDK prelude.
- **A Compliance part in the book** (`book/src/compliance/`): a
  [Compliance Overview](book/src/compliance/overview.md) with an obligation to
  page table, the 88-provision control map summarised by framework and the
  protections that are off until switched on; a
  [GDPR](book/src/compliance/gdpr.md) page (where personal data sits, article
  by article what the SDK carries and leaves, erasure and what it does not
  reach); and a
  [Cyber Resilience Act](book/src/compliance/cyber-resilience-act.md) page (what
  the project publishes, Annex I property by property, the advisory missing from
  RustSec). The EU AI Act chapter (retitled "EU AI Act: Is Your Agent
  High-Risk?"), Audit Trail, Human Oversight and Provenance and Signatures move
  into the part; their URLs are unchanged. Every claim is taken from a row of
  `docs/compliance/controls.toml`, and none says the SDK is compliant with
  anything.
- **Book pages for the five examples that had none** (`book/src/examples/`):
  [Resilient Agent](book/src/examples/resilient-agent.md),
  [Tool-Loop Agent](book/src/examples/tool-loop-agent.md),
  [Live Model](book/src/examples/live-model.md),
  [Langfuse Agent](book/src/examples/langfuse-agent.md) and
  [Swarm](book/src/examples/swarm.md). Each is in the table of contents, the
  overview table and the doctest harness; the overview no longer says five
  examples have no page.

### Behaviour Changes

- **`Telemetry::install_subscriber` always installs a subscriber** (except under
  `OTEL_SDK_DISABLED=true`). It installed nothing when no trace or log provider
  was built, "so a process run without a collector keeps no subscriber at all",
  which left such a process showing nothing: not its own warnings, not how to
  turn export on. The subscriber now has a layer that prints warnings and errors
  to standard error (`RUST_LOG` in `target=level` form decides what; default
  `warn`) and logs `Telemetry::describe()` once. It still never replaces a
  global subscriber already set. Code that calls it and then installs its own
  with `.init()` now panics (the slot is taken) where it did not before:
  install yours first, or use `Telemetry::layer()` in it. `tests/telemetry_install_subscriber.rs`
  asserted the old contract and now asserts this one.
- **A lost export is reported.** After the exporter's retries, a `warn` names the
  signal and the destination, at most once every 30 s with a count of the ones
  held back, on a target `Telemetry` never exports. The SDK's own per-batch
  `ExportError` event is left out of the printed output (the warning supersedes
  it). Before, a collector that was not there produced no output in nine seconds.
- **"no live event subscribers; event persisted only" is `debug`, not `warn`.**
  The code's own comment says it is not an error: it is the ordinary path of
  every blocking call. It was three warnings a request, unseen until a
  subscriber printed warnings.
- **Exports carry five more resource attributes** (above); an export to a party
  that should not learn them uses `without_resource_detection`.
- **The proxy variables are read for the export.** `HTTP_PROXY`, `HTTPS_PROXY`,
  `ALL_PROXY` and `NO_PROXY` (upper or lower case; upper wins when both are set,
  unlike curl) were ignored by the exporter and now decide where it connects. A
  pod that sets `HTTP_PROXY` for its other traffic and exports to an in-cluster
  collector now sends the export to the proxy, which cannot reach the collector,
  unless `NO_PROXY` lists it: `.cluster.local` or `.svc.cluster.local` for a
  `*.svc.cluster.local` name (`.svc` alone does not match, and a CIDR matches
  only a destination written as an IP). The startup line says `via proxy`, and
  a lost export names the proxy. `without_proxy()` restores the old behaviour.

### Fixed

- **`hello-agent`, `genai-agent`, `rig-agent`, `mcp-agent`, `multi-lang-team`,
  `incident-response` and the SLIMRPC `slim_node` waited on Ctrl-C alone**, so a
  `SIGTERM` ended them without a flush (and `slim_node` without
  `service.shutdown()`). They wait on `shutdown_signal()` now. Compiled, not
  run: they need a model or a second process.
- **`langfuse-agent` exited 1 when run with no environment**: it built every
  signal to `localhost:4318` and its final `shutdown()?` reported the three
  lost exports. It now exports only where an endpoint is named (the Langfuse
  preset names one for traces). With an unreachable endpoint configured it still
  fails, and says so.
- **`describe()` printed `console every 0 s`** for a metric interval under one
  second; it prints `500 ms` now.
- **Documentation that was wrong or out of date**, each checked against the
  code or by running it: the server and SDK READMEs' default features and the
  feature a `Telemetry` needs; `examples/README.md` and the book's overview on
  which binaries print a startup line and build nothing without an endpoint;
  the Collector example's `otlp` exporter comment (Langfuse takes OTLP/HTTP
  only); the `otel` module docs and the pipeline docs (it exports metrics
  only, gRPC only); the exported description of `a2a.server.push_deliveries`
  (four outcomes); `observability.md`'s claims about warnings, `OTEL_SDK_DISABLED`,
  which `OTEL_*` variables win over code, the self-targets filtered from the
  layer, test coverage, panel descriptions, the alerts and the console sample
  (now one run); the production guide's shutdown snippets (`ctrl_c()` only);
  `README.md`'s "zero cost when disabled" and "each panel checked"; the deploy-agent
  Dockerfile's claim that the agent reads the image's CA certificates (the SDK
  trusts its bundled Mozilla roots only) and its manifest's grace period,
  now 45 s to include the telemetry flush.

### Internal

- **ADR 0028 (Proposed), and the experiment behind it in
  `docs/adr/0028-wasm-spike/`: the server on Cloudflare Workers.** The real
  `RequestHandler` was run inside `workerd` (`wrangler dev --local`) behind a
  runtime seam; 11 of 11 checks pass, the native lib suite is unchanged (1,545
  passed, identical per-test results), and `reproduce.sh` regenerates every
  figure from a clean worktree. **No crate changed**: the server and client do
  not build for `wasm32-unknown-unknown` at this commit, and the types crate
  builds but panics on its first timestamp. The ADR asks the maintainer to
  qualify ADR 0003 before any of it lands.
- **`scripts/check_book_examples.py` fails the build when an example has no
  book page** (`ci.yml`, `static-checks`). Every directory under `examples/`
  with a `Cargo.toml` needs `book/src/examples/<name>.md`, a `SUMMARY.md` entry
  and an overview row, unless `scripts/book_examples_allowlist.txt` names it
  with a reason (`harness`, the shared coverage-matrix library, is the one
  entry). A page left for an example that no longer exists, and an exemption
  whose example has gained a page, fail it too. Its proof in
  `prove_gates_fail.sh` drops an example from `SUMMARY.md`.
- **Lints CI's clippy 1.99.1 reported on this release's own code, fixed.** The
  console exporter's three `export` impls were `async fn`s that never await
  (`unused_async_trait_impl`) and return ready futures now; eight
  `assert!(x.is_empty())` in the new tests are `assert_eq!` (`assert_is_empty`);
  `shutdown_signal` allows `manual_async_fn` off Unix, where it has no handlers to
  register before returning its future; and its doc comment no longer links to
  `Telemetry`, which exists only with `otlp`, so `cargo doc -p
  a2a-protocol-server` builds. A test helper named `Message` shadowed a generated
  type (`a2a-codegen --check`, ADR 0016) and is `MessageText`; so is a second,
  in a test the mutation-testing fixes added. All were found by CI (the
  `manual_async_fn` one on Windows). Each was reproduced, with rustc 1.99.1 or
  the codegen tool, before it was fixed, except that second `Message`, which CI
  reported and which the tool passes once renamed.
- **The coverage job frees the workspace's instrumented build output before it
  builds the SLIMRPC binding** (`coverage.yml`: `rm -rf target/llvm-cov-target`
  once the report is written). On this release's PR the runner reported 33 MB
  free after that build and the job's post step failed with `No space left on
  device`; two earlier attempts died with a `rust-lld` Bus error while linking a
  SLIMRPC test binary, which is inferred, not shown, to be the same cause. CI
  only; no library code changed.
- **Eight gaps the `mutation-test` label's first `cargo-mutants` run found in
  this release's own code, closed.** Five were tests too weak to see a change.
  The resource-detection test asserted 8 of the 16 names in `os_type` and
  `host_arch`, so the `aix` arm, the `netbsd` arm and six others could be
  deleted unseen. The `rfc3339` test had no date after 2100-02-28, before which
  one term of the century correction is zero. The warning that an `https://`
  proxy is not supported, said once, was not tested to be said once.
  `Conn::poll_flush` was not tested to wait for TLS bytes the socket had not
  taken; it is now, against a peer that stops reading. And the `prometheus`
  arm for traces and logs was asserted by two words the fallback message also
  contains. Three were mutants no test could tell from the original.
  `percent_decode` combined hex digits with `high << 4 | low`, where `|` can
  become `^` unseen; it is `(high << 4) + low`, identical for all 256 digit
  pairs. `prometheus` exporter selection went through a helper whose whole body
  was `Ok(true)` with the feature on; it is inline in its match arm.
  `ProxyConnector::poll_ready` is `Poll::Ready(Ok(()))`, the value its mutant
  returns, so it carries `#[mutants::skip]` as ADR 0006 asks. No behaviour
  changed.
- **Six more survivors from the second `cargo-mutants` run, closed.** Four were
  tests too weak to see a change: nothing asserted the "(N more since the last
  report)" suffix of a lost-export warning, present or absent; `Target::proxied`
  had no direct test; nothing tested the unit boundaries of `human` at exactly
  1 µs, 1 ms and 1 s; and the test of the cause slot overwrote what
  `Cause::record` wrote before reading it. Two were mutants no test could tell
  from the original, in the console metric exporter: `shutdown_with_timeout`
  returns `Ok(())`, and `temporality` returns `Temporality::Cumulative`, which
  is the SDK's own default (a test asserts that). Both carry
  `#[mutants::skip]`, and so does `force_flush`, the same shape as the first.
  No behaviour changed.
- **Three survivors from the third `cargo-mutants` run, closed.** All three
  were tests too weak to see a change. Nothing checked that `Conn::poll_shutdown`
  closes the write half of the socket (shard 0): a request and its reply now
  cross a `Conn` to a real socket, to the collector and through a forward
  proxy, and the peer answers only after the client has shut its write half, so
  `poll_write`, `poll_shutdown` and `poll_read` are each needed for the reply
  to arrive. The `Delta` arm of the console's `temporality` text (shard 6) is
  unreachable through the exporter, which only asks for running totals, so a
  test names both values. And an error status with no description was not
  checked to print `status=error` with nothing after it (shard 13). Shard 13's
  runner was shut down by GitHub before it ran a mutant, so its 25 mutants were
  run locally with the CI selection and flags; this was the one that survived.
  Each test fails with its mutant applied by hand. No behaviour changed.
- **One more survivor, from the fourth `cargo-mutants` run, closed.** Deleting
  the `"zstd"` arm of `compression::from_env` sent `zstd` to the arm for unknown
  values, and the test asserted the variable, the value and `gzip`, all of which
  that arm's message also carries. It now asserts the reason, `not supported`.
  The three earlier runs had recorded this mutant as caught, because a
  PostgreSQL test unrelated to compression ran past its time limit while the
  mutant was applied. No behaviour changed.
- **`a_call_the_caller_drops_is_recorded_as_cancelled` accepts a recorded
  duration of half the wait or more**, where it accepted the wait less 10 ms.
  On this release's PR the macOS stable job failed twice running, measuring
  49.8 ms and then 48.0 ms of a 60 ms wait (59.8 ms on 2026-10-25, which the
  10 ms was added for); Linux reads 61 to 62 ms. Why macOS reads short is not
  established. The test still fails for a recorded duration of 0 or of 5 ms
  (both checked by applying them by hand). Test only; no library code changed.
- **`lease_renewals_are_in_the_parents_trace` waits for two recorded renewals**
  (10 s deadline, the exported span names in its failure message) instead of
  sleeping one second. It failed once on Windows in the `v0.15.0` release run
  ("1 renewals"); the same tree passed there 150 of 150 times in a probe, and
  the cause of that failure is not established. No library code changed.
- **The SLIMRPC binding's lockfile moves `crypto-bigint` from 0.7.6 to 0.7.9.**
  crates.io yanked 0.7.0 through 0.7.8 on 2026-10-25, and `cargo-deny`'s yanked
  check then failed the "SLIMRPC binding (out-of-workspace)" job. 0.7.9 is the
  one 0.7 release not yanked. The crate reaches the binding only through
  `agntcy-slim-auth`'s `p256`, and no other lockfile holds it. Lockfile only:
  the binding is unpublished. Cargo also re-resolved `tempfile`'s `getrandom` to
  0.3.4 (its own range is `>=0.3.0, <0.5`; both versions were already locked).
  The binding builds, passes `clippy --all-targets -D warnings`, and its tests
  pass (133, 10 ignored: the SPIFFE suites need SPIRE).

## [0.15.0] - 2026-10-25

### Known Limitations

- **Several replicas sharing a store need session affinity.** Without every
  turn of a task routed to one replica, two replicas can run the same task
  at once (`the_single_writer_refusal_does_not_cross_replicas`), and a
  continuation saved on one can overwrite what the other's turn wrote
  meanwhile: the stored task loses it, though a subscriber still receives
  the event (`tests/stale_write_position`). Compare-and-set task writes and
  cross-replica single-writer admission would close it; they are planned,
  not built. One replica, or several with affinity, is the supported
  topology for 0.15 ([Running More Than One Replica](book/src/deployment/horizontal-scaling.md)).

### Breaking Changes

- **`serve::Server` is no longer `UnwindSafe` or `RefUnwindSafe` with
  `tls-rustls`** (server; on by default in the SDK). The field holding
  `Server::with_tls`'s configuration carries rustls' server configuration,
  which implements neither. Code that sent a `Server` across
  `std::panic::catch_unwind` wraps it in `AssertUnwindSafe`. Found by
  `cargo-semver-checks` 0.51.0 against 0.14.1 (`auto_trait_impl_removed`),
  the one break that run reported which this list did not name.

- **The client refuses an agent's object that a2a.proto does not allow
  (`ClientError::InvalidAgentResponse`).** Spec §5.7: implementations SHOULD
  reject messages missing a REQUIRED field. On every binding (JSON-RPC,
  HTTP+JSON, WebSocket, gRPC) the tasks `get_task`/`cancel_task`/`list_tasks`
  return, what `send_message` returns, every streamed event, and the push
  configs the push-config calls return are checked against
  `a2a_protocol_types::required`: a task with an empty `id`, a status in
  `TASK_STATE_UNSPECIFIED`, a message with no `messageId`/`role`/parts, an
  artifact with no `artifactId` or parts, a status/artifact update with no
  `taskId`/`contextId`, a push config with no `url` or `authentication.scheme`
  now fail the call with the new `ClientError::InvalidAgentResponse(String)`
  naming the call and field (not retryable; converts to `ErrorCode::InvalidAgentResponse`).
  A refused stream event ends the stream. They used to be returned as if valid.
  What integrators see: calls to a non-conformant agent that used to succeed
  now fail; match `InvalidAgentResponse` to tell this from a transport fault.
  `ClientError` is `#[non_exhaustive]`, so adding the variant breaks no match.
  Crates: `a2a-protocol-client`, `a2a-protocol-types`.
  Tests: `crates/a2a-protocol-client/tests/received_validation/` (`json.rs`,
  `grpc.rs`, `cards.rs`).
- **Agent cards a client receives must pass `AgentCard::validate` and
  `validate_security_schemes`.** `resolve_agent_card`, `fetch_card_from_url`,
  `CachingCardResolver` and `get_extended_agent_card` refuse, with
  `InvalidAgentResponse`, a card the server would refuse to publish (no
  description, no skills, empty `provider.url`, a skill with no name, a
  signature with no `signature`, an empty `protocolVersion`, a security
  scheme with an empty REQUIRED string, ...). A refused extended card is not
  adopted (its capabilities and security are not acted on).
  `ClientBuilder::from_card` does not validate the card it is handed.
  Crates: `a2a-protocol-client`. Tests:
  `cards.rs a_discovered_card_that_breaks_a_publishing_rule_is_refused`,
  `an_extended_card_that_breaks_a_publishing_rule_is_refused_and_not_adopted`.
- **The gRPC conversion refuses an absent REQUIRED field.** `TryFrom<pb::…>`
  now returns `ConvertError { reason: "missing required field" }` for
  `Task.id`, `TaskStatus.state` (0), `Message.message_id`/`role` (0),
  `Artifact.artifact_id`, `TaskStatusUpdateEvent`/`TaskArtifactUpdateEvent`
  `task_id`/`context_id`, `AgentCard.name`/`description`/`version`/
  `capabilities` (no longer defaulted to the empty set), `AgentInterface`
  `url`/`protocol_binding`/`protocol_version`, `AgentProvider`, `AgentSkill`
  `id`/`name`/`description`, `AgentCardSignature` fields, `apiKey.name`,
  `httpAuth.scheme`, `openIdConnectUrl` and the authorization-code,
  client-credentials and device-code flows' URLs — the cases where the same
  JSON without the key does not parse. A server answers such a gRPC request
  `INVALID_ARGUMENT` with a `BadRequest` violation (it used to reach the
  handler, which refused most of them with the same code). A client reports a
  response its conversion refuses as `InvalidAgentResponse` (it was
  `Transport`). Crates: `a2a-protocol-types`, `a2a-protocol-client`.
  Tests: `crates/a2a-protocol-types/src/proto/convert/required_tests.rs`,
  `card.rs agent_card_missing_capabilities_is_refused`,
  `tests/proptest_proto_convert.rs unspecified_state_and_role_are_refused`.

- **An authenticating interceptor needs a scheme declared in the public
  card, and an extended-card scheme must also be in the public card**
  (server; spec §13.3). §13.3: agents "MUST authenticate requests using one
  of the schemes declared in the public `AgentCard.securitySchemes`".
  `RequestHandlerBuilder::build` compared the interceptors with the schemes
  of the public and extended cards pooled, and passed when none were
  declared, so a public card declaring nothing built behind any
  authenticator, and a scheme declared only on the extended card passed for
  the public card's. It now refuses, with `InvalidParams` naming §13.3:
  (1) any authenticating interceptor (bundled or custom) behind a public
  card that declares no `securitySchemes`; (2) a scheme the extended card
  declares that the public card does not declare under the same name with
  the same definition; (3) as before, an authenticator whose
  `checks_scheme` answers `Some(false)` to every scheme, now of the public
  card only. A custom authenticator that answers `None` (the trait default)
  is still trusted to check one of the declared schemes: only its author
  knows what it checks, and refusing it would refuse every custom
  authenticator ever written. Without a public card (`without_agent_card`)
  the extended card's schemes are checked as before.
  **Migration:** declare the scheme your interceptor checks in the card you
  pass to `with_agent_card`: for `BearerTokenAuthInterceptor` or
  `JwtAuthInterceptor`, `securitySchemes: {"bearer": {"httpAuthSecurityScheme":
  {"scheme": "Bearer"}}}` (in Rust,
  `.with_security_schemes(HashMap::from([("bearer".into(),
  SecurityScheme::Http(HttpAuthSecurityScheme { scheme: "Bearer".into(),
  description: None, bearer_format: None }))]))`); for
  `ApiKeyAuthInterceptor`, an `apiKeySecurityScheme` naming its header; for
  `MutualTlsAuthInterceptor`, a `mtlsSecurityScheme`; and repeat every
  extended-card scheme in the public card. The ITK agent now declares its
  bearer scheme in every pass (it guards the extended card in every pass),
  and the SLIMRPC binding's tests declare `mtlsSecurityScheme` for
  `SlimPeerAuthInterceptor::required()`. Tests:
  `auth::declared_tests::an_authenticator_behind_a_public_card_declaring_no_scheme_is_refused`,
  `auth::declared_tests::a_scheme_declared_only_on_the_extended_card_is_refused`.

- **The MSRV is 1.94, and the SQL stores use `sqlx` 0.9** (server, SDK,
  SLIMRPC binding; 2026-10-23). `sqlx` 0.9 needs Rust 1.94, and its pool
  types are in the SQL stores' public constructors (`SqliteTaskStore`,
  `PostgresTaskStore` and the rest take a `SqlitePool` / `PgPool`), so an
  integrator building one from its own pool moves to `sqlx` 0.9 with this
  release. `STABILITY.md` §5 now lets a minor release raise the MSRV to
  adopt a direct dependency's major version, which is this case; it was
  folded into 0.15.0 so that the stores' move does not need a second
  breaking release. sqlx 0.9 refuses a statement built at run time unless
  it is vouched for; the seventeen such statements in the library (the
  `ListTasks` filters, retention, caller-key erasure, the audit key lookup,
  the SQLite schema helpers and the several-part artifact append; counted
  with `grep -rn AssertSqlSafe` outside tests) are built from static text,
  `&'static str` names and integer positions only, and each is marked
  `sqlx::AssertSqlSafe` with that reason. `pg_schema::apply` and the SQLite
  `add_applied_seq` helper now take `&'static str`, so a run-time string
  cannot reach them.

- **Every agent card declares the budget extension** (server; ADR 0023).
  `https://a2a-rust.com/extensions/budget/v1` joins the failure taxonomy
  as an extension the builder adds to every card, never as required. A
  card that carries a signature but does not declare it is refused at
  `build()`, as for any extension the builder would have to add to signed
  bytes. Migration: declare it on the card, then sign the result.

- **`TenantLimits::max_stored_tasks` is removed** (server), with
  `TenantLimits::with_max_stored_tasks` and
  `TenantLimitsBuilder::max_stored_tasks`. Deprecated since 0.10.0 and
  0.12.0 as "never enforced": the field sat on `PerTenantConfig`, which a
  store never sees, so nothing read it. Migration: give the tenant its own
  `TaskStoreConfig::max_capacity` with
  `TenantAwareInMemoryTaskStore::with_tenant_override`. The deprecations
  made in 0.15.0 stay until 0.16.

- **`TaskStore::save_appending_history` takes a `HistoryLimit`, not a
  message count** (server; release audit F2). The limit bounds bytes as well
  as messages. A store that overrides the method trims with
  `limit.trim(&mut history)`, and a caller that passed `n` passes
  `HistoryLimit::new(n, usize::MAX)` to keep the old meaning.

- **OpenTelemetry instrumentation is on by default; export is `otlp`**
  (server, client, SDK; decided 2026-10-14, the exporter's place settled
  2026-10-18). The server's `otel` feature is now the instrumentation only
  (`OtelMetrics`, the spans' OpenTelemetry bridge), through the OpenTelemetry
  API (`opentelemetry`, `tracing-opentelemetry`), and is a default feature
  of the server, the client and the SDK: a process that installs a provider
  gets the SDK's spans and metrics with nothing to enable, and one that
  installs none records to nothing. `Telemetry`, the `Langfuse` preset and
  `init_otlp_pipeline` moved to a new `otlp` feature, with the SDK and
  exporter dependencies; it is default in `a2a-protocol-sdk` only.
  Migration: a direct dependency on `a2a-protocol-server` that used
  `Telemetry` asks for `features = ["otlp"]`; on the SDK nothing changes.
- **The metric catalogue follows the OpenTelemetry conventions** (server,
  `otel`; decided 2026-10-15). `a2a.server.latency`, deprecated in 0.14.0, is
  removed: read `a2a.server.request.duration` (Prometheus
  `a2a_server_request_duration_seconds`), the same histogram under the
  conventions' name, or `rpc.server.call.duration`. Units are UCUM: a count
  is brace-annotated (`{request}`, `{connection}`, `{error}`, `{queue}`,
  `{delivery}`, `{response}`). Rendered through `opentelemetry-prometheus`
  0.33.1 the other metric names are unchanged (measured 2026-10-18).
- **`init_otlp_pipeline` and `init_otlp_pipeline_with_endpoint` take
  `service_name: Option<&str>`** (server, `otel`). The environment now wins:
  `OTEL_SERVICE_NAME`, or `service.name` in `OTEL_RESOURCE_ATTRIBUTES`, names
  the service, and the argument only when they do not; `None` leaves it to
  the environment and the SDK's `unknown_service` default. The `&str`
  overwrote the environment, against the OpenTelemetry environment-variable
  specification. Migration: wrap the name in `Some`. Tests:
  `otel::pipeline::tests`.
- **`ConvertError` and `SseParseError` are `#[non_exhaustive]`** (types,
  client; release audit F26). A match on `SseParseError` needs a wildcard
  arm, and a `ConvertError` is built with the now-public
  `ConvertError::new(field, reason)` rather than a struct literal, so a
  variant or field added later is not a breaking change.
- **A push config whose `authentication` carries no credentials is
  refused** (server; spec §4.3.3: a delivery MUST carry the credentials its
  config specifies). `CreateTaskPushNotificationConfig` answers
  `InvalidParams` naming `authentication.credentials` when they are absent,
  empty or whitespace; such a config was stored and every delivery went out
  with no `Authorization`, or with an empty `Bearer `. Leave
  `authentication` out for an unauthenticated webhook. Test:
  `push_config_credentials`.
- **A card carrying a credential in a URL is refused when published**
  (server; spec §14 "The Agent Card SHOULD NOT include sensitive
  credentials", §13.3 an extended card no "unmasked credentials"). `build()`,
  the static, dynamic and hot-reload card handlers and per-caller extended
  cards refuse a card any of whose URLs carries user information
  (`user:pass@host`) or a query parameter named like a secret (`token`,
  `access_token`, `api_key`, `apikey`, `auth`, `client_secret`, `key`,
  `password`, `secret`, `sig`, `signature`; case-insensitive), naming the field. New
  public `agent_card::card_credentials(&card)` lists them. Tests:
  `file_url_ssrf::build_refuses_a_card_carrying_a_credential`,
  `agent_card::lint::tests::a_credential_in_any_card_url_is_found_and_refused`.
- **A card whose OAuth or OpenID Connect URLs are not URLs is refused when
  published** (types/server; a2a.proto: each OAuth flow URL "MUST be in the
  form of a URL", and `oauth2_metadata_url` "TLS is required").
  `validate_security_schemes`, which `build()` and every card handler run,
  now refuses a flow URL, refresh URL or `openIdConnectUrl` that is not
  `scheme://authority...`, and an `oauth2MetadataUrl` that is not `https`.
  The deprecated implicit and password flows' URLs are checked only when
  given. A client applies the same check to a card it fetches. Tests:
  `security::url_tests`.
- **A client built from a card refuses to build when the card marks an
  extension `required` that it does not ask for** (client; a2a.proto
  `AgentExtension.required`: "the client must understand and comply").
  `build()` returns `ExtensionSupportRequired` naming the extension; ask for
  it with `with_extensions`, or call `skip_required_extension_check()` when
  an interceptor sends the `A2A-Extensions` header. A client built for an
  endpoint, not a card, is unaffected.
- **`build()` refuses a card whose declared authentication the
  interceptors do not enforce** (server; spec §3.3.2 "Servers MUST reject
  requests with invalid or missing authentication credentials", §7.4 "MUST
  authenticate every incoming request based on ... its declared
  authentication requirements", §13.3 "MUST authenticate requests using one
  of the schemes declared in the public AgentCard"). Two configurations that
  built now fail with `ServerError::InvalidParams`, naming the fix: a card
  (or a skill) with `securityRequirements` when no interceptor authenticates,
  which served every request without the credential the card demanded; and
  an authenticating interceptor that checks none of the card's declared
  schemes. `RequestHandlerBuilder::authenticated_upstream()` accepts the
  first when a gateway in front of the process authenticates. A requirement
  list containing an empty requirement (`{}`, optional authentication) asks
  for nothing. New provided trait method
  `ServerInterceptor::checks_scheme(&SecurityScheme) -> Option<bool>`
  (default `None`, not checked); the bundled authenticators answer it.
  Tests: `auth::declared_tests`, and `tests/auth_every_method.rs`, which
  calls every method in `Method::ALL` on both HTTP bindings without a
  credential and with a wrong one and requires `401` each time.
- **A replay longer than `subscribe_replay_limit` is read in pages**
  (server; spec §3.5.2 "each stream MUST receive the same events in the
  same order"). The limit now bounds each read of the event log, not the
  replay. A stream that lagged by more than the limit ended with
  `streamLagged`; it now reads the log a page at a time, from where each
  page ended, before it reads the live channel again. The same holds for a
  turn a `SubscribeToTask` stream missed between polls, and for a
  `Last-Event-ID` resume, which had a silent gap: the replay stopped at the
  limit and the stream went on live, past the positions between the
  replay's end and where its receiver attached, with no `streamLagged`
  (`event_log_tests` pinned `[snapshot, 1, 2]` for a log of five); a
  resume whose first read the log had not caught up with within
  `subscribe_replay_catchup` skipped the same way, and now reads on while
  the log moves forward (found by an independent review). A page the log
  cannot supply is still reported as `streamLagged`, after what it did
  supply, once, and the final state of a finished task follows the last
  page. Memory stays bounded by the limit, but a client resuming with
  `Last-Event-ID: 0` now receives the whole retained history on one
  connection, read a page at a time as it is delivered, where it received
  at most one page before.
- **HTTP+JSON responds `application/a2a+json` when the request states no
  preference** (server; spec §11.1 "Content-Type: application/a2a+json SHOULD
  be used for requests and responses"). `RestDispatcher` and the axum router
  answered `application/json` unless `Accept` preferred the A2A type or the
  body was labelled with it. Now `Accept` still decides when it names either
  JSON type, an `application/a2a+json` body is still answered in kind, a
  request asking only for `text/event-stream` gets `application/json`
  (released a2a-go v2.6.0 sends exactly that on its streaming calls and
  decodes an error body only under `application/json`), and everything else,
  including a request with no `Accept`, gets `application/a2a+json`.
  `scripts/go_sdk_interop.sh` passes both directions on every binding. A
  report asking a2a-go to read `application/a2a+json` errors is drafted,
  held, in `docs/upstream/`.
- **A `SubscribeToTask` stream has no idle bound by default** (server;
  spec §3.1.6 "The stream MUST terminate when the task reaches a terminal
  state"). `HandlerLimits::subscribe_max_idle` is `Option<Duration>`,
  `None` by default; it was a `Duration` of five minutes, after which a
  stream to a task still parked in `input-required` ended with no terminal
  state. `with_subscribe_max_idle` sets `Some` and keeps its signature.
  Every binding already ends a stream whose client has gone (SSE keep-alive,
  the WebSocket read loop, gRPC's closed channel, SLIMRPC's session close),
  so the bound is needed only to reclaim subscriptions whose clients are
  still connected. A bound too large to add to the clock (`Duration::MAX`)
  panicked inside the stream's poll; it is now no bound. A store that keeps
  failing while a stream waits is warned about once per run of failures,
  not at every poll (see the upgrading guide).
- **SLIMRPC: the binding is built on `agntcy-slim-rpc` 3.4.1**
  (`a2a-protocol-slimrpc`; audit register R13). Its six SLIM requirements
  move to the line 3.x requires: `agntcy-slim-rpc` 2.3 → 3.4,
  `agntcy-slim-auth` 0.15 → 0.17, `agntcy-slim-datapath` 0.18 → 0.19,
  `agntcy-slim-service` 0.12 → 0.13, `agntcy-slim-config` 0.16 → 0.17,
  `agntcy-slim-session` 0.7 (locked 0.7.10 → 0.7.21). They are public
  dependencies: `SlimRpcServer::from_app`, `SlimRpcTransport::from_app`,
  `SlimRpcMulticast::from_app` and the builders' `with_identity` take SLIM
  types, so a caller passes types from the same SLIM versions. The binding's
  own code compiled unchanged. Over the wire, SLIM 3.x sessions are MLS and
  verify the peer's identity token on the request that sets one up; a peer
  on the 2.x line was not tried against it.
- **`TaskState`'s discriminants are its proto numbers** (types; spec §1.4,
  ADR 0016). `TaskState` is generated from `a2a.proto` now, and
  `TaskState::Completed as i32` is 3, as `TASK_STATE_COMPLETED` is in the
  proto and the prost enum; it was 5, its place in the hand-written
  declaration. The wire form and `TaskState::ALL` are unchanged.
- **`ClientRequest` has a new public field, `query_params`** (client;
  a2a.proto `APIKeySecurityScheme.location`: "Valid values are "query",
  "header", or "cookie""; audit register R67). The client refused an API
  key that goes in the query string, because a request reached the
  transports with headers only. An interceptor now adds query parameters
  to `query_params`, and the JSON-RPC and HTTP+JSON transports append them
  to the request URL, percent-encoded, after any query the route builds
  itself; a retried call sends them on every attempt. `Debug` prints their
  names, never their values. A struct literal that names every field of
  `ClientRequest` no longer compiles: build it with `ClientRequest::new`,
  or add `query_params: Vec::new()`. This field is why the next release
  is 0.15.0 rather than 0.14.2.

  The `Transport` trait gains `send_request_with_query` and
  `send_streaming_request_with_query`, with defaults, so a custom
  transport still compiles. The default sends a call with no query
  parameters as before and refuses one with them, with
  `ClientError::InvalidAuthScheme`, rather than send it without its
  credential. gRPC, WebSocket and the SLIMRPC binding have no query string
  and refuse the same way.

- **A caller is named by how it authenticated** (server; spec §13.1,
  RFC 7519 §4.1.2; audit register R12). The bundled interceptors record
  `bearer:<label>`, `api-key:<label>` and `jwt:<issuer>:<sub>` (the issuer
  percent-encoded) where they recorded the bare label or `sub`. A `sub` is
  unique only within its issuer, so one `sub` from two accepted issuers was
  one caller: one task partition, one rate-limit budget, one approver. The
  new identity is what caller isolation, the rate limiter, the audit actor,
  `ApprovalRequest.requested_by` and `ApprovalGate::with_approvers` see.
  **An approver allowlist of bare names matches nobody**: write it with
  `auth::caller_id`, e.g. `caller_id::bearer("alice")`. Code that compares
  `CallContext::caller_identity()` against a label must compare against the
  qualified form. **Stored partitions keep their old keys until migrated**:
  run `migrate_caller_keys` once on each SQLite or PostgreSQL tenant-aware
  task and push-config store (see Added), or each caller's tasks and
  webhooks are not found under its new identity. A pending approval stored
  before the upgrade names its requester bare; the four-eyes check still
  refuses that requester, under any issuer.
- **A JWT with no `sub` is refused under caller isolation** (server; audit
  register R12). It authenticated and named nobody, and under the default
  isolation was served the tenant's shared records. It is now refused with
  `PermissionDenied` unless callers share tasks
  (`share_tasks_across_callers`).

### Added

- **`a2a.server.executor.duration`: an agent executor's run time is a metric**
  (server, `otel`). The run is the executor's work for one turn of a task,
  which usually outlives the call that started it, so `rpc.server.call.duration`
  did not measure it and only the `invoke_agent` span's duration did. The new
  histogram (seconds, the call buckets' 0.1 ms to 100 s) carries an `outcome`:
  `completed`, `error`, `budget_exhausted` (the executor timeout, or the
  caller's deadline or tokens), `canceled` (the task was canceled or halted,
  or the server was shutting down) or `aborted` (the future ended without
  returning: a panic, or a dropped run, reported from a guard so it is not
  missing). It comes from a new defaulted `Metrics::on_executor_run`, which the
  OpenTelemetry exporters, `Arc<T>` and the forwarder `with_additional_metrics`
  builds all forward; the server dashboard gains an "Agent executor" row
  (p50 and p95 by outcome, runs per second by outcome). Tests:
  `handler/executor_metrics_tests.rs` (one per outcome and the duration; each
  fails with its path removed).

- **Erasure and legal holds are on record in the audit chain** (server,
  `audit`; GDPR Art. 5(2), CRA Annex I Part I (2)(m)). Four record kinds:
  `task.deleted`, written by the audited task store when a task that was
  there is deleted (the id, context and tenant, never content); `erasure`,
  written by the new `AuditLog::record_erasure(chain, by, Erased)` for the
  `purge_expired` and `erase_caller` an operator runs on a concrete store
  (who, the scope, the counts; the erased caller in a new `erasedCaller`
  field that `with_privacy` seals under that caller's own key, so erasing the
  person later removes their name from the record of their erasure); and
  `hold.placed` and `hold.released`, which `place_hold` and `release_hold`
  now write, naming the person (`by`, a new parameter; both methods are new in
  0.15). Tests: `audit/task_store/tests.rs`
  `deleting_a_task_is_a_chain_record_and_deleting_nothing_is_not`,
  `audit/tests/holds.rs` `placing_and_releasing_a_hold_are_chain_records_naming_the_person`,
  `audit/privacy/tests/erasure.rs` `an_operator_erasure_names_the_operator_and_seals_the_caller`;
  each fails with its record not written (or, for the third, the field left
  unsealed).

- **`RequestHandlerBuilder::with_additional_metrics`** (server). `with_metrics`
  replaces the default, which exports to the global OpenTelemetry meter
  provider, so a handler given a counter of its own stopped exporting
  `rpc.server.call.duration` and the `a2a.server.*` instruments, and nothing
  said so. `with_additional_metrics` keeps what the handler reports to (the
  default, or whatever `with_metrics` set) and adds the caller's: each of the
  nine `Metrics` callbacks goes to the existing metrics, then to the new
  ones. `examples/agent-team` (five handlers) and `examples/resilient-agent`
  (two acts) lost their server metrics this way; both now use it, and
  agent-team's `MetricsForward`, which `Arc<T>` made redundant, is gone.
  Tests: `metrics/tests.rs` `a_tee_forwards_every_callback_to_both_in_order`
  (fails with any callback dropped) and `tests/default_metrics.rs`
  `additional_metrics_are_reported_beside_the_default_not_instead_of_it`
  (fails when the method replaces). `scripts/check_otel_metrics_coverage.py`
  now also holds `Arc<T>` and the new forwarder to every callback.

- **`scripts/check_otel_wiring.py`, a CI gate for what a binary exports.** It
  fails a tracked `fn main` that starts no `Telemetry` and is not on its
  exemption list (the TCK runner, an MCP stdio server, the benchmark
  harnesses and the code generator, each with its reason), and a manifest
  that takes `a2a-protocol-client`, `-server` or `-sdk` with
  `default-features = false` and no `otel`. Stale exemptions fail too.
  `--self-test` proves each rule on a fixture repository. Run on the tree
  before the fixes below it named exactly `examples/live-model`, the SLIMRPC
  manifest and `itk/Cargo.toml`.

- **Agent-card requests are traced and measured** (server, `tracing` and
  `otel`; HTTP semantic conventions at `55c98cf`). A request for
  `/.well-known/agent-card.json` on any listener that serves it (JSON-RPC,
  HTTP+JSON, the axum router, WebSocket, gRPC) had no span and no metric. It
  is now an HTTP `SERVER` span (`GET /.well-known/agent-card.json`; the
  conventions' Required, Conditionally Required and Recommended attributes;
  a query recorded only as `REDACTED`; error status for a 5xx and not a 4xx)
  under the handler's `InboundTracePolicy`, and a point in
  `http.server.request.duration` with bounded attributes. `url.scheme` is
  `https` when this server's TLS ended the connection: `serve` now marks
  such requests. Tests: `tests/card_request_telemetry.rs` (every listener,
  each failing with its instrumentation removed);
  `tests/card_span_over_tls.rs` (fails without the mark).

- **Every control-map row cites a check; AIA-6 and AIA-53 no longer stand
  uncited** (docs, CI; EU AI Act Art. 6, Art. 53).
  - **"Is Your Agent High-Risk?"** (book, deployment): Article 6's two
    routes, the 6(3) exception and the rule that profiling stays high-risk,
    the 6(4) duty to document and register, which SDK feature answers which
    high-risk obligation (Art. 12, 14, 15, 50(2)), and what stays the
    provider's. Every article number and quoted condition was checked
    against the Official Journal text (OJ L, 2024/1689). AIA-6 cites it
    through `check_compliance_map.py`'s document claims.
  - **`scripts/check_no_model_artifacts.py`**, a new `static-checks` step:
    no published crate's `cargo package --list` holds a model-format file
    or one over 10 MiB, and no dependency graph, every feature on, holds a
    crate that loads or runs models. AIA-53's "contains no general-purpose
    AI model" cites it. It fails on a `.gguf` planted in a crate, and
    `prove_gates_fail.sh` carries that injection.

- **SLIMRPC: an agent card declares SLIM's authentication**
  (`a2a-protocol-slimrpc`; A2A §13.3, SLIMRPC §8.4; WP-A, Medium).
  `SlimPeerAuthInterceptor::declare_on(&mut card)` adds
  `server::peer::security_scheme()` to `securitySchemes` under
  `SECURITY_SCHEME_NAME` (`slimMls`), and for `required()` a security
  requirement naming it. A2A has no scheme type for SLIM, so it is a
  `mtlsSecurityScheme` whose description names SLIM, the kind whose
  credential the transport proves for both peers. The interceptor now
  answers `checks_scheme`: a mutual-TLS scheme and no other, so
  `RequestHandlerBuilder::build` refuses `required()` behind a card whose
  only scheme is one it cannot check, where it used to trust it for any.
  Tests: `server/peer/tests.rs`
  `declare_on_names_the_scheme_and_requires_it_only_when_required`,
  `it_checks_the_scheme_it_declares_and_no_other`,
  `the_handler_builds_only_behind_a_card_that_declares_slim` (the last two
  fail without `checks_scheme`).

- **A call's `SERVER` span carries its addresses** (server, `tracing`;
  `docs/rpc/rpc-spans.md`). `network.peer.address` and `network.peer.port`
  come from the connection the call arrived on, when the binding knows it.
  `server.address` and `server.port` come from the request's `Host`, as the
  conventions ask; they are left unset on gRPC and for a `Host` that is
  not a valid authority. None of them is on the duration metric. Tests:
  `rpc_span/address.rs`.

- **`http.client.request.duration` for every webhook POST** (server,
  `otel`). Each push-notification attempt, retries included, is measured
  with the HTTP conventions' advisory buckets and the attributes of its
  `POST` span: `http.request.method`, `server.address`, `server.port`,
  `http.response.status_code` when a response came back, and `error.type`
  (the status for a 4xx or 5xx, `timeout`, or `transport`), on the global
  meter provider as it is at the measurement. The URL is not an attribute.
  Test: `tests/webhook_client_metrics.rs`
  `every_webhook_attempt_is_measured_with_the_http_client_attributes`.

- **`examples/live-model`: the SDK end to end against a real model.** A
  check suite over any OpenAI-compatible endpoint (llama.cpp's
  `llama-server`, vLLM, Ollama): the tool loop streamed and
  signature-verified on JSON-RPC, HTTP+JSON, gRPC and WebSocket; a client
  holding another key refusing the signed artifacts; a repeated
  side-effecting call run once under its idempotency key; a one-token
  budget exhausted; delegation under a budget; halt cancelling a working
  run; and an approval quorum of two. Each check reports pass, fail or
  not exercised. The README records the runs: model, quantization,
  llama.cpp build and commands.

- **SLIMRPC: every operation over the fabric, and an equivalence suite
  against JSON-RPC** (`a2a-protocol-slimrpc`; spec §5.1, §12, §12.1).
  `tests/operations.rs` drives ListTasks, CancelTask, SubscribeToTask and
  the four push-config methods through the typed client, reads each error
  path off the wire (status and type-name prefix), and checks that a
  non-empty unsupported `A2A-Version` gets `VersionNotSupportedError`.
  `tests/equivalence.rs` serves one `RequestHandler` over SLIMRPC and
  JSON-RPC at once and checks equal tasks, events and cancelled tasks,
  equal reads of one task and push config, and eight faults decoding to
  the same `ErrorCode`. `docs/upstream/agntcy-slim-rpc-error-details.md`
  drafts an issue for `agntcy/slim` (not filed): `RpcError::details`
  never reaches the caller.

- **Handler-wide conformance tests** (server `tests/handler_conformance/`,
  SDK `tests/binding_equivalence/`): the required-extension refusal on every
  operation through the handler, JSON-RPC and HTTP+JSON;
  `ContentTypeNotSupported`, terminal-task and unknown-task refusals on the
  streaming send; no store read before admission on any operation; another
  caller's task and an unknown one answering identically on the wire; a
  second cancel having the first's effect; nothing sent to a deleted push
  config; progress reaching the stream while the executor runs; Get
  returning the whole push config, which outlives the task's completion; a
  streaming send's task events honouring `historyLength`; the empty
  ListTasks page's wire form; artifact-update id binding; gRPC
  `VersionNotSupported` for a non-empty unsupported version; every
  binding's error carrying a code and a message; and events arriving in
  generation order on every binding. Each was shown to fail with its
  behaviour removed.

- **Every control-map row cites what proves its claim, and CI enforces
  it.** `docs/compliance/controls.toml` had 42 of 88 rows citing no test.
  86 now cite a test or a CI step that proves what the row's `sdk` text
  says the SDK offers; the two whose text says the SDK is not what the
  provision governs (AIA-6, AIA-53) are listed with the reason in
  `UNCITED` in `scripts/check_compliance_map.py`. A row whose claim is what
  a document says (SECURITY.md's channels and timelines, the book's
  chapters, ADR 0015, the licence) cites the map's CI step, and
  `DOC_CLAIMS` quotes what each document must keep saying; the rendered map
  lists every quote. `--check` fails on an uncited row, a stale `UNCITED`
  entry, or a document that stops saying what is quoted, and
  `scripts/prove_gates_fail.sh` proves the new rule. Every test the map
  cites was run on 2026-10-23: 187, all passing, the PostgreSQL ones
  included. New test: server
  `audit::executor_detail_tests::an_executors_own_fields_are_recorded_in_the_tenants_chain`.

- **`a2a_protocol_types::required`**: `check_event`, `check_received_event`,
  `check_task`, `check_response`, `check_push_config`, `check_card`,
  `check_status`, `check_message`, `check_artifact` — the REQUIRED-field rules
  both ends apply. **`ClientError::InvalidAgentResponse`**.

The upgrading guide (`book/src/reference/upgrading.md`, 0.14 → 0.15) has a
section "The client refuses what `a2a.proto` does not allow".

- **`WebSocketDispatcher::with_agent_card_max_age` and
  `GrpcDispatcher::with_agent_card_max_age`** (server; spec §8.6.1). Both
  listeners served the card with `max-age=3600` fixed; the default is
  unchanged. Test:
  `agent_card_on_every_listener::the_websocket_and_grpc_listeners_send_a_configurable_cache_control`.

- **`TelemetryBuilder::with_export_where_configured` and
  `Telemetry::install_subscriber`** (server `otlp`; SDK prelude). The first
  builds each signal only when an endpoint is configured for it —
  `OTEL_EXPORTER_OTLP_ENDPOINT`, the signal's own
  `OTEL_EXPORTER_OTLP_{TRACES,METRICS,LOGS}_ENDPOINT`, or code — for a binary
  that must run with or without a collector; the second installs the
  telemetry's layer as the global `tracing` subscriber when traces or logs
  were built, never replacing one. Every binary in the repository now uses
  both; before, each read only `OTEL_EXPORTER_OTLP_ENDPOINT`, so a
  metrics-only endpoint exported nothing. `Telemetry` is in the SDK's
  prelude, and `hello-agent` and `tool-loop-agent`, which built no telemetry,
  now do. Tests: `export_where_configured_builds_only_the_signals_with_an_endpoint`,
  `installs_once_and_only_when_traces_or_logs_were_built`, and the CLI's
  `the_cli_exports_its_calls_when_a_collector_is_named` (a metrics-only
  endpoint exports metrics alone).

- **A delegation's lease renewals and drop-time cancel are in the parent's
  trace** (client). Both run on spawned tasks, which start with no current
  span and no ambient `CurrentTrace`, so each of their calls began a trace
  of its own. They now run in the span and ambient trace current when the
  delegation started. Tests: `delegation_spans.rs` (two).

- **Spans for a card fetch and for a push notification received**
  (client). Fetching an agent card is the HTTP conventions' `CLIENT` span,
  `GET`, with the method, the URL without userinfo or query, the server, the
  response status and `error.type` on failure; it had none.
  `WebhookReceiver::receive` runs in an `INTERNAL` span,
  `a2a.webhook.receive`, which continues the push sender's `traceparent`
  when the integrator's server recorded no span, and records the outcome,
  the task and a rejection's kind, never a secret; before, a notification
  started no span and its sender's trace ended at the POST. `error.type`
  for `ClientError::InvalidAgentResponse` is now `invalid_response`, on call
  spans and the call metric too, not `_OTHER`. Tests: `card_fetch_span.rs`
  (three), `webhook_receiver_spans.rs` (three),
  `card_span::tests::the_url_keeps_its_origin_and_path_and_drops_userinfo_and_query`,
  `every_error_class_has_its_error_type`.

- **A JSON-RPC client call's span carries `jsonrpc.protocol.version`,
  `rpc.method_original` and `jsonrpc.request.id`** (client), as the
  server's already did: `docs/rpc/json-rpc.md` makes the first two
  Conditionally Required (the protocol's `2.0` is not the default `1.0`, and
  the method as sent, `SendMessage`, differs from `rpc.method`) and the id
  Recommended. Over HTTP and WebSocket; after a transport retry the id is
  the last request's, recorded once. Before, `rpc.method_original` was
  recorded only for a method outside the A2A service. Tests:
  `call_span::jsonrpc_tests` (four), the WebSocket transport's
  `build_rpc_request_notes_its_id_for_the_call_span`, and the SDK's
  `observability_e2e`, which now requires the client span's id to equal the
  id the server span recorded.

- **The tool loop's `GenAI` metrics and the rest of its `chat` span**
  (server `tool-loop` with `otel`). Each model turn records
  `gen_ai.client.inference.duration`, the `gen_ai.client.inference.usage.*`
  token counters and `gen_ai.client.inference.operation.*` token histograms;
  each tool call `gen_ai.execute_tool.duration`; each run
  `gen_ai.invoke_agent.duration`, `.inference_calls` and `.tool_calls` — the
  names `open-telemetry/semantic-conventions-genai` gives them at `9ffc7e0`,
  with its advisory buckets. They go to the global meter provider as it is at
  each measurement, or to `ToolLoop::with_meter`; `ToolLoop::with_agent_name`
  sets `gen_ai.agent.name`. The `chat` span adds `gen_ai.response.id`,
  `gen_ai.response.finish_reasons`, `server.address` and `server.port`, from
  the new `ModelTurn::response_id` and `finish_reason` (read from the
  completion by `OpenAiChatModel`) and the new `ToolModel::server`, whose
  default reports none. A turn or call abandoned unfinished — cancelled, or
  its future dropped, as an executor timeout does — is recorded with
  `error.type` `cancelled`; before, its span ended looking successful.
  Tests: `rpc_span::tool::tests::metrics` (eight, each proven to fail with
  its behaviour removed), `the_response_id_and_finish_reason_are_read`,
  `the_server_is_the_endpoints_host_and_port`, and
  `a_turn_exports_its_chat_span_and_the_genai_metrics` (through `Telemetry`
  to a stand-in OTLP collector).

- **Prometheus rules and Grafana dashboards** (`deploy/observability/`): a
  Prometheus configuration for OTLP push and one for scraping an
  OpenTelemetry Collector (with the Collector's configuration), 13 recording
  rules, 8 alerts with `promtool` unit tests, and three Grafana dashboards
  (server, client and GenAI) with provisioning. Built from the metric and
  label names a live Prometheus 3.15.1 recorded from `tck/sut`, the `a2a`
  CLI and tool-loop agents against a local model, not from the code;
  `scripts/check_dashboards.py` checks every rule and every panel query
  against a running stack, and passed on both paths (README: what was
  measured and the panels left empty). The GenAI dashboard charts the tool
  loop's model calls, tokens, tool calls and agent runs; the server
  dashboard's "Task store" row charts `db.client.operation.duration`.
  Screenshots are in the README and the book's observability page.

- **A database `CLIENT` span and `db.client.operation.duration` for each
  task-store operation** (server; the OpenTelemetry database conventions,
  `open-telemetry/semantic-conventions@55c98cf`). A store that names its
  database through the new defaulted `TaskStore::database`, returning a
  `StoreDatabase`, is wrapped by the handler: each trait operation is one
  span named `{db.operation.name} {target}` with `db.system.name`,
  `db.operation.name` (the method, such as `get` or `append_event`),
  `db.namespace` and `server.address`/`server.port` where known, and on
  failure `error.type` (`unavailable` for a temporary failure, otherwise
  the A2A error code) with error status; the histogram carries the same
  attributes. The SQLite stores answer `sqlite`; the PostgreSQL stores
  `postgresql`, with the database name, host or socket, and a port other
  than 5432 from the pool's options; the encrypted and audited wrappers
  forward. The in-memory stores, and a custom store that does not
  override `database`, record neither. Tests: `store_spans`,
  `store_operation_duration`, and unit tests in `store::traced` and
  `store::database`, each shown to fail with its behaviour removed.

- **A quorum on the approval gate: `ApprovalGate::with_quorum(n)`** (server;
  EU AI Act Art. 14(5), control map row AIA-14-5, now `supported`). The gate
  waits for `n` distinct allowed approvers, told apart by authenticated
  identity, to approve the same pending request and digest before the
  decision reaches the executor. Until then each approval is recorded on the
  task's status message under `QUORUM_METADATA_KEY`
  (`a2a-rust.com/approval-quorum`), the approver is answered with the task
  still `input-required`, and the executor does not run. An approver
  approving twice counts once, the requester is refused as before, a decision
  on another digest is refused and not counted, and a deny from any approver
  reaches the executor at once. `VerifiedApproval` gains `approvers` (every
  approver counted) and `required`. Each decision is its own `approval` audit
  record, which under a quorum also holds `required`, `approvedBy` and
  `reachedExecutor`. The record lives in the task store, so replicas sharing
  one count together; two approvals arriving at two replicas at once can
  overwrite each other's record, leaving the count short (never over) until
  the approver sends again. `build()` refuses a quorum of `0`, one above the
  `with_approvers` list, or one above `1` with tasks isolated by caller. The
  default, `1`, is the gate as it was. The approval extension's wire contract
  (`approval/v1`: request and decision) is unchanged; the record is new,
  additive metadata on the status message.

- **`Server` warns when it serves plaintext beyond loopback** (server, spec
  §7.1: production deployments MUST use encrypted communication). Once, at
  start, when the listener has no TLS and its address is not loopback,
  naming `Server::with_tls` or a terminating proxy; a warning, not a
  refusal, because TLS is often terminated in front. The client has warned
  about credentials over plaintext since 0.12. Found by the ledger audit of
  2026-10-20.

- **Tool calling in the SDK: `tool_loop`** (server, feature `tool-loop`,
  off by default; forwarded as `tool-loop` by the SDK; ADR 0022). Two new
  open traits, `ToolModel` (one completion turn: preamble, conversation and
  tool specifications in, text and tool calls out) and `Tool` (name,
  description, JSON Schema, `read_only`, `call`, given a `ToolInvocation`:
  the arguments and an idempotency key, HMAC-SHA256 under the loop's
  secret of the task and message, the tool and its canonical arguments;
  ADR 0026); `ToolSet`, which refuses a second tool
  of one name with `DuplicateTool`; `ToolLoop`, which runs turns until the
  model answers without a call, at most `max_turns` (default 8), sends an
  unknown tool, arguments that are not a JSON object and
  `ToolError::Failed` back to the model as that call's result, and runs a
  reply that is exactly one JSON call to a known tool (bare, fenced or in
  `<tool_call>` tags) as that call (`with_recover_text_calls`, on by
  default), and answers a call identical to one that succeeded in the run,
  to a tool not declared read-only, with the earlier result instead of
  running it again (`with_idempotency_secret`, `run_scoped`);
  `ToolLoopExecutor`, which emits `Working`, the `answer`
  artifact, the `tool-trace` artifact when a tool ran and `Completed`, or
  `Failed` with failure class `internal` (the provider failed, or
  `ToolError::Fatal`) or `budget-exhausted` (the turn bound), and stops at
  the next turn or call when cancelled; and `OpenAiChatModel`, a
  `ToolModel` for `POST {base}/chat/completions` (OpenAI, llama-server,
  vLLM, Ollama) on the push sender's hyper client; `ring`, in the graph
  already with `tls-rustls`, for the keys. HTTPS needs `tls-rustls` and is refused at construction without it; an
  optional bearer key; a 120 s turn timeout and an 8 MiB response cap, both
  settable; a non-2xx answer is `ModelError::Status` with at most 512 bytes
  of the body; arguments read as a JSON string or an object; extra request
  fields (`chat_template_kwargs`, `temperature`) through `with_extra_body`.
  Each turn is a `chat {model}` span and each call an `execute_tool {name}`
  span, with the `GenAI` conventions' attributes and token usage, inside the
  executor's `invoke_agent` span; neither records prompt, arguments or
  results. New example: `examples/tool-loop-agent`, run against
  llama-server b11751 on Qwen3-1.7B (its README has the results). Tests:
  `tool_loop::run::tests`, `rpc_span::tool::tests`, `tests/tool_loop/`.

- **Every example, the TCK SUT, the ITK agent and the `a2a` CLI start the
  SDK's `Telemetry`** (examples, `tck/sut`, `itk`, `tools/a2a-cli`). Each
  builds it from the standard `OTEL_*` environment as `langfuse-agent` does,
  keeps the guard for the life of the process, and flushes it before every
  `std::process::exit`: with `OTEL_EXPORTER_OTLP_ENDPOINT` naming a
  collector, the process's traces, metrics and logs go there; unset, nothing
  is built or installed, so a plain `cargo run` needs no collector. Not
  `hello-agent`, whose point is that the prelude alone says hello, nor
  `mcp-agent`'s `tool_server`, which runs no A2A code. The CLI now depends on
  `a2a-protocol-server` (for `Telemetry`, the `otlp` feature) as well as the
  client.
- **`rpc.client.call.duration`** (client, `otel`, on by default): the
  semantic conventions' client histogram, the counterpart of the server's
  `rpc.server.call.duration` — unit `s`, the same advisory buckets, with
  `rpc.system.name`, `rpc.method`, `rpc.status_code`, `error.type` on
  failure, and `server.address` and `server.port`
  (`docs/rpc/rpc-metrics.md`). A streaming call is measured until its stream
  is dropped, as its `CLIENT` span is. It records through the global meter
  provider as it stood when the client was built (the one `Telemetry`
  installs), under the meter `a2a.client`. The `CLIENT` span gains
  `rpc.status_code` — the JSON-RPC code, the HTTP status, or the gRPC status
  name, `OK` on a gRPC or SLIMRPC success — and records the error a stream
  ended on, which it did not before. Its `error.type` is now that status
  when the agent answered with one, as the conventions ask ("If a status
  code is returned and it indicates an error, `error.type` SHOULD be set to
  that status code"): a failed HTTP+JSON or gRPC call that recorded the
  JSON-RPC code (`-32001`) records `404` or `NOT_FOUND`; JSON-RPC is
  unchanged.
- **`ClientBuilder::with_credential_preflight(CredentialPreflight)`**
  (client; spec §3.1.11 "The client MUST authenticate the request using one
  of the schemes declared in the public `AgentCard.securitySchemes` and
  `AgentCard.security` fields"). A client built from a card keeps the card's
  security schemes and requirements and checks each call after its
  interceptors run, where the §13.4 plaintext check runs, so streaming calls
  and every transport are covered: at least one requirement must be met,
  each of its schemes by `Authorization` with the declared HTTP scheme (no
  case), `Bearer` for OAuth 2.0 and OpenID Connect, or the header, query
  parameter or cookie an API-key scheme names; a mutual-TLS scheme counts as
  met. `Warn`, the default, sends the call and logs once per client for a
  call with no credential and once for one with an undeclared credential;
  `Refuse` returns `ClientError::InvalidAuthScheme` before anything is sent;
  `Off` does not look. Neither names a credential value. A client built for
  a bare endpoint, or from a card without `securityRequirements`, checks
  nothing; fetching the extended card replaces the requirements, as it does
  the capabilities. Tests: `declared_credentials`,
  `credential_preflight::tests`.

- **JSON-RPC spans carry `jsonrpc.protocol.version`, `jsonrpc.request.id`
  and `rpc.method_original`** (server; `docs/rpc/json-rpc.md`). Over HTTP and
  WebSocket: `jsonrpc.protocol.version` `2.0` (Conditionally Required when
  not `1.0`), the request's id (not for a notification or a `null` id, which
  the conventions say not to capture), and the method as sent
  (`SendMessage`), which differs from the qualified `rpc.method` and so is
  Conditionally Required. The id and method are cut to 128 bytes.
- **Push webhooks carry the trace and have a span** (server). Each webhook
  POST from `HttpPushSender` runs in an HTTP `CLIENT` span named `POST`
  (`docs/http/http-spans.md`): `http.request.method`, `server.address`,
  `server.port`, `url.full` (the origin and `/REDACTED`, since a webhook's
  path or query is often its secret), `http.response.status_code`,
  `http.request.resend_count` on a retry, and `error.type` with `Error`
  status for a failed attempt (the status, `timeout` or `transport`). With
  `otel`, the POST carries that span as `traceparent` and `tracestate`, so a
  receiver that continues the trace hangs off a span the exporter has; it
  sent neither before. Nothing is sent when no span is recorded.
- **The originating principal on delegated calls, by RFC 8693 token
  exchange** (server, client; ADR 0021; OWASP ASI03). `JwtAuthInterceptor`
  reads the `act` chain of a verified token. The caller identity stays its
  `sub`, the principal the work is for, and `CallContext::actors` lists the
  agents acting for it, nearest first (at most `MAX_ACTORS`, 16). A token
  whose chain cannot be read is refused. With `audit`, a call record
  carries the chain in `detail.actors`, which audit privacy seals per
  person, opens on reveal, and erases per person. The client's
  `token_provider::TokenExchange` performs the exchange at the identity
  provider (`ExchangeRequest`: subject and actor tokens, audience,
  resource, scopes), so a delegating agent sends a token naming the
  principal instead of its own credential.
- **Kill-switch conformance** (server, feature `conformance`).
  `conformance::halt::check_store` grades a `HaltStore` against its
  contract (replace and remove per scope, scopes that do not collide,
  another handle reading what was written, pushed writes and cancels when
  the store pushes); `check_handlers` builds two replicas on it and grades
  the halt end to end: running tasks stopped on both, new work refused on
  both, a restarted replica halted, a resume on either lifting it
  everywhere, and a tenant's halt sparing other tenants (judged once the
  other replica has read the halt, so a store that widens a tenant's halt
  to every tenant fails). The in-memory, SQLite and PostgreSQL stores pass. `Report`'s heading now names what it
  graded (`executor conformance:`, `halt store conformance:`, `kill switch
  conformance:`).
- **The server signs what it serves** (server, feature `signing`; ADR
  0027). `RequestHandlerBuilder::with_content_signer` signs every agent
  message and artifact on a stream, a push, and in what `SendMessage`,
  `GetTask`, `ListTasks`, `CancelTask` and `SubscribeToTask` return, over
  exactly what is sent; content already signed is passed on as it is. The
  queue writer now refuses an event whose provenance value is malformed.
  `with_content_keys` verifies each message sent to the agent, refusing one
  whose signature fails with `InvalidParams`; the executor reads the result
  from `RequestContext::content_check` (`ContentCheck`), and
  `require_signed_content` refuses unsigned messages too.
- **The client verifies what it receives** (client, feature `signing`; ADR
  0027). `ClientBuilder::with_content_keys` checks every artifact and agent
  message in streams and in what `send_message`, `get_task`, `list_tasks`
  and `cancel_task` return; one whose signature fails ends the call or the
  stream with the new `ClientError::ContentNotVerified`.
  `require_signed_content` refuses unsigned content too.
- **Delegation keys its sends** (client; ADR 0026). `Delegation::start`,
  and so `start_leased` and `start_budgeted`, gives the child's message a
  fresh idempotency key (64 hex digits from two version-4 UUIDs) when it
  has none and the agent is known to honour keys; a caller's key is kept.
  `Delegation::idempotency_key` returns it, and
  `A2aClient::honours_idempotency` says whether the agent's card advertised
  the extension (or the builder was told it does).
- **A production profile that refuses to start** (server; ADR 0025).
  `Profile::Production` makes `build()` refuse a handler with no
  authenticating interceptor, no halt store, no lease store or no content
  signer (ADR 0027; feature `signing`), naming the call that supplies each. `ServeConfig::production()` makes
  `Server::serve_with_shutdown` return at once, accepting nothing, when it
  would serve plaintext beyond loopback (unless
  `with_tls_terminated_in_front()`) or a binding the agent card does not
  declare; the new `ServeReport::refused` says why. Profiles now combine,
  and the `profile` module no longer needs the `audit` feature. Defaults
  are unchanged.
- **Halts and cancels are pushed to every replica** (server; ADR 0024;
  EU AI Act Art. 14(4)(e); OWASP ASI10). `HaltStore` gains `signals()` and
  `publish_cancel`, provided methods whose defaults keep a store polling.
  `PostgresHaltStore` notifies `a2a_halts` in the transaction of each write
  and removal, and `a2a_cancels` for a cancel; one listener per store
  forwards both, and sends `Changed` after a reconnect so nothing missed
  stays missed. `InMemoryHaltStore` broadcasts the same. The handler's
  poller reads the store at once on a signal, and fires the token of a task
  a cancel on another replica names; the poll stays behind the push.
  `push_tests.rs` sets the interval to an hour and still sees the other
  replica stop at once; `postgres_pushes_between_replicas` runs two stores
  on PostgreSQL.
- **A budget travels down the delegation chain** (types, server, client; ADR 0023;
  G5 in `docs/swarm-orchestration.md`; OWASP ASI08). A message carries
  `{"remainingMs": n, "meters": {"tokens": n}}` under `a2a-rust.com/budget`
  (`a2a_protocol_types::budget::Budget`). Every server honours it, with no
  switch. Before any work it refuses a malformed budget, or one with no
  time left, as invalid params. The turn's executor timeout becomes the
  smaller of the server's and the caller's, and a turn the caller's
  deadline stops ends `failed`, classed `budget-exhausted`, saying so.
  `RequestContext::budget()` is the turn's `BudgetHandle`: what is left,
  `charge` for real spend (lock-free, never refused), and `grant`, which
  carves a child's budget out of the parent's, reserving each meter so
  ten children cannot each be given the whole. Each status update carries
  what is left and spent (`BudgetStatus`). `Delegation::start_budgeted`
  sends a grant and, when the child settles, charges the parent what the
  child reported spending on the update that settled it in place of the
  reservation, and keeps the whole reservation on any other ending; a start
  the agent refused returns it. `ToolLoop::run_within` (and `ToolLoopExecutor`)
  charges each reply's reported usage to the `tokens` meter and stops once
  it is spent. A wire contract test pins the shape
  (`tests/golden/uri_contracts/budget.json`), and
  `delegation_budget_tests.rs` runs it two hops deep against real servers.
- **Leases on delegated tasks: a child whose parent goes quiet stops**
  (types, server, client; ADR 0020; G1-B in `docs/swarm-orchestration.md`;
  OWASP ASI08). The extension `https://a2a-rust.com/extensions/lease/v1`
  carries a grant on the message that starts a task
  (`a2a_protocol_types::lease::Lease`, `{"ttlMs": n}`). A `GetTask` that
  activates the extension renews the lease (`A2aClient::renew_lease`). A
  server with `RequestHandlerBuilder::with_lease_store` (`InMemoryLeaseStore`,
  `PostgresLeaseStore`) declares the extension, checks a grant against
  `MIN_LEASE_TTL` (1 s) and `with_max_lease_ttl` (default one hour) before
  any work, and stops a task whose lease lapses, as a halt does. A store it
  cannot read counts as a live parent. `Delegation::start_leased` grants
  the lease and renews it every third of its time to live while the handle
  lives, so a parent that crashes, hangs or detaches leaves a child that
  stops within about one time to live (measured in
  `delegation_lease_tests.rs`).
- **A mutual-TLS client certificate names the caller, on every binding**
  (server; ADR 0018; OWASP ASI07). The HTTP `Server`, the WebSocket
  listener and gRPC carry the leaf certificate a client presented in a
  mutual-TLS handshake to the interceptors. It is
  `CallContext::peer_certificate`, through the `PeerCertificate` request
  extension and the `:peer-certificate` headers-map key. Until now the
  dispatchers never saw it, so mutual TLS admitted a connection without
  naming who made it. `auth::MutualTlsAuthInterceptor` (`tls-rustls` or
  `grpc-tls`) names the caller `mtls:sha256:<fingerprint>`, or
  `mtls:<name>` through `with_names`, and refuses a call without a
  certificate (`401`, `WWW-Authenticate: MutualTLS`; gRPC
  `UNAUTHENTICATED`). No new dependency: `tls-rustls` and `grpc-tls` now
  name `ring`, which rustls already linked.
- **`CombinedDispatcher`: JSON-RPC and HTTP+JSON on one listener**
  (server). Serving both bindings on one port took a hand-written hyper
  accept loop; four examples each carried one, outside `Server`'s timeouts,
  connection limit and graceful shutdown. `CombinedDispatcher` is a
  `Dispatcher` like the others. A `POST` to the JSON-RPC path (default `/`,
  or one segment set with `with_jsonrpc_path`), and its preflight, go to
  JSON-RPC; everything else goes to REST. `echo-agent`, `genai-agent`,
  `rig-agent` and `multi-lang-team` now use it.
- **A server span and `rpc.server.call.duration` on every binding,
  SLIMRPC included** (server, client, `a2a-protocol-slimrpc`; ADR 0013,
  decision 2). `BindingSpan` and `CustomBinding` let a binding outside
  `a2a-protocol-server` open the span and record the metric the in-crate
  bindings do: same name, A2A attributes, trace policy and remote parent,
  with its own `rpc.system.name` and status mapping. `a2a-protocol-slimrpc`
  now records all eleven methods as `slimrpc`, with its gRPC-style status
  codes and `OK`; before, its calls recorded neither. On the client,
  `Transport::rpc_system` (defaulted to `None`) names a custom transport's
  system on its calls' `CLIENT` spans: `jsonrpc` for `WebSocketTransport`,
  `slimrpc` for `SlimRpcTransport`. A custom transport's spans now also keep
  the agent's name, description and version from the card, which they
  dropped before.
- **Mutual TLS to the OTLP collector, and OTLP/HTTP with JSON bodies**
  (server). `TelemetryBuilder::with_client_certificate_pem(chain, key)`, or
  `OTEL_EXPORTER_OTLP_CLIENT_CERTIFICATE` and `OTEL_EXPORTER_OTLP_CLIENT_KEY`
  (and their per-signal forms; the files win over code), present a client
  certificate on gRPC and OTLP/HTTP alike; half a pair, or a file that is not
  PEM, fails `build`. `OtlpProtocol::HttpJson` and
  `OTEL_EXPORTER_OTLP_PROTOCOL=http/json` are a new `otlp-json` feature (server
  and SDK), off by default: `opentelemetry-otlp`'s `http-json` feature makes
  JSON the default of every exporter built without a protocol, which Cargo
  would impose on an application's own. Without the feature `http/json` is
  still refused, now naming the feature. Each is tested end to end against a
  stand-in collector, with a counter-test that the collector refuses a client
  without the certificate.
- **`A2aClient::follow_task(id, poll_every)`** (client; spec §7.6.2: a
  client without an open stream "SHOULD perform one of the following:
  Subscribe to Task ... Register a webhook ... Begin polling the Task").
  Returns a `TaskFollow` whose `next()` yields the task's updates until it
  ends: from `SubscribeToTask` when the agent can stream, and from `GetTask`
  every `poll_every`, reporting each status change once, when its card does
  not declare streaming or it refuses the subscription
  (`UnsupportedOperation`, as for a task already ended). Test:
  `follow_task_e2e`.
- **`erase_caller(identity)` on every tenant-aware store** (server; spec
  §13.4 "Agents SHOULD provide mechanisms for users to request deletion of
  their data"). The in-memory, SQLite and PostgreSQL task stores remove the
  caller's partitions in every tenant with their tasks, logged events and
  idempotency keys; the push-config stores remove its webhooks; the SQL
  stores do it in one transaction. Reports a new
  `store::caller_keys::CallerErasure` (partitions, rows). New
  `RequestHandlerBuilder::with_push_config_store_arc`, to keep a handle to a
  push-config store as `with_task_store_arc` does for a task store. Test:
  `erase_caller`, on all three backends.
- **A card-signing key past its `exp` is not used** (types; spec §8.4.3
  "Expired or revoked keys MUST NOT be used for verification"). `Jwk` gains
  an `exp` member (seconds since the epoch, the JWT `NumericDate`), which
  some key sets publish though RFC 7517 defines none, and
  `Jwk::with_expiry` to publish it; `Jwks::candidates` skips a key at or
  past it. A revoked key is one the set no longer lists, and nothing in the
  SDK caches a set. Test: `signing::keys::tests::an_expired_key_is_not_used`.
- **`auth::CredentialSet`: rotating and revoking static credentials while
  serving** (server; spec §13.4 "Credentials SHOULD be rotated periodically",
  agents "SHOULD implement credential revocation mechanisms").
  `BearerTokenAuthInterceptor::credentials()` and
  `ApiKeyAuthInterceptor::credentials()` return a handle to the set the
  interceptor checks; `add`, `add_labelled` and `revoke` apply from the next
  request on. Until now the lists were fixed at construction. Test:
  `credential_rotation`.
- **`ClientBuilder::require_https_credentials()`** (client; spec §13.4
  "Credentials SHOULD be transmitted only over encrypted connections"). A
  call carrying a credential (`Authorization`, `Proxy-Authorization`,
  `Cookie`, or where an `AuthInterceptor::api_key` puts its key) to an
  `http://` or `ws://` endpoint on a non-loopback host is logged once by
  default, and with this option refused before sending with the new
  `ClientError::InsecureTransport`, which names the header and the origin
  only. Tests: `credentials_plaintext`, `plaintext::tests`.
- **A push config sending credentials to an `http://` webhook is logged**
  (server; spec §13.4). It is stored and delivered as asked, as before;
  `HttpPushSender::https_only()` still refuses it. The warning names the
  webhook's origin, never its path, token or credentials. Test:
  `push_plaintext_credentials`.
- **`CallContext::task_id()`** (server; spec §13.1 "Servers MUST implement
  authorization checks on every A2A Protocol Operations request"). The task a
  request names (the `id` of `GetTask`, `CancelTask` and `SubscribeToTask`,
  the `taskId` of a push-config operation or of a message continuing a task)
  is on the context before the interceptor chain runs, so an authorizer can
  decide per task; `None` for a request that names none. New
  `CallContext::with_task_id` for a context built by hand. Test:
  `authorization_per_task`, every method on both HTTP bindings.
- **`A2aError::insufficient_scope(message, scopes)`** (types; spec §3.3.2
  "Servers SHOULD indicate what permission or scope is missing"). A `403` /
  `PERMISSION_DENIED` like `permission_denied`, carrying the RFC 6750 §3
  challenge `Bearer error="insufficient_scope", scope="..."`, which every
  binding sends as it sends a `401`'s (header, gRPC metadata, WebSocket
  `ErrorInfo` metadata). A scope that is not an RFC 6749 `scope-token` is
  left out.
- **`JwtValidator::require_scope(scope)`** (server, `auth-jwt`). A token that
  verifies but does not grant the scope in its `scope` claim (space-separated
  string or array) is refused with `insufficient_scope`, naming only the
  scopes it lacks; a token that fails any other check is still `401`, before
  its scopes are read. Test: `jwt_required_scope`.
- **`webhook::WebhookReceiver`: the receiving side of push notifications**
  (client, feature `webhook`, on by default in the client and the SDK; spec
  §4.3.3, §13.2). Register the config `set_push_config` returned, then hand
  each webhook request's headers and body to `receive()`; the outcome's
  `status()` is the HTTP status to answer. It checks the notification token
  (either header spelling, looked up by SHA-256 and compared in constant
  time), the config's `Authorization` scheme and credentials (401), the body
  as a `StreamResponse` (400) and its task against the registered task
  (403); it answers a byte-identical replay as `Duplicate` with 200 (a
  bounded cache, on by default) and, with `with_rate_limit`, more than the
  config's allowance with 429. `new_notification_token()` makes a 244-bit
  token, and `register` refuses a token another config already uses. The
  feature adds `ring`, which the default `tls-rustls` build already links.
  Tests: `webhook_receiver`, and the SDK's `webhook_receiver_e2e`, in which
  the server's `HttpPushSender` retries after a lost acknowledgement.
- **`ClientBuilder::require_https_webhooks()`** (client; spec §13.2 "Clients
  SHOULD use HTTPS endpoints for webhook URLs"). `set_push_config`, and a
  send carrying an inline push config, warn on an `http://` webhook to a
  non-loopback host; with this option they refuse it before sending, with
  the new `ClientError::InsecureWebhook` (the URL's origin only). Tests:
  `webhook_https_tests`.
- **`url_guard::check_public_url` and `url_guard::resolve_public_url`**
  (server; spec §14 "File references within A2A messages MUST be validated
  to prevent SSRF"). The checks the push sender has applied to webhook URLs,
  now public so an executor that fetches a `Part`'s URL can apply them:
  loopback, private, link-local, CGNAT, ULA and IPv4-mapped addresses,
  numeric IPv4 spellings (`0x7f.1`, `2130706433`) and internal names are
  refused; `resolve_public_url` also resolves the host (bounded by a
  timeout) and refuses it if any address it resolves to is private.
- **`RequestHandlerBuilder::refuse_private_file_urls()`** (server; spec §14).
  Off by default, since the SDK never fetches a `Part`'s URL itself; when
  set, a message whose file part points at a private or internal address is
  refused with `InvalidField` (`InvalidParams` on the wire) naming the
  part, before the executor sees it. Tests:
  `file_url_ssrf`.
- **`agent_card::card_warnings(&card)`, logged by `build()`** (server). The
  problems a card likely has but the specification allows: an interface URL
  that is not absolute, or not `https` off loopback (a2a.proto
  `AgentInterface.url`, spec §7.1); a custom binding that is not a URI
  (§5.8); an HTTP authentication scheme not in the IANA registry (read
  2026-10-18); an extension URI with no version (§4.6.3); a URL naming an
  internal host (a private, link-local or CGNAT address, or a name under
  `.internal`, `.local`, `.lan`, `.corp` or `.intranet`; loopback is a
  local test and is not reported), which spec §13.3 and §14 say a card
  should not disclose.
- **A listener warns when its binding is not on the card** (server; spec
  §5.2 "Agents MUST declare all supported protocols in their AgentCard").
  `Server`, the WebSocket listener and the gRPC listener check the handler's
  static card when they start. New provided method
  `Dispatcher::protocol_binding()` (default `None`); `JsonRpcDispatcher`
  and `RestDispatcher` answer it.
- **`EventEmitter::auth_required(explanation)` and
  `relay_auth_required(&downstream_status)`** (server; spec §7.6.1 "MUST
  transition the TaskState to `TASK_STATE_AUTH_REQUIRED`" and "MUST include
  a TaskStatus message explaining the required authorization"; §7.6.2). The
  first parks the task with the explanation as its status message; the
  second passes a downstream agent's request on as this task's own. A
  streamed task that parks at `AUTH_REQUIRED`, gets its credential out of
  band and finishes stays on one stream (§7.6.1 SHOULD), now pinned by
  `tests/auth_required_in_task.rs`.
- **`RequestContext::referenced_tasks()`** (server; spec §3.4.3 "Agents
  SHOULD use referenced tasks to understand the context and intent of
  follow-up requests"). The handler resolves a message's `referenceTaskIds`
  before the executor runs: in the caller's own partition, unknown ids
  skipped, duplicates once, at most 16. `with_referenced_tasks` sets them
  for a context built by hand. Tests: `tests/referenced_tasks.rs`.
- **An opt-in total task cap across tenant partitions** (server; the
  maintainer's 2026-10-18 decision). The default tenant store's bound was
  `max_tenants × max_capacity` (ten million tasks at the defaults).
  `TenantStoreConfig::with_max_total_tasks(n)` caps the sum: a new task at
  the total first evicts its own partition's oldest finished task, and is
  otherwise refused as temporary (HTTP `503`, gRPC `UNAVAILABLE`); an update
  is never refused and no other tenant's task is evicted for it.
  `TenantAwareInMemoryTaskStore::total_tasks()` reports the count. Tests:
  `store::tenant::store::total_cap_tests`.
- **`wss://` from the WebSocket listener itself** (server, feature
  `tls-rustls`; spec §7.1 "Production deployments MUST use encrypted
  communication", §13.4). `WebSocketDispatcher::with_tls(rustls::ServerConfig)`
  and `with_tls_pem(cert_chain, key)` make the listener TLS only, offering
  ALPN `http/1.1`, with the TLS handshake bound by the handshake timeout. The
  agent card (§8.1) is served over the same session: a TLS stream cannot be
  peeked at, so the start of the first request is read and replayed to the
  card responder or the WebSocket handshake. With `Server::with_tls` and
  `GrpcDispatcher::with_tls`, every listener the SDK starts can now terminate
  TLS; the acceptor and PEM handling `Server` had are shared, not copied.
  Tests: `tests/websocket_tls_origin.rs` (a request over `wss://`, the card
  over the same TLS listener, a plaintext client refused).
- **An opt-in `Origin` allowlist on the WebSocket listener** (server; the
  maintainer's 2026-10-18 decision; WebSocket binding §7, cross-site
  WebSocket hijacking). `WebSocketDispatcher::with_allowed_origins([...])`
  refuses an upgrade whose `Origin` is not listed with `403` and the
  `PERMISSION_DENIED` error body, before the version check;
  `require_origin()` also refuses an upgrade with no `Origin`. Origins are
  compared whole, ignoring ASCII case. Default unchanged: any origin, and
  none, is accepted.
- **Audit records protect who and what they name, and a person can be
  erased** (server, `audit` and `encryption`; GDPR Art. 25(1), 32(1)(a),
  17; ADR 0015 amended 2026-10-16). With `AuditLog::with_privacy`, each
  record's caller and requester are sealed under a key of each person's own
  (AES-256-GCM, a key derived per value), and each content digest becomes
  an HMAC bound to its record, so it confirms no guess without the key; the
  chain still verifies with no key. `AuditLog::reveal` and `digest_matches`
  serve whoever holds the keys. `AuditLog::erase_subject` destroys one
  person's key (refused under a legal hold, recorded first as
  `subject.erased`), reports records from before protection that still name
  them, and leaves the chain verifying; retention drops keys no record
  names. `AuditStore` gains subject-key methods with refusing defaults; the
  SQLite and PostgreSQL stores add the table `a2a_audit_subject_keys`. The
  record schema stays `a2a-audit/1`. An erased person's records remain
  linkable to each other, and copies of a key in backups or logs survive;
  the book's audit page and the ADR list what erasure does not do. An
  append costs about 2.7x on the in-memory store and 1.8x on SQLite
  (measured; the ADR gives the numbers).
- **Encryption at rest for task content** (server, `encryption` feature;
  GDPR Art. 25(1) and 32(1)(a)). `store::encrypted::EncryptedTaskStore`
  wraps any `TaskStore` and seals every part of a task's history, status
  message and artifacts, and of every event in its log, before the wrapped
  store sees it. Each part is sealed whole, under a context naming the
  tenant partition, the task, the record or log position, the message or
  artifact holding it and its index, so a part altered, moved to another
  such place, sealed under an unknown key or stored unsealed fails the read
  (`accept_unsealed_parts` reads unsealed parts, for migrating a store in
  place). `Aes256GcmCipher` is AES-256-GCM (`ring`) over a key ring for
  rotation, counts its seals against the SP 800-38D nonce budget, and is
  also a `PushConfigCipher`; `ContentCipher` takes a cipher of your own.
  Ids, states, timestamps, names, `metadata` and the order of messages and
  artifacts are not protected; the security page lists what is not. Tested
  over the in-memory, SQLite and PostgreSQL stores (the last in CI's live
  database job) and a running handler.
- **A final `Task` snapshot before a stream closes, opt-in** (server; spec
  §11.7 "MAY optionally resend a final `Task` snapshot before closing").
  With `HandlerLimits::with_final_task_snapshot(true)` a send stream and a
  `SubscribeToTask` stream send the finished task once more, after the
  terminal state, on every binding. It is read once the store holds the
  terminal event, waiting at most `subscribe_replay_catchup`, and is not
  sent when the store cannot show the finished task by then. A send
  stream's snapshot carries the history its request asked for.
- **`VersionQueryInterceptor` sends `A2A-Version` as a request parameter**
  (client; spec §3.6.1 "Clients MAY provide the `A2A-Version` as a request
  parameter instead of a header"). Opt-in, through `with_interceptor`: every
  request on the JSON-RPC and HTTP+JSON transports carries
  `?A2A-Version=1.0` beside the header, for a proxy or gateway that drops
  headers it does not know. A parameter the request already names is kept.
- **`CachingCardResolver::with_always_revalidate`** and
  **`CachingCardResolver::with_default_freshness`** (client; spec §8.6.2).
  `with_always_revalidate(true)` revalidates on every resolve however fresh
  the card is, as the resolver did before this release, for an agent whose
  card changes sooner than its `max-age` says. `with_default_freshness`
  gives a card whose response has no `max-age` and no `Expires` a lifetime
  (§8.6.2: clients MAY apply a default cache duration); without it, off by
  default, such a card is revalidated on every resolve. The rustdoc of
  `resolve_agent_card` and `fetch_card_from_url` now says they are one-shot
  and uncached and points to the resolver. The client crate now depends on
  `httpdate` directly, to read `Date` and `Expires`; it was already in the
  graph through `hyper`.
- **SLIMRPC: the caller SLIM verified, and an interceptor that
  authenticates by it** (`a2a-protocol-slimrpc`; spec §13.1, §12.6; audit
  register R13). On a session with MLS — every session the SLIM 3.x client
  opens — the server records the caller's verified token subject and issuer
  under `:slim-peer-subject` and `:slim-peer-issuer`, keys a caller cannot
  set: the server drops a caller's own `:`-prefixed keys, and now SLIM's
  `slim-peer-subject` too. `SlimPeerAuthInterceptor::new()` records the
  caller as `slim:<issuer>:<subject>` (built by `server::peer::caller_id`,
  in the form of `jwt:<issuer>:<subject>`) with the auth scheme `slim`, and lets a
  call with no verified peer through for another interceptor;
  `SlimPeerAuthInterceptor::required()` refuses that call as unauthenticated
  and counts as authenticating for the extended card. Before, the binding
  recorded no caller, and a SLIMRPC caller was identified only by a
  credential in its metadata. Tests: `tests/peer_identity.rs` end to end,
  over a real in-process MLS session, with a caller sending forged keys;
  unit tests in `server::methods` and `server::peer`, among them the rate
  limiter and the throttle keying two verified peers apart.

- **`TaskStore::get_with_position` and `store::AppliedPosition`** (server;
  audit register R69). `get_with_position` returns a task with the event-log
  position the stored record reflects; its default reports none. The
  processors run each event's writes inside `AppliedPosition::scope(seq, …)`,
  and every shipped store records `AppliedPosition::current()` atomically with
  the write, keeping the stored position on a write outside a scope. The SQL
  stores gain an `applied_seq` column: `SQLite` migration 8 and PostgreSQL
  migration 7 for `with_migrations`, and every `from_pool` adds it to a
  database an earlier version created, checking for it first so a start takes
  no lock on a table that has it (the book's *Task & Config Stores* page has
  the contract for a custom store). The `SQLite` migration runner now skips an
  `ADD COLUMN` whose column exists, so `with_migrations` succeeds on a
  database `from_pool` has already opened.

- **Public types generated from `a2a.proto`** (types; spec §1.4: derived
  forms "MUST be regenerated from the proto ... rather than edited
  manually"; audit register R49, ADR 0016). `tools/a2a-codegen`, outside
  the workspace, parses the proto with `protox` (no `protoc`) and writes
  `crates/a2a-protocol-types/src/generated/`, shaped by
  `crates/a2a-protocol-types/codegen.toml`. The generator refuses a
  configuration entry for a type, field or value the proto does not have,
  and a proto type the configuration does not mention. CI's static checks
  regenerate in memory and fail on any difference, and on any hand-written
  type, anywhere in the repository, declared under a generated type's name.
  Over the migration's phases,
  `TaskState`, `MessageRole`, `AgentInterface`, `AgentProvider`,
  `AgentExtension`, then the agent card family (`AgentCard`,
  `AgentCapabilities`, `AgentSkill`, `AgentCardSignature`), then `Task`,
  `TaskStatus`, `Message`, `Artifact`, `TaskStatusUpdateEvent` and
  `TaskArtifactUpdateEvent`, then the 14 request, parameter and response
  types (`MessageSendParams`, `ListTasksParams`, `TaskListResponse`,
  `TaskPushNotificationConfig` and the rest), then the 12 security-scheme
  and OAuth-flow messages (`ApiKeySecurityScheme`, `SecurityRequirement`,
  `StringList` and the rest), then the five `oneof`s (`Part` with
  `PartContent`, `SecurityScheme`, `OAuthFlows`, `StreamResponse`,
  `SendMessageResponse`) are generated, at their existing paths: every
  proto type now is. Docs now come from the
  proto's comments, with the Rust-specific notes kept. Fields follow the
  proto's order, so `AgentProvider` serializes `url` before
  `organization`, `AgentCard` writes `provider` and `version` after
  `supportedInterfaces`, a `Task` writes `artifacts` before `history`, and
  a `Message` writes `contextId` and `taskId` before `role`, and the
  security schemes write `description` first. The one other observable
  change is the member list in `OAuthFlows`' two-flows error, now in the
  proto's order (`deviceCode` last). Card
  signatures are unaffected: they sign the RFC 8785 canonical form, which
  sorts keys.
- **`migrate_caller_keys`** on `TenantAwareSqliteTaskStore`,
  `TenantAwarePostgresTaskStore`, `TenantAwareSqlitePushConfigStore` and
  `TenantAwarePostgresPushConfigStore`, with `store::caller_keys` (server;
  audit register R12). The old key does not record the scheme or issuer, so
  the deployer gives a map from an old caller name to its new identity;
  `caller_keys::bearer_labels`, `api_key_labels` and `jwt_subjects(issuer)`
  cover a deployment that used one authenticator. One transaction per call:
  a task moves with its logged events and idempotency keys, a sealed push
  config is opened under its old partition and sealed again under the new
  one, and a task id the new partition already holds refuses the whole run.
  A second run moves nothing.
- **`serve::Server::with_tls`** and **`Server::with_tls_pem`** (server,
  `tls-rustls`; spec §14: "Implementations SHOULD support HTTPS to ensure
  authenticity and integrity of the Agent Card"). Only the gRPC listener
  terminated TLS; the HTTP listener needed a reverse proxy. The graceful
  `Server` now runs each connection's TLS handshake on its own task, within
  `ServeConfig::header_read_timeout` when that is set, and then serves HTTP/2
  or HTTP/1.1 as ALPN agreed (`h2` and `http/1.1` are offered when the
  configuration names none). A failed handshake closes the connection and
  counts as a failed connection. `with_tls_pem` builds rustls' safe defaults
  on the `ring` provider from a PEM chain and key, and refuses a key that is
  not the certificate's. `serve`, `serve_with_addr` and the WebSocket
  listener stay plaintext.
- **`ExtendedCardProvider`** and
  **`RequestHandlerBuilder::with_extended_card_provider`** (server; spec
  §13.3: "Agents MAY return different extended card content based on the
  authenticated client's identity or authorization level"). Every
  authenticated caller got the one configured extended card. A provider is
  given the request's `CallContext` once the interceptor chain has
  authenticated it, and the configured extended card, which `build()` then
  requires; it returns the card for that caller, or an error that refuses
  the request. Its card is checked as the configured one is (valid, push
  notifications only with a push sender, the server's extensions added
  when missing, refused when signed without them), and one that fails is
  an internal error, not served.
- **`ListTaskPushNotificationConfigs` pages its configs** (server; spec
  §3.1.9: "MAY support pagination for tasks with many configurations").
  `pageSize` and `pageToken` were ignored and `nextPageToken` was always
  absent. With either, the configs are ordered by id and a page carries a
  `nextPageToken` while more remain; a token names the last config
  returned, so a config created or deleted between pages neither repeats
  nor shifts the rest. A token this server did not give is
  `InvalidField(pageToken)`, and `pageSize: 0` is unset, as proto3 reads it.
  Without either, every config is returned as before. Every binding pages
  through the new `RequestHandler::on_list_push_configs_page`.
- **`signing::sign_card_es256_with_jku`** and **`sign_card_ed25519_with_jku`**
  (types; spec §8.4.2: the protected header MAY include `jku`). The
  signers took no `jku`; these put one in the protected header, where the
  signature covers it, and refuse one that is not `https://` (RFC 7515
  §4.1.2).
- **`CachingCardResolver::with_options`** and
  **`ClientBuilder::from_signed_card`** (client; spec §14: clients SHOULD
  verify signatures when present). The caching resolver fetched with
  default options, so it could not verify a signed card at all, and a
  client built from a card the caller held had no way to verify it. The
  resolver now fetches with the `CardFetchOptions` it is given, signature
  keys included, and caches nothing that fails; `from_signed_card` (feature
  `signing`) verifies the card's JSON as given, before reading it into an
  `AgentCard`, then builds as `from_card` does.
- **`auth::caller_id`** (server): `bearer`, `api_key` and `jwt` build the
  identities the bundled interceptors record.
- **`DispatchConfig::with_agent_card_max_age`** (server; spec §8.6.1: the
  card's `max-age` SHOULD suit how often it changes). The JSON-RPC, REST
  and axum dispatchers served the public card with `max-age=3600` and no
  way to change it; it is now the deployment's, defaulting to 3600.
- **`AuthInterceptor::api_key`** (client; spec §4.5.2, §3.1.11). An
  agent whose card declares an `APIKeySecurityScheme` reads its key from the
  header, cookie or query parameter the scheme names; the client could only
  write the key into `Authorization`, which such an agent refuses. `api_key`
  sends it where the scheme says: a header or cookie over JSON-RPC,
  HTTP+JSON and gRPC, and a query parameter over JSON-RPC and HTTP+JSON
  (see Breaking Changes). A name that is not an HTTP token (RFC 9110
  `tchar`), which no header or cookie can carry, is refused when the
  interceptor is made, for a query parameter too, with the new
  `ClientError::InvalidAuthScheme`.
- **`ClientBuilder::with_extensions`** and
  **`WebSocketTransportConfig::with_extensions`** (client; spec §3.2.6: the
  `A2A-Extensions` service parameter lists the extension URIs "the client
  wants to use for the request"). The server read the header on every
  binding, but the client could send it only as a header the caller set
  itself. The builder now sends the URIs as one comma-separated
  `A2A-Extensions` header (gRPC metadata on gRPC) on every request, unless
  an interceptor set the header itself; the WebSocket transport sends them
  with its upgrade, where that binding's server reads them. A URI that is
  empty, or holds a comma, whitespace or anything but visible ASCII, is
  refused at build or connect.
- **Versioned documentation** (site). [a2a-rust.com](https://a2a-rust.com)
  was the book built from `main`, so a reader on a release saw the next
  release's unreleased and breaking changes. Now `/` is the latest
  release's book, built from its tag, `/vX.Y/` each release line's (all
  thirteen, 0.2 to 0.14), and `/dev/` is `main`'s, with `main`'s rustdoc at
  `/dev/api/`; `/api/` redirects to docs.rs at the release. Every page has
  a version selector, and a page that is not the latest release's says so.
  Only `/` is indexed. Built by `scripts/build_docs_site.py`, checked by its
  `check` step, which `prove_workflow_gates_fail.py` proves can fail; it
  deploys on every push to `main` and when the Release workflow succeeds.
- **`HttpPushSender::https_only()`** (server; spec §13.2: webhook URLs
  SHOULD use HTTPS). Opt-in: a push config whose URL is not `https://` is
  refused at creation with `InvalidParams` naming the reason, and a delivery
  to one is refused before any connection. Senders of your own report it
  through the new provided method `PushSender::requires_https` (default
  `false`). Off by default; `http://` webhooks are still delivered unless it
  is set.
- **`PushConfigCipher`** and the SQL push-config stores' `with_cipher` and
  `accept_unsealed_rows` (server, features `sqlite` / `postgres`; spec §13.2:
  agents SHOULD securely store push notification configurations and
  credentials). Opt-in: given a cipher, `SqlitePushConfigStore`,
  `PostgresPushConfigStore` and their tenant-aware forms write each config
  sealed (the whole JSON document, so the client's `token` and the webhook
  `credentials` are never stored in the clear) as `a2a-sealed:v1:<base64>`,
  bound to its row (tenant partition, task id, config id) as associated
  data, so a sealed config moved to another row does not open. With a
  cipher, an unsealed row is refused unless `accept_unsealed_rows` is set
  for migrating a table written before; without one, a sealed row is
  refused. The SDK ships no cipher: plug in an AEAD whose key your KMS
  holds (the tests use AES-256-GCM from `ring`).
- **`AuthFailureThrottle`** (server; spec §13.4: agents SHOULD rate-limit
  to prevent brute-force attacks). An interceptor, registered before the
  authenticating one, that counts per peer the calls refused as
  unauthenticated and, once a peer reaches `max_failures` in a window,
  refuses its calls as rate-limited (HTTP+JSON `429` with `Retry-After`,
  JSON-RPC `-32603`, gRPC `RESOURCE_EXHAUSTED`, the last two with a
  `RetryInfo` detail) until the window ends, the right credential included. A peer is the connection's
  IP address, or the client address trusted proxies appended to
  `x-forwarded-for`; at most `max_peers` are tracked. Opt-in: nothing
  throttles failed authentication unless it is registered.
- **`CallContext::peer_addr`**, **`PeerAddr`** and **`PEER_ADDR_KEY`**
  (server): the address of the connection a call arrived on. The SDK's
  servers (`serve`, `serve_with_addr`, `Server`, the WebSocket and gRPC
  dispatchers) record it; the axum router reads it from `PeerAddr` or from
  `ConnectInfo<SocketAddr>`. It travels in the headers map under
  `":peer-addr"`, which no client can send, as the request path does under
  `":path"`. `RateLimitInterceptor` does not use it: a caller it cannot
  identify is still keyed by trusted `x-forwarded-for` hops or the shared
  `anonymous` bucket.
- **`AgentCard::validate_security_schemes`** (types; proto `REQUIRED`):
  refuses a security scheme whose REQUIRED string is empty. See "refuses
  empty required strings" under Behaviour Changes.
- **`A2aError::with_retry_info_from_details`** (types): records the delay of a
  received error's `RetryInfo` detail, so `retry_after` (now a `const fn`)
  answers for an error from the wire as it does for one built with
  `rate_limited`.
- **`ClientError::CapabilityNotDeclared` and
  `ClientBuilder::without_capability_checks`** (client; spec §3.3.4), and the
  CLI's `--no-capability-check`: see Behaviour Changes. The error names the
  method that was not sent and the card field that would have declared it;
  it is not retryable.
- **`RequestHandlerBuilder::without_agent_card`** (server; spec §8.1): builds
  a handler with no agent card, which `build()` now refuses otherwise. See
  Behaviour Changes.
- **`RequestHandlerBuilder::share_tasks_across_callers`** (server; spec
  §13.1): keeps one set of records per tenant, shared by every caller in it,
  now that an authenticating interceptor isolates callers by default. See
  Behaviour Changes.
- **`A2aError::unavailable` and `A2aError::is_unavailable`** (types; spec
  §3.3.2). A failure a retry may get past: HTTP `503` with status
  `UNAVAILABLE`, gRPC and SLIMRPC `UNAVAILABLE`, JSON-RPC `-32603` as before.
  For a custom store; the SQL stores use it (see Fixed).
- **`RequestHandlerBuilder::with_extended_agent_card`** (server; spec §3.1.11,
  §13.3). The card `GetExtendedAgentCard` serves to an authenticated caller,
  distinct from the public one: a further skill, more detailed capabilities.
  A handler whose card declared `extendedAgentCard` could only serve that
  public card again, so authenticating gained a caller nothing. `build()`
  validates it, requires a push sender if it declares push notifications,
  and advertises the server's extensions on it as on the public card; it
  refuses one the public card does not declare, since it would never be
  served. Without it, the public card is served, as before.
- **`CardFetchOptions::with_signature_keys`** (client, feature `signing`;
  spec §14: clients SHOULD verify signatures when present). Card discovery
  with these options returns a signed card only if one of its signatures
  verifies against the supplied JWK Set; a tampered card, or one with no
  matching key, fails the fetch. So does an unsigned card, since a card
  whose signatures were removed on the way reads the same;
  `CardFetchOptions::with_require_signature(false)` returns it, verifying
  signatures only when present.
- **`dispatch::Hsts`** (server; spec §13.4). Wraps any `Dispatcher` and adds
  `Strict-Transport-Security` to every response. Opt-in: the built-in
  listener is plaintext, and the header only takes effect behind a TLS
  terminator.
- **`HttpPushSender::with_allowed_hosts`** (server; spec §13.2). An opt-in
  allowlist of webhook hosts, enforced when a push config is created and
  again before each delivery. `PushSender::allows_webhook_url` (default
  `true`) lets a custom sender do the same.
- **`CachingCardResolver::install_extended_card` / `clear_extended_card`**
  (client; spec §13.3). Serves the extended card in place of the public one
  for an authenticated session; `invalidate` clears both.
- **`RequestContext::accepted_output_modes()`** (server). The proto says an
  agent SHOULD use `SendMessageConfiguration.acceptedOutputModes` to tailor
  its output, and the executor had no way to see it. Empty means no
  preference.
- **Per-caller task isolation** (server; spec §13.1):
  `RequestHandlerBuilder::isolate_tasks_by_caller`. Tasks were scoped by
  tenant only, so two callers authenticated as different identities in one
  tenant, or in the default single-tenant deployment, could get, list,
  cancel and subscribe to each other's tasks and register webhooks on them.
  With this, the identity an authenticating interceptor sets selects the
  records a request can reach: another caller's task is `TaskNotFound`, the
  same answer as an unknown id, and `ListTasks` returns only the caller's
  own. The tenant-aware stores key by `TenantContext::partition` (the tenant,
  plus the caller when isolating); the halt switch, quotas, audit chains and
  metrics still see the tenant. `build()` refuses the setting over stores
  that do not partition by tenant and over a chain with no authenticating
  interceptor. Off by default: sharing within a tenant is still the
  behaviour unless a deployment asks for this. A tenant containing U+001F,
  the separator in a partition key, is now refused.
  `tests/caller_isolation.rs` covers every operation that names a task,
  lists, webhooks, background completion, and SQLite; a no-op isolation
  fails three of its tests, and a background spawn that drops the caller
  fails the completion test. The caller is recorded in a per-request slot
  once the interceptors have run, rather than by scoping the rest of each
  method in a second future, which had pushed request futures to 29-34 KB.
- **A requirement-level compliance ledger** (`docs/compliance/spec-ledger.json`,
  rendered as `spec-ledger.md`). Every RFC 2119 statement in the A2A
  specification (`main` at `12e9d2fb`, v1.0.1 plus editorial fixes and
  §7.6.4) and every `REQUIRED` field in the proto has an entry: who it binds,
  whether it is tested, partial or open, at what severity, and the code and
  test that say so. 512 entries; 220 tested, 19 gaps, 67 partial, 46
  implemented without a test, the rest the agent author's or optional.
  `scripts/check_spec_ledger.py` (CI) fails when the vendored specification
  carries a requirement with no entry, an entry points at text that is no
  longer normative, a cited test no longer exists, or the page is stale.
  The entries were made by seven read-only audit passes over the code; the
  ones this release changed were re-verified, and the ledger says so.
- **`profile::Profile::Auditable`** (server, `audit` feature; book, "Audit
  Trail"). `RequestHandlerBuilder::with_profile` turns on the audit trail and
  an approval gate in one call. `build()` refuses the configuration, naming
  what is missing, when:
  - the log is not required;
  - the log signs no checkpoints;
  - no authenticating interceptor is installed.
- **`a2a` CLI offline commands** (`tools/a2a-cli`). These need no agent,
  print JSON, and exit 1 when the check fails:
  - `a2a card sign` signs a card file with ES256 or EdDSA;
  - `a2a card verify` checks a card from a URL or a file against a JWK Set
    from an HTTPS URL or a file;
  - `a2a audit verify` checks an exported audit chain against a JWK Set and
    prints the chain report.

  The CLI now carries the release version (0.14.1), and `release.yml` checks
  it against the tag with the four library crates, so a release binary
  reports what it is.
- **Release binaries for the CLI, off until switched on** (`release.yml`,
  `cli-binaries`). When the repository variable `PUBLISH_CLI_BINARIES` is
  `true`, each tag builds `a2a` natively for x86_64 and aarch64 Linux,
  aarch64 macOS and x86_64 Windows. Each archive gets a SHA-256 file and
  SLSA build provenance, and is attached to the GitHub release. Homebrew and
  winget manifests are drafted under `packaging/`; they have not been
  submitted or validated.
- **The provenance extension: AI-generated marker and signed content**
  (types; `https://a2a-rust.com/extensions/provenance/v1`; book,
  "Provenance and Signatures").
  - `mark_ai_generated` marks a message or artifact as AI-generated and
    names its generator, in machine-readable metadata (EU AI Act Article
    50(2)).
  - With `signing`, `sign_content` signs the whole content, marker included,
    with ES256 or EdDSA, and `verify_content` checks it against a JWK Set.
    Changing the text, the marker or other metadata after signing breaks the
    signature. A signed artifact still verifies after a round trip through a
    server, and a file part's bytes, including a C2PA manifest inside them,
    come back unchanged (`tests/content_provenance_e2e.rs`).
- **Card verification with EdDSA and JWK Sets** (types and client;
  `signing`).
  - `sign_card_ed25519` signs with Ed25519, and verification accepts EdDSA
    as well as ES256.
  - `signing::{Jwk, Jwks, VerifyingKey}` and `verify_agent_card_with_jwks`
    select keys by `kid`. A key is only ever tried under the algorithm its
    type is for.
  - `verify_card_with_jwks` tries every signature on a card, which covers
    key rotation.
  - The client's new `jwks::fetch_jwks` fetches a set from a URL the caller
    trusts. It requires HTTPS (or HTTP to loopback), caps the body at 256
    KiB, and gives the whole fetch one time budget.
  - Checked against RFC 8037's Ed25519 example and against keys generated
    by OpenSSL.
- **`RequestHandler::halt` / `resume`: stop one tenant or the whole server**
  (server; book, "Human Oversight"). While a scope is halted, new sends are
  refused with the new `ServerError::Halted` (HTTP `503` and gRPC
  `UNAVAILABLE`; JSON-RPC answers an internal error reading
  `halted: <reason>`). Every running task's cancellation token fires, and an
  executor that returns on it without a terminal state has its task ended
  `canceled`, as on shutdown. Reads and cancels are still served. A send
  racing the halt is either refused or stopped; with the second check
  removed, the race test failed. With `audit`, each halt and resume is a
  `halt` record naming the operator and the tasks stopped. `resume` returns
  `ServerResult<bool>`.
  - **Across replicas and restarts** (ADR 0019): with
    `RequestHandlerBuilder::with_halt_store` a halt is also written to a
    `HaltStore` (`InMemoryHaltStore`, `SqliteHaltStore`,
    `PostgresHaltStore`), which each replica reads before admitting its
    first send and then every `with_halt_poll_interval` (one second by
    default). A halt given to any replica stops every replica's tasks
    within one interval, and a restarted process comes back halted. A halt
    the store cannot take still holds where it was given
    (`HaltReport::persisted` is `Some(false)`) and is written again on each
    poll. Without a store, a halt holds in one process, as before.
- **The approval extension, and `approval::ApprovalGate`** (types and
  server; `https://a2a-rust.com/extensions/approval/v1`; book, "Human
  Oversight"). An executor asks with `EventEmitter::request_approval`,
  naming the action by the SHA-256 digest of its canonical JSON
  (`approval::action_digest`, `signing` feature). The server records who
  asked. With a gate installed, a continuation carrying a decision reaches
  the executor (`RequestContext::approval`) only if all of these hold:
  - it answers the pending request and echoes its digest;
  - it comes from an authenticated approver the gate allows;
  - by default, the approver is not the caller whose run asked.

  Otherwise it is refused before anything runs. Without a gate,
  `RequestContext::approval` is always `None`. With `audit`, each admitted
  decision is an `approval` record naming the approver. The card declares
  the extension when a gate is installed.
- **`delegation::Delegation`: cancellation that follows a task to the agents
  it delegated to** (client; book, "Delegating to Another Agent"). It sends a
  child task as a stream and learns the child's id from its first event. It
  cancels the child, in the tenant it was sent to, when the parent cancels,
  when the child's stream is lost, or when the handle is dropped first. A
  dropped handle is what an aborted parent future leaves behind. A child that
  reaches `input-required` or `auth-required` is handed back, not cancelled.
  `detach()` keeps a child running. Outcomes are typed: completed, failed
  with its failure class, canceled, interrupted, message, lost, cancel
  requested, cancel failed, unreachable. `examples/swarm` now uses it in
  place of its hand-written cascade; its cancel gate still finds 0 of 256
  children running 5 s after the roots are cancelled, against 256 of 256 in
  the control arm (debug build, 2026-10-08). It does not cover a parent that
  crashes (gap G1-B in `docs/swarm-orchestration.md`).
- **A tamper-evident audit trail** (`audit` feature on the server, types and
  SDK crates; ADR 0015; book, "Audit Trail").
  `RequestHandlerBuilder::with_audit(Arc<AuditLog>)` records every call —
  refused ones included — every run of a task with the caller who started
  it, every event the agent emits with the run it belongs to, and every
  cancel request, in a SHA-256 hash chain per tenant. Each record carries
  the authenticated subject and scheme, the tenant and the W3C trace and span
  ids; content is recorded only as digests of its RFC 8785 canonical JSON.
  `CheckpointSigner` (ES256 or Ed25519 through `ring`, PKCS#8 as OpenSSL
  writes it) signs checkpoints that make truncation of a chain's tail
  detectable, and `a2a_protocol_types::audit::verify_chain` checks a chain
  and its checkpoints offline, reporting the first edit, gap, reordering,
  foreign record, missing start or removed tail. Stores: in memory, SQLite
  and PostgreSQL (`SqliteAuditStore`, `PostgresAuditStore`); replicas sharing
  one store interleave on a chain without forking it. Retention:
  `AuditLog::purge` with `AuditRetention::six_months()` (184 days; a shorter
  floor must be asked for by name), legal holds, and a signed anchor so a
  purged chain still verifies. `AuditLog::require_record(true)` refuses a
  call that cannot be recorded. A failed write is counted by
  `AuditLog::failures` and reported as `audit_append` through
  `Metrics::on_persistence_error`. Measured cost, release build on a 4-vCPU
  Xeon @ 2.10 GHz: 9.2 µs per record in memory, 272 µs on a SQLite file
  (medians of five runs).
- **`CallContext::auth_scheme` / `set_auth_scheme`** (server). The bundled
  API-key, bearer and JWT interceptors now record how they authenticated a
  caller (`"api-key"`, `"bearer"`, `"jwt"`) beside the identity; a custom
  interceptor can do the same.

- **`otel::Telemetry`: one entry point for traces, metrics and logs** (server,
  `otel` feature; ADR 0013 option 5). It reads the `OTEL_*` environment as the
  OpenTelemetry specification defines it — `OTEL_SDK_DISABLED`, the
  per-signal `OTEL_*_EXPORTER` (`otlp` or `none`; anything else is an error
  naming it), `OTEL_EXPORTER_OTLP_PROTOCOL` (`grpc` or `http/protobuf`, the
  default), endpoints, headers, timeout, `OTEL_EXPORTER_OTLP_CERTIFICATE`,
  and `OTEL_SERVICE_NAME` winning over `with_default_service_name` — installs
  the tracer and meter providers and the `tracecontext` + `baggage`
  propagators, and returns a guard whose drop or `shutdown` flushes all
  three. `layer()` is the `tracing` layer for spans and log records;
  `otel_metrics()` is the `Metrics` provider on its meter. OTLP/HTTP runs on
  this crate's hyper and rustls (ring) stack, not reqwest. Export runs on a
  private runtime, so it works from a plain `fn main` or a `current_thread`
  runtime and its shutdown neither deadlocks nor loses the final flush there.
  Over OTLP/HTTP a failed or refused export is counted and reported by
  `shutdown` and `force_flush`, which the OpenTelemetry SDK's own shutdown
  does not do (`opentelemetry_sdk` 0.32.1 returns `Ok` after a failed final
  export). Tested end to end against stand-in OTLP/HTTP, HTTPS (private CA,
  with the counter-test without it) and gRPC collectors.
- **`otel::Langfuse`: a preset for Langfuse.** `Langfuse::from_env()` reads
  `LANGFUSE_PUBLIC_KEY`, `LANGFUSE_SECRET_KEY` and `LANGFUSE_BASE_URL`, the
  variables Langfuse's own SDKs read. Unlike theirs, the base URL defaults to
  a self-hosted instance, `http://localhost:3000` (`LANGFUSE_SELF_HOSTED`),
  so nothing leaves the machine unless it is set;
  `TelemetryBuilder::with_langfuse` sends traces to
  `/api/public/otel/v1/traces` over OTLP/HTTP with Basic auth and
  `x-langfuse-ingestion-version: 4`, and turns metrics and logs off by
  default, because Langfuse discards OTLP metrics and has no OTLP logs route.
  Verified against a self-hosted Langfuse 4.53.0; see the book's new
  Langfuse page.
- **The call's `SERVER` span carries the draft OpenTelemetry A2A
  attributes** (`open-telemetry/semantic-conventions-genai` #195, open, read
  at `842a839`): `a2a.method.name`, `a2a.protocol.version`, `a2a.tenant`,
  `a2a.message.id`, `a2a.message.reference_task_ids`, `a2a.task.id`,
  `a2a.task.state`, `gen_ai.conversation.id` (the `contextId`) and
  `gen_ai.agent.{name,description,version}` from the card.
- **Every client call runs in a `CLIENT` span** (client, `tracing` feature;
  audit O8) named for the method like the server's, with the same A2A
  attributes, the agent from its card, `server.address` and `server.port`,
  and `error.type` on failure. A streaming call's span covers consuming the
  stream. With the new client `otel` feature — which the SDK's `otel` now
  turns on — the call's `traceparent` names that span, so the agent called
  becomes its child with no interceptor or `CurrentTrace` scope (audit O9);
  `ClientBuilder::with_trace_propagation(false)` and
  `ClientConfig::propagate_trace` turn it off.
- **`RequestHandlerBuilder::with_span_content_capture(true)`** records the
  request's message and the agent's replies and artifacts on the executor's
  span as `gen_ai.input.messages` / `gen_ai.output.messages`, in the GenAI
  conventions' JSON format, at most 64 KiB each; raw bytes are never
  recorded. Off by default: it copies what users and agents say into traces.
- **`examples/langfuse-agent`**: an orchestrator delegating to a worker,
  traced into one Langfuse trace, with a self-hosted Langfuse in two commands.

- `serve::Server::from_listener` builds the HTTP (JSON-RPC and REST) graceful
  server on a `tokio::net::TcpListener` the caller has already bound, as the gRPC
  and WebSocket dispatchers' `serve_with_shutdown` already allow. This covers
  systemd socket activation, sockets handed over for a restart without
  downtime, socket options `Server::bind` does not set (`SO_REUSEPORT`,
  `IPV6_V6ONLY`, a custom backlog) and binding before dropping privileges
  (#151). `Server::bind` now delegates to it.

- **`RequestHandler::undeclared_binding` and `warn_if_undeclared` are
  public** (spec §5.2: an agent MUST declare every protocol it serves). Every
  listener in this crate runs the check when it starts; a binding outside it
  could not. `SlimRpcServer` now runs it with its own `protocolBinding`, so a
  SLIMRPC agent whose card omits SLIMRPC is warned about at construction, as
  the other listeners' agents are.

### Behaviour Changes

- **The SQLite stores zero deleted content in the database file**
  (`secure_delete=ON` on every pooled connection: the task, tenant, push
  configuration, halt and audit stores). SQLite leaves a deleted or
  rewritten cell in its freed page until something reuses it, so `delete`,
  `purge_expired` and `erase_caller` left the bytes in the file. They are
  now overwritten with zeros there. They are not overwritten in the
  write-ahead log, which keeps a page's earlier image until a checkpoint
  truncates it, nor in a backup. Writes that free or rewrite content do a
  little more work; the cost was not measured. Test: `sqlite_pool.rs`
  `a_deleted_row_does_not_stay_in_the_database_file` reads the file's bytes
  and, as its control, finds the deleted text there with the pragma off.

- **`rpc.server.call.duration`, `rpc.client.call.duration` and
  `a2a.server.request.duration` use the semantic conventions' current
  advisory buckets, 0.1 ms to 100 s** (`docs/rpc/rpc-metrics.md` at
  `55c98cf`), instead of the 5 ms to 10 s set they advised at `838e414`.
  An agent call that waits on a model routinely takes longer than ten
  seconds, and every such call fell in the last bucket, so a p95 or p99
  read 10 s whatever the calls took (seen on the client dashboard over a
  harness whose calls took 10 to 80 s). The bucket boundaries are part of
  each series' identity: a stored histogram's quantiles across the upgrade
  mix the two sets until the old series ages out. ADR 0013 records the
  change. Tests: the boundaries asserted in `call_metric::tests`,
  `otel::rpc_tests` and `observability_e2e`.

- **A `Message` an executor writes during its task is streamed as a status
  update** (server; spec §3.1.2, §3.5.2). A stream is exactly one `Message`
  or a `Task` followed by status and artifact updates; a `Message` written
  after the task stream began was forwarded as a `Message` inside it, which
  is neither. The queue's writer now sends it as a `TaskStatusUpdateEvent`
  that keeps the state the turn last wrote (or, before any status, the
  state the turn began in) and carries the message in `status.message`. It
  does so once, before the event takes its log position, so the send
  stream, a `SubscribeToTask` stream and a resumed replay carry one payload
  at that position. The store appends the message to `history` as before,
  and, while the task is not finished, also holds it as the status message;
  push subscribers are sent the update. A turn that continues an existing
  task (a follow-up naming its `taskId`, §3.4.3) folds every `Message`, its
  first included: §3.1.2 gives the message-only stream to the exchange with
  "no task tracking", and §3.7 has an agent "attach Messages to status update
  events" once a task exists. A follow-up answered with a `Message` now gets
  a task stream (`Task`, then the status update) instead of a message-only
  stream, and a blocking follow-up the `Task` with the message as its status
  message instead of the bare `Message`; a `SubscribeToTask` stream on the
  task gets the same status update. An executor that answers a follow-up
  that way and sets no state leaves the task waiting where it waited
  (`input-required` or `auth-required`), the reply as its status message:
  the handler writes that state after the executor returns (not after a
  caller's `CancelTask`). Left `Submitted`, the stream ended on a state a
  client reads as an incomplete stream, and `Delegation` reported `Lost`
  after cancelling the child; it now reports `Interrupted` with the reply.
  An executor that answers a waiting task with an acknowledgement and
  returns, meaning to finish out of band, now leaves the task waiting for
  input rather than `Submitted`: set a state (`working`) to say otherwise.
  Still unsettled, as before: a follow-up whose executor writes nothing, or
  only artifacts, and one to a task an earlier turn left `working`. Push
  subscribers get the reply twice, in the folded update and the re-park.
  Found by the independent reviews (`tests/continuation_message.rs`,
  `tests/continuation_span.rs`, `delegation_tests.rs`
  `a_follow_up_answered_with_a_message_leaves_the_child_waiting`). Unchanged: a new task's first-event
  `Message` is a direct reply, a `Message` after only messages on a new task
  stays one, and one after the terminal status is stored and streamed to no
  one. A client that
  read mid-task `Message` events reads `status.message` on status updates
  instead. With it: the executor's span no longer ends the turn's recorded
  output at such a message (`rpc_span/span_tests.rs`
  `a_mid_task_message_does_not_end_the_turn`); the audit trail's
  `task.event` record of a status update that carries a message names the
  message's id, as the record of a `Message` event did (`tests/audit_trail.rs`);
  and a refused history append on a task with no history restores no
  history rather than an empty one. Tests: `tests/mid_task_message.rs`,
  `streaming/event_queue/message_fold.rs`, the processor and state-machine
  unit tests named `carried`, `tests/event_processing_tests/streaming_mode.rs`;
  each part was shown to fail with it disabled.

- **A refused call is recorded, and counted, for why it was refused**
  (server; spec §3.3.2; audit WP-A). `ServerError::metric_label`, which
  names an error in the audit trail's `call` records and in
  `a2a.server.errors`, answered `protocol` for every A2A error a binding
  sent as it was, so a missing credential and a denied operation were one
  record. It now answers `unauthenticated` or `permission_denied` for a
  refused credential and the error code's label (`version_not_supported`,
  `content_type_not_supported`, …) for any other. A dashboard or query that
  matched `protocol` matches these instead. Tests: `error/label_tests.rs`
  `a_protocol_error_is_labelled_for_what_it_says`;
  `tests/audit_trail.rs`, `tests/audit_security_events.rs` (each fails with
  the old label).

- **SLIMRPC: the binding takes the server at its default features, and its
  `otlp` feature, on by default, builds the SDK's `Telemetry` into
  `slim-node` and `examples/in_process`** (`a2a-protocol-slimrpc`). The
  server dependency was `default-features = false`, a leftover from before
  `tracing` and `otel` were the server's defaults; a SLIMRPC call's `SERVER`
  span and `rpc.server.call.duration` now reach the application's
  subscriber and meter provider. The node builds `Telemetry` from the
  `OTEL_*` environment, as every other binary does, and exits with status 2
  on a configuration it cannot use. Two conflicts with SLIM's own
  dependencies first kept `otlp` off by default, and both are fixed: the
  server's OTLP/gRPC no longer links tonic's TLS (see the server entry
  below), which had made tonic refuse the `https://` endpoint SLIM dials and
  hang the TLS node suites; and `install_crypto_provider()`, which
  `SlimRpcServerBuilder::build` and `SlimRpcTransportBuilder::connect` call,
  installs SLIM's rustls provider when the process has none, since rustls
  picks no default with `ring` and `aws-lc-rs` both linked and SLIM's first
  handshake panicked. `default-features = false` drops `Telemetry` and the
  exporters. Tests: `tests/out_of_process.rs`
  `the_node_builds_telemetry_from_the_otel_environment`;
  `tests/crypto_provider.rs` (fails without the builder's install);
  `tests/remote_node_tls.rs` and `remote_node_mtls.rs` at default features
  (hang with tonic's TLS linked, killed at 300 s).

- **The server's OTLP/gRPC export dials TLS through its own connector**
  (server, `otlp`). `opentelemetry-otlp` was built with tonic's TLS
  (`tls-ring`, `tls-webpki-roots`), and tonic with any TLS feature refuses
  an `https://` URI it holds no TLS config for, whoever dials it, which broke
  a dependent dialling TLS through a connector of its own (SLIM's client).
  An `https://` collector is now reached over the same rustls stack as
  OTLP/HTTP: Mozilla's roots, the extra CAs, the client certificate, ALPN
  `h2`, with tonic given the endpoint as `http://` and the connector always
  dialling TLS. It works whether or not something else in the process turns
  tonic's TLS on. Tests: `tests/telemetry_export` (all 31, the four gRPC TLS
  ones included, in a test build that does link tonic's TLS).

- **An executor's `RequestContext::activated_extensions()` lists only the
  extensions the agent card declares** (server; spec §4.6.3: an extension
  or version the agent does not support SHOULD be ignored for the
  interaction). The response's `A2A-Extensions` header already echoed only
  the declared set, but the executor was given the raw request list, so it
  was told `…/ext/v2` was active when the card declared only `…/ext/v1`. The
  `RequestContext` given to `execute` and `cancel` now carries the
  intersection, in request order; interceptors (`CallContext::extensions`)
  and the required-extension check still see the raw list, and without a
  card the list passes through. An executor that read an undeclared
  extension no longer finds it: declare it on the card. Tests:
  `handler_conformance/extension_activation.rs` (three).
- **A refused request's `google.rpc.BadRequest` names the missing field
  itself, and the `SendMessage` ids by their wire paths** (server; spec
  §5.7, §11.6). A missing key was reported at its parent (`message` for a
  message without `role`, `""` for a request without `message` or `id`); it
  is now `message.role`, `message.messageId`, `message`, `id`, `url`, on
  JSON-RPC, WebSocket and HTTP+JSON. The `SendMessage` id checks named
  `message.id`, `task_id` and `context_id` (reference ids were not checked); they are
  `message.messageId`, `message.taskId`, `message.contextId` and
  `message.referenceTaskIds`, in the violation's `field` and the message
  ("message.messageId must not be empty or whitespace-only"). A client
  matching the old strings must use the new ones. Tests:
  `handler_conformance/required_fields.rs` (two).
- **An empty push-config `url` is `InvalidParams` naming `url`, checked
  before the task lookup** (server). It reached the lookup first and
  answered `TaskNotFoundError` for an unknown task, while the same request
  without the key answered `InvalidParams`. Tests: `required_fields.rs`,
  `set_push_config_rejects_an_empty_or_schemeless_url_when_private_urls_are_allowed`.

- **Default metrics follow the global meter provider at each measurement**
  (server, client). A handler given no `with_metrics`, and every client's
  `rpc.client.call.duration`, bound their instruments to the provider that
  was global when the builder or client was created; one built before
  `Telemetry` was installed recorded into the no-op meter for its whole
  life. Both now ask for the global provider at each measurement and build
  their instruments again when it has changed (a pointer comparison while
  it has not). An `OtelMetrics` passed to `with_metrics` still records to
  the meter it was built from. `scripts/check_otel_metrics_coverage.py` now
  also requires the default to forward every `Metrics` callback. Tests:
  server `default_metrics.rs` and client `call_metric_late_provider.rs`
  (built first, then a provider installed, then replaced), each failing with
  the old binding.

- **An interface with an empty `protocolVersion` is no longer selected**
  (`ClientBuilder::from_card`); it counted as compatible ("the card did not
  say"). A card whose only interface states none is refused with
  `InvalidEndpoint` listing what it offers. Test:
  `selection_tests.rs an_interface_without_a_protocol_version_is_passed_over`.
- **`list_tasks` sends `status_timestamp_after` in UTC with `Z`** on every
  binding: an offset or lower-case `t`/`z` is rewritten (millisecond
  precision); an already canonical value is sent as given; a value that is
  not ISO 8601 is refused unsent with `ClientError::Protocol(InvalidParams)`.
  Test: `wire.rs list_tasks_sends_status_timestamp_after_in_utc`,
  `list_tasks_refuses_a_timestamp_that_is_not_iso_8601_unsent`.
- **An inline push config's `task_id` is no longer sent with `SendMessage`**
  (a2a.proto: "Task id should be empty"); it is cleared, with a debug log,
  rather than refused. Test: `wire.rs an_inline_push_configs_task_id_is_not_sent`.
- **A JSON-RPC stream frame whose id does not echo the request's is warned
  about** once per stream (§9.4.2) and still delivered. Tests:
  `stream_ids.rs a_stream_frame_with_another_id_is_warned_about_once_and_delivered`,
  `a_stream_frame_echoing_the_request_id_is_not_warned_about`.
- **The HTTP+JSON client refuses a nested object bound for a query string**
  (§11.5) with `ClientError::Transport`; none reaches it today because every
  operation with one is `POST`. Test:
  `query.rs build_query_string_refuses_a_nested_object`.
- **Card validation refuses an `http://` URL in the deprecated implicit and
  password OAuth flows** (proto: "The OAuth2 standard requires the use of
  TLS"); publishing and receiving such a card both fail. Test:
  `security_url_tests.rs the_deprecated_flows_urls_must_be_https`.

- **`HttpPushSender` refuses to deliver without the configured
  credentials** (server; spec §13.2: agents MUST include the credentials
  `authentication` specifies). A stored config whose `authentication`
  names a scheme other than `Bearer` or `Basic`, or carries no (or blank)
  credentials, was delivered with no `Authorization` header; `send` now
  returns `InvalidParams` naming §13.2 before connecting, and the delivery
  path records the failure as for any other. Creation already refused such
  configs, so only one that reached a store another way is affected; leave
  `authentication` out for an unauthenticated webhook. The tests that pinned
  the old behaviour are inverted:
  `push_sender_tests::scheme_without_credentials_is_not_delivered`,
  `unknown_scheme_is_not_delivered`, `empty_credentials_are_not_delivered`.

- **`Telemetry` reports a `service.instance.id`** (server `otlp`): a random
  UUID per process unless `OTEL_RESOURCE_ATTRIBUTES` names one. Replicas of
  one service no longer export identical resources; in Prometheus each
  becomes its own `instance`. Test:
  `each_process_gets_an_instance_id_unless_the_environment_names_one`.

- **A malformed request without a credential is refused as
  unauthenticated, on every binding** (server; spec §7.4, §3.3.2;
  `docs/handoff.md` "Open engineering"). **A caller the interceptor chain
  does not admit now gets `401` (HTTP+JSON), `UNAUTHENTICATED` (gRPC) or the
  refusal frame (`-32600` with its `UNAUTHENTICATED` detail, WebSocket)
  where it got `400`, `INVALID_ARGUMENT` or `-32602` naming the field it got
  wrong.** HTTP+JSON (`RestDispatcher` and the axum router) read the body,
  its JSON and its shape, and the query values that must parse
  (`historyLength`, `ListTasks`' filters, `pageSize`), before the handler
  saw the call; gRPC converted the request; WebSocket parsed each frame's
  params. A request the binding cannot read is now admitted first
  (`RequestHandler::admit_unreadable`, the JSON-RPC fix of the same
  morning): one the chain refuses gets the refusal with its challenge, and
  one it admits gets the answer it always got, byte for byte. Likewise
  `ListTasks`' `pageSize` and `statusTimestampAfter` and
  `CreateTaskPushNotificationConfig`'s `taskId`, which the handler checked
  before admission on every binding, JSON-RPC included, are checked after
  it. Interceptors now see these calls (no task id; `on_complete` `Failed`
  with the validation error), so a rate limiter counts them and an audit
  interceptor records them. Still answered before admission, because they
  refuse a well-formed request alike and say nothing about its fields: the
  `A2A-Version` check (§3.6.2: the version names the semantics the request
  is read with), the request media type, the body's size cap (`413`) and
  read timeout (`408`) (§13.4 asks for size limits), the query-string
  length cap, routing (`404`/`405`), a gRPC message tonic cannot decode, and
  a WebSocket frame that is not JSON (`-32700`) or not a request
  (`-32600`), which names no method to admit it as, as over JSON-RPC. The
  tenant too: with a `TenantResolver`, a client tenant the resolver
  disagrees with, or none under `require_resolved_tenant`, was refused
  `InvalidParams` before admission, telling an unauthenticated caller that
  strict multi-tenancy was on or which tenant its credential did not map
  to; a refusal from `resolve_tenant` now goes through the chain first
  (`handler/tenant_resolution.rs`). Tests: `auth_before_params` (one module
  per binding, the handler checks over JSON-RPC and HTTP+JSON, and the
  tenant).
- **Two names the semantic conventions spell differently** (server and
  client; checked against `open-telemetry/semantic-conventions@55c98cf` and
  `semantic-conventions-genai@547934a`). The executor span's finish reason
  is `gen_ai.response.finish_reasons`, a string array recorded whether
  content capture is on or not; it was a `finish_reason` member of the
  captured `gen_ai.output.messages`, which `gen-ai-output-messages.json`
  deprecates in its favour. A client call whose `rpc.method` is `_OTHER` is
  named for its `rpc.system.name` and records `rpc.method_original`, as
  `docs/rpc/rpc-spans.md` asks and as the server already did; it was named
  `_OTHER`.
- **SLIMRPC: a streaming call's span and duration cover its stream**
  (`a2a-protocol-slimrpc`), as on the server crate's bindings (the entry
  on streaming calls below): `SendStreamingMessage` and `SubscribeToTask`
  are recorded when the event stream ends — `OK`, the error it ended on,
  or `cancelled` when the stream is dropped first — through
  `BindingSpan::run_with` and `StreamEnd`.
- **SLIMRPC: a client-side status is not a server error**
  (`a2a-protocol-slimrpc`). The binding sends gRPC's status codes and its
  server span now counts them as a gRPC server span does
  (`CustomBinding::with_grpc_server_errors`; `docs/rpc/grpc.md`): only
  `UNKNOWN`, `DEADLINE_EXCEEDED`, `UNIMPLEMENTED`, `INTERNAL`, `UNAVAILABLE`
  and `DATA_LOSS` set `error.type` and `Error` status. A `GetTask` for an
  unknown task is `rpc.status_code` `NOT_FOUND` with no `error.type`, as on
  the gRPC binding; it was counted as a server error.
- **A handler records its metrics to the global meter provider by default**
  (server, `otel`). `RequestHandlerBuilder` defaulted to `NoopMetrics`, so a
  process that installed a provider — `Telemetry` does — got the SDK's spans
  and none of its metrics until every handler was given
  `.with_metrics(...)`, though the `otel` entry above says it gets both with
  nothing to enable. With the `otel` feature (on by default) the default is
  now `OtelMetrics` on the global meter provider as it stands when
  `RequestHandlerBuilder::new` runs; with none installed that is
  OpenTelemetry's no-op meter. Without the feature it is still
  `NoopMetrics`, and `with_metrics` still replaces it.
- **A WebSocket call's span links to the upgrade request's trace instead
  of being its child** (server, WebSocket binding). A frame carries no
  trace context — the binding defines none, and this SDK's client sends
  none — so the only `traceparent` a WebSocket call had was the upgrade
  request's, and every call on a connection was recorded as a child of the
  operation that opened it: W3C Trace Context §3.4 has each request name its
  own parent, which is why the client already refused to send one fixed at
  the handshake. Each call's `SERVER` span is now a root of its own with a
  span link to that trace, under `InboundTracePolicy::Continue`. The
  upgrade's `traceparent` and `tracestate` reach the handler as the
  pseudo-headers `:upgrade-traceparent` and `:upgrade-tracestate`, so a
  WebSocket call's `CallContext` no longer carries the handshake's trace
  either.
- **A streaming call's span and `rpc.server.call.duration` cover its
  stream** (server; `SendStreamingMessage` and `SubscribeToTask` over SSE —
  JSON-RPC, HTTP+JSON and the axum router — gRPC and WebSocket). The
  semantic conventions' `docs/rpc/rpc-spans.md`: "For streaming RPCs, the
  span SHOULD cover the full lifetime of the request and/or response
  streams until they are closed or terminated", and the metric "SHOULD be
  the same as the RPC server span duration". Both ended when the handler
  returned the stream, and recorded a success whatever the stream did next.
  Now the call is recorded when its stream ends: completed; failed with the
  error its last frame or status carried (as `rpc.status_code` and, by the
  binding's rule, `error.type`); or `cancelled` when the client went away
  or the stream was dropped. The `SERVER` span's exported end time is the
  stream's end, after the SSE writer's `a2a.sse` span. The SSE writer now
  notices a client that went away at once — it watches the response body
  close, as the gRPC forwarder does (N31) — instead of at its next send,
  which a quiet task might not make for a long time, so the reader leaves
  the task's queue then too. A dashboard reading `rpc.server.call.duration`
  for the two streaming methods sees stream lifetimes, not handshakes.
  A binding outside the server crate gets the same through
  `BindingSpan::run_with`, `StreamEnd` and `StreamOutcome` (new).
- **`error.type` marks a server failure, by each binding's conventions**
  (server; `rpc.server.call.duration` and the `SERVER` span). It was set,
  with `Error` status, for every status but success. Now: every JSON-RPC
  error code, as `docs/rpc/json-rpc.md` says ("All JSON RPC error codes
  SHOULD be considered errors"); on HTTP+JSON a 5xx and not a 4xx, as
  `docs/http/http-spans.md` says of a server span ("For HTTP status codes in
  the 4xx range span status MUST be left unset"); on gRPC only `UNKNOWN`,
  `DEADLINE_EXCEEDED`, `UNIMPLEMENTED`, `INTERNAL`, `UNAVAILABLE` and
  `DATA_LOSS`, as `docs/rpc/grpc.md` says of a server span. `rpc.status_code`
  is unchanged and still carries every status, so a `GetTask` for an unknown
  task over gRPC is `NOT_FOUND` with no `error.type`. A dashboard counting
  client errors from `error.type` on HTTP+JSON or gRPC reads
  `rpc.status_code` instead. `CustomBinding::with_grpc_server_errors` gives
  a binding outside this crate gRPC's rule; without it every failure is
  still an error.
- **A stream that lags past the in-memory event log's bound is held, not
  cut off** (server; spec §3.5.2 "each stream MUST receive the same events
  in the same order"; maintainer decision 2026-10-20). The in-memory stores
  dropped a task's oldest logged event on every append past
  `TaskStoreConfig::max_events_per_task` (512 by default), whether or not a
  stream still owed it, so a stream that fell further behind than that
  found the head of its catch-up gone and ended with `streamLagged`. A
  stream now holds the positions it still owes (new `store::EventHolds` /
  `EventHold`, and the provided trait method
  `TaskStore::hold_events(&TaskId, after) -> Option<EventHold>`, default
  `None`: a store without it trims as before, and the SQL stores keep the
  whole log so need nothing). The log drops an event only once no attached
  stream owes it, up to a ceiling: new
  `TaskStoreConfig::max_held_events_factor` (`with_max_held_events_factor`),
  `Some(8)` by default, so 8 × 512 = 4,096 events a task; `None` is no
  ceiling, and `Some(1)` holds nothing past the bound, the old behaviour.
  The ceiling is a multiple of the bound rather than a count, so a
  deployment that lowered the bound for large artifact chunks keeps its
  worst case proportional. Past the ceiling the stream still ends with
  `streamLagged` after what it delivered, and the store reports it as
  `on_persistence_error("event_append", "hold_ceiling")`
  (`metrics::event_append_error::HOLD_CEILING`). **Memory:** a task with a
  slow or stalled stream attached now keeps up to the ceiling in its log
  instead of the bound, until the stream advances or disconnects and the
  next append, or for a finished task the next eviction sweep, trims it
  back. The ceiling's report is counted on every append it forces and
  logged once per task; `Some(1)` consults no hold and reports nothing, as
  before. A streaming send takes its hold before the task's event
  processor starts appending; it was taken after a store read that a
  `historyLength` request makes, and a stream that lagged during that read
  still ended with `streamLagged` (both found by the independent review). The audit, encrypted and tenant-aware wrappers
  forward the hold (the tenant store to the request's partition). Tests:
  `tests/stream_lag_holds.rs` (a lag of 62 positions behind a bound of 10
  now delivers every one, through a send, a subscription and a tenant
  store; past the ceiling it still ends with `streamLagged`),
  `store::task_store::in_memory::hold_tests`, `store::holds::tests`,
  `streaming::event_queue::hold_tests`, and the tenant store's
  `event_log_tests`.

- **An agent message on a task status, or in a snapshot's history, is
  given the task's ids when it carries none** (server; a2a.proto `Message`:
  "For server messages, `context_id` must be provided"). Only a direct
  reply was stamped; a status message an executor wrote without ids went
  out without them. Ids an executor set are kept, and user messages are
  untouched. An `AUTH_REQUIRED` status with no message is now logged as a
  warning (§7.6.1), not refused, since the details may have been negotiated
  out of band.
- **`RateLimitInterceptor` keys an unidentified caller by its peer address,
  and every key names its kind** (server; the maintainer's 2026-10-18
  decision; release audit F10, F11). A caller no interceptor identified
  shared one `"anonymous"` bucket, so one client could spend every other's
  budget. With `trusted_proxy_hops == 0` it is now keyed by the connection's
  peer address, which every built-in listener records; an IPv6 address
  spends its `/64`'s budget, since one host is routinely given a whole
  `/64`. Behind trusted proxies a short forwarded chain still falls back to
  the shared bucket, since the peer there is the proxy. Keys now carry a
  prefix (`id:<identity>`, `ip:<address>`, `tenant:<id>`, `anon`): an
  identity was the bare key, so one spelled `anonymous` or `tenant:acme`
  spent that bucket. A `RateLimitCounter` table sees the new key spellings;
  rows keyed the old way age out with their window. Size `max_buckets` for
  the distinct client addresses one window sees. Tests:
  `rate_limit::keying_tests`.
- **`serve` and `serve_with_addr` run on `Server`, so their connections
  get its default timeouts** (server). Both ran their own accept loop, which
  installed no hyper timer and wrapped no socket in an idle timer, so a peer
  that sent its request headers a byte at a time, or nothing, held a task
  indefinitely (release audit F1). They now bind a `Server` under
  `ServeConfig::default()`: a 30-second header-read timeout and a 75-second
  idle timeout (counting traffic in either direction, so an SSE stream with
  its 30-second keep-alive stays open), with connections still unbounded.
  Their signatures are unchanged; for a ceiling, TLS or a graceful drain,
  use `Server` directly. Regression test:
  `serve::tests::serve_with_addr_cuts_off_a_peer_that_never_finishes_its_headers`,
  which fails against the old loop.
- **The HTTP+JSON client sends an array query parameter by repeating it**
  (client; spec §11.5 "Multiple values MAY be passed by repeating the
  parameter name"). `ids=1&ids=2` where it sent one JSON-encoded value,
  `ids=%5B1%2C2%5D`. No v1.0 GET or DELETE request has a repeated field, so
  nothing on the wire changes today.

- **`CachingCardResolver` uses a fresh card without a request** (client;
  spec §8.6 and §8.6.2: clients SHOULD honor RFC 9111 when fetching Agent
  Cards). `resolve()` sent a request on every call, so a card's
  `Cache-Control: max-age` never saved one. It now returns the stored card
  while it is fresh (RFC 9111 §4.2): for its `max-age`, or else its
  `Expires` minus its `Date`, less the age it arrived with (`Age`, and
  `Date` against the local clock; §4.2.3). A stale card is revalidated as
  before; a card marked `no-cache` or `Vary: *` is revalidated on every
  resolve, and a `no-store` card is still not stored. A card with a
  lifetime and no `ETag` or `Last-Modified` is now kept while fresh, where
  it was not kept at all. After a `304`, the card is as fresh as the `304`
  says. **A card the agent changes is seen only once the stored one goes
  stale**: up to an hour behind this SDK's server defaults (`max-age=3600`).
  `with_always_revalidate(true)` restores the request on every resolve (see
  Added).

- **`CachingCardResolver::with_options` starts a new cache** (client;
  RFC 9111 §4.1). A clone shares its original's cache and installed
  extended card, so `resolver.clone().with_options(other)` returned a card
  stored under the original's credential, and now, with freshness, would
  have done so without a request. It starts with nothing stored and no
  extended card.
- **SLIMRPC: `AuthFailureThrottle` counts a verified caller's failures
  against that caller** (`a2a-protocol-slimrpc`; audit register R41). The
  server sets `:peer-addr` for a call with a verified peer to an address
  standing for it: in `100::/64`, the IPv6 discard-only block (RFC 6666),
  the low 64 bits a hash of the peer's identity keyed per process, port 0.
  `CallContext::peer_addr` therefore returns that address for such a call.
  Before, SLIMRPC set no `:peer-addr` and the throttle never throttled a
  SLIMRPC caller. A call without a verified peer is unchanged: no
  `:peer-addr`, not throttled, and in the rate limiter's anonymous bucket
  unless an interceptor identifies it.

- **A credential refused over WebSocket says so in the body** (server and
  client; A2A §12.6 "Handle Challenges", §3.3.2). The `-32600` the
  WebSocket binding answers now carries a `google.rpc.ErrorInfo` from
  `a2a-rust.com`: `UNAUTHENTICATED` with the challenge an HTTP binding sends
  as `WWW-Authenticate` in its `www-authenticate` metadata, or
  `PERMISSION_DENIED` (binding specification §6). It had no `data`, so
  nothing told it from a malformed request, and the challenge was dropped.
  The client reports the two as `ClientError::UnexpectedStatus` with status
  `401` and `403`, on a unary call and as a stream's first item, so
  `BearerAuthInterceptor` drops a refused token over WebSocket as it does
  over HTTP and gRPC; they were `ClientError::Protocol(InvalidRequest)`
  (see the upgrading guide).

- **The handler's own request checks name the field at fault** (server;
  §3.3.2 SHOULD, audit register). An empty or over-long id, a message with
  no role or no parts, metadata that is not an object or too large, a part's
  `raw` that is not base64, a push config's required ids, `url`, scheme,
  token or credentials, `ListTasks`' `pageSize` and `statusTimestampAfter`,
  and a bad REST query parameter now answer `InvalidParams` with a
  `google.rpc.BadRequest` detail naming the field. They named it in the
  message only. They are the new `ServerError::InvalidField { field, message }`,
  which answers with the same code, status, message and metric label as
  `InvalidParams`; code matching `ServerError::InvalidParams` on these errors
  must also match `InvalidField` (see the upgrading guide).

- **A `google.protobuf.Struct` error detail is written as ProtoJSON writes
  an `Any`** (server; §3.3.2, §9.5, §11.6, audit register R19). An error's
  `data` went into its details array with its fields beside `@type`
  (`{"@type":"…Struct","streamLagged":6}`), which a ProtoJSON decoder of
  `google.protobuf.Any` rejects: `Struct` is a special-cased well-known type,
  whose `Any` carries its JSON under `value`. It is now
  `{"@type":"…Struct","value":{"streamLagged":6}}`, and a non-object `data`
  is the `Struct` `{"value": …}`, the detail the gRPC binding already sent.
  This SDK's clients read both shapes. A client that read the fields beside
  `@type` (a2a-go's `errordetails.Typed` does) reads them under `value`.

- **The REST client labels its requests `application/a2a+json`** (client;
  §11.1, audit register R50). A request body is sent as
  `application/a2a+json`, a GET or DELETE carries no `Content-Type`, and a
  non-streaming call sends `Accept: application/a2a+json, application/json`.
  Every request was labelled `application/json`, bodiless ones included.
  a2a-java 1.3.0.Final and older refuse any other request type; see the
  upgrading guide.

- **SLIMRPC: a multicast member's lag report is the typed `streamLagged`
  error** (binding; audit register R66). The multicast client built it as a
  `Transport` error carrying only text, so `ClientError::is_stream_lagged`
  missed it. The stream still ends there. A unicast SLIMRPC stream's lag
  still arrives as the server's error text without the marker: the SLIMRPC
  specification defines no error details.

- **SLIMRPC: a caller's forwarding headers are dropped** (binding; audit
  register R41). `x-forwarded-for`, `x-real-ip` and `forwarded` reached the
  handler, where with `trusted_proxy_hops > 0` the rate limiter and the
  auth-failure throttle took the caller's own value as its address. No proxy
  appends to them on a SLIM session.

- **A JSON-RPC notification is run and gets no reply** (server; JSON-RPC
  2.0 §4.1 under A2A §9, audit register R21). A request with no `id` member
  was run and answered with `"id": null`. It is now answered with
  `204 No Content` over HTTP; a batch's array leaves notifications out (and
  is `204` when every member is one); WebSocket sends no frame. A streaming
  method sent as a notification is refused rather than run, as inside a
  batch. A notification refused on its headers gets no answer either.
  `"id": null` is a call and is still answered. Six of the seven new HTTP
  tests and the WebSocket test failed before the change; the seventh guards
  an id-less object that is not a valid Request, still Invalid Request.

- **A task snapshot is held to the state-transition rules a status update
  is** (server; audit register R36(b)). An executor's `Task` snapshot was
  stored without `can_transition_to`, so one moving a `Working` task back to
  `Submitted` was stored as written. The background processor now marks the
  task failed for such a snapshot and refuses one leaving a terminal state,
  as it does for a status update; the sync collector answers
  `InvalidStateTransition`. A snapshot that keeps the task's state is stored
  as before.

- **An appended artifact chunk with no parts is refused** (server; a2a.proto
  `Artifact.parts`, approved in the audit register). The event queue refused
  an artifact update with no parts unless it was an append, and the
  processors exempted appends too, so an empty chunk for an artifact id the
  task did not hold was stored as a zero-part artifact. All three paths now
  apply the proto's rule to every chunk; each new test failed before the
  change. The upgrading guide says what an executor that ended a stream
  with an empty last chunk does instead.

- **`GetExtendedAgentCard` answers `ExtendedAgentCardNotConfiguredError`
  when the card declares the capability and no extended card is set**
  (server; §3.1.11, §13.3, audit register R68). It served the public card in
  the extended card's place, and the ledger marked §13.3's MUST TESTED on the
  one other path that returned `-32007`. A handler with no card at all
  (`without_agent_card()`) and no extended card answers
  `UnsupportedOperationError` where it answered `-32007` (§13.3, approved in
  the register). The examples, the TCK SUT and the ITK agent that declare the
  capability now pass their public card to `with_extended_agent_card`, which
  keeps what they serve unchanged. The upgrading guide says how to do the same.

- **The event queue refuses an executor's event the gRPC binding cannot
  carry** (server; spec §4: every binding MUST represent the same thing). A
  `raw` part that is not base64, an integer beyond ±2^53 in metadata or a
  `data` part, or a `metadata` that is not a JSON object, written by an
  executor, was stored and served over JSON, and the task then answered
  `INTERNAL` over gRPC. The write now returns `InvalidParams` naming the
  field, as a client's message already did, and a `raw` that is base64 is
  kept in standard padded form.
- **The event queue refuses an executor's event missing a REQUIRED field**
  (server; a2a.proto). A message with no `messageId`, role or parts, an
  artifact with no `artifactId`, or a status in `TASK_STATE_UNSPECIFIED`,
  written as an event or inside a status update or task snapshot, was stored
  and served as written; over gRPC the last reads as a task with no state.
  The write now returns `InvalidParams` naming the field. A client's message
  was already held to the same fields. The conformance harness grades
  artifacts the queue refused, so `artifacts_have_ids` still fails an
  executor that writes one without an id.
- **Push-config operations need a push sender, and so does a card that
  declares push** (server; spec §3.3.4). Without a sender, Create answered
  `PushNotificationNotSupported` while Get, List and Delete still ran
  against the push-config store, so a client was told `TaskNotFound`, `[]`
  or success; all four now answer `PushNotificationNotSupported`. And `RequestHandlerBuilder::build`
  refuses an agent card that declares `capabilities.pushNotifications`
  with no `with_push_sender`, since every push-config operation would
  refuse what the card advertises; wire a sender or declare it false.
- **ListTasks follows a2a.proto's `ListTasksRequest` and
  `ListTasksResponse`** (server, every task store). `statusTimestampAfter`
  includes its bound ("greater than or equal to"); every store excluded
  it. `totalSize` is the number of tasks the filters match before
  pagination; the SQL stores reported the page's length, and the in-memory
  store every task in the partition, whatever the filters. And the SQL
  stores' `pageSize` is the page size used, as the in-memory store's
  already was, not the number of tasks on the page. The official Python
  SDK (a2a-sdk 1.2.2) does all three the same way. The SQL stores now run
  a `COUNT(*)` over the filters on each call.
- **SLIMRPC refuses a call that declares no `A2A-Version`**
  (`a2a-protocol-slimrpc`; spec §3.6.2: an empty value MUST be read as
  0.3). The server served such a call as 1.0, because the official
  `a2a-slimrpc` crate sent no version when this was decided, against its
  0.2.4. Its `A2AClient` sends one now (`a2a-client-lf` 0.2.8 adds it to
  every call's parameters, and `a2a-slimrpc` 0.3.2 forwards them as
  metadata), so the refusal the HTTP and gRPC bindings already make by
  default is now SLIMRPC's default too: `VersionNotSupportedError`.
  `SlimRpcServer::with_require_version(false)` restores the old behaviour.
  This crate's unicast client sends the version, as it has since the
  workspace's 0.10.0 release; its multicast client (`SlimRpcMulticast`)
  sent none, which the old default hid, and now adds it the same way.
- **On HTTP+JSON the path's id is the request's, and an empty one is
  refused where it stands** (server and client; spec §11.3; under
  `google.api.http` a field bound from the path is not read from the
  body). `RestDispatcher` and the axum router let a body's `id` or
  `taskId` replace the path's, so `POST /tasks/A:cancel` with `{"id": "B"}`
  cancelled B, which a proxy authorizing by path would not have allowed.
  The axum router also never read the body of `:cancel` or `:subscribe`,
  so a malformed one was served and its `metadata` and `tenant` were
  dropped. And both dropped empty path segments before routing, so
  `GET /tasks//pushNotificationConfigs` was `GetTask` for a task named
  `pushNotificationConfigs`. Now the path's value is set over the body's,
  both routers read those bodies (a malformed one is `400`), an empty
  segment reaches its handler as an empty id and is `InvalidParams`, and
  one trailing `/` is still tolerated. The REST client refuses an empty or
  whitespace-only path id with `InvalidParams` before building the URL;
  `get_task` with `""` sent `GET /tasks/` and failed to parse the task
  list it got back.
- **A header sent as several lines is read as one field** (server; spec
  §9.2 and §11.2: service parameters are HTTP headers; RFC 9110 §5.3). The
  JSON-RPC, HTTP+JSON, axum, gRPC and WebSocket bindings each kept one line
  of a repeated field, the last. `A2A-Extensions` sent as two lines,
  `urn:a` then `urn:b`, activated `urn:b` only: a card requiring `urn:a`
  answered `ExtensionSupportRequiredError`, and the JSON-RPC response
  echoed `urn:a`, read from the first line, as activated. Every binding
  now joins the lines in order with `", "`, as the SLIMRPC binding already
  did, and `cookie` lines with `"; "` (RFC 9113 §8.2.3); the echo reads the
  same value. A field that takes one value and arrives on two lines, such
  as `Authorization` or a tenant header, reaches interceptors and executors
  as both values joined rather than as the last line, so a bearer token
  sent on two lines is refused.
- **`StaticAgentCardHandler::new` refuses a card a server may not publish**
  (server; proto `REQUIRED` fields, spec §5.7). `build()`, hot reload and
  the dynamic card handler run `AgentCard::validate` and
  `validate_security_schemes`; this public constructor did not, so a card
  handed to it directly was served with, say, no skills or an API key
  scheme without a name. It now returns `InvalidParams`, as `build()` does.
- **Timestamps are read as RFC 3339, as gRPC reads them, 1969 included**
  (types and server; spec §5.6.1: ISO 8601 date-times; §5.1: the bindings
  MUST represent the same thing). `parse_iso8601_to_unix_millis` trimmed
  whitespace, took unpadded or signed fields and an offset without minutes,
  rolled `2026-02-30` into March, and refused every instant before 1970.
  An executor's status timestamp in one of those shapes went out as written
  on JSON-RPC and HTTP+JSON, while the gRPC conversion, which parses with
  the `time` crate's RFC 3339 parser, answered `INTERNAL`. A
  `statusTimestampAfter` before 1970 was `InvalidParams`. The parser now
  reads exactly what the gRPC conversion reads, which a test checks over
  1,655,808 strings: fixed-width fields, a real date, an offset with
  minutes, a fraction with digits, `t`/`z` in either case, second 60 only
  as a month's last, and years 0000 to 9999 in UTC.
  `unix_millis_to_iso8601` writes a pre-1970 instant as itself rather than
  as the epoch. A status timestamp is kept as written only in the canonical
  shape (`T`, `Z`, no offset); any other accepted form is written as the
  same instant canonically.
- **JSON-RPC follows three more JSON-RPC 2.0 rules** (server and types;
  spec §9.3: requests MUST follow JSON-RPC 2.0), over HTTP and WebSocket.
  An `id` that is not a string, a number or `null` (§4) is now an Invalid
  Request (`-32600`, null id); it was run and echoed.
  `JsonRpcRequestId`'s `Deserialize` refuses it. An omitted `params` (§4.2:
  it "MAY be omitted") is read as `{}`, so `ListTasks` without one lists,
  and a method with a required field names it; it was "missing params".
  `SendStreamingMessage` or `SubscribeToTask` inside a batch is
  `UnsupportedOperationError` (`-32004`); it was `-32602`, though the params
  were valid. Notifications are still answered.
- **A request's `Content-Type` is read as a media type** (server; spec
  §9.1 and §11.1 name `application/json` and `application/a2a+json`; RFC
  9110 §8.3.1 makes the type and subtype case-insensitive). The JSON-RPC,
  HTTP+JSON and axum bindings compared the raw value by prefix,
  case-sensitively. `Application/JSON` was refused with
  `ContentTypeNotSupportedError`, and a type that only begins with one of
  the two, such as `application/json-rpc` or `application/json-patch+json`,
  was accepted. Now the type and subtype before any `;` parameter must be
  one of the two, in any case. A client that sends `application/json-rpc`
  is refused where it was served (a2a-java 1.3.0.Final's REST server also
  matches by prefix, and would accept it).
- **A message's raw parts must be base64, and its numbers exact in
  protobuf** (server; spec §4: every binding MUST represent the same
  data). A part's `raw` is protobuf `bytes`, and the numbers in metadata
  and in a `data` part are protobuf doubles, which the gRPC conversion
  refuses beyond ±2^53 rather than round. The JSON bindings stored any
  `raw` string and any integer, and the task's history holds the message,
  so one such send made the task unreadable over gRPC: `GetTask`, every
  `ListTasks` page holding it and `SubscribeToTask` answered `INTERNAL`.
  Such a send is `InvalidParams` now, naming the field. A `raw` in
  URL-safe or unpadded base64 is still accepted, as gRPC reads it, and is
  kept in standard padded form, so every binding returns the same string.
  `base64` becomes an unconditional dependency of the server crate (it was
  already in the build through the SQL, JWT and OTLP features).
- **An agent card needs a description and a version** (types; `a2a.proto`
  marks `AgentCard.description` and `AgentCard.version` REQUIRED).
  `AgentCard::validate`, which `RequestHandlerBuilder::build`, hot reload
  and the dynamic card handler run, refused an empty name but not an empty
  description or version, and `AgentCard::new` leaves the description
  empty, so a card built with it was published with `"description": ""`,
  which over gRPC is the REQUIRED field absent. Both are refused now: add
  `with_description` to a card built with `new`. The book's and READMEs'
  snippets do, and the ones compiled but never run also gained the skill
  they lacked, without which `build()` refused them already.
- **ListTasks refuses a `pageSize` outside 1 to 100** (server; a2a.proto:
  `page_size`'s "minimum value is 1" and "maximum value is 100"; spec
  §3.3.2 and §13.4: servers MUST validate all input parameters). A larger
  size was clamped to 1000 and served, and 0 read as the default of 50;
  both are `InvalidParams` now, on every binding, as the spec's own
  validation example answers `pageSize=150` with a 400. Omitting `pageSize`
  still lists 50. A message's `referenceTaskIds` are checked as its
  `taskId` is (non-empty, not only whitespace, at most `max_id_length`);
  they were stored unchecked.
- **The dynamic agent card handler sends no `Last-Modified`** (server; spec
  §8.6.1 makes the header optional). `DynamicAgentCardHandler` stamped it
  with the current second on every response, which told clients the card
  changed at every request, and answered `If-Modified-Since` with `304` only
  within that same second. A card produced per request has no modification
  date (RFC 9110 §8.8.2.1), so the handler now omits the header and ignores
  `If-Modified-Since` (§13.1.3). Revalidate with `If-None-Match`: the `ETag`,
  a hash of the card, is unchanged. The static handler still sends
  `Last-Modified`, the time it loaded the card.
- **A stream that falls behind catches up from the task's event log**
  (server; spec §3.5.2: each stream MUST receive the same events in the same
  order). A reader slower than the agent lost the events the bounded
  broadcast queue overwrote, and its stream ended with `streamLagged`. When
  the task store keeps an event log, as every built-in store does, the reader
  now reads what it missed from the log, after the last position it
  delivered, and goes on. This is the reader every binding streams
  `SendStreamingMessage` and `SubscribeToTask` from. A subscriber attaching
  mid-task resumes from where it attached, not from the start of the log.
  Only a complete replay is used, with no position missing (a failed
  append leaves the log without one), and a live event that does not
  follow it is reported as a lag. When the log cannot supply every missed event the
  stream ends with `streamLagged`, as before: a store without a log, a lag
  beyond what the log still holds (the in-memory stores keep the last 512
  events of a task by default; the SQL stores keep the whole log). More
  missed events than one read of the log returns (`subscribe_replay_limit`)
  are read a page at a time (see the next item). A `read` dropped while the log is read, as the SSE and gRPC
  bindings drop one whenever a keep-alive or a closed client wins the race,
  leaves the replay owed to the next `read`.
- **A subscription across turns receives every turn's events** (server;
  spec §3.5.2: each stream MUST receive the same events in the same order).
  A `SubscribeToTask` stream on a task parked at `input-required` waits for
  the next turn's queue, polling every `subscribe_reattach_interval`. What
  the turn broadcast before the poll found its queue never reached the
  subscriber, and a turn that ran from start to end between two polls
  reached it as one terminal status built from the stored task, or, if it
  parked the task again, not at all; no `streamLagged` said so. When the
  store keeps an event log the stream now replays those events from the
  log, with their positions, before it goes on. So does a subscription on
  another replica sharing the store, which used to get only the ending: the
  book's "Running More Than One Replica" says what it sees now. Without a
  log each turn numbers its events from 1, and a subscriber skipped as many
  of a turn's events as it had seen positions in the turns before; it now
  receives them, and reports with `streamLagged` the ones broadcast before
  it found the turn's queue. A subscription to a task with no live queue
  starts at the log's end, so the next turn does not replay the history its
  snapshot holds.
- **Breaking: `ClientBuilder::from_card` follows the card's interface order**
  (client; spec §8.3.2: a client MUST select the first supported interface,
  preferring earlier entries). It ranked by `ClientConfig::default()`'s
  `preferred_bindings`, `["JSONRPC"]`, so a card listing `HTTP+JSON` before
  `JSONRPC` got JSON-RPC. It now applies no preference of its own: the card's
  first interface for protocol 1.x, with `build()` falling back down the list
  past one it cannot construct. `A2aClient::from_card` follows.
  `from_card_preferring` still applies a caller's explicit ranking, and
  `ClientConfig::preferred_bindings` is documented as the record of the
  ranking applied, which it always was. See the upgrading guide.
- **Breaking: only `A2A-Version: 1.0` is served** (server; spec §3.6.2:
  requests are processed with the requested `Major.Minor`'s semantics, and an
  unsupported version MUST be `VersionNotSupportedError`). Any `1.x` was
  accepted and served as 1.0; now `1.1`, a bare `1`, `v1.0` and `+1.0` are
  refused with a message naming `1.0`, on JSON-RPC, HTTP+JSON, gRPC,
  WebSocket and `validate_version_metadata` alike. A patch number is still
  ignored (§3.6). The WebSocket handshake's own copy of the check now calls
  the shared validator.
- **Breaking for clients built from a card: a call the card does not
  declare the capability for is refused before it is sent** (client; spec
  §3.3.4: clients SHOULD validate capability support against the card before
  an operation that needs it). `stream_message`, `subscribe_to_task` and
  `subscribe_to_task_from`, the four push-config methods, a send or stream
  carrying an inline push config, and `get_extended_agent_card` return
  `ClientError::CapabilityNotDeclared` without sending anything unless the
  card sets `capabilities.streaming`, `pushNotifications` or
  `extendedAgentCard` (respectively) to `true`. A conformant agent refuses
  those calls too, with `UnsupportedOperationError` or
  `PushNotificationNotSupportedError`, so against one what changes is where
  the refusal happens and which error carries it.
  `ClientBuilder::without_capability_checks` sends them anyway; a client built
  with `ClientBuilder::new` has no card and is unaffected. See the upgrading
  guide.
- **A rate limit refused over JSON-RPC or WebSocket is retried** (client;
  spec §3.3.2). Such a refusal is an `InternalError` whose details carry a
  `google.rpc.RetryInfo` delay; the client treated it as permanent, while the
  same refusal over HTTP+JSON (`429`) or gRPC (`RESOURCE_EXHAUSTED`) was
  retried. It is now `is_retryable()`, `retry_after()` returns the delay, and
  a client with a `RetryPolicy` waits that long and re-sends, `SendMessage`
  included: the request was refused before it was processed. Without a
  policy nothing is re-sent. A delegating executor that passes such an error
  through with `?` now records it as `Transient`.
- **Push webhook timeouts are 10 s per request and 30 s per delivery**
  (server; spec §4.3.3 and §13.2 recommend 10-30 s). `HttpPushSender`'s
  request timeout was 30 s and `HandlerLimits::push_delivery_timeout`, which
  bounds a whole delivery with its retries, was 5 s, so one attempt of the
  default three ever ran. Now every attempt starts inside the bound and the
  first two finish. With the bound equal to `push_delivery_budget` (30 s), an
  event's deliveries all run at once rather than 17 at a time; every config
  was attempted before and is now, and the event still finishes inside the
  budget. Both values remain configurable.
- **Breaking: `build()` requires an agent card** (server; spec §8.1: an A2A
  server MUST make an agent card available). `RequestHandlerBuilder` let a
  handler build without one, which then answered discovery with 404 and,
  with nothing declared, skipped every capability, input-mode and
  required-extension check (§3.3.4). `build()` now refuses that, naming the
  two ways out: `with_agent_card(card)`, or `without_agent_card()` for a card
  published elsewhere or a handler that is not a public A2A server (a test,
  for instance), which behaves as before. The README quick start and the
  book's examples now set a card. See the upgrading guide.
- **Breaking: an authenticating interceptor isolates tasks by caller**
  (server; spec §13.1: results scoped to the caller's authorization
  boundaries). Callers in one tenant shared its tasks unless the builder
  called `isolate_tasks_by_caller`, so with labelled tokens, labelled keys or
  JWT, Bob could read, list, cancel and subscribe to Alice's tasks and
  register webhooks on them. Now each caller an authenticating interceptor
  identifies gets a partition of its own, where another caller's task is
  `TaskNotFound`. A request it lets through without an identity (an
  unlabelled token or key) keeps the tenant's shared records, as before (a JWT
  without `sub` is refused, below); `isolate_tasks_by_caller` still refuses one. Unset
  stores default to the tenant-aware in-memory ones: up to 1,000 partitions,
  each with the whole `with_task_store_config` capacity (10,000 tasks by
  default), so that store's bound is per caller now. `build()` now refuses, naming the remedies, a supplied task or
  push-config store that does not partition by tenant, and an approval gate
  that needs a second approver, including the one `Profile::Auditable`
  installs: under isolation that approver could never reach the requester's
  task. `share_tasks_across_callers()` restores the old behaviour. A handler
  with no authenticating interceptor is unaffected. See the upgrading guide.
- **Breaking for the `otel` feature: OpenTelemetry 0.33** (server, client,
  sdk). `opentelemetry`, `opentelemetry_sdk`, `opentelemetry-otlp`,
  `opentelemetry-http`, `opentelemetry-appender-tracing` 0.32 to 0.33.1 and
  `tracing-opentelemetry` 0.33 to 0.34.0, together: the four Dependabot
  updates could not land one at a time. Types from these crates appear in
  the server's API (`Telemetry::tracer_provider`, `meter_provider`,
  `logger_provider`, `TelemetryError::Exporter`, whose source
  `ExporterBuildError` now has only `InvalidConfiguration` and
  `InternalFailure`), and the client reads the current span's context
  through `tracing-opentelemetry` 0.34, so its `traceparent` names the
  caller's span only under a 0.34 layer. An application must move to the
  same versions.
  `opentelemetry-otlp` 0.33 retries a failed export by default (four
  attempts, exponential backoff with jitter), so `Telemetry` export does too.
- **Every push webhook is attempted** (server; spec §4.3.3). Deliveries ran
  one after another and `push_delivery_budget` skipped whatever it did not
  reach: at the defaults a slow webhook estate reached 6 of 100 configs per
  event. Now every config is attempted, and the budget sets how many
  deliveries of an event run at once (17 at the defaults), events in order
  per webhook. `push_outcome::SKIPPED` is no longer reported.
  `PushRetryPolicy::with_max_attempts(0)` is taken as 1.
- **Breaking: an agent card the spec forbids is not published** (types,
  server; spec §5.7). `AgentCard::validate` now refuses empty
  `defaultInputModes`, `defaultOutputModes` or `skills`, and a skill with no
  `tags`; `RequestHandlerBuilder::build` runs it (it checked only the
  interface list), a `DynamicAgentCardHandler` answers `500` for an invalid
  produced card, and `HotReloadAgentCardHandler::reload_from_json` refuses one
  and keeps the current card. `AgentCard::new` now starts both mode lists at
  `["text/plain"]`; its docs no longer claim the bare card is valid. The
  `hello-agent`, `langfuse-agent` and `swarm` examples, the SDK README and
  two book pages published skill-less cards and now declare a skill. See the
  upgrading guide.
- **`AgentCard::validate` refuses empty required strings** (types; proto
  `REQUIRED`): an interface's `url`, `protocolBinding` or
  `protocolVersion`; the provider's `organization` or `url`; a skill's
  `id`, `name` or `description`; a signature's `protected` or `signature`.
  Over gRPC an empty string is the absent value. Security-scheme fields are
  checked by `AgentCard::validate_security_schemes` (the const `validate`
  cannot walk the scheme map): an API key's `name`, an HTTP scheme's
  `scheme`, an OpenID Connect `openIdConnectUrl`, and the URLs of the
  authorization-code, client-credentials and device-code flows.
  `RequestHandlerBuilder::build`, `HotReloadAgentCardHandler` and
  `DynamicAgentCardHandler` run both.
- **An executor's events are bound to its task** (server; proto
  `REQUIRED` `taskId`/`contextId`). A status, artifact or task event written
  with an empty task or context id gets the running task's; one naming
  another task or context is refused with `InvalidParams` before it is
  stored or sent. The server used to pass whatever the executor wrote.
- **An empty task id is `InvalidParams`** (server; proto `REQUIRED`).
  GetTask, CancelTask and SubscribeToTask answered `TaskNotFound` for an
  empty or whitespace-only id (gRPC's absent string, or JSON `""`).
- **An artifact with no parts is refused at the event queue** (server;
  proto: an artifact must contain at least one part). Persistence dropped
  it, but live subscribers had already been sent it. The executor's write
  now returns `InvalidParams`. An appending chunk may still be empty.
- **The WebSocket binding no longer routes `message/stream`** (server; spec
  §5.1). It was the one v0.3 method name still served there, and every other
  binding refuses it; it is now `-32601` like the rest. The binding has a
  written specification at `book/src/bindings/websocket.md`.
- **A stream ends at the terminal state** (server; spec §3.1.2, §3.1.6).
  Streams ended when the executor returned; one that kept running, or kept
  writing, after its terminal status held every subscriber's stream open.
  Readers now end once the terminal event is delivered; events an executor
  writes after it are not streamed.
- **One JSON-RPC error shape on every surface** (server; spec §9.5, §12.4).
  HTTP responses, SSE frames and WebSocket messages now build the error the
  same way: the A2A code, and `data` as the typed-details array. On WebSocket
  this is new: unary errors carried no `data`, mid-stream errors were all
  rewritten to `-32000` (losing the code and the `streamLagged` marker), and
  the over-capacity answer was `-32000` where the HTTP binding sends an
  overload as `-32603`. Unary JSON-RPC and HTTP+JSON responses now keep an
  error's own `data` as a detail. Invalid params answer with a
  `google.rpc.BadRequest` naming the field on JSON-RPC (HTTP and WebSocket)
  and gRPC, as HTTP+JSON already did.
- **The extended agent card is cacheable** (server; spec §13.3). `GET
  /extendedAgentCard` on `RestDispatcher` and the axum router sends
  `Cache-Control: private, no-cache`, `Vary: Authorization` and an `ETag`, and
  answers `304` to a matching `If-None-Match`.
- **A request body of the wrong shape names the field** (server; spec §11.6).
  The HTTP+JSON 400 now carries a `google.rpc.BadRequest` detail whose field
  violation is the path, for example `message.parts[0].text`. Malformed JSON is
  still a plain 400. New dependency: `serde_path_to_error` (MIT OR
  Apache-2.0), already in the lockfile through axum.
- **Empty push-config ids are InvalidParams** (server). Get, list and delete
  refused an absent id at parse but passed an empty one (gRPC's absent
  string) to the store, where list and delete answered success.
- **A push config with an authentication scheme other than Bearer or Basic
  is refused** (server; spec §4.3.3, §13.2). The sender can write only those
  two, and any other scheme was stored and then delivered without
  credentials. Creating one now answers InvalidParams naming the scheme.
- **A direct `Message` reply names its context** (server; spec §3.4.1). When
  the executor answers with a `Message` and leaves `contextId` empty, the
  reply now carries the task's `contextId`, on the blocking path and on a
  message-only stream. A `contextId` the executor set is kept.
- **Executor status timestamps are UTC** (server; spec §5.6.1). A timestamp
  with an offset other than `Z` is rewritten to the same instant in UTC, at
  millisecond precision; one that does not parse as ISO 8601 makes the write
  fail, as an oversized event does. Both used to go out unchanged on the
  JSON bindings, while gRPC refused the malformed one at encoding.
- **A push config with an empty or scheme-less `url` is refused** (server;
  the proto marks `url` REQUIRED). The host check that refused it runs only
  when private URLs are refused, so a sender that allows them stored an
  empty URL and listed it back. `TaskPushNotificationConfig::validate` now
  runs on every create, answering InvalidParams.
- **SLIMRPC status codes follow the SLIMRPC specification's table**
  (`a2a-protocol-slimrpc`; `spec/slimrpc_v1/slimrpc.md` §6.1, a MUST). The
  binding claimed to copy the gRPC binding's mapping and matched neither:
  `JSONParseError` is now `INTERNAL` (was `INVALID_ARGUMENT`),
  `ContentTypeNotSupportedError` `UNIMPLEMENTED` (was `INVALID_ARGUMENT`), and
  `VersionNotSupportedError`, which the table does not list, takes A2A §5.4's
  `FAILED_PRECONDITION` (was `UNIMPLEMENTED`). A rate-limit refusal is
  `RESOURCE_EXHAUSTED`, as an overload already was. The error type name in
  the message, which is what this SDK's client reads, is unchanged.
- **SLIMRPC metadata keys are case-insensitive** (`a2a-protocol-slimrpc`;
  SLIMRPC spec §3). The server passed keys through in the case the peer sent,
  and the handler looks them up in lowercase, so `Authorization`,
  `A2A-Extensions` or `A2A-Version` from a peer that capitalised them were
  ignored, the last one skipping the version check. Keys are lowercased now;
  two that differ only in case are joined with a comma.
- **A rate-limit refusal says it is temporary, and when to retry** (server,
  types; spec §13.4, §3.3.2). `RateLimitInterceptor` refused with a plain
  internal error, which reached callers as HTTP `500` / gRPC `INTERNAL`,
  indistinguishable from a server fault. It now refuses with the new
  `A2aError::rate_limited(message, retry_after)`, retrying when the window
  ends: HTTP+JSON answers `429` with status `RESOURCE_EXHAUSTED` and a
  `Retry-After` header, gRPC `RESOURCE_EXHAUSTED`, and every binding carries a
  `google.rpc.RetryInfo` detail. The JSON-RPC code stays `-32603`, the code
  §3.3.2 gives as its example for "rate limit exceeded". `A2aError::retry_after`
  reads the delay; it is whole seconds, so `A2aError` stays 96 bytes.
- **An overload's HTTP+JSON error body names `RESOURCE_EXHAUSTED`** (server).
  It answered `503` with `"status":"INTERNAL"`, naming a permanent failure
  under a status that says to retry; it now names the code the gRPC binding
  sends for the same error.
- **A mid-stream JSON-RPC error's `data` is an array of typed details**
  (server; spec §9.5). The SSE error frame copied `A2aError::data` through
  as an object, so the stream-lag signal went out as
  `"data":{"streamLagged":N}`, where §9.5 requires an array whose objects
  each carry `@type`. It now carries the same details as the REST frame:
  `ErrorInfo` when the error has an A2A reason, then the data as a flat
  `google.protobuf.Struct` detail
  (`[{"@type":"type.googleapis.com/google.protobuf.Struct","streamLagged":N}]`).
  `A2aError::is_stream_lagged` and `dropped_event_count` find the marker in
  either shape, so clients of this release read old and new servers alike;
  a client of an earlier release reading a server of this one does not see
  the lag marker and gets an ordinary internal error, which still ends its
  stream. Code that reads `data["streamLagged"]` by hand should switch to
  the predicates.
- **Listing or deleting push configs of an unknown task is `TaskNotFound`**
  (server; spec §3.1.9, §3.1.10). Listing answered an empty list and
  deleting succeeded, which a peer cannot tell from a known task with no
  webhooks. REST answers 404, JSON-RPC -32001, gRPC `NOT_FOUND`, as the
  reference SDK does. Deleting a config that is already gone on a task that
  exists still succeeds: the operation is idempotent.
- **A non-append artifact update for an id the task already holds replaces
  that artifact** (server). The proto says an artifact id "must be unique
  within a task", and the task stored a second artifact with the same id.
  Both the blocking and background paths replace in place now, as the
  reference SDK does.
- **`ROLE_UNSPECIFIED` is refused with `InvalidParams`** (server). The JSON
  bindings already refused a message without a role; over gRPC an absent
  role decodes as `ROLE_UNSPECIFIED` and was accepted.

- **Agent Card signatures canonicalize with protobuf field presence** (types,
  `signing` feature; spec §8.4.1 rule 1). A field that holds its default
  value is now left out of the signed bytes unless it is `REQUIRED` or
  declared `optional`: an empty `capabilities.extensions`, an extension's
  `required: false` or empty `description`, an interface's empty `tenant`,
  empty `securityRequirements`. Before, they were signed, so a card carrying
  one canonicalized differently from the specification's own worked example
  and its signature failed in any implementation that follows §8.4.1, and the
  reverse. A signature made by an earlier release over such a card no longer
  verifies here; re-sign it. `itk/interop/card_signature_interop.py` signs
  and verifies six cards both ways against the official Python SDK: four
  agree, and the two that do not are the SDK's own departures from §8.4.1
  (`_clean_empty` drops `REQUIRED` empty fields, and drops a security
  requirement whose scope list is empty, so its signature does not cover
  that requirement). Against the previous release the same run fails the
  card with defaults, which is the case this fixes.
- **Card signatures carry `typ: "JOSE"`** in the protected header (§8.4.2
  SHOULD).
- **A streaming send whose agent answers with a `Message` streams that one
  `Message` and closes** (server; spec §3.1.2). Every streaming send used to
  lead with a `Task` snapshot, so a direct-reply agent's stream was
  `[Task, Message]`, which is neither of the two shapes §3.1.2 allows. The
  executor's first event now decides: a `Message` makes it the message-only
  stream, anything else (or no event) the task-lifecycle stream that begins
  with the `Task`, as before. An agent that wants a task stream and
  narrates first must emit a status update before its first `Message`.
- **The axum adapter enforces `A2A-Version` and the request media type**,
  as `RestDispatcher` does: an absent version (0.3) or an unsupported one is
  `VersionNotSupportedError`, and a body in a media type other than
  `application/json` or `application/a2a+json` is
  `ContentTypeNotSupportedError` (§3.6.2, §11.1). Discovery, `/health` and
  `/ready` stay versionless. A client that called the axum router without
  the header must now send it.

- **The executor's span is `invoke_agent {card name}`** (server, `tracing`
  feature), the OpenTelemetry GenAI conventions' in-process agent span, with
  `gen_ai.operation.name = invoke_agent`, the agent's name, description and
  version, the context as `gen_ai.conversation.id`, and the task's final
  `a2a.task.state`; `a2a.task.id` and `a2a.context.id` stay. It was
  `a2a.execute`. Backends key on it — Langfuse shows the run as an AGENT
  observation and groups a context's runs into a session. A dashboard or
  alert matching the span name `a2a.execute` must match the new name, or the
  handler can keep the old span with
  `RequestHandlerBuilder::with_agent_span_conventions(false)` — which an
  application whose agent framework reports its own `invoke_agent` should do
  anyway, so the two do not nest.
- **With `otel` and a recording `tracing-opentelemetry` layer, the client
  sends a `traceparent` on every call**, naming its own span, over the
  JSON-RPC, HTTP+JSON and gRPC transports it builds. Before, it sent one only
  with `TracePropagationInterceptor` in a `CurrentTrace` scope; the server
  span a call causes is now the child of the client's span rather than of
  the caller's. Opt out with `ClientBuilder::with_trace_propagation(false)`.
- **HTTP+JSON responses negotiate their media type** (server; both
  `RestDispatcher` and the axum adapter). A client whose `Accept` prefers
  `application/a2a+json`, or whose request body is labelled with it and whose
  `Accept` is absent or a wildcard, now receives `application/a2a+json`, as
  spec §11.1 says SHOULD be used; every other client still receives
  `application/json`. Successes and errors are labelled alike; the agent card
  and `/health` and `/ready` are unchanged. A client that sends an A2A body
  and parses responses only under `application/json` must send
  `Accept: application/json`. Released a2a-go (v2.6.0) already does and is
  unaffected (`go_sdk_interop.sh`, every check passing). ACTS `REST-CT-001`,
  the only failure on earlier runs, now passes: HTTP+JSON 94/94 against
  a2a-itk `82458cea` (`acts/reports/2026-10-08/`).

### Fixed

- **A client call is recorded as it ended for the caller** (client,
  `tracing`/`otel`). Three calls were recorded wrongly. A call dropped
  while waiting for its agent (a `tokio::time::timeout`, a `select!`) ran
  nothing after its `.await`: its span ended with no status and
  `rpc.client.call.duration` held no point, so the calls callers gave up on
  were the ones the telemetry could not see. A stream dropped before its
  final event or the end of its body was recorded as a success. And a call
  ended at the transport, before the client had accepted the answer: an
  interceptor's `after` hook that refused it, a result that did not decode,
  a response `received` found malformed, a content signature that did not
  verify were all recorded as successes. The first two now end their span
  with `error.type` `cancelled` and `ERROR` status and record their
  duration with that `error.type`, as the server records a peer that goes
  away; the third ends with the refusal's class (`serialization`,
  `invalid_response`, the new `content_not_verified`, or the hook's own
  error). A stream read to its end is unchanged. The interceptors' error
  hook still runs only for a failure of the transport, and not for a dropped
  call (a destructor cannot await it). A call refused before anything is
  sent (an undeclared capability, a message that is not well formed, a
  `before` hook that fails) opens no span, as before: nothing was called.
  Internally the nine unary methods' send, `after` hooks and decoding are
  one `send_decoded`, and the span, clock and metric are one `InFlight`
  (`call_span/in_flight.rs`). Tests: `call_metric/tests.rs`
  `a_call_the_caller_drops_is_recorded_as_cancelled`,
  `a_stream_dropped_before_it_ended_is_recorded_as_cancelled`,
  `a_response_an_interceptor_refuses_is_recorded_as_failed`,
  `a_response_that_does_not_decode_is_recorded_as_failed` and
  `a_stream_an_interceptor_refuses_is_recorded_as_failed`, and the span's
  own in `call_span/in_flight/tests.rs`. The first two were written failing
  first (no point recorded; a success recorded); the others were checked by
  four mutations (ending before validation, no failure for a refused
  stream, a no-op `Drop`, no `content_not_verified` class), each caught.

- **A WebSocket client's spans and `rpc.client.call.duration` carry
  `server.address` and `server.port`** (client). A transport given to
  `with_custom_transport`, as `WebSocketTransport` is, recorded neither, so
  a dashboard could not tell its agents apart; found when the client
  dashboard's failure-ratio panel showed a series with no agent. The new
  defaulted `Transport::server_url` names the URL; `WebSocketTransport`
  returns its endpoint (`ws`/`wss` default to ports 80 and 443), and
  `build` and `build_grpc` both read it. Tests:
  `a_custom_transport_names_its_system_and_keeps_the_agent`,
  `a_custom_transport_given_to_build_grpc_names_its_system_and_address`
  (the first coverage of `build_grpc`'s custom-transport branch) and
  `ws_transport_names_its_rpc_system_jsonrpc`.

- **gRPC: a call whose answer cannot be converted to protobuf is recorded
  as failed** (server, `grpc`). SendMessage, GetTask, ListTasks,
  CancelTask, GetExtendedAgentCard and a directly answered
  SendStreamingMessage converted the handler's answer after the call's
  span had ended, so the caller got `INTERNAL` while the span and
  `rpc.server.call.duration` recorded `OK`. The conversion now runs inside
  the call. Test: `dispatch/grpc/native.rs`
  `an_answer_the_wire_cannot_carry_is_recorded_as_internal` (fails with the
  conversion moved back after the span).

- **SLIMRPC: `A2aClient::list_push_configs` works** (`a2a-protocol-slimrpc`;
  spec §12.1). `SlimRpcTransport` answered ListTaskPushNotificationConfigs
  with a bare list where the typed client expects the
  `ListPushConfigsResponse` page, so every call failed with "invalid type:
  map, expected a sequence" and `nextPageToken` was lost. Tests:
  `tests/operations.rs` `push_configs_round_trip_over_the_fabric`,
  `tests/equivalence.rs` `one_task_reads_the_same_through_slimrpc_and_jsonrpc`.

- **SLIMRPC: retryable errors carry the A2A error type name**
  (`a2a-protocol-slimrpc`; SLIMRPC §6.2). `RESOURCE_EXHAUSTED` (overload,
  rate limit) and `UNAVAILABLE` (halt, store temporarily unavailable) went
  out with a bare message; they now carry the `InternalError: ` prefix
  §6.2 requires on every error. Status codes are unchanged. Test:
  `src/error_tests.rs` `retryable_errors_carry_the_a2a_type_name`.

- **SLIMRPC: a call whose answer cannot be converted to protobuf is
  recorded as failed** (`a2a-protocol-slimrpc`). The unary methods
  converted the handler's answer after the call's span ended, so the
  caller got `INTERNAL` while the span and `rpc.server.call.duration`
  recorded `OK`. The conversion now runs inside the span. Test:
  `src/server/methods/tests.rs` `an_answer_the_wire_cannot_carry_fails_its_span`.

- **The WebSocket binding page states the service-parameter rules the
  dispatcher applies** (book; spec §12.3). It said a value "not valid UTF-8
  is dropped" and listed only `:path`. Any header line that is not visible
  ASCII is left out, valid UTF-8 such as `café` included; a header's lines
  are joined with `", "` (`"; "` for `Cookie`); `Last-Event-ID` is dropped;
  the reserved names are `:path`, `:peer-addr`, `:peer-certificate`,
  `:upgrade-traceparent` and `:upgrade-tracestate`. Test:
  `websocket_binding_spec.rs`
  `the_service_parameters_are_the_visible_ascii_upgrade_headers_and_the_reserved_entries`.

- **The control map no longer says a server interceptor can inspect or
  filter content** (docs). `ServerInterceptor`'s hooks are given the
  `CallContext` only (method, caller, tenant, task id, headers), never the
  message. AIA-5-1 and AIA-5-1-ba-bb say a server interceptor can refuse a
  call from its context and that content reaches only the executor;
  ASI-MODEL and AIA-15-5 say a content filter runs in a client interceptor,
  which is given each request's params and response's result, or in the
  executor.

- **Tests only: security tests that did not test what they were cited for** (server,
  client). `auth_secret_redaction.rs` (the API-key and bearer interceptors'
  `Debug`, including a derived-`Debug` byte rendering, and every event and
  span they emit while accepting and refusing); `push_log_redaction.rs
  the_token_and_credentials_are_never_logged` (a config with a token and
  `Bearer` credentials, events and span fields); `push_sender_tests
  a_webhook_that_never_answers_is_abandoned_at_the_timeout`;
  `tls_protocol_versions.rs` (hand-written TLS 1.0/1.1 `ClientHello`s get a
  fatal `protocol_version` alert from `Server::with_tls_pem`,
  `WebSocketDispatcher::with_tls_pem` and `GrpcDispatcher::with_tls`, TLS
  1.2 gets a `ServerHello` with an ECDHE AEAD suite, and the push sender
  offers only TLS 1.3/1.2 and forward-secret AEAD suites);
  `grpc_tls_apply_tests a_server_certificate_from_an_untrusted_ca_is_refused`
  (client, with a pinned-CA control); `audit_security_events.rs` (an
  authorization denial and a malformed request recorded with the caller);
  `a2a_json_security_considerations.rs` (malformed content refused on
  JSON-RPC and HTTP+JSON with the executor never run; another caller's task
  answers as a missing one on both).

- **Metadata on an appended artifact chunk is kept in SQLite and
  PostgreSQL** (server). Both event processors merge an appended chunk's
  `metadata` into the stored artifact and wrote the change as
  `ArtifactDelta::AppendedParts`, which the SQLite and PostgreSQL stores
  (and `EncryptedTaskStore` over them) apply by writing the new parts
  alone, so `GetTask` returned the first chunk's metadata there and the
  merged metadata in memory. An append that changes the metadata is now
  written with `save`; one that does not is still a delta.
  `tests/artifact_append_metadata.rs` fails without the fix on all three.

- **A continuation saved on one replica could hide another replica's event
  from a subscriber** (server, spec §3.5.2). A continuation's save replaces
  the stored artifacts and metadata with the ones its send read; when
  another replica's turn folded an event into the task after that read, the
  save dropped it and kept the log position that fold had stored, so the
  store said the task reflected an event it no longer held. A subscription
  on a replica with no live queue starts from the stored task and replays
  the log after that position, and the event was in neither: missing from
  its stream, with no `streamLagged`. The save is now built from the task
  the queue lease reads to number the turn, and records that read's
  position, so the subscriber replays the event. The task still loses it
  (a lost update, open: only a compare-and-set write would refuse the stale
  save), and the continuing turn's first fold stores a position past it
  again. Reproduced on the in-memory store shared by two handlers, on two
  `SQLite` pools and on two `PostgreSQL` pools on one database;
  `tests/stale_write_position`.

- **The axum `A2aRouter` did not warn about an undeclared binding** (server,
  spec §5.2). Every other listener warns when it starts if the static card
  declares no interface for the binding it serves; `A2aRouter::into_router`
  now does the same for HTTP+JSON. Found by the ledger audit of 2026-10-20.

- **A `wss://` interface was warned as plaintext** (server, the card lint
  `build()` runs). The check behind "is not https; production deployments
  MUST use encrypted communication" accepted only `https://`, so a
  production WebSocket interface on `wss://` drew the warning; `wss://` is
  now encrypted too, and `ws://` still warns. Found by the ledger audit of
  2026-10-20; `agent_card::lint::tests::a_wss_interface_is_encrypted_and_a_ws_one_is_not`.

- **A blocking request outlasting the idle timeout lost its answer**
  (server, `Server` / `serve`). `ServeConfig::idle_timeout` (75s by
  default) measured silence on the socket, and a blocking `SendMessage`
  is silent while its executor works, for up to `DEFAULT_EXECUTOR_TIMEOUT`
  (an hour): the connection was closed at 75s, the client saw a reset, and
  the task ran on. A request is now counted from the moment the service
  is called until its response is ready, and the timer gives no verdict
  while one is; the window restarts when it is answered. A peer stalled
  mid-body is still bounded by `DispatchConfig::body_read_timeout`.
  WebSocket (Ping at half the budget) and gRPC (tonic's own server) were
  not affected. Found 2026-10-20 driving a live model through the tool
  loop; `serve::graceful::tests::a_request_still_being_served_is_not_idle`.

- **A `message/stream` call is recorded as `_OTHER`** (server). The span and
  metric name mapping still treated the v0.3 spelling as
  `SendStreamingMessage`, though no binding routes it (WebSocket stopped on
  2026-10-12; JSON-RPC never did), so a refused legacy call was counted
  under a method it never reached and without `rpc.method_original`.
- **Requests refused before their span opens are in
  `rpc.server.call.duration`** (server). gRPC (a missing or unsupported
  `A2A-Version`, metadata or a request that does not convert), HTTP+JSON and
  the axum router (version, content type, path traversal, an oversized query,
  a path no route serves) answered these without recording them, while the
  JSON-RPC and WebSocket bindings did. Each is now one point with the status
  it was answered: on gRPC under its method, on HTTP+JSON with no
  `rpc.method`.
- **A failed or timed-out executor's `invoke_agent` span says so** (server).
  The GenAI conventions require `error.type` "if the operation ended in an
  error"; the span never set it. A run that ends `Failed` — the executor's
  own status, or the one written for an executor that returned `Err` or ran
  past its timeout — now sets `error.type` to the failure class
  (`internal`, `invalid-request`, `transient`, `policy-refusal`,
  `budget-exhausted`; a timeout is `budget-exhausted`) and the span's status
  to `Error` explicitly. The status was already `Error` only by way of the
  `executor failed` ERROR event and `tracing-opentelemetry`'s default of
  turning error events into span status.
- **The Go SDK interop's secured-card leg enforces the key it publishes**
  (`examples/echo-agent`, `itk/interop/go-sdk-client`). The handler build
  now refuses a card whose `securityRequirements` no interceptor checks, so
  echo-agent's `A2A_INTEROP_CARD=1` mode stopped starting and leg 1b of
  `scripts/go_sdk_interop.sh` failed. In that mode echo-agent now registers
  an `ApiKeyAuthInterceptor` on the card's `X-API-Key` header, with
  tenant-aware stores for the caller isolation that implies. The a2a-go
  client presents the key through a2a-go's own `AuthInterceptor`, which
  reads the scheme from our card, and first checks that a call without the
  key is refused.
- **The agent card's path answers every method other than `GET` and `HEAD`
  with `405` and `Allow: GET, HEAD`, on every listener** (server). RFC 9110
  §15.5.6 says a `405` MUST carry `Allow`. Measured before the fix, a
  `POST`, `PUT`, `DELETE` or `PATCH` of `/.well-known/agent-card.json` got
  the card (`200`) from the JSON-RPC dispatcher, `404` from REST and gRPC,
  and no response at all from WebSocket, which handed it to the WebSocket
  handshake. Only the axum router answered `405`, and only when the request
  carried `A2A-Version`. `StaticAgentCardHandler::handle` and
  `DynamicAgentCardHandler::handle` now answer `405` themselves, and the
  dynamic handler no longer produces a card for such a request. A matching
  `If-None-Match` does not turn the answer into `412`: RFC 9110 §13.2.1 has
  a server ignore preconditions when the answer without them would not be
  `2xx`. `tests/agent_card_methods.rs` probes all five listeners.
- **`EncryptedTaskStore` seals what a write stores, and the seal budget is
  enforced** (server; release audit F7). Every write sealed every part of
  the task, so an artifact streamed in 100 chunks cost 5,250 seals
  (measured), against a per-key budget of 2^32 random-nonce seals that was
  counted and never enforced. An artifact or status delta now goes first to
  the wrapped store's new `try_save_artifact_delta` or
  `try_save_status_delta`. Only the parts that write stores are sealed:
  the appended tail or pushed artifact, and the status message. Every other
  part is a placeholder that never opens, so a store that wrote it anyway
  fails the read rather than returning anything. Only when the store cannot
  apply the delta in place is the whole task sealed. The same 100 chunks now
  cost 100 seals on the in-memory, tenant-aware, SQLite and PostgreSQL
  stores. `Aes256GcmCipher::with_seal_limit` (default `DEFAULT_SEAL_LIMIT`,
  2^32) refuses a seal past the limit, failing the write, and warns at half
  and at nine tenths. The design notes' alternative, reusing ciphertext
  keyed by a digest of each part, was rejected on measurement: HMAC-SHA256
  ran at 0.20–0.37 GB/s here against 1.6–3.3 GB/s for AES-256-GCM, so it
  would have cost more than re-sealing. The tenant-aware in-memory store now
  forwards both delta writes to its partition, where it used to fall back to
  a whole-record `save`. `scripts/check_store_delta_forwarding.py`, a new CI
  gate, fails when a store overrides a delta write without its `try_` half.

- **The SQL stores' artifact appends are compare-and-append** (server;
  release audit F5). A replica whose snapshot predated another's write
  appended after parts or artifacts it had not seen. Under
  `EncryptedTaskStore`, whose parts are sealed to their positions, the
  record then failed to decrypt ("a stored value could not be opened",
  reproduced on PostgreSQL before this fix). PostgreSQL's part append now
  matches only when the stored artifact at that index has the expected id
  and part count, and its push only when the array has the expected length.
  SQLite's push gets the same length check. SQLite's journalled part append
  was already safe for a stale append of the same artifact, because its
  journal records positions, but it appended to whatever artifact had
  replaced it at that index. It now checks the artifact id. A mismatch
  falls back to a whole-record `save`: a lost update at worst, never a
  misplaced part. `tests/artifact_compare_and_append.rs` runs each case on
  PostgreSQL, SQLite and the in-memory store, which needed no change.

- **`CancelTask` no longer erases what another replica wrote meanwhile**
  (server; release audit F6). Cancel re-read the task, set `Canceled` and
  wrote the whole snapshot back with `save`, so a history message or artifact
  part appended between its read and its write was lost. It now writes the
  status alone with `save_status_delta`. Every shipped store applies that in
  place and still refuses to move a terminal task, so a task that finished
  meanwhile is still reported not cancelable. A new test forces the
  interleaving on the in-memory, SQLite and PostgreSQL stores; before the
  fix it fails on all three.

- **A send dropped mid-commit leaves no task behind** (server; release audit
  F9). A new task's row is saved before its inline push config is
  registered, and a client that went away during that registration left the
  row `Submitted` for good. No executor ran it, the retention sweeps delete
  only terminal tasks, and `GetTask` and `ListTasks` showed it. Its released
  idempotency key also let the retry create a second task for one send. The
  send's `CommitGuard` now deletes such a row, along with the push configs
  registered for it. It holds the per-context lock until the rollback is
  done, so a concurrent send for the same context waits instead of finding
  the doomed task. The approval's audit record is written before the save,
  so a continuation has nothing awaited between its save and its executor.
  Found on the way: the guard's release ran in a spawned task outside the
  request's tenant scope, so a dropped keyed send under a tenant released
  its key in the empty tenant's partition and kept it held in its own. The
  release now runs in the captured tenant and caller scope.

- **Task history is bounded in bytes** (server; release audit F2). It was
  capped at 1,024 messages and nothing else, so at the 4 MiB request body one
  task could hold 4 GiB of remote input. Every place a history grows now
  applies `TASK_HISTORY_LIMIT`: 1,024 messages and `MAX_TASK_HISTORY_BYTES`,
  16 MiB of serialized messages, oldest first, always keeping the newest.
  Those places are a send's turn appended by the store, and an agent's
  messages appended by the background processor and the blocking-send
  collector. The four copies of the count-only trim are now one,
  `HistoryLimit::trim`. That function measures nothing when a cheap upper
  bound shows the history fits, so a long conversation's turn costs a median
  of 10 µs, not 108 µs (measured; see its docs). Two defects were fixed on
  the way:
  - A refused agent-message save at the cap left the message in the
    in-memory task; it now restores exactly what the trim dropped.
  - No test reached the blocking-send collector's trim; one does now.

- **The in-memory idempotency index is bounded by count, not only by age**
  (server; release audit F8). Its 24-hour TTL let a client add one key per
  send for a day. `TaskStoreConfig::max_idempotency_keys` (default 100,000,
  about 230 MB at worst with the default id limit) caps it. A new key past
  the cap first expires old keys, then is refused with `503` /
  `RESOURCE_EXHAUSTED`, and the send runs nothing. A held key still replays,
  and no key is ever evicted to make room. The end-to-end test found that
  the send path rewrote any temporary store refusal at the claim to
  `INTERNAL`, which tells a client not to retry. The claim is the send's
  first side effect, so such a refusal is now `Overloaded`.

- **docs.rs shows which feature each item needs** (all four crates; release
  audit F27). Each manifest asked docs.rs for `--cfg docsrs`, and nothing
  used it. Each `lib.rs` now enables `doc_cfg` under it, so every
  feature-gated item carries an "Available on crate feature …" badge. The
  book's `/api/` build moves to nightly, as docs.rs uses, since stable
  refuses the attribute. The nightly CI job builds the same way and checks
  that a badge renders.

- **A PostgreSQL-only build no longer compiles SQLite** (server; release
  audit F25). The workspace's `sqlx` entry enabled its `sqlite` driver for
  every crate, so `--features postgres` linked `libsqlite3-sys`; each store
  feature now adds its own driver. `scripts/check_sql_driver_isolation.sh`,
  a new CI step, checks both directions against a control.

- **Push delivery with `max_attempts` set to 0** (server; release audit
  F19). The field is public, and the backoff guard computed
  `max_attempts - 1`: a panic in a debug build, and in release a backoff
  sleep after the only attempt. The loop already made one attempt; the guard
  now uses the same count. Test:
  `push_sender_tests::a_zero_attempt_policy_makes_one_attempt_and_no_backoff`,
  which fails with "attempt to subtract with overflow" on the old guard.
- **A stored push config with empty credentials sends no `Authorization`**
  (server), rather than `Bearer ` with nothing after it; for configs stored
  before creation refused them. Test:
  `push_sender_tests::empty_credentials_send_no_authorization_header`.
- **A task answered at `AUTH_REQUIRED` by a blocking send now records the
  rest of its run** (server; spec §7.6.1 "The agent MAY immediately continue
  Task processing after receiving the credential"). Once a blocking
  `SendMessage` answered at an interrupted state, nothing read the task's
  event queue: every event the executor emitted afterwards failed with "event
  queue: no active receivers", and the stored task stayed at `AUTH_REQUIRED`
  (or `INPUT_REQUIRED`) for good, so `GetTask` never saw it finish. Only a
  client holding a stream saw the rest. The collection now answers at the
  interrupted state and goes on storing the executor's events until the task
  ends or the queue closes. Found by the `follow_task` end-to-end test.
  Regression test: `auth_required_in_task::a_blocking_send_answered_at_auth_required_still_records_the_rest`.
- **An unauthenticated push-config call is refused for its credential, not
  told the agent lacks push** (server; spec §3.3.2). `Get`, `List` and
  `DeleteTaskPushNotificationConfig` checked the card's push capability
  before authenticating, so a caller with no credential got
  `PushNotificationNotSupported` where `CreateTaskPushNotificationConfig`
  gave `401`; a paged list also decoded its page token first. Found by
  `tests/auth_every_method.rs`, which failed on it.
- **The gRPC client keeps a rate-limit refusal's retry delay** (client).
  `RESOURCE_EXHAUSTED` carrying `google.rpc.RetryInfo` (which this SDK's
  server always attaches) became a 429 with no `retry_after`, so a gRPC
  client retried on its own backoff while the HTTP ones waited as asked.
  Clamped to the hour `Retry-After` is.
- **The client refuses a message with an empty `messageId` or no parts
  before sending it** (client; a2a.proto REQUIRED fields, spec §5.7), with
  `InvalidParams`, instead of spending a round trip on the server's refusal.
- **`with_connection_timeout`'s documentation said it covered DNS**
  (client). It bounds the TCP connect only, after name resolution, divided
  across the resolved addresses (hyper-util 0.1.21's `HttpConnector`). Its
  behaviour is now tested (`tests/connection_timeout.rs`), where before only
  its default and setter were.
- **A tenant partition no longer reserves room for its whole capacity on
  creation** (server; release audit F4). Each partition of
  `TenantAwareInMemoryTaskStore` pre-allocated its map for `max_capacity`
  tasks (10,000 at the default), and a partition is created for every
  tenant id a request names, so a run of junk tenant ids reserved that much
  per id before storing anything. A partition now starts at 256 and grows.
  `run_eviction_all` also sweeps a snapshot of the partitions instead of
  holding the tenant map's lock across every sweep (F24).
- **A subscription on another replica no longer loses the event being
  stored** (server; spec §3.5.2, "Each stream MUST receive the same events
  in the same order"; audit register R69). A `SubscribeToTask` on a replica
  with no live queue read the log's end and then the task. A processor on
  the replica running the task logs each event before it stores the state
  the event leads to, so an event logged before the first read and stored
  after the second was in neither the snapshot nor the replay, and the
  subscriber never saw it, with no `streamLagged`; one logged and stored
  between the reads arrived twice. The subscription now reads the task with
  the log position it reflects, in one store read
  (`TaskStore::get_with_position`), and replays from there. Every shipped
  store keeps that position; a custom store that does not falls back to the
  old order. During a rolling upgrade, a replica still on an earlier release
  does not write the position, so a subscriber can receive events its
  snapshot already holds twice until every replica runs this release.

- **A turn after a failed log append no longer reuses that append's
  position** (server; spec §3.5.2). The processor carries on past a failed
  append and stores the event's effect, so the stored task can reflect a
  position the log does not hold. The next turn was numbered from the log's
  end alone, so its first event took that position, and a subscriber that had
  seen the failed event live, or started from the stored task, dropped it as
  delivered. A turn is now numbered after the later of the log's end and the
  stored position; the skipped position is a hole a replay reports as
  `streamLagged`. Stores that report no position keep the old numbering.

- **The WebSocket and gRPC listeners serve the agent card** (server; spec
  §8.1, "A2A Servers **MUST** make an Agent Card available", §8.2). A
  deployment on either alone published none; both now answer `GET` and
  `HEAD` of `/.well-known/agent-card.json` as the HTTP dispatchers do
  (`ETag`, conditional requests, `max-age=3600`), and `404` without a card.
  The WebSocket listener reads the request line without consuming it and
  hands every other connection to the handshake unchanged. **A gRPC
  dispatcher with a card now accepts HTTP/1.1 connections** (tonic's
  `accept_http1`), because a card fetch is usually HTTP/1.1; gRPC is
  HTTP/2 either way. `GrpcDispatcher::into_service` is unchanged and
  serves no card.
- **A v0.3 agent card is read with its legacy fields** (client; spec App.
  A.2, A.2.2). A card fetched from its well-known path, or returned by
  `GetExtendedAgentCard`, that names its address with v0.3's top-level
  `url`, `preferredTransport` and `additionalInterfaces` has them folded
  into `supportedInterfaces` (with the card's `protocolVersion`, or `0.3`),
  and `supportsAuthenticatedExtendedCard` or `supportsExtendedAgentCard` is
  read as `capabilities.extendedAgentCard`. A field the v1.0 form sets wins.
  Each legacy field found is named in a `tracing` warning. Such a card used
  to come back with no interfaces and no extended card; interface
  selection still passes over a 0.3 interface for a 1.x one.
- **A database that cannot answer for now is a temporary failure** (server,
  `a2a-protocol-slimrpc`; spec §3.3.2: servers SHOULD tell temporary failures
  from permanent ones). Every SQL store answered every database error as
  internal (HTTP `500`, gRPC `INTERNAL`), so a pool with no free connection, a
  busy `SQLite` file, or a lost or restarting Postgres told a client not to
  retry a request that would have succeeded a moment later. Those answer HTTP
  `503` with status `UNAVAILABLE` and gRPC `UNAVAILABLE` now; other database
  errors, and every error on a send or a push config's creation, stay
  `INTERNAL`. This SDK's client re-sends those two calls on `503` as refused
  before any work began, and a store failure can come after the executor ran.
  The SLIMRPC binding answers a halted handler `UNAVAILABLE` too, as gRPC
  already did; it answered `INTERNAL`.
- **A signed agent card is verified as received** (client, types;
  `signing` feature; spec §8.4.3). The signer signs every field it serves,
  and the client verified the card after reading it into an `AgentCard`,
  which drops any field this SDK does not model (one a later minor version
  adds, say). So a correctly signed card from such a peer was refused by a
  fetch with `CardFetchOptions::with_signature_keys`. The fetch now
  verifies the card's JSON first. New in `signing`:
  `verify_card_json_with_jwks` and `canonicalize_card_json`, for a card
  fetched another way; `verify_card_with_jwks` still verifies a card as
  this SDK models it.
- **The axum router's own refusals are AIP-193** (server, `axum` feature;
  spec §11.6). Refusals the router made before any A2A handler ran went out
  as axum wrote them: a method a route does not take as a bodiless `405`, a
  body over the cap as plain-text `413`, one not received in time as
  plain-text `408`. They now carry an AIP-193 body (`error.code` and
  `error.message`, with `error.status` where the code has a name), as
  `RestDispatcher`'s do; the statuses and the `405`'s `Allow` header are
  unchanged. An unknown path outside the A2A routes is left to the app the
  router is merged into.
- **A `Last-Event-ID` on a WebSocket upgrade no longer replays the event
  log** (server, `websocket` feature). The WebSocket binding has no
  resumption: a resubscribe starts from the task's snapshot. But the
  upgrade request's headers reach every call on the connection, so one
  carrying `Last-Event-ID` made every `SubscribeToTask` on it replay the log
  from that position. The header is now dropped at the handshake.
- **A refused credential names its challenge on gRPC and SLIMRPC** (server,
  `a2a-protocol-slimrpc`; spec §3.3.2: servers SHOULD include the
  authentication challenge and specify the scheme). The HTTP bindings send
  `WWW-Authenticate` with their `401`; the gRPC dispatcher now sends the
  same challenge as `www-authenticate` metadata on its `UNAUTHENTICATED`
  status, and the SLIMRPC server, whose errors carry no metadata, ends the
  message with `(WWW-Authenticate: <challenge>)`. Both dropped it.
- **SLIMRPC multicast: an agent's error is its outcome**
  (`a2a-protocol-slimrpc`). SLIM names the member an error came from with
  its full four-part SLIM name; `SlimRpcMulticast::send_message` filed the
  error under that name while it pairs outcomes by the three-part agent
  address, so an agent that answered with an error was reported as a
  timeout. `stream_message` dropped every error frame, so a refusing agent's
  stream ended with no events, as `IncompleteStream`. Now the first reports
  the agent's own error, and the second ends that agent's stream with it.
- **`RestDispatcher` serves an id containing `..`** (server; spec §11.3
  routes). Its path-traversal guard refused any path containing `..`, raw
  or percent-encoded, so a task or push config id such as `v1..prod`,
  which JSON-RPC, gRPC and the axum router serve, answered 400 on every
  HTTP+JSON route. It now refuses a dot segment (`.` or `..` as a whole
  segment, RFC 3986 §3.3) at any encoding depth, with `\` read as a
  separator as well as `/`, and still refuses a path that has not finished
  decoding after eight passes.
- **An HTTP+JSON response names what chose its media type** (server; spec
  §11.1 negotiation; RFC 9110 §12.5.5). `RestDispatcher` and the axum
  router pick `application/a2a+json` or `application/json` from the
  request's `Accept`, else its `Content-Type`, and sent no `Vary`, so a
  shared cache could serve one client's representation to a client that
  asked for the other. Negotiated responses now carry `Vary: Accept,
  Content-Type`, beside the extended card's `Vary: Authorization`, and an
  `Accept` sent as two lines is read as one list; only its first line was
  read.
- **An agent card's explicitly empty `iconUrl` or `documentationUrl`
  survives gRPC** (types; spec §8.4.1: an optional field explicitly set to
  its default MUST be included). Both are `optional string` in a2a.proto,
  and the gRPC conversion mapped `""` to absent in both directions, so a
  card read over gRPC was not the card served over JSON, and a signature
  covering the empty field no longer verified. The conversion keeps it.
- **The agent card answers `HEAD`, and its conditional requests follow RFC
  9110** (server; spec §8.6.1: cache validation on the card endpoint).
  JSON-RPC and HTTP+JSON served `/.well-known/agent-card.json` (and the
  HTTP+JSON probes `/health`, `/ready`) for `GET` only, and the axum router
  exempted only `GET` from the `A2A-Version` check, so a `HEAD` was refused
  (RFC 9110 §9.3.2: answer it as a `GET`, without the body). In the static
  and dynamic card handlers, a `304` answered any method, not only `GET`
  and `HEAD` (§13.1.2); only the first `If-None-Match` line was read
  (§5.3); and in the static handler an `If-None-Match` that was not visible
  ASCII fell through to `If-Modified-Since`, which MUST then be ignored
  (§13.1.3).
- **A response, stream event or OAuth flow set with a field it does not
  know still parses** (types; spec §5.7: implementations SHOULD ignore
  unrecognized fields). `SendMessageResponse`, `StreamResponse` and
  `OAuthFlows` are protobuf `oneof`s and derived serde's externally tagged
  encoding, which reads one key and nothing else: `{"task": …, "x": 1}`,
  an event with a field from a later 1.x version, or a card whose
  `oauth2SecurityScheme.flows` held one, failed to parse. They now skip
  unknown keys and still refuse no member, or two. Error text for those
  cases changes, from serde's "unknown variant" to "a StreamResponse sets
  exactly one of task, message, statusUpdate, artifactUpdate; this one sets
  none". The hand-written visitor costs a little: timed interleaved with
  the derived one in a release build, a stream event's parse takes
  1.08-1.10x as long (32-57 ns more on medians of 419-608 ns).
- **`CachingCardResolver` takes the validators a `304` sends** (client;
  spec §8.6.2: clients SHOULD honour RFC 9111). RFC 9111 §4.3.4 has a
  cache that freshens a stored response from a `304` replace its stored
  header fields with the `304`'s. The resolver kept the first `200`'s
  `ETag` and `Last-Modified`, so after a `304` carrying a new `ETag`, or
  a `Last-Modified` the `200` lacked, it revalidated with the old ones. It
  now takes those the `304` carries and keeps the rest, and a `304` marked
  `no-store` leaves nothing stored, as a `200` so marked already did.
- **A security scheme naming two schemes is refused** (types; a2a.proto's
  `SecurityScheme` is a `oneof`). Its deserializer took the first arm it
  found, so `{"apiKeySecurityScheme": …, "httpAuthSecurityScheme": …}`
  became an API key scheme and the other was dropped without a word, where
  a `Part` with two content members, and `OAuthFlows` with two flows, are
  refused. It now fails to parse, whichever spelling each arm takes.
- **A push config a header cannot carry is refused when it is created**
  (server; spec §13.2: a webhook request carries the config's
  credentials). A token or credentials holding CR, LF or another control
  character was stored, inline or by `CreateTaskPushNotificationConfig`,
  and listed back, and then every delivery was refused before it was sent
  (the sender refuses CR/LF; the HTTP client any other control character).
  Creation now answers `InvalidParams` naming the field.
- **An HTTP+JSON body that never arrives is answered `408`** (server; RFC
  9110 §15.5.9; spec §5.1: the bindings MUST behave the same). Every
  failure to read a request body was answered `413 Content Too Large`, so a
  client whose upload stalled was told its body was too big, while the
  axum router answered the same request `408`. `RestDispatcher` now answers
  `413` for a body over `max_request_body_size`, `408` for one not received
  within `body_read_timeout`, and `400` for a body stream it cannot read
  (a malformed chunk, say). The JSON-RPC binding still answers a body it
  could not read with `-32700`.
- **A JSON-RPC request refused for a header is answered with its id**
  (server; spec §9.3: requests follow JSON-RPC 2.0, whose response `id`
  "MUST be the same as the value of the id member in the Request Object").
  An unsupported `A2A-Version` or `Content-Type` was refused before the
  body was read: the refusal answered `"id": null` and a batch got one
  object instead of an array, so a client that pairs responses with
  requests by id could not pair the `VersionNotSupportedError` a 0.3 client
  most needs to see. The refusal now waits for the body and carries each
  request's id, one error per request of a batch. A batch beyond
  `max_batch_size`, a body that is not JSON or is over the size limit, and
  a request without an id are refused once with a null id, as before.
- **A card's `ETag` is the same for the same card** (server; spec §8.6.1:
  an `ETag` "derived from ... a hash of the card content"). It hashed the
  card's serialization, and `securitySchemes`, a requirement's `schemes`
  and an OAuth flow's `scopes` are `HashMap`s, written in an order each
  map draws at random. A card with two or more entries in one of them had
  another `ETag` on each replica and after each restart, and a
  `DynamicAgentCardHandler` whose producer builds the card per request
  gave another on every request, so revalidation returned the whole card
  instead of a `304`. The static and dynamic handlers and the extended
  card (`GetExtendedAgentCard` over HTTP+JSON) now hash the card with its
  maps' keys sorted. The body is unchanged.
- **The agent card fetch sends `A2A-Version`** (client; spec §3.6.1:
  "Clients MUST send the `A2A-Version` header with each request"). Every
  transport sent it, but `resolve_agent_card`, `fetch_card_from_url` and
  the caching resolver fetched the card without it. They send `1.0` now,
  unless the caller's `CardFetchOptions` carry an `A2A-Version` of their
  own, which is sent instead.
- **`A2aError::is_stream_lagged` says what a lag does to the stream**
  (types; documentation only). It said the stream goes on after the lag
  and a consumer that keeps reading still gets every later event, including
  the terminal status. This SDK's servers send the lag only when the task's
  event log cannot replay what the consumer missed, and on the JSON-RPC,
  HTTP+JSON, WebSocket and gRPC bindings it is the stream's last frame. The
  doc now says so, and that the way back is to subscribe again (over SSE
  with `Last-Event-ID`). The SLIMRPC binding sends the error's text without
  the marker, so the check is `false` there.
- **A gRPC error carries its data, and a stream's error its marker**
  (server and client; spec §5.1: every binding MUST handle errors the same
  way). The JSON bindings send an error's `data` beside its `ErrorInfo`: a
  `google.rpc.BadRequest` as itself, anything else as a
  `google.protobuf.Struct` detail. The gRPC binding dropped it, so a gRPC
  client lost a bad request's field violations, and a stream's error was
  `INTERNAL` "event queue error" whatever it was: a lagged stream could not
  be told from an executor failure. The server now adds the same details to
  the status, and answers a stream's error as a unary call's; the gRPC
  client decodes a `Struct` detail into `A2aError::data`, as its REST
  transport does, so `is_stream_lagged` holds over gRPC, and a
  `BadRequest` into the typed detail the JSON bindings send. The types
  crate's `proto` module re-exports `prost_types`.
- **A resume across a hole in the event log starts from the snapshot**
  (server; spec §3.5.2: each stream MUST receive the same events in the
  same order). A failed append leaves the log without that position, and
  the processor goes on. A `SubscribeToTask` resumed with `Last-Event-ID`
  replayed the events on either side of the hole, skipping the lost one
  without saying so. It now starts from the task snapshot, as it does when
  the log no longer reaches back to the client's position.
- **A continuation sent the moment a task parks keeps its events and the
  task's artifacts** (server; spec §3.5.2: each stream MUST receive the same
  events in the same order; §3.3.3: multi-turn interactions). A stream
  delivers `input-required` as it is broadcast, before the background
  processor has logged and stored it, and a continuation was admitted as
  soon as the turn's executor returned. Its queue numbers on from the
  task's event log, which was read while the previous turn was still being
  logged, so its positions collided with that turn's; appends are
  idempotent by position, and every colliding event of the continuation
  was dropped from the log, which resumption and catch-up read. The
  continuation was also built from the task as read before the previous
  turn had stored it, and saving it removed what that turn had just
  stored, such as an artifact. A turn's queue is now released once its
  events are stored, bounded by the queue's write timeout, and a send that
  waited for a parked turn reads the task again. This holds within one
  process: a continuation sent to another replica while the first is still
  storing is not covered.
- **A malformed query value is refused on the HTTP+JSON binding** (server;
  spec §13.4: agents MUST validate all input parameters; §5.1: every
  binding MUST handle errors the same way). `RestDispatcher` and
  `A2aRouter` read a GET's `historyLength`, `pageSize`, `status` or
  `includeArtifacts` that did not parse as if it were absent:
  `GET /tasks/{id}?historyLength=-1` returned the whole history and
  `GET /tasks?status=TASK_STATE_WORKNG` listed every task, where JSON-RPC
  and gRPC refuse the same values. Both now answer `400 INVALID_ARGUMENT`
  naming the parameter, from one parser: the router read ListTasks with a
  parser of its own, which took `includeArtifacts=1` as absent where the
  dispatcher took it as `true`. A boolean is `true` or `1`, `false` or `0`,
  in any case; anything else used to read as `false`.
- **A client checks calls against the extended card once it has fetched
  it** (client; spec §3.1.11, §13.3: a client SHOULD replace its cached
  public card with the extended one). A client built from a card checks a
  call's capability against that card before sending it, and kept checking
  against the public card after `get_extended_agent_card`, so a capability
  only the extended card declares, such as `pushNotifications`, was still
  refused locally with `CapabilityNotDeclared`. The fetched card's
  capabilities now replace the public card's; `extendedAgentCard` stays
  declared, since the agent has just served it. A client built without a
  card, or with `without_capability_checks`, still checks nothing.
- **Proto3 defaults on the JSON bindings mean unset** (server; spec §5.1:
  every binding MUST return semantically equivalent results for the same
  request). `ProtoJSON` prints a field without presence at its default as
  "unset", and a client that prints every field (a2a-java's JSON-RPC
  transport does) sends `""` or `TASK_STATE_UNSPECIFIED` for "none". Only a
  message's `contextId` and `taskId` were read that way. The rest were read
  as values, where gRPC's conversion already read them as unset: a part with
  `"mediaType": ""` was refused as `ContentTypeNotSupported`; a push config
  created with `"id": ""` was stored under the id `""`, which Get and Delete
  refuse, and the next such create replaced it; `"token": ""` sent an empty
  token header; and ListTasks with `"contextId": ""`, `"pageToken": ""` or
  `"status": "TASK_STATE_UNSPECIFIED"` listed nothing. Each is now unset on
  ingress: a part's `mediaType` and `filename`, a push config's `id` and
  `token` (standalone or inline in `SendMessage`), and those three ListTasks
  fields.
- **A cancelled task's streams end on `Canceled` whatever its `cancel` hook
  writes** (server; spec §3.1.2, §3.1.6: a stream MUST close when the task
  reaches a terminal state). Only the default `AgentExecutor::cancel` wrote
  `Canceled` to the task's queue. An executor that overrode the hook to
  release what the task held, as the trait's documentation invites, and
  wrote nothing, had its task stored `Canceled` by `CancelTask` while an open
  `SendStreamingMessage` stream ended after `Working` with no terminal
  frame; a halt or shutdown that ended such a task left it `Working`, its
  streams likewise without a terminal frame. After a hook that returns `Ok`
  without writing a terminal state, the handler now writes `Canceled` to the
  queue itself, where it is stored and every open stream ends on it. The
  conformance kit's `cancel_emits_terminal` check, which failed such a hook,
  is renamed `cancel_succeeds`: it passes a hook that writes nothing, and
  still fails one that errors, panics or overruns.
- **A `SubscribeToTask` stream misses nothing between its first `Task` and
  its live events** (server; spec §3.1.6: the first event represents "the
  current state of the task at the time of subscription. This prevents a
  potential loss of information"). The `Task` was read from the store before
  the stream attached to the task's queue, and the store trails the queue:
  the background processor, or a blocking send's collector, stores each
  event after it is broadcast. An event broadcast before the attach and
  stored after the read was in neither, so a subscriber could see
  `submitted` and none of the work already done. The stream now attaches
  first, at an exact position, and reads the task once every event up to
  there is stored and none is being stored; the live stream then skips what
  that `Task` already holds. The wait is bounded by
  `HandlerLimits::subscribe_replay_catchup` (2 s by default), after which
  the task's event log fills the gap. A streaming send replayed by its
  idempotency key starts the same way.
- **The WebSocket client connects to `wss://`** (client, feature
  `websocket`; spec §7.1: production deployments MUST use encrypted
  communication). `tokio-tungstenite` was built without TLS, so every
  `wss://` endpoint failed with "TLS support not compiled in", although the
  transport accepted the scheme and the book's example used it. With
  `tls-rustls` (on by default) the transport now verifies the server against
  the roots the HTTPS transports use, Mozilla's with `ring`, or a
  configuration given to the new
  `WebSocketTransportConfig::with_tls_config`. Without the feature a
  `wss://` connect fails naming it.
- **`A2aRouter` reads a GET or DELETE request's fields from its query**
  (server, feature `axum`; spec §11.5: GET and DELETE MUST carry request
  parameters as path or query parameters). The router ignored the query of
  every `/tasks/…` route: `GET /tasks/{id}?historyLength=0` returned the
  task's whole history (a2a.proto: the server MUST NOT return more messages
  than requested), and `?tenant=` was dropped on GetTask, ListTasks, the
  push-config routes and `GET :subscribe`, so they read the default
  partition. Both are now read as the REST dispatcher reads them.
- **A refused inline push config no longer deletes the task it continues**
  (server; spec §3.4.3: a client continues a task by sending a new message
  with its `taskId`). A `SendMessage` continuing a task with a
  `configuration.taskPushNotificationConfig` the server refused (a webhook
  the SSRF screen rejects, a full per-task quota, push not supported) got
  the error, and the rollback meant for a send that had just created its
  task then deleted the task being continued, with its history and
  artifacts. A continuation now registers its config before the turn touches
  the task, so a refusal leaves the task as it was. If the config is
  accepted and the turn then cannot be saved, a config the server created is
  withdrawn; one the request named by id is kept.
- **The client advertises its own `acceptedOutputModes`** (client; a2a.proto
  `SendMessageConfiguration.accepted_output_modes`: agents SHOULD use it to
  tailor their output). A request with no `configuration` of its own was
  given `SendMessageConfiguration::default()`, whose `["text/plain"]` then
  kept `ClientConfig::accepted_output_modes` from applying, so every such
  request told the agent to answer in text only, whatever the client was
  configured with. It now carries the configured modes: by default
  `text/plain` and `application/json`.
- **A task's events reach every stream in the order they were written**
  (server; spec §3.5.2: events MUST NOT be reordered). Each event gets its
  position when it is written, but two writes on one task in flight at once
  (`CancelTask`'s beside the executor's, or an executor's own concurrent
  writes) could broadcast in the other order: a terminal event waits for the
  store's verdict before it goes out, and a write behind it went out first;
  on a multi-threaded runtime any two writes could swap. Streams then
  carried positions out of order, and the lag catch-up above, which skips a
  position a stream has passed, would have lost the terminal event. Writes
  to one queue now take turns from the moment a position is assigned until
  the event is broadcast. An uncontended write still finishes without
  waiting.
- **A `SubscribeToTask` stream on a tenant- or caller-partitioned task store
  stays open across turns** (server; spec §3.1.6: the stream MUST terminate
  when the task reaches a terminal state). The hook that reattaches the
  stream to the next turn's queue reads the task store while the stream is
  read, outside the request that opened it, so it read the default
  partition, found no task, and ended the stream at the first turn boundary
  without a terminal state. It now re-enters the subscriber's tenant and
  caller. Since an authenticating interceptor now isolates callers by
  default (§13.1, above), this reached every authenticated server.
- **The site's brand fonts load.** `theme/custom.css` asked for
  `fonts/…`, which resolves to `theme/fonts/` and answered 404 on the live
  site, so Space Grotesk and JetBrains Mono never loaded; it asks for
  `../fonts/…`. Release books built before this keep the fault.
- **This project's extension and binding URIs lead to their chapters.**
  `https://a2a-rust.com/extensions/idempotency/v1`, `…/extensions/failure/v1`
  and `…/bindings/websocket/v1` answered 404; each now redirects to the
  chapter that specifies it.
- **`Telemetry` reports a lost export on either transport, and only a lost
  one** (server, feature `otel`). It counted failed HTTP responses, so over
  gRPC a lost export was only logged, and with 0.33's retries an export
  refused once and delivered on retry would have been reported lost. It now
  counts exports the exporter gave up on, after its retries, on OTLP/HTTP
  and gRPC alike (`otel/telemetry/counting.rs`); `shutdown` and
  `force_flush` name each signal with `N export(s) failed after retrying and
  were dropped`.
- **HTTP+JSON ids with reserved characters are found** (server; spec
  §11.3). This SDK's client percent-encodes path ids; `RestDispatcher`
  matched them encoded, so a push config whose id held a space, `+`, `:` or
  `/` was created and never found. It now decodes ids (and the tenant) after
  routing, with `+` literal as in any URL path. The axum router decoded the
  whole tail before splitting it, so an encoded `/` or `:` broke the route;
  it now routes on the raw path, as `RestDispatcher` does.
- **A peer's task without `contextId` parses** (types). The proto leaves it
  optional and ProtoJSON omits an empty string; the whole response failed.
- **`CachingCardResolver` does not keep a `no-store` card** (client; RFC 9111
  §5.2.2.5), nor one without a validator to revalidate it by.
- **The axum router's agent card is cacheable** (server; spec §8.6.1). It
  served the JSON alone; it now sends the same `ETag`, `Last-Modified` and
  `Cache-Control: public, max-age=3600` as the JSON-RPC and REST
  dispatchers, and answers `304` to a matching `If-None-Match`.
- **CI on this branch** (`ci.yml`, `tck.yml`, `pin-freshness.yml`):
  `setup-go` asked for `1.26`, which resolved to the runner's cached 1.26.8,
  while the Go peers' `go.mod` require 1.26.9, so the Go interop job failed
  before building. It now asks for `1.26.9`. A redundant intra-doc link in
  `RequestHandlerBuilder` failed `cargo doc -D warnings`, and the SLIMRPC
  binding's own docs had two unresolved links and one redundant one (CI does
  not build those).
- **`GetExtendedAgentCard` enforces required extensions** (§3.3.4), as
  every other operation already did.
- **A card whose OAuth flow has no scopes parses** (types). ProtoJSON, which
  every official SDK emits, leaves an empty map out even where the proto
  marks it `REQUIRED`, and the five flows' `scopes` had no
  `#[serde(default)]`, so such a card failed to parse as a whole.
- **The `A2A-Version` request parameter is honoured** (server: JSON-RPC,
  REST, WebSocket upgrade, axum). Spec §3.6.1 lets a client send
  `?A2A-Version=1.0` instead of the header; only the header was read, so such
  a client was taken to speak 0.3 and refused. A non-empty header still wins.
- **REST's `ContentTypeNotSupportedError` carries its `ErrorInfo`** (§11.6).
  It was answered as a bare 400 with no `details`.
- **Card signatures always name their key.** `sign_card_es256` and
  `sign_card_ed25519` take the key id as `&str`, so the protected header
  always has the `kid` §8.4.2 requires.

- **`verify_agent_card` accepts the key form its documentation named.** Its
  docs said `SubjectPublicKeyInfo` DER, but only the raw 65-byte point
  verified, so a key exported with `openssl pkey -pubout -outform DER` was
  refused. Both forms now verify, and the docs say so.
- **A blocking `SendMessage` could lose events.** Its collector read the
  task's broadcast queue, which overwrites what a slow reader has not
  reached. An executor writing more than the queue's capacity (256) faster
  than the collector stored it lost the overflow from the stored task,
  which still ended `Completed`: 20 blocking sends of a 600-artifact burst
  kept between 255 and 600. The collector now reads the bounded
  persistence channel the background processor already used, so a slow
  collector makes the executor wait (bounded by the write timeout) instead
  of dropping events. Measured after: 600 of 600 in 60 of 60 sends.

- **A subscription that could not replay what its snapshot lacked went on
  live and skipped those events with no `streamLagged`** (spec §3.5.2: each
  stream MUST receive the same events in the same order). When the store had
  not applied the queue's events within `subscribe_replay_catchup`, the
  `SubscribeToTask` snapshot reflected fewer positions than the live
  receiver started after, and the log was replayed to cover them; a store
  without an event log, an unreadable or holed log, and a log that no longer
  reached back each returned early instead. The stream now yields the
  snapshot and then `streamLagged` counting the positions between. A resume
  against a whole snapshot is unchanged.
- **The agent-card lint accepted `iso-kam3-*` as HTTP authentication
  schemes.** They are the algorithms of the `Mutual` scheme, in IANA's second
  table, not registered schemes; removed from the allowlist, so a card naming
  one is warned about.
- **The task store a handler builds for itself reported to no metrics**
  (server). `RequestHandlerBuilder::build` made the default (and the
  caller-isolated default) store with the no-op `Metrics`, so on a handler
  with no store of its own the one sign that an event was lost, an append
  for a task the store no longer held (`a2a.server.persistence_errors{operation=event_append,
  error=task_absent}`), reached no exporter. The store now reports to the
  handler's `Metrics`. Tests: `handler/default_store_metrics_tests.rs`
  (the default and the caller-isolated default; both fail without the fix).
- **A request the tenant rule refused was counted as received and never as
  answered** (server). Under `require_resolved_tenant()`, or a resolver that
  disagrees with the client's tenant, `SendMessage`, `SendStreamingMessage`,
  `GetTask`, `ListTasks`, `CancelTask`, `SubscribeToTask` and the four push
  configuration methods returned from resolving the tenant after counting
  `on_request` and before any `on_error` or `on_latency`, so
  `a2a:server_unanswered:rate5m` and `A2ARequestsUnanswered` read every
  refusal as a request the server never answered. The refusal is now counted
  as an error with its latency, like every later failure. Test:
  `handler/tenant_refusal_metrics_tests.rs` (all ten methods; each of the
  nine call sites fails it when reverted on its own).
- **`live-model`, the SLIMRPC client and the ITK agent's client exported
  nothing** (examples, `a2a-protocol-slimrpc`, `itk`). `examples/live-model`
  started no `Telemetry`, so its run over every binding, the richest
  `gen_ai.*` source in the repository, recorded into the no-op provider.
  `a2a-protocol-slimrpc` and `itk` took `a2a-protocol-client` with
  `default-features = false` and no `otel`, which drops `tracing` too, so
  their outbound calls opened no `CLIENT` span and recorded no
  `rpc.client.call.duration` (neither lockfile had a `tracing` or
  `opentelemetry` edge on the client). `live-model` now starts `Telemetry`
  from the `OTEL_*` environment as the other examples do; both manifests ask
  for `otel`. Test: `bindings/a2a-protocol-slimrpc/tests/client_telemetry.rs`
  (a `CLIENT` and a `SERVER` span for one SendMessage; fails with the
  features removed). The ITK agent and `live-model` have no test of their
  own; `scripts/check_otel_wiring.py` is what pins them.
- **`incident-response` stopped exporting after its observability act**
  (examples). The act's two checks each install a meter provider of their
  own as the global one and shut it down, and the demo goes on serving;
  handlers on the default metrics follow the global provider at each
  measurement, so everything recorded after the act went to a provider
  already shut down and the `OTEL_*` export the demo had configured lost it.
  Each check now puts the provider that was global back when it returns.
  Test: `hardening/observability.rs`
  `the_checks_leave_the_global_meter_provider_as_they_found_it` (fails with
  the restore removed from both checks).
- **Six answers the axum router and the WebSocket server gave were recorded
  nowhere** (server). Not in `rpc.server.call.duration`, so a dashboard's
  error ratio and its status breakdown missed them. The axum router's
  `TimedBody` extractor answered a body over `max_request_body_size` (413) and
  one not received in `body_read_timeout` (408) with a response nothing
  marked, and a `historyLength` or list query that did not parse was refused
  outside the recorded call; the WebSocket server answered a binary frame
  (-32700) and a request over the 64 a connection may run at once (-32603)
  without recording either. The axum body rejections are now marked as
  refused, so the validating layer records them with their status; the two
  query parses run inside the recorded call, so `GetTask` and `ListTasks` are
  recorded refused, as `RestDispatcher` records the same requests; the
  WebSocket answers are recorded with their code. Tests:
  `tests/dispatch_tests/rpc_calls.rs`
  `the_axum_router_records_a_body_over_its_limit_and_one_never_finished` and
  `the_axum_router_records_an_unreadable_query_as_the_method_refused` (the
  second compares the router with `RestDispatcher`), and `websocket.rs`
  `ws_binary_and_busy_answers_are_recorded`; each fails with its fix
  removed. Still unrecorded: a WebSocket upgrade refused for its `Origin` or
  `A2A-Version` or timed out (logged, not metered), and a message over the
  size cap, which tungstenite refuses by closing the connection with no
  JSON-RPC answer.
- **A tool-loop run that failed on the model provider told the A2A caller
  what the provider had sent back** (server, `tool-loop`). `ToolLoopExecutor`
  failed the task with the error's full text: up to 512 bytes of the
  provider's response body for a non-2xx status (which can echo the prompt or
  name an account or a quota), and, for a connection failure, the provider's
  URL. The caller is another agent or a person across the network, not the
  operator. The task now fails with `the model provider answered HTTP {status}`,
  `could not reach the model provider`, `the model provider's answer could
  not be read` or `the model client is misconfigured`, and the full error is
  logged at WARN for the operator. A tool's own message, a timeout and an
  error a `ToolModel` returns as `Other` go through unchanged, and
  `ModelError`'s `Display`, for code calling the loop directly, is unchanged.
  Tests: `tool_loop/loop_error.rs`
  `the_caller_is_told_the_kind_of_failure_not_what_the_provider_sent`,
  `tests/tool_loop/executor.rs`
  `a_provider_failure_fails_the_task_as_internal_with_the_status_and_not_the_body`
  and `a_transport_failure_does_not_tell_the_caller_where_the_provider_is`.

- **A gRPC client built from `GrpcTransport::connect` recorded no span and no
  duration** (client, `grpc`). Handed to `with_custom_transport`, the route
  the book documents for configuring the transport yourself, `GrpcTransport`
  named neither its `rpc.system.name` nor its address, and a client call
  with no system opens no `CLIENT` span and records no
  `rpc.client.call.duration`; only `build_grpc` named them. The transport now
  reports `grpc` and the endpoint it dials. Found by the new
  `crates/a2a-protocol-sdk/tests/operation_matrix`, which runs all eleven
  operations of `Method::ALL` over JSON-RPC, HTTP+JSON, gRPC and WebSocket
  and requires a `SERVER` span and `rpc.server.call.duration` point from the
  server and a `CLIENT` span and `rpc.client.call.duration` point from the
  client for each (176 checks); the other 154 held. Tests:
  `tests/grpc_unary_success_tests.rs`
  `a_transport_from_connect_names_its_system_and_address`, and the matrix
  (22 gaps before the fix, all on the gRPC client; renaming one operation in
  either side's span-name table makes it name that operation on all four
  bindings and no other).

### Deprecated

- **`signing::sign_agent_card`** (types; spec §8.4.2), for
  `sign_card_es256(card, key, key_id)`, whose `key_id` is a `&str`. Its
  `key_id: Option<&str>` let a signature's protected header omit `kid`,
  which §8.4.2 requires, so it now refuses `None` with `InvalidParams`
  instead of writing a header without one: a specification correction
  (`STABILITY.md` §2), which is the only change for a caller passing a key
  id. Kept for the one minor release `STABILITY.md` §3 requires and removed
  in 0.16.0. Test: `tests/sign_agent_card_deprecated.rs`.
  (`sign_agent_card_ed25519`, its Ed25519 twin, was added and removed within
  0.15.0 and never published; `sign_card_ed25519` is the Ed25519 signer.)

- **`HotReloadAgentCardHandler::update`**, for **`try_update`** (server;
  spec §5.7: arrays marked as required MUST contain at least one element).
  `update` stored any card; `try_update` checks it as `reload_from_json`
  checks a reloaded one (`AgentCard::validate` and
  `validate_security_schemes`) and refuses with `InvalidParams`, leaving
  the current card in place.
- **`ClientConfig::preferred_bindings` and `ClientConfig::with_preferred_bindings`**
  (client). Nothing reads them once `ClientBuilder::from_card` follows the
  card's interface order (spec §8.3.2); setting them changed nothing a caller
  could see. Rank bindings with `ClientBuilder::from_card_preferring`, and see
  the choice with `ClientBuilder::chosen_interface`. Kept for one minor
  release, as `STABILITY.md` requires.
- **`OAuthFlows::Implicit` and `OAuthFlows::Password`** (types), as the A2A
  1.0 proto marks both fields `deprecated`. They remain, parse and serialize
  as before (spec §1.4 keeps deprecated definitions until the next major
  version); code that names them now gets a deprecation warning.

### Performance

- **A blocking send costs no more CPU than in 0.14.1; telemetry compiled in
  by default is about 5% of it** (measured, not a change). On
  `send_latency_breakdown`, `diag/send/echo_3_events/single_worker`
  (release profile, one pinned core of a shared 4-vCPU Xeon at 2.10 GHz,
  rustc 1.99.1, medians of 8 interleaved rounds): 0.14.1 106.2 µs
  (102.0–114.0), this tree 102.0 µs (101.0–115.9), this tree without
  `tracing` and `otel` 95.7 µs (87.4–106.0). The ranges overlap, so the
  first pair says "not slower", not "faster". With nothing installed, the
  default `tracing` and `otel` cost 4.6% and 6.6% of a send in two runs;
  `default-features = false` removes it. The `default` comment of the server
  crate said "a level check" until this. Against `6ce5dc9a` (2026-10-12, a
  commit that was 13.9% faster than 0.14.1 in the same run) this tree is
  11.5–16.0% slower in four runs, with no step between them larger than
  the noise; the cause was not isolated. Method, every run and its spread:
  `docs/readiness-bar.md`, "the CPU cost of a blocking send".

- **A send's context lookup copies one history, not ten** (server; ADR
  0017). `find_task_by_context` listed up to ten tasks of the context
  whole, to choose one. It now lists them with `history_length: Some(0)`,
  which `InMemoryTaskStore` honours while copying (`TaskStore::list` now
  documents that a store may), and reads the chosen task whole by id. A
  store that ignores `history_length` lists whole tasks; one of those is
  used as it is, with no second read. `RequestContext::stored_task` is
  unchanged: still the stored task, history included. Measured with
  `context_lookup_cost` (release build, shared 4-vCPU Xeon, medians of 201,
  three runs each). For ten tasks of 1,024 messages in one context the
  lookup went from 3.21–3.73 ms to 364–552 µs. For one task it is unchanged
  within noise: 132–146 µs before, 135–277 µs after.
- **A streaming send's background processor and SSE forwarder are boxed
  before they are spawned**, so the span, tenant-scope and tracker
  wrappers and tokio's task cell move a pointer rather than ~5.6 KB and
  ~1.9 KB futures. Echo benchmark, streaming, one worker, jemalloc, two
  independent replicates of 10 interleaved runs: 11,107 → 11,596 and
  10,821 → 11,598 requests/s (Mann-Whitney p 0.049 and 0.023); two
  workers, no change within noise.

- **A JSON-RPC request is parsed once, not three times.** The body was
  parsed into a `serde_json::Value`, converted to `JsonRpcRequest`, and
  its `params` cloned and converted again. A single request now parses
  straight into `JsonRpcRequest`, and `params` deserializes from the
  borrowed value. A body that fails the direct parse takes the old path,
  so error codes and messages are unchanged and a duplicated key is still
  accepted (last value wins). Echo benchmark, one worker, jemalloc, 3
  interleaved runs: 19,434–19,782 → 21,033–21,615 requests/s.

- **On a single-worker runtime a blocking `SendMessage` runs its executor
  in the task that collects its events.** Two tasks on one worker cannot
  run in parallel, so the split cost only handoffs. Echo benchmark, unary,
  16 connections, jemalloc, 3 interleaved runs: one worker 14,166–14,770 →
  19,308–20,105 requests/s (user CPU 50–53 → 41–42 µs/request). With more
  workers the executor keeps its own task, which runs beside the
  collection; on two workers this measured 12,133 vs 12,353 requests/s, no
  difference within noise. A container limited to one CPU gets one worker
  (`available_parallelism` reads the CPU quota).

- **An event write no longer allocates a large future, and the handler's
  persistence channels carry the broadcast's `Arc`.** `write` does its work
  synchronously when the persistence channel has room and no terminal
  verdict is awaited; only a full channel or a gated terminal event returns
  a waiting future. The channel's item is the shared event, not a deep copy,
  so its first block is 256 bytes rather than 11.5 KB. Four allocations
  above glibc's 1 KB thread-cache limit fewer per unary send (dhat). Echo
  benchmark, unary, 16 connections, 3 interleaved runs: one server core
  10,005 → 10,493 rps (user CPU 81.2 → 77.4 µs/request, ranges not
  overlapping); two cores, no difference within noise. A fast write still
  spends the task's cooperative budget, so a tight write loop yields as the
  awaited `send` it replaces did. The public
  `new_in_memory_queue_with_persistence` keeps its by-value channel.

- **A task's event queue no longer pre-allocates ~99 KB.** The broadcast
  channel carries events behind an `Arc` instead of by value (360 bytes per
  slot × 256 slots, allocated per task). Bytes allocated per unary
  `SendMessage` on the echo harness fell from 173,278 to 82,844 (counting
  allocator, 16 connections). Instructions per request changed by under 1%
  (cachegrind), so this is a memory win, not a throughput claim.
- The blocking-send collector no longer clones every event into a push
  buffer when no push sender is configured, and saves artifact updates as
  deltas (`save_artifact_delta`) as the background processor already did,
  instead of re-saving the whole task per chunk.

- **The in-memory store's TTL pass no longer scans every task under the write
  lock.** Every `eviction_interval` writes (64 by default) the pass visited
  every stored task — up to `max_capacity`, 10,000 by default — to find the
  terminal ones last written at least `task_ttl` ago, holding the store's
  write lock while it did, so every concurrent `save` waited behind a scan
  that in the common case found nothing. A write-ordered `expiry_index` now
  lets the pass walk only the entries that old. Measured on the default
  store behind `JsonRpcDispatcher`, `SendMessage` from a closed-loop client,
  server pinned to two cores, three runs each: 4,845 → 5,593 requests/s at
  16 connections and 4,236 → 5,001 at 64, with p99 falling from 6.4 to
  5.1 ms and from 23.5 to 20.1 ms. The index costs one ordered-map entry
  per stored task; at the default cap the plateau RSS under sustained load
  rose from 96.6 to 98.1 MiB. Eviction semantics are unchanged: the same
  tasks expire at the same moment, which `expiry_tests.rs` pins at the
  boundary for every write path that moves `last_updated`. Found by the
  2026-10-02 SDK comparison (`docs/sdk-comparison-2026-10-02.md`).

### Security

- **`TaskPushNotificationConfig`'s `Debug` prints the webhook's origin, not
  its URL** (types; spec §13.2, §13.4). A webhook URL routinely carries the
  receiver's secret in its path, query or userinfo; `{:?}` printed it whole,
  into logs and panic messages, while the server's and client's own log
  lines already printed only `scheme://host[:port]`. Tests:
  `push::tests::debug_redacts_secrets_but_keeps_presence` (which asserted
  the full URL and now asserts its absence),
  `push::tests::debug_url_never_echoes_what_it_cannot_cut`.

- **`rsa` is no longer in any lockfile** (RUSTSEC-2023-0071, Marvin timing
  side channel, no fixed release). It was in `Cargo.lock` only through
  `sqlx-mysql` 0.8, a driver no feature enables, and `osv-scanner.toml`
  waived it; `sqlx` 0.9 does not bring it, so the waiver is removed. CI's
  Go toolchain moves from 1.26.9 to 1.26.10 for GO-2026-6809
  (`crypto/x509`), which govulncheck reported reachable in all three Go
  interop modules under `itk/`; with 1.26.10 it reports none.

- **`tokio` requires 1.54.1; lockfiles move off this week's advisories**
  (client, server, SDK, SLIMRPC binding). `tokio` 1.54.0, yanked for a use-after-free race in `time::Interval` on
  the multi-threaded runtime (GHSA-5wjr-9356-4wc8; the server's halt sync
  uses `Interval`), is replaced by 1.54.1 in the workspace, SLIMRPC, fuzz
  and ITK lockfiles. `rustls-webpki` 0.103.15 (RUSTSEC-2026-0382, distrusted
  emSign intermediates) is replaced by 0.103.16 wherever it was locked, and
  the SDK-comparison harness's `webpki-root-certs` 1.0.9 (RUSTSEC-2026-0383)
  by 1.0.10. The SLIMRPC lockfile's `ctutils` 0.4.3 (RUSTSEC-2026-0385,
  mask generation and `u128` comparisons that could branch on secret values)
  becomes 0.4.4; it arrives through the SLIM crates' `p256` and `ml-kem`,
  and no other lockfile holds it. Its `crypto-bigint` 0.7.5
  (RUSTSEC-2026-0387, operations meant to be constant-time that could
  branch on secret values) becomes 0.7.6; it arrives through
  `agntcy-slim-auth`'s `p256`, and no other lockfile holds it either. In
  CI's hash-locked Python tools, `certifi` 2026.7.22 (GHSA-6q48-6c7w-9gw2)
  becomes 2026.10.19 and `uv` 0.13.1 (GHSA-hq85-5cpj-mr62) becomes 0.13.5,
  for the same emSign root. The `tokio` requirement rises from 1.44.2 to
  1.54.1, so a downstream lockfile that already holds 1.54.0 must update
  (crates.io has yanked it, so a new resolution never picked it; a
  lockfile written between 2026-10-19 and the yank could). 1.54.1 needs
  Rust 1.85, under this crate's 1.88.

- **`webpki-roots` requires 1.0.10** (client, server; RUSTSEC-2026-0381,
  affecting 1.0.0 to 1.0.9). The lockfiles already resolved 1.0.10, except
  the SDK-comparison bench harness, which is updated; the manifests admitted
  the affected versions to a downstream build that resolves its own.
  `check_advisory_floors.py` found it.

- **A keyed audit digest has one spelling** (server, `audit` and
  `encryption`). `AuditLog::digest_matches` decoded the hex of a stored
  `hmac-sha256:<kid>:<hex>` with `u8::from_str_radix`, which also takes
  uppercase digits and a leading `+`, so a record whose digest had a digit
  uppercased (or `0f` written `+f`) still matched. Nothing could be forged
  this way, since the tag bytes compared were the same, but a changed record
  matched. It now takes lowercase digits only, which is all the SDK writes.
  Found by the new `sealed_audit_value` fuzz target. Test:
  `hex_round_trips_and_refuses_what_is_not_hex`.
- **A control character in a client's id is refused** (server; spec §14
  "MUST sanitize user-provided content"). `validate_id` refused only empty
  and over-long ids, so `"a\r\nb"` as a message, context, task or reference
  task id was stored, and `GetTask`, `CancelTask` and `SubscribeToTask`
  logged the requested id with `%`, writing a line of the client's choosing
  into the server's log. The push-config `taskId` and `id` (create, get,
  list, delete, and inline in `SendMessage`) and the `ListTasks` `contextId`
  filter never reached `validate_id` at all. Each now answers
  `InvalidParams` naming the field — `-32602` on JSON-RPC, `400` on
  HTTP+JSON — for any `char::is_control` character (C0, DEL, C1), as the
  idempotency key already did; a tenant is refused likewise, not only for
  U+001F. An inline push config is checked before the task is created. The
  three entry logs and JSON-RPC's dispatch log now record the id and method
  with `?`, escaped, since they run before validation. Letters outside ASCII
  still pass. Tests: `control_chars_in_ids`, `handler::ids::tests`.
- **JSON-RPC authenticates a call before reading its params** (server;
  `docs/handoff.md` "Open engineering"). A call whose `params` did not
  deserialize was answered `-32602`, naming the field at fault, before the
  interceptor chain saw it, so a caller with no credential got the parameter
  schema where a well-formed call got `401`. Such a call is now admitted
  first: one the chain refuses gets the refusal (`401`/`403` with its
  challenge, `-32600` in the body; `200` inside a batch), and one it admits
  gets the `-32602` it always got. Interceptors now see these calls, with no
  task id, and `on_complete` reports them `Failed` with the `-32602`; a rate
  limiter counts them and an audit interceptor records them. HTTP+JSON, gRPC
  and WebSocket followed the same day (Behaviour Changes, "A malformed
  request without a credential is refused as unauthenticated"). Test:
  `jsonrpc_auth_before_params`.
- **A card-signing key whose `x5c` certificate has expired is not used**
  (types, `signing`; spec §8.4.3 "Expired or revoked keys MUST NOT be used
  for verification"; RFC 7517 §4.7). `Jwk` had no `x5c` member, so serde
  dropped it and a key whose certificate had expired still verified a card.
  `Jwk` gains `x5c: Option<Vec<String>>`, and `Jwks::candidates` (so
  `verify_agent_card_with_jwks`, `verify_card_with_jwks`,
  `verify_card_json_with_jwks` and the client's signed-card fetch) uses a
  key that carries one only when its first certificate holds the same key
  (algorithm, curve and key bits from the parsed `SubjectPublicKeyInfo`)
  and the verification time is within its `notBefore`..`notAfter`,
  inclusive. An empty chain, an entry that is not standard base64 or not
  exactly one DER certificate leaves the key unused; the refusal reads as
  a missing key. Keys without `x5c` are unchanged. Not checked: the chain
  to a trust anchor, the certificates' signatures, key usage, and
  revocation (CRL/OCSP); the set itself is the caller's trusted source.
  New dependency of the `signing` feature: `x509-parser` 0.18
  (MIT OR Apache-2.0, already in the lockfile through `rcgen`), default
  features off. Tests: `signing::keys::x5c::tests`,
  `signing::keys::x5c::spki_tests`, and
  `a_fetched_key_whose_certificate_has_expired_is_not_used` in the
  client's `jwks_fetch_tests`.

- **SLIMRPC: the binding has no advisory waivers** (`a2a-protocol-slimrpc`).
  With `agntcy-slim-auth` 0.17.5 the lock takes `mls-rs-crypto-awslc`
  0.25.1, which pins `aws-lc-rs =1.18.1`, so `rustls` resolves to 0.23.46
  (from 0.23.43) and RUSTSEC-2026-0285, -0346, -0348 and -0349 no longer
  apply; `agntcy-slim-config` 0.17.5 and `regorus` 0.12.0 replace
  `serde_yaml`, so `unsafe-libyaml` (RUSTSEC-2024-0449) is gone. The five
  waivers are removed from the binding's `deny.toml` and the six entries
  from its `osv-scanner.toml`; cargo-deny 0.20.2 and osv-scanner 2.6.0 report
  nothing without them. `deny.toml` allows `CDLA-Permissive-2.0` again, as
  the root does, for `webpki-root-certs`, which the SLIM 3.x tree brings.
  Since 2026-10-19 it carries one again: `mls-rs-crypto-awslc` 0.25.1 was
  yanked with no stated defect, and 0.25.0 would bring the four rustls
  advisories back; the binding's `deny.toml` explains the entry. Update the
  binding's lockfile by package name, never with a blanket `cargo update`,
  which cannot keep a yanked version and downgrades rustls to 0.23.43.
  Since 2026-10-23 it has none again: `agntcy-slim-mls` 0.5.1 accepts
  `mls-rs-crypto-awslc` 0.26.0, so a plain `cargo update` takes
  `agntcy-slim-rpc` 3.4.6 and the unyanked 0.26.0 with `rustls` staying at
  0.23.46, and cargo-deny reports the entry as no longer encountered.

- **`rustls` 0.23.46 is the floor** (client, server, sdk). RustSec published
  four rustls advisories on 2026-10-14, all patched in 0.23.46
  (GHSA-w44r-93j7-qv28, an unauthenticated ServerHello downgrading later
  connections to classical key exchange; GHSA-c8m5-c952-6cm4, `Debug` of
  client session values printing TLS secrets; GHSA-vp88-798v-h67f, buffered
  data sent to the outer server after an ECH rejection; GHSA-h2v8-394m-p7vv,
  Brotli certificate decompression, only with rustls's `brotli` feature).
  The published manifests required `>=0.23.45`, so a consumer could keep an
  affected rustls; they now require `>=0.23.46`, and the root and both
  benchmark-harness lockfiles take 0.23.46. `check_advisory_floors.py` found
  it: 8 requirement/advisory pairs, 0 after the change. The SLIMRPC binding
  stays on 0.23.43 for RUSTSEC-2026-0285's reason (aws-lc-rs), and waives
  the three that reach 0.23.43 in its `osv-scanner.toml`; cargo-deny matches
  waivers only by RUSTSEC id, which these advisories do not have yet.
- **The client's debug output names headers and JSON keys, never their
  values** (client). `ClientRequest`, `ClientResponse`, `CardFetchOptions` and
  `WebSocketTransportConfig` derived `Debug`, so `{req:?}` in an interceptor
  printed the `Authorization` an auth interceptor had added, a
  `CardFetchOptions` printed the bearer token its own example sets, and a
  push-config call's request or response printed the webhook's `token` and
  `credentials`. They print header names, sorted, and the top-level keys of
  `params` and `result` now, as the server's `CallContext` does; read a value
  from its field when you want one. The fields' names in that output changed
  with it: `extra_header_names`, `header_names`, `param_keys`, `result_keys`.
- **A failed push delivery is logged with the webhook's origin, not its
  URL** (server). The background delivery's failure and timeout warnings
  logged the whole webhook URL, so a secret in its path or query
  (`/hooks/<token>`, `?sig=`) went into every log pipeline the server
  feeds. `HttpPushSender`'s own log lines already named the origin only.
- **SLIMRPC: a caller's `:`-prefixed metadata key is dropped**
  (`a2a-protocol-slimrpc`). A key beginning with `:` names what a server
  records itself, `":peer-addr"` (the connection's address, for
  `CallContext::peer_addr`) and `":path"`, and the SLIMRPC server records
  neither, but it passed a caller's own through as request headers. A caller
  could therefore set the address `AuthFailureThrottle` charges its failed
  attempts to, and lock out the HTTP client at that address when one handler
  serves both. Its `":path"` gave no more than the path an HTTP caller
  already chooses. Such keys are dropped now, beside SLIMRPC's own.
  `":peer-addr"` is new in this release; `":path"` was passed through by
  the binding's 0.6.0.
- **A card signature naming a critical JWS extension is refused** (types;
  spec §8.4: card signatures are JWS "as defined in RFC 7515", whose
  §4.1.11 makes a JWS invalid when its `crit` names an extension the
  recipient does not understand). `signature_header`, which
  `verify_agent_card` and the JWK Set verifiers read, ignored `crit`, so a
  signature whose protected header carried `"crit": ["exp"], "exp": …`
  verified, its expiry unenforced. This verifier understands no extension,
  so any `crit`, in the protected header or the unprotected one (where RFC
  7515 forbids it), now fails verification.
- **SLIMRPC binding: RUSTSEC-2024-0449 waived, with its chain recorded**
  (`bindings/a2a-protocol-slimrpc/deny.toml`). `unsafe-libyaml` is
  unmaintained (an informational advisory, not a vulnerability) and reaches
  only the binding, as `serde_yaml` under `agntcy-slim-config` and `regorus`.
  Every `agntcy-slim-config` release through 0.17.2 still requires
  `serde_yaml`, so no bump available here removes it; the entry names the
  release that would. The four published crates do not contain it.
- **Code-scanning findings on `main` addressed** (OSV-Scanner, Scorecard,
  CodeQL; reproduced locally with the same tools before and after).
  - Test-peer dependencies: the Python peers floor `anyio>=4.14.2`
    (CVE-2026-63374, CVE-2026-64847) and `h11>=0.16.0` (CVE-2025-43859);
    the JavaScript echo agent moves `uuid` to `^11.1.1` (GHSA-w5hq-g745-h8pq)
    and gains a lockfile. The Java peer manages the Netty and Jackson
    artifacts the advisories name directly as well as through BOMs (Maven
    already resolved the fixed versions; OSV's source mode did not). A
    recursive `osv-scanner` over the tree finds no advisories.
  - Every Docker base image is pinned by digest; the Python and JavaScript
    agent images install from hash-locked files (`requirements.lock`,
    `npm ci`); CI's `pip install` steps install from hash-locked files under
    `.github/requirements/`. Scorecard Pinned-Dependencies: 23 findings to 0.
  - `osv.yml`'s `rust` job runs the scanner inline so Scorecard sees its SARIF
    upload: Token-Permissions 7 findings to 4 (the four remaining
    `contents: write` grants create releases and push the benchmark
    dashboard and the nightly prerelease, which need them).
  - CodeQL `rust/cleartext-logging`: four `jwt.rs` test assertions no longer
    format secret-derived values into their failure messages.
  - CodeQL Python code quality: 45 findings in the gate and probe scripts
    fixed (unclosed files, silent `except` blocks, unused imports).

- **Stored secrets are wiped on drop** (`zeroize`, already in the
  dependency graph through rustls, so no new crate). This covers the HS256
  secret held by `JwtValidator`, including one refused as too short, and the
  client secret held by `OAuth2ClientCredentials`. The copies a request
  makes while it is in flight are not wiped: `ring`'s HMAC key, the token
  request body, and hyper's buffers. The documentation on each field says
  so.
- **A server whose stores cannot isolate tenants now refuses requests that
  name one.** The default `RequestHandlerBuilder` store ignored the `tenant`
  field, so a request carrying one was served from records every tenant
  shares: on 0.14.1, tenant B could get, list, subscribe to and cancel
  tenant A's task (five of five probes,
  `docs/sdk-comparison-2026-10-02.md` §5.1). `TaskStore` and
  `PushConfigStore` gain `isolates_tenants()`, `false` by default and `true`
  on the six `TenantAware*` stores. A request that resolves to a non-empty
  tenant — named by the client or derived by a resolver — is refused with
  `UnsupportedOperation` unless both stores answer `true`. Requests without
  a tenant are unaffected.
- **`JwtValidator` refuses an HS256 secret shorter than 32 bytes** (server,
  `auth-jwt` feature). RFC 7518 §3.2 requires an HS256 key at least as long
  as the hash output; `with_hs256_secret` accepted any length, down to one
  byte, so a guessable secret let anyone who guessed it mint tokens the
  server accepted. `with_hs256_secret` now fails closed for a short secret:
  it logs an error and stores nothing, so every HS256 token is rejected. The
  new `try_with_hs256_secret` returns an error instead, for refusing the
  secret at startup, and `MIN_HS256_SECRET_LEN` names the bound. A
  deployment configured with a short secret stops accepting HS256 tokens on
  upgrade and must move to a secret of at least 32 bytes. RS256 keys under
  2048 bits were already refused by `ring`. The book's example, which taught
  a 13-byte secret, and `examples/genai-agent` now use
  `try_with_hs256_secret`.

### Changed

- **The book says what a message to an `AUTH_REQUIRED` task does while its
  executor still runs** (spec §7.6.1): it waits up to
  `executor_drain_timeout`, then is refused `UnsupportedOperation`, so an
  executor that should accept the next message returns after
  `auth_required`. Behaviour unchanged.

- **`examples/deploy-agent` runs on jemalloc** (`tikv-jemallocator`, not
  on MSVC). On the echo benchmark jemalloc took unary throughput from 7,189
  to 11,862 requests/s at the same resident memory; see the book's
  production chapter, Allocator. The SDK crates set no allocator.
- **A root `.dockerignore` excludes `**/target`.** The three images built
  from the repository root (`examples/deploy-agent`, and `itk`'s
  `rust-agent` and `tck`) compile inside the image, so the host's build
  output was dead weight: measured, the context went from 10.15 GB (322 s
  to transfer) to 26.53 MB (2.3 s) on a working tree with a full `target/`.
- **Behaviour change for deployments that send tenants to the default
  stores**, which now get `UnsupportedOperation` (`-32004` on JSON-RPC,
  `400` on HTTP+JSON) where they used to be served from shared records.
  Configure the `TenantAware*` stores; configuring the task store alone is
  enough, because an unset push-config store now defaults to
  `TenantAwareInMemoryPushConfigStore` when the task store isolates. Where
  tenants only key limits over deliberately shared records, call
  `RequestHandlerBuilder::accept_unisolated_tenants()`. A custom store that
  partitions by tenant must now say so by overriding `isolates_tenants()`;
  a wrapper store must forward it.

### Internal

- **The intermittent `log_redaction` test is explained and fixed.**
  `a_failed_or_timed_out_delivery_logs_the_origin_not_the_secret` missed a
  warning about once in 20 parallel runs. It captured with a scoped
  subscriber, and `tracing-core` 0.1.36 computes a callsite's interest from
  the first-reaching thread's default while only one dispatcher is
  registered, so another test's thread reaching the same `warn!` first made
  it "never interested" for every thread (two threads and two barriers
  reproduce it every time; 160 runs of the test did not). The test now
  captures through the shared global subscriber (`warning_lines_during`).
  Regression: `handler/shutdown/tests/warning.rs`
  `a_callsite_another_thread_reached_first_is_still_captured`.

- **A prompt-injection containment harness** (`a2a-protocol-sdk/tests/injection_containment/`,
  `docs/injection-containment.md`). A deterministic model that obeys every
  `CALL` line it reads drives a `ToolLoopExecutor` whose tools pay, fetch
  pages an attacker wrote, and reach other agents over A2A on loopback. 8
  tests show a control containing an injection (tool-call idempotency, the
  token budget, content provenance, the approval gate, delegated budgets,
  caller isolation, the halt), each failing alone when its control is
  switched off; 3 assert what is not contained, so a change that closes one
  is noticed.
- **The tool-calling study's raw runs are committed**
  (`scripts/tool_calling_study/runs/`), with the scripts that resume it;
  `docs/tool-calling-study.md` gives its method as a draft. All 16 cells
  have run; none is scored yet.
- **What the SDK seals at rest is checked four ways** (server, `encryption`).
  Against another implementation: `scripts/crypto_interop/check.py` (CI,
  `ci.yml`, from a hash-locked `cryptography`) states the byte layout and
  associated data of every sealed form (the `Aes256GcmCipher` value, a
  sealed part in a record and in the log, a sealed push config, the audit
  log's wrapped subject keys, sealed values and keyed digests) and decrypts
  the 21 values the SDK wrote into `tests/fixtures/crypto/rust_sealed.json`;
  `tests/crypto_interop.rs` opens the 22 the script sealed with its own
  nonces through `EncryptedTaskStore`, the SQLite push-config store and
  `AuditLog::reveal`. Against NIST: `tests/crypto_nist_gcm.rs` runs 37 CAVP
  AES-256-GCM cases (`gcmEncryptExtIV256.rsp`, `gcmDecrypt256.rsp`, quoted
  with their source and hashes) through the cipher's open path and the
  `ring` calls its seal makes. Fuzzing: four targets (`sealed_ring_value`,
  `sealed_part`, `sealed_push_config`, `sealed_audit_value`) feed what a
  store holds to each reader; ten minutes each, 0.7 to 2.1 million runs, one
  finding (Security, "A keyed audit digest has one spelling"). Mutation
  testing of `cipher.rs`, `seal.rs`, `push/cipher.rs` and `audit/privacy/`:
  210 mutants, 2 missed, both equivalent as written and both removed by
  rewriting the expression (an `Aes256GcmCipher` value too short for its tag
  is now refused before the cipher is asked).
- **Each extension and binding URI this SDK defines is pinned to its wire
  contract** (spec §4.6.3: "A new URI MUST be created for breaking changes to
  an extension"; §5.8, the same for a binding). `uri_contracts` (types)
  snapshots, for the failure, provenance, idempotency, lease and approval
  extensions, the metadata key, the field and variant names a peer reads
  (from the `Deserialize` impls, so a new optional field shows), a fully
  populated sample of each value as it serializes, and the tokens and what
  they mean; `uri_contract_websocket` (server) snapshots the WebSocket
  binding's frames from a live dispatcher. A contract that changes under an
  unchanged URI fails with the diff and the two ways out (bump the URI, or
  re-bless with `A2A_BLESS_URI_CONTRACTS=1`), as does a bumped URI with the
  old snapshot. RELEASING.md's checklist, which asked a person to notice,
  now points at them; whether a change is breaking stays a person's call.
- **The 22 TCK MUSTs the official suite does not grade are mapped to this
  repository's tests.** `tck/untested-musts.toml` names, for each, the tests
  that assert it; `check_tck_untested_map.py` (CI) fails when an entry cites
  no test, a missing one or an ignored one, or an ID twice. New tests: a
  cross-binding suite (`a2a-protocol-sdk/tests/binding_equivalence/`) that
  drives one agent over JSON-RPC, HTTP+JSON, gRPC and WebSocket for
  `BIND-EQUIV-001..004`, `VER-CLIENT-001/002`, `VER-SERVER-001` and
  `CARD-EXT-002`, and `CARD-SIGN-001..003` tests that verify the signature
  over a signing input the test builds itself. Still not graded by the
  official suite; `docs/official-tck-findings.md` §16 says what changed.
- **The spec ledger's `SUPPORTED` answers only a MAY.**
  `check_spec_ledger.py` now refuses `SUPPORTED` or `NOT_SUPPORTED` on an
  entry whose keyword is not MAY or OPTIONAL, as the ledger's own status key
  says. 23 entries broke it: 12 are now `TESTED` (two with tests added for
  them: the WebSocket binding's data types on the wire, and a client passing
  an opaque `contextId` through unaltered) and 11 `APPLICATION`, each naming
  the hook the SDK gives. The check fails on a seeded violation.
- **`check_advisory_floors.py` retries a transient fetch.** One connection
  reset from crates.io failed the gate on a pull request that touched none
  of its inputs. A reset, a timeout, a 429 or a 5xx is now retried up to
  three times, 2, 4 and 8 s apart; any other answer is not. An offline
  self-test of those rules runs first on every invocation (`--self-test`
  runs only it), and fails under each of 11 hand-applied mutants of the
  retry code.
- **The control map covers EU AI Act Articles 13 to 15 in full**: new rows
  for 14(4)(a) (monitoring) and 15(4) (resilience), both partial with cited
  code and tests, and integrator rows for the obligations only a provider or
  deployer can meet (13(1)-(3)(e), 14(1)-(3) and (4)(b)-(c), 15(1)-(3)), and
  for Arts. 4 (as replaced), 9, 10, 11 with Annex IV, 17, 26, 27 and
  50(3)-(4). 37 controls in all.
- **The control map follows Regulation (EU) 2026/1744, the Digital Omnibus on
  AI**, read in its Official Journal edition, and the AI Act's own Articles 5,
  6, 16, 43 and 47-49, read in theirs. New integrator rows for Art. 4a (bias
  detection with special-category data), the Art. 5 prohibitions (the
  original eight, and the two 2026/1744 added, from 2 December 2026), Art. 6
  (classification, the Annex III exception, safety components as redefined),
  Art. 16, Art. 25(2) and (4) (now fineable under Art. 99(4)(da); the SDK is
  under the open-source exemption), and Arts. 43 and 47-49 (conformity
  assessment, declaration, CE marking, registration). Rows 9-10-17, 11,
  15(5), 26-27, 50(2), 72(2) and 73 carry the amended text: among it the
  2 December 2026 deadline for Art. 50(2) where a generative system was on
  the market before 2 August 2026 (Art. 111(4)), and Art. 42(3), under which
  meeting the Cyber Resilience Act's Art. 12(1) conditions satisfies Art.
  15's cybersecurity requirements. The source note gives the amended
  application dates and Annex XIV's notified-body code for agentic AI.
  44 controls, 28 of them AI Act rows.
- **The regulatory control map cites the code behind each EU AI Act
  control**, as `code = ["path.rs::Symbol"]`, and `check_compliance_map.py`
  fails when a cited symbol is not declared in its file or when a supported
  or partial AI Act row cites none (26 symbols across 8 rows). Its preamble
  no longer says Article 25(4) exempts open-source components from "the
  value-chain duties": the exemption is from that paragraph's written
  agreement only.
- **The audit register's open items are committed**, as
  `docs/compliance/audit-register.md`: eleven decisions (R12, R13, R19, R21,
  R33, R36(b), R41, R49, R50, R66, R67) and the ledger entries that need one,
  each with the code's current behaviour at file and line, the spec text,
  the options and a recommendation. None is implemented yet.
- **The spec ledger audits the v1.0.1 release.** Its entries were made
  against upstream `main` at `12e9d2fb` (retrieved 2026-10-10), which is not
  the release: it differs from tag `v1.0.1` by 65 lines, mostly link URLs and
  type names, plus one section added after the release, §7.6.4 ("In-Task
  Authorization Scope"). The vendored copy is now the tag's text, byte for
  byte, as `docs/implementation/a2a-v1.0.1-specification.md` (it was
  `v1.0.0-specification-complete.md`). Its SHA-256 is pinned in the ledger,
  and `check_spec_ledger.py` fails when the text changes without the
  entries. Every `spec_line`, and every spec line number cited in a note, was
  moved through a line map built from the two texts: 191 entries moved. The
  three §7.6.4 entries are kept under `after_release`, outside the counts,
  because v1.0.1 does not contain them. Two requirement quotes now use
  v1.0.1's wording, `PushNotificationConfig`, and note that the v1.0.1 proto
  calls that message `TaskPushNotificationConfig`. The ledger has 509 entries
  for v1.0.1: TESTED 316, APPLICATION 87, SUPPORTED 56, NOT_APPLICABLE 22,
  PARTIAL 20, NOT_SUPPORTED 8.
- **The versioned documentation site deploys again.** `docs.yml` failed on
  `main` at `2b0a3abd`: its API step writes `/dev/api/index.html` as a
  redirect page with no `</head>`, and `build_docs_site.py`'s `make_noindex`
  removed the page's canonical link but added `noindex` only before
  `</head>`, so the page had neither and the site check refused it. A page
  without `</head>` now gets the tag after its doctype. The workflow-gate
  prover's fixture for the site check now runs `docs.yml`'s own API step
  instead of a hand-made dev book, and its healthy case failed before the
  fix with the fault CI reported. The full build (13 release lines and dev)
  checks clean locally: 794 book pages, 0 faults.
- **OSV-Scanner on the SLIMRPC binding.** RUSTSEC-2024-0449 (unsafe-libyaml,
  unmaintained) was waived in the binding's `deny.toml` but not in its
  `osv-scanner.toml`, so the scan failed on `main`; the waiver is mirrored.
  The file now says why RUSTSEC-2026-0285 is listed under both of its ids
  (osv-scanner matches it by the GHSA id and reports the RUSTSEC entry as
  unused, a warning). `osv.yml`'s blocking `Rust lockfiles` job runs on pull
  requests too: until now a pull request ran only the job that reports
  findings new against the base, so a finding already on `main` failed no
  pull request. That is what cargo-deny in `ci.yml` already does.
- **Workflow actions** (Dependabot #169): `ossf/scorecard-action` 2.5.0
  (`fa7117d`, the commit tag `v2.5.0` peels to).
- **`keep_alive_comments_reset_the_idle_bound` tolerates a runner stall**
  (client tests). It failed on Windows on `9aacd7a` and on `main` at
  `6ce5dc9`: the stub and the client share one thread, so a stall longer
  than the bound minus the 100 ms heartbeat interval fires the idle timeout
  correctly. The bound is now 3 s (about 2.9 s of stall tolerated, was
  0.9 s), and a failure reports the stub's longest gap between writes, which
  says whether the runner stalled or heartbeats were not counted.
- **Push delivery: a redirect is not followed and a response body is not
  read**, now pinned in `push_sender_tests.rs`
  (`a_redirect_is_not_followed`, `a_flooding_response_body_is_not_read`).
- **`scripts/adversarial/probe_push.py` exercises its receivers again.** On
  Python 3.13 and later `threading.Thread.__init__` sets `self._handle`,
  which shadowed the receiver's connection handler, so every handler thread
  died with a `TypeError` and every mode behaved as a silent hang. The hang
  mode also closed the socket it meant to hold (`finally: c.close()`).
  Measured on Python 3.13.16 by connecting to each mode before and after.
- **Workflow actions** (Dependabot #164): `actions/upload-artifact` 7.0.2,
  `actions/download-artifact` 8.0.2, `actions/setup-node` 7.1.0, and
  `dtolnay/rust-toolchain` at `e2a55d2` (2026-10-08, which hardens how the
  action handles its inputs). Its pin comment said `master 2026-09-03`; the
  previous pin was dated 2026-09-11 and the comment now gives the new
  commit's date.
- **The weekly mutation sweep's survivors are gone** (2026-09-28 onward, six
  each week). New tests fail under `SseParser::reset_event_state` emptied
  and under `RequestHandler::task_count` returning 0 or 1. The
  `TaskStore::earliest_event_seq` default and the conformance harness's
  `CancelOnFirstWrite::close` were equivalent mutants, no-ops either way, and
  are now written so the tool does not generate them; the queue test that
  justifies the second now checks that `close` really is a no-op. No
  behaviour changes. `check_endpoint_reachable`, the sixth, was already
  killed on `main`.
- **The conformance peers are at their latest releases, and no skip is
  left** (`itk/agents/`, `tck.yml`). `@a2a-js/sdk` 1.3.0, a2a-go v2.6.0 and
  `a2a-java` 1.4.0.Final each pass 21/21 on both bindings of this kit.
  a2a-java 1.4.0.Final accepts `application/a2a+json` on REST, so the java-sdk
  leg's skip of `a2a_media_type_accepted`, the last one, is gone. a2a-go
  v2.6.0 reads the spec's `securityRequirements` lists, so
  `go_sdk_interop.sh` step 1b runs the whole a2a-go client battery against
  such a card (36 checks) instead of asserting the rejection.
- **Pin freshness grades the Python SDK again** (`pin-freshness.yml`). Its
  python-sdk leg read `tck.yml` with a `python3` that had no PyYAML and
  died before grading anything (2026-10-05). Every leg now sets up Python
  and installs PyYAML from `.github/requirements/pyyaml.txt` first. The
  same run's go-sdk leg failed because a2a-go 2.6.0 needs Go 1.26, which
  the workflow has asked for since 2026-10-10.
- **The full mutation sweep fits its timeout again.** `a2a-server` is
  dealt round-robin across 24 shards instead of in 12 consecutive slices,
  after two of the 12 were cancelled at 240 minutes on 2026-10-12; projected
  from that run's own timings, the slowest shard drops from over 240 minutes
  to about 110. `mutation-history.md` records that sweep and the four before
  it, none of which had been recorded.
- **A pull request's mutation shards share the work evenly.** The
  incremental job's eight shards are dealt round-robin too. Sliced, a shard
  held whichever crates' files fell to it: in three runs on this branch the
  slowest shard, all `a2a-server` mutants, took 114 to 120 minutes against a
  120-minute timeout, while in one of them the two holding only `a2a-types`
  mutants took 20 and 27. On the same 1,160 mutants, the last sliced run
  lost four of its eight shards to that timeout; the first round-robin run
  finished all eight in 79 to 110 minutes and passed.
- **A failed executor is shown to be logged** (§3.3.2: servers SHOULD log
  system errors). `executor_failure_logged.rs` asserts the ERROR event
  that names the task and the cause; it fails with the call deleted,
  demoted to WARN, or without the cause.
- **Tests for four spec requirements that were implemented but untested**:
  §3.1.4 `includeArtifacts` omission (`list_tasks_artifacts.rs`), §3.3.4
  streaming refused without the capability on the JSON-RPC wire and §3.5.2
  one closed stream leaving another streaming (`streaming_contract.rs`). Each
  but the last is shown to fail with its check removed; the §3.5.2 test has
  no single-point fault to inject and guards against adding one.
- **The vendored proto is A2A v1.0.1's, and CI checks that it stays the
  latest release's.** All six copies were v1.0.0's text; v1.0.1 changed only
  comments, the normative `tenant` rules among them, and nothing compared the
  copies with upstream. `proto/UPSTREAM` pins the release (tag, commit,
  SHA-256); `check_proto_copies.sh` holds every copy to that hash offline,
  and `check_a2a_proto_upstream.sh` (`official-tck.yml`) fails when a newer
  release changes the file.
- **Workflow tokens are least-privilege at the top level.** `benchmarks.yml`
  and `itk-nightly.yml` granted `contents: write` to every job; only the job
  that pushes or uploads has it now. OpenSSF Scorecard's first run on `main`
  (2026-10-10, 5.8/10) scored Token-Permissions 0 on these two grants.
- **`osv.yml` scans `verification/loom-halt/Cargo.lock` on pull requests
  too**, now that `main` has it.
- **Proofs, not only tests** (`verification.yml`).
  - Kani proves the task state machine whole, in
    `crates/a2a-protocol-types/src/kani_proofs.rs`:
    - terminal states are final;
    - every unfinished task can be cancelled;
    - nothing re-enters `Submitted`;
    - terminal and interrupted states are disjoint.
  - Kani also proves the ES256 and EdDSA key parsers safe and exact over
    every byte string up to their SPKI length.
  - A loom model in `verification/loom-halt`, outside the workspace, checks
    the halt/admission protocol in every interleaving.
  - Each proof was shown able to fail. A weakened Kani assertion produced a
    counterexample, and the loom model with the second check removed finds
    the escaping interleaving.
- **ACTS is a merge gate, and a release attests it** (`acts.yml`,
  `scripts/acts_predicate.py`).
  - The A2A project's conformance suite runs on every pull request, at a
    pinned a2a-itk revision, over JSON-RPC, gRPC and HTTP+JSON.
  - It fails on any failed or errored test at any level. That is stricter
    than upstream's MUST-only `--require-conformant`.
  - `release.yml` runs it on the tag and attests the summary onto each
    `.crate`. The GitHub release waits on it.
  - The nightly also runs ACTS and publishes `acts_rust.json`.
  - `prove_workflow_gates_fail.py` proves the gate can fail: a report with
    one failed test fails it, and so does no report at all.
- **Upstream drafts, not sent:**
  - `docs/upstream/a2a-itk-dashboard-enrolment-draft.md` asks the ITK
    dashboard to list this SDK. It also reports that the shared driver
    hard-codes the `a2aproject` org in its history URLs, so this
    repository's nightly history never accumulates.
  - `docs/upstream/a2a-oversight-provenance-extensions-draft.md` proposes the
    approval and provenance extensions.
- **Supply-chain workflows: OSV-Scanner, CodeQL and OpenSSF Scorecard.**
  `osv.yml` scans all seven tracked `Cargo.lock` files on every pull request
  (blocking on findings the PR introduces) and daily (blocking on any), and
  reports, without blocking, the Go, Java, JavaScript and Python lockfiles of
  the conformance peers. Its first run found `itk/Cargo.lock` and
  `fuzz/Cargo.lock` still on h2 0.4.15 (RUSTSEC-2026-0258), which the root
  workspace had left behind; both now pin 0.4.20. The two documented
  waivers — `rsa` in the root lockfile, which no build resolves, and the
  SLIMRPC binding's RUSTSEC-2026-0285 — are recorded in an
  `osv-scanner.toml` beside each lockfile with a reason and an expiry; the binding's waiver was re-checked with `cargo update -p rustls --precise 0.23.45` on 2026-10-08 and is still blocked upstream.
  `codeql.yml` runs `security-extended` over the Rust sources, the workflows
  and the gate scripts; `scorecard.yml` publishes the Scorecard result for
  the README badge. `docs/openssf-best-practices.md` answers all 67
  passing-level criteria of the OpenSSF Best Practices badge with evidence,
  ready for the maintainer to register.
- **A regulatory control map, checked in CI.** `docs/compliance/control-map.md`,
  generated from `docs/compliance/controls.toml`, maps 27 provisions of the
  EU AI Act, the Cyber Resilience Act, the GDPR, OWASP's agentic Top 10 and
  ISO/IEC 42001 onto what the SDK provides, each with the tests or CI steps
  that prove it and what is missing. It states plainly that the AI Act places
  no obligation on this SDK. `scripts/check_compliance_map.py --check`, a
  static-checks gate proven by `prove_gates_fail.sh`, fails when a cited test
  no longer exists or is not a test, when a cited file or CI step is gone, or
  when the Markdown is stale.
- **Release tags must be signed** (`release.yml`). A tag must point at a
  commit on `main` and carry an SSH or OpenPGP signature that verifies
  against a key in `.github/release-signers/` as it stands on `main`
  (`scripts/verify_tag_signature.sh`, whose `--self-test` the same step runs
  and which proves ten verdicts). It fails closed: no key is committed yet,
  so no release passes until the maintainer adds one. See `RELEASING.md`.
- **`SECURITY.md` states the support period and has a section for EU Cyber
  Resilience Act integrators**: a line is supported exactly while it is the
  latest minor; Article 13(5) due-diligence material, the Article 13(6)
  reporting route, and that a reporter's legal notification duties take
  precedence over the coordinated-disclosure embargo.
- **The README's official-TCK badge said 88/114 MUST with 4 failing**, from
  before CORE-SEND-003 joined the baseline on 2026-09-26. It now says 87 and
  5, as the README's own text and `tck/conformance-baseline.json` do.

## [0.14.1] - 2026-09-30

### Security

- **`PathSegmentTenantResolver` let a caller choose its own tenant
  (GHSA-hr9h-6jvf-wvg6).** The resolver read the `:path` pseudo-header and,
  when that was absent, an ordinary request header named `path`. Only the
  WebSocket dispatcher set `:path`, so on every other entry point the tenant
  came from the `path` header, which any client can send. A server built
  with `.with_tenant_resolver(PathSegmentTenantResolver::new(..))` and served
  over JSON-RPC, HTTP+JSON, gRPC (as a metadata entry) or the Axum adapter's
  `A2aRouter` ran a request carrying `path: /tenants/<other>/x` as tenant
  `<other>`, with that tenant's tasks and push configurations to read, list,
  cancel, subscribe to and write. Path-based tenancy isolates tenants only
  when something in front of the agent authorizes each caller against the
  URL, as the multi-tenancy guide requires of this resolver, and that is the
  control the header defeated: a caller admitted to one tenant's URLs could
  act as another. On JSON-RPC the header overrode the URL outright
  (`/tenants/acme` with `path: /tenants/victim/x` ran as `victim`); on
  HTTP+JSON a URL naming a different tenant was rejected as a mismatch, so
  the header worked on routes without a tenant prefix, such as
  `/message:send`. Affected: 0.7.0 through 0.14.0, the
  releases in which the handler calls the configured resolver (0.3.0 through
  0.6.0 had the same fallback, but only application code that called the
  resolver itself was exposed). Not affected: the WebSocket binding,
  `HeaderTenantResolver`, `BearerTokenTenantResolver`, custom resolvers that
  do not read a `path` header, and servers with no resolver. The same gap meant the
  legitimate URL form never worked on the HTTP bindings: JSON-RPC served
  `/tenants/acme` from the default tenant, and HTTP+JSON rejected it with
  400 because the resolver found no tenant to match the URL's.
  - **Fix.** The JSON-RPC and HTTP+JSON dispatchers now record the request
    URI's path under `:path`, as the WebSocket dispatcher already did. A real
    header cannot carry that name. The resolver reads `:path` and nothing
    else.
  - **Migration.** gRPC and `A2aRouter` pass the resolver no URL, so there it
    now resolves no tenant: the request is served from the default tenant,
    or rejected under `require_resolved_tenant()`. A deployment that used
    this resolver on those entry points was taking the tenant from the client
    all along. Switch it to `BearerTokenTenantResolver`, or to
    `HeaderTenantResolver` behind a proxy that sets the header and strips any
    inbound copy.
  - **If you cannot upgrade yet,** strip the inbound `path` header (for gRPC,
    the `path` metadata key, which is the same HTTP/2 header) at a proxy in
    front of the agent, or change resolver as above.
  - **Known limitation.** The resolver returns the path segment as sent, but
    HTTP+JSON percent-decodes the tenant it takes from the URL, so on that
    binding a tenant whose name needs percent-encoding is rejected with 400.
    This fails closed.
  - **Regression tests:** `crates/a2a-protocol-sdk/tests/tenant_path_resolver_isolation.rs`
    sends a forged `path` header over JSON-RPC, HTTP+JSON, gRPC and
    WebSocket and asserts the executor never ran as that tenant, that the
    URL form selects the tenant where the URL can carry one, and that the
    URL still wins when both are sent;
    `crates/a2a-protocol-server/tests/axum_tenant_path_header.rs` does the
    same through `A2aRouter`. Before the fix all but the WebSocket case fail
    with the executor having run as the forged tenant.

### Internal

- **The vendored SLIMRPC specification follows upstream's 2026-09-29 merge;
  the binding does not.** Upstream merged its collaborative-task work to `main`
  as `1328426`, which added three documents to `main` and changed both
  vendored ones, and the nightly `Official TCK` run failed on the drift check —
  before the official suite ran, so `main` went without a full conformance
  result until this landed. All five documents are now vendored. Everything the
  merge added to the two the binding implements is A2A 1.1 (`SendLiveMessage`,
  a multicast form of it, and the `slimrpc-context-map` key it uses), and no
  A2A 1.1 specification has been released, so the binding is unchanged; the
  three collaborative-task documents are byte-identical to the branch text
  already triaged as not followed. The reasoning is in
  `spec/slimrpc_v1/README.md`. The binding's README and book chapter no longer
  say those documents never reached `main`, and now qualify "complete" as the
  A2A 1.0 surface. The three triage entries they held in
  `scripts/check_slimrpc_spec.sh` are removed, since a file on `main` can no
  longer match one.
- **Two `a2a-protocol-server` unit tests failed intermittently, each for a
  reason in the test.** Both surfaced on Dependabot pull requests whose diffs
  could not have caused them (#145, #146).
  `the_rps_unit_is_multiplied_by_the_window` failed on macOS with its 121st
  request admitted: the limiter counts in fixed windows aligned to the wall
  clock, so a minute boundary inside the test's loop resets the count, as it
  should. It and the one-second-window tenant test now discard any attempt
  that straddles a boundary and retry, via `refused_within_one_window`.
  `concurrent_claims_of_one_key_produce_exactly_one_winner` failed on Windows
  with no winner at all, which only a pre-existing claim can produce. Its
  database lived at `a2a-idem-{pid}`, and the removal ran while the store
  held the file open, which Windows refuses; a later test process given the
  same PID found the key already claimed. Reproduced by leaving such a
  database behind: 0 claimed, 16 replayed, the CI signature exactly. It and
  `a_migrated_store_can_take_an_artifact_append`, which used the same naming,
  now use a per-run directory and drop the store before removing it.
- **`prove_workflow_gates_fail.py` could not probe a release after a patch
  release.** Its breaking-patch scenario bumped to `{major}.{minor}.1`,
  which after 0.14.1 is the version already declared, so the bump was an
  empty commit and the probe crashed the Static checks job on this
  release's own pull request. It now bumps the patch number.

## [0.14.0] - 2026-09-26

**Cadence exception:** 0.13.0 came out on 2026-09-20, but this release can't wait until October. It fixes problems people are running into now. A dropped send could leave a task wedged or stuck in `working` (N26, N27). The client never let go of a revoked credential (N36). The official conformance suites also found several interoperability failures.

### Breaking Changes

- **`tracing` is a default feature of `a2a-protocol-client`,
  `a2a-protocol-server` and `a2a-protocol-sdk`.** A default build used to
  compile every log call and failure report in the crates to nothing, so an
  agent logged nothing however its subscriber was set up, and paths that
  report only through `tracing` — a trace header the WebSocket binding
  cannot carry, a skipped webhook — were silent (audit O13). The maintainer
  chose default-on (ADR 0013). `tracing` was already in every default build's
  dependency graph through the HTTP stack; the feature adds
  `tracing-attributes` and `syn`. With no subscriber installed the cost is a
  level check per call site. **Migration:** none for most builds. To keep the old behaviour,
  depend with `default-features = false` (and re-add `tls-rustls` on the SDK
  if you want HTTPS).

- **`default-features = false` on `a2a-protocol-sdk` now removes what the
  SDK's defaults enable.** The SDK took the client with the client's own
  defaults, so rustls stayed in an SDK built without default features,
  contrary to its manifest (audit K1). The SDK now takes the client and
  server without their defaults and forwards its own: a default build
  enables exactly what it did (plus `tracing`, above), and
  `cargo tree -p a2a-protocol-sdk --no-default-features` has no rustls,
  hyper-rustls or webpki-roots. **Migration:** a build that used
  `default-features = false` and still reached `https://` agents was relying
  on the defect; add `features = ["tls-rustls"]`.

- **A peer that goes away mid-call reads the same on every binding.** Found
  by driving each binding against a scripted peer that cuts streams off
  (audit N13): the same event — the connection or stream ending before a
  final event — was a retryable `Http` error on JSON-RPC and HTTP+JSON, a
  non-retryable `Transport("WebSocket connection closed")` on WebSocket, and
  a non-retryable `Protocol(InternalError)` on gRPC, so retry and resume
  logic, and the failure class an executor reports, depended on the binding.
  Now, over WebSocket, a stream in flight when the socket drops ends as
  `IncompleteStream` (retryable, as `EventStream` rules on every other
  binding), a unary call in flight fails with `HttpClient` (retryable), a
  connection that cannot be made is `HttpClient`, and a handshake the peer
  answers with an HTTP status is `UnexpectedStatus { status, .. }` — so a
  401 reaches `BearerAuthInterceptor`, which drops the refused token, as it
  already did over HTTP and gRPC. A frame over the size cap, or one that
  breaks the protocol, stays a non-retryable `Transport` error. Over gRPC, a
  response that ends with no `grpc-status` trailer — what tonic reports for a
  truncated stream — ends a stream as `IncompleteStream` and fails a unary
  call with `HttpClient`, both retryable. **Migration:** code matching
  `Transport(_)` to detect a dropped WebSocket, or
  `Protocol(e)` with `e.code == InternalError` to detect a truncated gRPC
  stream, should match `IncompleteStream { .. }` / `HttpClient(_)`, or ask
  `is_retryable()`. Not a wire change. The retry a retryable drop invites
  reconnects on the same `WebSocketTransport` (audit N18; see **Fixed**).

- **gRPC status codes map to what the caller has to do about them.** Over the
  gRPC transport and the slimrpc binding, `UNAUTHENTICATED` is now
  `ClientError::UnexpectedStatus { status: 401, .. }` and `PERMISSION_DENIED`
  is `UnexpectedStatus { status: 403, .. }`, the shapes the JSON-RPC and REST
  bindings already report. They were `Protocol(InvalidParams)` on gRPC and
  `Protocol(InternalError)` on slimrpc, which told a caller with a revoked
  token to fix its request, and meant `BearerAuthInterceptor` never dropped a
  token a gRPC agent had refused: the next call sent it again. A peer's
  `CANCELLED` is now a non-retryable `Protocol` error (`InternalError`, or
  the A2A code its `ErrorInfo` names) instead of a retryable `Timeout`, so a
  call the peer abandoned is no longer re-sent. **Migration:** code matching
  `Protocol(e)` with `e.code == InvalidParams` to detect an authentication
  failure over gRPC should match `UnexpectedStatus { status: 401 | 403, .. }`.
  A peer's cancellation now reports `is_retryable() == false`, so a retry
  policy stops re-sending it with no change to the caller's code. Neither is
  a wire change; both change the client's reading of one.

- **`ClientBuilder::from_card` refuses a card with no interface for A2A
  protocol major 1.** Such a card used to be accepted with a warning and then
  fail at the first call with whatever the wire produced — a v0.3 endpoint
  answers `-32601 method not found`. The error now lists what the card offers.
  Versions are read leniently: empty, `v1.0` and `1-preview` all count as
  major 1.

- **Graceful shutdown ends in-flight tasks before it drains connections.**
  `Server::serve_with_shutdown` used to drain first, and an open SSE stream is
  a connection that does not close until its task ends: the drain waited out
  its 15 s, `RequestHandler::shutdown` then cancelled tokens without waiting
  for any executor, and a delegating executor's downstream tasks outlived the
  process. Reproduced with a coordinator delegating to two a2a-go workers:
  exit after 16 s, `abandoned: 1`, no terminal event upstream, no `CancelTask`
  downstream. The order is now: stop accepting; let tasks finish on their own
  for up to `ServeConfig::completion_grace` (5 s); cancel the rest and give
  their executors `task_grace` (10 s) to cancel what they delegated and write a
  terminal event; then drain. With the completion window at zero, the same run
  exits in 0.02 s with both Go workers cancelled and the upstream stream
  ending `TASK_STATE_CANCELED`; with the default window it first waits up to
  5 s for the delegation to finish on its own (not re-measured). An
  executor that returns without a terminal state after shutdown has its
  `cancel` hook run, as `CancelTask` would. **The trade:** a task that outlives
  the completion window is now cancelled where it used to get the whole drain
  window; the three phases are bounded by 30 s together, a Kubernetes
  `terminationGracePeriodSeconds` default.

- **Every shipped `TaskStore` refuses to move a terminal task to a different
  state**, atomically with the write, answering `UnsupportedOperation` with a
  `TerminalStateConflict` in its data. Code that "reopens" a finished task
  through a shipped store now gets that error. See **Fixed** for why.

### Behaviour Changes

Changes in what a caller observes for the same input, with no change to any
type or signature — the entries a careful adopter has to read at every bump,
and which `cargo semver-checks` cannot see. Listed here as well as under
their own heading. An adopter asked for this section after 0.12.1's change to
what a new task on an existing context inherits (#130) reached them filed
under **Fixed**; that entry did say it changed the wire shape, but not where
someone scanning for such changes would look.

- **The axum adapter's successful responses carry `A2A-Version`** (server,
  `axum` feature; audit N38), as its errors and `RestDispatcher`'s responses
  already did. HTTP+JSON responses stay `application/json`, a deliberate
  deviation from spec §11.1's SHOULD for `application/a2a+json`: the
  official Go SDK's client cannot read an error labelled that way.
- **The axum adapter's error bodies are AIP-193, as `RestDispatcher`'s are**
  (server, `axum` feature; audit N37). It answered `{"error": "<text>"}`; it
  now answers `{"error": {"code", "status", "message", "details"}}` through
  the REST dispatcher's own builders. A client that read `error` as a string
  from the adapter reads `error.message`. REST's own route and body errors
  gain the `status` name they lacked.
- **A refused credential answers `401`/`403`, not `400`** (server; audit N36,
  ADR 0014). The auth interceptors answered every refusal as
  `InvalidRequest`: HTTP `400` on both HTTP bindings, gRPC
  `INVALID_ARGUMENT`. Now a missing or wrong credential answers `401` with a
  `WWW-Authenticate` challenge (`Bearer realm="a2a"`, or
  `ApiKey header="x-api-key"`) and gRPC `UNAUTHENTICATED`; the AIP-193 body's
  `status` says `UNAUTHENTICATED`. JSON-RPC answers the same `401` with the
  body unchanged (`-32600`); batches stay `200`. WebSocket is unchanged (the
  body alone). **Migration:** a client or alert that matched `400` for an
  auth failure matches `401`/`403`.
- **The SLIMRPC binding's server answers a refused credential
  `UNAUTHENTICATED` or `PERMISSION_DENIED`** (binding; audit N36). It mapped
  by error code alone, so the same refusal went out as `INVALID_ARGUMENT` and
  a client never dropped a revoked token there, the N36 defect the gRPC
  dispatcher had already been fixed for. Found auditing the book on
  2026-09-25. **Migration:** as for gRPC above.
- **`SendMessage` refuses a part whose `mediaType` the agent card does not
  declare, with `ContentTypeNotSupportedError`** (server; audit N34). When
  the card declares input modes — `defaultInputModes` or any skill's
  `inputModes` — a part carrying an explicit `mediaType` outside them is
  refused (-32005 over JSON-RPC, HTTP 400 over HTTP+JSON), as spec §3.1.1
  requires; it used to reach the executor. Parts without a `mediaType` are
  not checked, and nothing is enforced without a card or when the card
  declares no modes. **Migration:** declare every media type the agent
  accepts on its card, or call
  `RequestHandlerBuilder::allow_undeclared_input_modes()` to keep the old
  behaviour while the card is corrected.
- **A continuation sent the moment a task reaches `input-required` is
  admitted** (server; audit N21). It used to be refused as "already being
  processed" whenever the executor that parked the task had not yet returned
  — see **Fixed**.
- **A WebSocket stream the caller reads more slowly than the agent writes
  ends with a `stream_lagged` error once 64 frames are waiting** (client;
  audit N29). It used to be held back without loss, which stalled every
  other call on the socket — see **Fixed**. `ClientError::is_stream_lagged`
  identifies it; resubscribe to continue.
- **A call on a `WebSocketTransport` whose socket has dropped reconnects**
  (client; audit N18) instead of failing with a non-retryable `Transport`
  error. Code that matched that error to detect a dead transport now sees
  the call succeed, or fail retryably if the reconnect does.
- **A WebSocket request whose connection ends is cancelled** (server;
  audit N30). It used to run on after its peer had gone, which is how a
  peer that stopped reading held its connection open; a request on the HTTP
  bindings is dropped the same way when its client disconnects.
- **An idle `SubscribeToTask` stream over SSE now ends at
  `subscribe_max_idle`** (server; audit N25). It used to stay open for as
  long as its client did — see **Fixed**. A client that relied on the
  stream staying open resubscribes, which §3.5.2 supports.

### Security

- **The published manifests no longer admit dependency versions with RustSec
  advisories** (audit N24). A consumer's cargo keeps whatever version its own
  lockfile holds as long as our requirement admits it, so a fix that only
  moved this repository's lockfile never reached them — an adopter on 0.12.1
  found `cargo update -p a2a-protocol-client` leaving rustls 0.23.43 in place,
  inside RUSTSEC-2026-0285. Measured against the RustSec database on
  2026-09-24, the requirements admitted affected releases under seven
  advisories: `rustls` (RUSTSEC-2024-0336, RUSTSEC-2024-0399,
  RUSTSEC-2026-0285), `ring` (RUSTSEC-2025-0009), `bytes`
  (RUSTSEC-2026-0007), `sqlx` (RUSTSEC-2024-0363), `time`
  (RUSTSEC-2026-0009) and `tokio` (RUSTSEC-2025-0023, unsound). The floors
  are now `rustls >=0.23.45`, `ring 0.17.12`, `bytes 1.11.1`, `sqlx 0.8.1`,
  `time 0.3.47` and `tokio >=1.44.2`; each declares a `rust-version` at or
  below 1.88, and both lockfiles already held newer versions, so neither
  changed. **Effect on consumers:** upgrading to this release moves any of
  those dependencies still below its new floor. A new CI gate,
  `scripts/check_advisory_floors.py`, fails when a normal or build
  dependency of a published crate admits an affected, published version;
  `cargo deny`, which reads our lockfile, could not see this and was green
  throughout.

### Added

- **`ServerInterceptor::on_complete` and `CallOutcome`.** `after` runs only
  on success and its error replaces the response, so no hook saw a failed
  or abandoned call: an interceptor that acquired something in `before` —
  an in-flight gauge, a concurrency slot, an open audit record — had no
  place to release it. `on_complete` is called once per call, in reverse
  order, on every interceptor whose `before` ran (including the one that
  refused the call), with `Succeeded`, `Failed(&ServerError)` — the error
  the caller is sent — or `Cancelled`, when the call's future is dropped
  unanswered. It returns `()`, so it cannot change the response. The
  default does nothing, so existing interceptors compile and behave as
  before. Every `RequestHandler` method, on every binding, runs through it;
  calls made with `ServerInterceptorChain::run_before` and `run_after`
  directly do not. A new test drops a real JSON-RPC and HTTP+JSON
  connection mid-call and sees `Cancelled` in `on_complete` and `cancelled`
  in the `rpc.server.call` record — the first test of the premise that
  N26, N27 and N28 rest on: hyper drops a request's future when its client
  disconnects.

- **`Task::new`, `TaskQueryParams::new` (with `with_history_length` and
  `with_tenant`), and `a2a_protocol_server::CancellationToken`** (audit
  N16). A custom store or test built a `Task`, and every `GetTask` caller a
  `TaskQueryParams`, with a full struct literal that breaks when a field is
  added; and an executor that stored its `RequestContext::cancellation_token`
  needed its own `tokio-util` dependency at a matching version to name the
  type. The re-export reaches the SDK as `a2a_protocol_sdk::server::CancellationToken`.

- **`testing::ScriptedPeer`** (client feature `testing`): a loopback A2A
  peer that stalls, cuts off, mis-frames or refuses every call on purpose,
  over JSON-RPC, HTTP+JSON, WebSocket and gRPC, for testing code that calls
  agents — a coordinator has to survive the worker it delegates to, and a
  well-behaved test server never produces these failures. This crate's own
  tests drive every binding against every script with it
  (`tests/scripted_peer_tests.rs`), which is how N13 above was found. Adds no
  dependency; not for production builds.

- **`GrpcDispatcher::serve_with_shutdown` and
  `WebSocketDispatcher::serve_with_shutdown`**, with `with_completion_grace`,
  `with_task_grace` and `with_drain_timeout` on both dispatchers (defaults
  `DEFAULT_COMPLETION_GRACE`, `DEFAULT_TASK_GRACE`, `DEFAULT_DRAIN_TIMEOUT`).
  Both take a bound listener and a signal and return the same `ServeReport` as
  `Server::serve_with_shutdown`, in the same order: stop accepting, end the
  tasks in flight, then drain. Until now neither dispatcher took a signal, so
  a gRPC or WebSocket deployment had to assemble the sequence by hand or
  orphan its delegations on shutdown (audit S8). Two differences follow from
  the transports: gRPC connections are sent `GOAWAY` as soon as accepting
  stops, so they take no new calls during the completion window; and each
  WebSocket connection, once the tasks have ended, finishes the requests it
  has read and is closed with a Close frame. The gRPC server's own error is
  returned rather than discarded.

- **Release gates for what a tag publishes** (`scripts/check_release_tree.py`,
  four steps in `release.yml`). A release now fails if the tagged tree has
  entries under `[Unreleased]`, if any packaged file changed after the
  release's own notes were last edited, if a patch release or a second
  breaking minor in one calendar month carries `### Breaking Changes`, or if a
  packaged `.crate` was built from another commit or a dirty tree. `v0.13.0`
  would have failed three of them (audit N7, N10). The cost is ordering:
  release notes must be the last edit before the tag, and the next breaking
  release cannot be tagged before October 2026. Not a change to any crate.

- **The mutation gate grades function bodies it never could.**
  `scripts/install_cargo_mutants.sh` installs a pinned cargo-mutants 27.1.0,
  patched so that a function returning a `Result` alias (`A2aResult<T>`,
  `ClientResult<T>`) or a boxed future (`Pin<Box<dyn Future<Output = T>>>`,
  every object-safe async trait method here) gets a "replace the body" mutant
  that compiles. Stock 27.1.0 generated none that did for 346 functions, so
  whether their effects were tested at all had never been graded (audit N4,
  N9, N11). The survivors the patched build found are killed by tests on
  this branch. The install verifies the crate's checksum, applies the patch
  exactly, and refuses a binary without it.

- **A Go SDK interop gate in CI** (`go-sdk-interop`, `scripts/go_sdk_interop.sh`).
  The official Go SDK's client against this server, and this client against an
  a2a-go server, over JSON-RPC, HTTP+JSON and gRPC. Before it, a2a-go ran in CI
  only as a server driven by the in-repo TCK, which does not use
  `a2a-protocol-client`. Against the unfixed tree it was red for five of the
  defects fixed below (S2, S7, T1, C8 and C9 in the 2026-09-22 audit's
  numbering); its C10 check could not run there, because the T1 failure
  stopped card resolution first. `scripts/prove_gates_fail.sh` proves it fails when
  the push token stops reaching a Go webhook.

- **`RequestHandler::finish_in_flight` and `RequestHandler::cancel_in_flight`**,
  returning `InFlightReport`; `ServeConfig::completion_grace` and `task_grace`
  (`DEFAULT_COMPLETION_GRACE`, `DEFAULT_TASK_GRACE`); `ServeReport::tasks`; and
  `Dispatcher::request_handler`, defaulted to `None` and overridden by the
  JSON-RPC and REST dispatchers. Call `finish_in_flight` yourself when
  something else owns the sockets — `examples/deploy-agent` does, behind Axum.

- **`impl From<ClientError> for A2aError`**, so an executor that delegates can
  use `?`. A downstream `Protocol` error passes through unchanged; exactly the
  errors `is_retryable()` accepts fail the task as `Transient`, everything
  else as `Internal`, with the client's text and source chain kept. The server
  now honours a class recorded with the new **`failure::set_error_class`** /
  **`failure::error_class`**, because no error code can express `Transient`.

- **Client stream liveness bounds:** `ClientConfig::stream_idle_timeout` and
  `stream_first_event_timeout` (both 5 minutes by default, with builder
  setters), applied by `A2aClient` to every stream on every binding, and TCP
  keepalive (60 s idle, 15 s interval, 4 probes) on every client connection. A
  stream whose server sent one event and then went silent while holding the
  connection open used to hang `next()` forever. Any bytes reset the idle
  bound, SSE keep-alive comments included; on gRPC and WebSocket, which carry
  no heartbeat the client can see, it bounds the gap between events.

- **Resuming a broken stream:** `ClientError::IncompleteStream { last_event_id,
  detail }`, `EventStream::last_event_id()` and
  `A2aClient::subscribe_to_task_from(id, last_event_id)`, which sends
  `Last-Event-ID`. Verified against this repository's JSON-RPC and REST
  dispatchers. gRPC and WebSocket streams carry no ids.

- **`ClientConfig::max_event_size`** (16 MiB) and `EventStream::with_max_event_size`.

- **`CallInterceptor::on_error`** and **`TokenProvider::invalidate`**, both
  no-op by default. `BearerAuthInterceptor` uses them so an OAuth2 token the
  agent answered with `401` is not sent again; the refused call still fails.
  On every binding: gRPC's `UNAUTHENTICATED` and a WebSocket handshake refused
  with 401 reach it as `UnexpectedStatus { status: 401, .. }` since the two
  entries under **Breaking Changes**.

- **`OAuth2ClientCredentials::with_failure_backoff`** (1 s), **`Method::returns_empty`**,
  and **`push::webhook::notification_token`**, which reads the push token under
  either header name.

- **`TaskStore::save_appending_history`**, which persists a task whose history
  grew by the messages a turn added, without the caller shipping the history
  it has already stored. Additive: the default reads the record, appends,
  trims and writes it back through `save`, which is exactly what the send path
  did before, so a store that does not override it behaves as it always has.
  `InMemoryTaskStore` overrides it to extend a `Vec` in place.

  `task.history` is **ignored** by this method — the appended messages come
  from their own parameter. That is deliberate: reusing the field for the tail
  would put the caller back in possession of the whole conversation, which is
  the cost being removed. `max_history` is a parameter rather than a constant
  in the store, because retention is the handler's policy and a store
  inventing its own cap would silently disagree with the rest of the server.

  Measured on one box, 1,400 sequential posts to one channel: 3,962µs →
  2,748µs per send at the 1,024-message history cap, and growth across the run
  3.2x → 2.1x. It does **not** make a send O(1);
  `docs/swarm-scale-findings.md` has the per-stage attribution showing what
  still dominates.

- **Spans for every inbound call, on every binding** (ADR 0013; audit O1, O2,
  O4). With the `tracing` feature — on by default — each call runs in one
  `SERVER` span named `lf.a2a.v1.A2AService/{Method}`, with the semantic
  conventions' `rpc.system.name`, `rpc.method`, and on failure
  `rpc.status_code` and `error.type`; HTTP+JSON spans add `http.request.method`
  and `http.route`. The executor, event processor, push delivery and SSE
  writer run in child spans, the executor's carrying `a2a.task.id` and
  `a2a.context.id`. With `otel` and a `tracing-opentelemetry` layer, an
  inbound `traceparent` is the span's remote parent and the `traceparent` sent
  downstream names that recorded span instead of an id nothing records.
  `InboundTracePolicy::Drop` records no span for the call or anything it
  spawns. Gate: `crates/a2a-protocol-sdk/tests/observability_e2e/`, which
  failed on `main`.
- **`rpc.server.call.duration`** in `OtelMetrics`, and **`Metrics::on_rpc_call`**
  with **`RpcCall`** for any other exporter: every call's duration with the
  conventions' buckets (`otel::RPC_DURATION_BUCKETS`), method, and the status
  its binding answered with — including calls the handler never sees (a body
  that is not JSON-RPC, an unknown method, an HTTP+JSON request refused on its
  parameters) and calls whose peer went away (`error.type` `cancelled`)
  (audit O5, O11). Additive: the callback defaults to a no-op.
- **Connection statistics are reported.** `serve`, `serve_with_addr` and
  `Server::serve_with_shutdown` now call `Metrics::on_connection_pool_stats`,
  which nothing called, so the four `a2a.server.pool.*` instruments the book
  catalogues were never emitted (audit O10). The gRPC and WebSocket
  dispatchers' own listeners do not report them yet.

### Changed

- **A client stream that ends before its final event yields
  `ClientError::IncompleteStream` before `None`.** A final event is a
  `Message`, or a task in a terminal or interrupted state (§3.1.2, §11.7).
  Before, a stream cut off mid-task was indistinguishable from one that
  finished, and a half-written last frame was dropped silently.

- **`stream_connect_timeout` bounds only establishing a stream**; the first
  event has its own 5-minute bound. The 30 s connect timeout used to bound the
  first event too, which cut off agents — a2a-go among them — that send headers
  and then think. If you shortened the connect timeout to fail fast, also set
  `with_stream_first_event_timeout`.

- **The JSON-RPC client reads a streaming call's pre-stream error in either
  shape**: the plain JSON-RPC error body this server sends, or one SSE frame
  carrying it, which is what a2a-go's server sends. Both become the same
  `ClientError::Protocol`. The server's own shape is unchanged: a plain JSON
  200, which the official a2a-tck requires (STREAM-SUB-003/004). a2a-go
  v2.5.0's client reads streaming answers only as SSE, so it still loses
  these errors. That is a2a-go's to fix, and `go-sdk-interop` pins it.

- **Wire: an HTTP+JSON stream's error frame is a `google.rpc.Status`**
  (`{"error":{code,status,message,details}}`), the error's `data` a flat
  `google.protobuf.Struct` detail. a2a-go's REST client could not decode the
  old bare `{code,message}` frame. This client decodes all three shapes.

- **Push notifications carry the token under both `X-A2A-Notification-Token`
  (Python SDK) and `A2A-Notification-Token` (a2a-go)**, the same value in each.
  A Go webhook read no token before. This reverses 0.8's removal of the
  unprefixed name, which is a2a-go's rather than this SDK's invention. Push
  logs now show only `scheme://host[:port]`: webhook URLs often embed secrets.

- **`StringList` reads a2a-go's bare-array scopes and `null`** as well as the
  spec's `{"list":[...]}`, which is still the only form written.

- **A streaming client's terminal frame waits for the store's ruling** (at most
  the queue's write timeout, 5 s) and carries the stored state. See **Fixed**.

- **`save_status_delta` is now overridden by `SqliteTaskStore` and
  `PostgresTaskStore`**, not only by `InMemoryTaskStore`. Until now a SQL
  deployment got nothing from it — no regression, since the trait's default
  delegates to `save`, but no win either, and neither the trait docs nor the
  findings report said so. Measured on one box, 100 status events on a channel
  holding 200 messages, medians of five runs:

  | store | `save` | `save_status_delta` |
  |---|---|---|
  | `InMemoryTaskStore` | 8,499µs | 639µs |
  | `SqliteTaskStore` | 122,071µs | 37,216µs |
  | `PostgresTaskStore` | 287,287µs | 120,104µs |

  Both overrides update the `state` column that `list` filters on and the
  `updated_at` that §3.1.4 orders by, either of which is wrong in a way no
  round-trip through `get` would reveal. The SQLite override deliberately does
  *not* delete the artifact journal that its `save` deletes: `save` has just
  rewritten the document with every part in it, a status delta has not, so
  those rows are still the only record of appended parts.

- **`a2a.server.latency` uses the semantic conventions' buckets** (5 ms to
  10 s) and is deprecated in favour of `rpc.server.call.duration`, to be
  removed no earlier than the next minor release. It kept the SDK's default
  boundaries, sized for milliseconds, so every call under 5 s landed in one
  bucket (audit O5). Dashboards that read its `le` labels see new ones.

### Fixed

- **A continuation answering `input-required` at once is admitted** (server;
  audit N21). A blocking send returns when the task reaches an interrupted
  state, and a stream delivers that state, before the executor that wrote
  it has returned; admission refused any send while that executor's token
  was live, with an error telling the caller to wait for `input-required` —
  the state it had just been sent. The executor's writer now marks the turn
  *parked* when the latest state it writes is `input-required` or
  `auth-required`, and admission waits up to
  `HandlerLimits::executor_drain_timeout` (default 5 s) for a parked turn's
  executor to finish. A send into a task still working is refused at once,
  as before, and one still beside a live executor past the bound is refused
  exactly as before, so two executors never run for one task.
- **An idle `SubscribeToTask` stream over SSE ends at `subscribe_max_idle`**
  (server; audit N25). The bound was measured inside a future that each SSE
  keep-alive discarded, so with the defaults — keep-alive 30 s, bound 300 s
  — a subscription to a task parked at `input-required` never ended and
  polled the store for as long as its client stayed. `InMemoryQueueReader::read`
  is now cancel-safe across the reattach wait.
- **A send whose client goes away mid-commit no longer wedges its task**
  (server; audit N26). A request dropped between claiming its idempotency
  key and spawning its executor — a client timing out during a slow store
  write — kept the task's queue lease and cancellation token for the life
  of the process, so every later continuation was refused as "already being
  processed", and a retry carrying the same idempotency key waited on a
  task that did not exist. What the send took is now released when its
  future is dropped, and only what it took: a continuation dropped while it
  waits for the previous turn leaves that turn's executor cancelable.
- **A blocking send whose client goes away still records the task's
  outcome** (server; audit N27). The collector that persists a blocking
  send's events ran in the request's future, so when a client timed out on a
  slow agent the executor finished into nothing: the task stayed `working`
  in the store, and no later push notification was sent. The collection now
  runs on its own task, which shutdown waits for, and the request awaits it.
- **A failing interceptor `after` hook no longer orphans a send's task**
  (server; audit N28). `after` ran between spawning the executor and
  attaching what persists its events, so its error left a task the agent
  went on to complete at `submitted` in the store. It now runs once the
  response exists. `ServerInterceptor::after`'s documentation said it runs
  even when the handler fails; no method ever did that, and it now says so.
- **`WebSocketTransport` reconnects after its connection drops** (client;
  audit N18). Once its socket dropped, every later call failed at once with
  a non-retryable `Transport("WebSocket connection closed")`, so a
  long-lived client had to be rebuilt by hand after any server restart. The
  next call now reconnects to the same endpoint with the same configuration,
  bounded by `connect_timeout`, one reconnect for concurrent callers; a call
  that races the old connection's close fails as retryable. Streams already
  open keep the connection they were opened on.
- **An unread WebSocket stream no longer stalls other calls on the same
  socket** (client; audit N29). The transport's single reader waited for
  room in a stream's 64-frame buffer before reading anything else, so a
  `GetTask` made before reading an open stream timed out once the agent had
  sent more than 64 events. The reader no longer waits: a stream whose
  buffer overflows ends with a `stream_lagged` error, as a lagging reader
  does on the server.
- **A WebSocket peer that stops reading is closed by the idle timeout**
  (server; audit N30). With `with_idle_timeout` set, a peer that stopped
  reading while being streamed to was never closed: the stream's send held
  the sink's lock, the keepalive waited for that lock, and so did the
  closing handshake, so the connection and its `max_connections` slot were
  held for as long as the peer liked. The keepalive no longer waits, the
  connection's requests are cancelled when the peer is gone, and the close
  handshake is bounded at 1 s.
- **A gRPC stream the client cancels releases its subscription at once**
  (server; audit N31), rather than at the task's next event.
- **The agent card's poll watcher retries a card that failed to parse**
  (server; audit N32). It recorded the file's mtime even when the reload
  failed, so on a filesystem with coarse timestamps a fix written in the
  same second as a half-written file was never loaded.
- **Every task status carries a timestamp** (server; audit N35).
  `EventEmitter::status` — the helper executors are pointed to — built its
  status with `TaskStatus::new`, which leaves `timestamp` empty, so agents
  built on it served statuses with no time: spec §5.6.1 wants ISO 8601 UTC
  timestamps, `ListTasks` is ordered by them (§3.1.4), and ACTS
  DM-SERIAL-001 (a MUST) failed. The event queue now stamps any status
  update or task snapshot the executor left without one, before it is
  persisted or streamed; a timestamp the executor set is kept. The stamp
  counts toward `max_event_size`, so a status event within a few dozen
  bytes of that limit may now be refused.
- **The two HTTP+JSON dispatchers answer the same failure identically**
  (server; audit N37) — see **Behaviour Changes**. Found while fixing N36.
- **The axum adapter's successes carry `A2A-Version`** (server; audit N38)
  — see **Behaviour Changes**. Found while examining ACTS REST-CT-001.
- **This SDK's client recovers from a revoked or rotated token against this
  SDK's server** (server; audit N36). The client drops a cached token on
  `401`; the server never sent one, so the refused token was re-sent on every
  call until the provider's cache expired, on every binding. See **Behaviour
  Changes**. New in `a2a-protocol-types`: `A2aError::unauthenticated`,
  `A2aError::permission_denied`, `A2aError::auth_rejection` and
  `AuthRejection`/`AuthRejectionKind`, for custom interceptors to get the
  same statuses.
- **`SendMessage` checks message parts' media types against the agent
  card** (server; audit N34) — see **Behaviour Changes** for what is
  refused and how to opt out. Found by the ACTS conformance suite
  (CORE-SEND-004, a MUST); the official Rust SDK has the same gap open as
  a2aproject/a2a-rs#270.
- **JSON that is not a JSON-RPC Request object is answered Invalid Request
  (-32600), not Parse error (-32700)** (server, JSON-RPC and WebSocket; audit
  N33). A body with no `method`, a bare value such as `1`, an empty batch
  `[]` and a batch item that is not a request were all answered -32700,
  which JSON-RPC 2.0 reserves for a body that is not JSON; its own examples
  answer these -32600. Found by the ACTS conformance suite (CORE-ERR-006, a
  MUST). A client that matched on -32700 for these cases now sees -32600.

- **The agent card's poll watcher sees a change made just after it starts**
  (audit N23). It read the file's baseline mtime inside its own task, on
  the blocking pool, so a card rewritten before that read landed became the
  baseline and was never loaded. The baseline is now read when
  `spawn_poll_watcher` is called.
- **A WebSocket stream outlives the client that opened it**, as streams on
  the HTTP and gRPC bindings do (audit N22). Dropping the client, or the
  `WebSocketTransport`, aborted the connection's reader while the stream
  still held its own sender, so the stream went silent: no later event, and
  no end until the idle bound (five minutes by default) reported a
  `Timeout`. The stream now holds the connection open until it is dropped.
- **Wire: `RestDispatcher` answers an overload with `503` and an oversized
  body with `413`**, as the axum adapter always has, instead of `500` and
  `400`. The two HTTP+JSON dispatchers had separate mappings, so the same
  overload told a client of one to retry and a client of the other not to
  (audit N20). One mapping now serves both.
- **The pool counters count each connection once.** `OtelMetrics` added the
  cumulative totals in each report to its counters, so every report
  re-counted every earlier connection (audit O10).
- **The WebSocket dispatcher's documentation** said it routes the v0.3
  `method/verb` aliases; it routes only `message/stream` of them and refuses
  the rest with `MethodNotFound`, as its own test asserts (audit N19).

- **The book's Rust examples compile.** 127 of its 130 `ignore`d blocks now
  compile (the other 3 are `slimrpc`, outside the workspace), and 79 of those
  also run; the other 48 would call a network or a database. Most needed only
  hidden `# ` lines supplying what the page established in prose. Five could not have compiled as shown — each confirmed by
  compiling the original with only its missing context added: a client
  moved into two calls (E0382), a `match` on the `#[non_exhaustive]`
  `SendMessageResponse` with no wildcard arm (E0004), an
  `Arc<RateLimitInterceptor>` passed where a `ServerInterceptor` is required
  (E0277), `&*writer` on a queue writer with no `Deref` (E0614), and
  `ClientBuilder::new("…".into())` (E0283). Others named what does not exist
  (`send_streaming_message()`, `AuthInterceptor::new`, `RetryInterceptor`)
  or needed a feature the page did not mention (`hyper-util`'s
  `server-auto`, `tracing-subscriber`'s `json`). And compiling them contradicted the prose around them:
  - `AgentExecutor::cancel`'s default was documented as refusing with
    `TaskNotCancelable`; it has cancelled since 0.7.
  - The builder was said to choose the transport from the URL; without
    `with_protocol_binding` it is always JSON-RPC.
  - CORS read as on by default; it is off until `with_cors`.
  - "`sqlx::PgConnection` is not `Send + Sync`" — it is. A single connection
    is still the wrong choice, because every call then waits on its lock.
  - The body-size pitfall's fix checked only `size_hint()`, which a chunked
    body leaves unbounded; the page now adds `http_body_util::Limited`, as
    the REST dispatcher does.
  - The custom authentication interceptor compared tokens with a `HashSet`
    lookup and did not mark itself `authenticates()`; the page now points
    fixed token lists at the constant-time built-ins, and its example
    records the caller and sets the marker.
  - The production page's health-check example could not type-check; the
    REST dispatcher and `A2aRouter` already answer `/health` and `/ready`,
    and the JSON-RPC dispatcher does not, which the page now says.

  `.book-ignore-baseline` now holds only the three `slimrpc` blocks.
  Documentation only; no crate changed.

- **The configuration reference's defaults are the defaults.**
  `book/src/reference/configuration.md` gave `with_executor_timeout` a
  default of None; the builder sets one hour, so a task can be failed by a
  bound the page said was not there (audit S12). It also had no row for
  `TaskStoreConfig::max_events_per_task` (512) or `idempotency_key_ttl` (24
  hours), or for `require_version_header` (on) in `DispatchConfig` and
  `GrpcConfig`. `crates/a2a-protocol-sdk/tests/book_defaults.rs` now reads
  each defaults table on the page and compares it with the struct's real
  `Default`, in both directions: a documented value that differs, a row that
  names no field, and a field with no row all fail. Against the page as it
  was, it reports exactly those five. Documentation only.

- **The four crate READMEs — each crate's crates.io page — compile.** Seven
  of their eight Rust blocks did not: the client's documented
  `resubscribe()`, `get_authenticated_extended_card()` and
  `ClientBuilder::with_transport()`, none of which exist; a `Message` literal
  and `match`es that the types' `#[non_exhaustive]` rejects; a server prelude
  that is the SDK's. Each README is now a doctest of its crate
  (`#[cfg(doctest)] #[doc = include_str!("../README.md")]`), and its feature
  table — like the two in the book and the one in `crates/README.md` — is
  checked against the manifest by `scripts/check_feature_tables.py`, which
  found 17 missing or wrong rows. The book and root README no longer say
  `default-features = false` removes rustls from the SDK; it does not yet
  (audit K1).

- **Agent-card signing canonicalizes numbers the way RFC 8785 says.** Three
  deviations, each found by the RFC's own vectors, now in
  `crates/a2a-protocol-types/tests/rfc8785_vectors.rs`: a double exactly
  halfway between two shortest renderings took the upper one, not the even
  one (`1424953923781206.2`, Appendix B note 4); a card parsed from text
  could read a float one ULP off, because serde_json's default parser is not
  correctly rounded (5 of Appendix B's 24 values); and an integer beyond
  2^53 was written with all its digits rather than as the double JCS
  defines every number to be (`9007199254740993` is `9007199254740992`).
  Against V8's `JSON.stringify` over the same 1,000,077 random doubles, the
  canonicalizer disagreed 1,474 times from the bits and 237,518 times from
  the text; it now disagrees 0 times on either, and 0 times over 8,552,446
  structured ones (every `m × 2^e` with odd `m` below 4096), which is where
  the fix's own first version was caught wrong at 2^-24. **What it trades:** a
  signature made by 0.13.0 or earlier over a card holding an affected number
  does not verify here, and the reverse — though neither verified against
  any other conforming implementation, which is the correction (a
  specification fix, `STABILITY.md` §2). The `signing` feature now enables
  serde_json's `float_roundtrip`, which makes float parsing correctly
  rounded, and slower, in every crate of a build that enables `signing`.

- **`Server::serve_with_shutdown` sees its signal at the connection
  ceiling.** It waited for a connection permit before it looked at the
  signal, so with `ServeConfig::max_connections` set and every slot held by a
  stream that only shutdown could end, shutdown never began: the test that
  pins it timed out after 20 s with the only slot held. The signal is now
  watched while waiting for the permit as well as for the peer. The new gRPC
  and WebSocket shutdown paths are built the same way and tested for it.

- **An unreachable OAuth2 token endpoint is a transient failure.** A refused
  or dropped connection to the token endpoint, or to the OIDC discovery
  document, surfaced as `ClientError::Transport`, which is not retryable, so
  through `From<ClientError> for A2aError` a task whose token fetch met a
  restarting identity provider failed as `Internal` rather than `Transient`.
  It is now `ClientError::HttpClient`, as the transports report the same
  error. A token endpoint that answers 429 or 5xx is still `Transport`; that
  is recorded as open (audit N2).

- **Replicas starting together against an empty PostgreSQL database all come
  up.** `CREATE TABLE IF NOT EXISTS` races in PostgreSQL: two sessions that
  both find a table absent both create it, and the second fails with a
  duplicate key in `pg_type` or `pg_class`. Every schema-creating constructor
  — `PostgresTaskStore::{new, from_pool, with_migrations}`, the tenant-aware
  task store, both push-config stores and `PostgresRateLimitCounter` — ran its
  DDL unlocked (`with_migrations` included, because it creates
  `schema_versions` before it can lock it), so a multi-replica deployment's
  first start could crash a replica. Measured on 16.13 with 48 constructors
  against a fresh database: 2 failed on the first attempt; 0 of 48 in each of
  20 consecutive runs after. Each constructor's DDL now runs in one transaction
  holding a transaction-scoped advisory lock, the same key for every store, so
  they serialize against each other and against the migration runner. **The
  trade:** schema creation is serialized across the whole database — a
  cold start of N replicas does its DDL one replica at a time — and the key,
  the bytes of `"a2a_schm"`, is one an application's own advisory locks on
  the same database should avoid.

- **A task another replica canceled no longer ends `Completed`.**
  `CancelTask` on replica B wrote and answered `Canceled`, and replica A's
  executor, which never heard of it, then wrote `Completed` over it: every
  store applied status writes unconditionally. `tests/cross_replica_cancel/`
  reproduced it on the in-memory, SQLite and PostgreSQL stores. The refused
  write is now how A learns: it cancels its executor's token, stops persisting
  and pushing its events, answers a blocking caller with the stored task, and
  pushes the stored terminal status once. **The trade:** A learns at its
  executor's next write, not immediately, and non-terminal frames emitted in
  between still reach a client streaming from A. Custom `TaskStore`s are not
  covered unless they apply `store::refuses_write` inside their writes.

- **Agent cards from a2a-go with `securityRequirements` parse.** The reverse
  direction is a2a-go's to fix: v2.5.0 rejects the spec's shape. The interop
  gate pins that rejection and fails when it stops holding.

- **Deleting a push config on an a2a-go agent reports success.** a2a-go answers
  JSON-RPC with no `result` and HTTP+JSON with an empty 200; both are now
  accepted, for Empty-result methods only.

- **Interface selection skips other protocol majors**, so an a2a-go agent's
  v0.3 endpoint listed first is no longer chosen; binding names match
  case-insensitively in `build()` as in selection; and `build()` falls back to
  the card's next interface when the chosen one cannot be built.

- **HTTP+JSON streaming errors decode to the A2A error they carry**, from a
  non-2xx answer and from inside a 200 stream, where both used to surface as
  `UnexpectedStatus` or `Serialization("unknown variant …")`. The stream-lag
  signal survives the new server frame: `A2aError::is_stream_lagged` is true
  across a REST stream from this server.

- **OAuth2 refresh failures are shared, not serialised.** Every caller waiting
  on a refresh gets its outcome, failure included, from one request. Five
  callers with 1 s timeouts used to fail at 1, 2, 3, 4 and 5 s. A caller
  cancelled mid-refresh hands the attempt to the next waiter.

- **Short-lived OAuth2 tokens are cached for at least half their lifetime.** A
  token living 30 s or less was refetched on every call.

- **The SSE parser refuses a line that outgrows `max_event_size`** with
  `EventTooLarge` instead of silently truncating it past twice the limit.

- **`RequestHandler::shutdown` counts the live queues it destroys.**
  `ShutdownReport::queues_force_destroyed` was a hard-coded 0, so a shutdown
  that cut live streams reported itself graceful.

- **A message naming a live task in its own context is no longer refused.**
  `resolve_task_id` accepted only the single task `find_task_by_context`
  returns — the most recently updated non-terminal one — and rejected every
  other, including tasks that were live and in the same context. Since a
  message carrying a `contextId` and no `taskId` starts a *new* task
  (§3.4.3), one participant posting without a `taskId` forked the channel,
  the fork became the context's canonical task, and every other participant
  was locked out of the original permanently with
  `InvalidParams: message task_id does not match task found for context`.

  §3.4.3 permits continuing "a specific task" by `taskId`, and mandates
  rejecting exactly one mismatch: a `contextId` differing from that of the
  referenced task. §3.4.1 says a `contextId` "logically groups multiple `Task`
  objects". The implementation treated a context as holding one. It now
  resolves against the task the message actually names and refuses only a
  genuine cross-context reference.

- **A `taskId` that names no existing task now returns `TaskNotFound`**
  (§3.4.2) rather than `InvalidParams`, whether or not the context happens to
  hold some other task. The handler already returned `TaskNotFound` for that
  input when the context was empty; the only thing deciding between the two
  answers was whether an unrelated task existed nearby.

- **The in-memory event queue's documentation described the wrong lag
  behaviour.** It said a slow SSE consumer "receives `Lagged(n)` and skips
  missed events". The consumer does not resume: `InMemoryQueueReader::read`
  turns the lag into `A2aError::stream_lagged`, which `streaming::sse` writes
  as an `event: error` frame before closing the stream. A lagging tail gets a
  contiguous prefix and an announced end, never a silent hole — the stronger
  of the two behaviours, and the one a reader can recover from. Documentation
  only; no code changed.


### Internal

- **The documentation site serves each page's own canonical URL.** Every
  page's HTML declared the site root as its canonical and carried two
  identical site-wide descriptions; a script rewrote them after load, which
  Google advises against for canonicals (Search Console reported
  introduction.html as "Duplicate without user-selected canonical").
  `scripts/seo_postprocess.py` now writes the canonical, share URLs, titles
  and one description per page into the built HTML before deploy, and
  `docs.yml` fails the deploy if any page is wrong; `print.html` and
  `404.html` are `noindex`, the sitemap drops `introduction.html` (a copy of
  the root), the benchmark dashboard gains a canonical and description, and
  the JSON-LD's MSRV (it said 1.93) is read from `Cargo.toml`.
- **STABILITY.md's one-breaking-minor-a-month rule gains a declared
  exception** for fixes adopters cannot wait for. A release's notes may
  carry `**Cadence exception:** <reason>`; `check_release_tree.py cadence`
  then accepts a second breaking minor in the month and prints the reason,
  and refuses the line without one. Breaking changes in a patch release are
  still refused. `prove_workflow_gates_fail.py` proves the reasonless case
  fails.
- **The genai examples move to genai 0.6.5** (`genai-a2a-agent`,
  `incident-response`; supersedes Dependabot's #128). 0.12 kept them on 0.5
  because every 0.6.x depends on the unmaintained `paste`
  (RUSTSEC-2024-0436). That advisory is now `deny.toml`'s first ignore, with
  its justification: it is informational, `paste` is a compile-time
  proc-macro, and it reaches only these two examples, not a published
  crate. genai 0.7.0-beta.24 was considered and still depends on `paste`.
  No source change was needed; both examples' tests pass.
- **`scripts/check_panic_paths.py` counts `unreachable!`**, which panics as
  `panic!` does and was invisible to the gate. The one in library code — the
  JSON-RPC dispatcher's batch branch, after an `is_array()` test that made
  it redundant — is gone: the branch now matches the array directly.
- **`scripts/preflight.sh` builds with `CARGO_INCREMENTAL=0`, as CI does.**
  `ci.yml` does not set it, but both actions every CI job starts with export
  it (read at their pinned SHAs). Without it a `--full` run kept 16 GB of
  incremental state CI never builds and nearly filled a 38 GB allowance.
  `prove_gates_fail.sh` already turned it off; its comment saying CI did not
  is corrected.
- **The mutants the release PR's in-diff mutation run found surviving are
  now caught by tests**: stamping a task snapshot's status timestamp (N35;
  only status updates were tested), the gRPC `UNAUTHENTICATED` and
  `PERMISSION_DENIED` mappings for a refused credential (N36; their tests
  lived in the SDK crate, which the server crate's mutation run does not
  execute), admitting a send when the task's only token is already
  cancelled (N21), every `google.rpc.Code` name the REST dispatcher's
  plain error responses carry (N37; eight of its eleven arms were
  untested), and the agent-card poll watcher warning once about a card
  that stays broken (N32; in a test binary of its own, because a
  thread-local capture in the unit-test binary saw 0 warnings in some
  full-suite runs once another test had cached the callsite as
  uninterested). The default `ServerInterceptor::on_complete` is marked
  `#[mutants::skip]`: it is an empty future, and so is the mutant (ADR
  0006). Tests only; no behaviour changed.
- **The official TCK's `CORE-SEND-003` is baselined** as a defect in the
  suite: it declares no expected error, so it demands that a message part
  with an unsupported media type be accepted, and it began failing when N34
  started answering the `ContentTypeNotSupportedError` §3.1.1 requires.
  Evidence in `docs/official-tck-findings.md` §22.

### Correction, 2026-09-24: claims that were not true when published

An audit of every public claim from 0.6.0 onward — the READMEs inside the
published `.crate` files, the crate documentation, the book, the GitHub
release notes and this file — found claims their own artifacts contradict.
Those in the repository are corrected in this release. Those in published
artifacts cannot be changed: crates.io and docs.rs go on serving them for
each version below until the next release replaces the current one. Each
was checked against the published `.crate` for that version.

- **`a2a-protocol-types` README, 0.7.0 to 0.13.0:** "`A2A_VERSION` —
  `"1.0.0"`". The constant is `"1.0"` in every one of those versions.
- **`a2a-protocol-types` README, every version:** "zero `unsafe` blocks
  anywhere in this crate". From 0.12.0 the published crate's `build.rs`
  contains `unsafe { std::env::set_var("PROTOC", …) }`, as do the client's
  and server's; `#![forbid(unsafe_code)]` covers `src/` only. The 0.12.0
  release notes counted four such build scripts in the repository; there
  are five.
- **`a2a-protocol-client` README, every version:** documents
  `resubscribe()`, `get_authenticated_extended_card()` and
  `ClientBuilder::with_transport()`. None exists in any published version;
  the methods are `subscribe_to_task`, `get_extended_agent_card` and
  `with_protocol_binding` / `with_custom_transport`.
- **`a2a-protocol-client` and `a2a-protocol-server` READMEs, every
  version:** the `signing` feature is described as agent card signature
  verification ("signing verification" on the server). In both crates it
  only forwards `a2a-protocol-types/signing`; neither crate signs or
  verifies anything on its own. This is a security-relevant capability
  claim, and it was false.
- **The book's introduction**, as deployed: "zero surviving mutants
  enforced via `cargo-mutants` CI gate" (the gate covers only functions a
  pull request changes; the weekly sweep has recorded survivors), "the
  entire codebase is free of unsafe code", "no panics on any caller input or
  I/O failure" (no gate measures that), "exhaustive pattern matching"
  (protocol enums are `#[non_exhaustive]`), "zero-cost abstractions" (the
  extension points are trait objects and boxed futures), and "all defaults
  … are overridable". Each is rewritten to what its evidence supports.
- **0.7.0 release notes:** "Official-SDK interop is now proven, both
  directions … 20/20 on both JSON-RPC and REST for every one". The 20/20
  for the JavaScript and Java SDKs was scored after skipping checks they
  fail (`list_tasks_basic` and `a2a_media_type_accepted` for JavaScript,
  `a2a_media_type_accepted` for Java); the entry mentions that `--skip`
  exists but not that it was used on them. The reverse direction was the
  Python SDK's client only.
- **0.8.0 release notes:** "MUST compatibility **100%**" and an empty
  conformance baseline. True of what that run graded; four MUSTs have failed
  against the suite's stale v1.0.0 table since A2A v1.0.1, and the figure
  to quote is 88 of 114 passing and 4 failing.
- **0.12.1**, raised by an adopter: the change to what a new task on an
  existing context inherits (#130) was filed under **Fixed**. The entry
  said it changed the wire shape, but not where someone scanning for such
  changes looks; **Behaviour Changes** above exists for that.
- **`CITATION.cff`**: "A complete Rust implementation" — rewritten to what
  is measured.

## [0.13.0] - 2026-09-20

### Correction, 2026-09-23: nineteen entries that shipped in 0.13.0

The `v0.13.0` tag is on `391f0df`, the merge of #138, not on the release
preparation that wrote this section. The crates published as 0.13.0 were
built from that tag (their `.cargo_vcs_info.json` says so), so they carry
#138's changes — but at that commit this file still listed them under
`[Unreleased]`, and until today it went on doing so. Moved here verbatim; the
GitHub release notes for 0.13.0, extracted from this section at the time, do
not include them. Found by `cargo semver-checks` reporting nothing to do for
a tree whose `[Unreleased]` claimed a breaking change (audit N7).

#### Breaking Changes

- **`RetentionPolicy` and `PurgeReport` are `#[non_exhaustive]`.** Construct a
  policy with `RetentionPolicy::new` and the `with_*` setters, and read a
  report's fields rather than destructuring it exhaustively. `STABILITY.md` §4
  lists the configuration structs that carry this marking so a new option is
  additive; `RetentionPolicy` was missed by the 0.12.0 conversion that
  introduced the rule, and the list said otherwise until now. Marking both is
  what makes the two additions below — and the next one — non-breaking.

#### Added

- **`RetentionPolicy::idempotency_key_max_age`** (default 24 hours) and
  **`TaskStoreConfig::idempotency_key_ttl`** (default 24 hours), with
  `DEFAULT_IDEMPOTENCY_KEY_MAX_AGE` and `DEFAULT_IDEMPOTENCY_KEY_TTL`.
  `PurgeReport::idempotency_keys_deleted` reports what a sweep removed.

  **Behaviour change, and it is a trade rather than a pure fix.** Nothing ever
  removed an idempotency key. A key is released when the send holding it
  fails, and otherwise kept deliberately — it has to outlive the task it names
  or a late retry would execute the send a second time — so neither the
  retention sweep, which deletes tasks, nor the in-memory store's `task_ttl`
  and `max_capacity`, which bound tasks, touched the key index. A busy
  deployment accumulated one row per keyed send for the life of the database
  or the process.

  What expiring a key costs is exactly what the key was preventing: a retry
  arriving *after* the key expires re-executes the send. That is inherent to
  any expiring idempotency key, and it is why the default is a full day rather
  than something tidier — it has to exceed the longest window in which a client
  might still retry, and this SDK's own `RetryPolicy` is bounded in the low
  tens of seconds.

  The sweep will not delete a key younger than `terminal_max_age`
  (`effective_idempotency_key_max_age` clamps it). A key expiring while the
  task it names is still retained does not produce a replay and does not
  produce a clear "that task is gone" — it produces a *second* task alongside
  the first, and the caller ends up with two ids for one logical send. The
  invariant holds by construction rather than by the operator having read the
  documentation.

  Set either to `None` to keep the previous behaviour. The four SQL tables
  already carried a `created_at` column, defaulted and never read, so no
  migration is needed.

- **`RequestHandlerBuilder::with_inbound_trace_policy`** and
  `InboundTracePolicy` (`Continue` — the default and the previous behaviour —
  `Restart`, `Drop`). The inbound `traceparent` is joined while the
  `CallContext` is built, which is *before* the interceptor chain, which is
  where authentication happens: so on a public endpoint the peer choosing the
  `trace-id` and the sampling bit is, at that moment, anonymous. W3C Trace
  Context §7.2 names the consequences — forged `trace-id` collisions, and an
  attacker deciding what the operator's tracing vendor is billed for — and
  §3.4 names the remedy. Per handler rather than per process, so one process
  serving both a public front gate and an internal endpoint can hold a
  different policy on each.

- **`TaskStore::earliest_event_seq` and `TaskStore::event_log_covers`**, both
  defaulted, so an out-of-tree store compiles unchanged (`STABILITY.md` §4).
  `event_log_covers` is provided rather than implemented per store, so the
  comparison and its off-by-one live in one place. A resubscribe naming a
  position the log has dropped is now served the snapshot rather than a replay
  that silently begins later than asked.

- **`TaskStoreConfig::max_events_per_task`** (default `Some(512)`) and
  `DEFAULT_MAX_EVENTS_PER_TASK`. **Behaviour change:** the in-memory event log
  was unbounded and is now bounded.

- **`metrics::event_append_error`** with `position_conflict` and
  `task_absent`, both reported under `persistence_operation::EVENT_APPEND`
  alongside a warning. A collision whose stored payload matches what was
  offered is a replay rather than a loss and is deliberately not counted.

- **`metrics::MetricsHandle`**, and `with_metrics` on the five directly
  constructed stores, including `TenantAwareInMemoryTaskStore`, which hands it
  to every partition it creates.

- **`InMemoryTaskStore::is_prunable` and `idempotency_key_count`.**

- **`CHECK (seq > 0)`** on `task_events` and `tenant_task_events`. Postgres
  migration 6 adds it to existing databases. SQLite cannot — `ALTER TABLE` has
  no `ADD CONSTRAINT`, so an existing SQLite database keeps the unconstrained
  table; a database the runner builds from scratch gets it.

- **A `trace_context` fuzz target**, and `scripts/check_fuzz_matrix.py`, which
  fails when a fuzz target exists but no runner executes it. A target reaches
  CI through three files and only the workflow matrix has no build error
  behind it; `trace_context` itself shipped registered in `fuzz/Cargo.toml`
  and absent from the matrix, which is the drift the gate caught on its first
  run.

#### Fixed

- **`TraceContext::parse` could abort the process.** A `traceparent` whose
  55th byte falls inside a multi-byte character panicked, and the root
  manifest sets `panic = "abort"`, so a peer-supplied header on the public
  `on_send_message` path was process death rather than a rejected request.

- **A version-`00` `traceparent` with a trailing field was accepted.** W3C
  §3.2.2.2 gives version 00's grammar exactly; the lenient trailing-field rule
  is §3.2.4's and applies only to a *higher* version.

- **Reserved `trace-flags` bits were propagated.** W3C §3.2.2.5.2 and §4.3
  both require a vendor to zero unknown bits on outgoing requests. `flags()`
  still reports the byte as received, for diagnostics.

- **An oversized or over-32-member `tracestate` was discarded whole**, taking
  every vendor's state with it. W3C §3.3.1.5 requires truncating *entries* —
  those over 128 characters first, then from the end — which is what happens
  now. `MAX_TRACESTATE_LEN` is public and documented because §3.3.1.5 requires
  the maximum to be documented.

- **`TracePropagationInterceptor`'s "never overwrite" guard was
  case-sensitive**, and `http`'s builder appends rather than replaces, so a
  user interceptor setting `Traceparent` put two `traceparent` fields on the
  wire and which one the peer joined was its parser's choice. `tracestate` had
  no guard at all.

- **An append that wrote nothing was discarded in silence.** All four SQL
  stores dropped `rows_affected()` from their `ON CONFLICT ... DO NOTHING`
  insert, and both in-memory stores returned `Ok(())` for a task they no
  longer held.

- **The SQLite retention orphan sweep ran only when a purge deleted a task**,
  so rows stranded by a partly-failed purge survived to be replayed to
  whoever next reused the task id. It runs on every sweep now, batched.

- **A negative stored event position was made positive** by `unsigned_abs`
  instead of refused.

- **`prune_empty_tenants` destroyed live idempotency indexes.** It decided on
  `count()`, which counts tasks, and an idempotency key deliberately outlives
  the task it names. A partition whose tasks had all been swept still held a
  live index, and pruning it discarded that index — so the next retry carrying
  one of those keys found no claim and executed the send a second time, which
  is the one outcome a key exists to prevent.

### Breaking Changes

In the `0.x` series a minor release may break and a patch may not
(`STABILITY.md` §2), and each item below is a minor-only change by that list.
The four crates bump in lockstep.

`bindings/a2a-protocol-slimrpc` moves with them, to `0.5.0` on the `0.13`
pins: `SlimRpcServer::builder` takes `Arc<RequestHandler>`, so
`a2a-protocol-server` and `a2a-protocol-types` are *public* dependencies and
its requirement on them is a tight `0.13`. That bump is in this release rather
than after it because the binding depends on the workspace by `path` as well
as by version, so `^0.12` stops resolving the moment the crates read 0.13.0.
Publishing it remains the separate step `RELEASING.md` describes — and the
one that has never been run.

- **`RequestContext` is `#[non_exhaustive]` and carries a new `call_context`
  field.** Measured with `cargo semver-checks check-release -p
  a2a-protocol-server --baseline-version 0.12.1` **at the time that bullet was
  written**: 196 checks, 195 pass, and one fails —
  `struct_marked_non_exhaustive` on `RequestContext`. That "exactly one" was
  never right even then: the third bullet below renames a public field on
  `PurgeReport`, which is public, unconditionally reachable and not
  `#[non_exhaustive]`, so `struct_pub_field_missing` must fire too. Treat the
  figure as the record of one run, not as evidence that the list is complete —
  the list is complete because each entry below was derived from the diff. The
  added field is not a second finding, because once a struct is
  `#[non_exhaustive]` an added field is no longer separately observable.

  What breaks: constructing a `RequestContext` with a struct literal from
  outside `a2a-protocol-server`. Nothing in this repository did —
  `git ls-files '*.rs' | xargs grep 'RequestContext {'` finds only the
  definition and function signatures, and all 19 construction sites already
  use `RequestContext::new`. `book/src/deployment/testing.md` did teach a
  literal, in a `rust,ignore` fence that nothing compiled; it now uses `new`.

  Migration: replace `RequestContext { message, task_id, context_id, .. }`
  with `RequestContext::new(message, task_id, context_id)` followed by
  `.with_stored_task(..)` / `.with_metadata(..)` for anything else you set.

  The attribute is deliberate rather than incidental. This type grows, and
  every previous growth would have been a break for a literal nobody writes;
  `#[non_exhaustive]` is what makes this the last time.

- **`EventQueueReader::read` now yields a `StreamEvent`, not a
  `StreamResponse`.** The event travels the queue with the position it holds
  in the task's log: `StreamEvent { seq: Option<u64>, event: StreamResponse }`.
  `Reattached::Channel` and the persistence channel carry the same type.

  What breaks: any code that matched on the result of `read()` directly. The
  migration is `.event` for a value, or `.map(|e| e.event)` for the `Result`:

  ```rust,ignore
  // before
  while let Some(Ok(StreamResponse::StatusUpdate(u))) = reader.read().await { .. }
  // after
  while let Some(Ok(ev)) = reader.read().await {
      if let StreamResponse::StatusUpdate(u) = ev.event { .. }
  }
  ```

  Why it is a type change and not an accessor: the position has to be the
  *same number* the store wrote, and the only way to guarantee that is to
  assign it once and carry it. See the `Added` entry below.

- **`PurgeReport::journal_orphans_deleted` is now `orphan_rows_deleted`.**
  The sweep reclaims two side tables now, not one — the artifact journal and
  the event log — so the old name described half of what the number counts.
  Renamed rather than kept and widened: a field whose name names one of its
  two sources is read as the count for that source.

  Migration: rename the field at the read site. Its meaning is unchanged for
  anyone who had only the journal, and it is still normally zero (a non-zero
  count means rows outlived their task, which happens on a pool without
  `foreign_keys=ON`).

- **`build()` refuses an agent card it would have to edit.** The builder
  advertises the extensions this server can honour by appending to
  `capabilities.extensions`, and `canonicalize_card` strips only
  `signatures` — so those entries are inside the bytes a signature covers. A
  card signed before they existed was served canonicalizing to bytes nobody
  signed, and every client that verified it failed, silently, because nothing
  on the server side reads the signature again. Measured on the default
  configuration: the operator's card canonicalized to 360 bytes, the card
  actually served to 767.

  `book/src/deployment/security.md` tells operators to sign the card and hand
  it to `RequestHandlerBuilder`, so the instruction and the code together
  produced the failure.

  What breaks: `build()` now returns `InvalidParams` for a card that carries a
  signature and does not already declare every extension this server
  advertises. A card that declares them is served byte-identical to what was
  signed; an unsigned card, or one with an empty signature list, is
  advertised on as before.

  Migration: declare the extensions on the card *before* signing it —
  `IDEMPOTENCY_EXTENSION_URI` when the configured store reports
  `supports_idempotency()`, and `FAILURE_EXTENSION_URI` always — then sign the
  card that results. The security page shows the order in a block that
  compiles.

- **`IdempotencyClaim` and `KeyError` are `#[non_exhaustive]`.** Both were
  exhaustive public enums that callers must `match`. Every out-of-tree
  `TaskStore` matches on the first, and the outcomes it can report are not a
  closed set — an expiry, or an "in flight, wait" that would let a second
  caller be told to retry instead of handed a `TaskNotFound`, are both
  plausible additions. `STABILITY.md` §4 promises new variants will not break
  external implementations, and without the attribute they would have. Their
  siblings added in this same release — `RecordedEvent`, `StreamEvent`,
  `Outcome`, `CheckResult`, `TraceContextError` — were all marked; these two
  were missed.

  What breaks: a `match` on either from outside its crate needs a wildcard
  arm. `IdempotencyClaim` is also re-exported from `store` now, where its
  sibling `RecordedEvent` already sat — it is the return type of a public
  `TaskStore` method and was reachable only at
  `store::task_store::IdempotencyClaim`.

- **`FailureClass::ALL` is `&'static [FailureClass]`, not `[Self; 5]`.** The
  enum is `#[non_exhaustive]` so that adding a variant is not breaking, and
  then exported a constant whose *type* encoded the variant count — so the
  sixth variant would have changed `ALL`'s type and broken every caller that
  bound it, which is the exact break the attribute three lines above it
  promises not to inflict.

  Migration: `for c in FailureClass::ALL` becomes `for &c in
  FailureClass::ALL`; `.into_iter()` becomes `.iter().copied()`.

- **A keyed `message/send` is retried only against a peer known to honour the
  key.** `RetryTransport` treated any valid key in `Message.metadata` as
  making the send retryable, on the stated grounds that a server without the
  extension "refuses a keyed send outright". That is true of this SDK's
  server and of nothing else: the key rides in `Message.metadata`, which A2A
  defines as free-form, and the extension is deliberately not part of A2A
  v1.0 — a conformant Python, Java, Go or JavaScript server ignores it and
  runs the send. Retrying there starts a second task, which is the exact
  double execution `is_idempotent_method` exists to prevent. There is no
  handshake that would fix it: A2A's `A2A-Extensions` header is the server
  reporting what it activated, not a demand a server must reject.

  What breaks: a client built for a bare endpoint no longer retries a keyed
  send after an ambiguous failure. `ClientBuilder::from_card` reads the
  advertisement off the agent card and enables it automatically;
  `ClientBuilder::with_peer_honouring_idempotency(true)` asserts it for a peer
  you know implements the extension.

- **`message.id` is validated at ingress.** It is now checked for being
  non-empty after trimming, and bounded in length. It was checked nowhere,
  while being stored verbatim in `idempotency_keys.message_id`, keying history
  de-duplication, and interpolated into the error a caller sees when an
  idempotency key is held by a different message. A spec-invalid empty id was
  accepted, stored, and echoed in task history, where an empty id collides
  with every other empty id.

  **The length bound is not `max_id_length`**, and the difference matters.
  That field is documented as covering task and context ids, which are
  commonly short and often chosen by the deployment; a message id is minted by
  the client and is conventionally a UUID — 36 characters, which is what
  `Message::id`'s rustdoc tells a caller to send. Holding both to one number
  meant a deployment tightening `max_id_length` below 36, a reasonable thing
  to do for the two ids it was documented to cover, refused *every* message
  from a conformant client. `HandlerLimits::effective_max_message_id_length`
  is `max_id_length` floored at `MIN_MESSAGE_ID_LENGTH` (36), so the bound
  still tightens with the field but never past what the protocol requires.

  What breaks: a send whose `messageId` is empty, whitespace-only, or longer
  than `effective_max_message_id_length()` is rejected with `InvalidParams`
  instead of accepted.

### Added

- **SSE frames carry an `id:`, and `Last-Event-ID` resumes from it.** This is
  what the event log was for. A reconnecting client sends back the position of
  the last frame it saw and receives exactly what it missed, after the `Task`
  snapshot and before the live stream.

  The measured problem: in `mcp-bridge`'s demo the sample agent emits three
  progress steps 120 ms apart and the caller sees **one**, because a snapshot
  is a fold and a poller only ever observes the latest one. Three events
  arriving as one is not a rendering detail — it is the intermediate states
  being unobservable.

  **The position is assigned once, in `InMemoryQueueWriter::write`, and
  carried on both channels.** That is the whole design. The writer is the
  single fan-out point — it feeds the persistence channel that writes the log
  and the broadcast channel that feeds SSE — so the `id:` a subscriber reads
  and the `seq` the store holds are the same value by construction. The
  alternative, counting frames at the SSE layer, agrees with the log right up
  until the first lagged consumer, snapshot frame, or failed append; and a
  resumption offset that is off by one drops an event with nothing to
  indicate it. `tests/event_log_tests/resumption.rs` asserts the two
  sequences are equal, end to end through `build_sse_response`.

  **Frames the server synthesized carry no `id:`** — the `SubscribeToTask`
  snapshot, and the terminal frame the reattach hook rebuilds from stored
  state. Neither is in the log, so neither can be replayed from, and giving
  either one a position would hand the client an offset that loses an event.

  **A continuation resumes the numbering.** The queue is per-turn but the log
  is per-task, so the writer is seeded from `last_event_seq` before the
  executor starts. Without it a second turn would restart at 1 and, because
  appends are idempotent *by position*, have every one of its events silently
  swallowed as a replay.

  **The offset is client-supplied, so it is bounded and parsed.** Replay is
  capped by the new `HandlerLimits::subscribe_replay_limit` (default 1,000),
  and truncation is not loss: each replayed frame carries its own `id:`, so a
  client continues from the last one it got. A `Last-Event-ID` that is not a
  position is ignored rather than rejected — a client may echo one back from
  an unrelated stream, and failing the reconnect over it would turn a
  harmless mistake into a dropped connection. A store that keeps no log is
  the same: the header is ignored and the client gets the snapshot, which is
  the pre-resumption behaviour.

  `CorsConfig`'s default `Access-Control-Allow-Headers` gains `last-event-id`
  for the same reason `a2a-version` and `a2a-extensions` are already there: a
  cross-origin client that cannot send the header cannot resume.

  WebSocket, gRPC and SLIM streams are unchanged. Resumption is the SSE
  binding's `id:`/`Last-Event-ID` pair; inventing a spelling for the others
  would be a protocol extension this server made up. Note that
  `bindings/a2a-protocol-slimrpc` is outside the root workspace and takes
  `RequestHandler` as a public dependency, so `cargo check --workspace` does
  not compile it — its `event_stream` adapter needed the same one-line change
  and only the binding's own gates caught it.

- **An append-only event log, so what the agent emitted is recorded and not
  just what it folded into.** `TaskStore` gains `supports_event_log`,
  `append_event`, `last_event_seq` and `read_events`, all defaulted.

  A task's state is a fold: a stored snapshot folded together with deltas.
  Issue #130 happened because that fold was wrong in a way nothing could
  observe — artifacts from one task appeared on another, and the only record
  was the folded result, which is to say the bug itself. There was nothing to
  check it against. This adds the thing there was nothing to check against.

  The snapshot stays and stays authoritative for reads; the log is a
  parallel, ordered record of the events themselves. That makes a wrong
  snapshot *detectable*, gives a reconnecting subscriber the events it missed
  rather than a fold it cannot interpret, and is the substrate anything like
  a signed execution receipt would need.

  **`seq` is a position, not a counter.** The same `(task_id, seq)` written
  twice leaves one event, so a retried or overlapping append is safe without
  a read first — the property `sqlite_store::journal` already relies on, for
  the same reason. That makes `last_event_seq` load-bearing rather than
  convenient: a task parked at `input-required` and then continued gets a
  *second* processor, and a `seq` restarting at 1 would collide with
  positions the first one wrote, so the continuation's events would be
  silently dropped. Both processors seed from the store instead.

  **Both fold paths record, which is not one site but two.** A blocking send
  folds in `sync_collector`; a streaming one folds in the background
  processor. A log covering only streaming sends would be a history whose
  completeness depended on which method the caller happened to use. Each
  records *before* folding, so what the agent emitted is durable before the
  state derived from it is.

  A failed append is logged and counted under the new `event_append`
  persistence-error label, never fatal. The log is a record of the run, not a
  precondition for it — and a gap is visible, because the sequence skips a
  position.

  All four methods default to refusing rather than succeeding. A custom store
  that does not implement them reports no log, which is inconvenient;
  defaulting `append_event` to `Ok(())` would advertise a log that silently
  lost every event, which is worse. `supports_event_log()` is what a caller
  checks rather than discovering it by getting nothing back.

- **The event log is backed by every store this crate ships**, not only the
  in-memory one: `SqliteTaskStore`, `PostgresTaskStore`,
  `TenantAwareSqliteTaskStore`, `TenantAwarePostgresTaskStore` and
  `TenantAwareInMemoryTaskStore` all report `supports_event_log() == true`.

  Two tables, `task_events` and `tenant_task_events`, created both by the
  migration runners (`SQLite` migration 7, `PostgreSQL` migration 5) and by
  each store's `from_pool` DDL. Both paths, because a store built by one of
  them and not the other would still answer `supports_event_log() == true` —
  the flag is a property of the type, not of the schema — so the failure
  would be an empty history rather than an error at startup. That is the
  mistake the artifact journal shipped with, and there is now a test for each
  path on each backend.

  The tenant tables are keyed `(tenant_id, task_id, seq)`. Task ids are
  caller-supplied, so two tenants may legitimately use the same one; an
  unscoped log would hand one tenant's resuming subscriber the other's
  messages, which is a cross-tenant read of message content rather than a
  missed deduplication. Unlike `tenant_idempotency_keys` these tables *do*
  carry `ON DELETE CASCADE`: a key that outlives its task is the safe
  direction, an event that outlives its task is not.

  Deletion is explicit as well as cascaded, and the `SQLite` retention sweep
  reclaims orphans by anti-join, because `ON DELETE CASCADE` only fires with
  `foreign_keys=ON` — which this crate's own pool sets and a pool handed to
  `from_pool` may not.

- **A conformance harness for `AgentExecutor`, behind the `conformance`
  feature.** The TCK grades servers. Nothing graded the thing an adopter
  actually writes, and the paths they get wrong are the awkward ones —
  cancellation arriving mid-work, a parked task reported as an error, an
  event emitted after a terminal status. Those are exactly the cases people
  skip when writing tests by hand.

  `conformance::check(Arc::new(MyExecutor)).await.assert_pass()` drives the
  executor against a real event queue — no server, no ports, no model — and
  grades thirteen protocol invariants that hold for any agent whatever it
  does: it ends in a terminal or interrupted state, its transitions are legal,
  nothing follows a terminal status, artifacts carry ids, parking is not
  reported as an error, it returns within the harness's time limit, it
  observes a cancellation token set before it starts *and* one that arrives
  during its first write, it does not report cancellation as a failure,
  `cancel` leaves subscribers a terminal state, and it does not panic — on
  the normal run, the cancelled run, or the mid-run cancellation. It says
  nothing about whether the agent is any good at its job.

  **Three rules keep the score honest, and each is the repository's own.** A
  check that did not apply is *not graded* rather than counted as a pass — a
  report that grades nothing fails rather than reporting full marks, which is
  the failure `tck/`'s README records having shipped once. Every verdict
  carries its reason, so a failure says what was observed and why it matters,
  not just which check. And the executor runs inside `tokio::spawn`, so a
  panicking executor is a graded failure rather than a panic that takes the
  adopter's whole test run down with it.

  Its own tests are deliberately broken executors, each breaking one invariant
  (or, where an executor genuinely breaks two, naming both), asserting the
  harness names those and no others. A conformance
  harness whose failures are untested is a gate that cannot fail, which is
  the thing this repository checks for everywhere else — and the discipline
  paid immediately: the first draft of one fixture broke two invariants
  rather than one, and the harness was right where the test was wrong.

  No new dependencies. Documented on `book/src/deployment/testing.md` with a
  compiled example.

- **A failed task says why, as a class a caller can `match` on.** Shipped as
  the declared extension `https://a2a-rust.com/extensions/failure/v1`, so
  servers stay conformant and the TCK is unaffected.

  A failed task was `TaskState::Failed` plus prose written by whoever wrote
  the executor, which left a caller matching English for decisions that are
  genuinely different: never retry and fix the request; retry with backoff;
  stop and escalate to a person; retry only with more budget. An orchestrator
  without those either retries what can never succeed or abandons what would
  have worked on the second attempt.

  `a2a_protocol_types::failure` holds `FailureClass` — `InvalidRequest`,
  `Transient`, `PolicyRefusal`, `BudgetExhausted`, `Internal` — with
  `is_retryable()` and `needs_human()`, plus `set_class` / `class_of`.
  `Task::failure_class()` reads it off the status message. The prose stays
  exactly where it was, for the human reading the incident.

  **Most classification is automatic and its limit is stated rather than
  papered over.** When an executor returns `Err`, the server maps the error
  code — which can only ever produce `InvalidRequest` or `Internal`, because
  no `ErrorCode` carries the meaning "transient" or "refused on policy".
  Guessing `Transient` there would tell callers to retry things that can
  never succeed. An executor deadline is `BudgetExhausted`, since a deadline
  is a bound that was hit rather than an agent that broke. The two classes no
  error code can express need the agent itself, which is what
  `EventEmitter::fail(class, reason)` is for.

  `BudgetExhausted` is deliberately **not** `is_retryable()`: the identical
  request hits the identical bound. It is retryable with a larger budget,
  which is a different request. Asserted in its own test so the distinction
  cannot be quietly relaxed into "retry anything that is not the caller's
  fault".

  An unrecognised class from a newer peer reads as `Internal` rather than as
  an error. A peer that classifies more finely must not have its failures
  become unreadable, and "something went wrong at the agent" is true of every
  class a future version could add.

  Advertised unconditionally on the card, unlike idempotency, whose
  advertisement is gated on the store: every server classifies, because the
  classification happens in the server's own failure path. Never `required`,
  and an operator's own declaration is left alone.

  New book page, `book/src/client/failure-classes.md`, with both examples
  compiled and run as doctests.

- **W3C trace context crosses an A2A hop.** The defining property of A2A is
  that work crosses process and organisational boundaries, and until now
  nothing survived the crossing: each agent mints its own task id, so even
  correlating by hand failed at the first hop. One identifier now does —
  `traceparent`, which every other ecosystem already speaks.

  `a2a_protocol_types::trace_context` is the wire format: `TraceContext`
  with strict [W3C](https://www.w3.org/TR/trace-context/) parsing, the
  `traceparent`/`tracestate` header names, and `child` / `child_bytes` for
  deriving the next hop. It mints nothing — this crate depends on `serde` and
  `serde_json` and has no random source — so a caller supplies the identifier
  bytes, the same call the `Artifact::new` id already makes.

  The server parses an inbound `traceparent`, advances the span, and exposes
  it as `RequestContext::trace_context()`. The client gains
  `TracePropagationInterceptor`, which writes the ambient `CurrentTrace` onto
  every outbound request on the canonical A2A transports — JSON-RPC, REST and
  gRPC. **The `websocket` transport is the exception, and cannot be
  otherwise:** it carries headers only on the HTTP upgrade, and a
  `traceparent` fixed at connect time would report every request on that
  connection as the same span, which W3C §3.4 forbids. A dropped trace header
  is reported once per connection at `warn`; a dropped *credential* header is
  still reported on every call, because that one can be fixed by reconnecting.
  `CurrentTrace::scope` sets it and
  `CurrentTrace::start_root()` begins a chain. The indirection through a
  task-local is deliberate: a client is built once and reused for many
  delegated calls, so a trace fixed at construction would label all of them
  with the first request's.

  Three decisions worth stating, because each could reasonably have gone the
  other way. **The server propagates and never invents** — `None` from
  `trace_context()` is evidence that the caller was not tracing, not a gap in
  the plumbing, and a chain that wants a trace starts one at its first hop.
  **A malformed `traceparent` is dropped, not repaired** — uppercase hex, an
  all-zero id, a bad width: attaching work to a guessed-at trace is a wrong
  answer where a missing trace is only an absent one. **An explicit header on
  the request wins over the ambient scope**, because silently replacing it
  would move the callee into a different trace than its caller asked for.

  This is propagation, not tracing. Nothing records a span, measures a
  duration or exports anything, and the `otel` feature is still metrics-only.
  What it buys is that whatever *does* record spans can stitch the hops
  together — across languages, since `traceparent` is a wire format and the
  interoperability kit already runs Python, JavaScript, Go and Java agents.

  Proven over a real socket rather than asserted: `tests/trace_propagation_e2e.rs`
  drives two chained agents and checks that the trace id holds across both
  hops while each hop advances the span — the two ways a propagator can be
  wrong while looking right.

- **An executor can see who called it.** `RequestContext` gains
  `call_context`, and with it five accessors: `caller_identity()`,
  `tenant()`, `http_header(name)`, `activated_extensions()` and
  `request_id()`.

  The handler already built a `CallContext` carrying all of this and handed
  it to the interceptor chain — then dropped it. `build_request_context` took
  a message, two ids and a metadata blob, so the seam was severed by
  construction. The consequence was not cosmetic: an executor could not
  enforce "only this tenant may invoke this skill", which rules out a large
  class of real deployments, and the only channel left for anything
  caller-specific was `Message.metadata` — which the *caller* writes, so it
  is not a fact about the caller at all.

  Every accessor returns `None` rather than a default when nobody said.
  An executor that refuses anonymous work needs `None` to mean "nobody
  established an identity", never a value that reads like an answer.
  `call_context` is `None` when an executor is driven directly, as a unit
  test or a conformance harness does, rather than being given a synthetic
  context claiming a call that never happened.

  `CallContext` also gains `tenant()` and `with_tenant()` — private field,
  so additive — populated from the tenant scope the handler has already
  entered.

- **Constructors for `Message`, `Task` and `MessageSendParams`, and a
  `# Construction` pointer on the three `agent_card` structs.** Measured on
  this tree, the workspace holds 105 `Message` struct literals, 75
  `MessageSendParams` literals and 82 `AgentCard` literals. `Message` had two
  inherent methods and no constructor; `Task` and `MessageSendParams` had no
  `impl` block at all, so sending one line of text cost a ten-line `Message`
  literal — five of its eight fields written as `None` — inside a four-line
  `MessageSendParams` literal, and reading the answer back out cost a
  hand-written `artifacts → first → text` walk that six files under
  `examples/` had each written separately.

  `Message::new`, `user`, `agent`, `user_text`, `agent_text`, and `with_*` for
  all five optional fields. `MessageSendParams::new` plus `with_tenant`,
  `with_configuration`, `with_metadata`. `Task::text` and `Task::texts`.
  Nothing is breaking: every field stays public and every literal still
  compiles.

  The id stays a parameter rather than something the constructor invents.
  `a2a-protocol-types` depends on `serde` and `serde_json` and nothing else,
  so it has no random source, and a clock-derived id is not unique under
  concurrency — the same reason
  [`Artifact::new`](https://docs.rs/a2a-protocol-types) already takes one.

  `Task::text` skips artifacts that carry no text rather than stopping at the
  first, so it answers where the hand-written `artifacts.first()` chain
  returns `None`. That divergence is deliberate, documented on the method, and
  asserted in its test; it is the rule `Message::text` already applies across
  parts.

  The `# Construction` sections exist because discoverability, not absence,
  was the `AgentCard` problem: the twelve `with_*` builders have shipped since
  0.10 in `agent_card/builders.rs`, one file over from the field definitions,
  and three examples written in a single session on 2026-09-19 each reached
  for a literal instead — 38, 43 and 45 lines, verified by counting the three
  functions.

- **Client-supplied idempotency keys on `message/send`, as a declared
  extension.** A send whose connection drops after the request bytes are on the
  wire is ambiguous: the task may exist or it may not. The client rightly
  refuses to retry it — `message/send` creates server-side state — which left a
  caller with no recovery but to list a context's tasks and pattern-match
  message content to guess whether the send landed. With a key the server
  deduplicates on, a retry returns the task the first attempt created, in
  whatever state it reached, and the agent runs exactly once.

  Not part of A2A v1.0, and shipped as
  `https://a2a-rust.com/extensions/idempotency/v1` so servers stay conformant
  and the TCK is unaffected. `Message.extensions` is a list of extension URIs
  and cannot carry a value, so the key travels in `Message.metadata` under
  `a2a-rust.com/idempotency-key` while `extensions` declares the URI.
  `a2a_protocol_types::idempotency` holds the constants, `set_key`, `key_of`
  and `validate_key`.

  Keys are scoped to the **tenant**, not the context: a retry of a send whose
  `context_id` the server assigned would otherwise mint a fresh context and
  miss the deduplication, which is the case the key exists for. They must be
  unguessable — within a tenant the key is the handle to the task — so
  `MIN_KEY_LEN` rejects the short mistakes and the character set is restricted
  to ASCII alphanumerics and `-`, `_`, `.`, `:`, because a key reaches store
  keys and log lines.

  Three outcomes, and the third is the one worth stating. A free key is
  claimed; the same message replays to the first task without executing
  anything; a *different* message on the same key is refused with
  `InvalidParams`. A genuine retry resends the identical message, so a
  differing message id means the key was reused across two distinct sends, and
  returning the first task there would answer a message that was never sent —
  a silent wrong answer, which is the failure a caller can least detect.

  The claim sits under the per-context lock, after everything that can reject
  the request on its own terms and before the send's first side effect:
  claiming later would let two racing duplicates each lease an event queue,
  claiming earlier would burn a key on a request that was never going to run.
  Every failure path after it releases the key, the discipline the queue lease
  and cancellation token already follow — a key held by a send that never
  created a task would make the caller's own retry replay to a task that never
  existed. A streaming replay attaches to the original task's live queue as
  `SubscribeToTask` does, rather than returning a snapshot and closing, which
  would read as a task that had finished emitting.

- **Agent cards advertise idempotency support, derived from the store.** The
  builder adds the extension exactly when `TaskStore::supports_idempotency()`
  is `true`, so the card and the behaviour cannot drift: swapping in a store
  without the index removes the advertisement in the same change. An operator's
  own declaration is left untouched, `required` flag included; the generated
  entry is never `required`, since a client that sends no key must not be
  locked out. A server whose store cannot honour a key refuses a keyed send
  rather than running it undeduplicated, so the failure mode is a loud refusal
  and never silent at-least-once delivery.

- **Every bundled store honours keys**: in-memory, `SQLite`, `PostgreSQL` and
  the tenant-aware variant of each. The SQL stores claim inside one
  transaction — `INSERT ... ON CONFLICT DO NOTHING` picks the winner and the
  loser reads the holder under `SQLite`'s write lock or Postgres's
  `FOR UPDATE` — because a claim that neither inserted nor found a holder must
  never pass for a successful one. The tenant-aware SQL stores key the table
  `(tenant_id, key)` to match `tenant_tasks`: without the tenant in that key
  one tenant's key would collide with another's and the second tenant's send
  would replay to the first tenant's task. No table carries a foreign key to
  the tasks table, deliberately — a cascade would free the key when a retention
  sweep removed its task, letting that send run a second time.

  `TaskStore` gains `supports_idempotency`, `claim_idempotency_key` and
  `release_idempotency_key`. All three have defaults, so existing custom stores
  keep compiling; the defaults report no support rather than a successful
  claim, so forgetting to implement them cannot produce a send that quietly
  runs twice.

- **A keyed send is retried on the ambiguous failure it was built for**
  (client). `SendMessage` and `SendStreamingMessage` remain non-idempotent by
  method, but a send carrying a valid key now joins the idempotent set for that
  call, so a dropped connection is retried instead of surfaced. The safety
  argument needs no capability negotiation: either the key is honoured and the
  retry is deduplicated, or the server refuses the keyed send outright rather
  than running it undeduplicated. The key is validated rather than merely
  found, so the predicate answers "this send will be deduplicated" and not
  "something is present under that name".

- **Book: "Idempotent Sends"** (`book/src/client/idempotency.md`), covering how
  to attach a key, how to choose one, what the three outcomes mean, how to tell
  whether a server honours keys, and the two obligations on a custom store —
  an atomic claim, and never deleting a key with its task. Every Rust block on
  the page is compiled and run as a doctest.

- **Tool calling, in `examples/rig-agent`.** Nothing in this repository showed
  it: `grep -ril 'tool_call\|ToolCall\|tool_choice\|function_call'` over
  `examples/`, `crates/` and `book/src/` returned zero files, and the rig
  example's own comment said "no tools, no history". An A2A SDK whose examples
  never call a tool cannot show an adopter how to build an agent that does
  anything, which is the gap this closes.

  It is deliberately **not** in the crates. A2A has no tool concept — the
  protocol carries messages, tasks and artifacts *between* agents and says
  nothing about what happens inside one — so tool calling belongs between an
  executor and its model, and `examples/rig-agent/src/tools.rs` imports no
  `a2a-protocol-*` type at all. Many model turns, one A2A task. Putting a tool
  abstraction in `a2a-protocol-server` would make it a framework and put it on
  a model-provider's release treadmill; hosting one costs neither.

  `src/agent.rs` writes the loop out rather than delegating to `rig-agent`'s
  runner, because the loop is the part an adopter has to get right. Three
  things it gets right, each a bug if the shape is copied without them: the
  catalogue goes on *every* request, since a provider holds no state between
  turns; a tool error is returned to the model as that call's result rather
  than failing the task, so an unknown service is recoverable; and the loop is
  bounded (`MAX_TURNS`, 6), because unbounded it holds the A2A task open until
  the server's hour-long executor timeout. `src/tools.rs` declares two tools
  over a fixed inventory — one with no arguments, one with a required string —
  which covers both JSON Schema shapes a provider must encode and, because a
  model must discover the inventory before querying it, produces a *two*-round
  loop. One round is the case that still works when the loop is written wrong.

  When tools ran, the task carries a second `tool-trace` artifact naming each
  call and its result. Eleven new tests drive the loop against a scripted
  model that records what it was sent, covering the tool-error and turn-limit
  branches a live provider reaches only by chance.

- **`examples/mcp-agent` — tools over MCP, agents over A2A.** The A2A project's
  own guidance is that the two protocols compose: *"A2A handles inter-agent
  collaboration and MCP handles tool integration."* Nothing showed what that
  looks like, so this does. The agent spawns an MCP server as a child process,
  asks it what it offers with `tools/list`, builds its catalogue **and its
  agent-card skills** from the answer, and calls it with `tools/call`. A2A
  never carries a tool call; MCP never carries a task; they meet in one
  process and nowhere else.

  It is `rig-agent` with exactly one thing changed, which is the point. Diff
  the two `src/agent.rs` files: the loop is the same — same three rules, same
  bound — and only the tool source moves. Nothing in the crate knows what the
  tools are, so `MCP_SERVER_BIN` repoints it at any other MCP stdio server, in
  any language, with no code change.

  The distinction the bridge exists to keep is between a tool that **ran and
  refused** and a session that is **gone**. The first is information for the
  model — it can read the reason and try something else — so it returns as a
  tool result and the A2A task stays alive. The second cannot be re-prompted
  away, and answering regardless would hand the caller a guess dressed as a
  researched answer, so the task fails. The model-unreachable fallback the
  sibling examples use is deliberately not extended to a dead MCP server, and
  an agent that cannot reach its tools at startup refuses to start rather than
  serve a card advertising them.

  Built on `rmcp` 3.4, the official Rust MCP SDK, for both halves: the client
  and the bundled `mcp-tool-server` binary. Fourteen tests, and the split
  matters — ten drive the bridge over an in-process `tokio::io::duplex` pipe,
  which is real MCP framing and reaches the branches (a refusal, a session
  dying mid-call, a model that never stops calling); four spawn the *shipped*
  binary the way a real MCP client does, which is what proves its schemas
  reach the wire from the `JsonSchema` derive and that its `main` writes
  nothing but MCP to stdout. A stray `println!` there desynchronizes every
  frame, and no in-process test would notice.

  Verified end to end against llama.cpp `b23701f` serving Qwen3-1.7B: both
  demo questions answered over A2A with tools fetched over MCP, including the
  recovery path — the server refused an unknown service, the refusal crossed
  back as a tool result, the model read the hint the server wrote and called
  the tool it named. Worth noting from that run: the first question took *two*
  MCP round trips where the same question against `rig-agent`'s in-process
  tools took one. The tool count is the model's choice, not a property of the
  code, which is why the bound is not optional.

- **`examples/mcp-bridge` — the reverse direction: an A2A agent, callable as
  an MCP tool.** `mcp-agent` serves A2A agents; this serves everyone else. An
  MCP client needs no A2A library, no A2A concept and no new code to call a
  remote agent — it needs a command in its server list. The bridge discovers
  the agent card at startup, publishes one MCP tool per advertised skill with
  the agent's own descriptions, and forwards each call as an A2A task.

  More of A2A survives the crossing than expected, and that is the finding
  worth recording. `Completed` is a tool result and `Failed`/`Rejected`/
  `Canceled` carry `isError`, which is unsurprising. What is new is that
  MCP's `2026-07-28` specification added long-running tasks (SEP-2663), so an
  A2A task now maps onto an MCP task rather than a blocked request: the
  caller polls `tasks/get`, the agent's own status messages cross to it, and
  `tasks/cancel` reaches the agent as `CancelTask`, cooperative on both
  sides. Before that extension this bridge could not have been built without
  lying about one side or the other.

  Three asymmetries are stated where they happen rather than papered over.
  A2A skills carry no argument schema — A2A's calling convention is a
  `Message` — so every published tool takes one string and the skill's
  description is what steers a caller's model; inventing a schema per skill
  would publish a contract the agent never agreed to. A2A has no skill
  selector, so the chosen tool travels as message metadata the agent may
  route on or ignore. And `input-required` is reported rather than bridged:
  MCP has a counterpart, but wiring it needs the bridge to hold an A2A task
  id across MCP calls, and a half-built version would strand tasks with no
  way to answer them, so a paused task returns an error saying what the agent
  is waiting for — a caller can tell "waiting" from "broken".

  It refuses to start when the agent is unreachable, when the card advertises
  no skills, and when two skill ids collide as MCP tool names (naming both).
  An MCP server that came up with an empty or wrong tool list is
  indistinguishable, to its caller, from an agent with nothing to offer.

  Fifteen tests. The mapping is pure, so its awkward cases are asserted on
  values; the bridge needs a real agent, so those tests start one on
  localhost and drive the bridge with a real MCP client over an in-process
  pipe. `bridge-demo` covers the child-process spawn by standing up a sample
  agent, spawning the bridge against it and acting as an MCP client — no
  model, no network, deterministic.

  Its transcript is in the README, and it earns its place: the sample agent
  echoes back the skill hint, proving the metadata arrived; the agent's own
  progress message crosses two protocols into `tasks/get`; and it arrives
  *coarsened* — three agent steps 120 ms apart, of which the caller sees one,
  because the bridge polls at 250 ms. The cost of polling rather than
  streaming, visible in the output rather than discovered later.

- **The OTLP export sequence is documented.** `book/src/deployment/observability.md`
  covered metrics conceptually and the instrument catalogue, and said nothing
  about how to make bytes leave the process. It now carries the call order as
  a compiled example, and the four things that bite:

  `init_otlp_pipeline` **must be called from inside a Tokio runtime** — tonic
  spawns onto the ambient runtime while building the channel — and since the
  release profile sets `panic = "abort"`, getting that wrong aborts the
  process rather than returning an error. The transport is **gRPC only**, so
  `OTEL_EXPORTER_OTLP_PROTOCOL` has no effect and an endpoint pointed at a
  collector's `:4318` produces silence. **`OTEL_SERVICE_NAME` is overridden**
  by the `service_name` argument: `Resource::builder()` installs
  `EnvResourceDetector`, which reads it, and `.with_attributes` then
  overwrites what it found — the opposite of what the OTel
  environment-variable specification prescribes. And the install is
  **last-write-wins, process-wide**.

  A table now says which `OTEL_*` variables reach the exporter (endpoint,
  headers, timeout) and which do not (protocol, service name). `book-tests`
  gains the `otel` feature so the sequence compiles as a doctest rather than
  being an `ignore`d block, which is the same reason `postgres` was added for
  the horizontal-scaling page.

  The catalogue section gains **the Prometheus name for every instrument**,
  measured by rendering the catalogue through `opentelemetry-prometheus`
  0.32.0 rather than derived from the translation rules: `a2a.server.requests`
  arrives as `a2a_server_requests_total`, `a2a.server.latency` as
  `a2a_server_latency_seconds`, and gauges keep their names unsuffixed. The
  dotted names are *correct* — OTel names metrics
  `http.server.request.duration` — and nothing in the book had previously
  said what they translate to, which is the likeliest reason mapping them
  took guesswork.

  Two deviations are now stated there rather than changed, because the page
  calls these names a contract and changing a published contract needs its
  own release and upgrade note: the units are not UCUM (`request` should be
  `{request}`), which the same measurement shows costs nothing observable —
  UCUM and non-UCUM units render byte-identical exposition through this
  exporter — and `a2a.server.latency` should conventionally be
  `a2a.server.request.duration`, which *is* visible, since no OTel dashboard
  template will match `a2a_server_latency_seconds`.

### Changed

- **Seven example files and five book pages now use the new constructors.**
  Net 113 lines removed from `examples/` and 108 from `book/`: `rig-agent`'s 43-line
  `AgentCard` literal, `mcp-bridge`'s 45-line one and `mcp-agent`'s 38-line
  one become chained builders; `hello-agent`'s 16-line send becomes
  `MessageSendParams::new(Message::user("test-msg", parts))` and its
  five-line artifact walk becomes `task.text()`. The examples are what an
  adopter copies, so leaving them on the literal would have shipped the
  ergonomics and taught around them.

  Three cards stopped setting `AgentCard.url`. That field is
  `#[serde(skip_serializing)]` — a served card never carries it, so no client
  could ever read what those agents were setting — and its own doc says to
  publish an address through `supported_interfaces`, which all three already
  did. `rig-agent`'s test asserted on it; it now asserts on
  `supported_interfaces[0].url`, which is the address a client actually sees.

  `examples/mcp-agent` and `examples/mcp-bridge` each carried a `uuid_like()`
  helper — a nanosecond timestamp, not unique under concurrency and
  sequential enough to guess — written to avoid a `uuid` dependency the
  workspace already carries and seven other examples already use. Both now
  take `uuid` and call `Uuid::new_v4()`.

  One book snippet did not compile. `book/src/deployment/testing.md` set a
  `context_id` field on `MessageSendParams`, which has no such field — the
  snippet was inside a `rust,ignore` fence, so nothing ever built it. It is
  now three lines that would.

- **`examples/rig-agent` defaults to `qwen3:1.7b`, not `qwen3.5:0.8b`.**
  Measured 2026-09-19 against llama.cpp `b23701f` with `--jinja` and the
  example's own catalogue: Qwen3.5-0.8B-Q4_0 answers in prose, and forced with
  `tool_choice: "required"` still emits no tool call, running to
  `finish_reason: "length"` after 7,400+ tokens. Qwen3-1.7B-Q4_K_M returns
  `finish_reason: "tool_calls"` on the first request. The sibling LLM examples
  call no tools, so their 0.8B default stays right for them; the README
  records both results so the next person does not rediscover this.

  The end-to-end run is in `examples/rig-agent/README.md` as two verbatim
  transcripts. The first is kept **with the model's arithmetic error intact** —
  it reports an uptime the tool never returned, and the trace line beneath it
  shows the real figure. That is the argument for the trace artifact stated as
  evidence rather than as a claim: grounded and verifiable are different
  properties, and only the second is one the protocol can carry.

### Fixed

- **The idempotency extension's card description carried 30 literal
  spaces.** Someone reflowed a multi-line string literal without stripping
  the indentation, so the description a client reads off the card said "a
  retried&nbsp;&nbsp;…&nbsp;&nbsp;send returns". Noticed while adding the
  failure extension beside it.

- **Three documents said the SDK had no `traceparent`, and now say what it
  does have.** `otel/pipeline.rs`'s "no part of this workspace reads or
  writes W3C `traceparent`", `book/src/deployment/observability.md`'s "There
  are no traces", and the `docs/rust-sdk-assessment.md` comparison row were
  all accurate on 2026-09-19 and were made false by the change above. Each
  now separates the two claims that were being run together: no spans are
  exported, which is still true, and trace context is not carried, which is
  no longer. The assessment row has now been corrected twice in opposite
  directions, which is recorded there rather than quietly overwritten.

- **The spawned executor ran under the empty tenant.** `spawn_executor` used
  a bare `tokio::spawn`, and `TenantContext` is a `tokio::task_local`, which
  a spawned task does not inherit. So every store call an executor made ran
  with `TenantContext::current() == ""` — against a tenant-aware store, that
  partitions the executor's writes away from the request that caused them.
  The handler's own doc comment recorded the mechanism and the measurement
  ("The same probe's executor saw `\"\"`") without the spawn being fixed.

  Two other spawns in the same crate already did the right thing — the
  background event processor and the sync collector both capture the tenant
  and re-enter `TenantContext::scope`. This one now does too.

  The regression test is `the_spawned_executor_runs_inside_the_tenant_scope`
  in `tests/request_context_tests.rs`, and it was checked against the defect
  rather than merely written: with the scope removed it fails naming the
  empty tenant, with it in place it passes.

- **A test's panic hook was silencing every other test in its binary.** Three
  tests — two in `a2a-protocol-server`'s `agent_card/hot_reload.rs`, one in
  `a2a-protocol-client`'s `auth.rs` — wrapped an expected panic in
  `take_hook()` / `set_hook(Box::new(|_| {}))` / `set_hook(hook)`. `set_hook`
  is process-global and libtest runs a binary's tests as parallel threads in
  one process, so each of those windows silenced *every* thread for its
  duration: a genuinely failing test that raced one was listed under
  `failures:` with no `---- stdout ----` section and no message, and the build
  went red with its reason absent from the log, non-deterministically.

  It had already cost something measurable. Three gates in
  `scripts/prove_gates_fail.sh` —
  `cargo test -p a2a-protocol-server --features {sqlite,postgres,auth-jwt}` —
  reported `INCONCLUSIVE` rather than `PROVEN`, because the injected defect
  failed correctly and its panic message never reached the output the prover
  greps.

  The fix is a deletion at all three sites. `catch_unwind` returns the payload
  each test asserts on, and libtest already captures panic output per test and
  discards it when the test passes, so the swap was buying a suppression it
  already had. Measured on the pattern in isolation: an unrelated failing test
  in the same binary lost its marker in 3 of 3 parallel runs with the swap and
  kept it in 3 of 3 without, and the expected panic's own text appeared in 0 of
  3 runs either way.

- **`examples/rig-agent`'s executor was undocumented.** Its doc comment had
  run together with the one below it, so both attached to `SLOW_PREFIX` and
  `RigAgentExecutor` carried none.

- **Three documents claimed OpenTelemetry trace export that does not
  exist.** The `otel` feature is metrics-only: `opentelemetry_sdk` is
  compiled with `features = ["metrics", …]`, `opentelemetry-otlp` with
  `["grpc-tonic", "metrics"]`, there is no `TracerProvider`, and
  `grep -rni 'traceparent\|tracestate' crates/` matched nothing before this
  change — it now matches exactly the one doc comment added here that says so.
  Corrected in
  `book/src/deployment/troubleshooting.md` ("Metrics / **traces** over
  OTLP"), `docs/rust-sdk-assessment.md` (`✅ otel feature: **traces** +
  metrics`, in a row comparing this project against `a2a-rs` — an overclaim
  in the worst possible place, now `◑ metrics only` with a §10 recording the
  change), and `book/src/deployment/observability.md`, which told readers
  that identifiers on spans let one incident "be followed across the
  delegation chain when agents call agents". It cannot: separate processes
  produce separate span trees, and each agent mints its own task id, so the
  identifier changes at every hop.

- **`init_otlp_pipeline_with_endpoint`'s rustdoc said
  `OTEL_EXPORTER_OTLP_PROTOCOL` still applies.** It cannot —
  `MetricExporter::builder().with_tonic()` is hard-coded and the
  HTTP/protobuf exporter is not compiled in. A reader who set
  `http/protobuf` and pointed the endpoint at a collector's `:4318` would
  get gRPC spoken at an HTTP port, silence, and a doc line saying that
  should have worked.

### Internal

- **`scripts/check_panic_hooks.sh` forbids process-global panic hooks.** The
  regression test for the defect above, and for its class: a grep gate is the
  only mechanical check available, because "some other test lost its message"
  is not observable from inside the test that lost it. Registered in `ci.yml`'s
  Format job, paired with an injection in `scripts/prove_gates_fail.sh`, and
  listed in the gate-reachability input table. Proven both ways before
  shipping — exit 0 on the fixed tree, exit 1 naming all six lines on the tree
  as it stood, and `prove_gates_fail.sh --only check_panic_hooks` reporting
  PROVEN with the tree clean afterwards. The harness counts 66 gates now.

  The three gates the hook had made INCONCLUSIVE —
  `cargo test -p a2a-protocol-server --features {sqlite,postgres,auth-jwt}` —
  were re-run afterwards and all three report PROVEN, "gate exited 101 citing
  the injected defect". With a local PostgreSQL installed, three gates that
  had been PRE-BROKEN for want of one prove as well, so the selection is
  9 proven, 0 unproven.

- `check_doc_versions.py` gates dependency snippets in prose against the
  current release line. `a2a-protocol-sdk = "0.7"` means `^0.7`, which resolves
  to nothing in the 0.12 line, so a reader copying it got an SDK without any
  fix released since — including the #130 artifact-duplication fix. Measured at
  the 0.12.1 tag, 28 snippets across 14 files named 0.7, 0.8 or 0.11 against
  exactly one that was current. `release.yml` could not catch it: it verifies
  the four crate manifests against the tag, and prose is not a manifest.
  Historical snippets are allowlisted with a reason each, and an entry matching
  no snippet fails the gate, so the allowlist cannot rot either.

- **`tck.yml` gates the three example agents' TCK grades.** Those figures were
  dated manual measurements printed beside "no CI job gates it", and one had
  already drifted to "20/20" before a person reading the page caught it. The
  blocker was recorded as a CI-cost decision; it was not one. The TCK grades
  protocol conformance, which never reaches the executor's brain, and all three
  agents are written to serve without a model — measured 2026-09-17 with no
  `OPENAI_API_KEY`, no `OPENAI_BASE_URL` and nothing on any model port, all
  three score 21/21 graded, 0 failed, 1 N/A, the same figure the pages claimed.
  The new `tck-example-agents` matrix costs a build per agent and no secret.
  Eight documentation sites now name the job instead of disclaiming one.


## [0.12.1] - 2026-09-17

### Changed

- **SLIMRPC branch-spec triage: the broadcast-live design is now
  *collaborative task*.** The nightly `Official TCK` run of 2026-09-12
  ([34671180590](https://github.com/tomtom215/a2a-rust/actions/runs/34671180590))
  failed at `scripts/check_slimrpc_spec.sh`, as designed, on two untriaged
  files. Upstream `cb245fc` (2026-09-11) reframed the design on
  `feat/slimrpc-collaborative-channel`: `spec/v1/a2a-broadcast-live.md` gave way
  to `spec/v1/a2a-collaborative-task.md`, `spec/v1/slimrpc-broadcast-live.md`
  was renamed `spec/v1/slimrpc-collaborative-task.md`, and the profile was
  rewritten seven further times the same day. Both are triaged as not followed —
  but the SLIMRPC profile's **reason has changed and the old one is not
  reused**: its §§3.1 and 4 now accept 1.0's `SendStreamingMessage` as the
  activation call, so "requires A2A 1.1 and `SendLiveMessage`" is no longer true
  of that text. What blocks it now is the base spec's §4.3, which makes
  appending peer messages to A2A 1.1's `timeline` as `TimelineEntry(Message)` a
  MUST; its native mode's requirement for SLIM shared-responses group channels,
  which this binding does not implement; and the document never having reached
  upstream `main`. Recorded in `KNOWN_BRANCH_SPECS`, the vendored spec's README
  table, the binding's README and the book chapter.
- **Correction: the 2026-09-02 rename was `daddfb2`, not `0c38776`.** Entries
  written for the previous triage — including the 0.12.0 note below — credited
  the `slimrpc-collaborative-channel.md` -> `slimrpc-broadcast-live.md` rename
  to `0c38776` (2026-09-03), which changes only upstream's `examples/` tree. It
  was the branch *tip* when `check_slimrpc_spec.sh` observed the move, and that
  check clones `--depth 1`, so a tip is all it can see. The live documents now
  cite `daddfb2` (2026-09-02); the released 0.12.0 entry is left as published.

### Fixed

- **A new task on an existing context no longer inherits the previous
  task's artifacts, history and metadata** (server). When a client sent a
  second message on a context without a `taskId`, the server minted a fresh
  task id — correctly, that is a new round — and then built that task with
  the previous task's `artifacts`, `history` and `metadata` copied in. The
  agent's own output was merged on top, so every round after the first
  returned the whole context's accumulated artifacts, growing by one each
  turn ([#130](https://github.com/tomtom215/a2a-rust/issues/130)).
  `a2a.proto` scopes all three fields to the task, not the
  context: artifacts are "a set of output artifacts for a `Task`", history
  "the history of interactions from a `Task`", metadata "custom metadata
  about a task". `build_initial_task` had keyed the carry-forward on "a task
  exists for this context" when `resolve_task_id` had already decided the
  opposite; it now carries state forward only when the ids match — a genuine
  `InputRequired` continuation of the same task (A2A spec §3.4.3), where
  wiping the accumulated state would be the bug. Only `artifacts` was
  visible on the send response, because history is omitted from it unless
  the client asks for a `history_length`; `history` and `metadata` leaked
  the same way and are observable through `GetTask`. Affects 0.11.0 and
  0.12.0. Executors are unaffected: a multi-turn executor reads the previous
  task from `RequestContext::stored_task`, which is passed to it separately.
  This changes the wire shape for a given input and is therefore
  patch-eligible only as the specification correction `STABILITY.md` §2
  carves out; the requirement it corrects is `Task.artifacts`,
  `Task.history` and `Task.metadata` being task-scoped in `a2a.proto`.
- **Three wrong claims about the `rig-agent` example, and a stale TCK figure
  in seven places.** The prose described messages being passed to
  `rig_core::agent::Agent`, a type the example does not use — rig 0.41 moved
  the run loop into a separate crate, so the example defines its own
  single-turn `RigAgent<M>` over `rig_core::completion::CompletionModel`. The
  quickstart said the model defaults to `gpt-4o-mini`; it defaults to
  `qwen3.5:0.8b`. And "passes the TCK 20/20" was carried in seven places
  against a measured 21/21 graded with 1 not applicable. Each TCK site now
  carries a dated measurement, the reproducing command, and the fact that no
  CI job gates it — which is how the figure drifted, since `tck.yml` points
  only at `echo-agent` and `a2a-tck-sut`, never at the example agents.
- **Six markdown table rows rendered with missing columns.** Four rows in the
  two example tables (`examples/README.md`, `book/src/examples/overview.md`)
  were short of the declared four columns; `benches/README.md` was missing a
  bench module's File cell and had listed 14 of the 15 `[[bench]]` targets
  registered in `benches/Cargo.toml`; and a retired waiver row in
  `book/src/reference/conformance-history.md` had five cells against a
  six-column header, its `Why` cell having swallowed the `Removable when`
  content. A repo-wide scan that discounts pipes escaped inside code spans now
  reports zero mismatched rows in any markdown file.

### Security

- **`rustls` advanced to 0.23.45 (RUSTSEC-2026-0285).** rustls below 0.23.45
  accepted TLS 1.3 handshake messages sent at the wrong encryption level when
  they followed a key-changing message in the same record, contrary to
  RFC 8446 §5.1, which requires terminating the connection with
  `unexpected_message`. The advisory states the handshake transcript stays
  authenticated, so this is a conformance failure rather than a
  handshake-forgery or confidentiality break. The workspace lockfile moves
  0.23.44 → 0.23.45; no manifest changed, because every crate here already
  requests `rustls = ">=0.23, <0.24"`.
- **The SLIMRPC binding cannot take that upgrade yet, and now says so.**
  `bindings/a2a-protocol-slimrpc` reaches rustls with default features, so
  `aws-lc-rs` is in its graph, and rustls 0.23.45 requires `aws-lc-rs ^1.18`
  while `mls-rs-crypto-awslc 0.23.0` — the only release in the `^0.23` range
  that `agntcy-slim-auth 0.15.4` admits — pins `aws-lc-rs =1.16.2`. No
  published version of either resolves it, verified against the crates.io
  index rather than assumed. The binding's `deny.toml` therefore carries a
  dated `RUSTSEC-2026-0285` ignore recording the full chain, the severity as
  the advisory states it, and the two upstream releases that would let the
  entry be deleted. The four published crates are unaffected: they select the
  `ring` backend and are on 0.23.45.

### Internal

- **`check_slimrpc_spec.sh` now prints only the remedy that applies.** Every
  failure — including a branch-only specification nobody has triaged, which
  touches no vendored file — ended with "read the diff above",
  `./scripts/check_slimrpc_spec.sh --update` and "refresh the hashes". The
  2026-09-12 nightly failed exactly that way: no diff, every vendored copy
  matching, and instructions to re-vendor. The vendored-copy verdict is now
  tracked separately from the run's overall verdict, so that block prints only
  when a vendored file really has drifted. Exit codes are unchanged, and both
  paths were exercised: removing a `KNOWN_BRANCH_SPECS` entry exits 1 with the
  triage instructions and no re-vendor advice; appending a line to
  `spec/slimrpc_v1/slimrpc.md` exits 1 with the diff and the re-vendor advice.
- **The shutdown warning tests now say why a capture failed.** `Test (stable,
  windows-latest)` in
  [34679831264](https://github.com/tomtom215/a2a-rust/actions/runs/34679831264)
  failed `hung_cleanup_is_warned_about` with `got []`, while the behavioural
  half of that same test passed — the timeout fired and the report was correct,
  and only the WARN capture came back empty. Two guards replace that message.
  `warnings_during` no longer discards the result of `set_global_default`: if
  another global subscriber won the race, `WarnCapture` is not in the dispatcher
  and no capture can ever fill. The comment there denied this ("the sink below
  is what decides whether we capture anyway"); it is false, and it is now a
  panic that names the cause. Second, a canary WARN emitted from the test file
  with the sink armed, and cleared before the code under test runs, separates
  "the capture never worked" from "the callsite under test emitted nothing" —
  the latter being the 2026-08-19 interest-cache failure the file already
  documents. The canary is deliberately a different callsite, so it proves the
  subscriber, sink and level filter and nothing about that callsite's cached
  interest. Both guards proven to fire by injection: dropping every event in the
  layer trips the canary; installing a competing global subscriber first trips
  the install check. **The intermittent Windows failure itself is neither fixed
  nor explained by this** — fifteen runs of the full `--all-features` lib suite
  on Linux (nine before the change, six after) did not reproduce it — so the
  next occurrence will say which half is broken instead of `got []`.
- **Fixed: the shutdown warning capture collected other threads' events.**
  The guards above did their job on the first try. `Test (1.88,
  windows-latest)` in
  [34681613416](https://github.com/tomtom215/a2a-rust/actions/runs/34681613416)
  failed `clean_shutdown_warns_about_nothing` with `got ["executor cleanup did
  not finish within the shutdown timeout"]` — and the canary did not fire, so
  the capture was working. That narrowed it to one thing: the warning was not
  this test's. Its own `shutdown_with_timeout` reported
  `executor_cleanup_completed == true`, so the code under test never reached
  that `trace_warn!`. The sink is process-wide and the subscriber global, while
  `CAPTURE_LOCK` serialises only the tests in that one file — so a WARN from
  any of the other ~1100 tests running in parallel landed in it. `ACTIVE_SINK`
  now records the capturing thread alongside the sink and the layer ignores
  events from any other thread; the subscriber stays global, which is what the
  callsite-interest reasoning there requires. Regression test
  `a_warning_from_another_thread_is_not_captured` reproduces the CI failure
  exactly — without the fix it fails with the identical message. Note the
  constraint this adds, recorded at the static: every test in that file drives
  a current-thread runtime, so a future test on a multi-threaded runtime would
  have to widen the check. **The earlier `got []` failure in run 34679831264 is
  a different symptom and is still unexplained** — it is not this bug, since an
  empty sink is not a contaminated one.

## [0.12.0] - 2026-09-10

### Breaking Changes

- **Public configuration structs gain fields; struct-literal construction
  breaks.** `HandlerLimits` gains `push_delivery_budget` and
  `executor_drain_timeout`; `WebSocketTransportConfig` (client) gains
  `max_pending_requests`. Code that built either as a struct literal must
  use `Default::default()` and the `with_*` setters — both structs are also
  `#[non_exhaustive]` from this release (next entry), so adding the fields
  to the literal is not the fix. This release is the deliberate breaking
  minor before the two clean minors `STABILITY.md` §7 requires, so every such
  future-proofing change lands here rather than one per release.
- **Fifteen public configuration structs are `#[non_exhaustive]`**, so the
  next field any of them gains is an addition rather than a break. Server:
  `HandlerLimits`, `DispatchConfig`, `CorsConfig`, `GrpcConfig`,
  `CacheConfig`, `PushRetryPolicy`, `RateLimitConfig`, `TaskStoreConfig`,
  `TenantStoreConfig`, `PerTenantConfig`, `TenantLimits`. Client:
  `ClientConfig`, `RetryPolicy`, `WebSocketTransportConfig`,
  `GrpcTransportConfig`. (`ServeConfig` already was.) The wire types in
  `a2a-protocol-types` are deliberately not included: they are the spec's
  data types and a literal is their intended construction. **Migration:**
  a struct literal of any shape, `..Default::default()` included, stops
  compiling outside the crate; build the value with `Default` (or the
  documented constructor — `CorsConfig::new`/`permissive`,
  `CacheConfig::with_max_age`) and chain the `with_<field>` setter for each
  field you set. Every public field on every struct in the list now has one;
  the ones added for this are `DispatchConfig::with_require_version_header`,
  `CorsConfig::with_allow_origin`/`with_allow_methods`/`with_allow_headers`/
  `with_max_age_secs`, `RateLimitConfig::with_requests_per_window`/
  `with_window_secs`/`with_trusted_proxy_hops`/`with_max_buckets`,
  `TaskStoreConfig::with_max_capacity`/`with_task_ttl`/
  `with_eviction_interval`/`with_max_page_size`,
  `TenantStoreConfig::with_per_tenant`/`with_max_tenants`,
  `PerTenantConfig::with_default`/`with_overrides`/`with_override`,
  `TenantLimits::with_max_concurrent_tasks`/`with_executor_timeout`/
  `with_event_queue_capacity`/`with_rate_limit_rps` (and a deprecated
  `with_max_stored_tasks`, matching the field), and ten on `ClientConfig`
  (`with_preferred_bindings`, `with_accepted_output_modes`,
  `with_history_length`, `with_return_immediately`, `with_request_timeout`,
  `with_stream_connect_timeout`, `with_connection_timeout`,
  `with_max_response_size`, `with_tls`, `with_tenant`). The upgrading guide
  has a before/after. `STABILITY.md` §4 no longer carries the "still
  exhaustive as of 0.11" exception.
- **`ClientError` gains `TooManyPendingRequests { limit }`.** The enum is
  `#[non_exhaustive]`, so exhaustive matches already carried a wildcard.
- **`GrpcTransportConfig` is `#[non_exhaustive]`** (client) — one of the
  fifteen above, listed on its own because it also gained fields
  (`bare_address_scheme`, `tls_config`; under Changed below) and because
  `cargo-semver-checks` grades the attribute a major change and this heading
  is where STABILITY.md says those go.
- **`InMemoryQueueWriter` is no longer `UnwindSafe` or `RefUnwindSafe`**
  (server). It now holds the `Arc<dyn Metrics>` it reports dropped events
  through, and a trait object without those bounds removes the auto-impls.
  Only code that wrapped a writer in `catch_unwind` or `AssertUnwindSafe`
  can notice; the handler that owns the writer never had either bound.

### Added

- **`AgentCard::with_streaming(bool)`** (types): the one capability nearly
  every card sets, without a `with_capabilities(AgentCapabilities::none()
  .with_streaming(true))` of its own. With it, `AgentCard::new(name, version,
  interface)` and the `with_*` methods from 0.10 are enough to write every
  card in this repository's examples: `examples/multi-lang-team`'s
  `make_worker_card` is now eleven lines instead of a thirty-line literal,
  and **`examples/hello-agent` publishes a card** — it had none, so the
  "smallest complete agent" answered `404` at
  `/.well-known/agent-card.json` and the CLI's discovery path could not
  reach it (found building `tools/a2a-cli`). A fourth test resolves the card
  and greets the agent through a client built from it. `resolve_agent_card`
  is in `a2a_protocol_sdk::prelude` so that test, like the agent, needs no
  fully-qualified path.
- **`CardFetchOptions`, `resolve_agent_card_with_options` and
  `fetch_card_from_url_with_options`** (client, `discovery`): headers and a
  budget for the agent-card fetch. `resolve_agent_card` and
  `fetch_card_from_url` sent no headers and allowed a fixed 30 seconds, so a
  card behind authentication could not be fetched and the budget could not
  be shortened; both now delegate to the `_with_options` variant with
  `CardFetchOptions::default()`, which is those same values. Tests: a server
  that answers `401` without an `Authorization` header refuses the plain
  call and admits the options one; a server that sends headers at two thirds
  of a 600 ms budget and then stalls fails at one budget, not two. The CLI's
  discovery now carries `--header` and is bounded by `--timeout`.
- **`ClientBuilder::chosen_interface()`** (client): the card interface
  `from_card` picked — the first the caller prefers, else the card's first,
  and following `with_protocol_binding` when that moves the endpoint —
  as `Option<&AgentInterface>`, `None` for a builder from `ClientBuilder::new`
  or after switching to a binding the card does not advertise. gRPC and
  WebSocket need constructors other than `build()`, so a caller that has to
  choose one no longer re-implements the preference rule to learn which; the
  CLI's `connect.rs` had, and now reads it here.
- **`tools/a2a-cli`** (unpublished, built from the repository): `a2a
  card|send|stream|task get|cancel|list` over `a2a-protocol-client`, with
  `--binding`, `--timeout`, `--header`, `--tenant` and `--grpc-plaintext`;
  exit 0/1/2; eleven end-to-end tests against an in-process server. It
  is the first client written against the SDK from outside, and the three
  API gaps it hit are the `CardFetchOptions`, `chosen_interface` and
  `with_streaming` entries above.
- **`examples/resilient-agent`**: durability over a SQLite handler restart,
  failure injection (an executor failing its first N attempts, a webhook
  refusing its first M deliveries, a client retrying transport faults), and
  two replicas over a shared PostgreSQL store and rate-limit counter — each
  act measured and reported, with `[NOT RUN]` when the database is absent
  and `RESILIENT_REQUIRE_ALL=1` to make that exit 4. Runs in CI's
  `example-surface` job against its PostgreSQL service.
- **A Rust worker in `examples/multi-lang-team`** (`--bin rust-worker`,
  port 9104), so the example shows the worker side of the SDK next to the
  coordinator; the coordinator's demo now prints the fan-out artifact.
- **`book/src/reference/upgrading.md`**: one section per breaking boundary
  from 0.7 to 0.12 with compiling "after" snippets, and how to read a
  release's breaking section under STABILITY.md.
- **SLIMRPC binding**: `SlimRpcTransportBuilder::with_slow_consumer_timeout`
  (default 30 s) — a unicast stream whose consumer reads nothing for that
  long is abandoned with one final `ClientError::Timeout` instead of the
  process buffering the agent's frames without bound in the upstream
  per-call channel; a runnable `examples/in_process.rs`; the binding's
  tests under `coverage.yml` (measured 65.61% line coverage on
  2026-09-10); packaging verified through a config-level `[patch]` of the
  SDK pins rather than `--no-verify`; a README "Backpressure" section with
  file:line evidence for every path.
- **Four CI gates that the review backlog called for**, each proved able
  to fail by `scripts/prove_gates_fail.sh` injections:
  `check_gate_reachability.py` (every gate runs on the events that can
  break it — B13), `check_timeout_nesting.py` (an inner timeout fits the
  bound enclosing it, the push sender's schedule included — B14),
  `check_inert_bounds.py` with a reasoned allowlist (a configurable bound
  honoured by one sibling and not the others — B21), and the reverse
  direction of `check_api_reference.py` (every root-level export is on the
  API Quick Reference, which gained 123 rows).
- **`scripts/check_dco.sh`**, the DCO check as a script. `dco.yml` now runs
  it on every push to `main` as well as every pull request — the
  reachability gate's first finding was that a commit reaching `main` by
  any other route than a pull request was never graded — and
  `benchmarks.yml` runs it over the commit it pushes with `GITHUB_TOKEN`,
  which creates no workflow run. That bot identity is exempt only while the
  commit touches the two generated benchmark pages (PROVENANCE.md §3.2).
- **`HandlerLimits::push_delivery_budget`** (default 30 s): the total time
  one event's push deliveries may take across every registered config, per
  request batch on the blocking path. It was a `Duration::from_secs(30)`
  literal in two places, so the term that decided how many of an operator's
  webhooks were called was not a knob (review backlog B3).
- **`HandlerLimits::executor_drain_timeout`** (default 5 s): a blocking
  `SendMessage` whose executor returned without a terminal state, on an event
  queue that never closed, used to wait forever. It now answers with the task
  as collected — what a closed queue would have produced — and reports
  `Metrics::on_error("SendMessage", "executor_drain_timeout")`.
- **`GrpcDispatcher::with_max_connections`**: a ceiling on served gRPC
  connections, the one bound `with_http2_keepalive` and
  `with_max_connection_age` did not provide. The permit is taken before
  `accept()`, as the WebSocket dispatcher's has been, and rides with the
  socket until tonic drops it.
- **`WebSocketTransportConfig::max_pending_requests`** (client, default 64):
  the `N+1`-st request awaiting a response on one connection is refused up
  front with the new retryable `ClientError::TooManyPendingRequests` rather
  than queued invisibly in an unbounded map (B17).
- **`HttpPushSender::with_dns_timeout`** (default 5 s): the SSRF pre-flight's
  hostname lookup had no bound of its own, so a hung resolver spent the whole
  `push_delivery_timeout` before any request was made and was then reported
  as a webhook timeout. The bound is counted in `max_delivery_duration`, so
  the shipped default schedule is now 98 seconds against the 5-second
  `push_delivery_timeout`, not 93.
- **A dropped persistence event is counted.** When the background
  processor is gone, `InMemoryQueueWriter` reports the drop through
  `Metrics::on_persistence_error("queue_handoff", "channel_closed")`; until
  now the only report was a trace line that compiles to nothing without the
  `tracing` feature (B18).

### Fixed

- **A blocking caller sees the executor's error text** (server). When an
  executor returned `Err`, the `Failed` status update carried the text only
  as `metadata.error` on the streamed event; `sync_collector.rs` copies
  `status.message` onto the task and nothing else, so a blocking
  `SendMessage` returned a `Failed` task with no message and no metadata,
  and `GetTask` afterwards said no more (`examples/resilient-agent`, Act 2).
  The failure event's `status.message` is now an agent-role `Message` with
  one text part holding the error — the spec's field for "additional status
  updates for the client" — so the blocking response, the stored task and
  every `GetTask` after it carry the text; `metadata.error` stays on the
  event for streaming callers. An executor timeout reports the same way.
  Found and fixed while splitting `handler/messaging/mod.rs`'s
  `send_message_inner` into phases (`validation`, `continuation`,
  `admission`, `eviction`, `create`, `execute`), which took the file off
  `.file-length-baseline`; the split changes no error, message, metric or
  eviction order.
- **One deadline per token request and per stream start** (client).
  `OAuth2ClientCredentials::refresh` and `discover_token_endpoint` read the
  body under a second full request timeout after the headers had spent
  most of one, and the JSON-RPC and REST streaming transports read a
  failed stream start's error body under a second full
  `stream_connect_timeout`: a server that answered late and then stalled
  held the caller for close to twice the documented bound. Found by
  `scripts/check_timeout_nesting.py`; each path now takes one deadline for
  headers and body, with a regression test that fails against the old
  code (1.0 s against a 600 ms bound).
- **The blocking path reports every push outcome** (server). Push
  deliveries made for a blocking `SendMessage` reported nothing but trace
  lines, which a default build compiles away; they now report
  `delivered`, `failed`, `timeout`/`timeout_truncated` and `skipped`
  through `Metrics::on_push_delivery` exactly as the background path does.
- **DCO ran on pull requests only.** A commit reaching `main` by a direct
  push, a merge made outside a pull request, or a workflow's own push was
  never graded; `dco.yml` now runs on pushes to `main` too, and
  `benchmarks.yml` grades its own commit before pushing it (see the
  `scripts/check_dco.sh` entry above).
- **`Arc<T>` forwards every `Metrics` hook.** The blanket impl for `Arc<T>`
  forwarded six of the eight methods and defaulted `on_persistence_error` and
  `on_push_delivery` to no-ops, so a handler built with an `Arc<dyn Metrics>`
  — which `RequestHandlerBuilder::with_metrics` accepts through that impl —
  dropped every persistence error and every push outcome silently. Found by a
  test that passed against a bare implementation and failed against an `Arc`
  of the same.

- **An empty `contextId` or `taskId` on an incoming message is "unset", not
  an invalid id.** The A2A JSON bindings are ProtoJSON, and both fields are
  proto3 strings without presence, so `""` is the unset value. a2a-java's
  JSON-RPC transport prints every field (`alwaysPrintFieldsWithNoPresence`)
  and therefore sends `"contextId": ""` for none; this server answered
  `InvalidParams: context_id must not be empty or whitespace-only`, which
  failed every JSON-RPC pairing with the Java SDK in the first official ITK
  nightly (4 of 60 scenarios; the same peer passed over gRPC and HTTP+JSON,
  whose printers omit defaults). The server now maps an exactly-empty id to
  absent before validation and generates the context as for an omitted one;
  whitespace-only ids are still rejected, because no printer produces them
  for "unset". Regression test in
  `a2a-protocol-server/tests/proto3_empty_ids.rs` posts the raw JSON-RPC
  body the Java client sends.
- **The weekly mutation sweep's 55 survivors are addressed** (run
  34078555778 at `d3ebb72`, all in `a2a-protocol-server`). Twenty-six sat in
  `parse_numeric_ipv4`, merged on 2026-08-27 while the per-PR gate was
  passing over a failed baseline (the defect `db795c2` fixed); the function
  is rewritten to fill the leading octets and bound the tail, which retires
  the shift-and-or operators whose mutants were equivalent, and its packing
  is pinned by a table naming the exact address of every `inet_aton` form
  and the first value each part cannot hold. `labelled_constant_time_match`
  selects with an XOR-swap for the same reason, still branchless.
  `build_jwks_client` is one function with the feature split inside it, so
  its plaintext body is no longer a `#[cfg]` twin that `--all-features`
  compiles out. The rest are tests named for the mutant each kills: the
  tenant-slot sweep, the Postgres rate-limit counter's sweep cadence (live
  database, `#[ignore]`d like the store suite), the WebSocket keepalive sent
  once per silence, `IdleTimeout` forwarding flush and shutdown, the push
  truncation boundary, the SQLite delta payload and statement choice, and
  `rows_for_append` on a delta covering every part. Three survivors remain
  by construction — `TenantLimits::builder`, `PerTenantConfig::builder`,
  `SseBodyWriter::close` — and the ledger, `CONTRIBUTING.md`, ADR 0006 and
  `ROADMAP.md` now carry the current figures and the two false-green sweeps.

### Changed

- **`a2a-protocol-server` depends on `mutants`** (0.0.4, MIT, zero
  dependencies, attribute macros that return their input unchanged) so the
  three mutants no test can distinguish — `TenantLimits::builder`,
  `PerTenantConfig::builder`, `SseBodyWriter::close` — carry
  `#[mutants::skip]` in source with the equivalence argument as the comment,
  instead of surviving every sweep. ADR 0006 records the supply-chain
  review; the two files grade at 38 mutants, 0 missed with the sweep's own
  command line.
- **Dependencies refreshed for 0.12.** Both lockfiles updated
  (`cargo update`: 238 workspace entries, 57 in the SLIMRPC binding), and
  the majors: opentelemetry/opentelemetry_sdk/opentelemetry-otlp 0.32
  (the SDK's removed `rt-tokio` feature is the only change needed),
  tokio-tungstenite 0.30 everywhere it appears (tungstenite 0.30 rejects
  malformed `Sec-WebSocket-Key` server-side), rig-core 0.42 (the example's
  `Agent` moved out of rig-core; a thirty-line local shim keeps the example
  tool-less and behaviour-identical). genai stays at 0.5: every 0.6.x and
  the 0.7 betas depend on the unmaintained `paste` (RUSTSEC-2024-0436),
  which `cargo deny check advisories` rejects. base64 stays at 0.22
  because hyper-util still requires it and `deny.toml` denies duplicate
  versions. `deny.toml` gains seven reviewed skips for splits the ecosystem
  is mid-way through (RustCrypto digest 0.10/0.11 via tungstenite against
  sqlx, syn 2/3 via async-trait, base64 through the examples' reqwest and
  rcgen, redox_syscall through libredox) and loses the two `thiserror` skips
  that no longer matched anything.
- **MSRV lowered from 1.93 to 1.88; edition 2024.** The workspace had
  never needed anything newer than 1.88 — `cargo check --workspace
  --all-features --all-targets` on 1.88.0 passed without a code change —
  and 1.88 is the oldest toolchain the dependency tree (`time`,
  `serde_with`, `darling`) declares support for. Lowering an MSRV is not a
  breaking change. With it the crates move to edition 2024 and the
  MSRV-aware resolver (`resolver = "3"`), so `cargo update` keeps
  dependency versions compatible with the declared floor. `cargo fix
  --edition` made the mechanical changes (`$x:expr_2021` in macro
  matchers, `ref` patterns, `unsafe { set_var }` in the four build scripts,
  now carrying `SAFETY` notes); the edition's tail-expression drop-order
  change was reviewed at every site the migration lint reported and the
  full suite passes on both 1.88.0 and stable. Two places that seeded the
  environment for a reader — the OTLP pipeline test and the
  incident-response observability check — now pass the endpoint explicitly
  through the new `init_otlp_pipeline_with_endpoint`, and the rig example
  builds its client from explicit settings, so no library or example code
  writes the process environment. CI's clippy matrix now runs on stable
  only: 0.1.88's `similar_names` and `cognitive_complexity` verdicts differ
  from current clippy's, and lint verdicts are a property of the linter,
  not of the compatibility floor; the Test matrix keeps its 1.88 leg on all
  three platforms.

### Added

- **`grpc-tls` feature on `a2a-protocol-server`: TLS on the gRPC listener
  itself.** Until now the dispatcher was plaintext-only and the book said to
  terminate TLS in a proxy or mesh, which the official Go, Python and Java
  SDKs also expect — but a2a-rs serves gRPC TLS in-process, and a deployment
  without a mesh had no in-tree answer. `GrpcDispatcher::with_tls` takes a
  `tonic::transport::ServerTlsConfig` (re-exported from `dispatch::grpc`
  with `Identity` and `Certificate`, so no tonic dependency of your own):
  the server certificate and key, and for mutual TLS a client CA, with
  `client_auth_optional` to admit clients that present none. Every `serve*`
  method applies it; `into_service` still hands the bare service to a server
  the caller configures. The listener then speaks TLS only — a plaintext
  client is refused at the handshake, with no fallback. tonic builds its
  acceptor from the process-level rustls provider and panics when none is
  installed and both `ring` and `aws-lc-rs` are linked (this workspace's
  `--all-features` build is such a binary); the dispatcher installs `ring`
  when nothing is installed, which is what a single-provider build does
  implicitly, and never overrides an installed one. A rejected
  configuration (a key that does not match its certificate) is an
  `std::io::Error` from the `serve*` call, not a panic. The SDK crate's
  `grpc-tls` now enables both sides. Proved end to end in
  `a2a-protocol-sdk/tests/grpc_server_tls_e2e.rs` against the client's
  `grpc-tls` transport with `rcgen` certificates: pinned-CA round trip,
  plaintext refusal, mutual TLS admitting a CA-signed client and rejecting
  one without a certificate, optional client auth, and the
  certificate/key-mismatch error; `grpc_server_tls_provider_e2e.rs`, in its
  own binary because the provider is process state, proves the
  install-when-absent path.
- **`init_otlp_pipeline_with_endpoint`** (`a2a_protocol_server::otel`):
  `init_otlp_pipeline` with the collector endpoint given explicitly instead
  of read from `OTEL_EXPORTER_OTLP_ENDPOINT`.
- **Coverage measures the PostgreSQL stores instead of failing to exclude
  them.** The coverage workflow now runs the same live-database suites the
  `test-postgres` CI job runs (`postgres_store_tests`, `multi_replica`)
  under instrumentation against a `postgres:16` service and merges the
  profiles into one report. The seven PostgreSQL files had sat at 0-8% on
  the dashboard — 42% of every uncovered line in the repository — behind an
  ignore list that never took effect (`codecov.yml` keeps the history).
  Measured locally the same way: the PostgreSQL files at 94.55% in
  aggregate. Two live-database tests were added for the paths the
  isolation tests did not reach — the tenant-aware task store's
  `context_id`, `status` and `statusTimestampAfter` filters, cursor
  pagination, delete and count, and the push-config stores' list, delete
  and count — proving along the way that a delete under the wrong tenant is
  a no-op that leaves the other tenant's row intact.
- **Nightly run in the official ITK, with published metrics.** Until now
  the only cross-SDK evidence graded by someone other than this project was
  the official TCK; the upstream Integration Testing Kit job was
  dispatch-only and had never completed. `itk/run_itk.sh` now adopts the
  shim every SDK repository carries over a2a-itk's shared driver, and
  `.github/workflows/itk-nightly.yml` runs the shared nightly set at 02:00
  UTC with this repository mounted as the system under test against every
  peer in the ITK's `matrix.yaml` — official Python, JavaScript, Go, Java,
  and a2a-rs — over JSON-RPC, gRPC and HTTP+JSON. Results are uploaded as a
  run artifact and, from `main`, to the rolling `nightly-metrics`
  prerelease the ITK dashboard reads, the same mechanism the other SDKs
  use. The workflow also diffs the vendored `instruction.proto` against
  upstream so the agent cannot silently drift from the schema it is graded
  against. Not a PR gate. The first run reported 56 / 60 and found the
  empty-`contextId` bug fixed under "Fixed" above; the run on the final
  commit of this change reports 60 / 60.
- **Feature-matrix CI job.** `cargo hack clippy --each-feature` now lints
  every feature of each published crate on its own, plus no-default-features
  and all-features (39 combinations, 8 minutes warm), so a `#[cfg(feature)]`
  gap is caught before a downstream build enables an unusual subset. The
  hand-picked combination list stayed as it was; this is the exhaustive
  complement. `scripts/preflight.sh` picks the gate up automatically.
- **Dependabot** (`.github/dependabot.yml`): weekly grouped minor/patch bumps
  for the workspace, the SLIMRPC binding's own lockfile and GitHub Actions;
  majors arrive as separate pull requests. Its commits are exempt from the
  DCO gate by exact author identity — a version bump carries no authored
  content to certify, and the review and merge stay human — recorded in
  `PROVENANCE.md` §3.2.
- **`STABILITY.md`**: the API stability policy — what "breaking" means for
  these crates, deprecate-then-remove with a one-minor window, at most one
  breaking minor per month, `### Breaking Changes` changelog headings,
  `cargo-semver-checks` as the proof, the MSRV rule, what is and is not
  covered, and the criteria for `1.0`. Linked from the README's Stability
  section.
- **crates.io Trusted Publishing.** The release workflow's publish job now
  exchanges its GitHub OIDC token for a short-lived crates.io token
  (`rust-lang/crates-io-auth-action`), falling back to the environment
  secret with a warning until every crate has a trusted publisher
  configured, and failing if neither credential exists. The one-time
  crates.io setup is in `RELEASING.md`.

- **`grpc-tls` feature (client, forwarded by the SDK crate): gRPC over TLS.**
  `grpc` alone never had a TLS connector — tonic's is a feature, and none was
  enabled — so an `https://` gRPC endpoint opened a plaintext HTTP/2 stream to a
  TLS port and failed on the handshake with an error that read like a network
  fault. With `grpc-tls`, `https://` endpoints are verified against the same
  bundled Mozilla roots (`webpki-roots`, ring) the HTTP transports use, or
  against a `tonic::transport::ClientTlsConfig` supplied through the new
  `GrpcTransportConfig::with_tls_config` for a private CA or client
  certificate. Without the feature an `https://` endpoint is now refused up
  front with a message naming it, instead of failing after the fact. No new
  crate versions: tonic's `tls-ring` and `tls-webpki-roots` resolve to the
  `rustls 0.23` / `tokio-rustls 0.26` / `webpki-roots 1.0` already in the
  tree. Proved in `a2a-protocol-sdk/tests/grpc_address_e2e.rs` against a
  tonic TLS listener presenting an `rcgen` certificate: a pinned CA round-trips
  over a bare target and over `https://`, the bundled roots reject that CA,
  and TLS against a plaintext listener does not fall back. The connector
  never touches the process-level rustls crypto provider: tonic under
  `tls-ring` uses an installed default or else `ring` explicitly, so a binary
  that links both `ring` and `aws-lc-rs` (this workspace's `--all-features`
  build is one) gets a connection error, not a panic, and an application that
  wants `aws-lc-rs` installs it itself — `grpc_tls_provider_e2e.rs`, in its
  own test binary because the provider is process state, proves both. The
  feature does not pull in `tls-rustls`; a gRPC-only client gets no
  hyper-rustls. The tonic types a caller needs (`ClientTlsConfig`,
  `Certificate`, `Identity`) are re-exported from `transport::grpc`, so no
  direct tonic dependency is required, and `ClientBuilder::with_grpc_tls_config`
  carries a pinned CA or client certificate through `from_card(..).build_grpc()`.
  A bare target that the policy dialled with TLS and that refused the
  handshake fails with the policy named and `GrpcBareAddressScheme::Http`
  suggested, since a plaintext peer on a private network is the usual cause.
- **`GrpcBareAddressScheme`** (`a2a-protocol-client`), with
  `GrpcTransportConfig::with_bare_address_scheme` and
  `ClientBuilder::with_grpc_bare_address_scheme`: how a bare `host:port` gRPC
  target is dialled. `HttpsExceptLoopback` (default) uses TLS for every host
  except `localhost` / `127.0.0.0/8` / `::1`; `Https` and `Http` force one
  scheme. See the fix below for why a bare target is accepted at all.
- **§7.6.4 conformance test** (`auth_required_state_tests.rs`). Upstream
  added *In-Task Authorization Scope* on 2026-07-30 (`6550d34`): the
  `TASK_STATE_AUTH_REQUIRED` transition "MUST NOT" be treated as authorization
  for anything. This server has always satisfied that by construction — the
  interceptor chain runs before every method and nothing reads the task's
  state for an authentication decision — and the test pins it: a continuation
  of an `AUTH_REQUIRED` task with no credential is refused on the
  interceptor's terms, runs no executor, and leaves the task's history
  untouched, while the same continuation with the credential is processed.
  `SPEC_COMPLIANCE.md` carries the row.

### Changed

- **`GrpcTransportConfig` is `#[non_exhaustive]`** (breaking for code that
  built it as a struct literal; `GrpcTransportConfig::default()` plus the
  `with_*` setters covers every field). It gained `bare_address_scheme` and,
  under `grpc-tls`, `tls_config`, and a public-fields config struct that grows
  by a field on every such change would break its callers each time. No code
  in this repository, its examples or its bindings constructed it literally.
- **`GrpcTransport::connect` rejects a non-target address by name.** An
  address with a scheme other than `http`/`https`, a path, a query, or userinfo
  is an `InvalidEndpoint` that quotes the input (`grpc://…`, `agent:50051/a2a`,
  `user:pw@agent:50051`), where before anything without `http(s)://` was
  refused with the same one-line message.

- **Official TCK: two more `a2a-tck` checks baselined, same stale-specification
  cause as the first two.** The nightly of 2026-09-01
  ([33467431712](https://github.com/tomtom215/a2a-rust/actions/runs/33467431712))
  went red on `CORE-CANCEL-002` [`http_json`] and `STREAM-SUB-003` [`grpc`]
  while the nightly 22 hours earlier had passed **on the identical commit**
  (`b6f3afb`). `a2a-tck` landed
  [`de6af18`](https://github.com/a2aproject/a2a-tck/commit/de6af188d2d65779719c88d4f6bb5b180a4fa91d)
  (PR #207) in between, which replaced "any error is acceptable" assertions
  with assertions on the error each requirement names — a good change that
  reached two rows §5.4's table has stale in the copy the suite vendors:
  `TaskNotCancelableError` (`409` there, `400` current) and
  `UnsupportedOperationError` (`UNIMPLEMENTED` there, `FAILED_PRECONDITION`
  current). Both requirements pass on every binding where the two copies agree
  and fail only on the one where they do not, which is the shape of a stale
  table rather than of a defect here. No SDK behaviour changed;
  `tck/conformance-baseline.json` now carries four entries and
  `docs/official-tck-findings.md` §21 records the evidence.
- **Corrected: the specification divergence is a pinned release, not an
  in-place amendment.** 0.11.0's changelog entry and §20 both said upstream "amended the
  document in place under the same `1.0.0` version string". It did not. A2A
  released the change as **v1.0.1** (2026-05-28, commit `757f0ec`, PR #1627);
  the `v1.0.0` tag still carries the old table, and `a2a-tck`'s vendored copy is
  byte-identical to it — its own `specification/version.json` names that tag and
  a 2026-03-13 download. The earlier reading came from a reproduction that
  compares a vendored file against `main`, through which a tagged patch release
  and a silent rewrite look the same. Every conclusion drawn from the divergence
  is unaffected; the mechanism, and so the right thing to ask upstream for, is
  not. Full timeline and commands in `docs/official-tck-findings.md` §21.1.
- **Reported upstream as
  [a2aproject/a2a-tck#231](https://github.com/a2aproject/a2a-tck/issues/231).**
  §20 had left "reporting it upstream is the obvious next step" undone for two
  days; it is done, and it was worth the wait, because §21.1's correction
  changed what there was to report — "your vendored copy is a superseded release
  and `make spec` moves it" is a different request from "upstream rewrote a
  published document". The submitted body, the duplicate check and the limits of
  that check are kept in `docs/upstream/a2a-tck-231-spec-pin-report.md`.
- **The root cause is reported to the specification as
  [a2aproject/A2A#2200](https://github.com/a2aproject/A2A/issues/2200).** §3.6
  promises patch releases do not affect protocol compatibility; `v1.0.1` changed
  six wire-observable §5.4 rows. Reading `757f0ec` and `v1.0.1`'s release notes
  before filing narrowed the claim and improved it: the change was announced and
  was made deliberately, to align the HTTP codes with `google.rpc.Code`, so the
  issue asks how §3.6 is to be reconciled and how an implementer is meant to
  notice — not for a revert. Record, including what it deliberately does not
  claim, in `docs/upstream/a2a-2200-patch-versioning-report.md`.
- **SLIMRPC branch-spec triage: `slimrpc-broadcast-live.md`.** Upstream moved
  `feat/slimrpc-collaborative-channel` at `0c38776` (2026-09-03), replacing the
  spec that branch had been triaged against, so
  `scripts/check_slimrpc_spec.sh` — which surveys every upstream branch on every
  run — began failing as designed. The new spec is not followed by this binding,
  and that is a fact rather than a preference: its own §3 requires A2A 1.1 and
  the `SendLiveMessage` method, and neither exists in any released A2A
  specification. `a2aproject/A2A` is tagged `v1.0.1`, `SendLiveMessage` appears
  nowhere in its docs, and this SDK implements the ratified 11-method v1.0
  surface that `check_method_denominator.py` holds it to. Re-triage if A2A 1.1
  ships.
- **Official TCK: the `--deselect` on the minimal-capability profile is gone.**
  [a2aproject/a2a-tck#225](https://github.com/a2aproject/a2a-tck/issues/225),
  filed from this repository, was fixed upstream on 2026-08-31 by PR #226. The
  minimal profile now grades the same 66 MUST requirements with nothing
  excluded, measured with and without the flag.

### Fixed

- **gRPC client: a card's `host:port` gRPC address is accepted.** Since A2A
  `cfc9d34` (2026-07-21) the proto's `AgentInterface.url` comment says a gRPC
  interface's address "should be in the format `hostname:port`", example
  `grpc.example.com:443` — a gRPC target, which carries no scheme. That is what
  every official SDK's agent advertises, and what this repository's own
  conformance SUT advertises (`grpc.insecure_channel("http://…")` fails on the
  Python side). `GrpcTransport::connect` refused anything that did not start
  with `http://` or `https://`, so `ClientBuilder::from_card(..).build_grpc()`
  could not reach a spec-format gRPC agent at all; the examples worked only
  because each one prepends `http://` by hand. A bare target is now normalised
  per `GrpcBareAddressScheme` — the same decision the official SDKs leave to
  a channel factory (Python) or make as plaintext-unless-configured (the A2A
  project's own Rust SDK); this one defaults to TLS and exempts loopback, and
  says so. Proved in `grpc_address_e2e.rs`: `from_card` on `127.0.0.1:{port}`
  and on `localhost:{port}` builds and talks; the `Http` policy dials a
  non-loopback spelling in plaintext; without `grpc-tls` a non-loopback target
  is refused with the feature named rather than downgraded to plaintext.
- **Examples and the ITK agent advertise spec-format gRPC targets.** The
  `agent-team` example's gRPC card said `http://127.0.0.1:{port}`, and its
  gRPC tests bypassed the card with a hand-built transport; the ITK current
  agent's card said the same and its peer client prepended `http://` to any
  scheme-less address it met — a workaround for the limitation fixed above,
  written on the assumption that scheme-less was the peer's quirk rather than
  the specification's form. Both cards now advertise `127.0.0.1:{port}`, test
  56 reaches the gRPC agent through `ClientBuilder::from_card(..).build_grpc()`,
  and the ITK workaround is gone; the in-repo ITK traversal self-test (one-hop
  GRPC, two-hop JSONRPC→GRPC) passes against the rebuilt agent.
- **Book: `tls-rustls` was documented as off by default for the client and
  the SDK crate.** It has been on by default since the feature existed
  (`default = ["tls-rustls"]` in both manifests; the client's own README said
  so). `reference/configuration.md` now agrees with the manifests.
- **Client: the Agent Card's `tenant` now rides on every request, not only on
  `SendMessage`.** A2A §8.3.2 rule 4 says a client **MUST** "set the `tenant`
  field in every request message to exactly the value declared in the selected
  `AgentInterface` entry". `ClientBuilder::from_card` has carried that value in
  `ClientConfig::tenant` since the multi-tenancy work, and the book said it was
  "applied to all requests" — but only `send_message` read it. `GetTask`,
  `ListTasks`, `CancelTask`, `SubscribeToTask`, the four push-config methods
  and `GetExtendedAgentCard` all went out with no tenant, so a task created
  under `acme` was then looked up under the default partition and answered
  `TaskNotFound`, and against a server that resolves the tenant itself every
  follow-up call was a cross-tenant request. Found by diffing the client's
  eleven methods against the rule while re-reading the specification at
  upstream `main` (`f63dbb4`), not by a report. A per-request `tenant` still
  wins over the client default, and an absent tenant still leaves the field
  out (the parameterless `GetExtendedAgentCard` stays `null` on the wire).
  Proved three ways: a capturing transport asserts the field on each of the
  nine methods by name; `tests/tenant_round_trip_tests.rs` drives a client
  built from a tenant-bearing card through send → get → list → cancel →
  push-config create/get/list/delete against a real server with
  tenant-partitioned stores over both JSON-RPC and REST; and the same test's
  control shows the task is `TaskNotFound` to an untenanted client, so the
  suite cannot pass against a server that ignores tenants.
- **REST binding: the tenant travels as the `/{tenant}/…` path prefix on the
  client, and the server accepts every place the proto lets it arrive.**
  `a2a.proto` binds each method twice — a primary pattern with no tenant in
  the path and an `additional_bindings` pattern with it as the leading segment.
  The reference SDKs' REST clients send the prefix form (`a2a-python` 1.1.4
  `_get_path`), and the reference server reads *only* that form. Ours did the
  reverse on the client (tenant as `?tenant=` on GET/DELETE and in the body on
  POST — the primary-pattern transcoding of §11.5) while the server, like the
  reference, read only the prefix, so the two halves of this SDK disagreed
  with each other and every tenanted GET lost its tenant. Now the client emits
  `/{tenant}/tasks/{id}` (percent-encoding the segment, keeping the field in
  POST bodies exactly as the Python SDK does), and the server honours, in
  this order, the path prefix, then `?tenant=` on GET/DELETE, then the body
  on POST — including `POST /tasks/{id}:cancel`, whose `body: "*"` was never
  read at all, dropping `metadata` too. The path tenant is percent-decoded
  before it names a partition and is injected into POST bodies that omit it
  (`/acme/message:send` with no `tenant` in the body lands in `acme`). Seven
  server tests in `tests/rest_tenant_binding_tests.rs` pin each arrival path
  and the precedence, each with a `TaskNotFound` control.
- **Official TCK: one failure no longer reports as three.** The
  minimal-capability and required-extension runs inherited the default
  `success()` condition, so a red gate skipped them — while their own gates
  carried `always()` and ran anyway, dying on reports that were never written
  (`error: TCK report not found`). Both profiles are now measured whenever the
  SUT built, and each gate runs unless its own run step was skipped, so a
  nightly reports every profile's drift in one run instead of one per night.
- **`sustained_load_tests` was intermittent, and the tolerance was hiding it.**
  Both leak tests sampled their late probe the instant the load loop exited.
  The loop awaits each request, but the work it starts finishes asynchronously —
  event queues and cancellation tokens are released when a *task* completes, not
  when its request returns — so the last few in-flight tasks were counted as
  growth. Both allowed `early + 1`, a tolerance with no derivation behind it, and
  on 2026-09-04 `cancellation_tokens_do_not_accumulate_under_sustained_load` read
  `0 -> 2` and failed on CI while the identical commit passed in the sibling run
  and in five consecutive local runs. The late probe now polls until two reads
  agree, bounded by a five-second timeout. This costs no detection power, which
  is why it is the fix rather than a wider tolerance: a leaked entry is never
  released, so the count a real leak settles on *is* the leaked count. Measured
  after the change — the two leak probes read `0 -> 0` where one had read
  `0 -> 2`, and the capacity test's real numbers (`50 -> 200`, against a ceiling
  of 200) are unchanged.
- **`clippy::redundant_clone` on nightly.** `eviction/fixtures.rs` cloned a
  `TaskId` into a map insert and dropped the original unused. Caught by the
  `Nightly (informational)` canary, which floats with the nightly toolchain
  precisely so a lint reaches this repository before it reaches stable and
  blocks everything.

## [0.11.0] - 2026-08-30

### Added

- **`WEBSOCKET_BINDING_URI`** (`a2a-protocol-types`) —
  `https://a2a-rust.com/bindings/websocket/v1`, this project's identifier for
  its WebSocket binding. §5.8 says a custom binding (§12) **SHOULD** be
  identified by a URI rather than a bare name, so that two projects cannot
  define incompatible bindings under the same word; `"WEBSOCKET"` is exactly
  the collision that rule exists to prevent. `AgentInterface::protocol_binding`
  is a free-form string the caller supplies and the crates never match on, so
  nothing breaks by adopting it — but a card advertising the bare name should
  move, and readers should accept both for as long as cards in the wild carry
  the old spelling. The conformance SUT now advertises the URI while the
  examples keep `"WEBSOCKET"`, so CI exercises both paths rather than
  asserting the migration is complete.

  This repository's other custom binding was already compliant:
  `a2a-protocol-slimrpc` advertises upstream's
  `https://a2a-protocol.org/bindings/experimental-slimrpc/v1`. WebSocket was
  the outlier, which is the sort of thing a spec diff surfaces and an
  inventory of your own bindings would have surfaced sooner.

### Changed

- **BREAKING (wire format): six §5.4 error mappings corrected.** A server on
  0.11 answers some errors with a different HTTP status and gRPC code than one
  on 0.10. Anything asserting the old values — a client matching on status, a
  test, a gateway rule — needs updating.

  | A2A error | HTTP was → is | gRPC was → is |
  |---|---|---|
  | `TaskNotCancelableError` | `409` → **`400`** | — |
  | `ContentTypeNotSupportedError` | `415` → **`400`** | — |
  | `InvalidAgentResponseError` | `502` → **`500`** | — |
  | `PushNotificationNotSupportedError` | — | `UNIMPLEMENTED` → **`FAILED_PRECONDITION`** |
  | `UnsupportedOperationError` | — | `UNIMPLEMENTED` → **`FAILED_PRECONDITION`** |
  | `VersionNotSupportedError` | — | `UNIMPLEMENTED` → **`FAILED_PRECONDITION`** |

  `ErrorCode::http_status()` and `ErrorCode::grpc_status()` keep their
  signatures and return the new values, so this is a behavioural break that
  `cargo-semver-checks` cannot see: it compares API surface, and no surface
  changed. It is classified here by hand, which is the only way this class of
  change gets classified.

  Why the old values were wrong: `docs/implementation/v1.0.0-specification-complete.md`
  was a snapshot of the specification taken 2026-03-31 and never refreshed.
  Upstream amended the document in place under the same `1.0.0` version string,
  so the vendored copy and the published spec disagreed in six of §5.4's nine
  rows while both claimed to be v1.0.0. The SDK implemented the stale table and
  the TCK graded against it, so they agreed with each other and failed
  conformant third-party agents — the symmetric misreading the TCK's own README
  warns about. Found by running the kit against an agent built on the official
  Python SDK (`a2a-sdk` 1.1.2), which answered `400` where the kit demanded
  `409`; the SDK was right.

  **If you grade against the official `a2aproject/a2a-tck`, expect two MUST
  failures** — `HTTP_JSON-STATUS-001` and `GRPC-ERR-002`. They are not defects
  in your agent. That suite vendors its own copy of the specification and it
  carries the same stale table, disagreeing with the published document in the
  same six rows. Both entries are baselined here with the evidence and a
  two-command reproduction in `docs/official-tck-findings.md` §20, and they
  clear when the suite refreshes its copy.

- **BREAKING (wire format): the Axum adapter now agrees with the REST
  dispatcher.** It carried a second, hand-written copy of §5.4's table which had
  drifted from the shared one in three further places: `TaskNotCancelable` and
  `InvalidStateTransition` answered `409` and `PushNotSupported` answered `501`,
  where the table says `400` for all three. The duplicate is deleted and the
  adapter defers to `ErrorCode::http_status()`; a test now fails if the two
  disagree again. `PayloadTooLarge` (`413`) and `Overloaded` (`503`) remain
  adapter-specific, because A2A has no error code for either.

- **BREAKING (wire format): push notifications are delivered as
  `application/a2a+json`.** §4.3.3 specifies the A2A media type for the
  webhook `POST`; the sender was using `application/json`. A receiver that
  matches the header exactly — a framework body parser keyed on
  `application/json`, a gateway content-type rule, a WAF — will stop accepting
  deliveries until it also accepts `application/a2a+json`. The body is
  unchanged, and `+json` structured-suffix parsers already handle it.

### Fixed

- **The vendored specification is current again.** Re-vendored from upstream
  with a provenance header naming its source, retrieval date, and the one-line
  command to refresh it.

  The refreshed document carries four normative changes beyond §5.4's table,
  and 0.11.0 absorbs all four. Two were already implemented and needed only to
  be confirmed against code rather than against the diff: `PushNotificationConfig`
  is already the flat `TaskPushNotificationConfig` in both `proto/a2a_v1/a2a.proto`
  and `a2a-protocol-types`, and `AuthenticationInfo` already carries `scheme` +
  `credentials` rather than `schemes[]` + `token`. The other two ship here — the
  push-webhook `Content-Type` and §5.8's URI-identified bindings, both above.

  Worth recording that the first reading of this diff reported all four as
  unimplemented. A specification diff says what the document changed, not what
  the code does; two of the four had been implemented ahead of the vendored
  snapshot, which is invisible from the diff alone.

- **The TCK no longer fails conformant agents on `application/a2a+json`.** The
  `a2a_media_type_accepted` check ran against JSON-RPC, where §9 specifies
  `Content-Type: application/json` and §14.1.1 scopes the A2A media type to
  §11's REST binding. It is REST-only now. Two official SDKs (`@a2a-js/sdk`,
  `a2a-java`) were carried in the ITK's `--skip` list as divergent for as long
  as the check was mis-scoped; both were conformant.

- **The TCK's availability classifier no longer reads a refusal as an
  absence.** `-32004` (`UnsupportedOperationError`) counted as "this binding
  does not offer the method", which is what the stale §5.4 table implied by
  mapping it to gRPC `UNIMPLEMENTED`. Under the corrected table it is
  `FAILED_PRECONDITION` — served, and refused — so only `-32601` (JSON-RPC
  *Method not found*) now means absent, matching REST's `404`/`501` and gRPC's
  `UNIMPLEMENTED`. This was fabricating `BIND-EQUIV-001` violations against
  agents that offer an operation on both bindings and decline it on both.

### Internal

Not shipped in the published crates; recorded because the conformance claims
rest on them.

- **Every third-party SDK pin was behind upstream**, and all three moved:
  `@a2a-js/sdk` 1.0.0 → 1.1.0, `a2a-go/v2` v2.3.1 → v2.5.0, `a2a-java`
  1.0.0.CR1 → 1.3.0.Final. Three of the four entries in the ITK's divergence
  table went with them; one genuine divergence remains (`a2a-java` rejecting
  `application/a2a+json` on REST, re-verified at 1.3.0.Final).

- **`itk/agents/java-sdk` produces a permissive `TaskAuthorizationProvider`.**
  a2a-java 1.3.0.Final enforces a fail-closed default for task authorization,
  so an agent with no provider answers `TaskNotFoundError` to every task read —
  a denied read reported as "not found" rather than leaking that the task
  exists. Granting explicitly keeps the authorization path executing, where
  `a2a.authorization.required=false` would switch it off.

- **`pin-freshness.yml`** re-resolves every SDK pin on a three-week cadence and
  re-grades against the current skip list. Every mechanism needed to catch the
  stale pins already existed — the kit exits 1 on a skipped test that passes —
  and never fired, because nothing moved the pins.

## [0.10.0] - 2026-08-27

### Deprecated

- **`TenantLimits::max_stored_tasks`** (`a2a-protocol-server`). It named a cap
  on stored tasks and nothing ever read it, for a structural reason: it sits on
  `PerTenantConfig`, which the handler holds, and a store is constructed
  independently and handed to the builder — a store never sees one. The working
  equivalent is the new `TenantAwareInMemoryTaskStore::with_tenant_override`,
  which gives a named tenant its own `TaskStoreConfig` and so its own
  `max_capacity`.

  Deprecated rather than removed: removing a public field is a semver break, and
  this crate's version is bumped as step 1 of a release (`RELEASING.md`), not
  mid-branch. The deprecation carries the whole point anyway — every use site
  now gets a compiler warning naming the replacement, which is louder than the
  silence the field had before.

### Added

- **`TenantAwareInMemoryTaskStore::with_tenant_override`** — gives a named
  tenant its own `TaskStoreConfig`, so `max_capacity`, `task_ttl`,
  `eviction_interval` and `max_page_size` can all differ by tenant. It lives on
  the store rather than on `TenantStoreConfig` because that config is
  exhaustively constructible through its public fields, so adding one would
  break every downstream struct literal.


- **Task retention for the persistent stores.** `purge_expired` on
  `SqliteTaskStore`, `PostgresTaskStore` and their tenant-aware variants deletes
  terminal tasks older than a [`RetentionPolicy`], in batches, and returns a
  `PurgeReport` describing what it did. **Nothing is deleted unless you call
  it.** The in-memory store has always forgotten tasks after an hour and the
  persistent stores kept them forever; the divergence between the two was the
  defect, not the growth, and neither behaviour was written down. Growth is
  modest either way — measured at 826 B/row on PostgreSQL and 781 B/row on
  SQLite, so under a GiB per million tasks. Retention is deliberately not wired
  to a timer inside the store: a sweep that fires on its own fires during your
  traffic peak, and the store does not know when that is. Call it from whatever
  already schedules work.

- **Deployment-wide rate limiting.** `RateLimitInterceptor::with_shared_counter`
  accepts any `RateLimitCounter`, so N replicas enforce one limit rather than N
  copies of it. `PostgresRateLimitCounter` is the bundled implementation.

- **A server that can be stopped without cutting live calls.** `Server`,
  `ServeConfig` and `serve_with_shutdown` bind a listener, serve until a
  shutdown signal, and drain in-flight requests within a deadline, returning a
  `ServeReport`.

- **Connection-level timeouts**, so a slowloris costs the attacker something:
  `ServeConfig` bounds header-read time, idle time and shutdown drain
  (`with_header_read_timeout`, `with_idle_timeout`, `with_drain_timeout`) and
  caps concurrent connections (`with_max_connections`).

- **`set_caller_identity` / `with_caller_identity`** on the rate-limit
  interceptor, so a per-caller limit is keyed by the caller.

- **`TaskState::ALL`** — the nine protocol states as an array, for exhaustive
  iteration over a `#[non_exhaustive]` enum.

- **Constructors for `AgentCard`, `AgentInterface` and `AgentSkill`.** Building
  a card required a struct literal naming all fifteen fields — no `new`, no
  builder, no `Default` — which is the first thing anyone has to do with this
  SDK. `AgentCard::new(name, version, interface)` takes exactly what
  `AgentCard::validate` requires, so a constructed card is valid by
  construction, and the rest is chained `with_*` in the style
  `AgentCapabilities` already used. `AgentInterface::jsonrpc/grpc/rest` spell
  the spec's own binding names and default `protocol_version` to
  `A2A_VERSION`. This repository contained 122 `AgentCard { .. }` literals when
  the constructors were written.

- **Connection bounds for `GrpcDispatcher`:** `with_http2_keepalive` and
  `with_max_connection_age`, both opt-in. `GrpcConfig` bounded message size and
  per-connection concurrency and nothing bounded a connection that simply
  exists: measured, 400 TCP connections opened against the dispatcher and left
  silent were all accepted, none refused, and the oldest was still alive twelve
  seconds later. HTTP/2 keepalive is the gRPC-native answer and distinguishes a
  peer that has stopped answering from one that is merely quiet, because a
  conformant client's HTTP/2 stack answers a PING without the application being
  involved — so a streaming RPC waiting for its next event is left alone. They
  live on the dispatcher rather than on `GrpcConfig` because that type has
  public fields and no `#[non_exhaustive]`, so adding a field to it would break
  every struct-literal construction.

- **Connection bounds for `WebSocketDispatcher`:** `with_max_connections` and
  `with_idle_timeout`. The handshake timeout — documented in that module as the
  slowloris defence — covers only the part before the upgrade completes;
  nothing bounded a peer that completed the handshake and then went quiet.
  Measured: 400 idle handshaken connections accepted and held with none
  refused, and a connection idle for 12 seconds still being served, because the
  read loop awaited the next frame with no bound and the accept loop spawned a
  task per socket with no ceiling. Both default to **off**, so nothing changes
  unless you ask. `max_connections` takes its permit before `accept()`, so
  excess load waits in the kernel's listen backlog rather than as unbounded
  tasks. `idle_timeout` counts traffic in *both* directions and sends a
  WebSocket Ping at the halfway mark, which conformant clients answer
  automatically — so it closes peers that are unresponsive rather than peers
  that are merely quiet. That distinction is why it is opt-in and why it is
  safe to opt in: an A2A subscription may legitimately be silent for hours.

- **`push_outcome::SKIPPED`**, reported once per push-notification config the
  per-event delivery budget never reaches.

- **A CI gate for the cancellation class:**
  `scripts/check_cancellation_release.py`. A file that claims a guard slot with
  `compare_exchange(false, true, ..)` must contain an `impl Drop` that releases
  one. Three of this release's defects were that shape — state claimed on one
  path and released on another, with an `.await` in between, so dropping the
  future runs neither — and all three were found by a person reading code.
  `clippy::await_holding_lock` covers the inverse case; nothing covered this
  one.

- **A reusable over-the-wire adversarial probe suite** (`scripts/adversarial/`),
  the third leg of this project's negative-input testing beside the
  unit/property tests and the libFuzzer harnesses. Seven black-box probes — one
  per request binding (JSON-RPC, WebSocket, gRPC), plus authentication,
  sustained concurrency, SSE streaming and outbound push delivery — send
  malformed, hostile and edge-case requests to a *running* server backed by a
  real model and, after every request, re-probe the agent card to confirm the
  process stayed up. They are standard-library only (the gRPC leg compiles the
  repo proto at startup) and exit `0`/`1`, so they double as CI gates.
  Documented in the book under **Testing & Deployment → Adversarial Testing**,
  with the full account of the three defects they found (see Fixed and Security,
  below). The push probe counts how many times the server actually dialed each
  hostile receiver, so a run that exercises nothing fails loudly rather than
  passing vacuously — the trap a wrong push-config field name
  (`pushNotificationConfig` for the correct `taskPushNotificationConfig`) had
  already sprung once.

### Fixed

- **The gRPC binding processed a request with no `a2a-version`.** JSON-RPC and
  WebSocket reject an absent protocol version — spec §3.6.2 reads absent as
  protocol 0.3, and §737 requires `VersionNotSupported` for a version this
  server does not speak, so a 1.x-only server must refuse it — and the gRPC
  binding did not, a cross-binding inconsistency only visible because the
  adversarial probe hits every binding with the same attack. `validated_metadata`
  ran its version check only when the value was present, so absent fell through
  to accepted, under a docstring that claimed it "mirror[ed]" the other bindings
  — which reject it. It now delegates to the shared `validate_version_metadata`
  the other three use, so all four agree. `GrpcConfig` gains
  `require_version_header` (default true, `with_require_version_header` to relax
  it), matching `DispatchConfig`. Verified over the wire: a versionless gRPC call
  now returns the same `VersionNotSupported` the other bindings emit, and the
  client-driven surface sweep stays 44/44, because the client injects the
  version.

- **Non-streaming `SendMessage` delivered webhooks on the request path.** The
  synchronous collector awaited push-notification delivery inline, so a webhook
  the caller registered on that same request delayed the response by up to the
  30-second delivery budget — measured 15 seconds against a hanging webhook for a
  three-event task. Streaming and `returnImmediately=true` were already
  unaffected, and it did not starve other clients (the blocked work is an async
  task, not a thread), but it coupled a caller's latency to a client-controlled
  endpoint and held task-store slots for up to 30 seconds, for a
  fire-and-forget notification whose errors are swallowed anyway — the webhook
  anti-pattern, and inconsistent with the streaming path, which already delivers
  off-path. Delivery is now buffered during collection and handed to a single
  spawned task that sends the events in order under the same budget and
  per-delivery timeout, with the tenant context carried across the spawn exactly
  as the streaming background processor does. Verified over the wire:
  `SendMessage` against a hanging webhook returns in 0.01 s (was 15 s) and the
  webhook is still dialed in the background. A regression test asserts the
  collector returns in under a second even against a sender that would sleep an
  hour.

- **A push-backoff test failed on runner speed rather than on behaviour**
  (`a2a-protocol-server`). `backoff_is_paid_between_attempts_but_not_after_the_last`
  asserted total elapsed in `[1800ms, 2800ms)` around three real loopback round
  trips, so its 800ms of headroom above the correct 2000ms had to absorb every
  scrap of per-request overhead. Measured on a Windows CI runner: **2954ms for
  a correct run**, failing by 154ms.

  It now asserts the gaps *between* request arrivals and the gap from the last
  arrival to `send()` returning. Each spans one round trip instead of three, so
  overhead cannot accumulate into the margin — and the assertions say what the
  test is named for: the second gap must differ from the first (1500ms, not
  500ms), and nothing beyond a response round trip may follow the final
  attempt. Its mutation coverage is unchanged and its messages are sharper: a
  trailing backoff now reports "1.501256836s elapsed between the last request
  arriving and send() returning" instead of a total that could mean anything.

- **`PerTenantConfig` now enforces the limits it declares** (B22). All five
  `TenantLimits` fields were stored, resolvable, and read by nothing in the
  request path, under a module header that advised operators to set
  `max_concurrent_tasks` for noisy-neighbour isolation. Four are now enforced
  and the fifth is deprecated in favour of a store-level equivalent (see
  Deprecated, above):

  | Limit | Where | On exceeding |
  |---|---|---|
  | `max_concurrent_tasks` | per-tenant semaphore; permit taken before any side effect and held for the executor's life | `ServerError::Overloaded` |
  | `executor_timeout` | resolved before the executor is spawned | the task fails |
  | `event_queue_capacity` | at queue creation | the stream buffer is that deep |
  | `rate_limit_rps` | `RateLimitInterceptor`, via the new `with_tenant_config` | the request is refused |

  The permit is taken *before* the queue, the task row and the cancellation
  token exist, so a refused request leaves nothing behind — rejecting later
  would make the limit cost the tenant the resources it protects. A tenant at
  its limit is refused rather than queued: queueing converts a declared bound
  into unbounded latency and unbounded memory.

  Three of the four the handler applies on its own. `rate_limit_rps` is opt-in
  and needs the same config handed to
  `RateLimitInterceptor::with_tenant_config`, because a request is counted in
  an interceptor rather than in the handler. It counts against a bucket keyed
  by tenant *in addition to* the per-caller bucket, so a tenant's allowance is
  not multiplied by its number of callers — and the unit is converted rather
  than reinterpreted: the per-window allowance is `rate_limit_rps ×
  window_secs`, because the field is documented in requests per second and
  `requests_per_window` is not.

  What made this enforceable at all was measuring where the tenant is visible.
  `TenantContext` is a `tokio::task_local!`, and a probe recording it at two
  points of one `SendMessage` saw `"acme"` in the interceptor chain and `""` in
  the spawned executor. So an interceptor can resolve a tenant, and anything
  the executor needs must be resolved before the spawn and moved in — which is
  where the timeout and the permit are now taken.

- **The WebSocket client's connect had no deadline.** `connect_async_with_config`
  was awaited bare, so `WebSocketTransport::connect*` could wait indefinitely on
  a server that completed the TCP handshake and then never answered the HTTP
  upgrade — the connection is established, so no OS timeout applies. Measured
  against a listener that accepts and holds the socket open in silence: still
  pending at 3 seconds, with no deadline to reach. Every sibling transport
  already bounded this (JSON-RPC and REST via `ClientConfig::connection_timeout`,
  gRPC via `GrpcTransportConfig::connect_timeout`), and
  `connection_timeout`'s own documentation names the hazard. `WebSocketTransportConfig`
  now carries `connect_timeout`, defaulting to the same 10 seconds and applied
  around the whole handshake — TCP, TLS, and upgrade. Reverting the fix does not
  fail the new test; it hangs the suite, killed at 120 s having never returned.

- **`ClientConfig::preferred_bindings` selected nothing.** The field documented
  an ordered client preference — *"the client tries each in order, selecting the
  first one supported by the target agent's card"* — and no code read it.
  `ClientBuilder::from_card` took `supported_interfaces.first()`, which is the
  **agent's** first choice, the inverse of the preference the field describes;
  the builder consulted a different, singular field with almost the same name.
  A caller who ranked `GRPC` and met a card advertising `[JSONRPC, GRPC]`
  silently got JSONRPC. `from_card` now honours the default preference list, and
  the new `ClientBuilder::from_card_preferring(card, preferences)` takes an
  explicit one. Matching is ASCII-case-insensitive, because the spec's binding
  names are upper-case and cards written by hand are not always. The resulting
  config records the preference that was applied rather than the default.

- **`with_protocol_binding` moved the binding and left the endpoint behind.** An
  agent card gives each binding its own URL, so the two are a pair. Measured
  against a card advertising JSONRPC at `:1111` and GRPC at `:2222`,
  `from_card(card).with_protocol_binding("GRPC")` produced binding `GRPC` at
  endpoint `:1111` — a client that would speak gRPC to the JSON-RPC port, with
  no error anywhere. A builder created from a card now retains that card's
  interfaces and moves the endpoint and tenant with the binding. Builders made
  with `ClientBuilder::new` are unaffected, which is every
  `with_protocol_binding` call site in this repository and its book.

- **One slow multicast consumer stalled every other member's stream**
  (`a2a-protocol-slimrpc`). `stream_message` gave each invited agent its own
  bounded channel — under a comment saying "a slow or silent agent cannot hold
  up another's events" — and then awaited `send` on those channels from a single
  shared loop. The split handles a *silent* agent, which produces no frames; it
  does nothing for a *slow consumer*, whose full channel parks the one loop
  every member's frames arrive through. Measured with two agents emitting 300
  events each and one consumer never polled: the live member's stream reached
  151 events in 25 seconds and never resumed. It now reaches 300 in 220 ms. The
  send is non-blocking, and a member that falls behind is handed an error naming
  how many events it missed rather than a stream with a silent hole in it —
  the same contract the SSE fan-out already has through broadcast's `Lagged`.

- **The Axum router ignored `body_read_timeout`.** `DispatchConfig` carries two
  body bounds; this router honoured `max_request_body_size` — wired in
  deliberately, with a comment explaining that Axum's `Bytes` extractor would
  otherwise fall back to its own `DefaultBodyLimit` and make the knob a no-op —
  and never applied the timeout sitting beside it. Measured with
  `body_read_timeout(1s)`, announcing a 1000-byte body and sending 8:
  `JsonRpcDispatcher` answered at 1.002 s and the Axum router had said nothing
  after 12 seconds. A slowloris body is what the knob is for, and the one shape
  a size cap cannot catch — the bytes never arrive, so the cap is never
  reached. A `TimedBody` extractor now reads the body under the configured
  deadline and answers `408 Request Timeout`; measured at 1.002 s.

- **`TaskStoreConfig::max_page_size` did nothing on any SQL store.** The four
  SQL stores hold a connection pool and no config, and each carried its own
  hardcoded `n.min(1000)` — the same number `TaskStoreConfig` uses as its
  default, so the configurable bound and the hardcoded one agreed by
  coincidence. Measured with the cap set to 10 against 60 stored tasks and a
  client asking for 100: `InMemoryTaskStore` returned 10 and `SqliteTaskStore`
  returned all 60. The book documented the field as capping `list` generally,
  under Design Considerations rather than under any one store. Each SQL store
  now takes `with_max_page_size`, all five sites default to the new
  `DEFAULT_MAX_PAGE_SIZE`, and the book says which config applies to which
  store. Third instance of one shape: a configurable bound whose default equals
  the hardcoded fallback, inert until the person who cares tightens it.

- **Concurrent creates walked straight through the per-task push-config cap.**
  `max_push_configs_per_task` is enforced by reading the task's configs,
  deciding, then storing — two `.await` points apart, with nothing held across
  them. Every concurrent caller read a count under the cap and every one of
  them stored. Measured against a cap of 5 with 32 concurrent creates, three
  runs stored 12, 17 and **32** — the last being all of them, a documented
  ceiling doing nothing at all. Unlike the task store's transient overshoot
  this was permanent: nothing re-checks a stored config. The sequence now holds
  a per-task lock, reusing the same bounded facility `SendMessage` already used
  against the identical race on `context_id`; the same three runs now store
  exactly 5. The *global* cap remains approximate across distinct tasks
  (measured 10, 5, 5 against a cap of 5) — making it exact means one
  server-wide lock on every create, which is a throughput decision rather than
  a bug fix, and is recorded as backlog B20.

  Worth knowing who this bit: `InMemoryPushConfigStore` has its *own* per-task
  cap, default 100, enforced atomically under its own lock, and the builder
  never passes `HandlerLimits` to it. At the shipped defaults both are 100 and
  the store's correct check masked the handler's racy one — so the defect
  appeared for an operator who *lowered* `max_push_configs_per_task`, and for
  every SQL-backed deployment at any setting, since those stores do not
  self-enforce and the handler's check is the only one there is.

- **`with_stream_connect_timeout` did nothing on the gRPC transport.**
  `ClientBuilder` carries three timeouts and `build_grpc` passed two, dropping
  the third; the gRPC transport then bounded a stream's first event on the
  *unary request* timeout instead. Both default to 30 seconds, so the knob only
  failed once you set it — and the sync `build()` path even rejects a zero
  value for it while `build_grpc` silently accepted one. `build_grpc` now
  validates and passes it, and the transport gained
  `with_stream_connect_timeout` (on the transport rather than on
  `GrpcTransportConfig`, whose public fields make adding one a breaking
  change). REST and JSON-RPC were already correct; the WebSocket transport has
  no such knob and consistently uses its own.

- **A hot-reload SIGHUP watcher could abort the process, minutes after
  startup.** `spawn_signal_watcher` registered its handler *inside* the spawned
  task, and `tokio::signal::unix::signal` panics — it does not return an error
  — when the runtime has no signal driver, which is what a hand-built runtime
  without `enable_all()` gives you. The panic therefore arrived after the
  function had returned a `JoinHandle`: nothing for the caller to see coming,
  nothing to catch, and under this workspace's release `panic = "abort"` a
  process abort rather than one lost feature. Registration now happens
  synchronously in `spawn_signal_watcher`, so the failure lands in the caller's
  own startup path where it is deterministic — and the `# Panics` section is now
  attached to the function that actually panics. The loop also stops discarding
  the `Option` from `recv()`, which was a latent hot loop: `None` means no
  further signals can arrive, and it answered by asking again immediately,
  forever.

- **`InMemoryCredentialsStore`'s `Debug` panicked on a poisoned lock.**
  Formatting runs while somebody is diagnosing the first failure, often from a
  logging path, so a panicking `Debug` replaces the diagnosis with a second
  failure — and a process abort in release. It now reports `<lock poisoned>`.
  The accessors on the same type still propagate poisoning, deliberately: a
  credentials lookup that quietly returns `None` is a silent auth downgrade,
  which is worse than a panic. Different answers for different call sites, both
  now written down.

- **`HotReloadAgentCardHandler` turned one panic into permanent failure.** Both
  accessors `expect`ed on their `RwLock`, and poisoning is sticky, so a single
  panic anywhere under the write lock made `current()` — the function that
  answers `GetAgentCard` — panic from then on, and abort the process in release.
  Both now recover from poisoning, which is correct rather than merely
  convenient here: the only write is a whole-value assignment of an
  already-constructed `AgentCard`, so no reader can observe a half-updated one.

- **`max_tenants` was documented by the half of its effect that is
  comfortable.** "Prevents unbounded memory growth from tenant enumeration
  attacks" is true and stops one step short: tenant ids come from resolvers
  that all read client-controlled input, so an enumerator does not get memory —
  it gets a lockout of every tenant that arrives afterwards. The reclamation
  path, `prune_empty_tenants`, is not automatic and only removes partitions
  whose task count is zero, so at the shipped one-hour TTL a burst of 1,000
  junk tenant ids locks out new tenants for an hour even with pruning
  scheduled. Both methods now document that, with a test pinning it.

- **`InMemoryTaskStore`'s documentation described a design it does not have.**
  It said eviction "runs as a background task" and that "writers are not
  blocked during the O(n) cleanup". There is no `spawn`: the sweep is awaited
  inside `save` and holds the write lock for its whole duration, so the write
  that triggers it pays for it and every concurrent writer waits. Measured at
  50,000 terminal tasks with `eviction_interval` 1000, the quietest of 1,000
  consecutive saves took 3.99 µs and the one that swept took 4.54 ms — about
  1,100×. The behaviour is reasonable and unchanged; what was wrong is that the
  paragraph an operator reads to size a latency budget named the two properties
  that would have made the number not matter. Corrected with the measurements
  and a note that `eviction_interval` is a tail-latency knob. The observable
  half — `save` never returns with the store over capacity — now has a test.

- **A cancelled WebSocket request leaked its entry in the client's pending
  map, and so did an abandoned stream.** The map is keyed by JSON-RPC request
  ID, has no capacity bound, and lives as long as the connection. An entry was
  removed on exactly three paths — a routed response, the explicit timeout
  branch, and connection teardown — and a caller whose future is *dropped*
  takes none of them; neither does a consumer that walks away from a stream the
  server never fed, because all three streaming removals need a frame to
  arrive. Measured: five cancelled requests left five entries, five abandoned
  streams left five more, each pinning a channel sender. Registration moved to
  the caller and both ends are now owned by a `Drop` guard, which travels with
  the `EventStream` for the streaming case. A `select!` racing a request against
  a shutdown signal is an ordinary way to write a client, so on a long-lived
  connection this was unbounded growth on the request path.

- **The event queue's `write_timeout` was never applied.** `DEFAULT_WRITE_TIMEOUT`
  is public, `new_in_memory_queue_with_options` takes it, `EventQueueManager`
  prints it in `Debug` — and the field was `#[allow(dead_code)]`. The
  persistence channel is a bounded `mpsc` whose `send` waits with no deadline of
  its own, so a stalled background processor parked the executor indefinitely:
  measured, `write` blocked after 1,024 events and was still blocked eight
  seconds later, with no metric and no trace. The deadline is now enforced and a
  full channel returns an error naming the cause. A *closed* channel is still
  not an error. The surrounding documentation, which said this channel "will
  never lose events" / "will never lag" / that the processor "must never miss
  events", now says what it does: full past the deadline is an error naming the
  stalled processor, closed drops the event and returns `Ok`, and the second is
  reported only by a `trace_warn!` that compiles to nothing without the
  non-default `tracing` feature.

- **Push-notification configs the delivery budget never reached were invisible.**
  At the shipped defaults — 100 configs per task, a 5-second per-delivery
  timeout, a 30-second per-event budget — a webhook estate that is timing out
  receives 6 of 100 and the other 94 were reported by a single `trace_warn!`,
  which compiles to nothing without the non-default `tracing` feature. Each
  skipped config now increments `push_outcome::SKIPPED`. The `Semaphore::new(16)`
  built per event, described in a comment as a cap on concurrent deliveries that
  the sequential loop never performed, has been removed.

- **`shutdown_with_timeout(t)` could take `2t`.** The drain phase ran to
  `now + t` and the executor's cleanup hook was then given a *fresh* full `t`.
  Measured: `shutdown_with_timeout(30s)` took 60 seconds. The number an operator
  puts here is the number they put in `terminationGracePeriodSeconds`, so
  overrunning it means `SIGKILL` part-way through the cleanup graceful shutdown
  exists to perform. The two phases now share one deadline.

- **The push retry policy could not run at the shipped defaults.**
  `HttpPushSender::new()` schedules three attempts at a 30-second request
  timeout with `[1s, 2s]` backoff — 93 seconds — inside a
  `push_delivery_timeout` that defaults to 5. Measured against a real socket:
  one attempt of three reaches the webhook. The defaults are unchanged, because
  choosing between them is a deployment decision; what is new is
  `PushSender::max_delivery_duration()` (a defaulted trait method) and
  `push_outcome::TIMEOUT_TRUNCATED`, so a truncated schedule is counted rather
  than mistaken for a slow endpoint, and the arithmetic is documented on
  `HandlerLimits::push_delivery_timeout`.

- **Agent-card discovery applied its 30-second timeout twice.** The request and
  the body read each had their own, so a server that stalled on headers and then
  drip-fed the body held the caller for both: measured at **55 seconds** against
  a documented 30. One deadline now covers the whole fetch.

- **The JWKS and OIDC-discovery body read had no deadline at all.** The
  30-second budget bounded the request; the body was bounded by a size cap,
  which bounds memory and not time. A server dripping one byte every 300ms held
  the fetch open indefinitely — measured still running at 45 seconds. This sits
  on the request path, because a `kid` that misses the cache forces a JWKS
  refetch inside token validation.

- **`codecov.yml`'s PostgreSQL exclusion has never applied.** All five listed
  paths are still in the coverage denominator, together with two never listed:
  957 lines and 881 of 2,096 missed lines — 42% of every uncovered line in the
  repository. Reported coverage is 94.06%; without them it is 96.46%. The
  patterns are deliberately unchanged (Codecov *accepts* them, so validation
  proves nothing); `scripts/check_codecov_ignores.py` is the check that will
  settle the next attempt.

- **Four broken rustdoc intra-doc links** in `rate_limit/` that resolved only
  under workspace feature unification, so `cargo doc -p a2a-protocol-server`
  emitted four warnings while CI's `cargo doc --workspace` emitted none.

- **`signing` feature descriptions overstated what they enable.** Neither
  `a2a-protocol-client` nor `a2a-protocol-server` contains any
  `#[cfg(feature = "signing")]` code; both forward the types-crate feature so
  you can call `sign_agent_card` / `verify_agent_card` yourself. The client's
  read "Enable agent card signing verification", which claimed an integration
  that does not exist.

- **Per-caller rate limiting was not per-caller.** `caller_identity` existed on
  the interceptor and no code path ever set it, so every caller shared a single
  bucket and one noisy client could exhaust the limit for everyone. The identity
  is now settable and is set from the authenticated principal.

- **The SQLite artifact journal was created only by `from_pool`**, so a store
  opened through the migration path lacked the table the incremental artifact
  writer depends on.

- **SLIMRPC did not transmit or check the protocol version**, and leaked
  internal transport metadata into the A2A header map that applications see.

- **The SLIMRPC binding's packaging gate could not pass during a release.** The
  binding depends on the SDK crates by `version` *and* `path`; `cargo package`
  strips the path, so the pin resolves against crates.io — where the version
  being prepared does not exist yet. Reverting the pin broke the binding's build
  instead, so no pin value was green between the version bump and publication,
  and the 0.10.0 prep was reverted rather than accommodated. `ci.yml` now runs
  `scripts/package_binding.py`, which skips registry resolution alone for
  exactly that state — every pin naming the in-tree version, and that version
  absent from the index — proves the rest of packaging with `cargo package
  --list`, and fails on everything else, a typo'd pin included. The skip is a
  warning annotation on the job, and it closes itself once the SDK is published.

- **The nightly canary tested but never linted**, so it could not give notice of
  the breakage it exists for. Rust 1.98.0 turned three clippy lints red on code
  identical to `main`, two of which (`map_or_identity`,
  `unused_async_trait_impl`) did not exist in 1.94.1 at all — and the canary ran
  only `cargo test`. It now runs `cargo clippy --all-targets --all-features -D
  warnings` as well, on the same informational job, which is where a lint
  arrives before it reaches stable. The toolchain is still not pinned: a
  `rust-toolchain.toml` takes precedence over the toolchain the CI action
  selects, which would silently redirect the MSRV leg of the `1.93` matrix at
  the pinned version instead — a worse failure than the one it would prevent.

- **The SLIMRPC spec drift check was blind to any spec file it had not been
  told about.** It fetched two files by URL and hash-compared them, so it could
  report agreement about those two while upstream added a third — which is what
  happened. `spec/v1/slimrpc-collaborative-channel.md` has existed on an
  upstream branch, and the official `a2a-slimrpc` crate has implemented it,
  without anything here noticing. The check now clones upstream and takes its
  inventory from `main`, so an added or withdrawn spec file fails as loudly as a
  changed one, and it surveys the other branches: a branch-only spec must carry
  a written disposition or CI fails until somebody triages it.

- **The provenance manifest had gone stale, in the project's favour.**
  `docs/provenance-manifest.md` is written for a downstream project's counsel —
  the A2A project and the Linux Foundation are named in it — and nothing forced
  it to be regenerated. It still reported 749 commits and 19.4% of history
  passing the project's own DCO gate; re-measured at `7093af3` the figures are
  977 commits and **39.2%**. A counsel-facing document that understates the
  project is the harder defect to notice, because nobody is motivated to check
  it. `release.yml` now runs `scripts/check_provenance_manifest.py`, which fails
  a release whose manifest was measured at a different commit or whose headline
  figures do not match a fresh run, and refuses outright on a shallow clone —
  the truncation that made the two previous hand-derived figures wrong by 2.3x.

- **`SECURITY.md` understated release-artifact verifiability.** It said all
  release tags were lightweight and unsigned, so there was "nothing to verify".
  `v0.8.0` and `v0.9.0` are annotated tag objects carrying a tagger and a date —
  `release.yml`'s annotated-tag gate working, in the two releases cut since it
  landed. Signing remains a genuine gap and is still stated as one. The same
  stale claim is corrected in `RELEASING.md`, `ROADMAP.md` and `PROVENANCE.md`.

- **`prove_workflow_gates_fail.py` could not see a new script-invoking step.**
  It discovered gates by looking for an explicit `exit` in the step body;
  steps whose verdict is a checker's exit code were reachable only through a
  hand-maintained list, so *adding* one left it silently unproven. Discovery
  now also matches a step running one of this repository's own scripts, which
  turns a forgotten registry entry into a failed build.

- **The SQLite connection pragmas were written out four times.** `journal_mode`,
  `busy_timeout`, `synchronous` and `foreign_keys` appeared byte-identically in
  `store::sqlite_store`, `store::tenant_sqlite_store`, `push::sqlite_config_store`
  and `push::tenant_sqlite_config_store`, with nothing asserting they agreed and
  three of the four also hard-coding the same pool size. Correcting one in
  response to a bug report would have left the other three at the old value with
  every test still passing — the shape `DEFAULT_MAX_PAGE_SIZE` already cost this
  repository once. They now live in one private module, with a test that asks
  `SQLite` for the effective values on a real file rather than asserting the
  builder was called. Measured while writing that test: removing `journal_mode`
  or `synchronous` fails it, and removing `busy_timeout` or `foreign_keys` does
  not, because `sqlx` already defaults both to the values being asked for. Both
  facts are now recorded next to the pragmas.

- **The `unwrap`/`expect` count in library code is now a CI ratchet** rather
  than a number typed into `CONTRIBUTING.md` that no method could reproduce
  (B5). `scripts/check_panic_paths.py` strips comments and string literals with
  a state machine and follows `#[cfg(test)]` gating transitively — a file
  declared by a test-gated module is test code too, even though it carries no
  attribute of its own and no "test" in its name. The measured surface of the
  published crates: **0 `.unwrap()`, 10 `.expect(`, 0 `panic!`, 0 `todo!` in
  runtime library code**, with `build.rs` reported separately. Adding one now
  fails the build until the baseline is updated deliberately.

- **`AgentExecutor::cancel`'s documentation described behaviour it stopped
  having in 0.7.** The rustdoc said "the default implementation returns an error
  indicating the task is not cancelable. Override this to support task
  cancellation." The default has cancelled since 0.7 — it emits the terminal
  `Canceled` status — and the inline comment beside it says the refusing default
  was removed precisely because it left `Working` tasks uncancelable out of the
  box. A reader of docs.rs would have believed the opposite of what the code
  does, and would have written an override to get behaviour they already had.

- **Four book chapters, and a gate that could not see whether they compiled.**
  `deployment/{troubleshooting,observability,multi-tenancy,security}.md` close
  B10. Separately: `a2a-book-tests` registers pages by hand, and nothing checked
  the list — an unregistered page's Rust was never compiled and nothing said so,
  because a page with no `ignore`d blocks looks exactly like a page being
  compiled. Four pages were already unregistered. `check_book_code.sh` now fails
  on an unregistered page, and on an exclusion whose page no longer exists.

- **The example coverage matrix could print a total larger than the grid.**
  `a2a-example-harness` scores which A2A methods ran over which bindings, and
  its summary took "exercised" from one collection and "not applicable" from
  another — two collections a single cell can be in at once. A cell that was
  both excused and then exercised, or excused twice, was counted twice, so the
  line read `1 exercised, 1 not applicable, 43 missing, of 44 cells`. Both
  numbers now come from one classification of the grid, so the three buckets
  partition it by construction; `excuse` deduplicates as `record` always has;
  and an excuse for a cell that was then exercised no longer prints under "Not
  applicable" while the grid shows it as `ok`.

- **Four examples each carried their own copy of the agent card's interface
  list.** `Endpoints` and `interfaces()` were byte-identical in `genai-agent`,
  `rig-agent` and `multi-lang-team`, and `echo-agent` wrote the same four
  `AgentInterface` literals inline. Each copy hand-wrote `"HTTP+JSON"` — the
  string the SDK ships `AgentInterface::rest` to avoid, its own documentation
  saying a typo there "is a card that lies". All four now use one
  implementation in `a2a-example-harness`, built from the SDK's constructors,
  and a test asserts the card's `protocolBinding` values match the coverage
  matrix's column labels, which were two independent spellings of the same
  four names.

- **The three LLM-backed examples had no tests, and their success paths were
  assumed untestable.** `genai-agent`, `rig-agent` and `multi-lang-team` now
  have 19 tests between them, none of which touch a network or a model.
  `genai`'s service target is overridable, so the unreachable-provider branch
  runs against a dead endpoint identically on a laptop with `llama-server` up
  and on a CI runner with nothing; `rig`'s executor is generic over
  `CompletionModel`, so a fake that answers makes the *success* path testable
  with no provider at all — including that a real answer is not dressed as a
  mechanical fallback. Every example in the repository now has tests.

- **Every example was run end to end against a real model** — `Qwen3.5-0.8B`
  under `llama.cpp` — rather than only against fakes: 44/44 protocol cells
  each, 102/102 for `agent-team`, and zero mechanical fallbacks in any LLM
  run, so the fallback paths the unit tests exercise really are the degraded
  path. Recorded with the model's size and sha256 in
  `docs/lf-readiness-review.md` so the run is reproducible.

- **`rig-agent`'s README pointed at a llama.cpp download that 404s.** Its
  quickstart fetched `releases/latest/download/llama-bin-ubuntu-x64.tar.gz`;
  llama.cpp's release assets carry the tag in the filename, so no tag-less
  `latest/download` URL can resolve. Replaced with a from-source build,
  verified by running the commands exactly as written into a clean directory.

### Performance

- **Streaming artifact appends no longer pay for the stream so far.** SQLite
  journals appended artifact parts instead of rewriting the whole task on every
  delta.

### Security

- **The registration-time webhook filter now normalizes numeric IPv4
  encodings.** `validate_webhook_url` accepted the non-canonical forms the C
  resolver (`inet_aton`, and so `tokio`'s `lookup_host`) maps to private
  addresses — `http://2852039166/`, `http://0xA9FEA9FE/` and
  `http://0251.0376.0251.0376/` all denote `169.254.169.254`, the cloud metadata
  endpoint — because `Ipv4Addr::from_str` only accepts dotted-quad. Delivery
  already blocked them (`lookup_host` resolves the integer and the IP-range check
  re-runs on the resolved address), so this was a defense-in-depth gap rather
  than a live SSRF; it is nonetheless the exact attack class the filter targets,
  which already rejected the IPv4-mapped and NAT64 spellings of the same address.
  The static check now decodes the decimal, hex and octal forms and applies the
  same private-range test; public numeric hosts still pass, matching delivery.
  Found by an over-the-wire adversarial run against a live model — 20/20 hostile
  webhook URLs rejected with the guard on, up from 19/20.

- **`examples/genai-agent` no longer ships with the SSRF guard disabled.** Its
  server mode — labelled "the real deployment shape" in its own comment —
  hard-coded `allow_private_urls()`, silently turning the guard off for anyone
  adopting the example as a deployment template. It is now secure by default;
  `A2A_ALLOW_PRIVATE_WEBHOOKS=1` re-enables loopback webhooks for local testing,
  and the active posture is printed at startup. (Example crate, `publish =
  false`, so no published crate changed — but the template did.)

- **RUSTSEC-2026-0258** — h2 raised to 0.4.16, without the dependency downgrade
  the advisory's own suggested fix would have pulled in.

### Changed

- **The SLIMRPC binding now states where it diverges from the official
  `a2a-slimrpc` crate**, in its README, the book chapter and
  `spec/slimrpc_v1/README.md`. Both implement all eleven A2A methods, but
  neither is a superset of the other: this crate implements the multicast
  specification on upstream `main` and the official one does not; the official
  one implements Collaborate, whose specification is on an unmerged upstream
  branch, and this one does not. The two are different operations rather than
  two names for one, so "which is more complete" is the wrong question — the
  docs now say so with a comparison table instead of leaving a reader to infer
  it. Tracked as B24.

- **`PerTenantConfig` and `TenantLimits` now document that nothing enforces
  them** (`a2a-protocol-server`). All five per-tenant limits are stored,
  resolvable through `PerTenantConfig::get`, and read by no code in the request
  path — verified by enumerating every mention of the type outside its own
  module: an import, a field, the `tenant_config()` accessor, a `Debug` line, a
  builder field and its setter, and otherwise tests. `executor_timeout` and
  `event_queue_capacity` share their names with live process-wide fields on the
  handler and its builder, which is what made them look wired; those fields are
  not these fields.

  The module header previously advised operators to *"set per-tenant
  `max_concurrent_tasks` so that the sum across active tenants stays within the
  process-wide caps if noisy-neighbor isolation matters"* — advice to rely on a
  field nothing reads. Enforcing all five is a feature with open design
  questions (B22 in `docs/v0.9.0-post-release-review.md`); enforcing some of
  them would leave live knobs beside inert ones, which is the defect shape
  itself. So the behaviour is unchanged and the documentation is now exactly
  true: the module states what it stores and resolves, each field names what
  would enforce it, and the two builder setters no longer claim to enable
  per-tenant limits. **Data isolation is unaffected** — it runs through the
  tenant-aware stores' partitioning, not through any limit here.

- **`ClientConfig::max_response_size` now states what it reaches.** It governs
  every transport `ClientBuilder` constructs — JSON-RPC and REST directly, gRPC
  as its `max_message_size` — and not a transport supplied to
  `with_custom_transport`. Two shipped transports are in the latter position.

  `WebSocketTransport` is the nearer one, and the more misleading:
  `WebSocketTransportConfig::max_message_size` defaults to the *same constant*
  as `max_response_size`, and its doc said it was "the same response-size
  ceiling the HTTP and gRPC transports apply" — true of the default, and
  reading as though the two were connected. They are not. Tightening
  `max_response_size` to 1 MiB and connecting over WebSocket still admits
  32 MiB. Both ends now say so, and point at the setting that works.

  `SlimRpcTransport` is the other, and its real bound was traced through the
  stack rather than assumed:
  the binding's codec, `slim_rpc` 2.3 and `agntcy-slim-datapath` 0.18 all set
  none, so what applies is tonic 0.14's default — **4 MiB on receive**, eight
  times tighter than the 32 MiB documented here, and unbounded on send. Neither
  is settable from this repository; both are now written down where a reader
  hits them.

## [0.9.0] - 2026-08-16

### Breaking

- **`executor_timeout` now defaults to one hour instead of being unbounded.**
  An executor that never returns previously pinned its task, its event queue and
  its cancellation token for the life of the process, and nothing reclaimed
  them; with `max_cancellation_tokens` at 10,000, enough of them eventually stop
  the handler accepting work. The old rationale — that any fixed value would
  fail legitimately long-running tasks — is sound about *short* ceilings and
  wrong about the default, which put the safe configuration behind an action
  nobody is reminded to take.

  An hour cannot plausibly interrupt an interactive or streaming agent turn, and
  a task genuinely running longer should be using push notifications (§7) rather
  than holding an executor and a stream open. If yours legitimately exceeds it,
  set `with_executor_timeout()`, or call the new `without_executor_timeout()` to
  restore unbounded execution explicitly. A task that trips the ceiling fails
  visibly, as a Failed task with a timeout error.

- **`RequestHandler::shutdown()` and `shutdown_with_timeout()` now return
  `ShutdownReport`** instead of `()`. The type is `#[must_use]`, so existing
  callers get a warning rather than an error. Both previously discarded the
  executor-cleanup timeout with no log and no return value, which made a hung
  cleanup indistinguishable from a clean drain.

### Added

- **Two `Metrics` callbacks for the failures nothing else reported.**
  `on_persistence_error` fires when the background processor cannot persist a
  task — the SDK's one silent-data-loss path, since the streaming reader is a
  separate subscriber and receives the event whether or not the store accepted
  it. `on_push_delivery` reports every delivery attempt by outcome. Both were
  previously visible only through `tracing`, which is not a default feature of
  `a2a-protocol-server`, so a default build discarded them entirely. Both are
  exported by the bundled OTLP exporter as `a2a.server.persistence_errors` and
  `a2a.server.push_deliveries`.

- **`GET /ready`** on the Axum router — a readiness probe that actually reaches
  the task store, alongside `/health`, which remains a constant on purpose. A
  liveness probe that follows a downstream down restart-loops every replica
  during that downstream's outage. Backed by the new
  `RequestHandler::task_store_health()`.

- **`TaskStore::save_artifact_delta`**, defaulted to `save`, so every existing
  implementation keeps working. Lets a store persist what changed rather than
  the whole record. Implemented by all three bundled stores.

- **`ErrorCode::metric_label` / `A2aError::metric_label`** — a bounded,
  low-cardinality label for every error code, safe to use as a metric dimension.

- **`a2a-protocol-slimrpc` is publishable**, and the SLIMRPC specification is
  vendored at `spec/slimrpc_v1/` so the binding's method inventory is checked
  against the document it claims to implement.

- **`examples/deploy-agent`** — the smallest agent you can actually ship:
  environment configuration, liveness and readiness endpoints, a `SIGTERM`
  drain, a `0.0.0.0` bind, a two-stage `Dockerfile` and a Kubernetes manifest.

- **`examples/hello-agent`** — 23 lines of code against the umbrella crate
  alone.


- **`EventStream::from_event_channel` on `a2a-protocol-client`, which is what
  made an out-of-tree custom transport possible at all.**
  `Transport::send_streaming_request` must return an `EventStream`, and every
  constructor for one was `pub(crate)` — so a third-party binding could
  implement the unary half of the trait and not the streaming half. The trait is
  `pub`, its parameters are `pub`, its return type is `pub`, and it was still
  unimplementable outside the crate.

  The new constructor takes a `tokio::sync::mpsc::Receiver` of decoded
  `StreamResponse` values, so a transport hands over domain events and never
  has to know that the internal representation is SSE. Sending `Err` delivers
  that error to the consumer, which is how a transport reports a mid-stream
  decode failure — ending the stream silently would be indistinguishable, to
  the consumer, from finishing normally. The returned stream aborts its bridging
  task on drop, exactly as the built-in transports' streams do.

  Purely additive. Found by building `a2a-protocol-slimrpc` against the
  extension points rather than by reading them; `docs/rust-sdk-assessment.md`
  §4.1.1 has the correction to what it previously claimed.

- **`bindings/a2a-protocol-slimrpc` — the SLIMRPC protocol binding.** Carries
  A2A over the [AGNTCY SLIM](https://github.com/agntcy/slim) fabric per
  [`a2aproject/experimental-cpb-slimrpc`][slimrpc-spec], advertising
  `protocolBinding: https://a2a-protocol.org/bindings/experimental-slimrpc/v1`
  and addressing agents as `slim://[node[:port]/]domain/namespace/service`.

  All eleven methods in the spec's inventory — nine unary, plus
  `SendStreamingMessage` and `SubscribeToTask` as unary-request /
  streaming-response. Payloads are the canonical `lf.a2a.v1` protobuf messages
  (SLIMRPC uses the same service definitions as gRPC), so the wire is
  byte-compatible with the official Go, Python and Java SDKs. Error identity
  travels as the spec's `TaskNotFoundError: …` message prefix, because SLIMRPC
  has no `google.rpc.ErrorInfo` equivalent and a status code alone cannot
  distinguish `TaskNotCancelableError` from `ExtensionSupportRequiredError`.

  `SlimRpcServer` drives the same `RequestHandler` every other binding drives,
  so task state, streaming, push, tenancy and authorisation are not
  reimplemented and an agent behaves identically however it is reached.

  **Deliberately outside the workspace, with its own `Cargo.lock`.**
  `agntcy-slim-rpc` brings 379 transitive dependencies including `aws-lc-sys`, a
  native C crypto build; `a2a-protocol-types` has 12. None of that reaches the
  lockfile, `deny.toml` allow-list or audit surface of the four published
  crates, and none of them depends on this one.

  27 tests. The seven end-to-end ones are not mocked: one in-process SLIM
  `Service` hosts an agent app and a caller app and messages cross the real SLIM
  datapath, covering method registration, a unary round trip, a task fetched
  back by id, error identity surviving the fabric, a streaming send running to
  its terminal event, agent-card advertisement, and an unknown method being
  reported rather than hanging.

  [slimrpc-spec]: https://github.com/a2aproject/experimental-cpb-slimrpc

- **SLIMRPC multicast — one message, several agents, one outcome each.**
  Implements the separate [`spec/v1/slimrpc-multicast.md`][slimrpc-spec]:
  `SlimRpcMulticast` opens a SLIM group channel, invites specific agents by
  name, and broadcasts. Only `SendMessage` and `SendStreamingMessage` may be
  broadcast — task management stays point-to-point, because a task id is
  meaningful to exactly one agent.

  `MulticastOutcome` carries **exactly one outcome per invited agent**, which is
  the spec's requirement (*"Clients must wait for outcomes from every invited
  agent"*) and the reason multicast is not a `Transport`: `send_request` returns
  one value, and reducing N attributable answers to one would have to drop
  either the attribution or the failures.

  Two failure kinds stay distinct because they call for different responses. An
  agent that errors or stays silent past the timeout is an isolated per-agent
  outcome and the other agents' answers stand; a member that cannot be *invited*
  fails the whole call, because the group is misconfigured and waiting will not
  fix it. That line is the spec's own: *"Only channel creation, agent
  invitation, or request delivery failures constitute interaction-level
  failures."* `stream_message` gives each agent its own `EventStream`,
  demultiplexed from SLIM's interleaved source-tagged frames.

- **Verified across a real SLIM node.** `tests/remote_node.rs` runs three
  separate SLIM services in one process — an agent, a client, and a node that
  only routes — connected over loopback TCP. The agent and client share no
  `Service` and no memory; every message crosses a socket twice and is routed in
  between. `SlimRpcServer::from_app_with_connection` and
  `SlimRpcTransport::from_app_with_connection` take the connection id
  `Service::connect` returns.

  This found a real bug that in-process testing structurally could not: nothing
  announced a *client's* own name to the node, so while the agent was reachable,
  no route existed for its reply, and every call failed its session handshake
  with the caller's own name reported as unroutable. `Channel` sets a route
  outwards only. The client-side constructor is now `async` and subscribes the
  caller's name over the connection.

- **SLIMRPC verified across every topology a deployment actually uses**, and
  identity generalised beyond shared secrets. The previous entry listed these as
  limitations; they are now covered rather than documented.

  | Suite | Topology | What only this can catch |
  |---|---|---|
  | `remote_node_tls.rs` | one node, **verified TLS** | a TLS path that verifies — proven by refusing an untrusted CA |
  | `remote_node_multihop.rs` | **two peered nodes** | subscriptions crossing a node-to-node link |
  | `out_of_process.rs` | node in a **separate OS process** | anything relying on shared memory or a shared runtime |

  TLS uses a throwaway CA generated per run rather than committed PEM files, so
  nothing long-lived is in the repository and no certificate can expire the
  build. The rejection test is a differential against the same node in the same
  window — the trusted CA connects, the untrusted one does not — because SLIM
  retries a failed handshake rather than returning, and a bounded wait alone
  would only prove slowness.

  Multi-hop needs `ConnType::Peer` between nodes: an `Edge` link carries an
  attached app's traffic but does not share routing state, so an agent behind a
  second node stays invisible without it.

  `SlimRpcServerBuilder::with_identity` and the transport's equivalent now take
  SLIM's own `AuthProvider` / `AuthVerifier`, so JWT, SPIFFE via SPIRE and
  static tokens all work without this crate enumerating mechanisms and lagging
  behind SLIM. `with_shared_secret` becomes a convenience over it and is now
  fallible, since SLIM rejects a secret too short to be a credential. A builder
  with no identity is a build error: SLIM has no anonymous mode, and a default
  would quietly stand in for one.

- **SPIFFE and mutual TLS, both verified rather than asserted.** The previous
  entry listed SPIFFE as "supported but untested" and mutual TLS as absent.

  `spiffe.rs` runs against a **real** `spire-server` and `spire-agent`: the
  agent attests the test process over the Workload API and issues genuine
  JWT-SVIDs. A stub would have proven only that the types line up. Three tests —
  an A2A call carried by SPIFFE identity, the issued SPIFFE ID pinned to the one
  registered, and a verifier refusing a genuinely-issued SVID minted for a
  *different audience*. That last one is what gives the first its meaning: a
  verifier that accepted everything would pass the positive test.

  Two SPIFFE properties surfaced only by running it, both presenting as a
  session that never completes rather than an authentication error, and both now
  documented: `SpireIdentityManager` must be built **once and cloned** for
  provider and verifier (it holds an MLS signature key, so two managers carry
  two different ones), and each app needs its **own** SPIFFE ID (two apps
  sharing one cannot complete an MLS handshake). The testbed therefore registers
  an identity per app and selects between them with `with_target_spiffe_id`.

  `remote_node_mtls.rs` covers mutual TLS: a node that authenticates its apps,
  not merely itself. Three cases, meaningful only together — a client with a
  certificate from the node's client CA connects and A2A works; a client with no
  certificate is refused; a client with a well-formed `ClientAuth` certificate
  from a *different* CA is refused. The first alone would pass just as happily
  against a node ignoring client certificates entirely.

  Every negative case is paired with a control that succeeds in the same window,
  because SLIM retries a failed handshake rather than returning, and "did not
  connect" on its own can mean the fixture was broken rather than the control
  working.

  I had previously recorded that running SPIRE needed an agent this environment
  did not have. That was not checked, and it was wrong — SPIRE runs here fine.

- **SPIFFE trust-domain federation and credential rotation, both against real
  SPIRE.** The previous entry listed these as limitations.

  `spiffe_federation.rs` runs **two** independent SPIRE deployments. An SVID
  from an unfederated domain is refused; after a bundle exchange and entries
  naming each other, the same SVID is accepted — and a full A2A call runs
  between an agent attested by one organisation's SPIRE and a caller attested by
  another's. Both halves are needed: acceptance alone would be
  indistinguishable from a verifier that ignores trust domains, and rejection
  alone from federation simply being broken.

  Federation surfaced an ordering rule worth knowing: a registration entry
  naming `-federatesWith` is rejected outright unless that trust domain's bundle
  is *already* imported. Bundles must be exchanged before entries are created,
  which is why the testbed now splits `start_with` from `register` rather than
  doing both in one call — the ordering is visible at the call site instead of
  being a comment someone can miss.

  `spiffe_rotation.rs` issues 40-second JWT-SVIDs so a rotation happens inside a
  test rather than half an hour later. Three properties: the manager serves a
  renewed credential without being asked; the superseded credential stops
  verifying; and a live A2A agent keeps answering across the rotation —
  including a stream opened afterwards — without being restarted or handed a new
  manager. Each test *proves the rotation happened* before asserting anything
  about it, because a test that merely waited and then succeeded would pass just
  as happily if nothing had rotated at all.

- **`slim-node` — a standalone SLIM node binary.** Routes and runs nothing
  itself. Prints `listening on <addr>` once the socket accepts, so a supervisor
  waits for readiness instead of sleeping; refuses half a TLS configuration
  rather than silently serving plaintext. It exists so the out-of-process claim
  is testable, and because bringing up a node otherwise means installing the
  full AGNTCY SLIM distribution.

- **`benches/send_latency_breakdown.rs`** — attributes the cost of a blocking
  send across three axes (runtime worker count, executor event count, and a
  round trip that runs no executor at all), so a future regression in this
  class is attributable by measurement rather than by inspection.
- **`benches/examples/send_probe.rs`** — a diagnostic that walks the task
  store across its capacity boundary and reports send latency per 1,000-send
  bucket. This is what turned "sends are slow" into "sends are slow past
  exactly 10,000 tasks".

### Changed

- Public traits are documented as **unsealed and staying that way**, with the
  rules for extending them in CONTRIBUTING — including why a defaulted method is
  not free.

- `handler/messaging.rs` (2,395 lines) split into `messaging/{mod,decisions,
  tests}.rs`. `send_message_inner` remains ~530 lines and is recorded in
  `.file-length-baseline` as the outstanding work.



- **Expired tasks are now reclaimed only by the TTL sweep's own interval.**
  Previously an over-capacity write also ran the TTL pass as a side effect, so
  a full store expired terminal tasks on every write. It now runs every
  `eviction_interval` writes (default 64), as its configuration always
  described. `max_capacity` remains a hard cap enforced on every write, so
  memory is bounded exactly as before; only the promptness of TTL reclamation
  changes, and only within the interval that already governed it.

### Fixed

- **Streaming was quadratic in the length of the stream.** The background
  processor saved the whole task on every artifact event, and the in-memory
  store deep-clones, so event *i* copied *i* artifacts. A 502-event stream spent
  43.4 ms against the in-memory store versus 3.2 ms against one that discards
  everything. `save_artifact_delta` makes it linear: **120.6 ms to 3.6 ms** for
  the distinct-artifact shape, **43.4 ms to 2.5 ms** for the append shape, with
  both curves flat.

  The same change on the SQL stores removes the Rust-side serialization of the
  whole task and its transfer as a bind parameter: SQLite 144.5 ms to 127.6 ms,
  Postgres 798 ms to 500 ms with per-event cost flat (853/840/1000 µs at 50/250/
  500 chunks) where `save` grew (874/1183/1597 µs).

- The `Message::text()` helper and the `PartContent` re-exports the README quick
  start needed. That snippet had not compiled for three minor versions.


- **`message/send` cost 32× more on any server that had handled more than
  10,000 tasks.** The default `InMemoryTaskStore` caps itself at
  `max_capacity` (10,000). Once full it is over that cap on *every* subsequent
  write, and every such write ran a sweep that cloned every `TaskId` in the
  store into a `Vec`, sorted it, and then removed — normally — a single task.
  A blocking send performs several saves, so one request paid that O(n log n)
  sweep several times over.

  It is a cliff, not a slope, and it never recovers: the store stays at
  capacity for the life of the process. Measured with
  `cargo run --release -p a2a-benchmarks --example send_probe`, which walks the
  store across the boundary and reports the median per 1,000 sends:

  | Sends so far | Before | After |
  |---|---|---|
  | 10,000 (at the cap) | 65.2 µs | 62.9 µs |
  | 11,000 (past it) | **2.4 ms** | **67.3 µs** |
  | 16,000 | **2.0 ms** | **64.3 µs** |

  Two changes fix it. Capacity eviction now walks `order_index` — already
  sorted oldest-first — and stops as soon as it has enough victims, instead of
  collecting and sorting the whole store; the search for a terminal task to
  prefer is bounded by a scan window, so the sweep is O(1) in the store size
  rather than O(n). And the O(n) TTL pass no longer rides along with it: the
  two passes are now separately triggered, so the amortized sweep stays
  amortized instead of running on every write once the store is full.

  End to end over loopback JSON-RPC, `transport/jsonrpc/send/single_message`
  improves **91%** (2.18 ms → 191 µs, p < 0.05). The control holds: the same
  run measures `get_task` on a missing id — one round trip that performs no
  write — unchanged at −0.25% (p = 0.76), confirming the gain is specific to
  the write path and not machine-wide noise.

  This was previously attributed, in a benchmark comment, to cross-thread task
  scheduling on 4-core CI runners. That hypothesis is disproved here: running
  the same send on a single-worker runtime does not close the gap. Scheduling
  is real but second-order — worth ~50 µs of the remaining 191 µs, and it was
  entirely masked by the eviction cost.

  The new sweep's fallback path — topping up with in-flight tasks when the scan
  window holds too little finished work — was correct but untested at its one
  interesting point. Mutation testing showed it: the first draft sized the
  top-up as `overflow - terminal.len()`, and changing that `-` to a `+`, which
  makes the sweep evict *more* than the overflow, broke no assertion. Every
  capacity test either filled the quota from terminal tasks alone (returning
  before the top-up ran) or found none at all (where the two operators agree),
  so the partial-supply case in between went unexercised. It now has a test,
  and the arithmetic it turned on is gone: the top-up is a second bounded pass
  over the same window, sharing the first pass's exit condition, which also
  drops a `Vec` of candidate ids that were usually collected and discarded.


## [0.8.0] - 2026-08-13

> [!WARNING]
> **If you serve gRPC to 0.6 clients, upgrade order matters — get it wrong and
> it is an outage, not a degradation.**
>
> This release deletes the JSON tunnel that let a 0.6 gRPC client talk to a
> 0.7 server. The service those clients dial, `a2a.v1.A2aService`, is no longer
> registered on the listener, so their calls fail at the gRPC layer — they do
> not fall back to anything.
>
> **Move gRPC clients onto the canonical `lf.a2a.v1.A2AService` first, then
> upgrade servers.** The tunnel existed precisely to keep a mixed fleet working
> during that migration, and 0.8 closes the window.
>
> Unaffected: JSON-RPC, REST and WebSocket deployments, and any gRPC deployment
> already on the canonical binding — the default since 0.7, and what the
> official Go, Python and Java SDKs speak.

**A breaking release, and the deletions are the headline.** Three deprecations
announced in 0.7 come out here, on the schedule their own deprecation notes
named. Nothing is deprecated-but-kept a second time: an item that says "removed
in 0.8" is removed in 0.8.

All four crates are now at **0.8.0**. Until this release
`a2a-protocol-server` alone sat at 0.8.0 while the other three stayed at 0.7.0
— deliberately, because `cargo-semver-checks` had flagged breaking changes in
that crate and only that crate, and bumping just the crate that broke keeps the
check meaningful. That split was always documented as something the next real
release had to reconcile, and this is that release.

### Removed

- **The `grpc-legacy-json` feature and the pre-0.7 JSON-tunnel gRPC service.**
  Releases before 0.7 tunneled JSON inside a protobuf `bytes` envelope on a
  non-standard service (`a2a.v1.A2aService`), served alongside the canonical
  binding behind an off-by-default feature so 0.6 clients survived a rolling
  upgrade. That window has closed. Gone with it: the feature on both
  `a2a-protocol-server` and `a2a-protocol-sdk`, `dispatch/grpc/service.rs`,
  `GrpcDispatcher::into_legacy_service`, the `LegacyA2aServiceServer` /
  `LegacyGrpcServiceImpl` re-exports, the JSON codec helpers
  (`encode_json`/`decode_json`/`reader_to_grpc_stream`), the `a2a.v1` proto and
  its build-script compile step, and the coexistence test.

  **Migration:** none is available in-process — a 0.6 client must move to the
  canonical `lf.a2a.v1.A2AService`, which is the protobuf-native binding the
  official Go, Python and Java SDKs already speak. `GrpcDispatcher::serve` and
  `into_service` are unchanged for everyone already on it.

- **`RequestHandlerBuilder::with_event_queue_write_timeout` and
  `EventQueueManager::with_write_timeout`**, both deprecated no-ops since 0.7.
  Event-queue writes never block — the queue is a broadcast channel, so a slow
  streaming consumer receives an explicit lag error on its reader rather than
  exerting backpressure on the executor — so neither setter ever had an effect.

  **Migration:** delete the call. There is no replacement because there was no
  behaviour. To bound a slow consumer, size the queue with
  `with_event_queue_capacity` and handle the lag error on the reader.

  Said plainly rather than left to be discovered: the two *public setters* are
  gone, but the value they set is still threaded through
  `new_in_memory_queue_with_options` into an `#[allow(dead_code)]` field on
  `InMemoryQueueWriter`, and `DEFAULT_WRITE_TIMEOUT` is still exported. Those
  were not on the announced removal list, and changing that constructor's arity
  would be an unadvertised API break on top of an advertised one. The dead
  plumbing is tracked in `ROADMAP.md` for a later release.

- **The bare `a2a-notification-token` header from the default CORS
  `allow_headers`.** `x-a2a-notification-token` is canonical and stays; the
  unprefixed spelling was this SDK's own pre-0.7 name.

  **Migration:** a webhook receiver still reading the bare header can restore
  it by assigning to `CorsConfig::allow_headers` — it is a plain `String` and
  always was, so this changes a default, not a capability.

### Added

- **Act 5 grows to sixteen checks: every SDK capability that had no example now
  has one.** The previous entry left eight capabilities demonstrated by no
  example — measured by grep, not recalled — each for a stated reason. All eight
  are now covered, and each was proven able to fail by removing what it covers:

  | Capability | Demonstrated by | Proven failure |
  |---|---|---|
  | `ApiKeyAuthInterceptor` | Custom header, three cases (absent / wrong / right key) | `a request with no key SUCCEEDED` |
  | `JwtAuthInterceptor` via remote JWKS | ES256 tokens against a JWKS the example serves on a loopback socket | `a token signed by an unpublished key was ACCEPTED` |
  | `HandlerLimits` | `max_id_length` against a caller-controlled `context_id` | `a 33-char context_id was accepted with max_id_length 32` |
  | `RetryPolicy` (client) | Fault-injecting reverse proxy in front of a real agent | `2 injected 503s were not ridden out` |
  | `TenantAwareSqliteTaskStore` | Two tenants written, handler replaced, both read back | `after the restart acme can see globex's task` |
  | `PostgresTaskStore` | Round-trip through two handlers over one database | `task … did not survive the handler change` |
  | `init_otlp_pipeline` | A collector socket the example owns, asserting HTTP/2 bytes arrive | `3 requests recorded but the collector received 0 bytes` |
  | `HttpPushSender::with_tls_config` | `rcgen` cert + `tokio-rustls` sink, delivered with both trust stores | `delivery to a certificate the sender was told to trust failed` |

  The retry check is the one worth reading: it asserts a `503` on
  `SendMessage` *is* retried and a `502` is *not*. `503` means the request was
  refused up front, so re-sending a non-idempotent method is safe; `502` means
  a gateway may already have forwarded it, so retrying can create a second
  task. A retry layer that treats every 5xx alike passes both other assertions
  and fails only that one.

  New reporting state: `[NOT RUN]`, distinct from `[NOT BUILT]`. "The binary
  cannot do this" and "the binary can and nobody tried" are different facts.
  Only the PostgreSQL check can be `[NOT RUN]` (it names
  `A2A_TEST_POSTGRES_URL`); CI provides the service, and
  `INCIDENT_REQUIRE_ALL=1` makes either state exit `4` so a service that stops
  being provisioned fails the job instead of silently downgrading a check to a
  printed line.

  Two failures reported while writing these were the checks' own fault, not the
  SDK's, and are recorded because the distinction is the point: a JWT that
  expired 30 seconds ago is *correctly* accepted inside `JwtValidator`'s
  60-second clock-skew leeway, and a proxy that forwards a request without its
  headers strips `A2A-Version` and earns a `-32009`.

- **`examples/incident-response` now demonstrates the SDK capabilities a
  deployment needs, over a socket, with assertions.** Tenant isolation,
  authentication interceptors, rate limiting, persistent stores, agent-card
  signing, the `Metrics` hook, OpenTelemetry export and graceful shutdown all
  shipped and were covered by unit and integration tests, but no example
  exercised any of them end-to-end. Act 5 runs eight checks, each naming the
  specific wrong answer it rules out:

  | Check | Fails when |
  |---|---|
  | Tenant isolation (`TenantAwareInMemoryTaskStore` + `HeaderTenantResolver`) | A tenant sees another's task, or a caller authenticated as one tenant writes into another by naming it in `params.tenant` |
  | `BearerTokenAuthInterceptor` | An anonymous request succeeds, **or** a correctly authenticated one is refused |
  | `RateLimitInterceptor` | Every call is accepted, every call is refused, or more than the limit gets through |
  | `sign_agent_card`/`verify_agent_card` | A card whose interface URL was rewritten still verifies |
  | `SqliteTaskStore` | A task written through one handler is not readable through another over the same file |
  | `RequestHandler::shutdown` | It does not return within 5s |
  | The `Metrics` hook | Served requests reach no recorder |
  | `OtelMetrics` | No `a2a.server.requests` datapoint is collected after N requests |

  Runnable on its own as `cargo run -p incident-response -- harden` (exit code
  `3` on a failure), gated by its own step in `ci.yml`'s `example-surface` job,
  and proven able to fail by `scripts/prove_gates_fail.sh`, which removes the
  tenant resolver — a defect under which every request still succeeds and only
  the isolation check notices.

  Two details were verified rather than assumed. The OTel check collects from a
  real `ManualReader` rather than trusting the global provider, which defaults
  to a no-op under which a handler that records nothing looks identical to one
  that records everything. And the signing check tampers with
  `supported_interfaces[0].url`, not the deprecated top-level `AgentCard::url`
  — the latter is `#[serde(skip_serializing)]` because A2A v1.0 removed it, so
  it is absent from the canonical signing payload and rewriting it correctly
  changes nothing.

  Capabilities behind Cargo features print `[NOT BUILT]` with the feature they
  need instead of vanishing, so `--no-default-features` reports a narrower run
  as narrower (measured: 5 passed, 3 not compiled) rather than printing the
  same "all passed" line over fewer checks. `incident-response` gains
  `default = ["sqlite", "signing", "otel"]` so the documented command exercises
  everything.

- **All six examples now drive every A2A method over every binding they serve,
  and fail if the matrix has a gap.** Measured 2026-08-11, all six report
  **44 of 44 cells**. Before the change: `echo-agent` 4 methods / 2 bindings,
  `incident-response` 4 / 1, `genai-agent` 0 / 1 (it printed a URL and waited),
  `rig-agent` 0 / 1, `multi-lang-team` 1 / 1, and `agent-team`'s 100 feature
  tests had no rows for the question at all.

  `agent-team` gains a fifth agent that exists purely to be swept, because its
  four team agents are deliberately split by binding and no single one of them
  can answer the coverage question.

  Three examples depend on something CI does not have — an LLM provider, or
  worker agents in four other languages — and each reports that separately
  rather than letting a full matrix imply it: `genai`/`rig` print
  `LLM leg: NOT EXERCISED` and label every fallback answer
  `[no model reachable — mechanical fallback, not an LLM answer]`;
  `multi-lang-team` prints each worker as `REACHABLE` or `not reachable` and
  its artifact says `[no worker agents reachable — nothing was delegated]`.
  The fallbacks are opt-in per handler: server mode still fails the task on a
  provider error, which is correct for a real agent.

  `multi-lang-team`'s coordinator now probes workers once at startup instead of
  fanning out per request. With all four down every call paid a full timeout
  window, which made the sweep take minutes and told the reader nothing they
  had not been told at startup.

- **Both examples now drive every A2A method over every binding they serve, and
  fail if the matrix has a gap.** Measured 2026-08-11 before the change:
  `echo-agent` drove 4 of the 11 methods over 2 of the 4 transports, and
  `incident-response` 4 over 1 — while `examples/README.md` presented the first
  as demonstrating "the complete request lifecycle". `echo-agent`'s card also
  advertised neither push notifications nor an extended card, so seven methods
  were not merely undriven but unavailable on the server it started.

  Both now report 44 of 44 cells and exit 2 on any gap. `echo-agent` serves
  gRPC for the first time; `incident-response` serves REST, gRPC and WebSocket
  for the first time, on `port`, `port + 10` and `port + 20`. The narrative
  Acts 1-3 are unchanged — the sweep is a fourth act, so the example still
  teaches what an agent is before it measures what the SDK covers.

  Both also run counter-tests against a second agent advertising no optional
  capabilities, because a full matrix only shows the server says yes to
  everything it should. Five refusals are checked by error code: unknown task
  ids on `GetTask` and `CancelTask`, push config and extended card against a
  card lacking those capabilities, and streaming against an agent that never
  advertised it.

  Gated by `ci.yml`'s `example-surface` job, registered in `preflight.sh` and
  `prove_gates_fail.sh`. Proven able to fail: dropping the `ListTasks`
  recording from the shared harness, with every call still succeeding, exits 2
  and names all four cells.

- **`a2a-example-harness`** — the coverage matrix, the method sweep and the
  counter-tests, shared by both examples rather than copied into each. A
  duplicated scorer is one that eventually disagrees with itself: one copy
  loses a row and the example built on it reports a full matrix.

- **The SDK dogfood suite now runs in CI, and its feature table is computed
  from results.** `examples/agent-team` holds ~5,900 lines of end-to-end tests
  and had never been executed by any workflow: it is a `main()`, not
  `#[test]`s, so `cargo test --workspace` compiled it and ran none of it, and
  it appeared in the workflows only inside `cargo package --exclude` lists.
  Measured when first run locally on 2026-08-11: **86 tests, 71 passed, 15
  failed, exit 1**.

  Three independent defects kept that invisible, and all three are fixed:

  * **The feature table could not fail.** "SDK FEATURES EXERCISED" was a
    hardcoded array printed as `[x] <label>` in a loop, with no connection to
    any result. The failing run still printed `[x] Batch JSON-RPC (single,
    multi, empty, mixed, streaming rejection)` with all six batch tests red.
    Feature-gated labels were `#[cfg]`'d out of the *array*, so a build
    without `--features websocket` omitted the row entirely rather than
    reporting it unexercised — absence rendered as completeness. The table now
    lives in `examples/agent-team/src/features.rs`, where each claim names the
    tests that evidence it and renders `[x]`, `[FAIL]` or `[ ] NOT RUN` from
    their outcomes. A bidirectional drift check fails the run if a claim names
    no test that ran, names a test that does not exist, or if a test ran that
    no claim mentions.
  * **No `default` features.** `cargo run -p agent-team` — the command
    `examples/README.md` gave for "exercises every SDK feature" — compiled out
    WebSocket, gRPC, Axum, SQLite, signing and OTel, then printed "SDK dogfood
    complete". There is now a `default` set covering all six, and the binary
    exits 2 listing every unexercised area rather than claiming completion.
  * **Nothing ran it.** `ci.yml` gains a `dogfood` job running the suite with
    `--all-features`, registered in `scripts/preflight.sh` and
    `scripts/prove_gates_fail.sh` so the existing bidirectional job-coverage
    guard accounts for it.

  With all features enabled the suite is now **100 tests, 100 passing**.

- **`A2aError::is_stream_lagged`, `dropped_event_count`, `stream_lagged`, and
  `STREAM_LAGGED_MARKER`** (`a2a-protocol-types`), plus
  `ClientError::is_stream_lagged` / `dropped_event_count`
  (`a2a-protocol-client`).

  A server emits a recoverable lag signal when a streaming consumer falls
  behind; the stream continues and a consumer that keeps polling still
  receives later events. Recognising it required matching the raw
  `data.streamLagged` JSON key by hand, because the only predicate was
  `pub(crate)` inside `a2a-protocol-server`. Every out-of-tree consumer faced
  that, and the dogfood suite's own backpressure test got it wrong — it broke
  out of the read loop on the lag signal, then asserted on an event it had
  just stopped waiting for. The marker and message now have one definition in
  the types crate, which the server's `lag_error`/`is_lag_error` delegate to.

- **`extended-card-requires-auth`** (dogfood Test 68b) — asserts that an agent
  advertising `extendedAgentCard` with no authenticating interceptor refuses
  to serve it (spec §13.3). This path is bypassed on the shared analyzer agent
  by `allow_unauthenticated_extended_card()`, so without a dedicated test the
  suite would prove the card is served and never that it is protected.

- **`BIND-EQUIV-004` is now graded behaviourally, not just structurally.**
  §5.1 requires every binding to support the same authentication schemes. The
  runner has been checking the half a card can answer — v1.0 gives
  `AgentInterface` no security fields, so schemes declared once at card level
  bind every binding. Whether each binding *enforces* them was recorded as
  unmeasured, because no target here required credentials to withhold.

  `tck/sut` gains `SUT_PROFILE=secured`, whose card declares a bearer scheme
  and whose handler enforces it with a single `BearerTokenAuthInterceptor`.
  One interceptor above the dispatchers is the property under test: all four
  bindings are guarded by one implementation reading one `CallContext`, so a
  binding that stopped forwarding the header would fail rather than quietly
  serve anonymous traffic. `a2a-tck --equivalence --auth-token <t>` sweeps
  twice — every binding must refuse an uncredentialed request, and every
  binding must serve a credentialed one — and `tck.yml` gates both.

  The acceptance sweep caught a defect in the probe itself during development:
  an early draft sent the JSON-RPC method as `tasks/list` where this SDK's name
  is `ListTasks`, which authenticated and then failed dispatch, so two bindings
  reported a refusal and the check declared an asymmetry that did not exist. On
  the rejection sweep alone, a probe that can never succeed is
  indistinguishable from enforcement working. Recorded in
  `book/src/reference/conformance-history.md` rather than quietly fixed.

- **Four governance files**: `MAINTAINERS.md`, `.github/CODEOWNERS`,
  `SUPPORT.md` and `TRADEMARKS.md`. `GOVERNANCE.md`'s duplicated maintainer
  table is removed in favour of `MAINTAINERS.md`, so the two cannot drift.
  `CODEOWNERS` states in its own header that with one maintainer it is inert
  for that maintainer's own pull requests — real for outside contributions,
  and not a claim that these paths are independently reviewed today.

- **`docs/provenance-manifest.md` and `scripts/provenance_manifest.sh`** — the
  reproducible account of what this repository's history contains and what a
  DCO history rewrite would cost, for a downstream project's counsel. The
  script refuses to run on a shallow clone, which is what produced two
  successive wrong measurements.

### Fixed

- **The per-PR mutation gate could not see 41 of 141 library source files.**
  `mutants.yml`'s `Build PR source diff` step scoped with
  `git diff -M … -- 'crates/*/src/**/*.rs'`. A git pathspec is matched by
  `fnmatch` *without* `FNM_PATHNAME` unless it carries `:(glob)` magic, so `*`
  crosses `/` and the `**/` collapses to "one or more directories" — the
  literal slash after it still has to match something. The pattern therefore
  reached only files in a *subdirectory* of `src/`, and never a file sitting
  directly in `crates/<crate>/src/`. Measured with `git ls-files` at `6ebf821`:
  100 files matched, against **141** for `:(glob)crates/*/src/**/*.rs`.

  The 41 invisible files include `rate_limit.rs`, `serve.rs`, `builder.rs`,
  `executor.rs`, `method.rs`, `signing.rs`, `client.rs`, `retry.rs` and all
  four `lib.rs`. When the diff came back empty the job set `skip=true`, its own
  `if:` skipped the mutation step, the check went green, and the summary
  printed "No Rust source files changed in `crates/*/src/` — nothing to
  mutate", which was untrue.

  Proven on real history rather than a synthetic defect: nine of the last 120
  commits changed `crates/*/src` sources *only* in the invisible set. Replaying
  the step gives `ADDED` = 0 for `a9e1235` (a `rate_limit.rs` fix), `a116bc5`
  (another) and `e6aa9e1` (a feature commit) under the old pathspec, and 57,
  24 and **376** under the fixed one.

  The weekly full sweep is unaffected, and that was checked rather than
  assumed: it never passes `--in-diff`, and its `examine_globs` are consumed by
  cargo-mutants' glob crate where `**/` does mean "zero or more directories" —
  demonstrated by run `31352927429` finding 6 survivors in `rate_limit.rs`, a
  top-level file. So the headline mutation score is unchanged; what was
  narrower than it looked is every green *incremental* check on a PR touching
  only top-level modules.

- **The WebSocket client forwarded its end-of-stream control frame to the
  consumer.** The binding closes a stream with
  `{"result":{"status":"stream_complete"}}`; the reader task wrapped *every*
  frame as an SSE line and delivered it, consulting `is_stream_terminal` only
  afterwards for pending-map cleanup. The sentinel is not a `StreamResponse`,
  so it surfaced to callers as
  `unknown variant 'status', expected one of 'task', 'message', 'statusUpdate', ...`.

  The common case hides it: when a task reaches a terminal state the stream
  ends on that event and the sentinel is never parsed. It only bites when a
  stream ends *without* a terminal state — most obviously a task parked in
  `INPUT_REQUIRED`, i.e. any agent that asks a clarifying question over
  WebSocket. Found by driving all eleven methods against exactly such an agent
  while expanding `incident-response`; `echo-agent`, whose tasks always
  complete, never hit it.

  Fixed with a narrow `is_stream_complete_sentinel` consulted *before*
  delivery. Deliberately narrower than `is_stream_terminal`, which also treats
  a terminal task status as end-of-stream: suppressing those would truncate
  every stream at its most important frame, a worse bug than the one being
  fixed. Three regression tests pin the split, including one asserting the
  sentinel genuinely cannot deserialize as a `StreamResponse`.

- **The 15 failing dogfood tests**, all in the suite rather than the SDK, with
  one exception noted below. Root causes, each verified against a live server
  rather than inferred:

  * **9 tests** — `post_raw`/`get_raw` sent no `A2A-Version` header. The server
    negotiates the version per request and defaults to `0.3`, so every raw-HTTP
    test was asserting against a `VERSION_NOT_SUPPORTED` envelope instead of a
    real response. Both helpers now send it, sourced from
    `A2A_VERSION_HEADER`/`A2A_VERSION` so they cannot drift, and both return
    `Err` on a version rejection so the failure names its own cause.
  * **3 tests** (`resubscribe-rest`, `resubscribe-jsonrpc`, `push-delivery-e2e`)
    — each read one event and pattern-matched `StatusUpdate` to get a task id.
    The first stream event is a full `Task` snapshot; the observed order is
    `task` → `artifactUpdate` → `statusUpdate`. All three had never passed.
    Replaced with a shared `helpers::first_task_id`, which reads until an event
    names a task — the same correction already applied to `BIND-EQUIV-003`.
  * **`extended-agent-card`** — the only one that was a genuine configuration
    gap rather than rot: the analyzer's card never set
    `extended_agent_card`, so the handler correctly answered
    `UnsupportedOperation` per spec §3.1.11 and the test could not have passed
    on any commit. The capability is now advertised and the §13.3
    authentication requirement satisfied explicitly.
  * **`backpressure-lagged`** — asserted that a lagged consumer still receives
    the terminal event. It does not, by design: the SDK's own lag message says
    to resubscribe, and events are dropped for that consumer only while the
    store stays authoritative. The test now asserts that invariant — that
    backpressure costs events, never the task — by confirming via `GetTask`
    that the task still reached `Completed`.

- **A vacuous pass.** `wire-part-flat-oneof` asserts the *absence* of a
  `"type":"text"` discriminator, and passed against the version-error envelope,
  which also lacks it. Verified by injection: with the header removed it now
  reports `[FAIL] … request failed: server rejected protocol version …`
  instead of `[PASS] response uses v1.0 flat Part…`.

- **WebSocket dogfood tests aborted the process.** Tests 51-60 are
  `#[cfg(feature = "websocket")]`, and with no `default` features and no CI job
  they had never been compiled by an automated run. Enabling them showed the
  handshake omitted `A2A-Version` (spec §3.6.1 — for a WebSocket that is the
  upgrade request), and the test `.expect()`ed on connect, so one broken test
  killed the other 86. The handshake now carries the header, mirroring
  `a2a-protocol-client`'s own WebSocket transport, and the panic sites are
  `TestResult::fail`.

- **`push-delivery-e2e` asserted nothing about push delivery.** It passed on
  `task_id.is_some()`, which every other streaming test already proves, and
  would have stayed green with push entirely broken. It now requires the
  webhook to have actually received something — currently 6 events.

- **`examples/agent-team/src/tests/mod.rs` omitted the `transport` module**
  from its own module list, so the numbering jumped 41-50 → 61-90 and the only
  evidence that Tests 51-60 existed was the gap. `examples/README.md`'s
  "exercises every SDK feature with 81+ automated tests" and echo-agent's
  "complete request lifecycle" are corrected to what is measured.

- **`PROVENANCE.md` §2.1's commit counts were wrong**, measured on a shallow
  clone. Over the full 648 non-merge commits the DCO verdict is 126 pass / 477
  assistant-authored / 29 bot / 16 unsigned human — not 120 / 138 / 21 / 3, and
  the pass rate is 19%, not 43%. §2.1 also said §1's figure of 608 commits
  "could not be reproduced"; on a complete clone it reproduces exactly, and §1
  needed no change. The failure mode is worth knowing: a shallow clone drops
  the oldest commits, which here are precisely the non-compliant ones, so the
  pass count survives intact while every failure count shrinks. It reads as
  good news.

- **The 500-line file-length ratchet now covers `.sh` and `.py`**, not `.rs`
  alone. Widening turned up two over-limit scripts nobody had counted, which is
  the argument for a ratchet over a one-time split. Baseline 77 → 81 entries.

- **ADR 0006 no longer says developers must address all surviving mutants
  before merge.** The gate is `--in-diff`; the obligation is the lines a PR
  changes. `CONTRIBUTING.md` was corrected earlier and the ADR was not, leaving
  a governing document contradicting the enforced rule. Its stale 92% / 183
  figure is now the measured 94% / 125.

- **The verdict-bearing steps outside `ci.yml` are now proven able to fail.**
  `scripts/prove_gates_fail.sh` covers `ci.yml`'s eight gate jobs by injecting
  defects into tracked source and running cargo; that mechanism does not reach
  the other ten workflows, whose gates decide over *data* — a conformance
  report, a directory of mutation artifacts, a git range, a tag name.

  `scripts/prove_workflow_gates_fail.py` runs each step's real body from the
  real workflow file against synthetic healthy and defective inputs: 17 gates
  proven, 11 exempt with recorded reasons, 0 unproven, ~7s. It runs on every
  PR in the `fmt` job, and drift is a hard error both ways — a step that can
  fail with no registry entry, and a registry entry naming a step that no
  longer exists, both exit 2.

  It reproduces GitHub's shell mapping exactly (`bash -e` without a `shell:`
  key, `bash -eo pipefail` with `shell: bash`), because those two disagree
  about the defect it was written for, and it asserts each gate exits 0 on
  healthy input as well — a gate that fails unconditionally would otherwise
  score as proven while blocking every legitimate run.

- **The TCK drives all four bindings, and grades §5.1 equivalence.** It ran
  JSON-RPC and HTTP+JSON only; `--binding websocket` (§12) and
  `--binding grpc` (§10) complete the set, and `--equivalence` grades
  `BIND-EQUIV-001..004` — the four §5.1 `MUST`s that are statements about the
  *relation* between bindings and so cannot be graded one binding at a time.
  Upstream has carried them as `NOT TESTED` since April.

  The gRPC checks are separate assertions rather than a ProtoJSON adapter over
  the JSON ones. Converting protobuf responses and reusing the JSON assertions
  would be less code and would produce checks that cannot fail:
  `task_state_values` asserts the state is one of nine `TASK_STATE_*` strings,
  and over gRPC that string exists only after this crate's own enum-to-name
  mapping runs, so the assertion would be about the converter. `tck/sut` now
  serves all four bindings (`SUT_GRPC_HOST`, `SUT_WS_HOST`).

  `BIND-EQUIV-004` is graded structurally only, and says so in its output:
  the card declares security once at card level and no interface may override
  it, but proving each binding *enforces* those schemes identically needs a
  target configured to require credentials.

- **`SignatureHeader` and `signature_header()` in `a2a-protocol-types`** —
  decodes an `AgentCardSignature`'s JWS protected header, exposing `alg`,
  `kid` and `jku` so a caller can determine *which* key to retrieve before
  verifying. It performs no cryptographic verification, and the header is
  attacker-controlled until `verify_agent_card` succeeds against a trusted
  key — the docs say so at the call site. Key expiry and revocation
  (`CARD-SIGN-004`) remain the caller's responsibility, since neither is
  carried in the header; this provides the `jku`/`kid` needed to go and
  check them.

- **Unrecognised request parameters are now logged instead of vanishing.** The
  specification requires implementations to *ignore* unknown fields for
  forward compatibility (§11; the official TCK grades this as
  `DM-SERIAL-005`), so `ListTasks` with `{"contxtId": …}` must keep returning
  every task rather than erroring. It no longer does so invisibly: the
  JSON-RPC binding diffs each request's top-level keys against the ones the
  method understands and warns about the difference, naming them. Honouring
  §11 on the wire and telling the operator what was discarded are not in
  conflict.

  The accepted-key lists live on a new `AcceptedFields` trait and are verified
  against `a2a.proto` by `proto_field_alias.rs`, so a list that drifts short
  (spurious warnings) or long (silenced real ones) fails the build.

- **The TCK SUT takes a `SUT_PROFILE`.** `CORE-CAP-001/002/004` check that a
  server rejects push and streaming operations it never advertised, and the
  suite skips them against an agent that does advertise them — so those paths
  were unreachable from a single SUT, and the gate could never have caught a
  regression in `ensure_streaming_supported` / `ensure_push_supported`.
  `SUT_PROFILE=minimal` advertises nothing; CI runs the suite once per profile
  and gates both. `CORE-CAP-001` and `CORE-CAP-002` now pass rather than skip.

- **The TCK SUT serves gRPC.** Its agent card advertised only `JSONRPC` and
  `HTTP+JSON`, and the TCK builds one client per advertised interface — so the
  whole `GRPC-*` family and every core requirement's gRPC leg reported
  `SKIPPED` while this SDK shipped a gRPC binding. The suite went from
  `176 passed / 89 skipped` to **`244 passed / 21 skipped`, still 0 failed**,
  and MUST-level `SKIPPED` from 11 to 5. The binding passed everything on
  first exposure, so this closed no defects — it closed a hole in what the
  score was measuring. `SUT_GRPC_HOST` overrides the port.

### Changed

- **`AgentCard.url` is no longer emitted.** It is the v0.3 top-level URL; the
  v1.0 `AgentCard` has no `url` field, `supportedInterfaces` replaced it, and
  emitting it made this SDK's card fail the specification's own JSON schema
  (`'url' does not match any of the regexes: …`) — reported by `CARD-EXT-001`
  on both JSON bindings while gRPC passed, since protobuf cannot carry a field
  the schema does not define.

  The field is still **parsed**, so a card published by a v0.3 peer still
  loads; the reference implementation does the same, folding `url` into
  `supportedInterfaces`. To publish an agent's address, use
  `supported_interfaces`. Same policy as the `securitySchemes` change below:
  accept both vintages, emit only v1.0.

### Fixed

- **The official-TCK conformance gate could not fail.** The step
  `official-tck.yml` calls "THE GATE" ended `| tee /tmp/tck-gate.log`, and
  GitHub's default shell for a `run:` step with no `shell:` key is
  `bash -e {0}` — `-e` but not `-o pipefail`. The step's exit status was
  `tee`'s, which is 0 whatever the checker decided.

  Measured by running the step body verbatim: against an all-SKIPPED report
  the checker exits 1 and prints `UNDER-MEASURED — 0 MUST requirement(s)
  graded, floor is 88`; against a report carrying an injected MUST failure it
  exits 1 and prints `REGRESSION`. Both step bodies exited 0. With
  `set -o pipefail` both exit 1, a healthy report still exits 0, and the log
  the `tee` exists to write is still written.

  `--min-graded` was added so a run that measured nothing could not pass this
  gate; it could never fire. A guard behind a broken gate is not a guard. The
  minimal- and extension-profile suite steps carried the same masking, so
  their `|| suite_status=$?` never fired and their closing
  `exit "$suite_status"` was always 0 — requirement-level regressions were
  still caught there by the un-piped gate steps that follow, suite-level
  failures were not. Enabling pipefail does not turn these red: all three
  suites were run directly and exit 0.

- **A conformance check passed two bindings it never ran against.**
  `jsonrpc_envelope_format` returned `Ok(())` for `rest` and `websocket`
  rather than declaring itself inapplicable, so a `rest` run reported
  `22/22 passed` while 21 checks had actually run. Not-applicable is now a
  reported outcome, printed with its reason and excluded from the
  denominator, and a run that grades zero checks exits 2 — every check being
  skipped is a run that verified nothing, not a pass.

- **`scripts/preflight.sh` silently skipped any non-cargo gate.** It matched
  `run: cargo ` when collecting CI's gate steps, so the first gate invoking a
  script rather than cargo would have been dropped without a word. It now
  matches every `run:` step in the gate jobs and cross-checks both
  directions. 31 gates, up from 28.

- **The 500-line file limit was an unenforced checklist line.** CONTRIBUTING
  claimed 46 of 139 files exceeded it; the tree had 77 of 310.
  `scripts/check_file_lengths.sh` pins the current count so it cannot grow,
  and CI runs it.

- **`release.yml` accepted lightweight tags.** A release cut from one carries
  no tagger, date, or message, and cannot carry a signature. The workflow now
  requires an annotated tag and fails with the `git tag -a` invocation to fix
  it.

- **A `SubscribeToTask` stream ended when the executor exited, not when the
  task finished.** Spec §3.1.6: "The stream MUST terminate when the task
  reaches a terminal state." A task's event queue lives exactly as long as one
  executor invocation, so an agent that parked a task in `input_required`
  destroyed the queue at every turn boundary — closing the stream while the
  task was still running, having reported no terminal state at all.

  `InMemoryQueueReader` gained an optional reattach hook, consulted when its
  channel closes: terminal task → emit a `TaskStatusUpdateEvent` carrying that
  status and end; task gone → end; still running → wait for the next turn's
  queue and continue. The send path, executor lifecycle and queue manager are
  untouched.

  Two new `HandlerLimits` bound the wait: `subscribe_reattach_interval`
  (250 ms) and `subscribe_max_idle` (5 min), after which the stream ends and
  the client may resubscribe per §3.5.2.

  One in-repo test asserted the defect as a requirement and was rewritten.

  This closes the last baselined failure: the official TCK now reports
  **176 passed, 0 failed**, MUST compatibility **100%**, and
  `tck/conformance-baseline.json` is empty. That figure covers *tested*
  requirements only — 21 MUST requirements still report `NOT TESTED` because
  the SUT does not exercise them, and are a coverage gap rather than a pass.

- **A blocking `SendMessage` could never return a direct `Message`.** Spec
  §3.1.1 allows either a `Task` or a `Message`; `SendMessageResponse::Message`
  was never constructed anywhere in the server, so an agent that replied with
  a message got back a task stuck in `Submitted`. The response is now a
  `Message` when the executor produced a message and nothing task-shaped —
  narrower than the reference, which errors on mixed streams and would break
  agents that narrate progress. Closes `DM-MSG-001`.

### Changed

- **`AgentCard.securitySchemes` is now emitted in the v1.0 wire shape.**
  `SecurityScheme` is a protobuf `oneof` in `a2a.proto`, so its ProtoJSON
  encoding is a single-key object naming the arm
  (`{"apiKeySecurityScheme": {"location": "header", …}}`). This SDK emitted the
  v0.3 OpenAPI-style form instead (`{"type": "apiKey", "in": "header", …}`).

  A reference `a2a-sdk` client parsed that via its own legacy-compatibility
  shim, so this was not an interop break with the reference. But a peer feeding
  the card straight to `ParseDict(…, ignore_unknown_fields=True)` — the same
  option the reference resolver itself passes — got schemes with **empty
  contents**, and would conclude the agent supports no usable authentication.
  That is the silent-wrong-data failure mode again, pointed the other way
  across the wire. Verified by feeding this SDK's own card bytes to three
  reference parsers.

  **Both encodings are accepted** (the v1.0 form under either the `json_name`
  or the proto field-name spelling of the arm, and the v0.3 form), and a v0.3
  scheme normalises to the v1.0 form on re-emission. `ApiKeySecurityScheme`
  emits `location` — the proto field name — with `in` kept as an alias.

  This is a breaking change to a published wire format: the bytes on
  `/.well-known/agent-card.json` change. It only affects a consumer reading the
  card's raw JSON keys rather than parsing it with an A2A implementation, and
  deserialization is strictly more permissive than before. Five in-repo tests
  asserted the old encoding and passed confidently; they now assert the v1.0
  encoding plus v0.3 acceptance. See `docs/official-tck-findings.md` §7.

### Fixed

- **`SendMessageConfiguration.taskPushNotificationConfig` was parsed and
  dropped, so no webhook was ever registered or delivered.** The schema is
  explicit that this is how a client subscribes at send time — *"Task id should
  be empty when sending this configuration in a `SendMessage` request"* — and
  the reference implementation registers it before the executor starts. This
  SDK deserialised the field and never looked at it again:
  `ListTaskPushNotificationConfigs` came back empty and no notification fired.

  The config is now registered against the created task, with `taskId` filled
  in server-side, at the point the task is saved and before the executor is
  spawned, so the first status transition is already covered. Registration
  reuses the standalone `CreateTaskPushNotificationConfig` validation —
  capability check, task existence, SSRF screening, per-task and global quotas
  — rather than writing to the store directly, so the inline path cannot become
  an unguarded back door. Four counter-tests drive each of those guards to a
  failure through the inline path specifically.

  Against the official TCK this closes all six `PUSH-DELIVER-001/002/003` legs
  across both bindings, taking the run from 166/10 to **172/4** and MUST
  compatibility from 93.9% to **97.6%**. The baseline shrinks from 9 pairs
  across 5 requirements to 3 across 2.

  **Behaviour change:** a `SendMessage` carrying an inline push config against
  a server with no push support now fails with `PushNotSupported` instead of
  succeeding and ignoring the config. The reference skips silently here; this
  SDK does not, on the same reasoning as the alias fix below — a client that
  asked for notifications and will never receive any should be told.

- **Request parameters spelled with the protobuf field name were silently
  ignored, turning a filter into wrong data.** The A2A JSON model is generated
  from `a2a.proto`, and protobuf's canonical JSON mapping requires parsers to
  accept **both** the proto field name (`context_id`) and its `json_name`
  (`contextId`). This SDK accepted only the latter and ignored the former, so
  `ListTasks` with `{"context_id": "ctx-A"}` returned **every** task instead of
  the 3 in that context — no error, just the wrong answer. The official TCK
  sends the snake_case spelling for six fields; the reference implementation
  (`a2a-sdk` 1.1.2, whose types *are* protobuf messages) accepts both.

  All 73 multi-word fields across the 44 schema messages now accept both
  spellings and still emit only the `json_name`, per spec §5.5.

  The alias list is derived from `a2a.proto` rather than from the Rust field
  names — deriving it from the Rust names would prove only that the aliases
  match the Rust identifiers, and would go stale silently as the schema grows.
  `proto_field_alias.rs` parses the schema, computes camelCase with protobuf's
  own `ToJsonName` rather than serde's `rename_all`, and asserts per field that
  both spellings parse to the same value, that the sample used is
  distinguishable from the field being absent, that only the `json_name` is
  emitted, and that every schema field is covered or explicitly exempt. Five
  counter-tests drive each direction to a deliberate failure.

  Against the official TCK this closes 7 MUST-level (requirement, transport)
  pairs — `CORE-HIST-002`, `PUSH-CREATE-001/002`, `PUSH-DEL-001/002`,
  `PUSH-GET-001`, `PUSH-LIST-001` — taking the run from 158/12 to 166/10 and
  MUST compatibility from 85.4% to 93.9%. `tck/conformance-baseline.json`
  shrinks from 16 pairs across 12 requirements to 9 across 5.

  **Not fixed, and deliberately not fixable this way:** a parameter misspelled
  any *other* way (`contextID`, `contxtId`, `pagesize`) is still ignored.
  Rejecting unknown fields would close it, and was implemented and then
  reverted — the specification says implementations **SHOULD** ignore
  unrecognized fields for forward compatibility (§11), and the official TCK
  grades that as `DM-SERIAL-005`, which the change failed on both bindings.
  Pinned by `unrecognised_fields_are_ignored_not_rejected` so it is not
  re-attempted; see `docs/official-tck-findings.md` §3.3(b).

  Two deliberate divergences are recorded rather than papered over: a request
  carrying *both* spellings of one field is rejected here (`-32602 duplicate
  field`) where the reference takes the last key, and `AgentCard.securitySchemes`
  is still emitted in the v0.3 shape (§7 of the same document).

- **Mid-stream SSE errors escaped their JSON-RPC envelope, so no conformant
  client could read them.** Success frames on the JSON-RPC binding are wrapped
  in a `JsonRpcSuccessResponse` (§9.4.2), but error frames were emitted as a
  bare `A2aError` — a payload carrying neither `result` nor `error`. This
  SDK's own client reported

      Serialization("JSON-RPC 2.0 response carries neither `result` nor
      `error`; §5 requires exactly one")

  instead of the error the server was sending, which made the 0.7.0
  `streamLagged` truncation signal **unreadable over JSON-RPC**: a consumer
  that fell behind learned only that the frame was malformed, not that its
  view was truncated or that it should resubscribe. Error frames are now
  `JsonRpcErrorResponse` envelopes echoing the request id, with `error.data`
  preserved so the `streamLagged` marker survives. The REST binding keeps
  bare payloads per §11.7, asserted so the fix cannot leak across bindings.

  A second site had the same defect: the serialization-failure fallback
  emitted an ad-hoc `{"error":"serialization failed: …"}` string, which is
  neither a JSON-RPC error response nor an `A2aError`. Both sites now share
  one enveloping helper.

  Surfaced by `cargo bench -p a2a-benchmarks --bench backpressure`, which
  panicked at `stream_volume/502_events`. Confirmed pre-existing: the same
  panic reproduces on `b416c1a` (v0.7.0), so it is not a regression from the
  TCK work. Pinned by `streaming_error_envelope_tests.rs`, which forces a
  deterministic lag by overflowing a 2-slot ring before the reader is polled,
  rather than depending on benchmark timing.

### Changed

- **`backpressure` benchmark: the event queue is sized above the event
  count.** `stream_volume/502_events` ran against the default 256-slot
  broadcast ring, so the reader lagged and the stream ended in a
  `streamLagged` error — that configuration was measuring event loss as much
  as streaming cost. The historical "252→502 events: ~193µs/event" figure in
  the source comment was taken from such a run and has been removed as not
  comparable. Slow-consumer behaviour is still exercised deliberately in
  `bench_slow_consumer`.

### Fixed

- **The official-TCK workflow reported `Success` while 12 MUST-level checks
  failed.** Both suite steps carried `continue-on-error: true`, so the job
  went green with `Process completed with exit code 1` sitting in the
  annotations. That is the same class of defect this project criticises
  elsewhere — a published signal that does not reflect reality — and a green
  check nobody can trust is worse than a red one, because nobody reads a green
  check.

  Replaced with a differential gate. `tck/scripts/check_conformance.py`
  compares the suite's machine-readable `compatibility.json` against a
  checked-in baseline (`tck/conformance-baseline.json`) at
  (requirement, transport) granularity, and fails the job on **either**
  direction:

  - a MUST-level failure not in the baseline — a regression;
  - a baseline entry that now passes — a stale baseline.

  The second direction is what keeps the gate honest: a baseline allowed to
  rot is `continue-on-error` with extra steps. Transport granularity matters
  because `PUSH-CREATE-001` fails on `jsonrpc` and passes on `http_json`
  today, and a requirement-level baseline would miss it spreading. A missing
  or malformed report also fails, so "the suite never ran" cannot read as
  success.

  The gate was counter-tested against injected regressions, an injected fix,
  a failure spreading to a second transport, missing/malformed/wrong-shaped
  reports, and a SHOULD-level failure (which must not gate) — plus an
  end-to-end simulation of the job's two shell steps confirming the run
  step's `|| true` cannot mask the gate's exit code.

  CI now runs the full suite once rather than twice: the `--level must` run
  was verified to produce an identical set of gated failures.

- `docs/official-tck-findings.md` now states that **all 12 remaining failures
  are `MUST` level** (previously listed without their level, which understated
  them), records that reported MUST compatibility of 85.4% is computed over
  tested requirements only — 21 further MUST requirements report `NOT TESTED`
  — and documents the baseline and gate.

### Fixed (found by the official A2A TCK)

Adopted the A2A project's own conformance suite
([`a2aproject/a2a-tck`](https://github.com/a2aproject/a2a-tck)) alongside the
in-repo one, with a new `tck/sut` System Under Test implementing its
`messageId`-keyed behaviour contract. It immediately found two real defects:

- **Unsupported `Content-Type` returned the wrong JSON-RPC error code**
  (**breaking**, error code only) — the JSON-RPC binding rejected an
  unsupported media type with `ParseError` (-32700); spec §5.4 maps it to
  `ContentTypeNotSupportedError` (-32005). The body is never parsed on that
  path, so the old code both misreported the cause and withheld the §10.6
  `CONTENT_TYPE_NOT_SUPPORTED` reason, which is now attached. Three in-repo
  tests had asserted the incorrect code and passed.
- **The task state machine rejected conformant agents** — six MUST-level
  checks failed because `TaskState::can_transition_to` required
  `Submitted → Working` before any finish state, so an agent that answered in
  one step got `InvalidParams` from its own SDK. Spec §4.1.3 defines no
  transition matrix and requires no intermediate state, and the reference SDKs
  complete directly from `Submitted`. The table now enforces only that
  terminal states are final and that nothing re-enters `Submitted` /
  `Unspecified`; `Working → Rejected` is permitted too (§4.1.3 allows
  rejecting "later"). `state_validation_tests.rs` was rewritten to pin all 81
  matrix cells against an independently-computed predicate.

Conformance went from 87 to 158 passing checks against the same SUT.

The remaining failures are recorded in `docs/official-tck-findings.md` rather
than skipped. That document also carries a correction: an earlier revision
claimed a bug in the TCK's JSON-RPC client (snake_case params vs spec §5.5).
**That claim was wrong and is retracted.** The A2A JSON data model is
generated from protobuf, and ProtoJSON parsers accept both the camelCase
`json_name` and the original snake_case field name; the official Python SDK
was measured doing exactly that. §5.5 governs emission, not acceptance.

The real defect the TCK surfaced is this SDK's own: **unrecognised request
parameters are silently ignored** where the reference rejects them, so a
`ListTasks` filter misspelled by any means — snake_case, wrong case, or a
typo — returns every task instead of an error or a filtered set. The reported
`historyLength` overrun has the same single root cause (`historyLength` is
honoured; the ignored `history_length` is not). Scope and fix direction are in
the findings document; the fix is deliberately not rushed in here, since it
needs an alias list generated from the protobuf schema and a per-field test,
not patches for the six spellings the TCK happens to send.

### Added

- **Developer Certificate of Origin adopted.** The project now requires a
  `Signed-off-by:` trailer on every commit, certified under the
  [DCO 1.1](DCO) (verbatim text added at the repository root). A new
  `.github/workflows/dco.yml` gate fails any pull request containing a
  non-merge commit without a sign-off matching its git author, and
  additionally rejects commits whose author is a known AI-assistant service
  account — a sign-off is an assertion by a person, so a tool identity cannot
  make one.
- **`PROVENANCE.md`** — a provenance record covering (1) a full disclosure of
  this project's AI-assisted development, with reproducible commit-authorship
  figures; (2) a one-time blanket DCO certification by the maintainer covering
  every commit through `b416c1a`, made because rewriting history would
  invalidate all ten release tags and the SLSA attestations bound to the
  published v0.2.0–v0.7.0 crates; and (3) an inventory of third-party material
  in the tree (the spec's `a2a.proto`, vendored googleapis stubs, the ITK
  `instruction.proto`, and the a2a-inspector card ruleset) with its licensing.
- **`.github/PULL_REQUEST_TEMPLATE.md`**, leading with the sign-off
  requirement.
- **`docs/rust-sdk-assessment.md`** — a source-verified technical and
  governance comparison of this SDK against `a2aproject/a2a-rs`, prepared for
  A2A project / Linux Foundation review.

### Changed

- **AI-assisted commits are now authored by the human who directed them**,
  with the assistant credited via a `Co-Authored-By:` trailer, replacing the
  prior pattern of `Claude <noreply@anthropic.com>` in the git author field.
  `CONTRIBUTING.md` documents the convention; CI enforces it.
- `CONTRIBUTING.md`, `GOVERNANCE.md`, `README.md`, and
  `.github/workflows/README.md` updated for the DCO requirement.

### Added (governance, legal, and release policy)

- **Security and conduct reports now go to a reachable address.**
  `a2a-rust.dev` is not registered — DNS returns NXDOMAIN — so both
  `security@a2a-rust.dev` (`SECURITY.md`'s primary vulnerability channel) and
  `conduct@a2a-rust.dev` (`CODE_OF_CONDUCT.md`'s only reporting channel) were
  undeliverable, and anything sent to either would have bounced or been lost.
  Both now point at the maintainer address already published in this project's
  copyright headers, and `SECURITY.md` promotes GitHub Security Advisories to
  the preferred channel since it stays private without a PGP key. Each file
  states plainly that the `a2a-rust.dev` address does not work, so it is not
  reinstated on the assumption that it does.
- **`ROADMAP.md`** — what the repository has already committed to for 0.8
  (three deprecation removals), the gaps in the project's own verification,
  the supply-chain items still open (unsigned tags, no PGP key, the
  unregistered domain), and the measured 92/114 TCK position. Derived only
  from what is already in the tree; it asserts no dates.
- **`SECURITY.md` release-artifact verification table** — states what is and
  is not signed. All ten release tags are lightweight, so `git tag -v`
  verifies nothing; adopters needing a cryptographic link to this repository
  must use the SLSA build provenance attestations instead.
- **`CODE_OF_CONDUCT.md`** — Contributor Covenant 2.1, with the four-tier
  enforcement ladder (Correction / Warning / Temporary Ban / Permanent Ban)
  and a conduct-specific reporting address. Replaces the four-line clause in
  `GOVERNANCE.md`, which routed conduct reports to the *security* mailbox;
  that section is now a pointer. The document also states its own limitation:
  with a single maintainer there is no independent party inside the project to
  escalate a report *about* that maintainer to.
- **`NOTICE`** — canonical Apache-2.0-form notice carrying the project
  copyright and the third-party attributions previously only enumerated in
  `PROVENANCE.md` (the spec's `a2a.proto`, vendored googleapis stubs, the ITK
  `instruction.proto`, the a2a-inspector card ruleset).
- **`.github/ISSUE_TEMPLATE/`** — structured bug-report and feature-request
  forms, plus a `config.yml` that disables blank issues and routes security
  reports to GitHub Security Advisories rather than the public tracker.
- **`RELEASING.md` — "Path to 1.0.0"**: explicit criteria for what would
  justify a 1.0.0 release (no unresolved MUST-level TCK failures, no coverage
  regression below the measured floor, a clean full mutation sweep, no open
  P0/P1s, and a deliberate `pub` API surface review), plus a **post-1.0
  deprecation policy** — `#[deprecated]` for at least one minor version
  before removal, removal only in a major bump, with security fixes exempt.
- **`docs/upstream/`** — a bug report against `a2aproject/a2a-tck` with a
  standalone reproduction script, covering a harness defect this SDK triggers
  by behaving correctly. Filed 2026-08-07 as
  [a2a-tck#225](https://github.com/a2aproject/a2a-tck/issues/225); the file is
  retained as the record of what was submitted and the evidence behind it.

### Changed (public API documentation)

- **`otel::init_otlp_pipeline` now documents that it panics outside a Tokio
  runtime.** Behaviour is unchanged; the precondition was previously
  undocumented. Calling it outside a runtime raises `there is no reactor
  running, must be called from the context of a Tokio 1.x runtime` from
  inside `tonic`'s channel constructor rather than returning `Err` — and
  because `[profile.release]` sets `panic = "abort"`, that aborts the process
  in release builds. Also newly documented: the installed `MeterProvider` is
  process-global and last-write-wins, and `shutdown()` returns an error when
  metrics have been recorded and no collector is reachable (it attempts a
  final flush), which graceful-termination code should treat as "metrics may
  have been lost" rather than as fatal. Found by adding the first test to ever
  call the function.

## [0.7.0] - 2026-07-24

Interop, hardening, and edge-case fixes from an independent protocol audit,
plus a protobuf-native rewrite of the gRPC transport. Several public types
changed shape (0.x breaking — warrants a minor bump).

### Added (cross-SDK interop & adversarial testing)

- **Official-SDK interop is now proven, both directions.** The TCK runs
  against echo agents built on the official Python (`a2a-sdk`),
  JavaScript (`@a2a-js/sdk`), Go (`a2a-go/v2`), and Java (`a2a-java`) SDKs
  — 20/20 on both JSON-RPC and REST for every one — and the official
  Python SDK *client* drives our server end to end
  (`itk/interop/python_client_vs_rust.py`). New `itk/agents/*-sdk`
  agents; TCK gained `--skip` for documented reference-SDK divergences.
- **Upstream ITK current-mount.** `itk/` is now the a2aproject/a2a-itk
  "current" agent (`itk-current-agent`), implementing the multi-hop
  traversal instruction protocol on `a2a-protocol-{server,client}` across
  JSON-RPC, gRPC, and HTTP+JSON (plain, streaming, push, resubscribe). A
  deterministic in-repo self-test and a CI workflow that mounts this repo
  into the real ITK against the official Python baseline both cover it.
- **Fuzzing expanded 1 → 6 targets** (JSON, JSON-RPC envelope + params,
  SSE parser, protobuf↔serde differential round-trip, ISO-8601, JWKS),
  wired into CI (60s smoke per PR, 10-min nightly).
- **Hostile-peer harness**: our client vs malicious servers (oversized /
  slow-drip / truncated bodies, immediate close, valid-JSON-wrong-shape)
  — each must fail safely, no hang or panic.
- **`SPEC_COMPLIANCE.md`**: a §-by-§ traceability matrix (spec → impl →
  test evidence). Release CI gained `cargo-semver-checks` and CycloneDX
  SBOM generation + attestation alongside the existing SLSA provenance.

### Changed (cross-SDK interop)

- **Default `AgentExecutor::cancel` now cancels a working task** instead of
  refusing with `TaskNotCancelable` — the handler already triggers the
  cancellation token, so the default emits the terminal `Canceled` status
  (best-effort delivery). Every reference SDK requires working cancel out
  of the box; the pre-0.7 default made `WORKING` tasks uncancelable and
  mislabeled the refusal as the task's fault. The cancel handler treats a
  re-read of `Canceled` as success rather than a TOCTOU race.
- **Example agent cards advertise the spec-canonical `"HTTP+JSON"`**
  protocol binding instead of the legacy `"REST"` spelling (the official
  Python client cannot match `"REST"` when selecting a transport). The
  client still accepts both spellings when reading a peer's card.

### Added (spec-compliance closure pass)

- **gRPC errors carry `google.rpc.ErrorInfo`** (spec §10.6) — every
  A2A-specific error now attaches the machine-readable
  `reason`/`domain: a2a-protocol.org` detail to `status.details` via
  `tonic-types`, making the three bindings error-equivalent (§5.1). The
  gRPC client decodes it back to the exact `ErrorCode` instead of the lossy
  status-code inverse mapping, and the REST client now decodes AIP-193
  error bodies (§11.6) into structured `A2aError`s the same way.
- **`taskId`-only continuations** (spec §3.4.3) — `SendMessage` with a
  `taskId` and no `contextId` now infers the context from the referenced
  task instead of rejecting with `InvalidParams`; an unknown `taskId`
  returns `TaskNotFoundError` per §3.4.2.
- **`statusTimestampAfter` is enforced** (spec §3.1.4) — previously parsed
  and silently ignored by every store; now applied as a strictly-after
  filter in the in-memory, SQLite, Postgres, and tenant-aware stores, with
  malformed timestamps rejected as `InvalidParams` at the handler.
- **`ListTasks` sorts by status timestamp** (spec §3.1.4) — ordering now
  follows `status.timestamp` descending (write wall-clock only as the
  fallback for tasks without one), so a re-save that does not change the
  status — e.g. an artifact append — no longer spuriously bumps a task to
  the front. Status timestamps gained millisecond precision (§5.6.1) to
  keep the order deterministic. The in-memory store's page tokens changed
  format (`millis:seq`); SQL cursors are unchanged.
- **Required-extension negotiation** (spec §3.3.4) — agent-card extensions
  marked `required: true` are now enforced on every data-plane operation:
  clients that do not declare them in `A2A-Extensions` get
  `ExtensionSupportRequiredError` (previously never emitted). The HTTP
  bindings also echo the activated extension set (requested ∩ declared)
  in the response `A2A-Extensions` header, matching official-SDK behavior.
- **JSON-RPC SSE envelopes echo the request `id`** (spec §9.4.2) —
  streaming frames previously carried `"id": null`.
- **gRPC validates the `a2a-version` service parameter** (spec §3.6.2 /
  §10.2), matching the JSON-RPC/REST/WebSocket bindings; REST and the
  WebSocket handshake now reject unsupported versions with the real
  `VersionNotSupportedError` shape (AIP-193 body with `ErrorInfo`) instead
  of an anonymous 400. The client WebSocket transport now sends
  `A2A-Version` on the upgrade request (§3.6.1).
- **OIDC discovery test coverage** — `from_oidc_issuer`/`discover_jwks_uri`
  now have end-to-end tests (live issuer, missing `jwks_uri`, invalid
  JSON, HTTP error, unreachable issuer), plus a live-TLS JWKS end-to-end
  test and a new `JwtAuthInterceptor::from_jwks_url_with_tls_config` for
  identity providers behind private CAs.

### Changed (reference-SDK interop pass)

Running our TCK against an echo agent built on the **official Python
`a2a-sdk` (1.1.2)** — instead of hand-written stubs — surfaced three
interop deviations, all fixed for exact reference parity (TCK now passes
20/20 against the official SDK on both JSON-RPC and REST):

- **Missing `A2A-Version` request header is now rejected** (spec §3.6.2,
  **breaking**) — a request without the header (or with an empty value)
  MUST be interpreted as protocol 0.3, which this server does not
  implement, so all data-plane HTTP/WebSocket calls now fail with
  `VersionNotSupported` exactly like the reference SDK. Agent-card
  discovery (`/.well-known/agent-card.json`) stays versionless — clients
  fetch it before they know anything about the agent. Opt out via
  `DispatchConfig::accept_missing_version_header()` (HTTP) or
  `WebSocketDispatcher::accept_missing_version_header()` for manual
  testing / trusted deployments. The TCK itself now sends
  `A2A-Version: 1.0` on every request — previously its requests were,
  per spec, 0.3 requests, and the reference SDK correctly refused them.
- **v0.3-style method names and paths removed** (**breaking**) — the
  JSON-RPC and WebSocket dispatchers no longer accept `message/send`,
  `tasks/get`, `tasks/pushNotificationConfig/*`, etc. as aliases for the
  v1.0 PascalCase RPC names (`SendMessage`, `GetTask`, …), and the REST
  binding no longer accepts `/message/send` for `/message:send`. The
  reference SDK returns `MethodNotFound`/404 for these (its 0.3 support
  is a separate opt-in adapter with real 0.3 payload semantics — which
  the aliases never provided).
- **TCK tolerates non-JSON error bodies on non-A2A paths** — a framework
  plain-text 404 for an unrouted path is legitimate (the reference SDK
  returns one); the status code is the conformance signal.

### Changed (spec-compliance closure pass)

- **Extended agent card requires authentication by default** (spec §13.3,
  **breaking**) — when the agent card declares
  `capabilities.extendedAgentCard: true` but the interceptor chain contains
  no authenticating interceptor, `GetExtendedAgentCard` now refuses to
  serve the card instead of handing the "authenticated" card to anonymous
  callers. `ServerInterceptor` gained a `authenticates()` marker (default
  `false`; the built-in API-key/bearer/JWT interceptors return `true` —
  custom auth interceptors should override it), and
  `RequestHandlerBuilder::allow_unauthenticated_extended_card()` is the
  explicit opt-out for deployments that authenticate upstream.
- **Slow streaming consumers get an explicit lag error** (**breaking
  semantics**) — a consumer that falls behind the broadcast ring previously
  had the gap silently skipped; it now receives a marked stream error
  (`data.streamLagged`) and the stream closes, so the client knows its view
  is truncated and can resubscribe for a fresh snapshot (§3.5.2). Task
  persistence was never affected (it uses a dedicated lossless channel).
  Relatedly, `with_event_queue_write_timeout` / `with_write_timeout` are
  deprecated no-ops (queue writes never block) slated for removal in 0.8.
- **Resubscribe after a process restart serves snapshot-then-EOF** — a
  non-terminal task with no live event queue previously produced
  `Internal ("no active event queue")`; per §3.5.2 reconnection the stream
  now delivers the current `Task` snapshot and ends cleanly.
- **Wire form of the protocol version is `1.0`** (spec §3.6, **breaking
  constant**) — `A2A_VERSION` changed from `"1.0.0"` to `"1.0"` ("patch
  version numbers SHOULD NOT be used in requests, responses and Agent
  Cards"); the `A2A-Version` response header and the examples' agent cards
  follow.
- **HTTP responses emit `Content-Type: application/json`** (spec §9.1 and
  §11.1; previously `application/a2a+json`). Both media types remain
  accepted on ingress, and the registered `application/a2a+json` constant
  is still exported.
- **Push webhook auth scheme matching is case-insensitive** (RFC 9110) —
  a config spelling the scheme `Bearer`/`BASIC` previously fell through
  and silently sent **no** `Authorization` header; canonical
  capitalization is now emitted regardless of config spelling.
- **Push delivery no longer retries non-retryable 4xx** — 400/401/403/404
  and similar fail fast after one attempt; retry with backoff is reserved
  for 408, 429, 5xx, timeouts, and connection errors.
- **Weekly full mutation sweep re-enabled** in CI (the incremental gate
  only covers files a PR touches), and dedicated `websocket`/`grpc`
  feature test legs were added alongside a combined
  `auth-jwt + tls-rustls` leg.

### Changed

- **gRPC is now protobuf-native and wire-compatible with the official A2A
  SDKs** — the transport speaks the canonical `lf.a2a.v1.A2AService`
  (fully-typed messages generated from the specification's protobuf
  schema, kept byte-identical in-repo) instead of the pre-0.7 JSON-in-
  `bytes` tunnel on a non-standard service. A Go, Python, or Java SDK
  peer can now interoperate over gRPC. Details in ADR 0009.
  - `a2a-protocol-types` gains a `proto` feature exposing the generated
    message types (`a2a_protocol_types::proto`) and a bidirectional
    `TryFrom` conversion layer to the serde domain types (ProtoJSON
    semantics; property-tested through real encoded protobuf bytes).
  - `a2a-protocol-server`: `GrpcDispatcher` serves the canonical service.
    The deprecated JSON tunnel (`a2a.v1.A2aService`) can still be served
    *alongside it* for 0.6 clients via the new `grpc-legacy-json` feature
    (off by default, removal planned for 0.8); rolling-upgrade coexistence
    is covered by an e2e test. `into_service` now returns the canonical
    service type; the legacy service is available via `into_legacy_service`.
  - `a2a-protocol-client`: `GrpcTransport` speaks the canonical service;
    the tunnel client was removed. Conversion failures surface as
    non-retryable `ClientError::Transport` errors.
  - Wire compatibility is proven against the official A2A Python SDK:
    golden binary fixtures serialized by `a2a-sdk` are checked in under
    `tck/fixtures/grpc/`, validated in both directions (prost decodes the
    official bytes; the official SDK parses prost-encoded bytes) by the
    new `grpc-wire-compat` CI job.
- **`a2a-protocol-types`: push types accept spec-compliant JSON** —
  `AuthenticationInfo.credentials` and `TaskPushNotificationConfig.taskId`
  are now `Option<String>`, matching the canonical protocol schema (both
  were previously required, rejecting valid cross-SDK payloads at parse
  time — e.g. a push config nested in `SendMessageConfiguration` before the
  task exists). A standalone `CreateTaskPushNotificationConfig` without a
  `taskId` is now rejected by the server with a structured invalid-params
  error instead of a parse error; all push-config stores guard the missing
  routing key explicitly.
- **`a2a-protocol-types`: JSON-RPC request ids are three-state** —
  `JsonRpcRequest.id` is now the `JsonRpcRequestId` enum
  (`Absent`/`Null`/`Value`). An explicit `"id": null` request previously
  collapsed into a notification on round-trip; per JSON-RPC 2.0 it is a
  *call* and now round-trips faithfully. Server responses are unchanged.
- **`a2a-protocol-server`: concurrent streams capped at 1024 by default** —
  `max_concurrent_streams` previously defaulted to unlimited; every stream
  eagerly allocates channels and spawns tasks, an unauthenticated DoS
  vector. Deployments needing more must raise the cap explicitly
  (`usize::MAX` effectively disables it). `executor_timeout` deliberately
  keeps no default, now documented as a production recommendation.
- **`a2a-protocol-server`: `RateLimitInterceptor` hardened** — caller
  identity no longer trusts the client-controlled `X-Forwarded-For` header
  unless `RateLimitConfig::trusted_proxy_hops` is set (forged headers
  previously bypassed the limit entirely); the bucket map is bounded by
  `max_buckets` (default 10,000); `RateLimitInterceptor::new` is now
  fallible and rejects zero config values (`window_secs == 0` previously
  panicked with a divide-by-zero at request time).
- **`a2a-protocol-client`: buffered response bodies capped** — unary
  responses were collected with no size limit; they are now capped at
  32 MiB by default (configurable via
  `ClientBuilder::with_max_response_size`), enforced during the read.

### Fixed

- **`a2a-protocol-types`: ambiguous `Part` content rejected** — a part
  carrying more than one content member (`text`/`raw`/`url`/`data`) was
  silently coalesced to the first match; it now fails deserialization.
- **`a2a-protocol-types`: RFC 8785 canonicalization conformance (signing)**
  — object keys now sort by UTF-16 code units (§3.2.3) and doubles use
  ECMAScript `Number::toString` formatting (§3.2.2), fixing cross-SDK
  signature mismatches for cards containing supplementary-plane keys or
  non-integer numbers in `AgentExtension.params`.
- **WebSocket (opt-in)**: the server's 4 MiB message cap is now enforced at
  the protocol level via `WebSocketConfig` (previously tungstenite's 64 MiB
  default applied and oversized messages were fully buffered before the
  check); the client no longer leaks a pending-request map entry per
  timed-out request.

### Security

- **`a2a-protocol-server`: a configured `TenantResolver` is now enforced.**
  `with_tenant_resolver(...)` previously configured a resolver that was never
  consulted — the client-controlled `tenant` field alone selected the store
  partition, so any caller could read or write another tenant's tasks by
  naming it. The resolver is now authoritative (tenant derived from trusted
  request context); a client-supplied tenant that disagrees is rejected. With
  no resolver configured, behavior is unchanged (single-tenant / trusted
  caller).
- **`a2a-protocol-server`: push-notification config count is bounded on every
  store backend.** A per-task cap (`HandlerLimits::max_push_configs_per_task`,
  default 100) is enforced in the handler; previously only the in-memory store
  self-enforced, so the SQLite/Postgres stores let a client mint unbounded
  configs for one task (disk exhaustion, and delivery amplification since each
  event fans out to every config).
- **`a2a-protocol-server`: tenant-aware in-memory push-config store no longer
  allocates a partition on read.** `get`/`list`/`delete` for an unseen tenant
  were creating (and counting) a partition, an unauthenticated `max_tenants`
  exhaustion vector.
- **`a2a-protocol-server`: OpenTelemetry error metrics are labeled by a bounded
  discriminant, not the error message.** The `error` attribute was the rendered
  message (embedding client-controlled ids), a metric-cardinality-explosion
  vector; it is now `ServerError::metric_label()` (a fixed set).
- **`a2a-protocol-server`: rate-limit caller keys canonicalize IP forms.** An
  IPv4-mapped IPv6 address and its plain IPv4 form now share one bucket, so a
  client cannot obtain two budgets by presenting both.
- **`a2a-protocol-client`: JSON-RPC error `data` and unknown codes are no longer
  discarded.** All transports preserve `error.data`, and an implementation-
  defined code outside the closed `ErrorCode` set is retained (under the error
  `data`) instead of collapsing silently to `InternalError`.
- **`a2a-protocol-client` (gRPC/WebSocket): unparseable auth metadata now fails
  the request closed** instead of being silently dropped and sent
  unauthenticated. Header values are never echoed in the error.
- **`a2a-protocol-types`: webhook secrets are redacted in `Debug`.**
  `AuthenticationInfo.credentials` and `TaskPushNotificationConfig.token` no
  longer print their values via `{:?}` (presence is still shown);
  serialization is unchanged.
- **`a2a-protocol-types` / proto: bounded recursion.** `Struct`/`Value`
  conversion and RFC 8785 canonicalization reject nesting past a fixed depth
  instead of risking a stack overflow on a programmatically-built value.

### Fixed (additional hardening)

- **`a2a-protocol-server`: fire-and-forget (`returnImmediately`) sends now run
  to completion.** They spawned no background processor and no persistence
  channel, so the executor's events went nowhere: nothing was persisted, no
  push fired, and the task was stuck in `Submitted` forever. They now use the
  background processor exactly like streaming.
- **`a2a-protocol-server`: hitting `max_concurrent_streams` returns a clean
  overload error with no side effects.** The cap was detected only after the
  task and cancellation token had been committed, orphaning the task in
  `Submitted` and returning a misleading internal error; the queue is now
  leased (and the cap checked) before any side effect, surfacing the new
  `ServerError::Overloaded` (gRPC `RESOURCE_EXHAUSTED` / HTTP 503).
- **`a2a-protocol-server`: a second `message/send` to a task already being
  processed is rejected** instead of spawning a second executor and overwriting
  the first's cancellation token (which left the original work uncancelable and
  racing on store writes).
- **`a2a-protocol-server`: `CancelTask` no longer leaks an event queue.**
  Cancelling a task whose executor had already exited registered a queue via
  `get_or_create` that nothing removed — a permanent map + concurrency-slot
  leak keyed by task id.
- **`a2a-protocol-server`: per-artifact `parts` growth is bounded**
  (`HandlerLimits::max_parts_per_artifact`, default 10,000), so an unbounded
  stream of `append: true` artifact updates cannot grow one task record without
  limit.
- **`a2a-protocol-server`: REST query percent-decoding is UTF-8 correct** —
  multi-byte sequences (e.g. a percent-encoded non-ASCII tenant name) decode to
  the original string instead of per-byte Latin-1 garbage.
- **`a2a-protocol-server`: the bundled `HttpPushSender` rejects `https://`
  webhooks with a clear, actionable error** (it is HTTP-only) instead of an
  opaque connector error after every retry; HTTPS delivery is available by
  supplying a TLS-capable `PushSender`.
- **`a2a-protocol-server`: agent-card hot-reload watchers no longer block a
  runtime worker** on file I/O (reads run on the blocking pool).
- **`a2a-protocol-client` (gRPC): `RESOURCE_EXHAUSTED` maps to a retryable
  error**, and mid-stream `Unavailable`/`DeadlineExceeded` keep their retryable
  classification (they were non-retryable `Protocol` errors); the configured
  response-size cap now applies to the gRPC decode limit.
- **`a2a-protocol-client` (REST streaming): the non-2xx error body is read
  through the size-capped collector** (was unbounded), and a non-SSE `200`
  response surfaces as an error instead of dissolving into a silently empty
  stream.
- **`a2a-protocol-client` (WebSocket): terminal-state detection recognizes the
  canonical `TASK_STATE_*` wire strings**, fixing a pending-map + sender leak of
  one entry per completed stream against every spec-conformant server;
  `route_frame` no longer holds the pending-map mutex across a bounded stream
  send (a stalled consumer could wedge the whole transport).
- **`a2a-protocol-client` (SSE): a BOM split across reads is stripped**
  correctly (the first event was being lost), and the tail of an over-limit
  event is discarded to the next boundary instead of being re-parsed into a
  spurious frame.
- **`a2a-protocol-client`: the spec binding name `HTTP+JSON` resolves to the
  REST transport** (it was rejected as unknown), and the retry jitter uses
  `Duration::try_from_secs_f64` to avoid a panic on a near-`Duration::MAX`
  backoff config.

### Added (second hardening pass)

- **HTTPS is first-class.** `tls-rustls` is now a **default** feature of
  `a2a-protocol-client` and `a2a-protocol-sdk`, so the client reaches
  `https://` agents — the spec-standard transport — out of the box (opt out
  with `default-features = false`). A new `a2a-protocol-server` `tls-rustls`
  feature makes the bundled `HttpPushSender` deliver to `https://` webhooks;
  the SDK's `tls-rustls` enables it too. Without the feature, `https://` fails
  fast with an actionable error (client `build()` and the push sender) instead
  of a late, opaque connector error. `HttpPushSender::with_tls_config` accepts a
  custom rustls `ClientConfig` (internal/private-CA webhooks, or mutual TLS).
  A live-TLS end-to-end test drives the sender through a real handshake against
  a `tokio-rustls` server on loopback (runs in the `tls-rustls` and
  `all-features` CI jobs).
- **`a2a-protocol-server`: opt-in strict multi-tenancy**
  (`RequestHandlerBuilder::require_resolved_tenant`) — a configured
  `TenantResolver` that returns `None` rejects the request instead of falling
  back to the shared default (`""`) partition. Off by default to preserve the
  documented resolver contract.
- **`a2a-protocol-server`: a global push-config ceiling**
  (`HandlerLimits::max_total_push_configs`, default 100,000; per-tenant for
  tenant stores) via a new `PushConfigStore::count` method.

### Fixed (second hardening pass)

- **`a2a-protocol-client`: automatic retries no longer duplicate work.**
  Non-idempotent methods (`SendMessage`, `SendStreamingMessage`,
  `CreateTaskPushNotificationConfig`, and any unknown method) are retried only
  when the server rejected the request up front (`429`/`503`), never on an
  ambiguous timeout or connection error the server may already have processed.
- **`a2a-protocol-client`: `request_timeout` is a single per-call deadline.**
  It was applied once to the response headers and again to the body read,
  letting a slow server hold a call for up to 2× the configured budget.
- **`a2a-protocol-client`: `Retry-After` (delta-seconds) is honored** on
  `429`/`503` in preference to the computed backoff (clamped to `max_backoff`),
  so the client stops hammering a server that asked it to wait.
- **`a2a-protocol-client`: WebSocket streams get an establishment timeout.**
  The WS streaming path previously returned a stream with no timeout of any
  kind; a socket accepted but never answered would hang the consumer forever.
- **`a2a-protocol-types`: `JsonRpcResponse` deserialization enforces JSON-RPC
  2.0 §5** (exactly one of `result`/`error`). A malformed response carrying
  *both* was silently read as success with the error discarded; a mistyped
  `result` now surfaces the real type error instead of an opaque
  "no variant matched".
- **`a2a-protocol-server`: closed a lost-work race in the send path.** A
  continuation send that hit an already-registered event queue (reachable via a
  cancel-then-resend race or an aged-out cancellation token) spawned a second
  executor with no persistence channel — silently dropping the resent task's
  state transitions and push notifications while racing the original on store
  writes. Such a send is now rejected. The stale-token sweep no longer evicts a
  still-running task's token.
- **`a2a-protocol-server`: SQL push-config backends are bounded.** The global
  ceiling (above) closes a disk-growth vector where configs spread across
  unboundedly many task ids had no limit on SQL stores.
- **Docs:** corrected the book's push-notifications page (the bundled sender's
  HTTPS behavior, a removed non-existent "HTTPS-only enforcement" toggle, and a
  won't-compile `credentials` example) and the "all features off by default"
  note (now that `tls-rustls` is a default).

### Changed (third hardening pass)

- **WebSocket is a full-surface, authenticated transport** — the WebSocket
  binding now matches the JSON-RPC HTTP dispatcher's method surface and
  security posture:
  - `a2a-protocol-server`: the upgrade request's HTTP headers (plus the
    request path, under `":path"`) are captured during the handshake and
    passed to the handler for every request on the connection — tenant
    resolvers, strict multi-tenancy, and header-based auth now work over
    WebSocket exactly as over HTTP. Previously the dispatcher passed an
    **empty** header map, so every WebSocket client resolved to the default
    tenant partition and header-based authentication was impossible.
  - `a2a-protocol-server`: all push-notification-config methods
    (`CreateTaskPushNotificationConfig`, `GetTaskPushNotificationConfig`,
    `ListTaskPushNotificationConfigs`, `DeleteTaskPushNotificationConfig`)
    and `GetExtendedAgentCard` are now routed over WebSocket, and every
    method also accepts its v0.3 `method/verb` alias (`message/send`,
    `tasks/get`, …) for parity with the HTTP dispatcher. Previously only
    6 of the 11 A2A methods were reachable over WebSocket.
  - `a2a-protocol-server`: an upgrade request carrying an `A2A-Version`
    header with a major version other than 1 is rejected during the
    handshake (HTTP 400), mirroring the HTTP dispatchers.
  - `a2a-protocol-client`: new `WebSocketTransportConfig` +
    `WebSocketTransport::connect_with_config` configure the request
    timeout, upgrade headers, and the incoming message-size cap in one
    place. Incoming messages are now capped (default 32 MiB, the shared
    response-size ceiling of the HTTP/gRPC transports) at the WebSocket
    protocol level — previously tungstenite's 64 MiB default applied.

### Added (third hardening pass)

- **First-party authentication helpers (client + server)** — the SDK now
  *acquires* and *verifies* credentials, not just models the schemes and
  provides interceptor hooks (ADR 0010). Built on the existing `ring`/`hyper`
  stack — no OAuth-ecosystem dependencies.
  - `a2a-protocol-client`: a `TokenProvider` trait with `StaticTokenProvider`,
    a `BearerAuthInterceptor` that injects a fresh token before every request
    (so a rotating token stays current, including on retries), and
    `OAuth2ClientCredentials` — the RFC 6749 §4.4 client-credentials grant with
    token caching, proactive pre-expiry refresh, single-flight concurrent
    refresh, `Basic`/`Post` client-auth styles, and constructors that read the
    token endpoint from an agent card's OAuth2 flow or discover it from an OIDC
    issuer. Client secrets are redacted from `Debug` and never echoed in errors.
  - `a2a-protocol-server`: `ApiKeyAuthInterceptor` and
    `BearerTokenAuthInterceptor` (constant-time comparison, no feature flag)
    and, behind the new `auth-jwt` feature, `JwtAuthInterceptor` — verifies
    HS256/RS256/ES256, checks `exp`/`nbf`/`iss`/`aud`, and resolves keys from a
    static `Jwks`, a shared HS256 secret, or a remote JWKS endpoint (TTL-cached,
    refetched once on a key-id miss to follow rotation) with OIDC discovery.
    `alg: none` and unlisted algorithms are rejected, and HS256 is only ever
    checked against a configured secret — never a JWKS public key — so the
    RS256→HS256 confusion downgrade is structurally impossible. Rejections are
    generic (no oracle) and map to `InvalidRequest` (HTTP 400). JWT verification
    is cross-checked against independently-generated (Python) test vectors and
    a live interceptor→dispatcher→HTTP end-to-end test.
  - `a2a-protocol-sdk`: re-exports the client/server auth types from the
    prelude; the `auth-jwt` feature passes through to the server.
- **`A2A-Extensions` header wired end to end (spec §14.2.2)** — new
  `A2A_EXTENSIONS_HEADER` constant in `a2a-protocol-types`, and every
  server binding now parses the comma-separated extension URIs into
  `CallContext::extensions` for interceptors and resolvers (the accessor
  existed but nothing populated it — it always returned empty). Extension
  *data* continues to ride in-band in `Message::extensions`/metadata.

### Fixed (third hardening pass)

- **`a2a-protocol-server` (push): the notification token is sent as
  `X-A2A-Notification-Token`** — the header name the spec's push example
  uses and the official SDK's webhook receivers read. The bare
  `a2a-notification-token` name was this SDK's own pre-0.7 invention, so
  receivers written against the official convention never saw the token;
  it is still sent alongside the canonical name for migration and will be
  removed in 0.8. The default CORS allow-list now also includes
  `a2a-version` and `a2a-extensions` — protocol headers A2A clients send
  on every request, without which a browser client's preflight fails.
- **`a2a-protocol-server` (REST): the canonical `/{tenant}/...` bindings
  are now routed** — the spec proto's `google.api.http` additional
  bindings put the tenant as a bare first path segment
  (`/{tenant}/message:send`, `/{tenant}/tasks`, …), which is exactly what
  official-SDK REST clients send when configured with a tenant; only this
  SDK's own `/tenants/{tenant}/...` form was recognized, so canonical
  tenant-scoped requests 404'd. Both forms now work, with
  literal-beats-variable matching so a real route is never swallowed as a
  tenant. Verified live with the official Python SDK's `RestTransport`
  (tenant-scoped send/list/get all round-trip).

- **`a2a-protocol-types`: ProtoJSON empty-repeated omission no longer breaks
  cross-SDK parsing** — ProtoJSON printers (what every official A2A SDK
  uses on the JSON wire) omit empty repeated fields and empty maps, so
  absence means "empty". Twelve JSON-facing list/map fields required their
  key to be present and rejected real official-SDK traffic at parse time —
  found live by driving this SDK's JSON-RPC server with the official
  Python SDK's client, whose `configuration` object legitimately omits an
  empty `acceptedOutputModes` and was answered with
  "invalid params: missing field `acceptedOutputModes`". All repeated
  fields now deserialize absent-as-empty
  (`SendMessageConfiguration.acceptedOutputModes`, `StringList.list`,
  `SecurityRequirement.schemes`, `TaskListResponse.tasks`,
  `ListPushConfigsResponse.configs`, `AgentCard.{supportedInterfaces,
  defaultInputModes, defaultOutputModes, skills}`, `AgentSkill.tags`,
  `Message.parts`, `Artifact.parts`). Semantic must-be-non-empty
  requirements are unchanged and enforced where they belong: the server
  still rejects empty message parts with a structured invalid-params
  error, event processors still drop empty-parts artifacts, and
  `AgentCard::validate()` still requires at least one interface — errors
  that now name the real problem instead of a JSON "missing field" type
  error. Serialized output is unchanged. Verified end-to-end against the
  official Python SDK (`a2a-sdk` 1.1.2): agent-card resolution, unary
  send, SSE streaming, and gRPC (all 33 golden fixtures plus live
  unary/streaming/mid-flight-subscribe probes) all pass in both
  directions.
- **`a2a-protocol-server` (WebSocket): accept-loop and handshake
  resilience** — a transient `accept()` error (per-connection abort,
  fd-table exhaustion) no longer tears down the WebSocket server; the
  accept loop now follows the same retry-with-backoff policy as the HTTP
  serve path. A peer that opens a TCP connection but never completes the
  WebSocket handshake is disconnected after a bounded handshake timeout
  (default 10 s, configurable via `with_handshake_timeout`) instead of
  pinning a connection and file descriptor indefinitely. The server also
  completes the WebSocket close handshake on shutdown paths, disables
  Nagle's algorithm to match the HTTP path's latency profile, answers
  binary frames with an explicit error instead of silence, and correlates
  "server busy"/"message too large" rejections with the request `id` so
  clients fail fast instead of timing out on an unroutable null-id error.
- **`a2a-protocol-client` (WebSocket): dropped transports leaked their
  connection; disconnects hung in-flight requests** — dropping a
  `WebSocketTransport` now aborts its background reader/writer tasks and
  closes the socket (a tokio `JoinHandle` detaches on drop, so every
  dropped transport previously leaked a task and an open TCP connection
  until the server closed it). A server-initiated close, end-of-stream, or
  write failure now fails **all** in-flight requests promptly with a
  transport error and marks the connection dead so subsequent requests
  fail fast — previously a clean server close left pending requests
  hanging for their full request timeout, and new requests kept queuing
  against the dead socket.
- **CI: per-benchmark regression tolerance** — the benchmark gate accepts
  targeted `--override PATTERN=THRESHOLD` globs, and the known-noisy
  `from_str/16384` benchmark (tiny absolute runtime; tight-CI swings on
  identical code while its size neighbours stay stable) now carries a 75%
  tolerance instead of the whole gate being loosened. Overridden rows are
  labelled in the gate output, and a regression past the raised threshold
  still fails.
- **`a2a-protocol-client`: every streaming transport now bounds the wait for
  the first event** — the HTTP JSON-RPC, REST, and gRPC streaming paths
  previously bounded only stream *establishment* (response headers /
  stream open); a server that accepted the stream and then went silent
  hung the consumer forever. All three now apply the same
  first-event timeout the WebSocket transport already had (lifted after
  the first frame — long-running quiet tasks are unaffected because SSE
  keep-alives and the spec-required initial Task event count as frames).
- **`a2a-protocol-client`: streaming error responses keep `Retry-After`** —
  a rate-limited stream start (HTTP 429/503 with `Retry-After`) surfaced
  `retry_after: None` on the JSON-RPC and REST paths, so the retry layer
  used its own short jittered backoff instead of the server-directed
  delay the unary paths already honor.
- **`a2a-protocol-client`: SSE parser accepts bare-CR line terminators** —
  the WHATWG SSE grammar allows CRLF, LF, or CR line endings; the parser
  handled only LF/CRLF, so a CR-only server's entire stream accumulated
  into one "line" and was rejected as oversized. CR now terminates a
  line, with a CRLF pair split across reads counted as one terminator.
- **`a2a-protocol-server`: stale-token sweep can no longer evict a live
  replacement token** — between candidate collection (read lock) and
  removal (write lock), a cancel-then-resend race can insert a fresh live
  cancellation token for the same task id; the sweep removed it by id
  unconditionally, leaving the resent executor uncancelable for its whole
  run. Removal now re-validates each entry under the write lock and
  spares tokens that are neither cancelled nor aged.
- **`a2a-protocol-server`: background event processor survives a missing
  task row at startup** — if the just-saved task vanished before the
  processor's initial store read (capacity eviction under extreme churn,
  or a transient store fault), the processor exited early and dropped its
  persistence receiver: every subsequent state transition and push
  notification for the task was silently lost while the client's stream
  kept delivering. It now falls back to the send path's task snapshot,
  re-asserting the row on a confirmed miss.
- **`a2a-protocol-server`: path-traversal detection decodes to a fixpoint** —
  the REST guard decoded percent-encoding exactly twice, so
  triple-encoded `..` passed undetected (not reachable today — routing
  matches raw segments — but the detector no longer encodes that
  assumption; undecodable-after-8-passes input now fails closed).
- **Docs:** honest-limits pass — `TaskStoreConfig::max_capacity` documents
  the last-resort eviction of non-terminal tasks under overload,
  `HandlerLimits` sweep thresholds are documented as prune triggers
  rather than hard bounds, `PerTenantConfig` documents fairness under
  shared process-wide caps, the retry layer documents that interceptors
  run once per call (not per attempt), and push-config creation documents
  its sync-check-at-create / DNS-recheck-at-delivery split.

### Changed (fourth hardening pass)

- **`ListTasks` returns tasks most-recently-updated first (spec §3.1.4)** —
  every task store previously returned tasks in ascending `id` order (an
  arbitrary lexical order the spec does not permit), and paged with an
  `id`-based cursor. All five stores now order by last-update time,
  most-recent first, with a stable cursor:
  - `InMemoryTaskStore` keys a `BTreeMap<u64, TaskId>` update-order index by
    a monotonic per-write sequence, iterated in reverse — O(log n +
    page_size) pagination with no per-call sort, and a collision-free
    integer cursor. Every write (not just the first insert) re-positions the
    task to the front, so an in-place update correctly moves it.
  - The SQLite/PostgreSQL stores (and their tenant-scoped variants) order by
    `(updated_at DESC, id DESC)` with a composite row-value cursor
    `(updated_at, id) < (?, ?)`; because `id` is the primary key the pair is
    unique, so pagination never drops or repeats a row even when many tasks
    share a timestamp. SQLite now records `updated_at` at millisecond
    precision (fixed-width, so `TEXT` comparison matches chronological
    order); PostgreSQL serializes the cursor timestamp as a UTC-normalized
    microsecond string so paging is stable regardless of session time zone.
    New `(updated_at, id)` indexes back the ordering (added via SQLite
    migration v4 / PostgreSQL migration v3).
- **Capability validation enforced at the handler (spec §3.3.4)** — when an
  `AgentCard` is configured, the server now honors its advertised
  `capabilities`. `SendStreamingMessage` and `SubscribeToTask` return
  `UnsupportedOperationError` unless `capabilities.streaming` is `true`; the
  push-config operations (Create/Get/List/Delete) return
  `PushNotificationNotSupportedError` unless `capabilities.pushNotifications`
  is `true`. A card-less handler is unaffected (it publishes no capability
  contract), so existing card-less deployments keep working.

### Fixed (fourth hardening pass)

- **`GetTaskPushNotificationConfig` reports a missing config as
  `TaskNotFoundError`** (spec §3.1.8) instead of `InvalidParams` — over REST
  this changes the HTTP status from 400 to 404, matching the canonical error
  mapping and the other SDKs.
- **`CreateTaskPushNotificationConfig` validates that the target task exists**
  (spec §3.1.7), returning `TaskNotFoundError` instead of silently storing an
  unroutable, orphaned config for a task that was never created.
- **Cross-binding metadata portability** — the send path now rejects a
  client-supplied `metadata` value (on the request, the message, or any
  message part) that is present but not a JSON object, with a structured
  invalid-params error. A protobuf `google.protobuf.Struct` — the gRPC wire
  form of every `metadata` field — can only hold a JSON object, so a task
  accepted over JSON-RPC/REST with, e.g., `metadata: [1, 2, 3]` would fail to
  serialize the instant it was served over gRPC. Rejecting it at ingress
  keeps every accepted task representable across all A2A transports.
- **JWT `exp` is now fail-closed at the boundary** (`auth-jwt`) — the
  expiration check accepted a token at exactly `exp + leeway` (`now > exp`).
  RFC 7519 §4.1.4 requires a token to be rejected "on or after" `exp`, so the
  check is now `now >= exp + leeway`. `nbf` was already correct (valid at
  exactly `nbf`, per §4.1.5 "not before"). Tokens sitting exactly at their
  expiry instant are now rejected.
- **Mutation-test hardening of the changed surface** — closed every gap the
  incremental mutation-testing gate found in this pass's diff, so the new and
  touched code is covered by assertions that pin behavior, not just line
  coverage. This added deterministic tests for JWT time-boundary and JWKS
  parsing/redaction logic, a shared unit-tested pagination boundary helper
  used by all five stores, and DER length-encoding tests — and extracted a
  couple of pure helpers (`check_claims_at(now)`, `cache_is_fresh`) so the
  crypto/validation boundaries are testable without a wall clock.

## [0.6.0] - 2026-06-10

Released as a **minor** (not patch) bump: no public API signatures changed,
but observable behavior did — `Task.history` is now populated in responses,
streaming disconnects no longer fail running tasks, and `Working → Working`
status refreshes are accepted. Consumers relying on the old behaviors should
read the entries below.

### Changed

- **`Task.history` is retrievable, and send responses opt in to it** —
  `tasks/get` returns the populated history (subject to `historyLength`;
  previously the field was always absent). `message/send` responses and
  streaming snapshots omit history unless
  `SendMessageConfiguration.historyLength` requests it: echoing the
  just-sent message back doubled response payloads for large sends (+95%
  median at 1 MiB, caught by the benchmark regression gate).
- **A dropped `message/stream` connection no longer cancels work** — tasks
  continue to completion and clients reattach via `SubscribeToTask`.
  Anything depending on disconnect-kills-task semantics must cancel
  explicitly via `CancelTask`.

### Fixed

- **`a2a-protocol-server`: a client disconnecting from `message/stream` no
  longer fails the running task** — The event-queue writer treated a
  broadcast send with zero live receivers as an executor-fatal error, so the
  only SSE consumer dropping its connection (network blip, closed tab)
  marked the in-flight task `TASK_STATE_FAILED` even though every event had
  already reached the persistence channel. Zero receivers is now a non-error
  whenever a persistence channel exists — the task keeps running and clients
  can reattach via `tasks/resubscribe` (which is the reason that method
  exists). Sync mode is unchanged: there the sole receiver *is* the request.

- **`a2a-protocol-types`: `Working → Working` is now a valid state
  transition** — Repeated `Working` status updates, each carrying a new
  `status.message`, are how an agent narrates long-running work to streaming
  clients. The state machine rejected the self-transition, so any executor
  that emitted more than one progress note had its task marked
  `TASK_STATE_FAILED` by the background processor *while the SSE stream
  delivered the same events and a final `Completed` to the client* — a
  stream/store split-brain. Self-transitions remain invalid for every other
  state.

- **`a2a-protocol-client`: JSON-RPC error responses to `message/stream` are
  surfaced instead of dissolving into an empty stream** — A streaming
  request that fails up-front (e.g. continuing a task with the wrong
  `contextId`) returns HTTP 200 with a JSON-RPC error envelope. The client
  fed that body to the SSE parser, which found no frames and ended the
  stream with zero events and no error. The transport now checks the
  response content type: JSON-RPC error envelopes map to
  `ClientError::Protocol` with the original code, and other non-SSE bodies
  map to `ClientError::Transport`.

- **`a2a-protocol-server`: `Task.history` is now actually populated** —
  Nothing ever appended messages to task history: tasks were created with
  `history: None`, continuations never added the new message, and both event
  processors explicitly ignored agent `Message` events. `GetTask`'s
  `historyLength` parameter (fixed in 0.3.4 to "truncate history") truncated
  a permanently empty list, and multi-turn executors could not see prior
  turns via `RequestContext::stored_task`. Incoming user messages are now
  appended at send time, agent `Message` events are recorded by both the
  sync and background processors, and history is capped at 1,024 messages
  (oldest dropped first).

- **`a2a-protocol-server`: continuations no longer wipe accumulated task
  state** — Sending a follow-up message to an existing non-terminal task
  saved a freshly constructed task over the stored one, destroying its
  accumulated artifacts, metadata, and history. Continuations now carry all
  three forward; only the status returns to `Submitted` for the new turn.

- **`a2a-protocol-server`: JSON-RPC legacy alias `tasks/resubscribe`** —
  The dispatcher accepted the v1.0 method `SubscribeToTask` and the alias
  `tasks/subscribe`, but not `tasks/resubscribe` — the actual method name
  from the v0.2.x spec that older clients send. All three now route to
  resubscription.

All of the above were found by driving the new `incident-response` example's
multi-turn, multi-agent flow end-to-end against a live local model — including
a full probe of resubscribe-after-disconnect (works: a reattached client
receives the remaining events and terminal state) and real webhook push
delivery (works: deliveries carry the `a2a-notification-token` header and
continue while no SSE consumer is connected).

### Added

- **`incident-response` example** — A three-agent incident-response team
  (triage orchestrator + deterministic log-search agent + LLM runbook agent)
  demonstrating `INPUT_REQUIRED` multi-turn continuation, agent-to-agent
  delegation, streaming progress narration, artifacts, cooperative
  cancellation, and honest failure states. Runs fully local (llama-server /
  Ollama) or with hosted providers, and passes the TCK 20/20.
- **TCK: `a2a_media_type_accepted` conformance test** — Servers must accept
  the registered A2A media type `application/a2a+json` that production
  clients send, not just plain `application/json`. Added after the Rust
  client's requests were rejected by the JS ITK agent, whose JSON body
  parser was content-type-strict (fixed too); the cross-language CI matrix
  now enforces this for every agent.

### Security

- **`rustls-webpki` upgraded to 0.103.12** — Fixes RUSTSEC-2026-0098
  ([GHSA-965h-392x-2mh5](https://github.com/rustls/webpki/security/advisories/GHSA-965h-392x-2mh5)):
  name constraints for URI names were incorrectly accepted during X.509 path
  validation, which could allow a CA with URI `NameConstraints` to be bypassed
  for certificates it should have been restricted from issuing. The bug
  affected all rustls-webpki releases in the 0.103.x line through 0.103.11 and
  reaches `a2a-protocol-client` transitively via `rustls` → `hyper-rustls`
  / `tokio-rustls` whenever the `tls-rustls` feature is enabled.

  Exploitability in practice requires a trust chain whose CA uses URI name
  constraints — uncommon in the public Web PKI (Mozilla/`webpki-roots`), but
  possible for consumers wiring a private or enterprise CA bundle into the
  client. Out of caution this is shipped as a dedicated patch release so
  downstreams can pick it up without waiting for unrelated changes.

  No code changes are required by consumers; `cargo update -p rustls-webpki`
  on existing lockfiles is sufficient for anyone not yet moving to 0.5.1.

## [0.5.0] - 2026-04-02

### Breaking Changes

- **`TaskStore::save()` and `TaskStore::insert_if_absent()` now accept `&Task`
  instead of owned `Task`** — This eliminates forced `.clone()` at every call
  site. Store implementations that need ownership (e.g., `InMemoryTaskStore`)
  clone internally; database-backed stores (`SqliteTaskStore`,
  `PostgresTaskStore`) borrow fields directly and never clone.

  **Migration guide:**
  ```rust
  // Before (0.4.x):
  store.save(task.clone()).await?;
  store.insert_if_absent(task).await?;

  // After (0.5.0):
  store.save(&task).await?;
  store.insert_if_absent(&task).await?;
  ```

  Custom `TaskStore` implementations must update their method signatures:
  ```rust
  // Before:
  fn save<'a>(&'a self, task: Task) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>>;

  // After:
  fn save<'a>(&'a self, task: &'a Task) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>>;
  ```

- **Version bump: 0.4.1 → 0.5.0** — All four crates (`a2a-protocol-types`,
  `a2a-protocol-client`, `a2a-protocol-server`, `a2a-protocol-sdk`) are bumped
  to 0.5.0 to signal the breaking `TaskStore` trait change.

### Performance

- **Broadcast channel capacity increased from 64 to 256 events** — Pushes
  the per-event cost inflection from ~52 to ~252 events, reducing broadcast
  buffer pressure for high-volume streaming tasks.
- **`serde_helpers` module** (`a2a-protocol-types`) — `SerBuffer` provides
  thread-local reusable serialization buffers (2.3× less overhead on small
  payloads); `deser_from_str`/`deser_from_slice` enable borrowed
  deserialization (~15-25% fewer allocations).
- **SSE frame building uses thread-local reusable buffer** — Amortized 0
  allocations per event vs previous 1 allocation per event.
- **267 benchmarks, zero panics, zero errors** — Cleanest benchmark run in
  project history. All 13 benchmark suites (transport, protocol, lifecycle,
  concurrency, cross-language, realistic, error paths, backpressure, data
  volume, memory, enterprise, production, advanced) pass with zero failures.
- **Streaming bimodal distribution fully resolved** — Zero streaming benchmarks
  appear in the high-outlier list. `stream_drain` confidence interval tightened
  from [1.79ms, 2.11ms] (18% range) to [1.59ms, 1.67ms] (5% range).
- **Agent burst sub-linear scaling confirmed** — Per-agent cost drops from
  714µs/agent (10 agents) to 310µs/agent (100 agents). SDK handles high-fanout
  agent coordination without degradation.
- **Subscribe fan-out O(1) up to 5 subscribers** — 1 subscriber = 2.90ms,
  5 subscribers = 2.89ms. Broadcast channel delivers in a single pass.
- **Pagination context index 2x speedup** — Filtered walk at 1K tasks: 309µs
  vs unfiltered 592µs. BTreeSet context index eliminates half the scan work.
- **Tenant resolvers effectively free** — 88–173ns per request (~0.008% of a
  typical 1.6ms round-trip).
- **SSE streaming bimodal distribution eliminated** — Root-caused the ~24%
  high severe outlier rate in all streaming benchmarks to cross-thread task
  scheduling: on a 4-core system, `tokio::spawn` has a 3/4 probability of
  placing the SSE builder task on a different worker thread, causing a ~500µs
  cache-miss + work-stealing penalty. Three production fixes applied:
  1. Replaced `tokio::time::interval` with `tokio::time::sleep` + reset
     pattern in `build_sse_response` — eliminates persistent timer wheel
     registration during active streaming
  2. Added `tokio::task::yield_now()` before read loops in SSE builder
     (server) and body reader tasks (client JSON-RPC + REST)
  3. Transport streaming benchmarks now use `worker_threads(1)` runtime
     and streaming-specific warmup, reducing outliers from 24 high severe
     to 4 high mild and tightening confidence intervals by 3×

### Fixed

- **Benchmark server `AddrInUse` on CI** — Benchmark servers now set
  `SO_REUSEADDR` + `SO_REUSEPORT` via `socket2` and use a graceful shutdown
  handle (`watch::Sender<bool>`) so that rapid server cycling during cold-start
  benchmarks does not fail with `Address already in use` on CI runners where
  `TIME_WAIT` recycling is slower.
- **Criterion timeout warnings eliminated (round 2)** — Bumped `measurement_time`
  for 5 additional benchmark groups based on CI analysis: `transport/payload_scaling`
  (8s→10s), `concurrent/sends` (18s→30s), `realistic/payload_complexity` (10s→15s),
  `realistic/connection` (10s→15s), `enterprise/client_interceptors` (8s→10s).
  All 267 benchmarks now complete within their budget on CI runners.
- **Push config benchmark per-task limit** — `production/push_config/set_roundtrip`
  and `delete_roundtrip` now upsert a pre-created config instead of creating new
  configs each iteration, preventing `push config limit exceeded` panics during
  criterion warmup.

### Benchmarks

- **Transport payload scaling extended to 1MB** — Added 100KB and 1MB payload
  sizes to `transport_throughput.rs` for large-payload regression detection.
- **New `protocol/payload_scaling` isolation benchmarks** — Pure serde cost
  from 64B to 1MB in `protocol_overhead.rs`; compares `to_vec` vs `SerBuffer`
  and `from_slice` vs `from_str` for serde regression detection.
- **Cache-busting step for `data_volume/get` at 100K** — 4MB allocation to
  flush CPU caches between populate and measure, eliminating the cache warming
  artifact.
- **Documentation comments added** — Connection reuse best practices, cold
  start vs steady state explanation, concurrent store anomaly notes added to
  benchmark files.

### Changed

- **Benchmark documentation expanded** — Added 8 new "Known Measurement
  Limitations" entries to `benches/README.md` and the auto-generated GH Book
  benchmarks page: data_volume/save wide CIs, dispatch routing inverted results,
  cold start vs steady state, subscribe fan-out O(1) scaling, agent burst
  sub-linear scaling, tenant resolver overhead, pagination context index speedup.
  These complement the existing entries for streaming bimodal distribution,
  get()/100K cache anomaly, stream volume per-event cost inflection, and slow
  consumer timer calibration.
- **Stream volume scaling documentation** — Added detailed per-event cost
  analysis comments to `backpressure.rs` explaining the broadcast channel
  capacity-driven inflection at 252+ events.

### Performance

- **`a2a-protocol-server`: `InMemoryTaskStore::list()` O(n log n) → O(log n + page_size)** —
  Added `BTreeSet<TaskId>` sorted index and `HashMap<String, BTreeSet<TaskId>>`
  context_id secondary index. Eliminates the per-call sort that caused 20-70×
  regressions at 10K+ tasks. Uses `BTreeSet::range()` for O(log n) cursor
  positioning.
- **`a2a-protocol-server`: SSE per-event allocation reduced** — New
  `build_sse_message_frame()` serializes JSON directly into the SSE frame
  buffer via `serde_json::to_writer`, reducing per-event allocations from 2 to 1.
- **`a2a-protocol-types`: Part deserialization ~80 fewer allocations per Task** —
  Replaced `#[serde(flatten)]` on `Part.content` with a hand-rolled `Deserialize`
  implementation that reads all fields in a single pass without intermediate
  `serde_json::Value` buffering.

### Added

- **`advanced_scenarios` benchmark suite** — Tenant resolver overhead (header,
  bearer, path segment extraction); agent card hot-reload (read, update, complex
  card swap); `/.well-known/agent.json` discovery endpoint latency; subscribe
  fan-out (1–10 concurrent subscribers); streaming artifact accumulation cost
  (`task.clone()` at 0–500 artifact depth); pagination full walk (100–1K tasks,
  unfiltered + context-filtered); extended agent card round-trip.
- **`production_scenarios` benchmark suite** — SubscribeToTask reconnection,
  cold start vs steady-state, concurrent cancel+subscribe race, 7-step E2E
  orchestration, push config CRUD round-trip, parallel agent burst (10-100
  agents), dispatch routing isolation.
- **Timer calibration benchmark** — Measures actual `tokio::time::sleep()`
  duration to isolate CI timer jitter from real SDK overhead.
- **`NoopPushSender`** for benchmarks that require push notification support
  without performing actual HTTP webhook delivery.
- **`start_jsonrpc_server_with_push()`** helper for benchmark servers with push
  notification capabilities enabled.

### Fixed

- **`MultiEventExecutor` invalid state transitions** — Was emitting
  `Working → Working` status events in a loop, violating the A2A spec state
  machine. Now emits `Working` once, then N artifact events, then `Completed`.
- **`production_scenarios` push config benchmark** — Was using a server without
  push notification support, causing `PushNotificationNotSupported` errors.
- **`production_scenarios` dispatch routing benchmark** — Pre-allocate params
  outside the measurement loop for `direct_handler_invoke` to isolate handler
  dispatch cost from fixture allocation cost, producing a fairer comparison
  against the HTTP round-trip path.
- **`InMemoryTaskStore::insert()` unnecessary index operations** — Update path
  now skips BTreeSet and context index operations when the task already exists
  with the same context_id, eliminating variance from occasional BTreeSet node
  splits and reducing update cost from ~2.5µs to ~700ns.
- **Criterion `measurement_time` warnings** — Added `measurement_time` to 23+
  benchmark groups across 8 files, eliminating all 15 warnings and preventing
  23 borderline cases from triggering on CI runners.

## [0.4.1] - 2026-03-31

### Fixed

- **`a2a-protocol-client`: REST streaming deserialization failure** — The REST
  binding sends bare `StreamResponse` JSON in SSE frames (per A2A spec Section
  11.7), but the client always tried to unwrap a JSON-RPC envelope, causing
  `"data did not match any variant of untagged enum JsonRpcResponse"` errors.
  `EventStream` now tracks the transport binding and parses bare responses for
  REST streams.

## [0.4.0] - 2026-03-31

### Breaking Changes

This release implements full A2A v1.0.0 wire format compliance. The following
changes are **breaking** — existing clients and servers using the old wire format
will need to update.

- **Part wire format migrated to v1.0 flat oneof** — Parts no longer use
  `{"type": "text", "text": "..."}` discriminated format. The new format uses
  JSON member name as discriminator: `{"text": "..."}`, `{"raw": "base64..."}`,
  `{"url": "https://..."}`, `{"data": {...}}`. Top-level `filename` and
  `mediaType` fields replace the nested `file` object. The `PartContent` enum
  now has `Text`, `Raw`, `Url`, and `Data` variants (previously `Text`, `File`,
  `Data`). The old `FileContent` struct is retained for backward-compatible
  constructors only.

- **Enum serialization uses ProtoJSON SCREAMING_SNAKE_CASE** — `TaskState` now
  serializes as `"TASK_STATE_COMPLETED"`, `"TASK_STATE_INPUT_REQUIRED"`, etc.
  (previously `"completed"`, `"input-required"`). `MessageRole` now serializes
  as `"ROLE_USER"`, `"ROLE_AGENT"` (previously `"user"`, `"agent"`). Legacy
  lowercase values are still accepted on deserialization via serde aliases.

- **`SendMessageResponse` uses externally tagged format** — Now serializes as
  `{"task": {...}}` or `{"message": {...}}` per proto `oneof payload` semantics
  (previously untagged — just the inner object).

- **Agent Card well-known path changed** — Discovery path is now
  `/.well-known/agent-card.json` (previously `/.well-known/agent.json`),
  matching the spec Section 8.2 and IANA registration (Section 14.3).

- **`OAuthFlows` is now an enum (oneof)** — Previously a struct with five
  optional fields, now an enum with one variant per flow type, matching the
  proto `oneof flow` definition. Only one OAuth flow can be specified at a time.

- **Error responses use AIP-193 format** — REST errors now follow
  `{"error": {"code": N, "status": "...", "message": "...", "details": [...]}}`.
  JSON-RPC errors include `google.rpc.ErrorInfo` in the `data` array with
  `@type`, `reason` (UPPER_SNAKE_CASE), and `domain` ("a2a-protocol.org").

### Fixed

- **HTTP error status codes corrected for all 9 A2A error types** — Per Section
  5.4: `ContentTypeNotSupported` → 415, `InvalidAgentResponse` → 502,
  `UnsupportedOperation` → 400, `ExtendedAgentCardNotConfigured` → 400,
  `ExtensionSupportRequired` → 400, `VersionNotSupported` → 400.

- **gRPC error status codes corrected for all 9 A2A error types** — Per Section
  5.4: `UnsupportedOperation`/`VersionNotSupported` → `UNIMPLEMENTED`,
  `ContentTypeNotSupported` → `INVALID_ARGUMENT`,
  `ExtendedAgentCardNotConfigured`/`ExtensionSupportRequired` →
  `FAILED_PRECONDITION`.

- **Blocking SendMessage now returns on interrupted states** — Per Section 3.2.2,
  `return_immediately=false` operations now correctly return when the task
  reaches `INPUT_REQUIRED` or `AUTH_REQUIRED`, not just terminal states.

- **`ListTasks` `includeArtifacts` parameter now applied** — Per Section 3.1.4,
  when `includeArtifacts` is false (the default), artifacts are omitted entirely
  from task responses.

### Added

- **`ErrorCode::a2a_reason()`** — Returns the UPPER_SNAKE_CASE reason string
  for `google.rpc.ErrorInfo`.
- **`ErrorCode::http_status()`** — Returns the correct HTTP status code per
  Section 5.4.
- **`ErrorCode::grpc_status()`** — Returns the correct gRPC status string per
  Section 5.4.
- **`A2aError::error_info_data()`** — Builds the `google.rpc.ErrorInfo` data
  array for error responses.
- **`TaskState::is_interrupted()`** — Returns true for `InputRequired` and
  `AuthRequired` states.
- **Missing error constructors** — `push_not_supported()`,
  `content_type_not_supported()`, `extension_support_required()`,
  `version_not_supported()`.

## [0.3.4] — Unpublished

> A standalone 0.3.4 release was never tagged or published to crates.io;
> the changes below first shipped as part of [0.4.0] - 2026-03-31.

### Fixed

- **`a2a-protocol-server`: SendMessage now rejects messages to tasks in terminal
  state** — Per A2A spec CORE-SEND-002, tasks in Completed, Failed, Canceled, or
  Rejected state cannot accept further messages. Previously, messages sent to
  terminal tasks were silently accepted and forwarded to the executor. Now returns
  `UnsupportedOperation` error. (Cross-SDK learning from a2a-java#741)

- **`a2a-protocol-server`: SendMessage with unknown taskId now returns
  TaskNotFound** — Per A2A spec section 3.4.2, when a client includes a `taskId`
  in a Message, it must reference an existing task. Previously, a client-provided
  `taskId` that didn't exist would create a new task with that ID. Now correctly
  returns `TaskNotFound` error. (Cross-SDK learning from a2a-java#766)

- **`a2a-protocol-server`: GetTask and ListTasks now apply `historyLength`
  parameter** — The `history_length` parameter was accepted in query/params but
  never actually used to truncate the message history in responses.
  `historyLength=0` now correctly returns no history, and positive values return
  only the N most recent messages. (Cross-SDK learning from a2a-python#573)

- **`a2a-protocol-server`: SubscribeToTask on terminal task now returns
  `UnsupportedOperation`** — Per A2A spec section 3.1.6, subscribing to a task
  in a terminal state should return `UnsupportedOperation`, not a generic
  internal error. (Cross-SDK learning from a2a-java#767)

### Added

- **`a2a-protocol-types`: `Artifact::validate()` method** — Validates that an
  artifact's `parts` list is non-empty per A2A spec requirements. Server-side
  event processing now validates artifacts before persisting them.
  (Cross-SDK learning from a2a-python#670)

- **`a2a-protocol-types`: `Part::text_content()` accessor** — Returns the text
  content of a text part, or `None` for non-text parts.

- **`a2a-protocol-server`: `ServerError::UnsupportedOperation` variant** — New
  error variant that maps to `ErrorCode::UnsupportedOperation` (-32004) for
  operations that are not valid for the current task state.

- **`a2a-protocol-server`: `SendMessageResult` now implements `Debug`** — Added
  `#[derive(Debug)]` to improve error messages in tests and logging.

- **`a2a-protocol-server`: SubscribeToTask emits Task snapshot as first event** —
  Per A2A spec, the first event in a `SubscribeToTask` stream must be a Task
  object representing the current state, preventing clients from missing state on
  reconnection. Added `EventQueueManager::subscribe_with_snapshot()`.
  (Cross-SDK learning from a2a-go#231, a2a-js#323)

- **`a2a-protocol-client`: `ClientBuilder::from_card()` preserves tenant** —
  The `tenant` field from `AgentInterface` was silently dropped when constructing
  a client from an `AgentCard`. Now preserved in `ClientConfig::tenant` and
  automatically applied to `SendMessage` requests. Added `with_tenant()` builder
  method for explicit configuration. (Cross-SDK learning from a2a-java#772)

- **`a2a-protocol-client`: `ClientConfig::tenant` field** — New optional field
  for default tenant in multi-tenancy scenarios.

- **`a2a-protocol-server`: A2A-Version header validation on incoming requests** —
  Both JSON-RPC and REST dispatchers now validate the `A2A-Version` header if
  present. Requests with incompatible major versions (not 1.x) are rejected with
  `VersionNotSupported` (-32009). (Cross-SDK learning from a2a-python#865)

- **`a2a-protocol-client`: JSON-RPC response ID validation** — The client now
  verifies that the response `id` matches the request `id` in JSON-RPC
  responses. Previously, any response was accepted regardless of ID, which could
  cause silent data corruption in pipelined scenarios.
  (Cross-SDK learning from a2a-js#318)

- **`a2a-protocol-server`: Artifact append now merges parts AND metadata** —
  When `TaskArtifactUpdateEvent` has `append=true`, the server now correctly
  merges parts into the existing artifact and deep-merges metadata (new keys
  override existing). Previously, appended artifacts were pushed as separate
  entries, losing the merge semantics. Both sync and background event processors
  are fixed. (Cross-SDK learning from a2a-python#735, a2a-java#615)

- **`a2a-protocol-types`: `TaskListResponse` fields are now required per spec** —
  `next_page_token` (`String`), `page_size` (`u32`), and `total_size` (`u32`)
  are now always present on the wire (not `Option`), matching the proto
  definition. Empty `next_page_token` means no more pages. All task store
  implementations now populate these fields.

- **`a2a-protocol-server`: `SendStreamingMessage` emits Task snapshot as first
  event** — Per A2A spec section 3.1.2, the first event in any streaming response
  MUST be a `Task` object. Previously only `SubscribeToTask` did this.

- **`a2a-protocol-server`: `GetExtendedAgentCard` capability check** — Per spec
  section 3.1.11, returns `UnsupportedOperationError` when
  `capabilities.extended_agent_card` is false/absent, and
  `ExtendedAgentCardNotConfiguredError` when capability is declared but no card
  is configured. Previously returned a generic internal error for both cases.

## [0.3.3] - 2026-03-30

### Fixed

- **`a2a-protocol-server`: `find_task_by_context` now prefers non-terminal tasks** —
  When multiple tasks shared the same `context_id` (e.g. after a task reached a
  terminal state and a new one was created), the lookup used `page_size=1` and
  returned whichever task the store ordered first, which could be the stale
  terminal task. Now fetches up to 10 candidates and returns the first
  non-terminal (active) task, falling back to the first terminal task only when
  no active task exists.

- **`a2a-protocol-server`: `context_locks` map no longer grows without bound** —
  The per-context mutex map (used to serialize concurrent `SendMessage` requests
  for the same `context_id`) never cleaned up stale entries, causing unbounded
  memory growth under sustained traffic with diverse context IDs. Stale locks
  (where no task holds a reference) are now pruned when the map exceeds the
  configurable `max_context_locks` limit (default 10,000).

- **`a2a-protocol-server`: `PayloadTooLarge` now returns correct JSON-RPC error
  code** — Previously mapped to `InternalError` (-32603), now correctly returns
  `InvalidRequest` (-32600) since an oversized payload is a client error.

- **`a2a-protocol-server`: params-level `context_id` now validated** — The
  `context_id` field at the `MessageSendParams` level (which takes precedence
  over `message.context_id`) was not checked by `validate_id()`, allowing
  empty/whitespace-only or excessively long values to bypass validation.

- **`a2a-protocol-server`: `eviction_interval=0` no longer panics** — Setting
  `TaskStoreConfig::eviction_interval` to 0 caused a panic in
  `u64::is_multiple_of(0)`. Now treated as "disable periodic eviction" (only
  capacity-based eviction triggers).

- **`a2a-protocol-server`: push config `list()` now returns deterministic order** —
  The in-memory push config store iterated over a `HashMap`, producing
  non-deterministic ordering. Results are now sorted by `(task_id, config_id)`.

- **`a2a-protocol-server`: cancel task TOCTOU race narrowed** — `on_cancel_task`
  now re-reads the task immediately before writing `Canceled` state, preventing
  it from overwriting a concurrent terminal transition (e.g. `Completed`) that
  occurred between the initial check and the save.

- **`a2a-protocol-server`: `page_size` clamped at handler level** — `ListTasks`
  now clamps `page_size` to 1000 at the handler layer before passing to the
  store, preventing oversized allocations from untrusted client input.

- **`a2a-protocol-server`: tenant store no longer allocates on read** — Read-only
  operations (`get`, `list`, `count`, `delete`) on the tenant-aware in-memory
  store no longer create a new tenant partition, closing a DoS vector where
  read requests with unknown tenant IDs could exhaust `max_tenants`.

- **`a2a-protocol-server`: `from_pool()` schema now matches `with_migrations()`** —
  The `SqliteTaskStore::from_pool()` and `TenantSqliteTaskStore::from_pool()`
  constructors now create the `created_at` column and composite
  `(context_id, state)` index, matching the schema produced by
  `with_migrations()`.

- **`a2a-protocol-server`: JSON-RPC serialization errors no longer produce `null`
  results** — `success_response` and `success_response_bytes` now return proper
  JSON-RPC error responses instead of silently producing `null` result values
  when `serde_json::to_value` fails. The `internal_serialization_error` fallback
  now returns HTTP 200 per JSON-RPC spec.

- **`a2a-protocol-types`: `MessageRole` serializes as lowercase** — Now serializes
  as `"user"` / `"agent"` per the A2A v1.0 JSON wire format, instead of
  proto-style `"ROLE_USER"` / `"ROLE_AGENT"`. The proto-style values remain as
  deserialization aliases for backward compatibility.

- **Unused example dependencies removed** — Removed `rig-core` from `rig-agent`
  and `bytes` from `echo-agent` examples.

## [0.3.2] - 2026-03-30

### Fixed

- **`a2a-protocol-server`: task_id not reused for non-terminal continuations** —
  `on_send_message` unconditionally generated a new `task_id` even when the
  client sent a `task_id` matching an existing non-terminal task (e.g.
  `input-required`). This violated A2A spec §3.4.3 (multi-turn conversation
  patterns) and caused non-deterministic `invalid params` errors on subsequent
  messages due to duplicate tasks per `context_id`. The handler now reuses the
  client-provided `task_id` when it matches the stored task. (#66)

## [0.3.1] - 2026-03-21

### Security

- **`rustls-webpki` upgraded to 0.103.10** — Fixes RUSTSEC-2026-0049
  ([GHSA-pwjx-qhcg-rvj4](https://github.com/rustls/webpki/security/advisories/GHSA-pwjx-qhcg-rvj4)):
  when a certificate had more than one `distributionPoint`, only the first was
  matched against each CRL's `IssuingDistributionPoint`, causing subsequent
  distribution points to be silently ignored and valid CRLs to be skipped.

### Fixed (Benchmarks)

- **`data_volume/save` eviction interference** — The `bench_save_at_scale` and
  `bench_store_with_history` benchmarks accumulated tasks across all criterion
  warmup and measurement iterations (sharing one store instance), triggering
  O(n log n) eviction scans every 64 writes once the store exceeded 10K tasks.
  Disabled eviction with `TaskStoreConfig { max_capacity: None, task_ttl: None }`
  to measure pure insert performance. Results: ~580µs → ~1.5µs (400× improvement).
- **`protocol_type_serde` throughput misreporting** — Throughput was set once for
  `AgentCard` but applied to all subsequent `Task` and `Message` benchmarks,
  causing incorrect bytes/sec reporting. Now sets correct `Throughput::Bytes`
  before each type's benchmark.
- **`realistic_workloads` fixture allocation in hot loop** — `mixed_parts_message()`
  and `nested_metadata()` were constructed inside `b.iter()` closures, measuring
  fixture creation cost instead of the send operation. Moved to pre-construction
  outside the hot loop.
- **Concurrent benchmark params allocation in hot loop** — `send_params()` with
  `format!` was called inside `b.iter()` closures in `concurrent_agents`,
  `cross_language`, and `backpressure` benchmarks. Pre-allocated params `Vec`
  outside the hot loop to avoid measuring allocation overhead.
- **`fixtures::large_metadata_message` unnecessary String clone** — Changed from
  cloning a `String` then wrapping in `Value::String` per entry to cloning the
  pre-built `Value::String` directly, eliminating one allocation per metadata entry.

### Improved (Performance)

- **`TCP_NODELAY` on all sockets** — Enabled `TCP_NODELAY` on server accept
  sockets and all client `HttpConnector` instances (JSON-RPC, REST, TLS,
  discovery). Eliminates ~40ms Nagle/delayed-ACK latency that caused constant
  overhead on SSE streaming regardless of event count.
- **`InMemoryTaskStore` switched to `BTreeMap`** — Replaced `HashMap` with
  `BTreeMap<TaskId, TaskEntry>` (added `Ord` to `TaskId`). List queries now
  use `BTreeMap::range()` for O(page_size) cursor seek instead of O(n) full
  scan + O(m log m) sort + O(m) clone. Benchmarked improvements:
  1K tasks 346µs → 20µs (17×), 10K tasks 4.2ms → 27µs (153×),
  100K tasks 4.5ms → 27µs (164×).
- **Batch request clone removal** — JSON-RPC batch dispatch now takes ownership
  of the parsed `Value::Array` instead of cloning each item, eliminating one
  heap allocation per batch element.
- **Benchmark server `TCP_NODELAY`** — The in-process benchmark server was
  missing `TCP_NODELAY`, causing all streaming benchmarks to report ~44ms
  (Nagle delay) instead of actual SDK latency (~1.5ms). Benchmark streaming
  results now accurately reflect SDK overhead.

### Fixed (CI)

- **`memory_overhead` benchmark crash** — The benchmark encoded deterministic
  allocation counts as `Duration::from_nanos()`, producing identical samples
  that caused criterion's statistical analysis to panic on NaN. Now measures
  real wall-clock time and verifies allocation counts via assertions.

## [0.3.0] - 2026-03-19

### Fixed (CI / Release Pipeline)

- **Release workflow missing `protoc`** — The release workflow uses
  `--all-features` which enables the `grpc` feature, requiring `protoc` for
  proto compilation via `tonic-build`. Added `arduino/setup-protoc` (same
  action used in `ci.yml`) to all four release jobs that build crates: CI
  matrix, package, publish-dry-run, and publish.
- **Benchmark `NoopExecutor` invalid state transition** — The `NoopExecutor`
  used by the `minimal_overhead` benchmark attempted a direct
  `Submitted → Completed` transition, which the state machine rejects. Added
  the required intermediate `Working` state update.

### Improved (Performance)

- **`JsonRpcVersion` deserialization** — Replaced `String::deserialize` with a
  zero-allocation `visit_str` visitor, eliminating a heap allocation on every
  JSON-RPC envelope (2× per request/response cycle).
- **`SendMessageResponse` deserialization** — Removed unnecessary
  `Value::clone()` in the no-`role` branch. When `"role"` is absent the value
  must be a `Task`, so the fallback path was dead code with a wasted clone.
- **Metadata size validation** — Replaced `serde_json::to_string` (allocates a
  throwaway `String`) with a zero-allocation byte-counting writer via
  `serde_json::to_writer` for the metadata size check on every `SendMessage`.

### Fixed (Dogfooding — Pass 16)

- **`ListPushConfigs` response format mismatch** — Both REST and JSON-RPC
  dispatchers now correctly wrap push config list results in
  `ListPushConfigsResponse { configs, next_page_token }` instead of serializing
  a bare `Vec`. Previously, every `list_push_configs` call via REST or JSON-RPC
  failed with a deserialization error. The Axum adapter and gRPC service were
  already correct.
- **Push config URL validation ignores `allow_private_urls`** — The handler now
  consults the push sender's `allows_private_urls()` method before rejecting
  loopback/private webhook URLs at config creation time. Previously,
  `allow_private_urls()` on `HttpPushSender` only affected delivery-time
  validation, not config-creation validation, causing all private-URL configs
  to be rejected even in testing environments.
- **Background processor silent exit on store failure** — The streaming
  background event processor now logs distinct error messages when the task
  store read fails or returns `None`, instead of silently returning.

### Added (Dogfooding — Pass 16)

- **`PushSender::allows_private_urls()`** — New trait method (default: `false`)
  that lets the handler query whether the push sender allows private/loopback
  webhook URLs. `HttpPushSender` implements it based on its
  `allow_private_urls` field.

### Fixed (Dogfooding — Pass 12)

- **`truncate_body` UTF-8 panic** — Response body truncation for error messages
  now uses char-boundary-safe slicing instead of byte-offset slicing. Previously,
  non-ASCII error responses (common with international error messages) could panic
  when the truncation point fell inside a multi-byte UTF-8 character.
- **SSE parser line buffer OOM** — The SSE parser now caps `line_buf` growth at
  2× `max_event_size` to prevent a malicious server from causing OOM by sending a
  single very long line without newlines.
- **`get_extended_agent_card` ignoring interceptor params** — The
  `GetExtendedAgentCard` method now forwards interceptor-modified params instead
  of discarding them and sending an empty object.
- **REST path parameter injection** — Path parameters (task IDs, config IDs) are
  now percent-encoded before interpolation into REST URLs, preventing path
  traversal via IDs containing `/` or `..`.
- **Silent-pass tests** — `test_list_tasks_context_filter` now correctly fails on
  wrong task count; `test_stale_page_token` validates error messages.

### Known Limitations

The following issues were identified during deep analysis at the time of this
release. **Every one of them has since been fixed** — the historical text is
kept for the record, with resolution notes:

- **Broadcast channel lag** — *(Resolved: the background event processor now
  consumes a dedicated lossless mpsc persistence channel that is unaffected
  by SSE backpressure, and a lagging streaming consumer receives an explicit
  marked stream error instead of silent event loss — see 0.7.0.)* If an SSE
  consumer falls behind, the broadcast channel drops events for both the SSE
  reader and the background event processor. State transitions missed by the
  background processor are not persisted to the task store.
- **SSRF DNS rebinding** — *(Resolved: `HttpPushSender` resolves DNS and pins
  the validated address for the actual connection.)* `HttpPushSender`
  validates webhook URLs against private IP patterns but does not resolve
  DNS.
- **WebSocket message size** — *(Resolved: the WebSocket dispatcher enforces
  a configurable message/frame size cap at the protocol level.)* The
  WebSocket dispatcher does not enforce a message size limit.
- **SQL push config stores** — *(Resolved: the SQLite and Postgres push
  config stores enforce per-task and global bounds.)* Unlike
  `InMemoryPushConfigStore`, the SQLite and Postgres push config stores do
  not enforce per-task or global config limits.

### Fixed (v0.3.0 Hardening — Pass 11)

- **Retry jitter** — backoff now applies full jitter (0.5–1.0× randomization)
  using `std::hash::RandomState`, preventing thundering-herd retry storms when
  multiple clients experience the same transient failure. No `rand` dependency.
- **gRPC timeout retryability** — `tonic::Code::DeadlineExceeded` and `Cancelled`
  now map to `ClientError::Timeout` (retryable) instead of `ClientError::Protocol`
  (non-retryable). `Unavailable` maps to `HttpClient` (retryable). Fixes silent
  failure-to-retry when switching from REST to gRPC transport.
- **SSRF validation at config creation** — push webhook URLs are validated for
  private/loopback addresses when the config is created, not just at delivery
  time. Closes the window where malicious URLs could be stored.
- **Push delivery amplification cap** — total push delivery time per event is
  capped at 30 seconds in both sync and background processors, preventing DoS
  via 100 slow webhook endpoints (previously unbounded: up to 25 minutes).
- **`connection_timeout` validation** — `ClientBuilder::build()` now rejects
  `Duration::ZERO` for `connection_timeout`, matching existing validation for
  `request_timeout` and `stream_connect_timeout`.
- **Streaming interceptor lifecycle** — `stream_message()` and
  `subscribe_to_task()` now call `run_after()` with a synthetic 200 response
  after stream establishment. Previously only non-streaming methods called the
  after-hook, leaving interceptors without cleanup/logging opportunities.
- **gRPC per-request timeouts** — `execute_unary` and `execute_streaming` are
  now wrapped in `tokio::time::timeout()` for per-request enforcement, matching
  REST/JSON-RPC behavior. Also sets `tonic::Request::set_timeout()`.

### Changed (v0.3.0 Hardening — Pass 11)

- **Breaking:** `ClientBuilder::from_card()` now returns `ClientResult<Self>`
  instead of `Self`. Agent cards with no `supported_interfaces` return
  `ClientError::InvalidEndpoint`, matching server-side validation.
- **Breaking:** `CallContext` fields are now private. Use accessor methods:
  `method()`, `caller_identity()`, `extensions()`, `request_id()`,
  `http_headers()`. Prevents interceptors from mutating security-critical
  context mid-request.
- **Breaking:** `HandlerLimits` zero values (`max_id_length=0`,
  `max_metadata_size=0`, `push_delivery_timeout=0`) are now rejected at
  `RequestHandlerBuilder::build()` time instead of failing at runtime.
- `with_task_store_config()` now triggers `debug_assert!` if called after
  `with_task_store()`, catching the silent-ignore footgun in development.

### Fixed (v0.3.0 Hardening — Pass 10.5)

- **Client timeout enforcement** — `connection_timeout` is now applied to the
  underlying `HttpConnector` via `set_connect_timeout()`. Previously configured
  but never enforced, causing TCP connections to hang for the OS default (~2 min)
  when servers were unreachable. Response body collection is now wrapped in
  `tokio::time::timeout()` to prevent slow-body hangs.
- **Multi-tenant data leak** — `find_task_by_context()` now uses
  `TenantContext::current()` instead of hardcoded `tenant: None`, preventing
  cross-tenant context lookups in multi-tenant deployments.
- **Bug #38 race condition** — background event processor now subscribes to the
  broadcast channel BEFORE the executor is spawned, eliminating the window where
  fast-completing executors (<1ms) finish before the subscription is active.
- **SQLite production readiness** — all 4 SQLite stores now configure WAL
  journal mode, `busy_timeout=5000ms`, `synchronous=NORMAL`, and
  `foreign_keys=ON` via `SqliteConnectOptions::pragma()`. Default pool size
  increased from 4 to 8.
- **Background event processor error handling** — in-memory task state is now
  reverted when `task_store.save()` fails, preventing phantom state divergence
  between memory and persistence.
- **Sync/streaming behavioral consistency** — invalid state transitions in
  streaming mode now mark the task as `Failed` (matching sync-mode behavior)
  instead of being silently ignored.
- **Unbounded artifact accumulation** — added `max_artifacts_per_task` (default:
  1000) to `HandlerLimits`, enforced in both sync and streaming paths.
- **Cancellation token race** — cancellation token is now inserted BEFORE
  `task_store.save()`, eliminating the window where `CancelTask` silently fails.
- **Connection pool idle timeout** — configured 90-second `pool_idle_timeout`
  on all hyper clients to prevent idle connection accumulation.

### Added (v0.3.0 Hardening)

- `HandlerLimits::max_artifacts_per_task` — configurable limit (default 1000)
  preventing O(n²) serialization cost and unbounded memory growth.
- `HandlerLimits::with_max_artifacts_per_task()` builder method.
- `CallContext::method()`, `caller_identity()`, `extensions()`, `request_id()`,
  `http_headers()` read-only accessor methods.

### Added (Framework Integration)

- **Axum framework integration** (`axum` feature) — `A2aRouter` builds an idiomatic
  `axum::Router` that wraps the existing `RequestHandler`. All 11 A2A v1.0 REST
  methods are mapped, including SSE streaming. The router is composable with other
  Axum routes, middleware, and layers. Zero business logic duplication — delegates
  entirely to the existing handler. Feature-gated behind `axum` in both
  `a2a-protocol-server` and `a2a-protocol-sdk`. 9 integration tests.

### Added (Testing)

- **TCK wire format conformance tests** — 44 tests in
  `crates/a2a-types/tests/tck_wire_format.rs` validating wire format compatibility
  against the A2A v1.0 specification. Covers ProtoJSON SCREAMING_SNAKE_CASE for
  `TaskState` and `MessageRole`, `SecurityRequirement`/`StringList` wrapper format,
  `Part` type discriminator, all `SecurityScheme` variants, cross-SDK interop
  fixtures (Python, JS, Go payloads), JSON-RPC 2.0 envelope, error codes, and
  full round-trip serialization of complex objects.
- **Mutation testing** — adopted `cargo-mutants` as a required quality gate with
  zero surviving mutants across all library crates. Configuration in `mutants.toml`.
- **Mutation testing CI** — on-demand via `workflow_dispatch` in
  `.github/workflows/mutants.yml`. Surviving mutants fail the build.
  Nightly schedule and PR-gate triggers are currently disabled to save CI time.
- **ADR 0006** — documents the rationale for mutation testing as a required quality
  gate, including alternatives considered and consequences.
- **60+ new tests** to kill surviving mutants across all crates, covering:
  state machine transitions, serde round-trips, builder patterns, hash functions,
  HTTP date formatting, rate limiter arithmetic, Debug impls, Arc delegation,
  OTel instrument recording, cancellation tokens, and more.
- **Wave 2 inline unit tests** — added `#[cfg(test)]` modules directly to 9 critical
  `a2a-protocol-server` source files covering the full request pipeline:
  `handler/messaging` (22 tests: ID validation, empty parts, metadata size limits,
  happy path, `return_immediately`), `handler/event_processing` (16 tests: state
  transitions, artifact updates, push delivery, `collect_events`),
  `handler/push_config` (8 tests: push CRUD), `handler/lifecycle` (21 tests:
  get/list/cancel/resubscribe/agent card), `handler/mod` (11 tests: builder
  accessors, Debug), `dispatch/rest` (40 tests: path parsing, response helpers,
  error mapping), `dispatch/jsonrpc` (13 tests: header extraction, param parsing,
  batch handling), `dispatch/grpc` (12 tests: config builders, encode/decode,
  error-to-status mapping), `dispatch/websocket` (5 tests: param parsing, error
  display). Total workspace test count: **~1,630 passing tests** (~1,850 with all feature flags).

### Added (PostgreSQL Support)

- **PostgreSQL-backed stores** (`postgres` feature) — `PostgresTaskStore` and
  `PostgresPushConfigStore` provide persistent store implementations using `sqlx`
  with the PostgreSQL driver. Multi-tenant variants `TenantAwarePostgresTaskStore`
  and `TenantAwarePostgresPushConfigStore` partition by `tenant_id` column.
  `PgMigration` and `PgMigrationRunner` provide forward-only schema versioning.
  Feature-gated behind `postgres` in both `a2a-protocol-server` and
  `a2a-protocol-sdk`.

### Added (Beyond-Spec Enhancements)

- **OpenTelemetry metrics integration** (`otel` feature) — `OtelMetrics` implements the
  `Metrics` trait with native OTLP export via `opentelemetry-otlp`. Instruments: request
  counts, response counts, error counts (with error type label), request latency (seconds),
  queue depth, and HTTP connection pool stats (active, idle, created, closed). Use
  `init_otlp_pipeline()` to bootstrap the global meter provider.
- **Connection pool metrics** — `ConnectionPoolStats` struct and
  `Metrics::on_connection_pool_stats()` callback for monitoring active/idle connections,
  total connections created, and connections closed.
- **Hot-reload agent cards** — `HotReloadAgentCardHandler` wraps agent cards behind
  `Arc<RwLock<_>>` for runtime updates without restarts. Three reload strategies:
  `reload_from_file()` (on-demand), `spawn_poll_watcher()` (periodic polling,
  cross-platform), `spawn_signal_watcher()` (Unix SIGHUP).
- **Store migration tooling** (`sqlite` feature) — `Migration` and `MigrationRunner`
  provide forward-only schema versioning for `SqliteTaskStore`. Built-in migrations:
  v1 (initial schema with indexes), v2 (add `created_at` column), v3 (composite index
  on `context_id, state`). Tracks applied versions in a `schema_versions` table.
- **Per-tenant configuration** — `PerTenantConfig` and `TenantLimits` allow operators
  to set per-tenant overrides for max concurrent tasks, executor timeout, event queue
  capacity, max stored tasks, and rate limits, with fallback to defaults.
- **`TenantResolver` trait** — abstracts tenant identity extraction from requests.
  Built-in implementations: `HeaderTenantResolver` (default `x-tenant-id`),
  `BearerTokenTenantResolver` (with optional token-to-tenant mapping),
  `PathSegmentTenantResolver` (URL path segment by index).
- **Agent card signing E2E test** — `test_agent_card_signing` in the agent-team suite
  generates an ES256 key pair, signs a card, verifies the signature, and tests tamper
  detection (`#[cfg(feature = "signing")]`).

### Fixed (Pass 10 — Exhaustive Audit)

- **Bug #40: Event queue serialization error silently swallowed** —
  `InMemoryQueueWriter::write()` used `unwrap_or(0)` when measuring serialized
  event size via `CountingWriter`. If `serde_json::to_writer` failed during size
  measurement, the error was silently masked and the event was sent through the
  channel without validation. Now propagates the serialization error as
  `A2aError::internal("event serialization failed: ...")`.
- **Bug #41: Capacity eviction fails when insufficient terminal tasks** —
  `InMemoryTaskStore` capacity eviction only removed terminal (completed/failed)
  tasks. When the store exceeded `max_capacity` with mostly non-terminal tasks,
  eviction could not remove enough entries, leaving the store permanently over
  capacity. Now falls back to evicting the oldest non-terminal tasks as a last
  resort to guarantee the hard capacity limit is enforced.
- **Bug #42: Lagged event count not exposed in reader warning** — The broadcast
  channel `Lagged(n)` error discarded the count of dropped events (`_n`). Now
  includes the actual count in the `trace_warn!` message for production
  observability (`"event queue reader lagged, {n} events skipped"`).

### Added (Pass 10)

- **1 new unit test** — `capacity_eviction_falls_back_to_non_terminal_when_needed`
  verifies that the fallback eviction path correctly removes non-terminal tasks
  when there are insufficient terminal tasks to bring the store under capacity.
- **94 passing E2E tests** with all features enabled (websocket, grpc, axum,
  sqlite, signing, otel). All tests exercised in a single dogfood run.
- **Total workspace tests: 1,750+** passing across all crates.

### Fixed (Pass 8 — Deep Dogfood)

- **Bug #32: Timeout errors misclassified as Transport (CRITICAL)** — REST and
  JSON-RPC transports mapped `tokio::time::timeout` errors to
  `ClientError::Transport` instead of `ClientError::Timeout`. Since `Transport`
  is non-retryable, timeouts never triggered retry logic. Fixed in both
  `rest.rs` and `jsonrpc.rs`.
- **Bug #33: SSE parser O(n) dequeue** — `SseParser::next_frame()` used
  `Vec::remove(0)` which shifts all remaining elements. Replaced internal
  `Vec<Result<SseFrame, SseParseError>>` with `VecDeque` for O(1) `pop_front`.
- **Bug #34: SSE parser silent UTF-8 data loss** — Malformed UTF-8 lines were
  silently discarded, causing data loss when multi-byte sequences split across
  TCP chunks. Now uses `String::from_utf8_lossy()` to preserve data with
  replacement characters instead of dropping entire lines.
- **Bug #35: Double-encoded path traversal bypass** — `contains_path_traversal()`
  only decoded one level of percent-encoding, allowing `%252E%252E` to bypass
  the check. Now applies two decoding passes to catch double-encoded sequences.
- **Bug #36: gRPC stream errors lose error context** — `grpc_stream_reader_task`
  mapped gRPC stream errors to generic `ClientError::Transport` instead of using
  `grpc_code_to_error_code()`, losing protocol error codes. Fixed to use
  `ClientError::Protocol(A2aError)` with proper code mapping.

### Added

- **6 new regression tests** — timeout retryability (Bug #32), SSE VecDeque
  dequeue correctness (Bug #33), SSE lossy UTF-8 (Bug #34), double/single/raw
  path traversal (Bug #35), exhaustive retryable classification.
- **3 new E2E tests (76-78)** — timeout retryable verification, concurrent
  cancel stress test (10 parallel), stale page token graceful handling.
- **Total E2E tests: 82** (98 with optional gRPC+WebSocket+Axum+SQLite+signing+OTel).

### Fixed (Pass 9 — Scale Probing)

- **Bug #37: SSE parser unbounded error queue** — `SseParser` internal frame
  queue could grow without bound from malicious streams. Added `max_queued_frames`
  limit (default 4096) with `with_max_queued_frames()` builder method.
- **Bug #39: Retry backoff float overflow** — `cap_backoff()` could panic on
  `f64::INFINITY` or `NaN` from extreme multiplier values. Now checks for
  non-finite results before `Duration` conversion.
- **Bug #38: Background event processor race** (documented) — In streaming mode,
  the background processor subscribes after the executor starts, so fast executors
  may complete before the subscription is active. Documented as known limitation.

### Added (Pass 9)

- **10 new deep dogfood E2E tests (81-90)** — state transition ordering, executor
  error propagation, streaming completeness, oversized metadata rejection, artifact
  content correctness, GetTask history, rapid sequential throughput, cancel terminal
  task, agent card semantic validation, GetTask-after-stream sync.
- **6 new Axum + SQLite E2E tests (93-98)** — Axum send/stream/card discovery,
  SQLite task store lifecycle, SQLite push config CRUD, combined Axum+SQLite stack.

### Fixed (Pass 7 — Deep Dogfood)

- **Graceful shutdown executor hang** — `shutdown()` and `shutdown_with_timeout()`
  now bound `executor.on_shutdown()` with a timeout to prevent indefinite hangs
  if an executor blocks during cleanup.
- **Push notification body clone per retry** — `body_bytes.clone()` inside the
  retry loop now uses `Bytes` (reference-counted) instead of `Vec<u8>`, reducing
  allocations from O(n × retries) to O(n) for push delivery.
- **Webhook URL missing scheme validation** — `validate_webhook_url()` now
  explicitly requires `http://` or `https://` schemes, rejecting `ftp://`,
  `file://`, and schemeless URLs that previously bypassed SSRF validation.
- **Push config unbounded global growth (DoS)** — `InMemoryPushConfigStore`
  now enforces a configurable global limit (default 100,000) in addition to the
  existing per-task limit. Prevents memory exhaustion from attackers creating
  millions of tasks with configs.
- **gRPC error code mapping incomplete** — added mappings for `Unauthenticated`,
  `PermissionDenied`, `ResourceExhausted`, `DeadlineExceeded`, `Cancelled`, and
  `Unavailable` tonic status codes to A2A error codes.
- **BuildMonitor cancel race** — `cancel()` now checks if the task is already
  cancelled before emitting a `Canceled` status, preventing invalid terminal
  state transitions.
- **CodeAnalyzer missing cancellation re-check** — added cancellation check
  between artifact emissions to allow faster abort during analysis.
- **Webhook/JSON-RPC/REST accept loop break on error** — accept loops in all
  agent-team server functions now `continue` on transient accept errors instead
  of terminating the entire server.
- **Coordinator silent client build failure** — now logs warnings with agent
  name and URL when client construction fails, aiding debugging.

### Added

- **`InMemoryPushConfigStore::with_max_total_configs()`** — configures the
  global push config limit (default 100,000) to prevent DoS via unbounded
  config creation.
- **8 new SSRF validation tests** — webhook URL scheme rejection (ftp, file,
  schemeless), CGNAT range, unspecified IPv4, IPv6 unique-local, IPv6 link-local.
- **4 new E2E tests (72-75)** — push config global limit enforcement, webhook
  URL scheme validation, combined status+context_id ListTasks filter, and
  metrics callback verification.
- **Total E2E tests: 68** (73 with optional gRPC+WebSocket transports).

### Fixed (Pass 6)

- **gRPC agent-team placeholder URL** — the gRPC `CodeAnalyzer` still used
  `"http://placeholder"` in its agent card (same Bug #12 pattern). Fixed by
  adding `GrpcDispatcher::serve_with_listener()` and using the pre-bind pattern.
- **REST transport query string encoding** — `build_query_string()` did not
  percent-encode parameter values. Values containing `&`, `=`, or spaces
  would corrupt query strings. Added RFC 3986 percent-encoding.
- **WebSocket stream termination detection** — replaced fragile
  `text.contains("stream_complete")` with proper JSON-RPC frame deserialization
  that checks for terminal task states. Prevents false positives from payloads
  containing the word "stream_complete".
- **Background event processor silent data loss** — 5 `let _ = task_store.save(...)`
  call sites in the streaming background processor silently dropped store errors.
  Now logs failures via `trace_error!`.
- **Metadata size validation bypass** — `unwrap_or(0)` allowed unserializable
  metadata to bypass size limits. Now rejects with `InvalidParams` error.
- **`InMemoryCredentialsStore` lock poisoning** — changed from silent `.ok()?`
  to `.expect()` (fail-fast). Poisoned locks now surface immediately instead of
  masking failures with silent `None` returns.
- **Rate limiter TOCTOU race on window advance** — replaced non-atomic
  load-check-store sequence with a `compare_exchange` (CAS) loop. Two concurrent
  threads can no longer both reset the counter to 1 on window boundary, which
  previously allowed 2N requests through per window.
- **Rate limiter unbounded bucket growth** — added amortized stale-bucket cleanup
  (every 256 `check()` calls). Buckets from departed callers are now evicted when
  their window is more than one window old.
- **Clippy `is_multiple_of` lint** — replaced manual `count % N == 0` with
  `count.is_multiple_of(N)` throughout.

### Added

- **`GrpcDispatcher::serve_with_listener()`** — accepts a pre-bound
  `TcpListener` for the gRPC server, enabling the same pre-bind pattern used
  by HTTP dispatchers. Ensures agent cards contain correct URLs.
- **`encode_query_value()`** — internal URL encoding for REST transport query
  string parameters (RFC 3986 §2.3 unreserved character set).
- **`is_stream_terminal()`** — WebSocket transport now uses structured JSON
  parsing for stream completion detection, with 6 new unit tests.
- **Protocol version compatibility warning** — `ClientBuilder::from_card()` now
  emits a `tracing::warn!` when the agent card's major protocol version differs
  from the client's supported version (currently `1.x`).
- **21 new unit tests** — comprehensive coverage for credentials store
  (multi-session, multi-scheme, overwrite, debug security), auth interceptor
  (basic/custom schemes), client builder validation (zero timeouts, unknown
  bindings, empty interfaces, config propagation), server builder validation
  (zero executor timeout, empty agent card interfaces, full option chain),
  rate limiter concurrency (200 concurrent requests), and stale-bucket cleanup.

### Changed

- **WebSocket transport** (`websocket` feature flag) — `WebSocketDispatcher` for
  server-side WebSocket support via `tokio-tungstenite`. JSON-RPC 2.0 messages
  are exchanged as WebSocket text frames. Streaming methods send multiple frames
  followed by a `stream_complete` response. Client-side `WebSocketTransport`
  provides persistent connection reuse.
- **Multi-tenancy** — `TenantAwareInMemoryTaskStore` and
  `TenantAwareInMemoryPushConfigStore` provide full tenant isolation using
  `tokio::task_local!` via `TenantContext::scope()`. Each tenant gets an
  independent store instance. SQLite variants (`TenantAwareSqliteTaskStore`,
  `TenantAwareSqlitePushConfigStore`) partition by `tenant_id` column.
- **TLS/mTLS integration tests** — 7 tests covering client certificate
  validation, SNI hostname verification, unknown CA rejection, and mutual TLS
  with valid/invalid/rogue client certificates. Uses `rcgen` for test-time
  certificate generation and `tokio-rustls` for TLS server.
- **Memory and load stress tests** — 5 tests for sustained concurrent load
  (200 concurrent requests, 500 requests over 10 waves), task store eviction
  under load, concurrent multi-tenant isolation (10 tenants × 50 tasks), and
  rapid connect/disconnect cycles.
- **Agent-team dogfood tests 51-55** — WebSocket send message, WebSocket
  streaming, tenant isolation, tenant ID independence, and tenant count tracking.
  Total agent-team E2E tests: 55 (66 with coverage gap tests, 69 with gRPC).
- `tls::build_https_client_with_config()` made public for custom TLS scenarios.
- `serve()` and `serve_with_addr()` server startup helpers — reduces the ~25 lines
  of hyper boilerplate per agent to a single function call. Both `JsonRpcDispatcher`
  and `RestDispatcher` implement the new `Dispatcher` trait.
- `RetryPolicy` and `ClientBuilder::with_retry_policy()` — configurable automatic
  retry with exponential backoff for transient client errors (connection errors,
  timeouts, HTTP 429/502/503/504). Ships as a transparent `RetryTransport` wrapper.
- `ClientError::is_retryable()` — classifies errors as transient or permanent.
- `EventEmitter` upstreamed to `executor_helpers` module — reduces event emission
  from 7-line struct literals to one-liners (`emit.status(TaskState::Working).await?`).
  Previously lived only in the agent-team example.
- `CallContext::request_id` — first-class request/trace ID field, automatically
  populated from the `X-Request-ID` HTTP header when present.
- `Metrics::on_latency(method, duration)` callback — the #1 production metric.
  All handler methods now measure and report request latency.
- Blanket `impl Metrics for Arc<T>` — eliminates the `MetricsForward` wrapper
  pattern when sharing metrics across handlers.
- `CallContext::http_headers` field — interceptors can now inspect
  `Authorization`, `X-Request-Id`, and other HTTP headers for auth decisions.
- `HandlerLimits::push_delivery_timeout` — configurable per-webhook timeout
  (default 5s) prevents one slow webhook from blocking all subsequent deliveries.
- Background event processor for streaming mode — push notifications and task
  store updates now fire for every event regardless of consumer mode.
- `SqliteTaskStore` and `SqlitePushConfigStore` — persistent store reference
  implementations behind the `sqlite` feature flag, using `sqlx` for async
  SQLite access. Includes schema auto-creation, cursor-based pagination,
  and upsert support.
- `boxed_future` helper function and `agent_executor!` macro in
  `executor_helpers` module — reduces `AgentExecutor` boilerplate from
  5 lines to 1 line per method.
- Doc examples for `TaskStore` and `AgentExecutor` traits — `# Example`
  sections in rustdoc for crates.io users.
- Explicit `sqlite` feature gate in CI — clippy and test steps for the
  `sqlite` feature flag alongside existing feature-specific gates.
- `JsonRpcDispatcher` now serves agent cards at `GET /.well-known/agent.json`,
  matching the existing `RestDispatcher` behavior.
- `EventQueueManager::subscribe()` creates additional readers for an active
  task's event stream, enabling `SubscribeToTask` (resubscribe) when another
  SSE stream is already active.
- Agent-team example refactored from monolithic 2800-line `main.rs` into
  best-practice modular structure (25 files) with 50 E2E
  tests across 5 categories (basic, lifecycle, edge cases, stress, dogfood).
- Client `send_message()` and `stream_message()` now merge client-level config
  (`return_immediately`, `history_length`, `accepted_output_modes`) into
  request parameters automatically. Per-request values take precedence.
- Dogfooding documentation restructured into modular book sub-pages: bugs
  found, test coverage matrix, and open issues roadmap.
- `EventEmitter` helper in agent-team example — caches `task_id` +
  `context_id` from `RequestContext`, reducing 9-line event struct literals
  to 1-line calls. Proof-of-concept for upstream `executor_helpers` addition.
- 10 new dogfood regression tests (tests 41-50) covering agent card URL
  correctness, push config listing via JSON-RPC, event classification,
  resubscribe, multiple artifacts, concurrent streams, context filtering,
  file parts, and history length.
- `RateLimitInterceptor` and `RateLimitConfig` — built-in fixed-window
  per-caller rate limiting as a `ServerInterceptor`. Caller keys are derived
  from `CallContext::caller_identity`, `X-Forwarded-For`, or `"anonymous"`.
- `TaskStore::count()` method — returns the total number of stored tasks.
  Useful for metrics and capacity monitoring. Has a default implementation
  returning `0` for backward compatibility. Implemented for both
  `InMemoryTaskStore` and `SqliteTaskStore`.

### Improved

- `InMemoryQueueWriter::write()` no longer allocates a full `String` to
  measure serialized event size. Uses a zero-allocation `CountingWriter`
  that counts bytes via `serde_json::to_writer()` instead of `to_string()`.
- `InMemoryTaskStore::save()` no longer holds the write lock during O(n)
  eviction sweeps. Eviction is decoupled from the insert and runs in a
  separate lock acquisition, reducing write lock contention under high
  concurrency.

### Changed

- **Breaking:** `PartContent` now uses `#[serde(tag = "type")]` with variant
  renames (`"text"`, `"file"`, `"data"`) per A2A spec. The old `Raw` and `Url`
  variants were merged into `File` with a new `FileContent` struct. Wire format
  now requires `{"type": "text", "text": "..."}` instead of `{"text": "..."}`.
  Backward-compatible `Part::raw()` and `Part::url()` constructors are provided.
- **Breaking:** `RequestHandler` stores changed from `Box<dyn TaskStore>` /
  `Box<dyn PushConfigStore>` / `Box<dyn PushSender>` to `Arc<dyn ...>` for
  cloneability into background tasks. `RequestHandlerBuilder` methods updated
  accordingly; `with_task_store_arc()` added for sharing store instances.
- **Breaking:** All `RequestHandler::on_*` methods now accept an additional
  `headers: Option<&HashMap<String, String>>` parameter for HTTP header
  forwarding to interceptors. Pass `None` if headers are not available.
- `handler.rs` (1,357 lines) split into 8 top-level modules under
  `handler/`: `mod.rs`, `limits.rs`, `helpers.rs`, `messaging.rs`,
  `lifecycle/` (5 sub-modules), `push_config.rs`, `event_processing/`
  (2 sub-modules), `shutdown.rs`. No public API changes.
- **Breaking:** `EventQueueManager` internals redesigned from `mpsc` to
  `tokio::sync::broadcast` channels. This enables multiple concurrent
  subscribers per task. Slow readers receive `Lagged` notifications instead
  of blocking the writer. The public `EventQueueWriter` / `EventQueueReader`
  traits are unchanged.

### Fixed

- `SubscribeToTask` (resubscribe) now works when another SSE reader is already
  active for the same task. Previously, `mpsc` channels allowed only a single
  reader, so resubscription returned "no active event queue for task".
- `ClientBuilder::with_return_immediately(true)` now actually propagates to
  the server. Previously, the flag was stored in `ClientConfig` but never
  injected into `MessageSendParams.configuration`, so the server always
  waited for task completion.
- JSON-RPC `ListTaskPushNotificationConfigs` now correctly parses
  `ListPushConfigsParams` instead of `TaskIdParams`. The field name mismatch
  (`id` vs `task_id`) caused silent deserialization failure — push config
  listing via JSON-RPC was completely broken while REST worked.
- Agent-team example agent cards now contain correct URLs via pre-bind
  listener pattern. Previously, cards were built with `"http://placeholder"`
  before the server bound to a port.
- Agent-team webhook event classifier now checks correct field names
  (`statusUpdate`/`artifactUpdate` instead of `status`/`artifact`).

## [0.2.0] - 2026-03-15

### Added

- Initial implementation of the A2A (Agent-to-Agent) v1.0.0 protocol specification.
- Core protocol type definitions and serialization.
- HTTP server with streaming (SSE) and JSON-RPC 2.0 dual transport.
- Client library for interacting with A2A-compatible agents.
- `SECURITY.md` with coordinated disclosure policy.
- `GOVERNANCE.md` with project governance and contribution guidelines.
- Health check endpoints (`GET /health`, `GET /ready`) for liveness/readiness probes.
- Request body size limits (4 MiB) to prevent DoS via oversized payloads.
- Content-Type validation on both JSON-RPC and REST dispatchers.
- Path traversal protection on the REST dispatcher.
- `TaskStoreConfig` with configurable TTL and capacity for `InMemoryTaskStore`.
- `RequestHandlerBuilder::with_task_store_config()` for store configuration.
- `ServerError::PayloadTooLarge` variant for body size limit violations.
- Executor timeout support via `RequestHandlerBuilder::with_executor_timeout()` to kill hung executors.
- Per-request HTTP timeout for `HttpPushSender` (default 30s) via `HttpPushSender::with_timeout()`.
- `TaskState::can_transition_to()` for handler-level state machine validation.
- Cursor-based pagination for `ListTasks` via `TaskStoreConfig`.
- URL percent-decoding for REST dispatcher path parameters.
- BOM (byte order mark) handling in JSON request bodies.
- Comprehensive hardening, dispatch, handler, push sender, and client test suites (1,769 tests).
- `#[non_exhaustive]` on 9 protocol types (7 enums, 2 structs) for forward-compatible evolution.
- SSRF protection for push notification webhook URLs (rejects private/loopback addresses).
- HTTP header injection prevention for push notification credentials.
- SSE parser memory limits (16 MiB default) to prevent OOM from malicious streams.
- Streaming task cancellation via `AbortHandle` on `Drop`.
- CORS support via `CorsConfig` for browser-based A2A clients.
- Graceful shutdown via `RequestHandler::shutdown()`.
- Path traversal protection against percent-encoded bypass (`%2E%2E`).
- Query string length limits (4 KiB) for DoS protection.
- Cancellation token map size bounds with automatic stale token cleanup.
- Amortized task store eviction (every 64 writes instead of every write).
- `ClientError::Timeout` variant for distinct timeout errors.
- Separate `stream_connect_timeout` configuration for SSE connections.
- Server benchmarks for task store and event queue operations.
- Carg