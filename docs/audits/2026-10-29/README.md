<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Pre-release audit, 2026-10-29: the task list

Eight independent read-only reviews of `main` at `8dc4331` (the 0.15.1
preparation merged), each told to find every way the SDK fails a real
deployment, followed by live runs against a real PostgreSQL, llama.cpp
`llama-server` b11977 with Qwen3.5 and Gemma 4, and the observability stack.
The eight reports are in this directory, as written: **they are readings of
the code by a model, not measurements**, and a finding in them is a claim to
verify, not a fact. This file is the task list built from them; its
`Status` column is the only place a finding is marked as checked.

Status values:

- `open`: reported by reading; nobody has confirmed or refuted it yet.
- `confirmed (read)` / `confirmed (live)`: checked by opening the code, or by running it.
- `refuted`: checked and found wrong; the note says how (and what was changed anyway, if anything).
- `not a defect`: true as reported, but not the SDK's to fix; the note says why.
- `verified live`: a behaviour run live and found working, recorded so it is not re-tested blind.
- `fixed`: changed, with the regression test or live run named in the note.
- `fixed (part)`: partly changed; the note says what is still open.
- `deferred`: real, and left for after this release, with the reason.

Severity is the reviewer's; where a check changed it, the note says so.

## A. First-run experience and deployment ([report](A-dx-deploy.md))

| ID | Severity | Finding | Status | Note |
|---|---|---|---|---|
| A1 | CRITICAL | The PostgreSQL store is built without TLS: managed Postgres refuses it, and `sslmode=prefer` silently sends credentials and task content in plaintext | fixed | confirmed live (require/verify-full refused to start; prefer connected with pg_stat_ssl.ssl=f); sqlx tls-rustls-ring-webpki; tests/postgres_tls.rs fails without it |
| A2 | HIGH | Readiness runs `SELECT COUNT(*) FROM tasks` every 5 s, and nothing in deploy-agent bounds that table: readiness slows with age until every replica goes NotReady at once | fixed | TaskStore::health(): SELECT pg_is_in_recovery() / SELECT 1 |
| A3 | HIGH | The port is bound only after the database connects and the schema is applied, and the manifest has no startupProbe: a slow or blocked database causes a liveness restart loop that hides the real error | fixed | startupProbe; clear startup log and one-line error |
| A4 | HIGH | The workspace release profile sets `panic = "abort"`, so one panic in an executor kills the whole replica and its in-flight streams, defeating the SDK's own `catch_unwind` | fixed | release profile unwinds; live check pending |
| A5 | MEDIUM | SIGTERM on an idle pod stops accepting connections at once, with no preStop delay and no readiness flip, so rollouts drop requests during endpoint propagation | fixed | ServeConfig::with_drain_delay + readiness draining; tests/rest_readiness.rs |
| A6 | MEDIUM | axum's drain has no bound in deploy-agent, so `shutdown_with_timeout` (claim release, cleanup hook) may never run before SIGKILL | fixed | deploy-agent on Server: bounded drain |
| A7 | MEDIUM | "The smallest agent you can actually ship" skips most of the book's own production checklist: no header-read or idle timeout, no connection cap, no executor timeout, no stream or rate limits, no Production profile | fixed (part) | Server timeouts; no auth/rate limit/profile still (documented as not covered) |
| A8 | MEDIUM | The manifest is rejected by Pod Security Standards "restricted" (no `seccompProfile`), and it mounts the service-account token | fixed | seccompProfile, automountServiceAccountToken: false |
| A9 | MEDIUM | No PodDisruptionBudget, anti-affinity or topology spread for the two "HA" replicas | fixed | PDB, topology spread, explicit rolling update |
| A10 | MEDIUM | Every replica runs DDL and `CREATE OR REPLACE FUNCTION` at every start, so the runtime database role must own the schema; least privilege is impossible | open | |
| A11 | MEDIUM (conjecture) | `CREATE INDEX IF NOT EXISTS` at every replica start may take a SHARE lock on `tasks` and stall all writes behind any long transaction | open | |
| A12 | MEDIUM | `.dockerignore` excludes only `target`, so `COPY . .` puts `.env` files and `.git` into the build stages | fixed | .dockerignore: env files, keys, .git, built book |
| A13 | MEDIUM | Following the top-level README's deploy commands fails twice (missing Secret, then `:latest` pull) | open | |
| A14 | MEDIUM | The generic `DATABASE_URL` env var plus generic table names (`tasks`, `push_configs`) in the `public` schema collide with an existing database or a developer's shell | open | |
| A15 | LOW | Pool sizes are fixed and doubled per replica, cannot be set through the environment, and readiness checks only one of the two pools | open | |
| A16 | LOW | `AGENT_URL` is not validated, defaults to `localhost` behind a `0.0.0.0` bind, and the manifest hardcodes the `default` namespace | fixed | AGENT_URL and PORT=0 validated; namespace from the downward API |
| A17 | LOW | deploy-agent's rustdoc "Run it" curl omits `A2A-Version` and gets `VERSION_NOT_SUPPORTED` | fixed | rustdoc curl has the A2A-Version header |
| A18 | LOW | Startup output says nothing about the store; startup errors are bare `Debug` values; lifecycle lines bypass the log pipeline | fixed | tracing logs: store, redacted URL; Display errors |
| A19 | LOW | Dockerfile details: no HEALTHCHECK, unpinned apt package, an inaccurate distroless rationale, and a slow rebuild | open | |
| A20 | LOW | The observability quick start puts Grafana on host port 3000, the port hello-agent hard-codes | fixed | Grafana on host port 3001 |
| A21 | LOW | a2a-cli silently replaces the URL the user typed with the card's URL, and gives no hint when that URL is unreachable | open | |
| A22 | LOW | Configuration names differ across binaries | open | |
| A23 | LOW | deploy-agent cannot be copied out as a template, though the docs say to do exactly that | open | |
| A24 | INFO | There is no ready-to-run server binary; every deployment means writing and building a Rust crate | open | |
| A25 | LOW | No Ingress, TLS, NetworkPolicy or metrics-scrape objects, and `/metrics` needs a feature deploy-agent does not enable | open | |
| A26 | LOW (conjecture) | With no CPU limit, the tokio worker count and jemalloc arena count follow the node's cores against a 256 Mi memory limit | open | |
| A27 | LOW | The card's version is always `0.0.0` | open | |

