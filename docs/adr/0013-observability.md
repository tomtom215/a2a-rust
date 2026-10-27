<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# ADR 0013: Spans, metrics and one telemetry entry point

**Date:** 2026-09-23
**Status:** Accepted — implemented as `otel::Telemetry` (2026-10-06); recorded
2026-10-18, when the metric catalogue moved to UCUM units and
`a2a.server.request.duration`
**Author:** Tom F.

---

## Context

The 2026-09-22 adopter audit (`docs/adopter-audit-2026-09-22.md`, section 1)
found observability absent where adopters look for it first:

- **No spans exist in any crate** (O1). A grep for `span!`, `info_span`,
  `#[instrument]`, `.instrument(` or `Span::current` over `crates/` returned
  nothing.
- **The span id sent downstream belongs to no recorded span** (O2). The
  server mints a child span id (`handler/helpers.rs`, `fresh_span_id`) and
  propagates it, so every downstream agent's trace points at a parent no
  backend has.
- **Work runs on `tokio::spawn` with no span** (O4), so even a user's own
  outer span does not parent the executor.
- **The latency histogram records seconds into millisecond buckets** (O5),
  and its names are not the semantic conventions'.
- **OTLP setup covers part of `OTEL_*`**, is gRPC-only, lets the argument
  override `OTEL_SERVICE_NAME`, and nothing flushes on shutdown (O12).
- The client records nothing (O8), and outbound `traceparent` is opt-in
  twice over (O9).

Trace context *propagation* is correct and tested (W3C, all nine binding
pairs against a2a-go when opted in). What is missing is recording.

The semantic conventions this ADR commits to were read from the
`open-telemetry/semantic-conventions` repository at `838e414`
(2026-09-22), not from memory:

- `rpc.server.call.duration`: histogram, unit `s`, stability
  release-candidate, advisory buckets
  `[0.005, 0.01, 0.025, 0.05, 0.075, 0.1, 0.25, 0.5, 0.75, 1, 2.5, 5, 7.5, 10]`.
  *Changed 2026-10-24:* at `55c98cf` the conventions advise
  `[0.0001, 0.0005, 0.001, 0.005, 0.01, 0.05, 0.1, 0.5, 1, 5, 10, 50, 100]`
  for both `rpc.server.call.duration` and `rpc.client.call.duration`, and
  both histograms use it from 0.15. The old set's top bucket was 10 s, so
  a dashboard's p95 of agent calls that wait on a model read 10 s whatever
  they took.
- RPC server span: kind `SERVER`, named `{rpc.method}`; `rpc.system.name`
  required; `rpc.method` the fully-qualified logical method, or `_OTHER`
  for one the server does not recognise; `error.type` required on failure.
