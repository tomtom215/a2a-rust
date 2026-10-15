<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# a2a-rust Benchmark Suite

Criterion-based benchmarks measuring the performance characteristics of the
`a2a-protocol-sdk` Rust implementation. All but one benchmark isolate **SDK
overhead** from agent logic by using trivial executors (echo / noop). The
exception is `coordinator_chain_under_fault`, which measures end-to-end
latency of an agent-level 5-hop coordinator chain under injected faults.

Published results: [`book/src/reference/benchmarks.md`](../book/src/reference/benchmarks.md),
generated from criterion output by `scripts/generate_book_page.sh`.

## Quick Start

```bash
# Run the 13 benchmark targets listed in run_benchmarks.sh (see below)
./benches/scripts/run_benchmarks.sh

# Run every [[bench]] target in benches/Cargo.toml (all 15)
cargo bench -p a2a-benchmarks

# Run a specific benchmark
cargo bench -p a2a-benchmarks --bench transport_throughput

# Save a baseline for regression detection
./benches/scripts/run_benchmarks.sh --save

# Compare against saved baseline
./benches/scripts/run_benchmarks.sh --compare
```

`benches/Cargo.toml` declares 15 `[[bench]]` targets. The `BENCHMARKS` array
in `run_benchmarks.sh`, and the full run in `.github/workflows/benchmarks.yml`,
cover 13 of them: `coordinator_chain_under_fault` and
`send_latency_breakdown` are not included, so they run only when named
(`cargo bench -p a2a-benchmarks --bench <name>`, or
`run_benchmarks.sh --bench <name>`) or when `cargo bench` runs every target.
Neither has results on the generated page.

## Benchmark Modules

