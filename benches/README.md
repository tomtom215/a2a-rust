<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# a2a-rust Benchmark Suite

Criterion-based benchmarks measuring the performance characteristics of the
`a2a-protocol-sdk` Rust implementation. Every benchmark isolates **SDK
overhead** from agent logic by using trivial executors (echo / noop).

## Quick Start

```bash
# Run all benchmarks
./benches/scripts/run_benchmarks.sh

# Run a specific benchmark
cargo bench -p a2a-benchmarks --bench transport_throughput

# Save a baseline for regression detection
./benches/scripts/run_benchmarks.sh --save

# Compare against saved baseline
./benches/scripts/run_benchmarks.sh --compare
```

## Benchmark Modules

| Module | File | What it measures |
|--------|------|------------------|
| **Transport Throughput** | `transport_throughput.rs` | Messages/sec, bytes/sec through JSON-RPC and REST HTTP transports; SSE streaming drain latency; payload size scaling |
| **Protocol Overhead** | `protocol_overhead.rs` | Serde ser/de cost per A2A type (AgentCard, Task, Message, StreamResponse); JSON-RPC envelope overhead; batch scaling |
| **Task Lifecycle** | `task_lifecycle.rs` | TaskStore save/get/list latency; EventQueue write→read throughput; end-to-end create→working→completed via HTTP |
| **Concurrent Agents** | `concurrent_agents.rs` | N simultaneous sends/streams (1, 4, 16, 64); store contention; mixed send+get workloads |
| **Cross-Language** | `cross_language.rs` | Standardized workloads reproducible across all A2A SDK languages (Python, Go, JS, Java, C#/.NET) |
| **Realistic Workloads** | `realistic_workloads.rs` | Multi-turn conversations (1–10 turns); mixed payload complexity (text, file refs, nested metadata); connection reuse vs per-request clients; interceptor chain overhead (0–10 interceptors); complex agent card ser/de (1–100 skills); conversation history scaling |
| **Error Paths** | `error_paths.rs` | Happy path vs error path latency ratio; task-not-found lookup cost; malformed JSON rejection throughput; wrong content-type rejection |
| **Backpressure** | `backpressure.rs` | Stream event volume scaling (3–101 events); slow consumer simulation (1ms/5ms read delays); concurrent stream fan-out under load (1–16 streams) |
| **Data Volume** | `data_volume.rs` | TaskStore get/list/save at 1K–100K pre-populated tasks; context_id filtering at scale; concurrent read contention at 10K tasks; history depth impact on store operations |
| **Memory Overhead** | `memory_overhead.rs` | Heap allocations per serialize/deserialize via counting allocator; allocation scaling with conversation history depth; allocation bytes per payload size (64B–16KB). Uses `iter_custom` with real wall-clock timing and tolerance-based allocation assertions (5% threshold to absorb serde_json version variance). |

## Architecture

```
benches/
├── Cargo.toml                      # Benchmark crate (publish = false)
├── README.md                       # This file
├── src/
│   ├── lib.rs                      # Shared helpers entry point
│   ├── executor.rs                 # EchoExecutor, NoopExecutor, MultiEventExecutor, FailingExecutor
│   ├── fixtures.rs                 # Deterministic test data + realistic payload generators
│   └── server.rs                   # In-process HTTP server startup
├── benches/
│   ├── transport_throughput.rs     # criterion benchmarks
│   ├── protocol_overhead.rs
│   ├── task_lifecycle.rs
│   ├── concurrent_agents.rs
│   ├── cross_language.rs
│   ├── realistic_workloads.rs      # real-world usage patterns
│   ├── error_paths.rs              # failure handling cost
│   ├── backpressure.rs             # streaming under load
│   ├── data_volume.rs              # store ops at scale
│   └── memory_overhead.rs          # heap allocation profiling
├── cross_language/
│   ├── canonical_agent_card.json   # Reference AgentCard for all SDKs
│   └── canonical_send_params.json  # Reference payload (256 bytes)
├── scripts/
│   ├── run_benchmarks.sh           # Run all + collect results
│   ├── compare_results.sh          # Cross-language comparison table
│   ├── cross_language_python.sh    # Python SDK runner
│   ├── cross_language_go.sh        # Go SDK runner
│   └── cross_language_js.sh        # JavaScript SDK runner
└── results/
    └── .gitkeep                    # Result JSONs (gitignored)
```

## What We Benchmark (and Why)

The SDK's value proposition is the **A2A protocol layer and runtime
efficiency**, not the agent logic itself. We benchmark what the SDK owns:

| Dimension | Why it matters at scale |
|-----------|----------------------|
| **Transport throughput** | At Anthropic/Google deployment scales, every microsecond of HTTP overhead per request compounds across billions of daily agent interactions |
| **Protocol overhead** | Serde cost determines the floor for any A2A operation; this is the tax every message pays |
| **Task lifecycle** | Store and queue operations are the backbone of task management; contention here limits vertical scaling |
| **Concurrency** | Agent orchestration is inherently concurrent; degradation curves predict capacity planning |
| **Cross-language** | Hard numbers for deployment decisions — not for competition, but for engineering planning where overhead savings become exponential |
| **Realistic workloads** | Multi-turn conversations, mixed payloads, interceptor chains — the patterns real applications actually use, not synthetic micro-benchmarks |
| **Error paths** | Production systems spend significant time on error handling; benchmarking only the happy path gives an incomplete picture |
| **Backpressure** | Slow consumers and high event volume expose buffering and flow-control overhead that synthetic tests miss |
| **Data volume** | Store operations must scale gracefully from empty to 100K+ tasks; degradation curves predict production capacity |
| **Memory overhead** | Allocation counts and bytes per operation reveal hidden costs that latency benchmarks alone cannot capture |

### What We Do NOT Benchmark

- **Agent intelligence** — LLM quality is an eval problem, not a perf benchmark
- **Task completion quality** — needs human-preference evaluation (LMSYS-style)
- **Network latency** — all benchmarks use loopback to isolate SDK overhead
- **TLS handshake** — benchmarks use plaintext HTTP

### Measurement Rigor

All benchmarks follow these practices for reproducibility and academic-grade rigor:

- **Deterministic inputs** — Fixed task IDs and payloads inside `iter()`. No incrementing counters that change HashMap distribution across iterations.
- **Setup outside measurement** — Store creation, server startup, `EventQueueManager` allocation, and resource initialization happen before `iter()`, not inside it.
- **`debug_assert!` for invariants** — Correctness checks inside measurement loops use `debug_assert!` to avoid string-formatting cost in release builds. Panicking assertions inside `iter()` corrupt timing data.
- **`black_box()` on inputs and outputs** — All measured inputs are wrapped with `criterion::black_box()` to prevent dead-code elimination by the compiler.
- **Tolerance-based allocation assertions** — Memory benchmarks use a 5% tolerance (calibrated against serde_json version variance) instead of exact `assert_eq!` counts. This avoids spurious CI failures on dependency updates while catching genuine regressions.
- **Side-effect interceptors** — The interceptor chain benchmark uses `CountingInterceptor` (`AtomicU64`) to verify interceptors are actually invoked during the timed region, with a post-benchmark assertion confirming `calls > 0`.

## Cross-Language Comparison

The `cross_language` benchmark defines 5 canonical workloads that can be
reproduced identically in every A2A SDK:

1. **echo_roundtrip** — 256-byte text send/receive (full HTTP)
2. **stream_events** — 3-event stream drain (Working + Artifact + Completed)
3. **serialize_agent_card** — Reference AgentCard ser/de round-trip
4. **concurrent_50** — 50 concurrent sends
5. **minimal_overhead** — Noop executor, pure SDK cost

### Fairness Guarantees

- All SDKs hit the **same Rust echo server** (eliminates server-side variance)
- All workloads use the **same JSON payloads** (`cross_language/canonical_*.json`)
- All measurements include **warm-up iterations** before timing
- Results are **median ± MAD** (not mean ± stddev)

### Running Cross-Language Comparisons

```bash
# 1. Run Rust benchmarks
./benches/scripts/run_benchmarks.sh --bench cross_language

# 2. Run other SDK benchmarks (each starts its own Rust echo server)
./benches/scripts/cross_language_python.sh
./benches/scripts/cross_language_go.sh
./benches/scripts/cross_language_js.sh

# 3. Generate comparison table
./benches/scripts/compare_results.sh
```

Results appear in `benches/results/comparison.md`.

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

## CI Integration

The `benchmarks.yml` workflow runs on-demand (`workflow_dispatch`) and on
pushes to `main`. It:

1. Runs all 10 benchmark suites
2. Archives criterion HTML reports as artifacts
3. Comments summary on PRs (when applicable)

Note: CI benchmarks run on shared runners, so absolute numbers will vary.
Use `--save` / `--compare` locally for reliable regression detection.

## Adding New Benchmarks

1. Create a new `.rs` file in `benches/benches/`
2. Add a `[[bench]]` entry in `benches/Cargo.toml`
3. Use helpers from `a2a_benchmarks::{executor, fixtures, server}`
4. Follow the existing pattern: module docstring → helpers → bench functions → criterion group
5. Add to the `BENCHMARKS` array in `scripts/run_benchmarks.sh`
6. Keep each file under 500 lines

## Dependencies

All benchmark dependencies are workspace-managed. No external tooling is
required beyond `cargo bench`. Optional:

- `cargo-criterion` for enhanced HTML reports
- `python3` for cross-language comparison table generation
- Language-specific SDKs for cross-language benchmarks