## B. TLS, CAs, proxies and the network ([report](B-tls-proxy-network.md))

| ID | Severity | Finding | Status | Note |
|---|---|---|---|---|
| B-01 | critical | The default client bindings (JSON-RPC, HTTP+JSON) cannot trust a private CA or the system store, or present a client certificate | fixed | ClientBuilder::with_tls_config, tls_config_from_pem, SSL_CERT_FILE/SSL_CERT_DIR; tests/private_ca.rs |
| B-02 | critical | Only OTLP export honours `HTTP_PROXY`/`HTTPS_PROXY`/`NO_PROXY`; every other outbound path ignores them | fixed | confirmed live (main: client error (Connect) through a proxy-only path); HTTP bindings, discovery, JWKS, token provider; gRPC and WebSocket not yet |
| B-03 | high | The card resolver, client JWKS fetch and token provider have no TLS option at all, so private-CA users of WebSocket and gRPC still cannot bootstrap | open | |
| B-04 | medium | Server-side OIDC discovery cannot use a private CA, and JWKS over `http://` is accepted from any host | open | |
| B-05 | high | Connection, DNS and TLS failures surface only as `client error (Connect)`, losing the cause | fixed | confirmed live; error chain reported; tests/private_ca.rs asserts UnknownIssuer |
| B-06 | high | SSE responses lack `X-Accel-Buffering: no`, so events stall behind a default nginx | refuted (live), headers added | nginx 1.29 default config did not buffer the stream with or without X-Accel-Buffering (first event 0.01 s both); headers kept for proxies that buffer or compress |
| B-07 | medium | Client pool idle timeout (90 s) exceeds this server's idle timeout (75 s) and AWS ALB's (60 s), so stale connections get reused | fixed | client pool idle 50 s |
| B-08 | medium | A path prefix from a reverse proxy becomes a tenant on HTTP+JSON, and the card is served only at the root | open | |
| B-09 | medium | Trust in `X-Forwarded-For` is a hop count only; there is no trusted-proxy address check, `ip:port` entries make a new key per connection, and `Forwarded` is unsupported | open | |
| B-10 | medium | Long-lived streams are not kept alive through L7 idle timeouts on the WebSocket and gRPC bindings (off by default) | open | |
| B-11 | medium | A blocking `SendMessage` longer than the LB idle timeout returns 504; nothing keeps it alive and the docs do not warn | open | |
| B-12 | medium | The WebSocket client dials addresses one at a time with no happy eyeballs, so dual-stack hosts with broken IPv6 fail inside the connect timeout | open | |
| B-13 | medium | HTTP push delivery pins only the first resolved address, with no failover across A/AAAA records or retries | open | |
| B-14 | medium | No certificate hot-reload; the gRPC listener cannot rotate certificates without a restart | open | |
| B-15 | medium | The CLI has no TLS or proxy flags, so it cannot reach private-PKI, self-signed or proxy-only agents | open | |
| B-16 | medium | The `deploy-agent` template binds `0.0.0.0` only, so it is unreachable in IPv6-only clusters, and it serves without `ConnectInfo` | fixed | binds [::] with 0.0.0.0 fallback |
| B-17 | low | `TlsConfig` is dead configuration: `without_tls()` is a no-op and `Rustls` claims to use the system's configuration | fixed | without_tls refuses https at build; TlsConfig::Custom |
| B-18 | low | `tls_config_with_extra_roots` silently drops an unparsable CA, while OTLP fails hard | open | |
| B-19 | low | gRPC client through L7 or TLS-terminating proxies: ALPN `h2` is mandatory, plaintext means h2c prior-knowledge, and one connection pins one replica | open | |
| B-20 | low | Root-store semantics differ between transports | open | |
| B-21 | low | The SSE response sets `transfer-encoding: chunked` explicitly, which is illegal in HTTP/2 | fixed | as B-06 |
| B-22 | low | The SSRF classifier misses some special ranges | open | |
| B-23 | low | OTLP proxy handling differs from curl and Go in a few places, and some `NO_PROXY` forms are ignored | open | |
| B-24 | low | No service discovery (mDNS, DNS-SD or registry) | open | |
| B-25 | low | The WebSocket binding cannot share the HTTP port or router | open | |
| B-26 | low | mTLS behind a TLS-terminating proxy has no forwarded-client-certificate support | open | |
| B-27 | low | IPv6 edge cases | open | |
| B-28 | low | DNS: no caching, the connect timeout does not cover resolution, and Kubernetes `ndots:5` amplifies lookups; push delivery has a lookup bound but transports do not | open | |
| B-29 | low | Crypto provider and cipher constraints | open | |
| B-30 | low | The push sender's connector sets no TCP connect timeout or keepalive | open | |

