<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# a2a-rust and a2a-rs, compared on 2026-10-02

Both SDKs' latest crates.io releases, measured on one machine on one day, by
procedures anyone can re-run from
[`benches/sdk-comparison/`](../benches/sdk-comparison/). It supersedes the
capability tables in [`rust-sdk-assessment.md`](rust-sdk-assessment.md) for
anything measured here. That document's governance and provenance sections
are not re-examined.

## Read this first

**Disclosure.** An AI assistant wrote this report and the harness behind it,
working inside the a2a-rust repository at its maintainer's request. That is a
source of bias toward a2a-rust. The mitigations: every comparison runs the same
procedure against both SDKs; conformance and interop are graded by the A2A
project's own suites, not by either SDK; every figure links to a raw result
file; and the findings that go against a2a-rust are stated as plainly as the
ones that favour it. The earlier v0.6.0 comparison did not hold to that
standard. It overclaimed, and this one is written not to.

**Bottom line.**

- **Conformance and interop: parity.** Both pass every MUST-level test of the
  official ACTS suite on all three transports: 155 of 155 each. Both pass 12 of
  12 scenarios of the official ITK interop suite. The difference is two
  SHOULD-level tests out of 281, in a2a-rust's favour.
- **Raw speed: a2a-rs wins clearly.** On the same echo agent it served
  1.17–1.62× the requests per second of a2a-rust 0.14.1, in every cell
  measured (1.17–1.58× unary, 1.24–1.62× streaming). One a2a-rust
  bottleneck this comparison found is fixed on this branch (`831b8ef`,
  +15–18%). a2a-rs is still faster after the fix.
- **Long-running servers: a2a-rust wins clearly.** a2a-rs's only task store
  never forgets a task: 565 MiB after 374,252 requests, still climbing.
  a2a-rust's default store plateaued at 96.6 MiB.
- **Real model in the loop: indistinguishable.** With an LLM generating, both
  SDKs added about 2 ms to a 1.5 s completion, and both relayed the model's
  output byte for byte.
- **Operational depth: a2a-rust has much more.** It ships auth, SQL
  persistence, rate limits and observability. Auth and SQLite persistence
  were exercised here and work; rate limits and observability were read in
  source, not run. a2a-rs's published crates have no auth, no persistent
  store and no rate limiting, and their observability is a few log lines.
- **Multi-tenancy: a2a-rust's claim is half true.** Tenant isolation works with
  the tenant-aware stores, but the **default configuration leaks across
  tenants exactly as a2a-rs does**: 5 of 5 probes, both SDKs. a2a-rs has
  no isolation option at all.
- **Size and supply chain: a2a-rs is smaller** at about 21k lines of code
  against 76k. a2a-rust compiles faster, links smaller and pulls fewer
  dependencies at default features; with every feature on it pulls more.
  Fresh-resolved lockfiles for both audit clean.

Labels used below: **VALIDATED** means measured here, with the procedure and
raw result named. **CONJECTURED** means reasoned but not measured. Anything
not measured is said to be unmeasured.

## 1. Subjects

| | a2a-rs (official, `a2aproject/a2a-rs`) | a2a-rust (`tomtom215/a2a-rust`) |
|---|---|---|
| Crates measured | `a2a-lf` 0.4.1, `a2a-server-lf` 0.5.1, `a2a-client-lf` 0.2.7, `a2a-grpc` 0.3.9, `a2a-pb` 0.3.1, `a2a-slimrpc` 0.2.11 (and `a2a-cli` 0.3.1, source only) | `a2a-protocol-types`, `-server`, `-client`, `-sdk`, all 0.14.1 |
| Published | 2026-09-30 | 2026-09-30 |
| Release commit (`.cargo_vcs_info.json`) | `32c31f69` | `2be81e79` |
| Edition / MSRV | 2024 / 1.85 | 2024 / 1.88 |