| Module | File | What it measures |
|--------|------|------------------|
| **Transport Throughput** | `transport_throughput.rs` | Messages/sec, bytes/sec through JSON-RPC and REST HTTP transports; SSE streaming drain latency; payload size scaling (up to 1MB) |
| **Coordinator Chain Under Fault** | `coordinator_chain_under_fault.rs` | Agent-level 5-hop coordinator chain under fault injection |
| **Send Latency Breakdown** | `send_latency_breakdown.rs` | Attributes blocking `message/send` latency to its parts rather than inferring them: runtime workers (multi vs single), executor event count (3-event echo vs 2-event noop), and the transport floor (`get_task` on a missing id, which shares the whole ingress path and does no executor work). Groups `diag/send` and `diag/floor`. |
| **Protocol Overhead** | `protocol_overhead.rs` | Serde ser/de cost per A2A type (AgentCard, Task, Message, StreamResponse); JSON-RPC envelope overhead; batch scaling; `protocol/payload_scaling` isolation benchmarks (64B–1MB, `to_vec` vs `SerBuffer`, `from_slice` vs `from_str`) |
| **Task Lifecycle** | `task_lifecycle.rs` | TaskStore save/get/list latency; EventQueue write→read throughput; end-to-end create→working→completed via HTTP |
| **Concurrent Agents** | `concurrent_agents.rs` | N simultaneous sends/streams (1, 4, 16, 64); store contention; mixed send+get workloads |
| **Cross-Language** | `cross_language.rs` | Five canonical workloads, measured for **this SDK only** (the name is historical; see the file's header). The actual cross-language comparison is `scripts/cross_language_python.sh` — see [Cross-Language Comparison](#cross-language-comparison) |
| **Realistic Workloads** | `realistic_workloads.rs` | Multi-turn conversations (1–10 turns); mixed payload complexity (text, file refs, nested metadata); connection reuse vs per-request clients; interceptor chain overhead (0–10 interceptors); complex agent card ser/de (1–100 skills); conversation history scaling |
| **Error Paths** | `error_paths.rs` | Happy path vs error path latency ratio; task-not-found lookup cost; malformed JSON rejection throughput; wrong content-type rejection |
| **Backpressure** | `backpressure.rs` | Stream event volume scaling (3–502 events); append volume (one artifact receiving 1–500 chunks, against the in-memory, discard and SQLite task stores); slow consumer simulation (1ms/5ms read delays); concurrent stream fan-out under load (1–16 streams); timer calibration. Higher event counts (252, 502) push per-event signal above CI noise floor. |
| **Data Volume** | `data_volume.rs` | TaskStore get/list/save at 1K–100K pre-populated tasks; context_id filtering at scale (exercises BTreeSet sorted index + context_id secondary index for O(page_size) queries); concurrent read contention at 10K tasks; history depth impact on store operations. Get benchmarks use 64 pseudo-random keys to avoid single-key HashMap anomalies. |
| **Memory Overhead** | `memory_overhead.rs` | Heap allocations per serialize/deserialize via counting allocator; allocation scaling with conversation history depth; allocation bytes per payload size (64B–16KB). Uses `iter_custom` with real wall-clock timing and tolerance-based allocation assertions (5% threshold to absorb serde_json version variance). |
| **Enterprise Scenarios** | `enterprise_scenarios.rs` | Multi-tenant task store isolation (1–100 tenants); push config store CRUD; eviction under memory pressure (100–10K at capacity); rate limiting overhead; CORS preflight; R/W mix ratios (100:0 → 0:100); large history (100–500 turns); cancel task round-trip; list tasks with pagination (10–50 page sizes); handler limits enforcement and rejection throughput; client-side interceptor chain (0–10 interceptors) |
| **Production Scenarios** | `production_scenarios.rs` | Full E2E production workflows: SubscribeToTask reconnection (snapshot replay); cold start vs steady-state latency; concurrent cancel+subscribe race; 7-step multi-context orchestration (send→follow-up→new-context→list→get→stream→cancel); push notification config full CRUD round-trip; parallel agent burst (10–100 concurrent agents, 3 ops each), with and without a shared client (`production/agent_burst_client_sharing`); dispatch routing overhead isolation (HTTP round-trip vs direct handler invoke) |
| **Advanced Scenarios** | `advanced_scenarios.rs` | SDK capability gaps: tenant resolver overhead (header/bearer/path extraction); agent card hot-reload (read, swap, complex swap); /.well-known discovery endpoint latency; subscribe fan-out (1–10 concurrent subscribers); streaming artifact accumulation cost (task.clone() at 0–500 artifact depth); pagination full walk (100–1K tasks, unfiltered + filtered); extended agent card round-trip |

## Architecture

```
benches/
├── Cargo.toml                      # Benchmark crate (publish = false), 15 [[bench]] targets
├── README.md                       # This file
├── requirements-cross-language.txt # Pinned Python environment for cross_language_python.sh
├── src/
│   ├── lib.rs                      # Shared helpers entry point
│   ├── executor.rs                 # EchoExecutor, NoopExecutor, MultiEventExecutor, AppendingExecutor, FailingExecutor, NoopPushSender
│   ├── fixtures.rs                 # Deterministic test data + realistic payload generators
│   ├── server.rs                   # In-process HTTP server startup (SO_REUSEADDR + SO_REUSEPORT, graceful shutdown)
│   ├── store.rs                    # DiscardTaskStore: keeps nothing, for cost attribution
│   ├── coordinator.rs              # ChainHopExecutor for the multi-hop chain benchmark
│   └── fault_transport.rs          # FaultInjectingTransport for the multi-hop chain benchmark
├── benches/
│   ├── transport_throughput.rs     # criterion benchmarks
│   ├── protocol_overhead.rs
│   ├── task_lifecycle.rs
│   ├── concurrent_agents.rs
│   ├── cross_language.rs           # canonical workloads, this SDK only
│   ├── realistic_workloads.rs      # real-world usage patterns
│   ├── error_paths.rs              # failure handling cost
│   ├── backpressure.rs             # streaming under load
│   ├── data_volume.rs              # store ops at scale
│   ├── memory_overhead.rs          # heap allocation profiling
│   ├── enterprise_scenarios.rs     # multi-tenant, CORS, eviction, rate limiting
│   ├── production_scenarios.rs     # real-world E2E workflows
│   ├── advanced_scenarios.rs       # SDK capability gap coverage
│   ├── coordinator_chain_under_fault.rs  # agent-level 5-hop chain under injected faults
│   └── send_latency_breakdown.rs   # attribution of blocking message/send latency
├── examples/
│   └── send_probe.rs               # Diagnostic probe: send latency vs number of stored tasks
├── dashboard/
│   └── template.html               # Interactive dashboard HTML template
├── cross_language/
│   ├── canonical_agent_card.json   # Reference AgentCard (not read by any code in this repo)
│   └── canonical_send_params.json  # Reference send params, 256-byte text part (not read by any code in this repo)
├── scripts/
│   ├── run_benchmarks.sh           # Run 13 of the 15 benches + write a summary JSON
│   ├── check_regression.py         # Regression check on criterion change estimates (CI gate)
│   ├── generate_book_page.sh       # Auto-generate book/src/reference/benchmarks.md
│   ├── generate_dashboard.sh       # Generate interactive dashboard from criterion data
│   ├── extract_benchmark_json.py   # Extract criterion results into structured JSON
│   ├── cross_language_python.sh    # Rust vs official Python SDK
│   ├── cross_language_bench.py     # Measurement harness for the above
│   ├── cross_language_floor.py     # Constant-response floor target for the above
│   └── generate_cross_language_page.py  # Renders the cross-language book page
├── results/
│   ├── cross-language-pinned.json  # Committed: the book page is generated from these
│   ├── cross-language-default.json # Committed: the book page is generated from these
│   └── .gitkeep                    # Other result JSONs are gitignored
└── sdk-comparison/                 # Standalone a2a-rust vs a2a-rs harness; see its README.md
```

## What We Benchmark (and Why)

The SDK's value proposition is the **A2A protocol layer and runtime
efficiency**, not the agent logic itself. We benchmark what the SDK owns:

| Dimension | Why it matters at scale |
|-----------|----------------------|
| **Transport throughput** | HTTP overhead is paid on every request, so it sets a per-request floor that multiplies with request volume |
| **Protocol overhead** | Serde cost determines the floor for any A2A operation; this is the tax every message pays |
| **Task lifecycle** | Store and queue operations are the backbone of task management; contention here limits vertical scaling |
| **Concurrency** | Agent orchestration is inherently concurrent; degradation curves predict capacity planning |
| **Cross-language** | Hard numbers for deployment decisions — not for competition, but for engineering planning: what the A2A layer costs per request in this SDK versus the official Python SDK |
| **Realistic workloads** | Multi-turn conversations, mixed payloads, interceptor chains — the patterns real applications actually use, not synthetic micro-benchmarks |
| **Error paths** | Production systems spend significant time on error handling; benchmarking only the happy path gives an incomplete picture |
| **Backpressure** | Slow consumers and high event volume expose buffering and flow-control overhead that synthetic tests miss |
| **Data volume** | Store operations must scale gracefully from empty to 100K+ tasks; degradation curves predict production capacity |
| **Memory overhead** | Allocation counts and bytes per operation reveal hidden costs that latency benchmarks alone cannot capture |
| **Enterprise scenarios** | Multi-tenant isolation, push notifications, eviction, rate limiting, CORS, read/write mix, handler limits — the operational concerns of production deployments |
| **Production scenarios** | Full E2E workflows: reconnection, cold start, race conditions, multi-context orchestration, agent bursts |
| **Advanced scenarios** | Tenant resolver overhead, agent card hot-reload, discovery latency, subscribe fan-out, artifact accumulation bottleneck, pagination walks — SDK capability paths the other modules do not exercise |

### What We Do NOT Benchmark

- **Agent intelligence** — LLM quality is an eval problem, not a perf benchmark
- **Task completion quality** — needs human-preference evaluation (LMSYS-style)
- **Network latency** — all benchmarks use loopback to isolate SDK overhead
- **TLS handshake** — benchmarks use plaintext HTTP

### Measurement Rigor

The benchmarks follow these practices for reproducibility:

- **Deterministic inputs** — Fixed task IDs and payloads inside `iter()`, or a fixed cycle over precomputed keys (`data_volume/get` walks 64 keys). The exception is the two `data_volume` save groups (`save/after_prefill`, `history_depth/save_with_turns`), which increment a task id per iteration because what they measure is inserting a new task.
- **Setup outside measurement** — Store creation, server startup, `EventQueueManager` allocation, and resource initialization happen before `iter()`, not inside it.
- **`debug_assert!` for invariants** — Correctness checks on results inside measurement loops mostly use `debug_assert!`, which compiles out of release builds. The exceptions are the `assert!(result.is_err())` checks on the task-not-found paths in `error_paths.rs` and `send_latency_breakdown.rs`. Calls inside the loop still `.expect()` their results.
- **`black_box()` where work could be elided** — Measured inputs and outputs are wrapped in `std::hint::black_box` where the compiler could otherwise elide the work. Six files do not use it: `transport_throughput`, `backpressure`, `concurrent_agents`, `production_scenarios`, `send_latency_breakdown` and `coordinator_chain_under_fault`.
- **Tolerance-based allocation assertions** — Memory benchmarks use a 5% tolerance (calibrated against serde_json version variance) instead of exact `assert_eq!` counts. This avoids spurious CI failures on dependency updates while catching genuine regressions.
- **Side-effect interceptors** — The interceptor chain benchmark uses `CountingInterceptor` (`AtomicU64`) to verify interceptors are actually invoked during the timed region, with a post-benchmark assertion confirming `calls > 0`.

## Known Measurement Limitations

The measured figures behind these notes are on the generated page, under
[Known Measurement Limitations](../book/src/reference/benchmarks.md#known-measurement-limitations).
They are not repeated here: copies in this file are not regenerated, and
earlier copies had drifted from the page until several contradicted it.

On that page, `generate_book_page.sh` interpolates most figures from the same
criterion estimates the tables come from, so they change whenever the page is
regenerated. Two notes there are still hand-written in the generator, numbers
included: streaming cross-thread scheduling and the `data_volume/save`
confidence intervals. `scripts/check_benchmark_prose.sh` (run by `ci.yml`, and
by `benchmarks.yml` after regenerating) checks three named claims against the
tables — transport payload insensitivity, connection reuse impact, and the
burst client-sharing A/B — not every number on the page.

What the benchmarks do about these effects:

- **Streaming cross-thread scheduling**: `tokio::spawn` can place the SSE
  builder task on a different worker thread from the request. Transport
  streaming benchmarks use a `worker_threads(1)` runtime to eliminate this
  variance. Production code uses `sleep` + reset (not `interval`) and
  `yield_now()` to minimize the impact.

- **`data_volume/get/100K`**: a 100K figure below the 1K/10K ones has been
  traced to CPU cache warming from `populate_store()`. A 4MB cache-busting
  allocation runs between populate and measure. Where the page still shows
  100K as the fastest, the step has not removed the effect, and the 1K/10K
  figures are the representative lookup baseline.

- **Stream volume and channel capacity**: the default broadcast channel
  capacity is 256 events (`DEFAULT_QUEUE_CAPACITY`, raised from 64), and
  `backpressure/stream_volume` runs on both sides of it (up to 502 events).
  Deployments with more than 256 events per task should increase
  `EventQueueManager::with_capacity()`. The `serde_helpers::SerBuffer` module
  can further reduce per-event serialization overhead via thread-local buffer
  reuse.

- **Slow consumer timer calibration**: on shared runners `tokio::time::sleep`
  overshoots its nominal duration. Interpret the slow consumer benchmarks
  against `backpressure/timer_calibration`, not the nominal 1ms / 5ms.

- **`data_volume/save` wide CIs**: `after_prefill/10000` has wide confidence
  intervals and a high outlier rate from BTreeSet rebalancing during sorted
  index inserts; the median is the representative figure. Acceptable
  tradeoff: BTreeSet enables O(page_size) pagination vs O(n) full scans.

- **Dispatch routing**: `direct_handler_invoke` calls the request handler
  directly; `full_http_roundtrip` sends a message through a JSON-RPC server
  over a warm keep-alive connection. The difference is the dispatch and
  transport overhead the direct call bypasses.

- **Cold start vs steady state**: the two measure different things and are
  not comparable. `first_request` creates a fresh server per iteration
  (sample_size=20); `steady_state` measures a full HTTP round-trip over an
  existing keep-alive connection.

- **Agent burst**: `production/agent_burst` builds a client per agent inside
  the measured region, so it includes client construction;
  `production/agent_burst_client_sharing` measures the same burst with one
  shared client.

- **Benchmark server socket reuse**: Servers set `SO_REUSEADDR` + `SO_REUSEPORT`
  and use graceful shutdown to prevent `AddrInUse` errors during rapid cold-start
  cycling on CI runners.

## Cross-Language Comparison

### `cross_language.rs`: this SDK only

Despite its name, the `cross_language` benchmark measures this SDK only. It
defines 5 workloads:

1. **echo_roundtrip** — 256-byte text send/receive (full HTTP)
2. **stream_events** — 3-event stream drain (Working + Artifact + Completed)
3. **serialize_agent_card** — Reference AgentCard ser/de round-trip
4. **concurrent_50** — 50 concurrent sends
5. **minimal_overhead** — Noop executor, pure SDK cost

The JSON files in `cross_language/` describe a reference AgentCard and send
payload for other-language implementations of these workloads. No code in
this repository reads them, and no other-language implementation exists.

### The Python comparison

The Python leg is implemented and its result is published:

```bash
./benches/scripts/cross_language_python.sh        # measure
./benches/scripts/generate_cross_language_page.py # render the book page
```

How it keeps the comparison fair (details in the header of
`cross_language_python.sh` and the docstring of `cross_language_bench.py`):

- **One client drives both servers**: a raw socket sending pre-serialized,
  byte-identical HTTP requests, so only the server differs. Neither SDK's own
  client is used.
- **The same echo contract on both servers**: this SDK's
  `examples/echo-agent` and the official `a2a-sdk` server in
  `itk/agents/python-sdk/agent.py`, the latter on uvicorn's fast path
  (`uvicorn[standard]`, i.e. uvloop + httptools).
- **CPU consumed per request**, not just latency, because latency cannot
  distinguish work from waiting.
- **Two CPU configurations**: `pinned` (each server on one distinct core) and
  `default` (nothing pinned).
- **A floor target** (`cross_language_floor.py`, a constant response with no
  A2A work) that bounds client and kernel cost for sequential latency.
- **Warm-up requests discarded** in every trial; results are the **median of
  trial medians with the min/max spread** across trials.

The Python environment is pinned in `benches/requirements-cross-language.txt`
and installed into a virtualenv by the script (`target/bench-venv` unless
`A2A_BENCH_VENV` is set). Output lands in
`benches/results/cross-language-{pinned,default}.json`, which are committed so
the generated page can be audited against them, and rendered to
[`book/src/reference/cross-language-benchmarks.md`](../book/src/reference/cross-language-benchmarks.md).

There is no Go or JavaScript leg. Stubs for both existed and were removed: they
emitted a fixed dict of zeros while `benches/README.md` described them as
runners and `compare_results.sh` invoked them as data sources. The comparison
table generator went with them — its entire workload vocabulary
(`echo_roundtrip`, `stream_events`, `concurrent_50`, …) was invented to match
the stubs' placeholder dict, so it had never tabulated a real measurement in any
language.

If a Go or JavaScript leg is wanted later, implement it the way the Python one
is built — a real server, a shared client, committed raw results — rather than
reinstating a placeholder that reads like data.

## Interpreting Results

Criterion produces HTML reports in `target/criterion/` with:

- **Statistical significance testing** — detects regressions vs baseline
- **Throughput curves** — msgs/sec and bytes/sec scaling
- **Violin plots** — full latency distribution visualization
- **Comparison overlays** — before/after when using `--save` / `--compare`

### Regression Detection

```bash
# Save baseline (e.g., before a PR)
./benches/scripts/run_benchmarks.sh --save

# Make changes, then compare
./benches/scripts/run_benchmarks.sh --compare
```

Criterion will flag any statistically significant regressions in the terminal
output and in the HTML reports.

For an automated programmatic check — used by the CI regression gate
described below — the `benches/scripts/check_regression.py` script reads
criterion's `change/estimates.json` files and exits non-zero when a
benchmark's **95 %-CI lower bound** of the median change exceeds a
configurable threshold (default 25 %; CI passes 50 %):

```bash
# After `./benches/scripts/run_benchmarks.sh --compare`:
python3 benches/scripts/check_regression.py --target-dir target/criterion --threshold 0.25
```

## CI Integration

The `benchmarks.yml` workflow has two jobs. Push and pull-request runs are
path-filtered: they trigger only when `benches/**`, `crates/**`, `Cargo.toml`
or `Cargo.lock` changes (pull requests also on changes to
`.github/workflows/benchmarks.yml`).

| Trigger | Job | What it does |
|---|---|---|
| Push to `main` | `Run Benchmarks` | Runs the same 13 benchmark targets as `run_benchmarks.sh`, checks that streaming cost stays linear in event count (`scripts/check_streaming_linearity.py`), regenerates `book/src/reference/benchmarks.md` and `book/src/reference/benchmark-dashboard.html`, runs `scripts/check_benchmark_prose.sh`, commits and pushes the two pages (skipped while a release is in flight), and uploads `target/criterion/` as the `criterion-reports` artifact. |
| `workflow_dispatch` | `Run Benchmarks` | Same as above, on demand. |
| Pull request to `main` | `Regression Gate` | Runs `transport_throughput` and `protocol_overhead` twice (base branch, then PR), compares via `check_regression.py` at a 50 % threshold with `*/from_str/16384` excluded (its between-launch variance is not a usable signal; a 75 % override was tried and was not enough — see `regression-gate.md`), and fails CI on a statistically-significant regression. |

### The PR Regression Gate

The regression gate is the CI signal that the root
[`README.md`](../README.md) advertises under "Regression-gated benchmarks."
It fails a PR when any gated benchmark's 95 %-CI lower bound of the median
change exceeds **50 %** — deliberately loose to absorb the noise floor of
shared GitHub-hosted runners without becoming a flake.

The full design — including the statistical test, the threshold
derivation (why 50 % and not 25 %), the per-benchmark exclusions, and the
practical limitations of benchmark-gating on shared CI runners — is documented in
[`book/src/reference/regression-gate.md`](../book/src/reference/regression-gate.md).
Read that page before filing an issue about a CI regression gate
failure: the most common cause is a runner-heterogeneity artifact,
not a real code regression.

Note: CI benchmarks run on shared runners, so absolute numbers will vary.
Use `--save` / `--compare` locally for reliable regression detection.

## Adding New Benchmarks

1. Create a new `.rs` file in `benches/benches/`
2. Add a `[[bench]]` entry in `benches/Cargo.toml`
3. Use helpers from `a2a_benchmarks::{executor, fixtures, server}`
4. Follow the existing pattern: module docstring → helpers → bench functions → criterion group
5. Add to the `BENCHMARKS` array in `scripts/run_benchmarks.sh`, and add a step to the `bench` job in `.github/workflows/benchmarks.yml` if CI should run it
6. To publish its results, add an `emit_table` call for its criterion group prefix in `scripts/generate_book_page.sh`
7. Keep each new file under 500 lines (CONTRIBUTING.md's file-length rule)

## Dependencies

The Rust benchmarks need only `cargo bench`; their dependencies are declared
in `benches/Cargo.toml`, mostly workspace-managed. Other tooling:

- `python3` — used by `generate_book_page.sh`, `generate_dashboard.sh` and
  `check_regression.py`
- `cargo-criterion` (optional) for enhanced HTML reports
- For the cross-language comparison: Python with a virtualenv built from
  `benches/requirements-cross-language.txt` (the script creates it),
  `taskset` (util-linux), and at least 4 logical CPUs