## C. Observability ([report](C-observability.md))

| ID | Severity | Finding | Status | Note |
|---|---|---|---|---|
| C-01 | High | `RUST_LOG` cannot reduce what is exported; spans and span-events from every crate at every level, DEBUG and TRACE included, are exported | fixed | span level filter + max level hints; test fails with filter removed |
| C-02 | High | Model-controlled tool name is a metric label (unbounded cardinality, prompt-injectable) | fixed | unknown tool is _OTHER on metric and span name; tests |
| C-03 | High | Push-webhook host (client-supplied) is a metric label | fixed | first 64 webhook hosts named, rest _OTHER; test |
| C-04 | High | `A2AAgentSilent` pages critical on every restart or rollout, and auto-resolves for a truly dead agent | open | |
| C-05 | Medium | No metrics↔traces correlation: the SDK emits no exemplars, and the stack does not store or link them | open | |
| C-06 | Medium | No trace_id/span_id or JSON in stdout/stderr logs | open | |
| C-07 | Medium | `OTEL_SDK_DISABLED=true` also silences all stderr diagnostics | open | |
| C-08 | Medium | Unknown or unsupported enum values are fatal at startup (spec says warn and ignore) | open | |
| C-09 | Medium | Streams the peer abandons count as server errors, so the error-ratio alert fires on normal UI behaviour and on every deploy | open | |
| C-10 | Medium | W3C baggage is never propagated across A2A, and `OTEL_PROPAGATORS` has no effect on the A2A wire | open | |
| C-11 | Medium | Hot-path INFO logs are exported as OTLP log records by default (volume), and the client logs full endpoint URLs | fixed | confirmed live; per-request lines are DEBUG |
| C-12 | Medium | The legacy `init_otlp_pipeline` is still public and violates several rules the new path fixed | open | |
| C-13 | Medium | No process or runtime metrics and no telemetry self-metrics; exporter health is not observable except in logs | open | |
| C-14 | Medium | Alert coverage gaps and an alert threshold that falls inside a coarse bucket | open | |
| C-15 | Medium | The Collector itself is unobserved, and the base compose has no memory limit for `memory_limiter` | open | |
| C-16 | Low | The Prometheus pull endpoint binds `localhost` by default and runs a separate unauthenticated listener | open | |
| C-17 | Low | WebSocket per-call credential warning is not rate-limited (log spam) | open | |
| C-18 | Low | Grafana trace→logs link likely queries the log line, not Loki structured metadata | open | |
| C-19 | Low | Retention and persistence gaps in the deploy stack | open | |
| C-20 | Low | No attribute-length limit for opt-in content capture | open | |
| C-21 | Low | `OTEL_EXPORTER_OTLP_METRICS_TEMPORALITY_PREFERENCE=delta` silently breaks the shipped stack | open | |
| C-22 | Low | No Kubernetes / Helm equivalent | open | |
| C-23 | Low | Instruments are rebuilt on every measurement | open | |