The `.crate` files were downloaded from `static.crates.io` and hashed; the
hashes are in `results/static/crates-SHA256SUMS`. The harness depends on
those exact versions (`=x.y.z`) from crates.io, never on a path. Conformance
and interop build each SDK from its release commit, because that is how the
official runner works.

## 2. Method

**Machine.** One container with 4 vCPUs (Intel Xeon @ 2.80 GHz, one thread per
core) and 15.7 GiB RAM, running Linux 6.18.44 and rustc 1.97.0
(`results/static/env.txt`). Loopback networking only.

**Harness.** One agent per SDK, each written idiomatically against that SDK's
own server API, in a standalone crate with its own lockfile, so neither SDK's
dependency features leak into the other's build. Both implement the same
contract:

- Echo: `Working`, then one artifact `Echo: <text>`, then `Completed`.
- `llm:<prompt>`: stream the model's tokens as artifact chunks, then
  `Completed`.

The load generator is a third crate that speaks raw HTTP/1.1. It sends
identical bytes to either server, gives every request a unique `messageId`,
and counts a response as success only if it is a completed task with no
`error`. The probes are Python with no SDK client involved.

**Performance protocol.** The server is pinned to CPUs 0–1 and the load
generator to CPUs 2–3. Every cell gets a fresh server process, a 3 s warm-up
and a 10 s measurement. Each cell runs three times, with the server order
rotated between repetitions. Tables report the median, with min–max as the
spread. All 72 cells recorded 0 errors.

**Not done.** No security audit. Client-side performance not measured; every
throughput figure is server-side. gRPC and REST performance not measured;
JSON-RPC only. ITK's nightly suite (Java, .NET and JS peers) not run, only
its PR suite. No PostgreSQL. No multi-machine or real-network test.

## 3. Conformance and interoperability — graded by the A2A project

### 3.1 ACTS (`a2aproject/a2a-itk` @ `b57c5332`)

**VALIDATED.** Each SDK's own ITK agent was run through the official ACTS
runner over three transports. The agent sources are the SDKs' own
`itk/`: neither was written for this comparison.

| | JSON-RPC | gRPC | HTTP+JSON | Total |
|---|---|---|---|---|
| a2a-rs: MUST / SHOULD / MAY | 59/59 · 28/29 · 13/13 | 47/47 · 27/28 · 13/13 | 49/49 · 29/30 · 13/13 | **278 / 281** |
| a2a-rust: MUST / SHOULD / MAY | 59/59 · 29/29 · 13/13 | 47/47 · 28/28 · 13/13 | 49/49 · 29/30 · 13/13 | **280 / 281** |
| Verdict | both CONFORMANT | both CONFORMANT | both CONFORMANT | |

The three failures:

- **a2a-rs: `CORE-CAP-002` (SHOULD), on all three transports.** "Streaming not
  supported returns UnsupportedOperationError". The stream opened and produced
  2 events against an agent whose card says it does not stream.
- **a2a-rust: `REST-CT-001` (SHOULD).** REST responses carry
  `Content-Type: application/json`, not `application/a2a+json`.

Raw reports: `results/acts/acts-report-*.json`. Each project's own ITK shim
was mirrored exactly, with two environment-only changes. Host networking let
the container reach this sandbox's proxy. A Dockerfile overlay trusts the
proxy's CA; `scripts/itk_dockerfile_overlay.py` reproduces it byte for byte
(sha256 `c0d8e553…`), and no test, scenario or runner file was touched.

### 3.2 ITK interop (PR scenario set, same checkout)

**VALIDATED.** Both: **12 of 12 passed.** Each scenario sends a nested
traversal through the SDK under test and released Go, Python and Rust peers
(`go_v10`, `python_v10`, `rust_v10`). The suite covers JSON-RPC, gRPC and
HTTP+JSON, each with send, streaming send, push notification and resubscribe.
`rust_v10` is released a2a-rs, so this suite tested a2a-rust ⇄ a2a-rs directly
(`results/acts/itk-*.txt`).

