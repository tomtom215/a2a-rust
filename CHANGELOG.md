<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Note on crate versions in this section

`a2a-protocol-server`'s `Cargo.toml` is at **0.8.0** while
`a2a-protocol-types`/`a2a-protocol-client`/`a2a-protocol-sdk` remain at 0.7.0,
ahead of the lockstep bump `RELEASING.md` otherwise calls for. This is
deliberate, not a mismatch to fix: `cargo-semver-checks` (which compares
against the last crates.io release) correctly flagged two breaking changes in
this section — `InMemoryQueueReader`/`SendMessageResult` losing
`UnwindSafe`/`RefUnwindSafe` (the streaming reattach hook below adds a
`dyn Fn` field) and `HandlerLimits` gaining two `pub` fields (the same fix's
reattach/idle bounds) — and per Rust's 0.x convention a breaking change bumps
the minor position. Bumping only the crate that actually broke, rather than
all four in lockstep, keeps that check meaningful without pre-deciding the
next release's version for crates it found no issue with. The next real
release should reconcile all four to whatever version it cuts, per the
existing `RELEASING.md` checklist.

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
- Cargo-fuzz target for JSON deserialization of all major protocol types.
- `docs/implementation/plan.md` documenting planned beyond-spec extensions (request IDs,
  metrics, rate limiting, WebSocket, multi-tenancy, persistent store).
- Pitfalls catalog (`book/src/reference/pitfalls.md`) with entries for serde,
  hyper, SSE, push notifications, async/tokio, workspace, and testing gotchas.

### Changed

- Eliminated unnecessary `serde_json::Value` clones in 8 client methods by
  moving the value into `ClientResponse` and extracting it after interceptors run.

- **Breaking:** `AgentExecutor` trait is now object-safe — methods return
  `Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>>` instead of
  `impl Future`. This eliminates the generic parameter `E: AgentExecutor` from
  `RequestHandler`, `RequestHandlerBuilder`, `JsonRpcDispatcher`, and
  `RestDispatcher`, enabling dynamic dispatch via `Arc<dyn AgentExecutor>`.
- `InMemoryTaskStore` now performs TTL-based eviction of terminal tasks (default
  1 hour) and enforces a maximum capacity (default 10,000 tasks).

### Fixed

- Invalid state transitions (e.g. Submitted → Completed) are now rejected with `InvalidStateTransition` error.
- Push notification delivery now properly times out instead of hanging indefinitely.

### Removed

- (Nothing removed — this is the initial release.)
