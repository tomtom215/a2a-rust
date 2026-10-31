# Audit A: first-run developer experience and deployment artifacts

Tree: /home/user/a2a-rust at 8dc4331 (crates at 0.15.1). Nothing was built or run. Every
"verified by reading" claim quotes the file. Items marked *conjecture* rely on a third-party
behaviour (PostgreSQL, kubelet, jemalloc) that I did not run.

Severity scale: critical = first deployment fails for a common setup; high = outage or data
loss under ordinary production conditions; medium = surprising failure or clear deviation
from best practice; low = DX friction or doc inaccuracy.

---

## A1 - CRITICAL - The PostgreSQL store is built without TLS: managed Postgres refuses it, and `sslmode=prefer` silently sends credentials and task content in plaintext

- **Where:** `Cargo.toml:90-93`, `crates/a2a-protocol-server/Cargo.toml:50`, `Cargo.lock` (sqlx-core / sqlx-postgres), `examples/deploy-agent/deployment.yaml:118-127`
- **Evidence:**
  - `Cargo.toml:90`: `[workspace.dependencies.sqlx] version = "0.9" default-features = false features = ["runtime-tokio"]`
  - `crates/a2a-protocol-server/Cargo.toml:50`: `postgres = ["dep:sqlx", "sqlx/postgres", "sqlx/json"]`. No `tls-*` feature on sqlx anywhere: `grep -rn "sqlx/tls\|tls-rustls-ring\|runtime-tokio-rustls" --include=Cargo.toml .` finds nothing.
  - In `Cargo.lock`, `sqlx-core 0.9.0` and `sqlx-postgres 0.9.0` depend on neither `rustls` nor `native-tls`.
  - sqlx-postgres 0.9.0 `src/connection/tls.rs`: `PgSslMode::Prefer => { if !tls::available() { return Ok(Box::new(socket)); }` and `PgSslMode::Require | VerifyFull | VerifyCa => { tls::error_if_unavailable()?;`. sqlx-core `net/tls/mod.rs`: `"TLS upgrade required by connect options but SQLx was built without TLS support enabled"`.
  - The docs never mention this. `grep -rni "sslmode\|ssl_mode"` over the book and code finds nothing. `book/src/getting-started/installation.md` says only "`postgres` | PostgreSQL-backed task and push config stores via `sqlx`", next to "`tls-rustls` is on by default".
- **Failure:** A user puts an RDS, Cloud SQL, Azure Flexible Server, Neon or Supabase URL in the `deploy-agent-db` Secret.
  - With `?sslmode=require` or `verify-full` (what those providers document), the process exits at once with `Error: Tls(... built without TLS support ...)`. No feature flag fixes it.
  - Without `sslmode`, sqlx defaults to `prefer` and connects in plaintext. Servers that require TLS refuse the connection (`no pg_hba.conf entry ... no encryption`, or rds.force_ssl). Servers that accept it carry the password and every task's content unencrypted across the network.
  - The two-replica manifest can only run against Postgres, so this blocks the documented multi-replica deployment on most hosted databases.