### 3.3 Each SDK's own client against each server, with a live model

**VALIDATED.** All four client→server pairings streamed one model completion
to `Completed`. All four reassembled the same 250-character text (sha256
prefix `8caa248473cee1b0`; `results/llm/interop.txt`).

## 4. Performance

### 4.1 Echo throughput and latency (JSON-RPC, server on 2 cores)

**VALIDATED** (`results/perf/perf.jsonl`, summarized in `perf_summary.md`).

`SendMessage`:

| Connections | a2a-rs, rps (p50 / p99 µs) | a2a-rust 0.14.1, rps (p50 / p99 µs) | a2a-rs ÷ a2a-rust |
|---:|---|---|---:|
| 1 | 4,938 [4,897–4,944] (176 / 451) | 3,244 [3,033–3,537] (229 / 1,408) | 1.52 |
| 16 | 7,700 [7,566–8,087] (1,883 / 3,807) | 4,868 [4,677–5,107] (3,179 / 5,983) | 1.58 |
| 64 | 6,344 [5,328–6,662] (9,015 / 16,327) | 4,347 [3,685–4,715] (14,615 / 22,767) | 1.46 |
| 256 | 4,309 [4,013–4,621] (59,199 / 80,511) | 3,690 [3,656–3,837] (68,479 / 95,103) | 1.17 |

`SendStreamingMessage` (SSE):

| Connections | a2a-rs, rps | a2a-rust 0.14.1, rps | a2a-rs ÷ a2a-rust |
|---:|---:|---:|---:|
| 1 | 3,637 | 2,939 | 1.24 |
| 16 | 7,422 | 4,574 | 1.62 |
| 64 | 6,039 | 4,068 | 1.48 |
| 256 | 4,756 | 3,436 | 1.38 |

What to read from it:

- **a2a-rs is faster in every cell.** a2a-rs also sends *more* bytes per
  response: 428 against 294, because it includes `history` by default.
- **Throughput falls past 16 connections on both.** That is most likely
  two saturated cores rather than an SDK property (CONJECTURED; not tested
  with more cores).
- **The a2a-rs rows are "tuned": one line added to its own example's server
  setup.** As shipped, a2a-rs's `helloworld` example serves with
  `axum::serve(listener, app)`, which leaves `TCP_NODELAY` off. A streamed
  response then stalls about 44 ms per request on Nagle and delayed ACK: 23
  streams per second on one connection, against 3,637 with `TCP_NODELAY` on.
  Unary requests are unaffected. Both configurations are in the raw data
  (`a2a-rs-asshipped`, `a2a-rs-tuned`). The headline uses the faster one,
  because the fix is a single `tap_io` call and a fair comparison should not
  rest on an example's socket setup.

### 4.2 Where a2a-rust's time goes, and what was fixed

**VALIDATED.** `perf record` was run on both agents under 16 connections. The
largest a2a-rust frame was the in-memory store's TTL pass: 9.3% of server CPU
as self time, 13.5% inclusive. Every 64th write, it scanned every stored task
(up to 10,000) under the store's write lock.

Disabling only that pass raised throughput from 4,269 to 5,881 rps (median of
3, `results/perf/ttl_attribution.txt`). That is more than its CPU share,
consistent with every concurrent save waiting on the lock (CONJECTURED).

**Fixed on this branch in `831b8ef`.** A write-ordered index lets the pass
visit only expired entries. Published 0.14.1 against the patched build, same
harness, interleaved runs (`results/perf/ab_ttl.jsonl`):

| Connections | 0.14.1, rps (p99) | patched, rps (p99) | a2a-rs, same session | Gap before → after |
|---:|---|---|---:|---:|
| 16 | 4,845 (6.4 ms) | 5,593 (5.1 ms) | 7,345 | 1.52× → 1.31× |
| 64 | 4,236 (23.5 ms) | 5,001 (20.1 ms) | 5,852 | 1.38× → 1.17× |

