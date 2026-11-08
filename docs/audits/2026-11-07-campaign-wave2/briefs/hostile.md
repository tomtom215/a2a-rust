<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Campaign wave 2: the hostile-peer brief

Written 2026-11-08 for the two agents that attacked commit a746210f; read `common.md` first. Kept so the campaign can be run again.

## Area HOSTILE: hostile peers, hostile clients, auth and tenant bypass, replica chaos

Goal: find where the **server, the client or the stores** can be made to misbehave by a peer that is hostile,
broken or merely unlucky, and where a replica fleet on PostgreSQL loses, duplicates or corrupts work. This is
authorized testing of our own code on loopback and on a local database; send nothing to the internet and aim
nothing at a host that is not yours. Every "internal address" in an SSRF test is a listener you start on
127.0.0.1 or a name you map yourself.

### What exists (build on it)
* The security notes and prior sweeps: `book/src/deployment/security-testing.md`, `docs/audits/2026-10-29/F-security.md`
  and `D-resilience.md`, `docs/audits/2026-11-01-triage/`, `docs/readiness-bar.md`, `SECURITY.md`.
* The chaos scripts: `scripts/live/` (`README.md` says what each needs and measured), `scripts/live/replica`
  (a replica binary whose message text selects what it does), `partition_live.py`, `terminate_live.py`,
  `restart_live.py`, `upgrade_live.py`, `lock_live.py`, `burst_live.py`, `soak_matrix.sh`,
  `scripts/live/delegation/` (`pglocal.py` runs a local PostgreSQL, `handoff_live.py`), `scripts/live/steering/`.
  They assume Docker containers; adapt them to the local PostgreSQL (see the common rules) and record what you
  changed. CI-sized versions are in `crates/a2a-protocol-server/tests/` (`multi_replica`, `replica_processes`,
  `cross_replica_cancel`, `claim_lapse`, `transient_store_errors`, `auth_*`, `caller_isolation*`).
* Fuzz targets in `fuzz/fuzz_targets/` (cargo-fuzz needs a nightly toolchain: only `stable` and `1.94.0` are
  installed and nightly cannot be downloaded unless the network allows it; if it cannot run, drive the same
  inputs through a test or a script and say so).
* The binaries: `examples/deploy-agent` (production-shaped server), `examples/echo-agent`,
  `examples/hello-agent`, the `a2a` CLI crate.

### What is new and has had no adversary yet (start here)
1. `ListTasks` page tokens: all five task stores now answer `InvalidParams` for a token they could not have
   issued. While closing it a defect was found: a token with a separator and a nonsense timestamp reached
   PostgreSQL's `::timestamp` cast and came back as `-32603` carrying the database's own message. **Hunt for
   the same class**: any caller-controlled string or number that reaches SQL, a cast or a parse and produces
   an internal error, a 500, a panic, or text from the database or the OS. Try ids with NUL, newline,
   10 000 characters, lone surrogates (as JSON `\ud800`), emoji, RTL marks; `contextId`, `tenant`, `status`,
   `pageSize` (-1, 2^63, 2^64, 1e999, "NaN"), `statusTimestampAfter`, `historyLength`, push-config ids and URLs,
   idempotency keys, extension URIs, metadata keys, over every binding (JSON-RPC, HTTP+JSON, gRPC, WebSocket,
   SLIMRPC if present) and every store (in-memory, SQLite, PostgreSQL, the tenant-aware SQL stores, the encrypted
   store). Compare the five stores' answers to the same hostile input: any difference between them is a finding.
2. **Steering** (`with_steering`, extension `https://a2a-rust.com/extensions/steering/v1`): a message to a
   `working` task goes into a bounded inbox that the executor reads; across replicas it goes through the claim
   store. Attack: a caller who does not own the task, a tenant crossing, a flood past the inbox bound, a
   steering message that names a task in a terminal state, one racing the terminal transition, duplicate message
   ids, steering with the extension not declared, oversized parts, from a replica that does not hold the claim,
   while the claim lapses, after the holder is `kill -9`ed.
3. **F14**: a streaming send whose executor panics ends with the `failed` frame (and before, hung until the
   deadline); try executors that panic after N events, return `Ok` with no terminal state, block forever, drop
   the queue, or write after the terminal event, with 1 and with 500 concurrent subscribers; check the hold time.
4. **E-F15**: `AuthFailureThrottle::with_shared_counter` (a PostgreSQL fixed-window counter) and the SQL
   sweep that now scopes by window width. Attack: distinct client addresses rotating through `X-Forwarded-For`
   (the forwarded-header rules are in `rate_limit/forwarded.rs`), a replica that dies mid-window, clock skew
   between replicas (`faketime` or a database clock), the table upgraded from the 0.15 shape with live rows,
   counter overflow, a million distinct keys.
5. **E-F25 (b)**: `PostgresTaskStore::with_log_notifications` (`LISTEN`/`NOTIFY`): a listener that dies, a pooler
   in transaction mode (`LogListen::Pool` through a pooler is documented as wrong: confirm the failure is
   loud and bounded), notification payload abuse (can a task id or tenant name break the channel payload or
   inject a second statement?), 200 subscribers on one task, 5 000 appends a second.
