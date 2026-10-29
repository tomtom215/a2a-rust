# Changelog

All notable changes to a2a-rust are documented in the project's [CHANGELOG.md](https://github.com/tomtom215/a2a-rust/blob/main/CHANGELOG.md).

## Versioning

a2a-rust follows [Semantic Versioning](https://semver.org/):

- **Major** (1.0.0) — Breaking API changes
- **Minor** (0.2.0) — New features; before `1.0` a minor may also carry
  breaking changes, batched and labelled as [STABILITY.md](https://github.com/tomtom215/a2a-rust/blob/main/STABILITY.md)
  requires — see [Upgrading Between Minor Versions](./upgrading.md)
- **Patch** (0.2.1) — Bug fixes, backward compatible, except that a correction
  of a demonstrated deviation from the A2A specification may change observable
  behaviour ([STABILITY.md](https://github.com/tomtom215/a2a-rust/blob/main/STABILITY.md)
  §2)

All four workspace crates share the same version number and are released together.

## Release Process

Releases are triggered by pushing a signed version tag:

```bash
git checkout main && git pull
git tag -s vX.Y.Z -m "Release vX.Y.Z"   # signed; -s implies -a
scripts/verify_tag_signature.sh vX.Y.Z   # the check release.yml will run
git push origin vX.Y.Z
```

The tag must be **signed** (`git tag -s`; required since 2026-10-08), by an SSH
or OpenPGP key listed in
[`.github/release-signers/`](https://github.com/tomtom215/a2a-rust/blob/main/.github/release-signers/README.md)
on `main`, and must point at a commit on `main`. The release workflow refuses
anything else before building. A lightweight tag records no tagger, no date and
no signature, so the tag alone attests nothing about who cut the release; the
workflow runs `git cat-file -t` and stops if it does not print `tag`, and an
annotated tag that is not signed by a listed key is refused as well. Creating a
release through the GitHub web UI produces a lightweight tag.

The [release workflow](https://github.com/tomtom215/a2a-rust/blob/main/.github/workflows/release.yml) automatically:

1. Validates the tag — annotated; on `main` and signed by a listed release key;
   on the release-preparation commit; nothing left under `[Unreleased]`;
   breaking releases within STABILITY.md's cadence — against the crates' and
   `tools/a2a-cli`'s versions, CHANGELOG.md, CITATION.cff, SECURITY.md and the
   provenance manifest
2. Runs CI (tests on Linux, macOS and Windows, clippy, fmt, doc, and an MSRV
   1.94 check) and a security audit (cargo-deny)
3. Packages all crates with SLSA build provenance and a CycloneDX SBOM
   attestation per crate, and checks each `.crate` was built from the tagged
   commit in a clean tree
4. Runs the A2A conformance suite (ACTS) on the tagged commit over JSON-RPC,
   gRPC and HTTP+JSON (`acts.yml`), fails on any failed or errored test, and
   attests the summary onto each `.crate` (predicate type
   `https://a2a-rust.com/attestations/acts-conformance/v1`; verify with
   `gh attestation verify <crate>.crate -R tomtom215/a2a-rust --predicate-type …`).
   The GitHub release, and so the publish, waits on it
5. Runs a publish dry run (`cargo package --workspace`) to verify the packages
   are publishable
6. Creates a GitHub release with notes extracted from CHANGELOG.md and the
   `.crate`, checksum and SBOM files attached. GitHub refuses a release body
   over 125,000 characters, so a section longer than 120,000 is released as its
   Known Limitations, Breaking Changes, Deprecated and Security sections plus a
   link to the whole section at the tag (0.15.0's section is about 289,000)
7. Publishes crates to crates.io in dependency order (behind a manually
   approved `crates-io` environment; it authenticates with crates.io Trusted
   Publishing, falling back to the environment secret)

### Publish Order

```text
a2a-protocol-types → a2a-protocol-server → a2a-protocol-client → a2a-protocol-sdk
```

Topological order over **all** dependency edges: server precedes client
because the client has a versioned dev-dependency on the server, which
`cargo publish` resolves against the crates.io index.

The SLIMRPC binding (`bindings/a2a-protocol-slimrpc`) is versioned
independently and is not part of the tagged release; `release.yml` does not
touch it.

## v0.15.1 (2026-10-26)

A patch release that makes OpenTelemetry export work in more deployments and
say what it is doing. A diff of the public items against v0.15.0 finds only
additions. Two behaviours change, and the second affects any pod that sets
`HTTP_PROXY`; [Upgrading Between Minor Versions](./upgrading.md#patch-releases-with-something-to-do) has both, and
[CHANGELOG.md](https://github.com/tomtom215/a2a-rust/blob/main/CHANGELOG.md)
has every item in full.

- **A quick start per deployment**, in [Observability](../deployment/observability.md):
  a laptop with and without a collector, Docker, Kubernetes, HTTPS with a
  public or private CA and mutual TLS, behind a proxy, Prometheus scraping,
  and stopping a pod.
- **`shutdown_signal()`**: a server that waited on Ctrl-C alone was ended by
  SIGTERM without a flush, and the collector received no spans and no metrics.
- **Proxies, gzip, a console exporter and a Prometheus endpoint** for the
  OTLP export; a startup line saying where each signal goes; a warning when
  an export is lost, with the transport's cause and, where the failure names the
  fix, a hint (an unknown CA, a `401`, a `404`, HTTP to port 4317 or gRPC to
  4318).
- **Behaviour changes**: `Telemetry::install_subscriber` always installs a
  subscriber, so a later `.init()` panics; and the proxy variables now apply to
  the export, so a pod with `HTTP_PROXY` set must list an in-cluster collector
  in `NO_PROXY`.
- **Fixed**: `langfuse-agent` exited 1 with no environment; examples that
  waited on Ctrl-C alone; documentation that was wrong, stale or overstated.

## v0.15.0 (2026-10-25)

A minor release that follows the A2A v1.0.1 specification where the SDK did
not, turns OpenTelemetry instrumentation on by default, and raises the MSRV to
1.94. It carries 26 breaking changes (counted on 2026-10-25 as the top-level
entries of the CHANGELOG's Breaking Changes section);
[Upgrading from 0.14 to 0.15](./upgrading/0.14-to-0.15.md) has the migrations an
existing deployment is most likely to trip over, and
[CHANGELOG.md](https://github.com/tomtom215/a2a-rust/blob/main/CHANGELOG.md)
has every item in full. The GitHub release body carries only the Known
Limitations, Breaking Changes, Deprecated and Security sections, with a link to
the whole section.

- **Stricter about what it accepts.** A client refuses an agent card it fetches
  that does not pass `AgentCard::validate` (a card handed to `from_card` is not
  checked), an object a2a.proto does not allow, and a gRPC message that lacks a
  REQUIRED field. A server refuses to publish a card that carries a credential
  in a URL, refuses a push config whose `authentication` carries no credentials
  (leave `authentication` out for an unauthenticated webhook), and refuses a
  JWT with no `sub` under caller isolation.
- **OpenTelemetry instrumentation is on by default** in the server, the client
  and the SDK (`otel`). Exporting (`Telemetry`, the `Langfuse` preset,
  `init_otlp_pipeline`) moved to a new `otlp` feature, which is default in
  `a2a-protocol-sdk` only: a direct dependency on `a2a-protocol-server` that
  used `Telemetry` asks for `features = ["otlp"]`. The metric catalogue follows
  the OpenTelemetry conventions (`a2a.server.latency` is removed), the server
  records `a2a.server.executor.duration`, and the client records each call as it
  ended for the caller: a call the caller dropped is `cancelled`, and an answer
  the client then refuses is a failure.
- **`SubscribeToTask`** reads a long replay in pages and reports a gap it
  cannot replay with `streamLagged`; it has no idle bound by default.
- **A caller is named by how it authenticated** (`bearer:`, `api-key:`,
  `jwt:`), and `build()` refuses a card whose declared authentication the
  interceptors do not enforce.
- **HTTP+JSON answers `application/a2a+json` when the request states no
  preference**; `Accept` still decides when it names either JSON type.
- **The SQLite stores overwrite deleted content with zeros in the database
  file.** They do not in the write-ahead log or in a backup.
- **Conformance:** four requirements of the A2A v1.0.1 ledger are only partly
  met; the ledger says which and what each needs.
- **Breaking, also:** the MSRV is 1.94 and the SQL stores use `sqlx` 0.9; every
  agent card declares the budget extension; `TaskState`'s discriminants are its
  proto numbers; `TaskStore::save_appending_history` takes a `HistoryLimit`.
- **Known limitation:** several replicas sharing a store need session affinity;
  one replica, or several with affinity, is the supported topology for 0.15
  ([Running More Than One Replica](../deployment/horizontal-scaling.md)).
- **Deprecated:** `sign_agent_card` (use `sign_card_es256`; it now refuses a
  `None` key id, and it is removed in 0.16.0),
  `HotReloadAgentCardHandler::update` (use `try_update`),
  `ClientConfig::preferred_bindings` (use `ClientBuilder::from_card_preferring`),
  and `OAuthFlows::Implicit` and `Password`, which still parse and serialize.
- **Security, visible on upgrade:** `JwtValidator` refuses an HS256 secret
  shorter than 32 bytes, so a deployment configured with a shorter one stops
  accepting HS256 tokens; a server whose stores cannot isolate tenants refuses
  requests that name a tenant.

## v0.14.1 (2026-09-30)

A patch release for one security advisory. No API changes. Full detail in
[CHANGELOG.md](https://github.com/tomtom215/a2a-rust/blob/main/CHANGELOG.md).

- **`PathSegmentTenantResolver` takes the tenant from the URL only
  (GHSA-hr9h-6jvf-wvg6).** From 0.7.0 through 0.14.0 it fell back to a
  `path` request header, which any client can send, so on JSON-RPC,
  HTTP+JSON, gRPC and the Axum adapter a caller could choose its own tenant.
  On gRPC and the Axum adapter, which carry no tenant in the URL, it now
  resolves none: use `BearerTokenTenantResolver` or `HeaderTenantResolver`
  there.
- Internal: the vendored SLIMRPC specification follows upstream's
  2026-09-29 merge (the binding is unchanged), and two intermittent server
  tests are fixed.

## v0.14.0 (2026-09-26)

A minor release of fixes, most of them found by an audit of drop and
cancellation paths and by the A2A project's two official conformance suites.
Carries seven breaking changes and ten behaviour changes. It ships in the
same calendar month as 0.13.0 under a declared cadence exception
([STABILITY.md](https://github.com/tomtom215/a2a-rust/blob/main/STABILITY.md)
§3), because the fixes can't wait. Full detail in
[CHANGELOG.md](https://github.com/tomtom215/a2a-rust/blob/main/CHANGELOG.md).

- **Tasks no longer wedge when a client goes away.** A send dropped mid-commit
  used to leave its task stuck; a blocking send whose client left left it
  `working`; a failing `after` hook orphaned it. Each now reaches a recorded
  state.
- **A refused credential answers the status a client acts on**: HTTP `401`
  with `WWW-Authenticate`, or `403`, and gRPC and SLIMRPC `UNAUTHENTICATED` or
  `PERMISSION_DENIED`, instead of `400` everywhere
  ([ADR 0014](https://github.com/tomtom215/a2a-rust/blob/main/docs/adr/0014-auth-rejection-status.md)).
  This SDK's client drops a revoked token on it.
- **Conformance fixes from the official suites**: message parts in media types
  the card does not declare are refused (opt out with
  `allow_undeclared_input_modes()`), JSON that is not a JSON-RPC request is
  `-32600`, and every task status carries a timestamp. Against ACTS every MUST
  passes on JSON-RPC, gRPC and HTTP+JSON; its one failure is a SHOULD this SDK
  deliberately does not follow (see
  [Conformance History](./conformance-history.md#deliberate-deviations)).
- **WebSocket is as robust as the HTTP bindings**: the client reconnects after
  a dropped socket, a slow reader no longer stalls other calls on the
  connection, and the server closes a peer that stops reading.
- **`ServerInterceptor::on_complete`** runs once per call with its outcome, so
  cleanup survives errors and disconnects; **spans for every inbound call** on
  every binding, and `tracing` on by default.
- **The published manifests no longer admit dependency versions with a RustSec
  advisory**, and a CI gate keeps it that way.
- **Breaking:** seven items. `tracing` is a default feature;
  `default-features = false` on the SDK removes rustls; a dropped peer reads
  the same on every binding; gRPC `UNAUTHENTICATED`/`PERMISSION_DENIED` are
  `UnexpectedStatus` 401/403 and a peer's `CANCELLED` is not retried;
  `ClientBuilder::from_card` refuses a card with no A2A 1.x interface; graceful
  shutdown ends in-flight tasks before it drains; and the shipped `TaskStore`s
  refuse to move a terminal task to another state. See
  [Upgrading from 0.13 to 0.14](./upgrading/0.13-to-0.14.md), which has a migration
  for each, and for the ten behaviour changes.

## v0.13.0 (2026-09-20)

A minor release that makes the event log the record, and spends it on stream
resumption. Carries nine deliberate breaking changes, batched as
[STABILITY.md](https://github.com/tomtom215/a2a-rust/blob/main/STABILITY.md)
requires. This page said three until 2026-09-23; eight were labelled in the
changelog at release, and the ninth, the `#[non_exhaustive]` marking of
`RetentionPolicy` and `PurgeReport`, shipped under `[Unreleased]` — see the
correction at the top of CHANGELOG.md's 0.13.0 section. Full detail in
[CHANGELOG.md](https://github.com/tomtom215/a2a-rust/blob/main/CHANGELOG.md).

- **An append-only event log, beside the snapshot.** A task's state is a fold,
  and until now the fold was the only thing kept — so a wrong fold had nothing
  to be checked against, which is how
  [#130](https://github.com/tomtom215/a2a-rust/issues/130) went unobserved.
  `TaskStore` gains `supports_event_log`, `append_event`, `last_event_seq` and
  `read_events`, implemented by every store this crate ships. The snapshot
  stays authoritative for reads; this is not event sourcing.
- **SSE frames carry an `id:`, and `Last-Event-ID` resumes from it.** A client
  that was disconnected sends back the position of the last frame it saw and
  receives exactly what it missed, rather than a snapshot it has to diff. The
  position is assigned once, where the event fans out to both the log and the
  stream, so the `id:` on the wire and the `seq` in the store are the same
  number by construction.
- **An executor can see who called it.** `RequestContext` gains a
  `call_context`, carrying caller identity, tenant, activated extensions and
  inbound headers across the `tokio::spawn` that task-locals do not survive.
- **Idempotency keys on `message/send`, as an SDK extension.** A send whose
  connection drops is ambiguous — the task may exist or it may not — and
  `message/send` creates server-side state, so a client is right to refuse the
  retry. A key the server deduplicates on makes that retry safe: the same
  message replays to the first task in whatever state it reached, and a
  *different* message on the same key is refused rather than silently answered.
  Keys are tenant-scoped, every bundled store honours them, and the card
  advertises support only when the store actually has it. This is **not part of
  A2A v1.0** — it ships as `https://a2a-rust.com/extensions/idempotency/v1`, so
  a server that enables it stays conformant. See
  [Idempotent Sends](../client/idempotency.md).
- **W3C trace context crosses an A2A hop**, so a delegation chain is one trace
  rather than several unrelated span trees.
- **A failed task says why**, as a `FailureClass` a caller can `match` on
  instead of parsing English out of a message.
- **A conformance harness for `AgentExecutor`**, behind the `conformance`
  feature: thirteen checks an implementation can run against itself.
- **Breaking:** nine items. `RequestContext`, `IdempotencyClaim`, `KeyError`,
  `RetentionPolicy` and `PurgeReport` are `#[non_exhaustive]`; `EventQueueReader::read` yields a
  `StreamEvent` carrying the log position; `FailureClass::ALL` is a slice
  rather than a fixed-size array; `PurgeReport::journal_orphans_deleted` is
  now `orphan_rows_deleted`; `build()` refuses a signed agent card it would
  otherwise have to edit; a keyed `message/send` is retried only against a
  peer that advertises the extension; and `message.id` is validated at
  ingress. See
  [Upgrading from 0.12 to 0.13](./upgrading/0.12-to-0.13.md), which has a migration
  for each.

## v0.12.1 (2026-09-17)

A patch release: one specification correction, one security advisory, and
documentation fixes. No API changes. Full detail in
[CHANGELOG.md](https://github.com/tomtom215/a2a-rust/blob/main/CHANGELOG.md).

- **A new task on an existing context no longer inherits the previous task's
  artifacts, history and metadata**
  ([#130](https://github.com/tomtom215/a2a-rust/issues/130)). When a client
  sent a second message on a context without a `taskId`, the server correctly
  minted a new task id and then built that task from the previous task's
  state, so every round returned the whole context's accumulated artifacts,
  growing by one each turn. `a2a.proto` scopes all three fields to the task.
  State now carries forward only when the ids match — the `input-required`
  continuation (spec §3.4.3) the carry-forward was written for in v0.6.0,
  where wiping accumulated state would be the bug. Affects 0.11.0 and 0.12.0.
- **`rustls` advanced to 0.23.45**
  ([RUSTSEC-2026-0285](https://rustsec.org/advisories/RUSTSEC-2026-0285)):
  rustls accepted TLS 1.3 handshake messages sent at the wrong encryption
  level when they followed a key-changing message in the same record,
  contrary to RFC 8446 §5.1. The handshake transcript stays authenticated, so
  this is a conformance failure rather than a forgery or confidentiality
  break. The SLIMRPC binding cannot take the upgrade — an upstream
  `aws-lc-rs` pin forecloses it — and carries a documented, dated waiver
  instead; the four published crates use the `ring` backend and are on
  0.23.45.
- **Documentation corrections**: three wrong claims about the `rig-agent`
  example, a TCK figure carried in seven places that measurement
  contradicted, and six table rows rendering short of their declared columns.

## v0.12.0 (2026-09-10)

The deliberate breaking minor before the two clean minors
[STABILITY.md](https://github.com/tomtom215/a2a-rust/blob/main/STABILITY.md)
requires; the migration for every item is in
[Upgrading from 0.11 to 0.12](./upgrading/0.11-to-0.12.md). (0.13.0, 0.14.0 and 0.15.0
each carried breaking changes too, so those two clean minors have not yet
started.) Highlights — see
[CHANGELOG.md](https://github.com/tomtom215/a2a-rust/blob/main/CHANGELOG.md)
for the full list (the Gates item also draws on ROADMAP.md's 0.12 table):

- **Fifteen public configuration structs are `#[non_exhaustive]`** with a
  `with_*` setter for every field, so a field can be added later without a
  break; `HandlerLimits` gains `push_delivery_budget` and
  `executor_drain_timeout`, the client's `WebSocketTransportConfig` gains
  `max_pending_requests`.
- **Bounds that were missing** — a drain that never closes, the push
  delivery budget, DNS lookups in the push sender, in-flight WebSocket
  requests, served gRPC connections (`with_max_connections`), and the
  SLIMRPC binding's slow-consumer stall.
- **Every dependency refreshed**, and the weekly mutation sweep's survivors
  addressed: the three proved equivalent carry `#[mutants::skip]`.
- **An executor's error text reaches a blocking caller** on the Failed
  task's status message; **the blocking path reports every push outcome**;
  **one deadline per token request and per stream start** in the client.
- **Adoption**: `tools/a2a-cli` (unpublished), a Rust worker in
  `multi-lang-team`, `examples/resilient-agent`, the upgrade guide, and
  `AgentCard::with_streaming`, `CardFetchOptions`,
  `ClientBuilder::chosen_interface`.
- **Gates**: reachability, timeout nesting, inert knobs, the reverse
  API-reference check, the weekly codecov-ignore check; DCO on every push
  to `main`; cargo-semver-checks over all four crates with all features;
  the toolchain action pinned by SHA.

## v0.7.0 to v0.11.0

Not summarised here; each release's section in
[CHANGELOG.md](https://github.com/tomtom215/a2a-rust/blob/main/CHANGELOG.md)
is the record, and the breaking items are in
[Upgrading Between Minor Versions](./upgrading.md). (This page carried a
"v0.7.0 upcoming" section until 2026-09-10, four releases after 0.7.0
shipped — the decay the sitemap and API-reference gates exist to catch, and
which nothing checks for this page.)

## Releases 0.2 to 0.6

Summarised on [Releases 0.2 to 0.6](./changelog/older.md).