The patched build's profile under the same load has no a2a-rust function
above 1.64% self time. The libc allocator (`malloc`, `free`, `memmove` and
their internals) accounts for about 30.6% of samples, and kernel scheduling
and wakeup paths for about 21.2% (`results/perf/profile_patched_top.txt`). The
remaining gap is therefore diffuse: allocation volume and cross-task handoffs
on the request path (CONJECTURED from the profile's shape; not attributed
further). a2a-rs's allocator share was not summed the same way, so no
comparison of the two is claimed.

### 4.3 Memory under sustained load

**VALIDATED** (`results/perf/mem.jsonl`). 64 connections sent `SendMessage`
for 60 s while RSS was sampled every 5 s.

| | RSS after 10 s | RSS after 60 s | Requests served | Trend |
|---|---:|---:|---:|---|
| a2a-rs | 125.5 MiB | 564.6 MiB | 374,252 | linear, about 1.5 KiB per task, no bound |
| a2a-rust 0.14.1 | 96.4 MiB | 96.6 MiB | 243,244 | flat from 10 s onward (10,000-task cap) |
| a2a-rust patched | 97.3 MiB | 98.1 MiB | 200,855 (40 s) | flat |

a2a-rs's `InMemoryTaskStore` is a `HashMap` with no TTL, capacity or eviction,
and it is the only store a2a-rs ships. At the measured rate of about 1.5 KiB
per task, a server that handles a million tasks holds roughly 1.5 GiB of task
records until it restarts. That figure is extrapolated, not measured.

### 4.4 With a real model

**VALIDATED** (`results/llm/llm_summary.txt`). The model was Qwen3.5-0.8B Q4_0,
sha256 `57d19977…f8cf`, served by llama.cpp `bed0a856` with one slot,
temperature 0 and seed 42. Five prompts × four rounds were run, with the
calling order rotated among calling llama.cpp directly, through a2a-rs, and
through a2a-rust.

| Path | TTFT median | Total median | Text identical to direct | Completed |
|---|---:|---:|---:|---:|
| direct to llama.cpp | 108.1 ms | 1,519 ms | 20/20 | 20/20 |
| through a2a-rs | 105.2 ms | 1,509 ms | 20/20 | 20/20 |
| through a2a-rust | 108.7 ms | 1,544 ms | 20/20 | 20/20 |

The paired overhead medians are 1.9 ms (a2a-rs) and −0.1 ms (a2a-rust) on time
to first token. The min–max spread is about ±200 ms. It moves with which path
asked a prompt first, which points at llama.cpp's prompt cache rather than
either SDK (CONJECTURED; the cache was not disabled to confirm it). **With a model in the loop, SDK overhead is below
what this setup can resolve.**

## 5. Features, verified by running them

### 5.1 Black-box probes

**VALIDATED** (`scripts/tenant_probe.py`, `scripts/feature_probe.sh`; results
in `results/probes/`).

Tenant isolation was probed over JSON-RPC using the A2A v1.0 `tenant` field.
Tenant A creates a task; tenant B then tries to reach it. A positive control
(A reading its own task) passed in every configuration.

| Tenant B tries to… | a2a-rs default | a2a-rust default | a2a-rust + `TenantAware*` stores |
|---|---|---|---|
| `GetTask` A's task | **leak** | **leak** | isolated (−32001) |
| `ListTasks` and see it | **leak** | **leak** | isolated (0 tasks) |
| `SubscribeToTask` A's live task | **leak** | **leak** | isolated (−32001) |
| `CancelTask` A's live task | **leak** (canceled) | **leak** (canceled) | isolated; A's task still `WORKING` |