- `rpc.system.name` lists `grpc` and `jsonrpc` among its values and allows
  custom ones ("If one of them applies, then the respective value MUST be
  used; otherwise, a custom value MAY be used").

Re-read upstream on 2026-09-23 while implementing (`docs/rpc/rpc-metrics.md`,
`rpc-spans.md`, `grpc.md`, `json-rpc.md` on `main`), which added three rules
the list above lacks, all implemented:

- The status attribute is `rpc.status_code` — Required for gRPC (the status
  name, `OK` included), "when available" for JSON-RPC (the error code as a
  string). On failure `error.type` "SHOULD be set to that status code".
- A span whose `rpc.method` is `_OTHER` is named `{rpc.system.name}`, and a
  JSON-RPC server that maps a method to `_OTHER` "MUST also set
  `rpc.method_original`" — here cut to 128 bytes, since the peer chooses it.
- For JSON-RPC `rpc.method` is Opt-In, because a general-purpose
  instrumentation cannot bound it. This server can — it serves eleven
  methods and maps the rest to `_OTHER` — so it sets it.

## Options

### 1. What records spans

**(a) `tracing` spans, exported through `tracing-opentelemetry`.** The
crates already log through `tracing` (behind the `tracing` feature); spans
become part of the same instrumentation, and an executor's own
`tracing::info!` inherits the request's span and trace id with no extra
work. `tracing-opentelemetry` 0.33.0 depends on `opentelemetry ^0.32`, the
version the workspace locks (checked against the crates.io index), with
`rust-version` 1.75, below the 1.88 MSRV.

**(b) The `opentelemetry` API directly.** No bridge, exact control of span
ids. But an executor's logs would not correlate with the spans, and every
adopter who already runs a `tracing` subscriber — the Rust default — would
have two unconnected systems.

**(c) Both, behind separate features.** Twice the instrumentation to keep
consistent, for no case (a) cannot serve.

**Chosen: (a).** Spans are `tracing` spans and compile to nothing without
the `tracing` feature, exactly as the log macros do today. The `otel`
feature turns on `tracing` and adds the bridge.

### 2. What the server span is on each binding

**(a) An RPC `SERVER` span per A2A method on every binding**, named
`lf.a2a.v1.A2AService/{Method}` — the fully-qualified method the gRPC
binding already serves — with `rpc.system.name` `grpc`, `jsonrpc` (the
JSON-RPC and WebSocket bindings), and the custom value `a2a_http_json` for
the REST binding, which the semantic conventions' RPC list does not cover.

**(b) An HTTP server span for REST**, with `http.*` attributes and a child
`INTERNAL` span for the A2A method. Truer to the HTTP conventions, but the
same A2A call then has a different span shape on each binding, and a
dashboard grouping by method has to know which.

**Chosen: (a)**, with `http.request.method` and `http.route` added on the
REST span so HTTP-oriented tooling still finds them. One span shape per A2A
call, whatever the binding, is what makes a cross-binding, cross-language
trace comparable.

### 3. Which span id goes downstream (O2)

The inbound `traceparent` becomes the server span's remote parent; the
downstream `traceparent` carries **the recorded span's own id**, read back
from the span, not a separately minted one. When no subscriber records spans
(the `tracing` feature off, or no OpenTelemetry layer installed), the
current minting stays: there is nothing to point at either way, and trace
ids still join the chain. `InboundTracePolicy` applies unchanged.

### 4. Metrics

`rpc.server.call.duration` is added with the advisory buckets above. The
existing `a2a.server.*` instruments stay, documented as deprecated, and are
removed no earlier than the next minor after the one that adds their
replacements — `STABILITY.md` §3's deprecate-first rule, applied to a
catalogue the book publishes as a contract. Task outcome, stream duration and
active streams are added as new instruments.

### 5. One entry point

`init_telemetry()` reads the `OTEL_*` environment as the OpenTelemetry
specification defines it — protocol (gRPC or HTTP/protobuf), endpoint,
headers, timeout, service name and resource attributes, with the
environment winning over defaults — installs tracer and meter providers,
and returns a guard whose drop, or explicit `shutdown`, flushes both. The
existing `init_otlp_pipeline*` functions stay, deprecated.

## Consequences

- The gate is built first: an end-to-end test with an in-memory span
  exporter and a manual metric reader that drives JSON-RPC, REST and gRPC
  calls and asserts the span tree (remote parent, executor child, downstream
  span id recorded) and every catalogued instrument. It fails on `main` on
  the day this ADR is written; each step of the work turns part of it green.
- `tracing` spans cost nothing when the feature is off. When it is on with
  nothing installed, a send cost 4.6% and 6.6% more in two runs
  (`docs/readiness-bar.md`, 2026-10), which retracted the earlier "a level
  check"; the cost with an exporter installed is measured before and after,
  not asserted.

  **Measured 2026-09-24**, a blocking `SendMessage` round trip over
  loopback to `serve_with_addr` with a trivial executor (criterion, bench
  profile, 4-vCPU Xeon at 2.1 GHz, two tokio workers; 15 s per point, and the
  before tree re-run to bound drift, which was within ±5%). Before is this
  branch without phase 2, after is with it; about 175 µs a call before.
  With no subscriber: JSON-RPC +1 to +5%, within the noise; HTTP+JSON +8 to
  +10%, which experiments could not attribute below that noise. With
  `OtelMetrics` alone: 0 to +15%. With a `tracing-opentelemetry` layer over a
  batch processor as well: +55 to +63%, about 100 µs a call, almost all of it
  the bridge recording the call's two spans; the before tree, which has no
  spans to record, pays nothing for the same layer. The first cut of this
  work cost +36% and +46% with no subscriber: the connection counter wrapped
  every response body, which made hyper send each fixed-size response
  chunked, and each call's future was boxed. Both were fixed before commit
  and the fix measured.
- Adding dependencies: `tracing-opentelemetry`, and the `trace` and
  `http-proto` features of the OpenTelemetry crates already in use. Each is
  checked against `deny.toml` before it lands.

## Open questions

- ~~Whether `tracing` becomes a default feature of the server and SDK
  crates.~~ **Decided 2026-09-23 by the maintainer: yes, with `tracing`'s own
  default features.** Measured before deciding: `tracing` was already in every
  default build's dependency graph through the HTTP stack, so the feature adds
  `tracing-attributes` and `syn`. The client takes it by default too: its
  failure paths report through `tracing` like the server's, and a default
  client build that dropped them was O13's other half. The SDK now takes the
  client and server without their own defaults (audit K1), so
  `default-features = false` on any of the three removes it again.

## 2026-10-06: option 5, the agent conventions, and the client

Implemented on `claude/zealous-lovelace-n5ulfc`, with what was decided and
why, so the next change to any of it starts from the reasons.

**Option 5 as written, with two departures.** `otel::Telemetry` reads the
environment as §5 says, installs both providers and returns a flushing
guard, and adds logs (`opentelemetry-appender-tracing` 0.32). The first
departure is a private `current_thread` runtime for export: tonic needs a
runtime to build its channel (the panic `init_otlp_pipeline` documents), and
exporting on the application's runtime deadlocks a `current_thread`
application at shutdown, because the final flush blocks the only thread the
export would run on. The second is an HTTP client of our own
(`otel/telemetry/http.rs`): `opentelemetry-otlp` offers reqwest, which
`deny.toml` bans, or `opentelemetry-http`'s hyper client, which uses Tokio's
timer wherever it is polled — and the SDK polls it on its batch thread, which
has no runtime. Ours spawns each request onto the export runtime.

Found while building it, each now covered by a test:

- `Option<Layer>::max_level_hint` is `OFF` for `None` (tracing-subscriber
  0.3.23). Composed as an `Option`, a signal that was off — logs, under the
  Langfuse preset — switched every span off. Absent signals are `Identity`.
- `tracing-opentelemetry` 0.33.0 appends a field recorded twice as a second
  attribute with the same key. The executor span's `a2a.task.state` first read
  `TASK_STATE_WORKING` on a completed task. Every attribute is now recorded
  once, at the point its value is final, and the SDK's gate fails on a
  repeated key.
- `opentelemetry-otlp` 0.32 builds an empty `ClientTlsConfig` for an
  `https://` gRPC endpoint, and tonic 0.14.6 enables no roots for an empty
  one. `Telemetry` passes the Mozilla roots and any extra CA explicitly.
- `opentelemetry_sdk` 0.32.1's batch processor computes its final export's
  result and returns `Ok` from `shutdown` regardless. The HTTP client counts
  failed and refused exports, and `shutdown` reports them; over gRPC a
  failure is still only logged.
  *Superseded 2026-10-12:* `opentelemetry-otlp` 0.33 retries a failed export
  by default, so counting refused responses reported an export delivered on
  retry as lost. `Telemetry` now counts exports the exporter gave up on, on
  both transports (`otel/telemetry/counting.rs`); 0.33.1's `shutdown` still
  returns `Ok` regardless.

**The executor's span is the GenAI conventions' `invoke_agent`** (internal,
`semantic-conventions-genai` `docs/gen-ai/gen-ai-agent-spans.md` at
`4f85037`; `development`). The executor is the agent, and the span covers one
run of it in this process. Langfuse maps `gen_ai.operation.name =
invoke_agent` to an AGENT observation and `gen_ai.conversation.id` to a
session (`langfuse/langfuse@1a21a42`, `ObservationTypeMapper.ts`,
`extractSessionId`), and so do other GenAI-aware backends. That is the whole
of the Langfuse integration: no vendor attributes. The cost is a renamed span
(`a2a.execute` before), listed under Behaviour Changes, with
`with_agent_span_conventions(false)` as the way back.

**The call spans carry the draft A2A conventions' attributes** (pull request
#195 at `842a839`, open). Its rule that A2A instrumentation "SHOULD NOT report
telemetry describing higher level GenAI agent operations" is kept for the
`SERVER` and `CLIENT` spans, which carry A2A attributes only. Its rename of
HTTP server spans to `{a2a.method.name}` is not followed: option 2's single
name per call on every binding stands, as #195's own gRPC rule has it, until
the conventions settle. Content capture is opt-in, as the GenAI conventions
make `gen_ai.input.messages` and `gen_ai.output.messages`.

**The client records a `CLIENT` span per call and sends it as the
`traceparent`** when the `otel` feature is on and a layer records (O8, O9).
The span is opened after the interceptors run, so it overrides what
`TracePropagationInterceptor` wrote; a `CurrentTrace` scope becomes its parent
when no recorded span is current. A streaming call's span is polled with the
stream, because `tracing-opentelemetry` ends a span at its last exit.
Propagation is skipped on a custom transport, where per-request headers may
not exist (the WebSocket transport warns when it drops one).

**Measured 2026-10-06**: a blocking JSON-RPC `SendMessage` round trip over
loopback to `serve_with_addr`, trivial executor, client and server in one
process on a two-worker runtime. Release profile, 4-vCPU Xeon at 2.1 GHz.
`main` at `20b162b` against this branch, 2,000 calls a run, 15 runs a round,
the round's median taken.

- With no subscriber, 15 interleaved rounds (nine before-then-after, six
  after-then-before, to expose order effects): a median of round medians of
  214.9 µs before and 220.3 µs after (+2.5%). Individual rounds ranged from
  −7.4% to +31.7%, and "after" was slower in 9 of 15. That is inside this
  host's run-to-run spread and is not attributed.
- With a `tracing-opentelemetry` layer over a batch processor and a
  discarding exporter, 3 rounds: 323.2 → 388.0 µs (+20%, about 65 µs a call;
  rounds +20.6%, +25.9%, +19.0%). That is the client span, a third span per
  call, plus the new attributes.

The client skips all of its attribute work when its span is disabled, and
the executor span formats its name only when enabled.

## 2026-10-18: bindings outside the server crate

Decision 2 says "every binding", and `a2a-protocol-slimrpc` recorded neither
the span nor `rpc.server.call.duration`: the span type and its per-binding
status mapping are private to `a2a-protocol-server`. Three ways to close it:

1. Make `ServerSpan`, `RpcSystem` and `WireStatus` public. That puts the
   in-crate bindings' internals in the public API.
2. Have SLIMRPC open its own span and record the metric itself. That copies
   the trace policy, the remote parent and the agent attributes, which
   would then drift.
3. A narrow public `BindingSpan`, opened with a `CustomBinding` (the
   binding's `rpc.system.name`, its status for each `ServerError`, and
   optionally a success status), wrapping the private `ServerSpan`.

**Chosen: 3.** SLIMRPC is `slimrpc`, with `server_error_to_rpc_error`'s
code as its status and `OK` on success, as on gRPC, whose codes it uses. On
the client, `Transport::rpc_system` (default `None`) lets a transport given to
`with_custom_transport` name its system on the `CLIENT` span: `jsonrpc` for
WebSocket, as the server records the same call, and `slimrpc`. The builder
reads it before wrapping the transport in retries.

## 2026-10-20: streams, WebSocket, the client's metric, every binary

Closing the audit's remaining gaps under the maintainer's requirement that
OpenTelemetry be wired and on by default on every path, with the exporter
still in `a2a-protocol-sdk` only. Read against
`open-telemetry/semantic-conventions@55c98cf` and
`semantic-conventions-genai@547934a`.

- **A streaming call ends when its stream does** (`rpc_span/stream.rs`).
  `rpc-spans.md`: the span "SHOULD cover the full lifetime of the request
  and/or response streams". The code that forwards a stream claims the call
  while it is set up inside it; a successful call hands itself to the claim
  and is recorded when the stream ends — completed, failed with the error it
  sent, or `cancelled` — re-entering the span so the exported end time
  moves. Every binding claims, SLIMRPC through `BindingSpan::run_with`. The
  gate's check that a call's span ended *before* its SSE writer asserted the
  opposite of the convention, and now asserts it.
- **A WebSocket call links to the upgrade's trace.** A frame carries no
  trace context, and inventing a carrier in `params` would be a private
  protocol extension, which the client's module docs already decline. The
  upgrade's `traceparent` names the operation that opened the connection, so
  each call's span is a root with a span link to it, not its child.
- **The client records `rpc.client.call.duration`** through the global meter
  provider, and its span `rpc.status_code`; with `otel`, a handler given no
  metrics records through `OtelMetrics` on the global provider, so a process
  that installs one gets both sides' metrics with nothing else to do.
- **Every example, the TCK SUT, the ITK agent and the CLI** build
  `Telemetry` from `OTEL_*`, exporting only when
  `OTEL_EXPORTER_OTLP_ENDPOINT` is set.
- **The SQL stores open no `db.*` spans.** sqlx 0.8 emits each statement as
  a `tracing` event (`sqlx-core` 0.8.6 `logger.rs`, `QueryLogger::finish`:
  `db.statement`, `rows_affected`, `elapsed_secs` at `DEBUG`, `WARN` past one
  second), not a span. Wrapping each of the stores' 232 statements, in 27
  files, several recorded over the 500-line limit, was not done; the book's
  "What is not measured" says what an operator gets instead.

## 2026-10-23: export where configured, one instance id per process

Found by pointing the repository's own binaries at a live Prometheus and an
OpenTelemetry Collector (`deploy/observability/`), and by an audit of every
binary's telemetry setup.

- **Every process reports a `service.instance.id`.** `Telemetry` set none
  unless `OTEL_RESOURCE_ATTRIBUTES` named one, so replicas of one service
  exported identical resources; Prometheus maps the attribute to `instance`,
  and CLI runs without one wrote series with no `instance` at all. It now
  adds a random UUID when the environment names none, as the conventions
  recommend and as the Collector does for itself.
- **`with_export_where_configured()`** builds a signal only when an endpoint
  names where it goes: `OTEL_EXPORTER_OTLP_ENDPOINT`, the signal's own
  `OTEL_EXPORTER_OTLP_{SIGNAL}_ENDPOINT`, or code. The seventeen binaries
  that had decided this themselves read only the general variable, so a
  metrics-only endpoint exported nothing; each now calls this instead.
  `install_subscriber()` replaces the registry-and-layer block sixteen of
  them repeated, and lets `hello-agent` keep its one dependency (the SDK
  prelude exports `Telemetry` under the default `otlp` feature).
- **Binaries without telemetry, by decision:** the in-repo TCK runner
  (`tck/src/main.rs`) depends on no SDK crate by design — a conformance kit
  that imported the implementation would share its misreadings — so it has
  no `Telemetry` to build; the MCP tool server
  (`examples/mcp-agent/src/bin/tool_server.rs`) speaks MCP over stdio, not
  A2A, and calls no SDK code. `hello-agent` and `tool-loop-agent` now build
  telemetry; the SLIMRPC binary and example are the binding's own change.

## 2026-10-23: the tool loop's metrics

The audit of every signal against the current conventions found the tool
loop recording spans and no metrics. It now records the `GenAI` metrics
under the names `semantic-conventions-genai` gives them at `9ffc7e0`:
`gen_ai.client.inference.duration` and the two token families for a turn
(not `gen_ai.client.operation.duration`, which those conventions now say
"SHOULD NOT be reported for inference operations"),
`gen_ai.execute_tool.duration` for a call, and `gen_ai.invoke_agent.*` for a
run. Decided with it:

- **Instruments are built at each measurement**, from the meter the loop was
  given or the global provider's as it is then. A model turn takes hundreds
  of milliseconds; building three or four instruments from the SDK's
  registry is not visible beside that, and it is what lets a provider
  installed after the loop was built still receive them, which caching an
  instrument at construction would not.
- **Abandoned work is recorded, as `cancelled`.** A turn or call ends three
  ways: answered, failed, or dropped unfinished. The span and the
  measurements are recorded on drop, so the third is not lost, and it is
  marked rather than left looking successful.
- **Token modality is `text`.** A `ChatRequest` carries text only, so every
  token counted is a text token; were images added, this would have to
  become what the provider reports, or `unknown`.
- **The run's metrics are the loop's, not the executor's.** The executor's
  `invoke_agent` span covers any executor; only the loop knows how many
  turns and calls a run made, and a loop run outside an executor is still
  an agent invocation.

## 2026-10-26: setups the exporter did not reach, and what it did not say

The maintainer's review said OTLP "does not work out of the box and does not
work for every setup", is "not intuitive devex", is not "industry standard or
best practice" and is "not complete". Each part was tested rather than taken as
read: the repository's `hello-agent` was run against a stand-in collector
(a threading HTTP server that decodes the protobuf; 2026-10-26), with the
environment cleared to the variables under test.

**What held.** "Does not work out of the box" did not: with
`OTEL_EXPORTER_OTLP_ENDPOINT` set, traces, metrics and logs arrived with the
right span names and cumulative metrics, and the 32 export tests passed.
Headers, `OTEL_TRACES_SAMPLER` (`always_off` exported no spans), delta
temporality (`OTEL_EXPORTER_OTLP_METRICS_TEMPORALITY_PREFERENCE`), resource
attributes and the log/trace correlation each worked as documented.

**What did not**, each reproduced before it was changed:

| Setup | Before |
|---|---|
| `OTEL_EXPORTER_OTLP_COMPRESSION=gzip` | Ignored: no `Content-Encoding`, the same 6,633-byte body |
| `HTTP_PROXY` with a collector only the proxy can resolve | Ignored: the proxy received nothing |
| A collector that is not there | Nine seconds, no output |
| No endpoint configured | One line, "listening"; no hint how to export |
| `OTEL_TRACES_EXPORTER=console`, `…_METRICS_EXPORTER=prometheus` | Refused (a panic in the examples) |
| Resource | `service.*` and `telemetry.sdk.*` only: no host, OS or process |
| `OTEL_PROPAGATORS=b3` | Accepted and ignored |
| `OTEL_EXPORTER_OTLP_ENDPOINT=127.0.0.1:4318` over HTTP | "invalid format" |

**The cause of the silent ones is one thing.** `opentelemetry-otlp` *warns and
ignores* a value it was not built for (compression, here), through `tracing`;
`Telemetry` builds its exporters before any subscriber exists, and
`install_subscriber` then installed only the exporting layers, which exclude the
exporter's own targets on purpose (an export that logs a failure must not
produce something to export). So the warning, the SDK's per-batch
`ExportError`, hyper's and rustls's messages all went nowhere. The ADR's own
text said `Telemetry` "refuses a value it cannot act on rather than ignoring
it"; for this variable that was false. The fixes follow from the cause:

