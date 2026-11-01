<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Decisions for the maintainer

Triage of 2026-11-01, `main` at `4a8243c`. 46 audit rows are `needs decision`, and the triage added
choices of its own (below). Nothing here is built. Each fork says what is being decided, what I would
do, and what that costs; the per-row options the triage agents wrote follow in the second half,
unedited except for layout.

**How to read the "my position" lines.** They are recommendations from reading the code, not
measurements. Where one rests on a run, it names the run (`live-results.md`). Work that needs no
decision (about 100 rows and 51 new findings) proceeds in the order in `PLAN.md` without waiting.

## Forks that gate work

### F1. What 0.16.0 breaks, because it is the last cheap window
Every crate already reads `0.16.0`, so `cargo-semver-checks` passes any break today. Once 0.16.0 is
published, the same break needs 0.17.0, and `STABILITY.md` §3.2 puts the next breaking minor a month
out at the earliest.

| Row | Break | Why it can only go now | My position |
|---|---|---|---|
| G-10 | `#[non_exhaustive]` on the wire structs (the triage counted 40 of 47 generated structs without it) | Any field the A2A spec adds is otherwise a breaking change, and `STABILITY.md` §4 already says they are `non_exhaustive` | **Correct §4 now, mark them in 0.17.0.** About 1,000 struct literals change and several types first need constructors; doing that unreviewed in the same release as the audit fixes is the kind of change that ships a regression |
| G-06 | `tls-rustls` as a server default feature | The server crate alone cannot reach any `https://` webhook or model; with `auth-jwt` it refuses every non-loopback token | **Do it in 0.16.0.** It fixes a first-run failure and removes a feature trap |
| G-18 | `RequestContext.context_id: String` becomes `ContextId` | Field type change | **Do it in 0.16.0**, listed by hand in Breaking Changes (semver-checks may not see a field-type change) |
| G-07 | `AgentCard::new` defaults `streaming: true` | Wire-visible default | **Do it in 0.16.0**; `hello-agent` cannot stream today |
| G-14 | What `Telemetry::init()` exports to | Behaviour default | Decide with C-08/C-10 (F3) |
| G-17 | `impl Into<Arc<_>>` on dispatcher constructors | Loses `const` on two constructors | **Defer**; additive helper instead |
| G-15 | Renames | Deprecated aliases in 0.16, removal in 0.17 | **Add aliases in 0.16** |
| NEW-g-api-4 | `sign_agent_card` was documented as removed in 0.16.0 (four places) and is still public | The promise is in `CHANGELOG.md`, the book and the rustdoc | **Remove it**, as promised, and list it in Breaking Changes and the upgrade guide |

Cadence: 0.15.0 was a breaking release in October, so a second one in November needs a
`**Cadence exception:**` line (`STABILITY.md`, `RELEASING.md`). You chose 0.16.0 already; this is only a reminder that the line is required.

### F2. Loopback and `HTTP_PROXY` (NEW-b-2, B-23)
Live (`live-results.md` LR-2): with `HTTP_PROXY` set and `NO_PROXY` unset, the client sends
`http://127.0.0.1:3000/...` to the proxy, and the error never mentions a proxy. The SDK allows plaintext
only to loopback, so a loopback token endpoint's client secret goes to the proxy in cleartext.
- Options: (a) exempt loopback (`localhost`, `127.0.0.0/8`, `::1`) by default, as Go's `net/http` does; (b) keep curl's behaviour and document it; (c) exempt loopback but log once at `warn`.
- **My position: (a)**, with `A2A_PROXY_LOOPBACK=1` to opt back in. A blanket `HTTP_PROXY` is the normal corporate laptop setup, and the safe reading of "plaintext only to loopback" is that loopback never leaves the host. Behaviour change; goes in Behaviour Changes.
- Also decide which `NO_PROXY` dialect to follow (B-23: `host:port` and `*.domain` entries never match; upper case wins over lower). **My position:** follow Go's, which is the documented superset.

### F3. OpenTelemetry environment errors (C-08, C-10, NEW-c-5)
The spec says an unrecognised enum value MUST warn and be ignored. This SDK fails `build()` and every binary exits 2, deliberately, because no subscriber existed to warn through; `Telemetry::notices()` now removes that reason.
- **My position: warn and fall back** for exporter/protocol/compression/boolean values, keep failing only for an unusable endpoint URL; accept `1`/`yes`/`on` as the spec's boolean rule requires. Opt-in W3C baggage propagation (C-10), off by default because baggage is a data channel to remote agents.

### F4. `Profile::Production` on a shared task store (E-F13, E-F12)
With an in-memory lease store, a parent's lease renewal that the balancer sends to another pod does nothing, so the pod running the child stops it while the parent is still renewing (about 1/8 per TTL on 2 pods, 1/2 on 5, by the triage agent's arithmetic). The kill switch with an in-memory halt store stops one pod.
- **My position:** when the task store hands out turn claims, Production refuses a process-local halt, lease, push-config or audit store, with an explicit opt-out. It amends ADR 0025. One-host SQLite deployments have no SQL lease store, so the opt-out matters. `max_concurrent_tasks` (E-F12) is documented per replica now and counted fleet-wide later.

### F5. Delegation on shutdown (H4 items 5 and 6, NEW-h-s0-2, NEW-h-designs-2/5)
A graceful redeploy of a supervisor fires every executor's cancel token, so the documented `wait(ctx.cancellation_token)` cancels every child and settles every journal record. The journal exists for exactly this case and loses it. On gRPC/WebSocket a long quiet child is cancelled after about six idle cuts.
- **My position:** reset the resume streak on any received event; when a journal is attached, shutdown **detaches** instead of cancelling; add stream keep-alive for gRPC/WebSocket (B-10); let a journaled delegation also be leased and budgeted (`DelegationOptions`, H18). Behaviour change.

### F6. Behaviour changes in patch releases (G/C-02)
The prepared 0.15.1 (never tagged or published: crates.io, GitHub releases and tags all end at 0.15.0) carries two runtime behaviour changes under `### Behaviour Changes`, and `check_release_tree.py` only objects to the literal heading `### Breaking Changes`, so the gate would have let a patch through.
- **My position:** a patch may not change what working code does at runtime without an `**Action required:**` line, and the gate checks it; and the release-notes fallback keeps `### Behaviour Changes`.

### F7. One HTTP pool (H2, ADR 0032)
Every `A2aClient` builds its own pool; there is no public way to share one.
- **My position:** document "share one `Arc<A2aClient>` per peer" now; build ADR 0032 when H6 (retry budget) or H8 (card cache) needs per-origin state, because that is when a pool earns its complexity.

### F8. A published container image (A13, A24)
Nothing publishes an image, so the front-page `kubectl apply` cannot work as written (no Secret, and `:latest` is pulled from Docker Hub).
- **My position:** fix the README and manifest now (create the Secret, `imagePullPolicy: IfNotPresent`, a pinned tag); publishing a signed image to GHCR from `release.yml` with provenance is your supply-chain call, because it adds a credential-bearing job to the release workflow.

### F9. Agent Skills (agentskills.io)
Verified from Anthropic's announcement (22 Oct 2026) and the AAIF page: the standard was donated to the Agentic AI Foundation under the Linux Foundation; 70+ platforms; maintained by individuals. The spec is small (a `SKILL.md` with `name` and `description` frontmatter, optional `scripts/`, `references/`, `assets/`; progressive disclosure). It has **no versioned releases yet** (a roadmap proposes them), and `allowed-tools` is marked experimental.
It is not the same thing as A2A's `AgentSkill` (a card entry advertising what an agent can do). The integration point is the tool loop.
- Options: (1) a feature-gated `skills` module: discover and validate (lenient, with diagnostics, as the client guide recommends), put the catalog in the tool loop's prompt, an `activate_skill` tool whose `name` is an enum of valid skills, path-confined resource reads, trust gating for project-scope skills, and optional advertisement of each skill as an `AgentSkill` on the card; (2) validation only (`a2a skills validate`); (3) do not adopt.
- **My position: (1) in 0.17.0, as its own ADR, without script execution** (scripts need a sandbox, and a writable skills directory is a prompt-injection channel, `docs/injection-containment.md`). Pin to a dated snapshot of the spec, since there is no version to pin. The guide's "run a skill in a subagent" pattern is exactly `Delegation`.

### F10. Messaging a running agent (teammate messages, live steering)
What the spec says (v1.0.1, §3.4 and §7.6): follow-up messages are defined for tasks in an interrupted state (`input-required`, `auth-required`). Nothing says what a server must do with a message sent to a task that is still **working**. This SDK refuses it (`UnsupportedOperation`, "already being processed", `handler/messaging/admission.rs`), and ledger row §7.6.1 is `PARTIAL` for the related case. So steering is neither required nor forbidden: it is room for an extension.
- Proposal (needs an ADR): an extension, negotiated through `A2A-Extensions` like idempotency, failure class and budget. A `SendMessage` naming a working task and carrying the extension is delivered to the running executor through a bounded per-task inbox (`ctx.inbox()`), is authorised and tenant-scoped exactly like a send, and is answered with the task's current state. Peers that do not send the extension keep today's refusal, so conformance is unchanged. Across replicas the message goes through the store to the replica holding the turn claim. Client side: `Delegation::steer(message)`; fan-out: `DelegationGroup::broadcast` (ADR 0033).
- **My position:** worth building after the audit PRs; I would not start it before the claim-liveness work (PR-B) because it rides on the same claim.

### F11. Changelog shape (G/C-01)
`CHANGELOG.md` is 805,661 bytes; `[Unreleased]` alone is 73,728 bytes in 109 bullets.
- **My position:** a `### Highlights / Action required` block of at most 15 lines at the top of every release (checked by `check_release_tree.py`), and move `### Internal` and lab measurements out to `docs/changelog-internal/`.