a2a-rs carries `tenant` but keys its store by task id alone. Its REST router
has no `/{tenant}/…` routes, so on REST a tenant cannot be expressed at all.
a2a-rust isolates only when the tenant-aware stores are configured. Its
multi-tenancy guide says so, but nothing warns a deployment that receives
tenants on the default store. `builder.rs` and `handler/mod.rs` contain no
such warning, and the probe's requests were served. `require_resolved_tenant`
does not help here: it rejects requests whose tenant cannot be resolved, and
does not partition the store. See finding R1 in §7.

Other a2a-rust claims, checked the same way:

- **SQLite persistence: holds.** A task read back intact after `kill -9` and
  a restart.
- **Bearer authentication: holds.** No token → 401, wrong token → 401, right
  token → 200; the agent card stays public.

### 5.2 Source audit

The audit was delegated to a sub-agent that read both crate sets. I
re-checked these rows against source myself:

- the store rows, also confirmed black-box in §4.3;
- tenant handling, also black-box in §5.1;
- body limits and the push-config cap;
- the multicast and CGNAT asymmetry;
- a2a-rs having no `Last-Event-ID` handling and no client retries.

The other cells come from the sub-agent's read and were not re-checked:
HTTPS rebinding coverage, redirect re-checks, client timeouts and caching,
JWT and mTLS, and observability. Two of the sub-agent's statements were wrong
and are corrected here.

1. It said a2a-rust has no SLIMRPC binding. It does, at
   `bindings/a2a-protocol-slimrpc`, but that binding is **not published to
   crates.io**.
2. It said a2a-rs's server interceptors "give no protection" when added. In
   fact `InterceptedHandler` implements no `RequestHandler`, so it cannot be
   passed to a router at all: wiring it is a compile error, not a silent
   bypass.

| | a2a-rs | a2a-rust |
|---|---|---|
| Task stores | In-memory only, unbounded | In-memory (bounded by default), SQLite, PostgreSQL, tenant-aware variants of each |
| Server authentication | None in the crates | API key, bearer, JWT/JWKS; mTLS on gRPC |
| Limits | 10 MiB body limit; 50 push configs per task | 4 MiB body; concurrency caps; rate limiting; executor timeout |
| Push SSRF guard | Screens at connect time, covering HTTPS DNS rebinding; re-checks each redirect; blocks multicast; no CGNAT block; no retry | Pre-flight checks; pins the IP for HTTP only; blocks CGNAT; does not block multicast; retries with backoff |
| SSE resume | No `Last-Event-ID`; a lagging subscriber is cut | `Last-Event-ID` replay from the event log |
| Observability | A few log lines | Spans, a `Metrics` trait, OTLP export, W3C trace context |
| Agent-card signing | Type only | Sign and verify (JWS) |
| Client | Interceptors; no retries, timeout or card cache | Retry policy, timeouts, caching card resolver |
| Transports (published) | JSON-RPC, REST, gRPC, **SLIMRPC** | JSON-RPC, REST, gRPC, **WebSocket** |
| CLI | **`a2a-cli`**, with signed releases | none |
| `unsafe` in library code | 0 occurrences, not forbidden | 0 blocks; `#![forbid(unsafe_code)]` |

## 6. Size, build and supply chain

**VALIDATED** (`results/static/`).

| | a2a-rs | a2a-rust |
|---|---:|---:|
| Code lines in the published crates (tokei; includes inline tests) | 21,110 (6 crates) | 76,539 (4 crates) |
| Unique normal dependencies of the server crate, default features | 145 | 55 |
| … with all features | 153 | 193 |
| Echo agent, clean release build (2 runs) | 123.3 s, 124.3 s | 62.0 s, 64.0 s |
| Echo agent, stripped binary | 6.10 MB | 4.17 MB |
| RustSec, freshly resolved harness lockfile (1,280 advisories) | 0 | 0 |
| RustSec, the repository's own `Cargo.lock` | RUSTSEC-2026-0285 (`rustls` 0.23.43), reachable from every crate, including `a2a-cli`'s release builds | RUSTSEC-2023-0071 (`rsa`), in the lockfile but in no resolved graph (`cargo tree -i rsa --target all` is empty) |
| Own test suite at the release commit (§8) | 825 passed, 0 failed | 2,991 passed, 0 failed |