1. **Read the variables whose upstream failure is a warning ourselves, and
   refuse what cannot be honoured** (`compression.rs`, `selection.rs`,
   `propagators.rs`), except where refusing is the wrong default. The
   exception is `OTEL_PROPAGATORS` and a proxy variable of a kind not spoken
   (`socks5://`): both are commonly set once for a fleet of other processes, so
   they are reported (`Telemetry::notices`, one warning) and skipped. A bad
   *exporter* value stays an error: it would otherwise export nothing and say so
   nowhere.
2. **Give the process somewhere to see it.** `install_subscriber` now adds a
   layer printing warnings and errors to standard error (`RUST_LOG` in
   `target=level` form, parsed by `Targets`, not `EnvFilter`, whose regex
   engine would be a dependency for a convenience) and logs
   `Telemetry::describe()` once. **This reverses the 2026-10-06 contract** that
   it installs nothing when no trace or log provider was built; a process with no
   collector showed nothing, which is the case that needed it most. The cost is
   that a program that later calls `.init()` on its own subscriber finds the slot
   taken; the CHANGELOG says so and no binary in this repository does it.
3. **Report a lost export where it happens** (`counting.rs`): signal,
   destination (userinfo removed), at most once per 30 s with a count, on a
   target in `SELF_TARGETS`. The SDK's own `…ExportError` event is filtered from
   the printing layer because this supersedes it; without that, an outage printed
   an ERROR every batch.