## D. Resilience ([report](D-resilience.md))

| ID | Severity | Finding | Status | Note |
|---|---|---|---|---|
| D-01 | High | The default timeout cuts off a blocking `SendMessage` to a slow agent, and the task is orphaned | fixed | confirmed live (CLI exit 1 at 30 s, task kept running); send_timeout 600 s on all transports + CLI --send-timeout |
| D-02 | High | A hung webhook stalls the streaming persistence pipeline (push runs inline per event) | open | |
| D-03 | Medium | An executor panic gives no `Failed` push and no terminal SSE frame | open | |
| D-04 | Medium | A config-list failure drops the push silently | open | |
| D-05 | Medium | Push is at-most-a-few-attempts, not durable, and has no dedupe id or dead-webhook handling | open | |
| D-06 | Medium | Blocking-path push jobs: unbounded spawn and no per-task ordering | open | |
| D-07 | Medium | The server WebSocket read loop blocks on an untimed write, defeating the idle timeout | open | |
| D-08 | Medium | A half-open client WebSocket stays wedged for about 15 minutes | open | |
| D-09 | Low | WebSocket client connect errors are classed non-retryable | open | |
| D-10 | Low | `Retry-After` is clamped to `max_backoff` and not jittered | open | |
| D-11 | Low | No retry by default, and no automatic stream resume | open | |
| D-12 | Low | Client pool hygiene: idle race, no max-lifetime, no per-host cap, no HTTP/2 keepalive | open | |
| D-13 | Low | Push SSRF: `https` is not pinned, and some reserved ranges are missing | open | |
| D-14 | Medium | The SQLite migration runner leaves an exclusive transaction open on error or cancel | fixed | Transaction rolls back on error/drop; test |
| D-15 | Medium | A PostgreSQL failover is classed as permanent, and poisoned connections stay pooled | open | |
| D-16 | Low | A store outage during a continuation is reported as `Internal`, not `Unavailable` | open | |
| D-17 | Low | A fixed per-store PostgreSQL pool of 10 exhausts `max_connections` at modest scale | open | |
| D-18 | Low | In-memory capacity eviction can evict live or parked tasks | open | |
| D-19 | Low | Audit per-tenant locks are never evicted | open | |
| D-20 | Low | gRPC and WebSocket listeners have no liveness bound by default | open | |
| D-21 | Medium | A rolling deploy terminally cancels long tasks | open | |
| D-22 | Low | Push DNS lookups occupy blocking-pool threads past their timeout | open | |
| D-23 | Low | `tokio::spawn` from `Drop` impls | open | |
| D-24 | Info | Panic surface on request paths | open | |
| D-25 | Info | Areas checked and found sound | open | |