### F12. The swarm designs (H5 to H19)
Each needs an ADR; none is built (ADRs 0031 to 0034 are written, not implemented). Small additive pieces that can ship without the decision are listed per row. **My position on order:** H16 (rate-limiter overflow bucket), NEW-h-designs-1 (jitter on overload retries), NEW-h-designs-4 (a halt store that tolerates an unknown scope, before ADR 0034 adds one) and NEW-h-designs-7 (ready while refusing everything) are fixes, not designs, and go first. Then H6, H10, H18/ADR 0033, H11, H7; H5, H9, H13, H14, H17, H19 each as its own ADR.

### F13. Event positions after a takeover (E-F8) — added during the fix work, 2026-11-01
Positions are broadcast to live subscribers before the processor logs them, and a takeover numbers the next
turn from the log's end, so a subscriber that saw the old owner's last positions (or resumes with
`Last-Event-ID` at one) skips the new turn's first events as already delivered. The lease watchdog
(group B) removes the partition case: the old owner stops at four fifths of the claim's life and writes a
terminal event through its own queue before anyone can take over (a subscriber on it gets that event from the
queue; `claim_lapse.rs` checks the stored end, not a subscriber's frame). What remains is an owner killed (SIGKILL,
OOM) with broadcast-but-unlogged events, a window bounded by the persistence backlog. The reaper and the
close-out now number from the later of the log's end and the stored position; a continuation already did.
- Options: (a) the claim row carries the owner's broadcast high-water mark, renewed with the claim, and a
  takeover numbers past it (schema change in two stores); (b) a takeover jumps to the next multiple of 2^20
  (no schema change; ids jump, still increasing; the replay reports the hole, which is honest but visible to
  every client after every takeover); (c) accept the window and document it.
- **My position: (c) now, (b) if a client shows it.** Both (a) and (b) change what a client sees, and the
  window needs a kill during a store stall. Not built.

### F14. A stream whose executor panics ends without a terminal frame (D-03 d)
The panic now stores `Failed`, pushes it and logs it, but a `SendStreamingMessage` stream is fed from the
executor's queue, which the unwind closes without a terminal event, so the stream just ends. The client has
to `GetTask`.
- Options: (a) the stream reader, on a closed queue with no terminal and no interrupted state seen, waits
  (bounded, a few seconds) for the stored task to become terminal and emits that as the last frame; needs the
  reader to track "interrupted" (a stream legitimately ends there); (b) the turn's cleanup broadcasts a
  `Failed` event on the queue's broadcast side as it unwinds (synchronous; needs a position, so it touches
  F13's numbering); (c) leave it: EOF without a terminal is already how a client learns to resubscribe.
- **My position: (a).** It is contained in the reader and uses the store as the source of truth. Not built.

### F15. Certificate reload (B-14) and the rest of B-18
Neither the HTTP server nor the WebSocket nor the built-in gRPC TLS can rotate a certificate without a
restart, short of a hand-written `ResolvesServerCert`. cert-manager style rotation is the normal Kubernetes
case. The rest of B-18: `tls_config_with_extra_roots` still skips a bad CA with a warning (the strict
constructor exists), and the server crate has no PEM client-config helper.
- Options: (a) a `ReloadingCertResolver` (watch the PEM files, or SIGHUP) usable by `Server::with_tls` and
  `WebSocketDispatcher::with_tls`, and gRPC served through `serve_with_incoming` over tokio-rustls with the
  same resolver (additive, M, needs a file-watch decision: poll or `notify`); (b) document restart-on-rotate
  and the hand-written resolver; (c) deprecate `tls_config_with_extra_roots` in 0.16.0 so nobody new uses it.
- **My position: (a) after 0.16.0 unless you want it in; (c) now.** Not built.


---

# Per-row options

Written by the triage agents from reading the code; their recommendations are theirs.

<a id="a14"></a>
## A14

**Row:** The generic `DATABASE_URL` env var plus generic table names (`tasks`, `push_configs`) in the `public` schema collide with an existing database or a developer's shell

Severity now: low (was medium: startup now logs the redacted database URL, and a table collision fails closed inside one transaction) · effort: S (docs + env alias) / L (schema or prefix option across every SQL string) · breaking: behaviour (example env var rename) or storage (table rename) depending on option · theme: pg-schema

**On `main`:** deploy-agent still reads the generic `DATABASE_URL`, and the PostgreSQL stores still create generic table names in whatever schema the connection's search_path names, with no schema/prefix option or documentation. The damage is smaller than reported: the startup log now names the database, and a clash with an existing `tasks` table is refused in one rolled-back transaction rather than silently adopted (read, not run).

**Open:** Pick a naming policy (see options). Independently, the search_path inconsistency in pg_schema (NEW-a-l-2) must be fixed before 'use a dedicated schema' can be recommended.

**Options:** (1) Docs + example only: document a dedicated schema via `options=-c search_path=a2a` (single schema on the path), and have deploy-agent read `A2A_DATABASE_URL`, falling back to `DATABASE_URL` with a WARN naming which it used. XS-S, no SDK change; relies on fixing NEW-a-l-2. (2) Add `with_schema(name)` (or a table prefix) to the four PostgreSQL stores and the migration runner: every SQL constant becomes schema-qualified; M-L, additive API, large test surface. (3) Rename tables to `a2a_*` with a migration: removes the clash for good but is a storage-breaking change for every existing deployment and the upgrade guard functions. Recommendation: (1) now, with NEW-a-l-2 fixed and a live test of the search_path recipe; (2) only if users ask for multiple A2A deployments in one database.

Live probe: To settle the collision outcome: PostgreSQL 16/17; `CREATE TABLE tasks (id serial primary key, title text)`; run deploy-agent with DATABASE_URL at that DB; expect exit 1 with a column/FK error and `\d tasks` unchanged (no applied_seq). For the schema option: `CREATE SCHEMA a2a`, URL `...?options=-c%20search_path%3Da2a`, start twice, confirm every A2A object is in `a2a` and none in `public`; repeat with `search_path=a2a,public` and a `public.tasks` present (expected to misbehave today, NEW-a-l-2).

Evidence: [`a-l.json`](a-l.json).

<a id="a22"></a>
## A22

**Row:** Configuration names differ across binaries

Severity now: low (unchanged) · effort: XS (table) / M (rename with aliases; ITK, go_sdk_interop.sh, scripts/adversarial and itk/Dockerfile.rust-agent set A2A_BIND_ADDR) · breaking: behaviour, for scripts setting the old example variable names, if renamed without aliases · theme: config-naming

**On `main`:** Still four spellings for 'where to listen' (PORT, A2A_BIND_ADDR, SUT_HOST, hard-coded), two for extra listeners even within the A2A_ prefix, two meanings for *AGENT_URL, and two database variables; nothing documents them in one place, and the book's env table is missing SDK-level variables added since.

**Open:** A naming decision (options) plus a single reference table. The bind-address-on-card behaviour of the A2A_BIND_ADDR examples is tracked as NEW-a-l-5.

**Options:** (1) Document only: one table in book/src/reference/configuration.md with every SDK variable (add SSL_CERT_FILE/DIR and the proxy scope) and a per-example section. XS, no break. (2) Converge examples on PORT/A2A_BIND_ADDR + A2A_PUBLIC_URL + DATABASE_URL-or-A2A_DATABASE_URL, keeping old names as aliases that log a deprecation line; rename mcp-bridge's A2A_AGENT_URL to A2A_REMOTE_URL. M, touches ITK/TCK scripts. (3) Leave examples as is and say in each README which variables it reads. Recommendation: (1) now; (2) folded into the A14 decision on the database variable, keeping PORT for deploy-agent since platforms inject it.

Live probe: none

Evidence: [`a-l.json`](a-l.json).

<a id="a24"></a>
## A24

**Row:** There is no ready-to-run server binary; every deployment means writing and building a Rust crate

Severity now: info (unchanged) · effort: M (publish deploy-agent image) / L (gateway binary) · breaking: none · theme: packaging

**On `main`:** Still true: every deployment means writing and building a Rust crate; there is no published image or env-configured server binary.

**Open:** A product-scope decision.

**Options:** (1) Keep the scope and say so on the README front page next to the deploy commands (XS). (2) Publish the deploy-agent image to GHCR from release.yml with provenance, behind a repository variable like PUBLISH_CLI_BINARIES; this also gives A13 a real `image:` to point at (M; adds a public artifact with CVE upkeep - see NEW-a-l-4b). (3) Build an `a2a-gateway`-style binary: OpenAI-compatible model behind ToolLoopExecutor, configured by env, published as image (L; new supported surface). Recommendation: (1) now; (2) when the maintainer is ready to own an image, since it fixes A13 at the root.

Live probe: none

Evidence: [`a-l.json`](a-l.json).

<a id="b-10"></a>
## B-10

**Row:** Long-lived streams are not kept alive through L7 idle timeouts on the WebSocket and gRPC bindings (off by default)

Severity now: medium · effort: M · breaking: behaviour for (2); wire for a stream heartbeat; (1) none · theme: keepalive

**On `main`:** Quiet WebSocket and gRPC streams still carry no traffic by default on either side, so an L7 load balancer's idle timeout (often 60 s) closes them. Turning keepalive on by default was explicitly decided against for gRPC.

**Open:** See options.

**Options:** (1) Client WebSocket transport sends a Ping every ~25 s by default: servers answer automatically, and frames on the wire keep an L7 LB open; non-breaking. (2) Server WebSocketDispatcher idle_timeout on by default at DEFAULT_WS_IDLE_TIMEOUT (75 s, ping at 37.5 s): behaviour change, closes unresponsive peers. (3) gRPC: HTTP/2 PINGs are hop-by-hop and do not cross an LB that terminates HTTP/2, and grpc-go GOAWAYs clients that ping more often than 5 min, so only an application heartbeat on the stream works end to end: a wire decision for an ADR. Recommendation: (1) now, a book section on LB idle settings for WebSocket and gRPC, opt-in client gRPC keepalive; (3) via ADR before any gRPC default changes.

Live probe: nginx with proxy_read_timeout 20s (WebSocket) and grpc_read_timeout 20s (gRPC) in front of the dispatchers; `a2a stream` to an agent that emits one event then is silent 40 s; observe the close at ~20 s; repeat with WebSocketDispatcher::with_idle_timeout(30s) to show pings keep it open