**Options considered and not taken.**

- *`opentelemetry-stdout` for the console exporter.* It prints several lines a
  record and depends on `chrono`. A one-line form is something to read; written
  in-tree (`console.rs`, about 250 lines) with the SDK's public accessors.
- *`EnvFilter` for `RUST_LOG`.* A regex engine and more for a convenience.
- *`gethostname` for `host.name`.* `unsafe` is forbidden here and a crate for one
  call is a dependency the policy asks to avoid when ≤100 lines do it. The cost:
  no `host.name` on a macOS process that does not export `HOSTNAME`.
- *Prometheus rendered in-tree.* The text format's name, unit and suffix rules
  are the translation the book's table was measured through; the official crate
  is the one source for it. It is a non-default feature (`otel-prometheus`) so
  the default graph does not grow, as `otlp-json` is.
- *`zstd`.* It needs a C library; `gzip` is pure Rust (`miniz_oxide`, already in
  the graph).
- *Proxy support through `reqwest`.* Banned (`deny.toml`); `hyper-util`'s
  `Matcher` and `Tunnel` do it from the stack in use.

**Found while testing it.**

- A proxy given in code must not consult `NO_PROXY`: the integration tests failed
  on a host whose `NO_PROXY` names `localhost`, because an ambient variable
  changed what an explicit setting did.
- `"no live event subscribers; event persisted only"` was a `warn` on the
  ordinary path of every blocking call, which its own comment says is not an
  error. Invisible until something printed warnings; now `debug`.
- `tracing-opentelemetry` puts `code.*`, `thread.*`, `busy_ns` and `idle_ns` on
  every span; they are exported, and left off console lines.

**Not done, and why.** The default metrics export interval (60 s) is the
specification's and is kept for the console exporter, so a console reader sees
metrics at exit or after `OTEL_METRIC_EXPORT_INTERVAL`. Exemplars,
exponential-histogram selection and the `otel.sdk.*` self-observability metrics
were not examined. `OTEL_EXPORTER_OTLP_*` over a Unix socket and SOCKS/HTTPS
proxies are not spoken. WebAssembly targets (no threads, no sockets, no Tokio
runtime) cannot run this exporter at all; that is ADR 0028's subject.