## E. Several replicas ([report](E-replicas.md))

| ID | Severity | Finding | Status | Note |
|---|---|---|---|---|
| E-F1 | High | a dead or stalled owner's task stays WORKING forever, and nothing reaps it | fixed | confirmed live (task WORKING 70 s after SIGKILL of its owner); reaper ends it failed/transient, webhook sent; 4 stores tested |
| E-F2 | High | rolling update and scale-down cancel in-flight turns, and deploy-agent keeps taking (and cancelling) new sends for about 15 s after SIGTERM | fixed | Sends refused once shutdown begins (HTTP 503 + Retry-After, gRPC UNAVAILABLE, RetryInfo everywhere), and the SDK client repeats them with a retry policy; a turn shutdown stops ends `failed`, class `transient`, not `canceled`. Tests on all four bindings, each failing with its part reverted; live on two probe replicas sharing PostgreSQL (2026-10-29): send answered during the 2 s drain delay, refused after it on the same connection, held turn ended `failed`/transient at 8.0 s, replica 2 served throughout |
| E-F3 | Medium-High | the owner holds the turn claim until its webhook deliveries finish, so continuations on other replicas are refused while the same send on the owner succeeds | open | |
| E-F4 | Medium (High for anyone using push) | the default push-config store stays in-memory when the task store is SQL; build() is silent and the book snippet omits it | fixed | build() refuses when the card declares push over a shared task store; book shared-stores page fixed |
| E-F5 | Medium-High | readiness runs a full COUNT(*), so all replicas go NotReady together, and a read-only database still reports ready | fixed | as A2; a standby is now not ready |
| E-F6 | Medium | pool size × replicas runs out of PostgreSQL's default `max_connections` | open | |
| E-F7 | Medium | claim renewal competes with all other traffic for a 10-connection pool, can wait 30 s for a connection against a 15 s TTL, and an executor cut off from the database keeps running unfenced | open | |
| E-F8 | Medium (conjecture) | after a takeover, an event position is reused, so subscribers that saw the old turn's live events silently miss the new turn's events | open | |
| E-F9 | Medium (conjecture) | a cross-replica cancel of an executor that exits quietly on its token never pushes CANCELED | open | |
| E-F10 | Medium | every replica re-runs CREATE OR REPLACE FUNCTION on every start, so the last replica to start sets the function bodies for the whole fleet (version skew) | open | |
| E-F11 | Medium (conjecture, verify with pg_locks) | replica startup DDL takes SHARE locks on hot tables with `statement_timeout=0`, and can stall all writers fleet-wide | open | |
| E-F12 | Medium | tenant `max_concurrent_tasks` is per process, so the fleet admits N× the limit | open | |
| E-F13 | Medium | `Profile::Production` accepts in-memory halt and lease stores, so the kill switch is per-replica in a "production" fleet | open | |
| E-F14 | Low | the per-task push-config cap uses a per-process lock, so it can be exceeded permanently across replicas | open | |
| E-F15 | Low-Medium | `AuthFailureThrottle` is per process with no shared option | open | |
| E-F16 | Low | rate-limit windows and sweeps use each replica's wall clock | open | |
| E-F17 | Low-Medium | lease deadlines use the replica's wall clock | open | |
| E-F18 | Medium | the audit chain is one optimistic append point for the whole fleet | open | |
| E-F19 | Low | the audit run registry is per process | open | |
| E-F20 | Low-Medium | a non-owner can synthesize the terminal frame without the real event's metadata | open | |
| E-F21 | Low | concurrent retention sweeps are not "disjoint", and report `complete` early | open | |
| E-F22 | Low | agent-card caching differs per replica | open | |
| E-F23 | Low | a send waiting for a remote claim holds the per-context lock for up to 5 s | open | |
| E-F24 | Low (conjecture) | a small idempotency replay window remains | open | |
| E-F25 | Low-Medium (partly KNOWN) | reattach polling is unbounded and shares the pool | open | |
| E-F26 | Low | SQLite claims use the wall clock, and nothing guards against network filesystems | open | |

## F. Security ([report](F-security.md))