Evidence: [`b.json`](b.json).

<a id="b-11"></a>
## B-11

**Row:** A blocking `SendMessage` longer than the LB idle timeout returns 504; nothing keeps it alive and the docs do not warn

Severity now: medium · effort: S · breaking: none for docs; behaviour for (2)/(3) · theme: lb-timeouts

**On `main`:** The rustdoc now warns, but the book does not and nothing keeps a long blocking SendMessage alive, so behind a 60 s LB it returns 504 while the task runs on, now more often because the client waits 600 s by default.

**Open:** See options.

**Options:** (1) Docs only: a book page 'Behind a load balancer' (blocking send longer than the LB idle gives 504; use send_streaming_message or return_immediately + subscribe; raise the LB idle) linked from the client and CLI pages. (2) Client: when the card declares streaming, run a blocking send over the streaming call and fold to the final result (SSE's 30 s comments keep the LB open): behaviour change on the wire, opt-in at first. (3) Server: leading-whitespace heartbeat on blocking JSON-RPC responses: commits a 200 before the outcome, breaking the HTTP-status mapping, and impossible for HTTP+JSON. Recommendation: (1) now, (2) as an opt-in, not (3).

Live probe: nginx proxy_read_timeout 10s in front of an agent whose turn takes 20 s; `a2a send` -> 504 / ambiguous failure while `a2a task get` later shows the task completed

Evidence: [`b.json`](b.json).

<a id="b-23"></a>
## B-23

**Row:** OTLP proxy handling differs from curl and Go in a few places, and some `NO_PROXY` forms are ignored

Severity now: low · effort: S · breaking: behaviour for loopback exemption / case precedence · theme: proxy-semantics

**On `main`:** Unchanged and now shared by every outbound path: NO_PROXY host:port and *.domain entries never match, upper case wins, loopback is not exempt, and SOCKS/HTTPS proxies are bypassed with a warning.

**Open:** See options.

**Options:** (1) Normalise NO_PROXY before hyper-util: '*.example.com' -> '.example.com', 'host:port' -> 'host' (errs toward direct): non-breaking for every entry that matched before. (2) Always exempt localhost, 127.0.0.0/8 and ::1, as Go does: behaviour change that closes NEW-b-2; the client test proxy_and_cert_env.rs relies on https://localhost being proxied and would need a non-loopback name. (3) Lower-case precedence as curl: behaviour change, little value. Recommendation: (1)+(2) in one change, document (3).

Live probe: none

Evidence: [`b.json`](b.json).

<a id="b-29"></a>
## B-29

**Row:** Crypto provider and cipher constraints

Severity now: info · effort: S · breaking: none for (1) · theme: crypto-policy

**On `main`:** Unchanged: ring only on default paths, TLS 1.2+ AEAD suites, no FIPS, CRL or OCSP, and no insecure mode. A custom ClientConfig is the escape hatch everywhere except the token provider and the gRPC client.

**Open:** See options.

**Options:** (1) Document 'bring your own ClientConfig' as the FIPS/CRL route, and close the two paths that cannot take one (token provider B-03, gRPC client B-20): no new dependency. (2) A `fips` feature switching every hard-coded ring to aws-lc-rs/fips: L effort across both copies of the net code, native C build. (3) A CRL helper (WebPkiServerVerifier::builder().with_crls()) in tls.rs. Recommendation: (1), then (3) on demand, (2) only when a user asks.

Live probe: none

Evidence: [`b.json`](b.json).

<a id="c-08"></a>
## C-08

**Row:** Unknown or unsupported enum values are fatal at startup (spec says warn and ignore)

Severity now: medium (unchanged) · effort: S · breaking: behaviour - a configuration that used to stop the process would start and export differently · theme: otel-config

**On `main`:** Unrecognised enum values and unparsable numbers in the OTEL_* variables fail build() and every binary exits 2, contrary to the spec's MUST warn-and-ignore; this is a documented choice, not an oversight.

**Open:** Decide the policy; then change selection/protocol/compression/timeout/prometheus-port parsing, the tests that pin refusal, and environment.md.

**Options:** (1) Spec behaviour: push a Telemetry notice (printed at WARN by install_subscriber) and treat the variable as unset: an unknown exporter item is dropped (the whole variable ignored if nothing valid is left), protocol falls back to http/protobuf, compression to none, timeout to 10 s, the Prometheus port to 9464; match enums case-insensitively; keep hard errors for unreadable certificate/key files and for values given in code. Trade-off: a typo starts a process that exports differently, visible only as one WARN line. (2) Keep refusing by default and add an opt-in lenient mode (builder method or env). Trade-off: still violates the MUST by default, fleets with a shared OTEL_* preset still crash-loop. (3) Hybrid: names that belong to other SDKs (zipkin, jaeger, zstd, prometheus without the feature) turn that item off with a notice; malformed numbers and protocol strings still refuse. Recommendation: (1), because the notices channel removes the reason the refusal was introduced and the crate already applies it to OTEL_PROPAGATORS.

Live probe: none

Evidence: [`c.json`](c.json).

<a id="c-10"></a>
## C-10

**Row:** W3C baggage is never propagated across A2A, and `OTEL_PROPAGATORS` has no effect on the A2A wire

Severity now: medium (unchanged) · effort: M · breaking: behaviour (option 1: OTEL_PROPAGATORS=none stops traceparent, baggage reaches remote agents) · theme: otel-propagation

**On `main`:** W3C baggage dies at every A2A hop (client, push, server), and OTEL_PROPAGATORS changes only the global propagator that A2A never uses.

**Open:** Pick a policy; implement on JSON-RPC/HTTP+JSON/gRPC client, push sender and listener; document WebSocket (no per-request headers) as unchanged.

**Options:** (1) Inject and extract through opentelemetry::global::get_text_map_propagator on every header-carrying binding, keeping InboundTracePolicy (Restart and Drop also discard inbound baggage) and with_trace_propagation(false) as the off switch for both. Spec-aligned; OTEL_PROPAGATORS=none would stop traceparent; any upstream baggage is forwarded to third-party agents. (2) Keep the hard-coded W3C trace context and add opt-in baggage propagation (ClientBuilder and RequestHandlerBuilder switches, default off), plus honour OTEL_PROPAGATORS=none as 'do not inject'. Low risk, partial spec alignment. (3) Document the gap only. Recommendation: (2) - baggage is a data channel to remote agents, so opt-in fits the crate's trust model; ADR 0034's lineage proposal could use the same channel.

Live probe: none to settle. To prove a fix: two agents exporting to a collector, send a request with 'baggage: tenant.id=t1' through agent A to agent B and assert B's executor sees it (opentelemetry::baggage on the current context) and B's span carries it if a span processor copies baggage.

Evidence: [`c.json`](c.json).

<a id="c-16"></a>
## C-16

**Row:** The Prometheus pull endpoint binds `localhost` by default and runs a separate unauthenticated listener

Severity now: low (unchanged) · effort: S · breaking: none · theme: otel-prometheus

**On `main`:** The default is the spec's and is documented; in Kubernetes the operator must set the host and rebuild with the feature, and the scrape port is a second, unauthenticated, uncapped listener.

**Open:** Choose whether /metrics can be mounted on the A2A server's own port; give the listener an idle timeout and a connection cap either way.

**Options:** (1) Leave it: spec default, documented. (2) Add an additive handler (e.g. Telemetry::metrics_service() or an axum route) so /metrics can sit on the A2A port behind its auth and connection limits; keep the separate listener for non-axum users; add idle timeout + cap. (3) Default the host to 0.0.0.0 when KUBERNETES_SERVICE_HOST is set - deviates from the spec and widens exposure silently. Recommendation: (2).

Live probe: none

Evidence: [`c.json`](c.json).

<a id="c-17"></a>
## C-17

**Row:** WebSocket per-call credential warning is not rate-limited (log spam)

Severity now: low (unchanged) · effort: XS · breaking: none · theme: client-logging

**On `main`:** The per-call WARN is real and deliberate; it also fires for the SDK's own non-credential A2A-Extensions header, where its advice is wrong.

**Open:** Decide the policy; at minimum exclude A2A-Extensions (delivered at connect) from the warning.

**Options:** (1) Rate-limit as counting.rs does (one line per 30 s with a held-back count). (2) Once per connection per distinct header-name set, plus a client counter. (3) Per call only for credential-bearing headers (Authorization, the card's apiKey names), never for A2A-Extensions. Recommendation: (3) combined with (1).

Live probe: none

Evidence: [`c.json`](c.json).

<a id="c-22"></a>
## C-22

**Row:** No Kubernetes / Helm equivalent

Severity now: low (unchanged) · effort: M · breaking: none · theme: k8s

**On `main`:** Kubernetes users get a Deployment with commented OTEL env but no PrometheusRule, dashboards as ConfigMaps, scrape objects or Collector values.

**Open:** Decide scope.

**Options:** (1) Generate a PrometheusRule from rules/*.yml and Grafana dashboard ConfigMaps (sidecar label) from dashboards/*.json, with a CI check that they match the sources. (2) Document how to load the existing files into kube-prometheus-stack and the Grafana Operator only. (3) A Helm chart for the Collector and stack - ongoing maintenance of a second deployment surface. Recommendation: (1), plus uncommenting the deploy-agent OTEL block behind a clear 'set the endpoint' note.

Live probe: none

Evidence: [`c.json`](c.json).

<a id="d-05"></a>
## D-05

**Row:** Push is at-most-a-few-attempts, not durable, and has no dedupe id or dead-webhook handling

Severity now: medium (unchanged) · effort: L · breaking: none for a header and docs; behaviour for auto-disabling configs; schema for an outbox · theme: push-durability

**On `main`:** All four sub-claims hold on main: a few in-memory attempts, nothing persisted across restart (and lanes now also shed under backlog), no dedupe id, and a 404/410 webhook is kept and paid for on every later event.

**Open:** durable outbox; delivery id header; dead-config handling; a sentence in the book that delivery is at-most-a-few-attempts and lost on restart.

**Options:** 1) Cheap now (S): send a stable A2A-Delivery-Id (taskId:seq for streamed events, taskId:requestId:index for a blocking batch) and document at-most-N-attempts / lost-on-restart semantics. 2) Dead-webhook policy (M): count consecutive failures per config in the push-config store; after 410 or N failures stop delivering (spec 4.3.3 allows it) and expose it in metrics/GetTaskPushNotificationConfig; behaviour change, opt-out flag. 3) Durable outbox (L/XL): outbox table written with the event-log append, worker with exponential backoff+jitter over hours, claimed per row so replicas share it; needs an ADR. Recommendation: do 1 now, 2 next behind a default-on limit with a metric, write an ADR for 3 (it overlaps E-F14 per-task cap across replicas).

Live probe: none to confirm; to prove an outbox: kill -9 a replica with deliveries queued against a down receiver, bring the receiver up, observe deliveries arrive from another replica

Evidence: [`d.json`](d.json).

<a id="d-10"></a>
## D-10

**Row:** `Retry-After` is clamped to `max_backoff` and not jittered

Severity now: low (unchanged) · effort: S · breaking: behaviour (retry timing) · theme: client-retry

**On `main`:** As reported: a Retry-After longer than max_backoff is cut to 30 s and retried early, without jitter, and the HTTP-date form is ignored. The clamp is documented, so changing it is a policy choice.

**Open:** Choose the policy; add jitter and HTTP-date parsing either way.

**Options:** 1) Honour Retry-After up to the parser's 1 h cap (ignore max_backoff for server-directed delays): correct per RFC 9110 but a call can wait an hour. 2) Fail fast: if Retry-After > max_backoff (or > remaining budget) stop retrying and return the error, which already carries retry_after, so the caller decides. 3) Keep the clamp. In every option add additive jitter (e.g. +0-10% or +0-1 s) to server-directed delays and parse the HTTP-date form (httpdate is already in hyper's tree). Recommendation: 2 + jitter + HTTP-date: never retries inside a penalty window, never parks a call for an hour.

Live probe: none

Evidence: [`d.json`](d.json).

<a id="d-11"></a>
## D-11

**Row:** No retry by default, and no automatic stream resume

Severity now: low (unchanged) · effort: M · breaking: behaviour if any default changes; none for an opt-in API · theme: client-stream-resume

**On `main`:** Plain EventStream and follow_task still never resume a cut stream and retries are still off by default; only the Delegation API resumes now.

**Open:** Decide on automatic resume for follow_task / an opt-in resuming stream, and on the retry default.

**Options:** 1) Make TaskFollow resume: on IncompleteStream resubscribe via subscribe_to_task_from(last_event_id) with the ResumePolicy Delegation already has, then fall back to polling GetTask (no API break; follow_task's contract is 'until it ends'). 2) Add opt-in EventStream::resuming(client, policy) for send/subscribe streams (send streams can only resume as SubscribeToTask once the task id is known). 3) Turn RetryPolicy on by default: safe for idempotent methods given safe_to_retry_non_idempotent (retry.rs:284-295), but changes timing for every caller. Recommendation: 1 now (reuse delegation::resume), 2 as opt-in, leave 3 off and document it in the quick start.

Live probe: none

Evidence: [`d.json`](d.json).

<a id="d-18"></a>
## D-18

**Row:** In-memory capacity eviction can evict live or parked tasks

Severity now: low (unchanged) · effort: S · breaking: behaviour (new tasks refused instead of old ones evicted) · theme: inmemory-store

**On `main`:** As reported, and documented as intended: once the oldest 1024 tasks are all live, capacity eviction removes running or parked tasks, silently.

**Open:** Policy choice plus a metric.

**Options:** 1) Never evict non-terminal: when the window holds no terminal task, refuse the new task with Overloaded (503, Retry-After) and count it; memory stays bounded by max_capacity, availability drops under overload. 2) Evict working before input-required/auth-required, and count every non-terminal eviction (metric + warn). 3) Keep, add the metric only. Recommendation: 1 for parked (interrupted) tasks at least, i.e. 2 with parked tasks never evicted, plus the metric; the in-memory store is the default for small deployments where losing a human's pending task is the worst outcome.

Live probe: none

Evidence: [`d.json`](d.json).

<a id="d-20"></a>
## D-20

**Row:** gRPC and WebSocket listeners have no liveness bound by default

Severity now: low (unchanged) · effort: S · breaking: behaviour (pings to existing clients; idle connections closed) · theme: listener-liveness

**On `main`:** Unchanged and deliberate: the gRPC and WebSocket listeners have no liveness bound, connection cap or post-accept handshake bound unless the deployer opts in.

**Open:** Decide the defaults.

**Options:** 1) Safe-by-default like the HTTP listener: gRPC HTTP/2 keepalive (60 s, 20 s) + tcp_keepalive, WS idle_timeout = DEFAULT_WS_IDLE_TIMEOUT (75 s, which pings at half and closes only unresponsive peers), and a default max_connections; opt-out setters. 2) Keep off, but make the deploy-agent and book's production recipe set them, and warn at serve() when unset. 3) Keep as is. Recommendation: 1 for WS idle_timeout and gRPC keepalive (both close only peers that do not answer a ping, which is the cost the 'off' rationale worries about least), 2 for max_connections.

Live probe: none (already measured by the authors: 400 silent gRPC connections alive)

Evidence: [`d.json`](d.json).

<a id="e-f12"></a>
## E-F12

**Row:** tenant `max_concurrent_tasks` is per process, so the fleet admits N× the limit

Severity now: medium · effort: S · breaking: none for (a); behaviour for (b) · theme: fleet-limits

**On `main`:** max_concurrent_tasks is enforced per process, so a fleet of N admits N times the limit, and the docs do not say so.

**Open:** A decision: document it as per replica, or enforce it fleet-wide.

**Options:** (a) Document it as per replica (size it as the fleet limit / N; HPA changes N), and log at build when a PerTenantConfig sets it over a store that hands out turn claims. XS, no behaviour change. (b) Count live claims per tenant at claim time: a2a_claim_turn takes pg_advisory_xact_lock(hashtext('a2a.tenant:'||scope)) and refuses past the limit (the scope column already holds the tenant). M; one more lock per turn, and the limit is passed to the claim function. (c) A lease-row semaphore table renewed by the claim renewer. L. Recommendation: (a) now; (b) when someone needs the cap to hold across replicas.

Live probe: none (settled by reading). Optional demo: report X15.

Evidence: [`e.json`](e.json).

<a id="e-f13"></a>
## E-F13

**Row:** `Profile::Production` accepts in-memory halt and lease stores, so the kill switch is per-replica in a "production" fleet

Severity now: high · effort: M · breaking: behaviour: build() refuses configurations it accepted · theme: production-profile

**On `main`:** Production still accepts in-memory halt and lease stores on a fleet. Severity raised from Medium: with an in-memory lease store, a parent's GetTask lease renewals that the balancer sends to other pods are no-ops, so the pod running a delegated child stops it while the parent is still renewing. And the halt (kill switch) stops one pod only.

**Open:** A policy decision that amends ADR 0025.

**Options:** (a) Add an additive default method to HaltStore, LeaseStore, PushConfigStore and AuditStore, e.g. fn process_local(&self) -> bool { false }, true for the InMemory* types. When task_store.turn_claims().is_some(), Production refuses: a process-local halt store; a process-local lease store; a process-local push-config store when a push sender is set (E-F4 refuses this only when the card declares push); and, under Auditable, a process-local audit store. It warns for a RateLimitInterceptor without a shared counter, tenant max_concurrent_tasks (E-F12) and AuthFailureThrottle (E-F15, no shared option yet). Single-process deployments (no turn claims) keep in-memory. Needs a SQLite lease store or an explicit with_single_host() escape for one-host SQLite. M; behaviour change for the few who combine Production, a SQL store and in-memory halt/lease. (b) Warn instead of refuse: no breakage, but ADR 0025 chose refusal because 'a warning in a log nobody reads protects nobody'. (c) Keep and document. Recommendation: (a).

Live probe: none (settled by reading). Optional demo: report X16, plus a lease variant: Production with InMemoryLeaseStore on 2 replicas; a delegated 'hold' with a 3 s lease renewed by GetTask with the lease extension every 1 s through a round-robin balancer; the task is stopped within a few TTLs.

Evidence: [`e.json`](e.json).

<a id="e-f18"></a>
## E-F18

**Row:** the audit chain is one optimistic append point for the whole fleet

Severity now: medium · effort: M · breaking: none for (a)/(c); wire/format for (b) · theme: audit-fleet

**On `main`:** One optimistic append point per tenant chain for the whole fleet: each append is at least 2 round trips and every success invalidates the other replicas' reads. Back of the envelope (conjecture, not measured): with N saturated contenders a request loses 8 rounds with probability about ((N-1)/N)^8, 0.4% at N=2, 3.9% at N=3, 17% at N=5. So failures may appear well below the report's N >= 9, and under require_record(true) (Auditable) those requests are refused.

**Open:** A decision on the append protocol; then thresholds measured (X17).

**Options:** (a) Append in one transaction under pg_advisory_xact_lock(hashtextextended('a2a.audit:'||chain,0)): read head, insert, commit. No lost races and the same single-chain order; throughput stays bounded by one transaction per append per chain. S-M, no format change. (b) Per-replica sub-chains with periodic cross-signed checkpoints: scales, but changes the chain model, export and verification. L. (c) Jittered backoff and more attempts: XS, a mitigation only. Also take time_ms from the database in (a). Recommendation: (a).

Live probe: Report X17: 1, 2, 3 and 5 replicas with AuditLog over PostgresAuditStore and require_record(true), load at 200-500 rps through a round-robin balancer; count 'audit append lost the race' errors and refused requests, and audited appends/s. Decides the thresholds; the mechanism is settled by reading.

Evidence: [`e.json`](e.json).

<a id="e-f25"></a>
## E-F25

**Row:** reattach polling is unbounded and shares the pool

Severity now: low · effort: M · breaking: none for (a)/(c); behaviour (latency) for (a) · theme: subscribe-polling

**On `main`:** Polling is unbounded by design and shares the pool with claim renewal; orphans no longer poll forever since the reaper. The cost is about 8 statements/s per idle cross-replica subscriber, on the pool that keeps claims alive.

**Open:** A decision on how to cut idle polling without breaking §3.1.6.

**Options:** (a) Back off the poll while nothing changes (250 ms doubling to a few seconds, reset on change); cheap, keeps §3.1.6, adds latency only to idle streams. (b) LISTEN/NOTIFY on appends (KNOWN unbuilt; poolers need care). (c) Run subscriber polls on a separate small read pool, and renewals on their own connection (E-F7). Recommendation: (a) plus (c) now, (b) later.

Live probe: none for the mechanism; capacity numbers need a run (2,000 idle subscriptions on one pod; pg_stat_statements calls/s and renewal latency).

Evidence: [`e.json`](e.json).

<a id="g-06"></a>
## G-06

**Row:** Default features surprise in both directions

Severity now: medium (unchanged; the server-only gap is wider than the report says) · effort: S · breaking: Adding tls-rustls to the server's defaults is a default-feature change: a minor-bump change under STABILITY.md section 2, so 0.16.0 only (cargo-semver-checks has no lint for an added default feature; it is not blocked). Removing otlp from the SDK defaults is also 0.16.0-only and is the kind cargo-semver-checks flags (a feature no longer enabled by default); code using Telemetry then fails to compile until it enables otlp (loud, not silent). · theme: features-defaults

**On `main`:** Both directions still hold: the SDK's defaults compile the OTLP exporter and tonic for hello-agent (deliberately), and the server crate alone cannot reach any https:// webhook, JWKS/OIDC issuer or model endpoint.

**Open:** Decide the server default; fix the 'Server only' docs either way.

**Options:** A) Server default = [tracing, otel, tls-rustls] in 0.16.0: matches client and SDK, fixes push/JWKS/model over https for server-only users; costs rustls+ring+webpki-roots+hyper-rustls+tokio-rustls in every server-only build (already in every SDK build). B) Keep the defaults; make the docs say 'server only: add features = ["tls-rustls"] or push, JWT (JWKS) and the model client cannot reach https://' and have RequestHandlerBuilder::build() / JwtAuthInterceptor log a startup error when they are configured without TLS: non-breaking, but leaves a default that silently cannot do the spec's normal case. C) Additionally move otlp out of the SDK defaults (hello-agent and every Telemetry user adds features = ["otlp"]). Recommendation: A in 0.16.0; keep the SDK's otlp default as decided on 2026-10-14 unless the measurement above shows a material cost, and then decide C with the number.

Live probe: measurement only, not live: cargo build --timings -p hello-agent with the SDK defaults vs default-features=false + [tls-rustls, tracing, otel, webhook] (Telemetry removed), clean target dir each, to put a number on the otlp cost before deciding option C

Evidence: [`g-api.json`](g-api.json).

<a id="g-07"></a>
## G-07

**Row:** Constructors produce invalid cards; streaming is opt-in

Severity now: medium (unchanged) · effort: M · breaking: Additive: AgentCard::minimal/new constructor taking description + skill, a detailed check returning a typed error with the skill index, a build()-time warning when streaming is undeclared. Changing new()'s signature or validate()'s return type needs a deprecation cycle (section 3.1: new item in 0.16, old deprecated, removed 0.17). Defaulting streaming to Some(true) in AgentCard::new is a wire-visible behaviour change (cards built with new() start advertising streaming): 0.16.0 only, with a Breaking Changes entry and an upgrade-guide section. · theme: agent-card-ergonomics

**On `main`:** All three sub-claims hold: new() yields a card build() refuses, validate() names no index, and streaming is opt-in, so the flagship example cannot stream; AgentCard's rustdoc also wrongly calls new() 'valid by construction'.

**Open:** Constructor/validation shape and the streaming default; fix the 'valid by construction' rustdoc (codegen.toml) and its example now.

**Options:** A) Additive only: AgentCard::minimal(name, version, iface, description, skill), AgentCard::check() -> Result<(), CardError> naming the field/index (keep validate()), and RequestHandlerBuilder::build() logging when capabilities.streaming is None; set with_streaming(true) in hello-agent. Zero break. B) A plus default streaming = Some(true) in AgentCard::new in 0.16.0: every executor streams through the event queue, so the SDK can always honour it; costs one Breaking Changes entry and changes what existing cards advertise. C) Make new() take description and skill (deprecate-first; churns every caller). Recommendation: A now (can land before 0.16.0), B in 0.16.0 if the maintainer accepts the wire change; not C.

Live probe: none

Evidence: [`g-api.json`](g-api.json).

<a id="g-09"></a>
## G-09

**Row:** Too many ways to serve, with inconsistent shapes

Severity now: medium (unchanged) · effort: L · breaking: Additive half (A2aRouter as a Dispatcher, ServeConfig accepted by gRPC/WebSocket): none. Unifying return types or receivers (gRPC io::Result<ServeReport> -> ServeReport, WebSocket self: Arc<Self> -> self) changes public signatures: section 3.1 wants a new name + #[deprecated] on the old one in 0.16, removal in 0.17; changing them in place is only possible in 0.16.0 and would be listed as Breaking (cargo-semver-checks would flag the signature change; passes now only because the version is already 0.16.0). · theme: serve-unification

**On `main`:** Eight entry points with three receiver shapes and three return types remain; only Server gets ServeConfig, and Axum users get none of its protections.

**Open:** Pick the target shape; the additive half can land before 0.16.0.

**Options:** A) Additive: impl Dispatcher for A2aRouter (call the axum Router as a tower service, map its body into DispatchResponse; body errors become truncation) so Axum users can run on Server; GrpcDispatcher/WebSocketDispatcher::with_serve_config(ServeConfig) reading the shared struct, their own grace/drain/max_connections setters deprecated in 0.16. B) A plus deprecate the free serve/serve_with_addr and the per-dispatcher serve/serve_with_addr in 0.16 (removed in 0.17), leaving one serve_with_shutdown shape per listener kind. C) Document the matrix in the book and change nothing. Recommendation: A in 0.16.0 (or before), B's deprecations in 0.16.0 so 0.17 can remove; do not change signatures in place.

Live probe: none

Evidence: [`g-api.json`](g-api.json).

<a id="g-10"></a>
## G-10

**Row:** Wire types are exhaustive structs with all-public fields

Severity now: medium (unchanged; STABILITY.md section 4 is wrong about it) · effort: XL · breaking: api: #[non_exhaustive] on a struct breaks every downstream struct literal and exhaustive destructure, with no deprecation path; 0.16.0 window only (cargo-semver-checks flags 'struct marked non_exhaustive'; passes now only because the version is 0.16.0; after 0.16.0 ships it is blocked until 0.17.0). Deprecating the url field and fixing STABILITY.md are additive. · theme: wire-types-non-exhaustive

**On `main`:** The wire structs are still exhaustive with all-public fields, contrary to STABILITY.md section 4; deploy-agent was moved to AgentCard::new in 57f034e but five other examples still teach the literal and the dead url field.

**Open:** Decide whether to mark them in 0.16.0; in any case correct STABILITY.md section 4, deprecate AgentCard.url, move the remaining example literals to constructors.

**Options:** A) Mark all 40 in 0.16.0 via codegen.toml attrs, after giving each type a constructor (TaskStatusUpdateEvent::new, TaskArtifactUpdateEvent::new, SendMessageConfiguration builders ...) and converting the in-workspace literals (XL). B) Mark only the protocol objects most likely to grow (AgentCard, AgentSkill, AgentInterface, AgentProvider, AgentExtension, Message, Part, Task, TaskStatus, Artifact, the two update events, TaskPushNotificationConfig) in 0.16.0; params later (L). C) Keep them exhaustive and rewrite STABILITY.md section 4 to say a new proto field is a breaking minor; deprecate url (S). Recommendation: 0.16.0 already breaks and section 7's clock restarts anyway, so it is the cheapest moment for B (or A if the effort fits); if neither fits 0.16.0, do C now so the policy stops promising what the code does not do.

Live probe: none

Evidence: [`g-api.json`](g-api.json).

<a id="g-14"></a>
## G-14

**Row:** `Telemetry` ergonomics

Severity now: medium (unchanged) · effort: S · breaking: #[must_use] on Telemetry: warning only (an error under -D warnings), same precedent as ServeReport in 0.16.0. #[deprecated] on init_otlp_pipeline*: additive now, removal no earlier than 0.17. Changing what init() exports to is a behaviour change: 0.16.0 only, with an upgrade-guide section; it silently stops export for a deployment relying on a localhost collector (a sidecar) with no OTEL_* variable set. · theme: telemetry-ergonomics

**On `main`:** All four points hold: no #[must_use] on the drop guard, init() exports to localhost by default, the older pipeline API is undeprecated, and Drop can block (documented).

**Open:** #[must_use] and the deprecation are mechanical; the init() default is the decision.

**Options:** 1) #[must_use = "dropping Telemetry shuts export down"] in 0.16.0 (recommended). 2) init(): A) keep the spec-conformant localhost default, document it at init(), and add an explicitly named Telemetry::init_where_configured() that examples use (additive); B) make init() export-where-configured in 0.16.0 (behaviour break, sidecar risk above); C) leave. Recommend A. 3) Deprecate init_otlp_pipeline* in 0.16.0, remove in 0.17 (recommended).

Live probe: none

Evidence: [`g-api.json`](g-api.json).

<a id="g-15"></a>
## G-15

**Row:** Naming inconsistencies

Severity now: low (report L-M) · effort: M · breaking: Each rename is a section 3.1 change: add the new name in 0.16 (additive), #[deprecated(since = "0.16.0")] on the old one, remove in 0.17 at the earliest; the SDK prelude must re-export both meanwhile. Renaming in place would be a break cargo-semver-checks blocks once 0.16.0 is published. · theme: naming

**On `main`:** Every naming inconsistency the report lists is still present.

**Open:** Choose which names are worth a deprecation cycle.

**Options:** A) A short list with real confusion, as new names + deprecated aliases in 0.16: RequestHandler::on_subscribe_to_task (for on_resubscribe), create_push_config / on_create_push_config (for set_push_config), a type alias for A2aRouter that names HTTP+JSON, RateLimitConfig::with_window(Duration), with_ prefixes on TenantLimitsBuilder. B) Normalise the whole builder vocabulary (many deprecations, little user value). C) Change nothing; add a naming rule to CONTRIBUTING.md for new APIs. Recommendation: A plus C.

Live probe: none

Evidence: [`g-api.json`](g-api.json).

<a id="g-c-01"></a>
## G/C-01

**Row:** Size and readability

Severity now: medium (unchanged; the file grew) · effort: M · breaking: none · theme: changelog

**On `main`:** The CHANGELOG is 806 KB and growing; [Unreleased] alone is 74 KB with ~100-word bullets, internal notes and lab measurements, and no release has a one-screen summary.

**Open:** A format decision for the maintainer (see options).

**Options:** (a) Add a '### Highlights / Action required' block of <=15 lines at the top of every release section (starting with 0.16.0), enforced by a check like check_release_tree.py (section exists, under N words); keep the detail below. Cheap, keeps one file; does not shrink it. (b) Move '### Internal' and measurement prose out of CHANGELOG.md into docs/changelog-internal/<version>.md and link it; user-facing file shrinks ~10-20%. Needs the release-notes extraction (release.yml:560-600) and check_release_tree.py to ignore the moved text. (c) Keep CHANGELOG.md to one paragraph per change and let the book's upgrading pages carry the migrations. Largest rewrite. Recommendation: (a) now for 0.16.0, plus (b) for Internal; also extend the release.yml oversize fallback to keep '### Behaviour Changes' (NEW-g-docs-7).

Live probe: none

Evidence: [`g-docs.json`](g-docs.json).

<a id="g-c-02"></a>
## G/C-02

**Row:** 0.15.1, a patch, broke existing code

Severity now: medium (unchanged); 0.15.1 is published, the open question is the policy and its gate · effort: S · breaking: none (process); option (a) would make some future patch releases minors instead · theme: release-policy

**On `main`:** Nothing stops a breaking behaviour change in a patch: semver-checks sees only API, and the release gate keys on the words '### Breaking Changes', which 0.15.1 avoided by filing its two changes under 'Behaviour Changes'. The docs are honest about 0.15.1 after the fact; STABILITY.md is silent on it.

**Open:** Policy and gate decision (options). Independently: STABILITY.md §1 should be brought to 0.15.1/0.16.0 (NEW-g-docs-4).

**Options:** (a) Amend STABILITY.md §2: 'A change that makes code which compiled and ran against the previous release fail, panic or send data elsewhere at runtime is breaking, whatever the API diff says', and extend check_cadence to refuse a patch whose section has '### Behaviour Changes' unless each entry carries a 'Spec correction:' or 'Security:' tag. Strict; turns releases like 0.15.1 into 0.16.0. (b) Allow behaviour changes in a patch but require an '**Action required:**' line per entry and make the release-notes fallback keep '### Behaviour Changes'; gate checks the line exists. Keeps cadence, makes the cost visible. (c) Status quo plus a STABILITY.md note that patches may change runtime behaviour under 'Behaviour Changes'. Honest but weakens the SemVer promise. Recommendation: (a) for anything that panics or reroutes traffic, with (b)'s release-notes fix regardless.

Live probe: none

Evidence: [`g-docs.json`](g-docs.json).

<a id="h5"></a>
## H5

**Row:** Control-plane cost is O(children) round trips and store operations per interval

Severity now: S1 (S0 at 10^5-10^6 leased tasks); unchanged · effort: XS (history_length=0, stagger) / S (subscriber caps) / M (per-replica sweeper) / L (lease groups, wire) · breaking: none for the XS/S pieces and the sweeper; wire (new extension method) for lease groups · theme: lease-control-plane

**On `main`:** Unchanged since the audit: per-child renewal tasks doing a full GetTask every ttl/3, a per-turn watcher reading the lease store every second, and unbounded, unthrottled 250 ms polling by idle subscribers with no subscriber cap.

**Open:** All of it. Cheap additive pieces (history_length=0, staggered first renewal, subscriber caps) are not done; lease groups / sweeper need a decision.

**Options:** (A) Per-replica lease sweeper instead of per-turn watchers: one statement per second per replica over the turns this replica runs (deadline_ms < now AND (tenant, task_id) in the running set), firing the tokens. Server-only, no wire change, M. Removes the 1 SELECT/s/turn. (B) Lease groups (the report's design): grant carries a group id; one renew call (new extension method on JSON-RPC/REST/gRPC) renews every child of the group in one UPDATE; one renewer per (client, peer). Wire change, amends ADR 0020, L; also removes the full-task read+sign per renewal. (C) Keep per-child GetTask renewals but make them cheaper: history_length=Some(0) and a random first-tick offset; note GetTask cannot omit artifacts, so renewal stays O(artifacts) under signing. Recommendation: ship C now, A next, subscriber caps now (HandlerLimits::max_subscribers{,_per_task}, default unbounded, Overloaded with Retry-After); B only via a NEW ADR after measuring A at 10^4-10^5 leased tasks per replica.

Live probe: PostgreSQL with pg_stat_statements; server with PostgresTaskStore + PostgresLeaseStore; N = 1,000 then 10,000 Delegation::start_leased(ttl = 30 s) children whose executor blocks (examples/swarm worker 'sleep:'); reset pg_stat_statements, wait 60 s, read calls/s for 'SELECT deadline_ms FROM a2a_leases' (expect ~N/s), 'UPDATE a2a_leases' (~N/10 /s) and the tasks SELECT (~N/10 /s). Subscribers: park N tasks at input-required with one SubscribeToTask each; expect ~4N task reads/s (8N with the event log). Decides how urgent the sweeper/groups are.

Evidence: [`h-designs.json`](h-designs.json).

<a id="h6"></a>
## H6

**Row:** No retry budget, circuit breaker or client-side concurrency limit; retries amplify overload

Severity now: S1; unchanged (amplification only for clients that opt in to retries) · effort: XS (jitter, max-elapsed) / S (per-client in-flight cap, per-client budget) / M (pool-scoped breaker and budget per ADR 0032) · breaking: none if every policy is opt-in; behaviour if jitter is added to Retry-After by default (delays become >= the server's value) · theme: client-resilience

**On `main`:** No retry budget, no circuit breaker, no HTTP in-flight cap and no deadline-aware retries. Retries are off by default; when on, overload refusals are retried up to 3 times following the server's constant 1 s Retry-After without jitter.

**Open:** Everything in the row, plus jitter on server-directed delays.

**Options:** (A) Per-client, opt-in, now (the recorded H-D2 recommendation): RetryPolicy::with_budget(ratio, min_per_sec) as a token bucket in the RetryTransport, ClientBuilder::with_max_in_flight(n) returning the existing retryable TooManyPendingRequests, RetryPolicy::with_max_elapsed(Duration). S, no shared state; but 1,000 clients in one process each have their own budget, so amplification towards one peer across clients is not bounded. (B) ADR 0032 sec. 5: per-origin state in HttpPool (in-flight cap, breaker, retry budget) shared by every client on the pool. M, after the pool. (C) Nothing in the SDK; document mesh retry budgets and outlier detection. Recommendation: A now with types designed to move into the pool, B when ADR 0032 is built; deadline-aware retries via an explicit per-call deadline (or the BudgetHandle) decided in the same ADR.

Live probe: One worker with with_max_concurrent_streams(8) and a slow executor; 1,000 clients with RetryPolicy::default() each sending once; record request arrival times at the worker (access log, or a2a.server.errors{error_kind="overloaded"} sampled every 100 ms). Decides whether refusals return in 1 s lockstep waves and whether total attempts reach ~4x sends.

Evidence: [`h-designs.json`](h-designs.json).

<a id="h7"></a>
## H7

**Row:** Readiness ignores saturation and halt; no load signal for balancers

Severity now: S2/medium (lowered from S1: since H1 saturation refusals carry Retry-After and are retryable, and a halted replica must stay reachable for reads and cancels) · effort: XS-S (ship-now pieces) / M (load header + consumer) · breaking: behaviour if saturation ever drives /ready by default; none for the additive pieces · theme: readiness-load

**On `main`:** /ready means 'not draining and the task store answers'. A replica at its stream cap, halted, or unable to read its halt store still reports ready; there is no load signal; the production listener does not require or warn about max_connections.

**Open:** Saturation/load signal (decision), halt visibility (decision), halt-store sync in readiness (no decision needed), max_connections warning (no decision needed).

**Options:** (A) /ready 503 above a high-water mark with hysteresis (e.g. unready at >= 95% of the cap for 2 probes, ready again at <= 80%), opt-in. Pro: L4 balancers stop sending. Con: replicas saturate together, so readiness flaps fleet-wide and removes capacity when it is most needed; probe periods (1-10 s) are too slow to be a load signal; Endpoints churn. (B) A load signal that is not readiness: a response header (A2A-Load: queues=...;cap=..., or ORCA endpoint-load-metrics) plus a /load JSON endpoint and an exported cap gauge, consumed by Envoy least-request/ORCA or ADR 0032's EndpointResolver. (C) Status quo plus docs (Overloaded + Retry-After, Envoy least_request). For halts: stay ready (reads and cancels must stay reachable, halt.rs:14-16), expose 'halted' in the body and as a gauge. Recommendation: B plus halt in the body; A only opt-in, never default; a halt store that has never been read is a dependency failure and should make the replica not ready.

Live probe: none (settled by reading). To prove a fix: a Kubernetes readinessProbe on /ready, saturate one replica, watch its Endpoints entry.

Evidence: [`h-designs.json`](h-designs.json).

<a id="h8"></a>
## H8

**Row:** Card discovery cost at fleet cold start

Severity now: medium (lowered from S1: the root-store rebuild is gone; per-fetch pools, no single-flight and no stale-if-error remain) · effort: XS (one client per resolver) / S (single-flight, stale-if-error) / M (URL-keyed CardCache on the pool) · breaking: none (all additive; stale-if-error opt-in or per RFC 5861 directive) · theme: card-discovery

**On `main`:** Partly improved by c4404e8 (roots and TLS session cache shared), but every card fetch still builds a new connection pool, concurrent stale lookups stampede, a failed revalidation discards a usable card, and a resolver covers one URL.

**Open:** Single-flight, stale-if-error, one client per resolver, multi-URL cache; a test for the once-per-process TLS config.

**Options:** (A) Inside CachingCardResolver, now: hold one HttpsClient per resolver, single-flight per resolver (reuse token_provider/flight.rs), opt-in stale_if_error(Duration), jittered refresh. S, additive, no API decision. (B) ADR 0032 sec. 6: a URL-keyed bounded CardCache on the shared HttpPool with single-flight, stale-if-error, background refresh at ~80% of freshness and watch(). M, after the pool. (C) ADR 0031 Merged: TTL cache with jitter over discovery sources, for fleet cold start. Recommendation: A now, B when the pool lands, C with the discovery crate.

Live probe: To measure: the swarm_scale card arm (A2A_SWARM_MAX=512 cargo test -p a2a-protocol-server --release --test swarm_scale -- --ignored --nocapture --test-threads=1) p50/p95 before and after; and `ss -tan state established | wc -l` while 512 tasks call resolve() on one stale resolver (expect 1 fetch after single-flight).

Evidence: [`h-designs.json`](h-designs.json).

<a id="h9"></a>
## H9

**Row:** Admission control has refuse-only semantics: no queue, priority or weighted fairness

Severity now: S1 for multi-tenant deployments; medium for single-tenant swarms (refuse + retryable Overloaded is a defensible default since H1) · effort: S-M (reservations) / M (bounded wait + DRR) / L (authenticated priority classes) · breaking: none if opt-in; behaviour if reservations become a default · theme: admission

**On `main`:** Admission is refuse-only with no reservation, queue, priority or fairness across tenants; one tenant inside its own limit can take the whole 1024-stream cap.

**Open:** All of it.

**Options:** (A) Reserved shares, still refuse-only: a tenant configured with max_concurrent_tasks gets a guaranteed floor of the global cap that other tenants cannot take. S-M. Fixes the fairness hole without adding latency; unused reservations idle capacity. (B) Opt-in bounded wait: acquire with timeout = min(max_wait, budget remaining), bounded queue length, FIFO per tenant with deficit round robin across tenants; a send whose budget would expire in the queue is refused at once with BudgetExhausted. M. (C) Priority classes. The report takes priority from message metadata (a2a-rust.com/priority): that is self-asserted by the caller, so every caller would claim the top class; priority must come from server-side identity/tenant config (CallContext), not from the message. L. Recommendation: A first, B opt-in second, C only with an authenticated source.

Live probe: none

Evidence: [`h-designs.json`](h-designs.json).

<a id="h10"></a>
## H10

**Row:** No task lineage, so there is no tree view and no "halt this job" scope

Severity now: S1; unchanged · effort: XS (halt-store forward compatibility) / M (ADR 0034 as written) / L (upstream parentTaskId) · breaking: none (new extension, new HaltScope variant on a #[non_exhaustive] enum); mixed-version behaviour of the halt store must be fixed first · theme: lineage-halt

**On `main`:** No lineage anywhere and HaltScope is still All|Tenant: a job can be stopped only by halting servers or tenants, or by every hop running Rust leases; trees are reconstructable only from trace ids.

**Open:** All of it.

**Options:** (A) ADR 0034 as written: lineage extension in Message.metadata (root defaults to the contextId, depth capped, malformed -> InvalidParams), HaltScope::Root scoped (tenant, root) through the halt store, no stored index. M. No protocol change; stops at non-Rust hops; lineage is caller-asserted, which is acceptable because it only scopes halts. (B) Protocol-level parentTaskId proposed upstream with server-side cascade. L, outward-facing. (C) A tenant per job: rejected in ADR 0034 (isolation is not job identity; default store ignores tenants). Recommendation: A, with two additions the ADR does not make: ship forward-compatible halt-store parsing at least one release before any new scope kind (NEW-h-designs-4), and fix a scope-key encoding where a root or tenant containing ':' cannot collide with another kind's key.

Live probe: none

Evidence: [`h-designs.json`](h-designs.json).

<a id="h11"></a>
## H11

**Row:** Metrics have no tenant, agent or job dimension and no control-plane instruments

Severity now: S2/medium (lowered from S1: part of the row was misread, see evidence) · effort: S (new default-no-op hooks) / S-M (bounded tenant attribute) · breaking: none (new trait methods with default no-op bodies; new attributes opt-in) · theme: otel-metrics

**On `main`:** No tenant/agent/job dimension on any metric and no control-plane instruments (lease, claim, halt action, budget spend); but budget-exhausted endings and overload/halt refusals are already counted, contrary to the report.

**Open:** Tenant dimension (bounded), control-plane hooks, refusal cause.

**Options:** (A) A bounded tenant attribute inside the OTel Metrics implementation, read from TenantContext::current() (the task-local every handler entry already sets), opt-in, allow-list or top-N plus 'other'; no trait change. S. (B) New default-no-op hooks: on_refusal(cause), on_lease(event: granted|renewed|lapsed), on_halt(scope_kind, action), on_budget_settle(meter, granted, spent), on_claim_takeover(); bounded label sets. S-M, additive. (C) Leave per-tenant and per-job accounting to spans and audit records and derive metrics in the collector (spanmetrics). No in-process cardinality risk; needs a collector and sampling distorts counts. Recommendation: B now, A opt-in with an allow-list; never put root/job ids on metric labels (ADR 0034 sec. 4).

Live probe: none

Evidence: [`h-designs.json`](h-designs.json).

<a id="h12"></a>
## H12

**Row:** No client load balancing across replicas of a peer, no outlier ejection, no hedging

Severity now: S2; unchanged (HTTPS h2 pinning makes it more relevant than the report says) · effort: XS (docs) / M (EndpointResolver hook on the pool) · breaking: none (opt-in) · theme: client-lb

**On `main`:** No client-side balancing, ejection or hedging; one URL per client, one gRPC channel, and over HTTPS one h2 connection per peer.

**Open:** Decision (ADR 0032 H12 section) and the measurement it asks for.

**Options:** (A) Decline in core (recorded H-D7): document L7 balancing (Envoy/mesh), headless Services + DNS, and the h2-pinning effect over HTTPS. (B) ADR 0032's EndpointResolver hook on the pool: a service name resolves to endpoints (ADR 0031 sources or DNS), power-of-two-choices by load, passive ejection; hedging only for calls marked idempotent (keyed SendMessage, GetTask, ListTasks), capped. Opt-in, M. (C) A full EndpointSet with active health checks. Recommendation: A now; B only after the probe above shows uneven load; C never in core.

Live probe: Three replicas behind an L4 balancer (Kubernetes ClusterIP Service or HAProxy 'mode tcp'), TLS on; one A2aClient fanning out 300 long-running streaming delegations; read active_queue_count (or a2a.server.queue_depth) per replica. Run again over plain http. Decision: if one replica holds most streams over https (expected from h2 multiplexing) and the spread is even over http, the hook is justified.

Evidence: [`h-designs.json`](h-designs.json).

<a id="h13"></a>
## H13

**Row:** Memory and payload scaling

Severity now: S1; unchanged · effort: S (SQL overrides) / M (byte budget) / M-L (blob offload, compaction) · breaking: none for the overrides and an opt-in byte budget; behaviour/wire for blob offload (readers receive url parts instead of raw) · theme: memory-payload

**On `main`:** All four sub-claims hold: count-bounded queues with no byte budget, base64 payloads inside the task row, SQL sends still O(history) twice, and an event log that grows without bound for a long-lived task.

**Open:** Byte budget, blob offload, live-log compaction (decisions); SQL save_appending_history overrides (no decision needed).

**Options:** (A) HandlerLimits::max_buffered_bytes: a process-wide semaphore over in-flight event bytes, Overloaded when exceeded. Additive, M. (B) A BlobStore trait with ArtifactOffload { threshold } rewriting raw parts to url parts before persistence and broadcast. M-L; readers then need fetch + auth for the url, and provenance signatures must cover url + digest. (C) RetentionPolicy::max_events_per_live_task with compaction to a snapshot event. M; a resume from before the compaction point must get streamLagged/the snapshot, not a silent gap. Recommendation: ship the SQL overrides now, then A; B and C via a NEW ADR.

Live probe: none to settle. To size: swarm-scale Finding 7's ageing probe on PostgreSQL before and after the override.

Evidence: [`h-designs.json`](h-designs.json).

<a id="h14"></a>
## H14

**Row:** Push delivery is not durable

Severity now: S2; unchanged · effort: XS (docs) / S-M (turn-end outbox) / M-L (full outbox) · breaking: none (opt-in); delivery semantics change to at-least-once where enabled · theme: push-durability

**On `main`:** Push delivery is best-effort and in-memory; a crash loses queued notifications, terminal ones included, with no record.

**Open:** All of it.

**Options:** (A) Transactional outbox: a PushOutbox row written with the status delta, drained by a per-replica SKIP LOCKED worker, at-least-once with seq for receiver deduplication. M-L, SQL stores only, one row per event per config. (B) Durable only for turn-ending events (terminal or interrupted): one row per turn end. S-M; covers the 'silently miss the terminal event' case the report names. (C) Best-effort, documented: receivers reconcile on start and periodically with GetTask/ListTasks or SubscribeToTask from Last-Event-ID. Recommendation: C now, B next, A only if adopters need every intermediate event.

Live probe: none to settle. To prove a fix: a webhook receiver that delays 10 s, kill -9 the server after the terminal status is stored, restart; today no terminal webhook arrives.

Evidence: [`h-designs.json`](h-designs.json).

<a id="h16"></a>
## H16

**Row:** The rate limiter fails closed above 10k distinct callers by default

Severity now: S2; unchanged (the lockout is worse than the report says: indefinite, not one window) · effort: XS-S (overflow bucket) / M (sketch limiter) · breaking: behaviour (a documented fail-closed default changes) · theme: rate-limit

**On `main`:** Still fail-closed at 10,000 tracked callers; an incumbent set of 10k callers active each window locks every new caller out for as long as they stay active, and the operator cannot tell this refusal from an ordinary rate limit.

**Open:** The policy decision, a distinct signal for cap refusals, and production docs.

**Options:** (A) Overflow bucket: when the map is full, a caller without a bucket is charged to a shared 'overflow' key with its own budget (like the existing 'anon' key) instead of being refused. Bounded memory, no lockout, coarser fairness for the overflow population. XS-S. (B) A sketch-based limiter (count-min or approximate sliding log): bounded memory, no cliff, approximate (collisions over-count and can refuse innocent callers at high load). M. (C) Raise or scale max_buckets with memory and document it, or point shared services at the shared counter: simple, keeps the cliff, and lengthens the O(n) eviction scan under the write lock. Recommendation: A, with a metric for overflow use; B only if A's shared budget proves too coarse.

Live probe: none

Evidence: [`h-designs.json`](h-designs.json).

<a id="h17"></a>
## H17

**Row:** The tool loop has no shared inference concurrency limit or 429 backoff

Severity now: S2; unchanged (mitigated: the failure is now retryable) · effort: S (semaphore decorator) / S-M (retry with budget) · breaking: none (new decorator, new ModelError variant on a #[non_exhaustive] enum) · theme: tool-loop-inference

**On `main`:** No shared inference concurrency limit and no in-loop backoff; a 429 still fails the whole task, now as Transient, so the A2A caller retries the whole task and re-spends the tokens already used.

**Open:** Shared concurrency limit; Retry-After-aware retry inside the remaining budget.

**Options:** (A) OpenAiChatModel::with_concurrency(Arc<Semaphore>) and with_retry(policy): provider-specific. (B) A ToolModel decorator (e.g. Limited<M>) holding a shared semaphore and a retry policy that honours Retry-After from a new ModelError::RateLimited { retry_after, .. } variant and stops when the BudgetHandle's remaining time is shorter than the wait: works for every ToolModel, the wait is charged to the deadline automatically, and should be recorded as a span event. (C) Leave it to a gateway (LiteLLM, Envoy) and rely on Transient plus caller retry. Recommendation: B.

Live probe: none to settle. To prove a fix: llama-server --parallel 1 (or a mock answering 429 with Retry-After: 2) and 20 concurrent tool-loop tasks; count provider calls, waits and task outcomes.

Evidence: [`h-designs.json`](h-designs.json).

<a id="h18"></a>
## H18

**Row:** Ergonomics: wiring a child into the parent's context is manual

Severity now: S2; unchanged · effort: S (combined constructor) / M (options builder + SDK helper) · breaking: none if added; api if the start_* matrix is later deprecated · theme: delegation-ergonomics

**On `main`:** Wiring a child into the parent's context is still manual, and the start_* constructors do not compose: journal, lease and budget cannot be combined in one call.

**Open:** A single-child helper or options builder; the journal x lease x budget combinations; lease restoration on reattach.

**Options:** (A) A context-level helper in a2a-protocol-sdk, which depends on client and server: ctx.delegate(&client, params).share(f).lease(ttl).on_behalf_of_caller().journal(j).start(), with wait() bound to ctx.cancellation_token. S-M. (B) A DelegationOptions builder in the client crate replacing the start_* matrix (lease, budget, journal, resume, id_wait), with A as SDK sugar over it. M; consolidates the API. (C) Docs and recipes only. Recommendation: B (it also closes the journal x lease x budget gap), then A; fan-out stays ADR 0033's DelegationGroup.

Live probe: none

Evidence: [`h-designs.json`](h-designs.json).

<a id="h19"></a>
## H19

**Row:** Swarm controls are Rust-only, so polyglot trees lose control at the first non-Rust hop

Severity now: S1 for polyglot fleets; unchanged · effort: XS (docs) / S per extension spec / M (shims + ITK cases) / L (language bindings) · breaking: none · theme: polyglot-extensions

**On `main`:** Trace context crosses non-Rust hops by standard headers; lease, budget, idempotency, failure class and approval are ignored by non-Rust peers; halts never cross a process boundary outside a shared halt store; cancel crosses exactly one hop.

**Open:** Published extension specs, reference shims for other SDKs, interop tests.

**Options:** (A) Publish each extension as a stand-alone spec page with JSON Schema and conformance vectors, and send the upstream drafts (budget exists; lease and lineage to write). S per extension; outward-facing, needs the maintainer's say-so. (B) Reference middleware for a2a-python and a2a-go (~200 lines each: read the budget and cap the timeout, honour lease renewal, cascade cancel) plus ITK interop cases. M. (C) Language bindings (pyo3 wheel of Delegation and CardCache). L, a product of its own. Recommendation: A then B; C out of scope.

Live probe: Needs the Python a2a-sdk: a Rust supervisor Delegation::start_leased(ttl = 5 s) to an a2a-python agent that sleeps 60 s; kill -9 the supervisor; the Python task is still running at t = 15 s (lease ignored). With OTel on both sides, check that the Python span carries the Rust trace id.

Evidence: [`h-designs.json`](h-designs.json).

<a id="l11"></a>
## L11

**Row:** A short-lived client (the CLI, a batch job) never shows on the client dashboard: each process exports one sample under its own `service.instance.id`, and every panel is a per-series `rate()` that needs two (50 CLI runs: 50 one-sample series, no panel data)

Severity now: low (unchanged) · effort: XS (docs) / S (panel) / M (Collector pipeline + live check) · breaking: none · theme: otel-dashboards

**On `main`:** A long-lived client process appears on all four client panels once it has exported twice inside the 5 m window (about 2 minutes after its first call). A short-lived CLI or batch job exports one point per process under its own random instance, so it shows in the job dropdown but on no panel, and each run adds a new set of histogram series (13 buckets + inf + sum + count per method/peer/error).

**Open:** Choose between docs, panel and Collector fixes (options); separately, per-run series churn from CI/batch clients is unbounded cardinality over time.

**Options:** (1) Docs: say under the client dashboard (not only GenAI) that one-shot processes never appear and to use Tempo traces for them; fix walkthrough.md's 'thin'. XS. (2) Add a client-dashboard panel that works for one-sample series, e.g. calls in range by short-lived processes: sum by (job, server_address) (last_over_time(rpc_client_call_duration_seconds_count[$__range]) and on (job, instance) count_over_time(rpc_client_call_duration_seconds_count[$__range]) == 1). S, no pipeline change, verify live. (3) Collector pipeline for client jobs: drop service.instance.id (transform), cumulativetodelta, then deltatocumulative to fold runs into one long-lived series per job; M, needs a live check and adds Collector state. Recommendation: (1) + (2) now; (3) only if batch clients are a stated target.

Live probe: Observability stack (deploy/observability compose) + hello-agent or deploy-agent: run `OTEL_EXPORTER_OTLP_ENDPOINT=http://localhost:4318 a2a send <url> hi` 50 times; in Prometheus `count by (instance) (rpc_client_call_duration_seconds_count{job="a2a"})` -> 50 series with 1 sample each, client dashboard panels empty; then run a client that loops for 3 minutes -> panels populated. Re-run after the chosen fix. A stable OTEL_RESOURCE_ATTRIBUTES=service.instance.id=cli is NOT a fix to test blindly: each run restarts the counter at its own start, so a series reading 1,1,1 has rate 0 unless Prometheus ingests start times.

Evidence: [`a-l.json`](a-l.json).

<a id="sec-13"></a>
## SEC-13

**Row:** HTTPS webhooks are not pinned

Severity now: low (unchanged); KNOWN · effort: M · breaking: none for (a); behaviour for (b) (proxies that filter CONNECT by domain refuse an IP literal) · theme: push-ssrf

**On `main`:** Unchanged: https webhooks are validated by one lookup and connected by another, so a rebinding name still gets a TCP connection and ClientHello to an internal address:port (blind scan). Since c4404e8 a proxied https delivery is resolved by the proxy, which widens the same gap.

**Open:** Pin https for direct connections; decide what the SSRF boundary is when a proxy is configured.

**Options:** (a) Direct connections: dial the validated SocketAddr for https too, with a per-request resolver override (hyper-util HttpConnector::new_with_resolver returning the pinned address for that host) so SNI and certificate checks still use the hostname; through a proxy, document that the proxy is the SSRF boundary and must enforce its own destination ACL. (b) Also pin through the proxy by sending CONNECT <ip>:<port> with SNI=hostname (breaks proxies that allow by domain). (c) Keep as is and expand the rustdoc to name the blind-connect residue and the proxy case. Recommend (a).

Live probe: Live: a rebinding DNS server (e.g. a small authoritative server answering a public IP first and 127.0.0.1/10.x afterwards, TTL 0) plus a listener on the private address; register https://rebind.<zone>:<port>/ and send an event; observe whether the private listener accepts a TCP connection/ClientHello. Repeat with HTTPS_PROXY set to a Squid on another host to see the proxy-side connect.

Evidence: [`f.json`](f.json).

<a id="sec-17"></a>
## SEC-17

**Row:** A custom `PushSender` gets no delivery-time SSRF guard

Severity now: info (unchanged) · effort: S · breaking: none for (a)/(c); api for (b) · theme: push-ssrf

**On `main`:** A custom PushSender (e.g. reqwest, which follows redirects and re-resolves) still gets only the creation-time static check; the requirement is in the book but not on the trait.

**Open:** Decide where the delivery-time guard lives; at least document the redirect requirement on the trait.

**Options:** (a) Handler runs url_guard::resolve_public_url before send() unless allows_private_urls(), skipped for senders that declare they screen themselves (new trait method, default false; HttpPushSender returns true to avoid a second lookup) - catches a name resolving inward but not a redirect or the sender's own re-resolution. (b) Pass the validated SocketAddr into send() (breaking trait change) so a sender can pin. (c) Rustdoc on the trait: screen with resolve_public_url, connect to the returned address, never follow redirects. Recommend (c) now and (a).

Live probe: none

Evidence: [`f.json`](f.json).