- **Fix:**
  - Add a sqlx TLS feature (`tls-rustls-ring-webpki`, to match the crate's rustls and ring choice) under the server's `postgres` feature, or as a `postgres-tls` feature that the SDK's `tls-rustls` turns on.
  - Document `sslmode=verify-full` and `sslrootcert`.
  - Make deploy-agent refuse a non-loopback `DATABASE_URL` with no `sslmode`, or at least warn about it.
  - Add a CI test that connects with `sslmode=require`.
- **Status:** verified by reading (lockfile, feature tables, sqlx source in `~/.cargo/registry`).

## A2 - HIGH - Readiness runs `SELECT COUNT(*) FROM tasks` every 5 s, and nothing in deploy-agent bounds that table: readiness slows with age until every replica goes NotReady at once

- **Where:** `crates/a2a-protocol-server/src/handler/introspection.rs:84-88`, `store/postgres_store/store_impl.rs:717-721`, `examples/deploy-agent/src/main.rs:196-206`, `deployment.yaml:154-159`, `book/src/deployment/health-checks.md:51-53`
- **Evidence:**
  - `task_store_health` is `crate::rpc_span::quietly(self.task_store.count())`.
  - The PostgreSQL `count` is `sqlx::query_as("SELECT COUNT(*) FROM tasks")`.
  - The readiness probe sets `periodSeconds: 5` and no `timeoutSeconds`, so the default of 1 s applies.
  - deploy-agent never calls `purge_expired`. `postgres_store/mod.rs` says of it: "Nothing calls this for you. A persistent store keeps every task until an operator says otherwise".
  - The in-memory default is `max_capacity: Some(10_000), task_ttl: Some(3600 s)` (`task_store/config.rs:264-265`). Setting `DATABASE_URL` therefore also switches retention from 1 hour to forever, without saying so.
  - `health-checks.md` claims count() is "a cheap query". On PostgreSQL, `COUNT(*)` scans the whole table (or a whole index).
- **Failure:** The `tasks` table grows without bound. Each replica runs a full scan every 5 s (2 replicas means 24 scans a minute, competing with real traffic). Once the scan takes longer than 1 s (millions of rows, a cold cache, or a vacuum running; the threshold is *conjecture*), every replica fails readiness together, because they all ask the same database. The Service then has no endpoints and the outage is total, though the database is healthy. This happens late, with no code change, so it is hard to diagnose.
- **Fix:**
  - Make the store health check `SELECT 1`, or `SELECT 1 FROM tasks LIMIT 1` if it must prove the table exists, and keep `count()` for metrics.
  - Set `timeoutSeconds` explicitly in the manifest.
  - Have deploy-agent run `purge_expired` on an interval (or show a CronJob) and document the change in retention.
  - Correct the "cheap query" claim.
- **Status:** query and probe verified by reading. The outage threshold is conjecture.

## A3 - HIGH - The port is bound only after the database connects and the schema is applied, and the manifest has no startupProbe: a slow or blocked database causes a liveness restart loop that hides the real error

- **Where:** `examples/deploy-agent/src/main.rs:257-263`, `deployment.yaml:146-151`, `store/pg_schema.rs:50-60,86-93`, sqlx-core 0.9.0 `pool/options.rs:541-555`, `pool/inner.rs:336-396`
- **Evidence:**
  - `let (router, handler) = app(&config).await?;` (connect and DDL) runs before `TcpListener::bind(("0.0.0.0", config.port))`.
  - The liveness probe sets `initialDelaySeconds: 3`, `periodSeconds: 10`, and no `failureThreshold` (default 3). Failures at about 3, 13 and 23 s mean SIGKILL at about 23 s.
  - sqlx `connect` sets `deadline = Instant::now() + self.acquire_timeout` (30 s by default) and retries `ConnectionRefused`/timeouts until the deadline. Two pools connect in sequence (task store, then push store).
  - `pg_schema::lock` runs `SET LOCAL statement_timeout = 0` and then `pg_advisory_xact_lock`. `lock_timeout` is "left as the operator set it", which is unbounded by default.
- **Failure:**
  - The database starts after the pods (a fresh cluster, a Helm install in one step) or sits behind a slow network path. sqlx waits up to 30 s, but the kubelet kills the container at about 23 s. `kubectl logs` shows only the telemetry line, never `PoolTimedOut`, so the user sees CrashLoopBackOff with no cause.
  - The same happens when schema DDL waits on a lock held by a long transaction (see A11): the wait has no bound, so the pod is killed and restarted indefinitely.
- **Fix:** Add a `startupProbe` (for example `failureThreshold: 30`, `periodSeconds: 2`), or better, bind first and serve `/healthz` while initialising, with `/readyz` returning 503 until the stores are open. Wrap startup errors with context (`connecting to DATABASE_URL: ...`). Set a bounded `lock_timeout` for the schema transaction.
- **Status:** verified by reading. The kill timing follows from kubelet defaults.

## A4 - HIGH - The workspace release profile sets `panic = "abort"`, so one panic in an executor kills the whole replica and its in-flight streams, defeating the SDK's own `catch_unwind`

- **Where:** `Cargo.toml:99-104`, `crates/a2a-protocol-server/src/handler/messaging/execute.rs:147-163`, `examples/deploy-agent/Dockerfile:31`
- **Evidence:**
  - `[profile.release] panic = "abort"`.
  - The SDK catches executor panics: `match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run.as_mut().poll(cx)))`, and the spawned path checks `joined.is_ok()`.
  - The Dockerfile builds `cargo build --release --locked -p deploy-agent`, so the shipped image aborts on any panic.
  - Nothing in `book/src/deployment/**` or `building-agents/**` warns about this. The only mention is in `reference/pitfalls/async-and-lifecycle.md`, about poisoned locks.
- **Failure:** A user's agent code panics on one bad input (`unwrap`, an index out of bounds, a model returning something unexpected). Under `cargo run` (the dev profile) the SDK turns that into a failed task. In the container the process aborts: every stream and in-flight task on the pod dies, the pod restarts, and claims are held for the 15 s TTL. A client that retries the same message moves the crash to the next replica (cascade: *conjecture*).
- **Fix:** Build deploy-agent with a profile that unwinds (for example `[profile.release-unwind] inherits = "release"`, `panic = "unwind"`, and `--profile` in the Dockerfile), or document the trade-off on the deploy-agent and production pages.
- **Status:** verified by reading. The cascade is conjecture.

## A5 - MEDIUM - SIGTERM on an idle pod stops accepting connections at once, with no preStop delay and no readiness flip, so rollouts drop requests during endpoint propagation

- **Where:** `examples/deploy-agent/src/main.rs:283-296`, `crates/a2a-protocol-server/src/handler/shutdown/in_flight.rs:188-192`, `deployment.yaml` (no `lifecycle`)
- **Evidence:**
  - `finish_in_flight`: "an empty handler returns at once".
  - deploy-agent's shutdown future is `shutdown_signal().await; tasks.finish_in_flight(...)`, after which axum stops accepting.
  - `grep -n "lifecycle\|preStop"` finds nothing in `deployment.yaml` or `book/src/deployment`.
  - `/readyz` keeps answering 200 during shutdown.
- **Failure:** In a rolling update or a node drain, the pod gets SIGTERM while kube-proxy, the ingress controller or a service mesh still route to it for 1 to 10 s. With no task in flight, the listener closes in milliseconds, and new connections get RST or ECONNREFUSED. Clients see 502s or connection errors on every deploy.
- **Fix:** Add `lifecycle.preStop: {sleep: {seconds: 5}}` (Kubernetes 1.30+; use `exec sleep` on older clusters, but the Debian image has `sleep`). Or keep accepting for N seconds after SIGTERM while `/readyz` returns 503. Add the delay to the budget for `terminationGracePeriodSeconds`.
- **Status:** verified by reading.

## A6 - MEDIUM - axum's drain has no bound in deploy-agent, so `shutdown_with_timeout` (claim release, cleanup hook) may never run before SIGKILL

- **Where:** `examples/deploy-agent/src/main.rs:283-301`, `book/src/deployment/production/graceful-shutdown.md` ("set the three durations with ... `with_drain_timeout`" applies to the SDK's `Server` only)
- **Evidence:** `axum::serve(listener, router).with_graceful_shutdown(...).await?;` has no timeout around it. axum 0.8.9 `serve/mod.rs:396-410` calls `conn.as_mut().graceful_shutdown()` and then waits for the connection to finish.
- **Failure:** `finish_in_flight` cancels only executors running on *this* replica. A connection held open by work that is not one of those executors keeps axum draining until the kubelet sends SIGKILL at 45 s. Examples: a `SubscribeToTask` SSE stream following a task running on the other replica (the cross-replica path polls the event log), or a continuation waiting on another replica's claim. Then "drained cleanly" never prints, `handler.shutdown_with_timeout` (which releases turn claims) never runs, and those tasks are held for the claim TTL. *Conjecture* about which request kinds stay open.
- **Fix:** Wrap the serve future in `tokio::time::timeout(drain_budget, ...)`, or use `Server::serve_with_shutdown` with `with_drain_timeout`, and always run `shutdown_with_timeout` afterwards.
- **Status:** the missing bound is verified by reading. The hanging-stream scenario is conjecture.

## A7 - MEDIUM - "The smallest agent you can actually ship" skips most of the book's own production checklist: no header-read or idle timeout, no connection cap, no executor timeout, no stream or rate limits, no Production profile

- **Where:** `examples/deploy-agent/src/main.rs:173,283`, `book/src/deployment/production.md:19-35`, axum 0.8.9 `serve/mod.rs:391`, hyper 1.12.0 `server/conn/http1.rs:253,364`
- **Evidence:**
  - The checklist includes "`ServeConfig::with_max_connections` set (connections are unbounded by default); header-read (30 s) and idle (75 s) timeouts left on", "Executor timeout set", "Max concurrent streams limited", "Rate limiting enabled for public-facing agents", and "`Profile::Production`...".
  - deploy-agent uses `axum::serve`, which builds `Builder::new(TokioExecutor::new())` without `.timer(...)`. hyper says `header_read_timeout` "Requires a [`Timer`] ... to take effect", so the 30 s default does nothing.
  - The builder is just `RequestHandlerBuilder::new(DeployAgent).with_agent_card(card)`.
  - The README's "What this does not cover" lists only TLS, auth, the database and telemetry backends.
- **Failure:** A user copies the "shippable" example, exposes it through an Ingress as the docs suggest (AGENT_URL is "the URL callers will use"), and gets a server open to slowloris, with unlimited connections and streams. Their real (slow, LLM-backed) executor has no timeout.
- **Fix:** Either apply the checklist in deploy-agent (connection cap and timeouts through `Server`/`ServeConfig`, or tower layers, plus an executor timeout and stream limits), or list each omission in "What this does not cover" with a link.
- **Status:** verified by reading. The hyper timer semantics come from its doc comment.

## A8 - MEDIUM - The manifest is rejected by Pod Security Standards "restricted" (no `seccompProfile`), and it mounts the service-account token

- **Where:** `examples/deploy-agent/deployment.yaml:94-96,166-170`
- **Evidence:**
  - The pod `securityContext` has only `runAsNonRoot: true, runAsUser: 10001`.
  - The container `securityContext` has `allowPrivilegeEscalation: false, readOnlyRootFilesystem: true, capabilities.drop: ["ALL"]`.
  - `grep seccomp\|automount` finds nothing.
  - The Dockerfile comment says "a container that runs as root is refused outright by many cluster admission policies", so admission policy is the stated concern.
- **Failure:** `kubectl apply` into a namespace labelled `pod-security.kubernetes.io/enforce: restricted` (the default on many managed platforms and hardened clusters) gives "violates PodSecurity restricted: seccompProfile (pod or container must set securityContext.seccompProfile.type to RuntimeDefault or Localhost)". The Deployment is created but no pod ever appears, and the ReplicaSet events are the only clue.
- **Fix:** Add `seccompProfile: {type: RuntimeDefault}` at the pod level, `automountServiceAccountToken: false`, and optionally `runAsGroup`/`fsGroup`.
- **Status:** manifest verified by reading. The PSS rule is standard Kubernetes behaviour.

## A9 - MEDIUM - No PodDisruptionBudget, anti-affinity or topology spread for the two "HA" replicas

- **Where:** `examples/deploy-agent/deployment.yaml:76-170`
- **Evidence:** The manifest has `replicas: 2` and no `affinity`, `topologySpreadConstraints` or `PodDisruptionBudget`. The containers.md table justifies two replicas for sharing, not availability.
- **Failure:** The scheduler can place both pods on one node, which then fails or is drained. `kubectl drain` evicts both at once because no PDB stops it. A full outage plus 15 s claim takeovers, during routine node upgrades.
- **Fix:** Add a PDB (`minAvailable: 1`) and `topologySpreadConstraints` over `kubernetes.io/hostname` (and the zone) with `whenUnsatisfiable: ScheduleAnyway`. Make `strategy.rollingUpdate` explicit (`maxUnavailable: 0`, `maxSurge: 1`) so the claim behaviour during a rollout is deliberate rather than the defaults.
- **Status:** verified by reading.

## A10 - MEDIUM - Every replica runs DDL and `CREATE OR REPLACE FUNCTION` at every start, so the runtime database role must own the schema; least privilege is impossible

- **Where:** `store/postgres_store/mod.rs:98-101,174-211`, `push/postgres_config_store.rs:56-90`, `store/claim/upgrade.rs:106,183-213`
- **Evidence:**
  - `new` calls `from_pool`, which calls `pg_schema::apply` with `CREATE TABLE IF NOT EXISTS ...`, `CREATE INDEX IF NOT EXISTS ...`, `CHECK_FUNCTION_SQL`/`RECORD_FUNCTION_SQL`/`CLAIM_FUNCTION_SQL` (`CREATE OR REPLACE FUNCTION a2a_legacy_write_record() ...`) and `CREATE TRIGGER ...` (guarded).
  - No constructor opens without DDL. `with_migrations` runs the migration runner instead, which also does DDL.
- **Failure (conjecture about PostgreSQL privileges, standard behaviour):**
  - A security-conscious team creates a migration owner role and a runtime role with DML-only grants. The runtime replicas then fail at startup: `CREATE OR REPLACE FUNCTION` requires ownership, and `CREATE INDEX` requires owning the table.
  - On PostgreSQL 15+, `public` no longer grants CREATE to PUBLIC, so a non-owner app user fails on the first `CREATE TABLE`.
  - The deploy docs never say what grants are needed.
- **Fix:** Add a constructor that only verifies the schema version, with migrations as an explicit step (a Kubernetes Job or init container using `PgMigrationRunner`). Document the privileges each mode needs.
- **Status:** the startup DDL is verified by reading. The privilege failures are conjecture from PostgreSQL semantics.

## A11 - MEDIUM (conjecture) - `CREATE INDEX IF NOT EXISTS` at every replica start may take a SHARE lock on `tasks` and stall all writes behind any long transaction

- **Where:** `store/postgres_store/mod.rs:186-196`, `store/pg_schema.rs:62-66`
- **Evidence:**
  - The authors fixed exactly this class for `ALTER TABLE`: "Not `ADD COLUMN IF NOT EXISTS`, which takes an `ACCESS EXCLUSIVE` lock on the table even when the column exists ... every replica's start would block reads and queue behind long transactions".
  - The same transaction still runs `CREATE INDEX IF NOT EXISTS idx_tasks_context_id ON tasks(context_id)` and three other indexes, with `statement_timeout = 0`.
- **Failure:** PostgreSQL's `DefineIndex` appears to open the table with ShareLock before `index_create` finds the name and skips. If so, each pod start (rollout, crash restart, scale-up) queues a SHARE lock behind any open write transaction on `tasks`. While it waits, every new INSERT/UPDATE on `tasks` from every replica waits behind it, and the cluster's writes stall.
- **Fix:** Measure it in `pg_locks` as was done for ALTER. If confirmed, guard each index with a `pg_class`/`pg_indexes` existence check in a `DO` block, as was done for the column, and set `lock_timeout` on the schema transaction.
- **Status:** conjecture (PostgreSQL internals). The code path is verified.

## A12 - MEDIUM - `.dockerignore` excludes only `target`, so `COPY . .` puts `.env` files and `.git` into the build stages

- **Where:** `.dockerignore` (only `**/target`), `examples/deploy-agent/Dockerfile:24,30`, `.gitignore` (`.env`, `.env.*`)
- **Evidence:**
  - `.gitignore` names "Environment secrets `.env` `.env.*`", but `.dockerignore` has only `**/target`.
  - The Dockerfile runs `COPY . .` in both the planner and build stages.
  - `.git` is 16 MB.
- **Failure:**
  - A developer with `DATABASE_URL=...` or `OPENAI_API_KEY=...` in `.env` (the examples read `OPENAI_API_KEY`, `LANGFUSE_*`) builds the image. The secrets go into the build-stage layers: in the local cache, and in any registry cache exported with `--cache-to mode=max`, or a `--target build` image. The runtime stage does not copy them, so the leak is in the cache and CI, not in the final image.
  - Any change to `.git`, the book or the docs invalidates `COPY . .` in the build stage and forces the 221 s rebuild.
- **Fix:** Add `.git`, `.env`, `.env.*`, `book/book`, `**/*.pem`, `**/*.key`, `docs/`, `book/` and benchmark results to `.dockerignore`, or copy only `Cargo.toml`, `Cargo.lock`, `crates/`, `examples/deploy-agent/` and the workspace members the recipe needs.
- **Status:** verified by reading.

## A13 - MEDIUM - Following the top-level README's deploy commands fails twice (missing Secret, then `:latest` pull)

- **Where:** `README.md:261-275`
- **Evidence:** The README shows
  ```sh
  cargo run -p deploy-agent
  docker build -f examples/deploy-agent/Dockerfile -t deploy-agent .
  kubectl apply -f examples/deploy-agent/deployment.yaml
  ```
  with no mention of the `deploy-agent-db` Secret or of editing the image. `deployment.yaml` uses `secretKeyRef: name: deploy-agent-db` and `image: deploy-agent:latest` with no `imagePullPolicy`.
- **Failure:** A newcomer runs the three lines. The pods sit in `CreateContainerConfigError` (no Secret). After the Secret is created they move to `ErrImagePull`/`ImagePullBackOff`, because `:latest` means `Always` and the image exists only locally. The deploy-agent README and containers.md do describe these "three edits", but the front page (the funnel entry) presents the commands as runnable.
- **Fix:** Add the Secret command and a one-line warning to the README, or point to the README section. Consider shipping the manifest with `imagePullPolicy: IfNotPresent` and a non-`latest` tag.
- **Status:** verified by reading.

## A14 - MEDIUM - The generic `DATABASE_URL` env var plus generic table names (`tasks`, `push_configs`) in the `public` schema collide with an existing database or a developer's shell

- **Where:** `examples/deploy-agent/src/main.rs:99`, `store/postgres_store/mod.rs:176-196`, `push/postgres_config_store.rs:30-36,83-88`
- **Evidence:**
  - `std::env::var("DATABASE_URL")`.
  - `CREATE TABLE IF NOT EXISTS tasks (id TEXT PRIMARY KEY, context_id TEXT NOT NULL, ...)`.
  - No schema or prefix option exists (`grep with_schema|table_prefix|search_path` finds nothing).
- **Failure:**
  - `DATABASE_URL` is routinely exported for sqlx-cli, Diesel, Rails or Django. `cargo run -p deploy-agent` in such a shell silently connects to that application's database, creates A2A tables, functions and triggers in `public`, and stops warning about running in memory.
  - If the application already has a `tasks` table, `CREATE TABLE IF NOT EXISTS` skips it and the next statements fail with a confusing `column "context_id" does not exist`. They roll back, because everything runs in one transaction. Or, if the columns happen to match, the store adds `applied_seq` and triggers to the application's table (*conjecture*).
  - The same applies when an operator "reuses the existing Postgres".
- **Fix:** Use an A2A-specific variable (for example `A2A_DATABASE_URL`) in the examples. Support or document a dedicated schema (`?options=-c%20search_path%3Da2a`). Log the database host and name at startup.
- **Status:** the env var and DDL are verified by reading. The collision outcomes are conjecture.

## A15 - LOW - Pool sizes are fixed and doubled per replica, cannot be set through the environment, and readiness checks only one of the two pools

- **Where:** `store/postgres_store/pool.rs:29-33`, `push/postgres_config_store.rs:56-60`, `examples/deploy-agent/src/main.rs:177-181`
- **Evidence:** `.max_connections(10)` appears in both. deploy-agent calls `PostgresTaskStore::new(url)` and `PostgresPushConfigStore::new(url)`, so each replica can open 20 connections. `/readyz` checks only `task_store.count()`.
- **Failure:** Scaling to 5 to 10 replicas (or an HPA) exhausts `max_connections` on small managed instances (often 25 to 100). New pods then fail at startup with `too many clients`, which ties into A3. A broken push-store pool is invisible to readiness.
- **Fix:** Read a pool-size env var in deploy-agent, or share one pool built with `pool_options()` for the task store and give the push store a small one. Document the connections each replica needs.
- **Status:** verified by reading.

## A16 - LOW - `AGENT_URL` is not validated, defaults to `localhost` behind a `0.0.0.0` bind, and the manifest hardcodes the `default` namespace

- **Where:** `examples/deploy-agent/src/main.rs:107-119`, `deployment.yaml:109-110`
- **Evidence:**
  - `let public_url = agent_url.unwrap_or_else(|| format!("http://localhost:{port}"));` performs no scheme, host or trailing-slash check. `AgentCard::validate` rejects only an empty URL (`types/src/agent_card/mod.rs:136`).
  - `PORT=0` is accepted, binding a random port while advertising `:0`.
  - The manifest sets `value: "http://deploy-agent.default.svc.cluster.local"`.
- **Failure:** `AGENT_URL=agent.example.com` (no scheme) or a URL with a path typo publishes a card no client can use. The pod starts and is Ready, and only clients fail. `kubectl apply -n agents` advertises the wrong namespace. With AGENT_URL unset in a container the card says localhost, and only the log line hints at it.
- **Fix:** Parse `AGENT_URL` as an absolute http(s) URL and reject anything else. Warn when it is a loopback host while bound to `0.0.0.0`. Reject `PORT=0`. In the manifest, use a downward-API namespace or a placeholder that cannot be mistaken for a working value.
- **Status:** verified by reading.

## A17 - LOW - deploy-agent's rustdoc "Run it" curl omits `A2A-Version` and gets `VERSION_NOT_SUPPORTED`

- **Where:** `examples/deploy-agent/src/main.rs:33-35` against `README.md:32-35`, `crates/a2a-protocol-server/src/dispatch/version.rs:107-115`, `dispatch/axum_adapter/validate.rs:63-64`
- **Evidence:** The rustdoc example is `curl -X POST localhost:8080/message:send -H 'content-type: application/json' -d '{...}'`, with no `A2A-Version` header. An absent header with `require` set is refused with "A2A version '0.3' is not supported by this server". The README and book versions include the header. Every other curl I checked (`README.md`, quick-start, hello-agent, genai, rig) includes it.
- **Fix:** Add `-H 'A2A-Version: 1.0'` to the rustdoc example.
- **Status:** verified by reading.

## A18 - LOW - Startup output says nothing about the store; startup errors are bare `Debug` values; lifecycle lines bypass the log pipeline

- **Where:** `examples/deploy-agent/src/main.rs:255-273,289-309`
- **Evidence:**
  - With `DATABASE_URL` set, the only lines are the telemetry line and `deploy-agent listening on 0.0.0.0:{} (advertising {})`. Nothing says "PostgreSQL store at host/db" or "schema ready".
  - `main` returns `Box<dyn Error>`, so a failure prints as `Error: PoolTimedOut` or `Error: Tls(...)` with no context about which variable or step failed.
  - "SIGTERM received, draining", "drained cleanly" and the not-graceful report use `println!`/`eprintln!` rather than `tracing`, so with OTLP logs on they never reach Loki.
  - `OTEL_SDK_DISABLED=true` installs no subscriber at all (`telemetry/mod.rs:212`), which also hides the SDK's warnings.
- **Fix:** Log the store kind and database host at startup, wrap startup errors with context, and emit lifecycle events through `tracing`.
- **Status:** verified by reading.

## A19 - LOW - Dockerfile details: no HEALTHCHECK, unpinned apt package, an inaccurate distroless rationale, and a slow rebuild

- **Where:** `examples/deploy-agent/Dockerfile:33-43`
- **Evidence:**
  - "Debian slim rather than distroless or scratch: the binary links glibc." But `gcr.io/distroless/cc-debian12` ships glibc and libgcc, which is enough for this binary (no OpenSSL; rustls; `ca-certificates` "is for tools run inside the container, not for the agent").
  - `apt-get install -y --no-install-recommends ca-certificates` is unversioned, so builds are not reproducible.
  - There is no `HEALTHCHECK` (and the image has no curl), so `docker run` and compose users get no health status.
  - The handoff records the image at 140 MB.
- **Fix:** Use `distroless/cc-debian12:nonroot` (pinned by digest), which removes the shell, apt and tools and probably roughly halves the size (*conjecture*). Or pin the package version. A HEALTHCHECK needs a binary, so consider a `deploy-agent --healthcheck` mode.
- **Status:** verified by reading. The size estimate is conjecture.

## A20 - LOW - The observability quick start puts Grafana on host port 3000, the port hello-agent hard-codes

- **Where:** `deploy/observability/docker-compose.yml:87-88`, `book/src/deployment/observability/quick-start.md:7,19,26-33`, `examples/hello-agent/src/main.rs:80,88`
- **Evidence:** Compose publishes `"3000:3000"` (Grafana, all interfaces). hello-agent runs `Server::bind("127.0.0.1:3000")`. The observability quick start uses hello-agent as its example agent and then says "open Grafana on `http://localhost:3000`".
- **Failure:** Running the stack and then hello-agent gives EADDRINUSE on Linux (a specific-address bind conflicts with Docker's wildcard bind). Starting hello-agent first makes `docker compose up` fail with "port is already allocated".
- **Fix:** Publish Grafana on another port (for example 3001), or make hello-agent read `PORT`.
- **Status:** verified by reading. The bind-conflict behaviour is standard Linux.

## A21 - LOW - a2a-cli silently replaces the URL the user typed with the card's URL, and gives no hint when that URL is unreachable

- **Where:** `tools/a2a-cli/src/connect.rs:106-122`, `tools/a2a-cli/src/error.rs:76-90`
- **Evidence:** `let (endpoint, binding) = (iface.url.clone(), iface.protocol_binding.clone());`. The `--binding` hint is printed only for `Discovery` (card-fetch) errors.
- **Failure:** `kubectl port-forward svc/deploy-agent 8080:80` followed by `a2a send http://localhost:8080 hi` fetches the card and then dials `http://deploy-agent.default.svc.cluster.local`. The user sees a bare DNS or connect error naming a host they never typed.
- **Fix:** On a transport error after discovery, print "the card at <url> advertises <iface.url>; pass --binding <b> to use <url> directly". Optionally log the chosen endpoint to stderr.
- **Status:** verified by reading.

## A22 - LOW - Configuration names differ across binaries

- **Where:** `examples/{echo,genai,rig,mcp,multi-lang-team}-*/src/main.rs` (`A2A_BIND_ADDR`), `examples/deploy-agent/src/main.rs:98-99` (`PORT`, `AGENT_URL`, `DATABASE_URL`), `examples/resilient-agent/src/scaling.rs:54` and `incident-response/src/hardening/durability.rs:242` (`A2A_TEST_POSTGRES_URL`), `tck/sut` (`SUT_HOST`, `SUT_ADVERTISE_URL`)
- **Failure:** A user who learned `A2A_BIND_ADDR=0.0.0.0:8080` from the LLM examples sets it on deploy-agent, which ignores it (8080 is the default, so the mistake also goes unnoticed). Four spellings exist for "where to listen and what to advertise", and the Postgres URL variable differs between examples.
- **Fix:** Pick one set (for example `A2A_BIND_ADDR` plus `A2A_PUBLIC_URL` plus `A2A_DATABASE_URL`) or accept both, and document it in one table.
- **Status:** verified by reading.

## A23 - LOW - deploy-agent cannot be copied out as a template, though the docs say to do exactly that

- **Where:** `book/src/deployment/containers.md:20-23`, `examples/deploy-agent/Cargo.toml:192-211`, `Dockerfile:9-10,24-31`
- **Evidence:**
  - containers.md says "Copy the two `deploy-agent` files into your own project and adapt them".
  - The Cargo.toml uses `edition.workspace = true`, `rust-version.workspace = true`, `license.workspace = true`, `tokio = { workspace = true, ... }` and `a2a-protocol-sdk = { path = "../../crates/a2a-protocol-sdk" ...}`.
  - The Dockerfile requires building from the a2a-rust repository root (`-p deploy-agent`).
  - `panic = "abort"` (A4) and the LTO settings come from the workspace and are lost when copied out.
- **Fix:** Ship a standalone template (a `cargo generate` template, or a `templates/agent` with crates.io dependencies and its own Dockerfile and profile), or list the edits needed.
- **Status:** verified by reading.

## A24 - INFO - There is no ready-to-run server binary; every deployment means writing and building a Rust crate

- **Evidence:**
  - `[[bin]]` targets are only `a2a-tck`, `a2a` (a client), fuzzers and `scripts/live/*`.
  - `tools/` holds `a2a-cli` and `a2a-codegen`.
  - containers.md: "There are no Helm charts, no Kustomize overlays and no published container images."
  - `packaging/README.md`: "drafts, not submitted ... No release can produce these binaries yet" (KNOWN; handoff "Publishing").
- **What a user must write:** an `AgentExecutor`, a card, store wiring, health routes, shutdown and the env config (that is, all of deploy-agent's `main.rs`), plus a Dockerfile that builds outside this workspace (A23). No generic proxy or bridge binary is configurable without Rust. `examples/mcp-bridge` is `publish = false` and a demo.
- **Suggestion:** A published `a2a-gateway`-style image (an OpenAI-compatible model behind `ToolLoopExecutor`, configured by env) would give users a first deployment with no Rust.

## A25 - LOW - No Ingress, TLS, NetworkPolicy or metrics-scrape objects, and `/metrics` needs a feature deploy-agent does not enable

- **Where:** `examples/deploy-agent/deployment.yaml`, `Cargo.toml:197`, `book/src/getting-started/installation.md` (`otel-prometheus`: "serve the metrics at `/metrics`")
- **Evidence:** deploy-agent enables `features = ["axum", "postgres"]`, not `otel-prometheus`. The manifest is only a Deployment and a Service. TLS and auth gaps are documented. NetworkPolicy and metrics are not mentioned in "What is not included".
- **Failure:** A cluster running Prometheus Operator has nothing to scrape without deploying an OTel Collector. With no NetworkPolicy, any pod in the cluster can drive the unauthenticated agent.
- **Fix:** Add optional commented examples (Ingress with TLS, a default-deny NetworkPolicy, a ServiceMonitor plus `otel-prometheus`), or list them under "What is not included".
- **Status:** verified by reading.

## A26 - LOW (conjecture) - With no CPU limit, the tokio worker count and jemalloc arena count follow the node's cores against a 256 Mi memory limit

- **Where:** `deployment.yaml:160-165`, `examples/deploy-agent/Cargo.toml:207-208`
- **Evidence:** The manifest sets `requests: cpu 50m, memory 64Mi`, `limits: memory 256Mi`, and no CPU limit. jemalloc is the global allocator.
- **Failure:** On a 64-core node, `available_parallelism` is 64 (with no CFS quota), giving 64 tokio workers and jemalloc's default of 4 x ncpu arenas. Retained dirty pages per arena could push RSS toward the limit and get the pod OOMKilled under bursty load, which a laptop test never shows.
- **Fix:** Set `TOKIO_WORKER_THREADS` or a CPU limit, and `_RJEM_MALLOC_CONF=narenas:N,background_thread:true`, or document both.
- **Status:** conjecture.

## A27 - LOW - The card's version is always `0.0.0`

- **Where:** `examples/deploy-agent/src/main.rs:141`, `examples/deploy-agent/Cargo.toml:188`
- **Evidence:** `version: env!("CARGO_PKG_VERSION").into()` and `version = "0.0.0"`.
- **Failure:** Every deployed copy advertises version 0.0.0, so clients and operators cannot tell rollouts apart by the card. This is minor, but it is the field users copy.
- **Fix:** Use an `AGENT_VERSION` env var or a build-time git describe.
- **Status:** verified by reading.

---

## Checked and found sound (no finding)

- Every top-level README, quick-start, hello-agent, genai and rig `curl` includes `A2A-Version: 1.0` and uses v1.0 names (`SendMessage`, `ROLE_USER`, `parts[{text}]`, `TASK_STATE_*`). deploy-agent's REST body is the one its own test asserts returns 200.
- Kubelet probes to `/healthz` and `/readyz` are not refused for a missing `A2A-Version` header: those routes are added after `into_router()` applied the validating layer, and the router's own `/health` and `/ready` are exempt (`validate.rs:50-56`).
- `readOnlyRootFilesystem: true` is safe: the only `temp_dir()` uses in server code are in `#[cfg(test)]` modules (`sqlite_pool.rs`, `sql_error.rs`).
- The Dockerfile pins base images by digest, uses `--locked`, runs as numeric UID 10001 matching `runAsUser`, uses an exec-form ENTRYPOINT, and its `rust:1.94` matches `rust-version = "1.94"`.
- Concurrent schema creation across replicas is serialized (`pg_schema` advisory lock), and `applied_seq` is added without `ACCESS EXCLUSIVE`.
- `a2a-observability_default` is the correct network name (`name: a2a-observability`).