| ID | Severity | Finding | Status | Note |
|---|---|---|---|---|
| SEC-01 | High | CSRF against loopback agents (no Content-Type, version in query) | fixed | confirmed live (SendMessage ran from a no-Content-Type POST); tests/browser_reachable_loopback.rs fails under the mutant |
| SEC-02 | High | No Host-header validation, so DNS rebinding reaches localhost agents | fixed | confirmed live (ListTasks answered Host: evil.example); loopback listeners check Host; single-label names allowed |
| SEC-03 | High; KNOWN | WebSocket accepts any `Origin` (cross-site WebSocket hijacking) | fixed | Default refuses every upgrade that carries an `Origin` unless listed (`with_allowed_origins`); no `Origin` (non-browser clients) accepted; `allow_any_origin()` opts out. Not "same host as `Host`", which DNS rebinding satisfies. Test fails with the old default; live in Chromium (2026-10-29): a page on another origin could not open the socket by default, could and read a GetTask answer when listed; a raw upgrade with no `Origin` got `101` |
| SEC-04 | Medium | Forced JWKS refetch with a forged `kid`, no cooldown | open | |
| SEC-05 | Medium | An empty API key fails open | fixed | CredentialSet::matching refuses an empty credential; unit test |
| SEC-06 | Medium; partly KNOWN | JWT defaults: no `iss`/`aud`, `from_oidc_issuer` does not bind the issuer, plaintext JWKS | fixed | `iss` and `aud` checked by default: a validator naming neither refuses every token (logged at construction) unless `allow_any_issuer`/`allow_any_audience`; `from_oidc_issuer` checks the document's `issuer` (§4.3) and binds it; JWKS and discovery URLs must be https or loopback http. Six mutations, each failing a test |
| SEC-07 | Medium | Internal error text returned verbatim to callers and persisted | open | |
| SEC-08 | Medium | OAuth client secret and tokens sent to `http://` endpoints and to card-controlled `tokenUrl` | open | |
| SEC-09 | Medium | a2a-cli sends `--header` credentials to card-chosen hosts and to the JWKS host | open | |
| SEC-10 | Low | Raw id logged at INFO before validation and before authentication | fixed (part) | per-request id lines moved to DEBUG; still logged at DEBUG before validation |
| SEC-11 | Low; KNOWN | `BearerTokenTenantResolver::new()`/`Default` use the raw secret as the tenant | open | |
| SEC-12 | Low | Gaps in the static SSRF host check | open | |
| SEC-13 | Low; KNOWN | HTTPS webhooks are not pinned | open | |
| SEC-14 | Low | Push fan-out amplification | open | |
| SEC-15 | Low, example | SSRF in the `agent-team` health monitor | open | |
| SEC-16 | Low, example | mcp-bridge forwards remote skill text as tool descriptions | open | |
| SEC-17 | Info | A custom `PushSender` gets no delivery-time SSRF guard | open | |
| SEC-18 | Info | Hardening notes | open | |

## G. API ergonomics and documentation drift ([report](G-api-docs.md))

