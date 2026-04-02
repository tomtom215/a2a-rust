<!-- SPDX-License-Identifier: Apache-2.0 -->

# Benchmark Dashboard

Interactive visualization of performance measurements for the `a2a-protocol-sdk`
Rust implementation. All data is auto-generated from
[Criterion.rs](https://github.com/bheisler/criterion.rs) benchmark results by CI.

<div style="margin: 16px 0; padding: 12px 16px; background: #141417; border: 1px solid #232329; border-radius: 8px; font-size: 14px;">
<strong>Open the interactive dashboard:</strong>
<a href="benchmark-dashboard.html" style="color: #00e5cc; text-decoration: none; font-weight: 600;">
Benchmark Dashboard &rarr;
</a>
</div>

## What the dashboard shows

| Tab | Contents |
|-----|----------|
| **Overview** | Key performance highlights, cross-language baseline |
| **Transport & Concurrency** | HTTP round-trip latency, payload scaling, concurrency curves |
| **Serde & Protocol** | Per-type serialization cost, batch scaling, `SerBuffer` vs `to_vec` comparison |
| **Data Volume** | Store operations at 1K-100K tasks, pagination index speedup |
| **Enterprise** | Multi-tenant isolation, rate limiting, CORS, eviction, large histories |
| **Production** | Agent burst scaling, E2E orchestration, cold start, push config CRUD |
| **Memory** | Heap allocation counts, bytes per payload, history depth scaling |

## Methodology

All benchmarks use [Criterion.rs](https://github.com/bheisler/criterion.rs)
with **median ± MAD** (Median Absolute Deviation) as the robust central
tendency measure. Each benchmark runs 100 samples with warm-up iterations to
avoid cold-start artifacts.

- **Environment**: CI runners (`ubuntu-latest`) — use for relative comparisons
  and regression detection, not absolute performance guarantees
- **Transport**: All HTTP benchmarks use loopback (127.0.0.1) to isolate SDK
  overhead from network latency
- **Determinism**: Fixed task IDs and payloads inside measurement loops

## Reproducing locally

```bash
# Run all benchmarks and generate the dashboard
cargo bench -p a2a-benchmarks
./benches/scripts/generate_dashboard.sh

# Open the dashboard
open book/src/reference/benchmark-dashboard.html
```

## See also

- [Benchmark Results](benchmarks.md) — Tabular results with all raw medians
- [Configuration Reference](configuration.md) — Tuning `EventQueueManager` capacity
- [Production Deployment](../deployment/production.md) — Performance best practices