6. **Parked-task expiry** (`RetentionPolicy::parked_max_age`, ADR 0056: delete, not cancel): does it ever delete a task
   that is live, claimed, or has a webhook delivery pending; does a client subscribed to it get a terminal answer.
7. **`with_blocking_over_stream`** (client): a hostile server that answers the stream with no terminal event, a
   terminal event for a different task, 10 000 events, then silence; the `GetTask` read-back convergence bound.

### Classes to attack across the whole server, client and CLI (record each as failed or found)
* **Transport.** Slowloris headers and bodies; `Content-Length` lies; chunked with huge chunks; a 2 GB body
  declared; deeply nested JSON (10^5 levels) and duplicate keys; invalid UTF-8; header floods and 100 KB
  header values; HTTP/2 rapid reset, continuation floods, `SETTINGS` abuse (the gRPC and h2 paths); WebSocket
  fragmentation, ping floods, 16 MB frames, binary frames, no close; SSE clients that never read (backpressure:
  is memory bounded per subscriber and per task); 10 000 idle connections; reconnect storms with
  `Last-Event-ID`. For each: limit hit with the documented status, memory and CPU bounded (measure RSS), the
  process survives, logs contain no `panicked`.
* **Auth.** JWT: `alg: none`, HS256 with the RSA public key as the secret, RS256/ES256 confusion, `kid`
  injection and path traversal in `kid`, JWKS served by a hostile host (redirects, huge bodies, slow, wrong
  content type, `jku`/`x5u`), expired and not-yet-valid, wrong `aud`/`iss`, clock skew, tokens in the query
  string, two `Authorization` headers, mixed-case scheme, bearer with trailing whitespace; static bearer and
  API-key comparison timing (record the method, not just a timing); mTLS identity from a proxy header
  (spoofed `X-Forwarded-Client-Cert`); auth-before-params (does a request with bad params and no credentials
  leak anything different from one with neither).
* **Isolation.** Tenant by path vs header vs token claim conflicts; `isolate_tasks_by_caller` on every
  operation that names a task (get, list, cancel, subscribe, push config CRUD, steer, resubscribe,
  extended card) compared byte for byte with "not found" (spec §3.3.2); list and count leaking other tenants'
  totals; push-config ids guessed across tenants; the claim, lease, halt and approval stores keyed by tenant;
  context ids shared between callers.
* **Outbound (SSRF and friends).** Push-notification webhook URLs to 127.0.0.1, `[::1]`, `0.0.0.0`,
  `169.254.169.254`, `::ffff:127.0.0.1`, decimal/octal/hex IPs, `localhost.` and names you map to private
  addresses (DNS rebinding: a name that resolves public then private; a resolver you control), redirects from a
  public-looking listener to a private one, `file://`, `gopher://`, userinfo tricks, ports, IPv6 zone ids;
  the client's card fetch and extended-card fetch, JWKS fetch, and `follow_task` against a peer that redirects
  across origins; proxy environment variables (`HTTP_PROXY`, loopback exemption `A2A_PROXY_LOOPBACK`).
  The guards are in `url_guard.rs`, `push/`, `net/`; try to bypass each.
* **Hostile servers against the client and the CLI.** Infinite stream; a card of 1 GB; a card that names another
  URL; a task with 10^6 artifacts; an event with a task id that is not the one asked about; SSE with no
  newline; gRPC trailers lying; a server that answers 200 to everything with garbage; compression bombs
  (gzip) on cards and events; slow-drip bodies; redirects loops. The client must stay bounded and classify.
* **Replica chaos on PostgreSQL** with the repository's replica binary behind a round-robin proxy you write:
  `kill -9` a replica mid-turn, `SIGSTOP` it past the claim time to live and `SIGCONT` it (a zombie that wakes
  and writes), pause PostgreSQL, drop connections (`pg_terminate_backend`), exhaust the pool, run migrations
  while serving, clock jumps, 50 replicas on one database, 10 000 tasks, a task cancelled while handed over.
  Invariants to check after every run, with a query or a log, not by eye: no task ran twice at once (claims),
  every task ends in one terminal state, no terminal state is overwritten, every terminal event reaches the
  log and the webhook outbox exactly once or at-least-once as documented, no orphan claims, counters (rate
  limit, lease) consistent.
* **Memory and cleanup.** After each class of attack above, is RSS back near baseline after the attack stops,
  do file descriptors and tasks return to baseline (`/proc/<pid>/fd`, `tokio` metrics if exposed), does a
  graceful shutdown with attackers connected finish inside its documented bound.

### Deliverables specific to this area
Scripts under `scripts/live/campaign/hostile/`, evidence under `docs/audits/2026-11-07-campaign-wave2/hostile/`.
Group the report by class. For each class say how many attacks you tried, how many the SDK refused as
documented, and list every deviation. When an attack needs a tool that is not installed (nightly, toxiproxy,
`h2spec`, `faketime`), say so and say what you used instead.