| ID | Severity | Finding | Status | Note |
|---|---|---|---|---|
| G-01 | H | `without_tls()` is a no-op, and JSON-RPC/REST/discovery cannot trust a private CA | fixed | as B-01 |
| G-02 | H | `build()` accepts values that panic or hang at runtime | fixed | refuse 0 capacity at build, clamp channels, clamp/refuse max_connections |
| G-03 | H | A production refusal exits 0 silently; `ServeReport` is not `#[must_use]` | fixed | ServeReport #[must_use]; hello-agent fails on refused |
| G-04 | M | `build_grpc()` ignores card selection | open | |
| G-05 | M | Examples copy the hand-rolled accept loop the SDK calls a defect | open | |
| G-06 | M | Default features surprise in both directions | open | |
| G-07 | M | Constructors produce invalid cards; streaming is opt-in | open | |
| G-08 | M | `agent_executor!` bodies cannot reference `self` | open | |
| G-09 | M | Too many ways to serve, with inconsistent shapes | open | |
| G-10 | M | Wire types are exhaustive structs with all-public fields | open | |
| G-11 | M | Configuration errors masquerade as protocol or transport errors | open | |
| G-12 | M | `EventStream` is not a `futures::Stream` | open | |
| G-13 | M | Every message constructor requires a caller-chosen id | open | |
| G-14 | M | `Telemetry` ergonomics | open | |
| G-15 | L-M | Naming inconsistencies | open | |
| G-16 | L | `EventEmitter::artifact(id, parts, append: Option<bool>, last_chunk: Option<bool>)` | open | |
| G-17 | L | `Arc` boilerplate | open | |
| G-18 | L | `RequestContext.context_id: String` but `task_id: TaskId` | open | |
| G-19 | L | Order-dependent builder calls | open | |
| G-20 | L | Default timeouts disagree, and the timeout error is not actionable | open | |
| G-21 | L | An executor returning `Ok(())` without a terminal state | open | |
| G-22 | L | `SendMessageResponse` has no accessors | open | |
| G-23 | L | Assorted best-practice items | open | |
| G/D-01 | M | `without_tls()` "Disable TLS (plain HTTP only)" | open | |
| G/D-02 | M | The server README is stale on `serve` | open | |
| G/D-03 | L | `ServeConfig::with_max_connections` rustdoc predicts existing fields | open | |
| G/D-04 | M | `packaging/README.md` says no signing key is committed | open | |
| G/D-05 | M | `tools/a2a-cli/README.md` exit codes and `--header` | open | |
| G/D-06 | L | Book graceful-shutdown page on gRPC | open | |
| G/D-07 | L | Book Axum page models a misconfiguration | open | |
| G/D-08 | L | Server crate quick start is outdated | open | |
| G/D-09 | L | `is_retryable` doc contradicts its match | open | |
| G/D-10 | L | The hello-agent framing | open | |
| G/D-11 | L | Prelude rustdoc list is out of sync | open | |
| G/C-01 | M | Size and readability | open | |
| G/C-02 | M | 0.15.1, a patch, broke existing code | open | |
| G/C-03 | L | Doctest coverage: what is not compiled | open | |

## H. Swarm-scale capability ([report](H-swarm-scale.md))

| ID | Severity | Finding | Status | Note |
|---|---|---|---|---|
| H1 | S0 | Overload and halt look like permanent failures on JSON-RPC and WebSocket | fixed | Overloaded carries RetryInfo 1 s and Retry-After |
| H2 | S0 | The HTTP client cannot use a private CA or mTLS, and every `A2aClient` owns a separate pool | open | |
| H3 | S0 | Tasks orphaned by a crashed replica stay `working` forever, and their leases keep being renewed | open | |
| H4 | S0 | Delegation cancels its child on any stream blip, and has no durable journal or reattach (G2 still open) | open | |
| H5 | S1 (S0 at 10^6) | Control-plane cost is O(children) round trips and store operations per interval | open | |
| H6 | S1 | No retry budget, circuit breaker or client-side concurrency limit; retries amplify overload | open | |
| H7 | S1 | Readiness ignores saturation and halt; no load signal for balancers | open | |
| H8 | S1 | Card discovery cost at fleet cold start | open | |
| H9 | S1 | Admission control has refuse-only semantics: no queue, priority or weighted fairness | open | |
| H10 | S1 | No task lineage, so there is no tree view and no "halt this job" scope | open | |
| H11 | S1 | Metrics have no tenant, agent or job dimension and no control-plane instruments | open | |
| H12 | S2 | No client load balancing across replicas of a peer, no outlier ejection, no hedging | open | |
| H13 | S1 | Memory and payload scaling | open | |
| H14 | S2 | Push delivery is not durable | open | |
| H16 | S2 | The rate limiter fails closed above 10k distinct callers by default | open | |
| H17 | S2 | The tool loop has no shared inference concurrency limit or 429 backoff | open | |
| H18 | S2 | Ergonomics: wiring a child into the parent's context is manual | open | |
| H19 | S1 for real fleets | Swarm controls are Rust-only, so polyglot trees lose control at the first non-Rust hop | open | |

## L. Found by running it, in no report

Each found during the live runs of 2026-10-29 (PostgreSQL 17, llama.cpp
`llama-server` b11977 with Qwen3.5-2B Q4_K_M, three probe replicas behind
HAProxy 3.2, the observability stack), not by any of the eight readings.

