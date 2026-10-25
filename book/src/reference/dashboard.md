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
| **Overview** | Key performance highlights, payload scaling, event queue throughput |
| **Transport** | HTTP round-trip latency, JSON-RPC vs REST, connection reuse, error paths |
| **Serde** | Per-type serialization cost, batch scaling, `SerBuffer` vs `to_vec`, interceptor overhead |
| **Concurrency** | Sends, streams, and store operations at 1-64 concurrent connections |
| **Data Volume** | Store operations at 1K-100K tasks, history depth scaling |
| **Backpressure** | Stream volume, slow consumer impact, concurrent streams, timer calibration |
| **Enterprise** | Multi-tenant isolation, CORS, rate limiting, eviction, large histories, pagination |
| **Production** | Agent burst scaling, E2E orchestration, cold start, push config CRUD, cross-language |
| **Memory** | Timings of the counting-allocator benchmarks (task and card serialize/deserialize), serialize time by payload size, history-depth scaling; allocation counts are asserted in the benchmarks, not charted |
| **All Results** | Searchable table of all individual benchmark measurements |

## Methodology

All benchmarks use [Criterion.rs](https://github.com/bheisler/criterion.rs)
with the **median** and its 95 % confidence interval (Criterion's default
level) as the robust central tendency measure. Each benchmark runs Criterion's
default of 100 samples unless its group sets a smaller `sample_size` (10–20 for
the slow end-to-end groups), with warm-up iterations to avoid cold-start
artifacts.

- **Environment**: CI runners (`ubuntu-latest`) — use for relative comparisons
  and regression detection, not absolute performance guarantees
- **Transport**: All HTTP benchmarks use loopback (127.0.0.1) to isolate SDK
  overhead from network latency
- **Determinism**: Fixed task IDs and payloads inside measurement loops,
  except the two `data_volume` save groups, which insert a new task ID per
  iteration

## Reproducing locally

```bash
# Run all benchmarks and generate the dashboard
cargo bench -p a2a-benchmarks
./benches/scripts/generate_dashboard.sh

# Build the book, and copy in Chart.js and the fonts as docs.yml does
mdbook build book
cp -r book/static/. book/book/

# Open the dashboard
xdg-open book/book/reference/benchmark-dashboard.html   # `open` on macOS
```

Opening the generated file under `book/src/` directly shows no charts: the
page loads Chart.js and the fonts from `../vendor/` and `../fonts/` of the
built site.

## See also

- [Benchmark Results](benchmarks.md) — Tabular results with all raw medians
- [Configuration Reference](configuration.md) — Tuning `EventQueueManager` capacity
- [Production Deployment](../deployment/production.md) — Performance best practices