`a2a-server-lf` depends on `reqwest` unconditionally, for push delivery, so
turning its default features off still leaves 128 crates.

## 7. Findings

These are suggestions for each project, ranked by severity within each list.
Nothing has been filed upstream.

### For a2a-rust

| ID | Severity | Finding | Status |
|---|---|---|---|
| R1 | High | Default configuration ignores `tenant` and leaks across tenants (§5.1). | **Fixed after this report:** a server whose stores cannot isolate now refuses any request naming a tenant (`-32004`); the same probe against the fixed build records `refused`. See CHANGELOG, Unreleased. |
| R2 | Medium | 1.17–1.62× slower than a2a-rs per request (§4.1). | Partly fixed in `831b8ef` (+15–18%). The rest is spread across allocation and task handoffs, with no single hotspot (§4.2). |
| R3 | Medium | Cancelling a task does not reach the tasks it delegated to. This is a protocol gap, but this SDK offers no helper for it either (`swarm-orchestration.md` G1). | Open. Measured in `examples/swarm`. |
| R4 | Low | ACTS `REST-CT-001`: REST content type is `application/json`, not `application/a2a+json`. | Open. |
| R5 | Low | The SLIMRPC binding is advertised in the README but not on crates.io. | Open. |
| R6 | Low | `A2aClient` is not `Clone`; sharing one client needs an `Arc`. | Noted. |

### For a2a-rs (offered, not filed)

| Severity | Finding |
|---|---|
| High | The only task store is unbounded: memory grows about 1.5 KiB per task with no limit (§4.3). |
| High | `tenant` is carried but never enforced, and REST drops it (§5.1). |
| Medium | The `helloworld` example's server setup costs streamed responses about 44 ms each (§4.1). The fix is one line. |
| Medium | ACTS `CORE-CAP-002`: it streams to clients when the card says it does not stream. |
| Low | `InterceptedHandler` is exported but cannot be used. |
| Low | The repository lockfile pins a `rustls` affected by RUSTSEC-2026-0285. |

## 8. Each SDK's own test suite

**VALIDATED** (`results/static/tests-*.txt`). Each suite was run at its
release commit with `cargo test`.

| | Scope | Test binaries | Passed | Failed | Ignored |
|---|---|---:|---:|---:|---:|
| a2a-rs | workspace default members, including CLI and examples | 30 | 825 | 0 | 0 |
| a2a-rust | the four published crates, default features | 130 | 2,991 | 0 | 24 |

Both are green. The counts measure each project's own testing habits, not
quality. They are graded by the project that wrote them, which is why §3,
graded by the A2A project, carries the weight.

## 9. Reproducing this

See [`benches/sdk-comparison/README.md`](../benches/sdk-comparison/README.md).
Everything ran in one session. A second run on other hardware will move the
absolute numbers. The ratios, the conformance tallies and the probe verdicts
should not move; if they do, that is a finding.

## 10. What would change these conclusions

- **Perf ratios** were measured with a 2-core server, loopback and an echo
  agent. A workload dominated by executor time (any real model, §4.4) hides
  them entirely. A larger core count could narrow or widen them; that is
  unmeasured.
- **The harness agents are mine.** They are written idiomatically against each
  SDK's documented API, but a maintainer of either SDK might write a faster
  one. The a2a-rs `TCP_NODELAY` case shows how much one line can matter. It
  was caught only because one cell came out about 160× slower than its
  neighbours; a smaller version of the same mistake would not have stood
  out.
- **Conformance** is graded on each SDK's own ITK agent, not on the harness
  agents. An application built differently could conform differently.