| ID | Severity | Finding | Status | Note |
|---|---|---|---|---|
| L1 | high | `/ready` on `RestDispatcher` and `CombinedDispatcher` was the `/health` constant; it never asked the store, though documented as probing it | fixed | `RequestHandler::readiness`; `tests/rest_readiness.rs` fails with the constant restored |
| L2 | medium | `/ready` waited out the pool's 30 s acquire timeout while PostgreSQL was paused | fixed | `READINESS_TIMEOUT` 2 s, `503 timeout`; probe timeout 3 s in the manifest |
| L3 | medium | An application's own INFO lines were hidden by the default filter (`deploy-agent` never said it was serving) | fixed | default prints the app crate's INFO, that crate only (a `Targets` prefix would have admitted `a2a_protocol_*` for the CLI's `a2a`) |
| L4 | medium | `scripts/preflight.sh` refused to run since `ci.yml` moved to nextest | fixed | tier names the nextest and doctest gates |
| L5 | medium | The tool loop sends its whole answer as one artifact at the end; no token streaming, so a streamed LLM turn shows nothing until it is done | open | needs streaming from `ToolModel`; design work |
| L6 | low | `a2a` CLI has no `task subscribe` (SubscribeToTask): a cut stream cannot be resumed from the CLI | open | |
| L7 | info | Qwen3.5-2B reported every service healthy after calling only `list_services` | not a defect | model quality; the example's preamble could require `service_status` per service |
| L8 | medium | Cross-replica subscribe and cancel: worked (13 parts streamed from r1 to a subscriber on r2, `CANCELED` delivered, r1's executor stopped one tick later) | verified live | recorded so it is not re-tested blind |
| L9 | medium | `deploy-agent` drain: `/ready` 503 within 0.5 s of `SIGTERM`, sends answered for the 3 s delay, then a clean exit | verified live | |
| L10 | high | PostgreSQL TLS: confirmed and fixed (A1); `verify-full` refuses a CA certificate used as a leaf, as it should | verified live | |
| L11 | low | A short-lived client (the CLI, a batch job) never shows on the client dashboard: each process exports one sample under its own `service.instance.id`, and every panel is a per-series `rate()` that needs two (50 CLI runs: 50 one-sample series, no panel data) | open | a Collector `cumulativetodelta` plus a rule over `increase()`, or guidance; the server and GenAI dashboards had data on every panel (live, 2026-10-29) |
| L12 | info | 321 executor panics across three replicas in 200 s of traffic: every replica kept serving, each task `failed` | verified live | the probe agent is built outside the workspace with Cargo's default (unwinding) profile, so this shows the recovery A4 restores, not the workspace profile itself |


## M. Found re-reading this branch's own fixes

| ID | Severity | Finding | Status | Note |
|---|---|---|---|---|
| M1 | low | `ServeConfig::with_allowed_hosts` lowercases its entries but keeps a trailing dot, while the `Host` it is compared with has its trailing dot removed: an entry written `agent.example.com.` never matches | confirmed (read) | `serve/host_guard.rs` `allows`, `serve/graceful/config.rs` `with_allowed_hosts` |
| M2 | low | A `Host` header that is not visible ASCII is answered rather than refused by the loopback host check (`to_str` fails, no URI authority, so no check) | confirmed (read) | not a DNS-rebinding path as far as read: browsers send the name in ASCII (punycode); a robustness gap |
| M3 | low | The missing-`Content-Type` refusal keys on `Content-Length` or `Transfer-Encoding`; an HTTP/2 request may carry a body with neither | confirmed (read) | a browser speaks HTTP/2 only over TLS, so this reaches only an `https://` loopback agent; not run |
| M4 | low | The reaper awaits each ended task's webhook delivery in turn (`reaper/mod.rs` `fail`, bounded per task by `push_delivery_timeout`, 30 s by default), so a pass of 32 abandoned tasks behind dead webhooks can take up to 16 minutes before the next pass looks again | confirmed (read) | the tasks are already `failed` when their delivery starts; only the next pass and the webhooks are late. Not run |
