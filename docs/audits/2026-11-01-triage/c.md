# Slice c: observability rows C-05 to C-23 (C-11 is not in this slice)

Baseline: `main` at `4a8243c66df5ae07a54a69168ab419ac609b06e9` (checked with `git rev-parse HEAD`).
Everything below comes from reading the tree and the vendored crates
(opentelemetry 0.33.1, opentelemetry_sdk 0.33.1, opentelemetry-otlp 0.33.1). I ran no
cargo, no promtool, no Docker and no live stack.

## Counts (18 rows)

| Verdict | Rows |
|---|---|
| confirmed | 9: C-05, C-06, C-12, C-13, C-14, C-15, C-19, C-21, C-23 |
| needs_decision | 5: C-08, C-10, C-16, C-17, C-22 |
| already_fixed | 1: C-07 (51d33eb) |
| partly_fixed | 1: C-09 (43e142b) |
| cannot_check | 1: C-18 (needs a live Grafana) |
| not_a_defect | 1: C-20 (misread; the real leak is NEW-c-4) |

There are 5 new findings, NEW-c-1 to NEW-c-5: one medium and four low.

## The five rows that matter most

1. **C-09, partly fixed.** Commit 43e142b changed only the Prometheus rules:
   - The error ratios now leave out `error_type="cancelled"`.
   - The server alert now has a traffic floor of 0.1 calls/s.
   - The promtool cases would fail against the old rules. I read them; neither I nor CI ran them, because no workflow runs promtool.

   Still open:
   - Spans still mark an abandoned call `ERROR` with `error.type=cancelled` (`rpc_span/mod.rs:392`).
   - On gRPC, `rpc.status_code` is recorded as lowercase `cancelled`. The crate's own gRPC rule says server-side CANCELLED is not an error.
   - `a2a:rpc_server_cancelled:rate5m` has no panel and no alert.
   - "cancelled" also covers unary calls cut by a load balancer's idle timeout (B-11), and those are now outside the only error alert.
   - Because of the floor, a job under 0.1 calls/s that fails every call can never page.
2. **C-08, needs a decision.** Unknown values are fatal, which contradicts a MUST in the spec:
   - Unknown or uppercase `OTEL_*_EXPORTER`, `PROTOCOL` and `COMPRESSION` values fail `build()`, and so does a non-integer timeout.
   - Every binary then calls `exit(2)`.
   - The spec says an unrecognised enum value MUST produce a warning and be ignored.
   - The refusal is deliberate and documented. But the reason it was introduced (no subscriber exists at build time) is gone: `Telemetry::notices()` exists, and the crate already reports-and-skips for `OTEL_PROPAGATORS`. Upstream opentelemetry-otlp also warns and ignores.
   - Recommendation: warn via notices and fall back.
3. **NEW-c-1, medium.** `A2AServerErrorRatioHigh` counts JSON-RPC caller errors as failures (TaskNotFound -32001, invalid params). Following the JSON-RPC conventions, every error code counts. The same traffic over REST (404) or gRPC (NOT_FOUND) is not counted. The walkthrough even shows the alert firing on deliberate task-not-found traffic.
4. **C-10, needs a decision.** W3C baggage never crosses an A2A hop (client, push or server), and `OTEL_PROPAGATORS` (including `none`) has no effect on the A2A wire. Baggage carries privacy risk to remote agents, so I recommend opt-in baggage propagation, and treating `OTEL_PROPAGATORS=none` as "do not inject".
5. **C-13 / C-15, confirmed.** The telemetry pipeline cannot observe itself:
   - There are no process or runtime metrics and no SDK self-metrics. Lost exports and spans dropped on a full queue show up only in log lines and at shutdown.
   - opentelemetry_sdk 0.33.1 offers `otel.sdk.processor.*` metrics behind an experimental feature that this crate does not enable.
   - Nothing scrapes the Collector.
   - In the base compose, `memory_limiter` has no container limit to take its percentage from. The book says it does.

## Other notes

- **C-07, already fixed.** Under `OTEL_SDK_DISABLED`, `install_subscriber` now installs the stderr layer. The child-process test in `tests/telemetry_export/console.rs:264` would fail with the old early return (read, not run). Remaining edge, filed as NEW-c-5: `OTEL_SDK_DISABLED=1` is silently not a disable.
- **C-20, not a defect as written.** The report read `input.to_string()` as the raw message. It is JSON built under a 64 KiB budget (`agent.rs:191-197`), and the budget was already there at 8dc4331. The budget still leaks through per-part overhead and per-event accumulation; that is filed as NEW-c-4.
- **C-12.** Two doc statements on the legacy pipeline are stale: "otlp compiled with grpc-tonic+metrics only" and "release builds panic=abort". The pipeline also has no tonic TLS, so it cannot reach an `https://` collector (read only).
- **C-17.** The per-call warning is deliberate and pinned by a test. It also fires for the SDK's own `A2A-Extensions` header, whose advice ("supply credentials at connect time") is wrong for that header.
- **NEW-c-2.** The book never mentions `with_span_level`, and `logs.md` still says every span is exported. Both are stale since C-01. The walkthrough says there are "eight alerts"; there are nine.
- **NEW-c-3.** Warnings from the upstream crates (an invalid temporality preference, an unknown sampler) are emitted before any subscriber exists, so they are lost.

## Not done, and why

- **C-18** needs a running Grafana 13.2.4 to see the LogQL it generates. The exact probe is in `c.json`.
- **Prometheus behaviour (C-05, C-21)** and the Collector's `memory_limiter` without a limit (C-15) are conjecture until the compose stack is run. The probes are in `c.json`.
- **promtool:** I did not run it, so the C-09 and C-14 rule-test claims are from reading the test files.
- **C-23:** the per-measurement cost was not measured.
- I assumed the RPC bucket advice the code cites (semconv at 55c98cf) and did not re-check it against the semconv repository.
