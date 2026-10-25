#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F.
#
# Generate a benchmark results page for the mdbook from criterion JSON output.
#
# Usage:
#   ./benches/scripts/generate_book_page.sh
#
# Reads criterion's estimates.json files from target/criterion/ and produces
# a Markdown page at book/src/reference/benchmarks.md suitable for mdbook.
#
# Criterion converts group name slashes to underscores in directory names:
#   benchmark_group("transport/jsonrpc/send") → target/criterion/transport_jsonrpc_send/
#
# Prerequisites:
#   - Run benchmarks first: cargo bench -p a2a-benchmarks
#   - python3 (for JSON parsing)

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
CRITERION_DIR="$REPO_ROOT/target/criterion"
OUTPUT_FILE="$REPO_ROOT/book/src/reference/benchmarks.md"
TIMESTAMP="$(date -u +%Y-%m-%d\ %H:%M\ UTC)"

if [ ! -d "$CRITERION_DIR" ]; then
    echo "Error: No criterion results found at $CRITERION_DIR"
    echo "Run benchmarks first: cargo bench -p a2a-benchmarks"
    exit 1
fi

# ── Helper: extract median from criterion estimates.json ──────────────────

# Extracts the median point estimate in human-readable units.
# Arguments: $1 = path to estimates.json
# Output: "123.4 µs" or "1.23 ms" or "456 ns"
extract_median() {
    local est_file="$1"
    if [ ! -f "$est_file" ]; then
        echo "—"
        return
    fi
    python3 -c "
import json
with open('$est_file') as f:
    d = json.load(f)
ns = d['median']['point_estimate']
if ns >= 1_000_000:
    print(f'{ns / 1_000_000:.2f} ms')
elif ns >= 1_000:
    print(f'{ns / 1_000:.1f} µs')
else:
    print(f'{ns:.0f} ns')
" 2>/dev/null || echo "—"
}

# Extracts the median point estimate as nanoseconds, quantised to exactly the
# precision `extract_median` prints into the tables.
#
# Prose derived from full-precision estimates and tables printed to four
# significant figures do not have to agree, and on 2026-08-18 they did not:
# `generate_book_page.sh` wrote "22.9%" and "128.6 µs" from the raw estimates
# while `check_benchmark_prose.sh`, which re-derives from the committed table,
# computed 22.8% and 128.7 µs from 196.5/241.4 and 323.0/194.3. Both were
# arithmetically right. The gate was red on `main` at 4ddb7ab, and its own
# remedy — "regenerate the page" — could not clear it, because a regenerate
# reproduced the full-precision answer every time.
#
# Rounding the inputs first makes prose and tables agree by construction, which
# is the property the checker is entitled to assume. It costs a tenth of a
# percent of precision in a sentence that is quoting a four-significant-figure
# table anyway.
extract_median_ns_as_tabled() {
    local est_file="$1"
    if [ ! -f "$est_file" ]; then
        echo "0"
        return
    fi
    python3 -c "
import json
with open('$est_file') as f:
    d = json.load(f)
ns = d['median']['point_estimate']
# Mirrors extract_median's unit selection and precision, exactly.
if ns >= 1_000_000:
    print(int(round(round(ns / 1_000_000, 2) * 1_000_000)))
elif ns >= 1_000:
    print(int(round(round(ns / 1_000, 1) * 1_000)))
else:
    print(int(round(ns)))
" 2>/dev/null || echo "0"
}

# Extracts the median point estimate as raw nanoseconds.
extract_median_ns() {
    local est_file="$1"
    if [ ! -f "$est_file" ]; then
        echo "0"
        return
    fi
    python3 -c "
import json
with open('$est_file') as f:
    d = json.load(f)
print(int(d['median']['point_estimate']))
" 2>/dev/null || echo "0"
}

# ── Helpers: derive prose numbers from the same estimates as the tables ───
#
# Prose that quotes a measurement has to be computed, not typed. Between v0.5.0
# and v0.8.0 this page carried "the ~1.4ms HTTP round-trip dominates" and
# "connection reuse saves ~140µs (9%)" while the tables directly above them had
# moved to 189.9 µs and a 39.5% saving — the tables regenerated on every run and
# the prose did not. Same disease the file-length ratchet was built to cure: a
# number nothing recomputes is a number that decays.
#
# Every helper below degrades to "—" (or an empty delta) when its estimates.json
# is missing, matching extract_median, so a partial bench run yields a page with
# visible gaps rather than a confidently wrong sentence.

# Percentage increase from $1 to $2, e.g. "20.0%".
# Median of $1 divided by the burst size $2, rendered as µs-per-agent. Lets the
# burst A/B be quoted per agent, which is the only form comparable to the
# single-request connection-reuse numbers above it.
derive_per_agent() {
    local est_file="$1" n="$2"
    local ns
    ns="$(extract_median_ns "$est_file")"
    if [ "$ns" = "0" ]; then
        echo "—"
        return
    fi
    python3 -c "print(f'{$ns / 1000 / $n:.1f} µs')"
}

# Absolute per-agent saving between two burst arms of size $3, in µs.
derive_per_agent_saving() {
    local slow_file="$1" fast_file="$2" n="$3"
    local slow fast
    slow="$(extract_median_ns "$slow_file")"
    fast="$(extract_median_ns "$fast_file")"
    if [ "$slow" = "0" ] || [ "$fast" = "0" ]; then
        echo "—"
        return
    fi
    python3 -c "print(f'{($slow - $fast) / 1000 / $n:.1f} µs ({(($slow - $fast) / $slow) * 100:.1f}%)')"
}

derive_pct_increase() {
    local from_file="$1" to_file="$2"
    local from to
    # Quantised: this figure is checked against the rounded table. See
    # extract_median_ns_as_tabled.
    from="$(extract_median_ns_as_tabled "$from_file")"
    to="$(extract_median_ns_as_tabled "$to_file")"
    if [ "$from" = "0" ] || [ "$to" = "0" ]; then
        echo "—"
        return
    fi
    python3 -c "print(f'{(($to - $from) / $from) * 100:.1f}%')"
}

# Absolute saving going from $1 (slow path) to $2 (fast path), rendered as
# "123.5 µs (39.5%)". The percentage is of the slow path, so it reads as
# "reuse removes this share of the cost".
derive_saving() {
    local slow_file="$1" fast_file="$2"
    local slow fast
    # Quantised: this figure is checked against the rounded table. See
    # extract_median_ns_as_tabled.
    slow="$(extract_median_ns_as_tabled "$slow_file")"
    fast="$(extract_median_ns_as_tabled "$fast_file")"
    if [ "$slow" = "0" ] || [ "$fast" = "0" ]; then
        echo "—"
        return
    fi
    python3 -c "
saved = $slow - $fast
pct = (saved / $slow) * 100
unit = f'{saved / 1_000:.1f} µs' if saved >= 1_000 else f'{saved:.0f} ns'
print(f'{unit} ({pct:.1f}%)')
"
}

# Difference between two medians ($1 larger, $2 smaller), rendered like a table
# cell, e.g. "132.4 µs". Quantised like derive_saving, for the same reason.
derive_diff() {
    local big_file="$1" small_file="$2"
    local big small
    big="$(extract_median_ns_as_tabled "$big_file")"
    small="$(extract_median_ns_as_tabled "$small_file")"
    if [ "$big" = "0" ] || [ "$small" = "0" ]; then
        echo "—"
        return
    fi
    python3 -c "
d = $big - $small
if d >= 1_000_000:
    print(f'{d / 1_000_000:.2f} ms')
elif d >= 1_000:
    print(f'{d / 1_000:.1f} µs')
else:
    print(f'{d:.0f} ns')
"
}

# Ratio of two medians ($1 over $2), e.g. "3.4×". With a third argument of
# "raw" it uses the whole-nanosecond figures the count-mode tables print;
# otherwise the figures the time tables print.
derive_ratio() {
    local top_file="$1" bottom_file="$2" mode="${3:-tabled}"
    local top bottom
    if [ "$mode" = "raw" ]; then
        top="$(extract_median_ns "$top_file")"
        bottom="$(extract_median_ns "$bottom_file")"
    else
        top="$(extract_median_ns_as_tabled "$top_file")"
        bottom="$(extract_median_ns_as_tabled "$bottom_file")"
    fi
    if [ "$top" = "0" ] || [ "$bottom" = "0" ]; then
        echo "—"
        return
    fi
    python3 -c "print(f'{$top / $bottom:.1f}×')"
}

# Cost of each extra event between two stream-volume rows: the difference of
# the two medians over the difference in event count ($3), e.g. "5.0 µs".
# Computed from the figures the table prints, so a reader can redo it.
derive_marginal_per_event() {
    local lo_file="$1" hi_file="$2" events="$3"
    local lo hi
    lo="$(extract_median_ns_as_tabled "$lo_file")"
    hi="$(extract_median_ns_as_tabled "$hi_file")"
    if [ "$lo" = "0" ] || [ "$hi" = "0" ]; then
        echo "—"
        return
    fi
    python3 -c "print(f'{($hi - $lo) / $events / 1000:.1f} µs')"
}

# Per-agent saving of a burst A/B pair ($1 per-agent-client arm, $2 shared-client
# arm, $3 burst size) as a share of the single-request saving ($4 new client,
# $5 reused client), e.g. "26%". The burst arms are raw nanoseconds, as in
# derive_per_agent_saving; the single-request pair is quantised, as in
# derive_saving.
derive_saving_share() {
    local slow_file="$1" fast_file="$2" n="$3" new_file="$4" reused_file="$5"
    local slow fast new reused
    slow="$(extract_median_ns "$slow_file")"
    fast="$(extract_median_ns "$fast_file")"
    new="$(extract_median_ns_as_tabled "$new_file")"
    reused="$(extract_median_ns_as_tabled "$reused_file")"
    if [ "$slow" = "0" ] || [ "$fast" = "0" ] || [ "$new" = "0" ] || [ "$reused" = "0" ]; then
        echo "—"
        return
    fi
    python3 -c "print(f'{(($slow - $fast) / $n) / ($new - $reused) * 100:.0f}%')"
}

# How many times larger the saving from $1 (slow path) to $2 (fast path) is than
# a stated percentage ($3), e.g. "4.6×". Used to size an older, understated
# figure against the one measured now.
derive_saving_vs_pct() {
    local slow_file="$1" fast_file="$2" old_pct="$3"
    local slow fast
    slow="$(extract_median_ns_as_tabled "$slow_file")"
    fast="$(extract_median_ns_as_tabled "$fast_file")"
    if [ "$slow" = "0" ] || [ "$fast" = "0" ]; then
        echo "—"
        return
    fi
    python3 -c "print(f'{(($slow - $fast) / $slow) * 100 / $old_pct:.1f}×')"
}

# 95% confidence interval of a median, rendered like a table cell but with one
# more digit, so a tight interval does not collapse into a single value.
extract_median_ci() {
    local est_file="$1"
    if [ ! -f "$est_file" ]; then
        echo "—"
        return
    fi
    python3 -c "
import json
with open('$est_file') as f:
    ci = json.load(f)['median']['confidence_interval']
lo, hi = ci['lower_bound'], ci['upper_bound']
def fmt(ns):
    if hi >= 1_000_000:
        return f'{ns / 1_000_000:.3f} ms'
    if hi >= 1_000:
        return f'{ns / 1_000:.2f} µs'
    return f'{ns:.0f} ns'
print(f'[{fmt(lo)}, {fmt(hi)}]')
" 2>/dev/null || echo "—"
}

# How `from_str` compares with `from_slice` across the payload sizes of
# protocol_payload_scaling: the range of time saved where it is faster, the
# 64 B pair, and the sizes where it is not. Computed from the estimates behind
# the table rows, so the sentence cannot outlive them.
derive_from_str_summary() {
    python3 - "$CRITERION_DIR/protocol_payload_scaling" <<'PY' 2>/dev/null || echo "—"
import json
import sys

base = sys.argv[1]
sizes = [(64, "64 B"), (256, "256 B"), (1024, "1 KiB"), (4096, "4 KiB"),
         (16384, "16 KiB"), (102400, "100 KiB"), (1048576, "1 MiB")]


def med(name, size):
    with open(f"{base}/{name}/{size}/new/estimates.json") as f:
        return json.load(f)["median"]["point_estimate"]


def fmt(ns):
    if ns >= 1_000_000:
        return f"{ns / 1_000_000:.2f} ms"
    if ns >= 1_000:
        return f"{ns / 1_000:.1f} µs"
    return f"{ns:.0f} ns"


faster, slower = [], []
for size, label in sizes:
    pct = (med("from_slice", size) - med("from_str", size)) / med("from_slice", size) * 100
    (faster if pct > 0 else slower).append((pct, label))

if not faster:
    print("no less time than `from_slice` at any payload size")
    sys.exit(0)
lo = min(p for p, _ in faster)
hi = max(p for p, _ in faster)
rng = f"{lo:.0f}%" if f"{lo:.0f}" == f"{hi:.0f}" else f"{lo:.0f}–{hi:.0f}%"
where = (
    "at every payload size"
    if not slower
    else f"at {len(faster)} of {len(sizes)} payload sizes"
)
first = f"{rng} less time than `from_slice` {where}"
second = f"({fmt(med('from_str', 64))} against {fmt(med('from_slice', 64))} at 64 B)"
if slower:
    second += f", and more time at {', '.join(label for _, label in slower)}"
print(first)
print(second)
PY
}

# ── Helper: emit a results table for all matching criterion directories ───

# Arguments: $1 = glob pattern prefix (matched against top-level criterion dirs)
#            $2 = "time" or "count" (display mode)
# Criterion structure: target/criterion/<group_name>/<bench_name>/new/estimates.json
# For parameterized benchmarks: target/criterion/<group_name>/<bench_name>/<param>/new/estimates.json
emit_table() {
    local prefix="$1"
    local mode="${2:-time}"
    local found=false

    # Collect all estimates.json files under matching directories
    local results=()
    for dir in "$CRITERION_DIR"/${prefix}*/; do
        [ -d "$dir" ] || continue
        while IFS= read -r est; do
            results+=("$est")
        done < <(find "$dir" -name "estimates.json" -path "*/new/*" 2>/dev/null | sort)
    done

    if [ ${#results[@]} -eq 0 ]; then
        return
    fi

    if [ "$mode" = "count" ]; then
        printf "| Benchmark | Median (ns) |\n" >> "$OUTPUT_FILE"
        printf "|-----------|-------------|\n" >> "$OUTPUT_FILE"
    else
        printf "| Benchmark | Median |\n" >> "$OUTPUT_FILE"
        printf "|-----------|--------|\n" >> "$OUTPUT_FILE"
    fi

    for est in "${results[@]}"; do
        rel="${est#$CRITERION_DIR/}"
        bench_name="${rel%/new/estimates.json}"
        if [ "$mode" = "count" ]; then
            raw_ns=$(extract_median_ns "$est")
            if [ "$raw_ns" = "0" ]; then
                printf "| \`%s\` | — |\n" "$bench_name" >> "$OUTPUT_FILE"
            else
                printf "| \`%s\` | %s |\n" "$bench_name" "$raw_ns" >> "$OUTPUT_FILE"
            fi
        else
            median=$(extract_median "$est")
            printf "| \`%s\` | %s |\n" "$bench_name" "$median" >> "$OUTPUT_FILE"
        fi
    done

    echo "" >> "$OUTPUT_FILE"
}

# ── Generate the page ─────────────────────────────────────────────────────

cat > "$OUTPUT_FILE" <<'HEADER'
<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Auto-generated by benches/scripts/generate_book_page.sh — do not edit manually -->

# Benchmark Results

Performance measurements for the `a2a-protocol-sdk` Rust implementation,
generated by [Criterion.rs](https://github.com/bheisler/criterion.rs) statistical
benchmarking.

> **Note**: These numbers were collected on CI runners (`ubuntu-latest`). Absolute
> values will vary by hardware. Use these for **relative comparisons** between
> operations and for **regression detection** across releases — not as guarantees
> of production performance on your specific hardware.
>
> To reproduce on your own machine: `cargo bench -p a2a-benchmarks`

HEADER

printf "**Last updated:** %s  \n" "$TIMESTAMP" >> "$OUTPUT_FILE"
printf "**Rust version:** %s  \n" "$(rustc --version 2>/dev/null || echo 'unknown')" >> "$OUTPUT_FILE"
printf "**Platform:** %s  \n\n" "$(uname -s)-$(uname -m)" >> "$OUTPUT_FILE"

# What the figures were measured with. OpenTelemetry instrumentation became a
# default feature of the SDK crates on 2026-10-18 (commit ce719b0), and the
# benchmark crate builds them with their default features, so a page generated
# on or after that date includes it and an earlier one does not. The sentence
# is chosen by the generation date because CI regenerates this page: a fixed
# "measured before" would turn false at the first run after the change.
# docs/readiness-bar.md measured the cost of default `tracing` and `otel` with
# nothing installed: 4.6% and 6.6% of the CPU time of a blocking send.
OTEL_DEFAULT_SINCE="2026-10-18"
if [[ "${TIMESTAMP%% *}" < "$OTEL_DEFAULT_SINCE" ]]; then
    cat >> "$OUTPUT_FILE" <<'OTEL_BEFORE'
> **Feature set**: these figures were measured before OpenTelemetry
> instrumentation became a default feature of the SDK crates (0.15.0).
> `docs/readiness-bar.md` measured default `tracing` and `otel`, with nothing
> installed, at 4.6–6.6% of the CPU time of a blocking send.

OTEL_BEFORE
else
    cat >> "$OUTPUT_FILE" <<'OTEL_AFTER'
> **Feature set**: these figures were measured with OpenTelemetry
> instrumentation, a default feature of the SDK crates since 0.15.0.
> `docs/readiness-bar.md` measured default `tracing` and `otel`, with nothing
> installed, at 4.6–6.6% of the CPU time of a blocking send.

OTEL_AFTER
fi

cat >> "$OUTPUT_FILE" <<'DASH_LINK'
> **Interactive dashboard**: See the [Benchmark Dashboard](dashboard.md) for
> charts, visual comparisons, and drill-down analysis of these results.

DASH_LINK

# ── Transport Throughput ──────────────────────────────────────────────────

cat >> "$OUTPUT_FILE" <<'SECTION'
## Transport Throughput

End-to-end HTTP round-trip latency through JSON-RPC and REST transports.
All measurements use loopback (127.0.0.1) to isolate SDK overhead from network latency.

SECTION

# Criterion dirs: transport_jsonrpc_send, transport_jsonrpc_stream, transport_rest_send, etc.
emit_table "transport_"

# ── Protocol Overhead ─────────────────────────────────────────────────────

cat >> "$OUTPUT_FILE" <<'SECTION'
## Protocol Overhead

Serialization and deserialization cost per A2A type. This is the baseline tax
every message pays regardless of transport.

Includes `protocol/payload_scaling` benchmarks that measure pure serde cost from
64B to 1MB — the correct regression detection target for serialization changes.
Also compares `serde_json::to_vec` vs `SerBuffer` (thread-local reuse) and
`from_slice` vs `from_str` (borrowed deserialization) paths.

SECTION

# Criterion dirs: protocol_type_serde, protocol_jsonrpc_envelope, protocol_stream_events, protocol_batch, protocol_payload_scaling
emit_table "protocol_"

# ── Task Lifecycle ────────────────────────────────────────────────────────

cat >> "$OUTPUT_FILE" <<'SECTION'
## Task Lifecycle

TaskStore and EventQueue operations — the backbone of task management.

SECTION

# Criterion dirs: lifecycle_store_save, lifecycle_store_get, lifecycle_store_list, lifecycle_queue, lifecycle_e2e
emit_table "lifecycle_"

# ── Concurrent Agents ─────────────────────────────────────────────────────

cat >> "$OUTPUT_FILE" <<'SECTION'
## Concurrent Agents

Scaling behavior under parallel load — how latency changes as
concurrency increases from 1 to 64 simultaneous operations.

SECTION

# Criterion dirs: concurrent_sends, concurrent_streams, concurrent_store, concurrent_mixed
emit_table "concurrent_"

# ── Realistic Workloads ───────────────────────────────────────────────────

cat >> "$OUTPUT_FILE" <<'SECTION'
## Realistic Workloads

Production-like usage patterns: multi-turn conversations, mixed payloads,
interceptor chains, and connection reuse vs per-request clients.

SECTION

# Criterion dirs: realistic_multi_turn, realistic_payload_complexity, realistic_connection, etc.
emit_table "realistic_"

# ── Error Paths ───────────────────────────────────────────────────────────

cat >> "$OUTPUT_FILE" <<'SECTION'
## Error Paths

Cost of error handling — comparing happy path latency to error path latency.
Production systems spend significant time on error paths; benchmarking only
the happy path gives an incomplete picture.

SECTION

# Criterion dirs: errors_happy_vs_error, errors_task_not_found, errors_malformed_request
emit_table "errors_"

# ── Backpressure ──────────────────────────────────────────────────────────

cat >> "$OUTPUT_FILE" <<'SECTION'
## Streaming & Backpressure

Stream throughput under varying event volumes and consumer speeds.
Reveals buffering and flow-control overhead that synthetic single-event tests miss.

The default broadcast channel capacity is 256 events (raised from 64 in
v0.5.0). Deployments with >256 events/task can set a higher value with
`RequestHandlerBuilder::with_event_queue_capacity()`. Since 0.15.0 a stream
that falls behind catches up from the task's event log (the in-memory stores
keep the last 512 events of a task by default), so a larger queue avoids the
catch-up reads rather than being the only protection against `streamLagged`.

SECTION

# Criterion dirs: backpressure_stream_volume, backpressure_slow_consumer, backpressure_concurrent_streams
emit_table "backpressure_"

# ── Data Volume ───────────────────────────────────────────────────────────

cat >> "$OUTPUT_FILE" <<'SECTION'
## Data Volume Scaling

TaskStore performance at realistic data volumes (1K to 100K tasks).
Shows how store operations scale as data accumulates over time.

SECTION

# Criterion dirs: data_volume_get, data_volume_list, data_volume_save, data_volume_concurrent_reads, data_volume_history_depth
emit_table "data_volume_"

# ── Memory Overhead ───────────────────────────────────────────────────────

cat >> "$OUTPUT_FILE" <<'SECTION'
## Memory Overhead

Wall-clock time per operation, in nanoseconds, with a counting allocator
(`#[global_allocator]`) installed. Each iteration counts its heap allocations,
and the benchmark asserts that the count stays within 5% of a figure measured
once beforehand, but the counts are not published: Criterion records the
elapsed time. Read these rows as ns per operation, not as allocation counts or
bytes.

| Metric | Unit |
|--------|------|
| `*_alloc_count` | ns per operation (the allocation count is asserted, not reported) |
| `*_bytes_per_payload` | ns per operation (the bytes allocated are asserted, not reported) |

SECTION

# Criterion dirs: memory_serialize, memory_deserialize, memory_history_scaling, memory_bytes_per_payload
emit_table "memory_" "count"

# ── Cross-Language Comparison ─────────────────────────────────────────────

cat >> "$OUTPUT_FILE" <<'SECTION'
## Cross-Language Comparison

Rust-side criterion baselines for the workloads in `benches/cross_language/`.
Every row below is this SDK only. The measured cross-SDK comparison — this
SDK's server against the official Python SDK's server — is on
[Cross-Language Benchmark](cross-language-benchmarks.md).

SECTION

# Criterion dirs: cross_language_echo_roundtrip, cross_language_stream_events, etc.
emit_table "cross_language_"

# ── Enterprise Scenarios ─────────────────────────────────────────────────

cat >> "$OUTPUT_FILE" <<'SECTION'
## Enterprise Scenarios

Workloads modeled on deployment patterns, run in one process over loopback:
multi-tenant isolation, push notification management, eviction under memory
pressure, rate limiting, CORS handling, read/write mix ratios, and large
conversation histories.

SECTION

# Criterion dirs: enterprise_multi_tenant, enterprise_push_config, enterprise_eviction, etc.
emit_table "enterprise_"

# ── Production Scenarios ─────────────────────────────────────────────────

cat >> "$OUTPUT_FILE" <<'SECTION'
## Production Scenarios

Full end-to-end workflows exercising the complete SDK pipeline in scenarios
modeled on real deployments (single process, loopback): task reconnection, cold
start latency, concurrent race conditions, multi-context orchestration, push
config lifecycle, parallel agent bursts, and dispatch routing overhead
isolation.

SECTION

# Criterion dirs: production_subscribe_to_task, production_cold_start, production_e2e_orchestration, etc.
emit_table "production_"

# ── Advanced Scenarios ──────────────────────────────────────────────────

cat >> "$OUTPUT_FILE" <<'SECTION'
## Advanced Scenarios

SDK capabilities exercising previously-unbenchmarked paths: tenant resolver
overhead, agent card hot-reload and discovery, subscribe fan-out for
reconnection bursts, streaming artifact accumulation cost, pagination full
walk, and extended agent card round-trip.

SECTION

# Criterion dirs: advanced_tenant_resolver, advanced_agent_card_hot_reload, advanced_agent_card_discovery, etc.
emit_table "advanced_"

# ── Agent-Level Latency Under Fault ──────────────────────────────────────

cat >> "$OUTPUT_FILE" <<'SECTION'
## Agent-Level Latency Under Fault

> **Not run by the benchmarks workflow; no results are published here.**
> `benchmarks.yml` does not run this bench, so the two groups below have no
> tables. To measure it locally:
> `cargo bench -p a2a-benchmarks --bench coordinator_chain_under_fault`.

End-to-end latency through a **5-hop in-process coordinator chain** as the
links between hops are made progressively less reliable. Unlike every other
benchmark on this page, this one does not measure SDK-layer overhead — it
measures the characteristic an agent-harness reviewer actually wants:
"what is the end-to-end latency of an agent chain when the network
between agents is unreliable, and how well do per-hop retries absorb it?"

The topology is:

```text
test client ─[link 0]─▶ coord 1 ─[link 1]─▶ coord 2 ─[link 2]─▶ coord 3 ─[link 3]─▶ coord 4 ─[link 4]─▶ leaf
```

Every coordinator forwards the message to the next hop via a pre-built
`A2aClient` wrapped in a `FaultInjectingTransport`. Each link applies its
own independent fault profile, so per-hop faults compound end-to-end the
way they would in a real deployment. Coordinators 1–4 retry their
downstream call up to 3 times on retryable errors; the bench harness
additionally retries the top-level `send_message` up to 8 times so the
published error rates have effectively-zero unrecoverable-failure
probability.

**Honest caveats** — read these before interpreting the numbers:

- **In-process, not network faults.** The injected "error" is a
  synthetic `ClientError::Timeout` returned before the wrapped transport
  is called. This exercises the SDK's retry path faithfully, but does
  *not* exercise TCP congestion control, DNS resolution, or
  transport-level head-of-line blocking. Treat the numbers as "latency
  under SDK-level retransmission pressure," not "latency under real
  network loss."
- **One topology.** Sequential delegation is the simplest multi-agent
  shape. Critic loops, parallel fan-out with deadline propagation, and
  plan-and-execute with replanning would be more rubric-relevant — this
  benchmark does not claim to cover those.
- **One benchmark does not retroactively make the other suites
  agent-level.** It is deliberately additive: the first concrete data
  point in the "agent-level latency under fault" shape that the rest of
  the suite was missing entirely.

### Group 1: per-hop latency injection (zero errors)

Varies per-link latency from 0 µs to 20 000 µs with zero synthetic
errors, isolating the chain's latency-compounding factor from retry
jitter. Five hops × per-hop latency gives the lower bound, plus the
JSON-RPC loopback cost of five nested sends (not measured here).

SECTION

emit_table "coordinator_chain_5hop_latency_injection"

cat >> "$OUTPUT_FILE" <<'SECTION'
### Group 2: per-hop error injection (3 retries per hop + 8 outer retries)

Varies per-link synthetic-fault rate from 0% to 5% with zero added
latency. Each coordinator retries its downstream call up to 3 times on
retryable errors; the bench harness retries the top-level call up to 8
times. Records *successful-path latency including retry cost*, which is
what "steady-state end-to-end latency under fault" means in practice.

SECTION

emit_table "coordinator_chain_5hop_error_injection"

# ── Footer ────────────────────────────────────────────────────────────────

cat >> "$OUTPUT_FILE" <<'FOOTER'
---

## Known Measurement Limitations

These notes help interpret benchmark results accurately and avoid
misdiagnosing CI variance as real performance changes.

### Streaming cross-thread scheduling

On N-core systems, `tokio::spawn` places the SSE builder task on a different
worker thread with (N-1)/N probability. This was once held to cost ~500µs per
request and to cause the ~24% bimodal distribution in the streaming
benchmarks; a later measurement (the 0.9.0 changelog entry) found cross-thread
scheduling real but second-order, about 50 µs of a 191 µs send.

**Mitigations (v0.5.0):** The SSE builder uses `sleep` + reset (not
`interval`) to eliminate timer wheel entries during active streaming.
Transport streaming benchmarks use `worker_threads(1)` runtime to eliminate
cross-thread variance entirely (24 high severe → 4 high mild outliers, 3×
tighter confidence intervals).

FOOTER

# Quoted from the tables above, so computed from the same estimates.json files.
DV_GET="$CRITERION_DIR/data_volume_get/lookup"
DV_SAVE_10K="$CRITERION_DIR/data_volume_save/after_prefill/10000/new/estimates.json"
STREAM_VOL="$CRITERION_DIR/backpressure_stream_volume"

cat >> "$OUTPUT_FILE" <<SECTION
### Data volume get() at 100K tasks

\`data_volume_get/lookup\` reports $(extract_median "$DV_GET/1000/new/estimates.json") at 1K
tasks, $(extract_median "$DV_GET/10000/new/estimates.json") at 10K and $(extract_median "$DV_GET/100000/new/estimates.json") at 100K. A 100K
figure below the 1K/10K ones has previously been traced to a **CPU cache
warming artifact** from the large \`populate_store()\` setup filling L1/L2
caches; a 4MB cache-busting step was added in v0.5.0 to flush caches between
populate and measure. Where the 100K figure is still the lowest, that step has
not removed the effect, and the 1K/10K numbers are the representative O(1)
lookup baseline.

### Stream volume per-event cost

The default broadcast channel capacity is **256** events (raised from 64 in
v0.5.0). \`backpressure_stream_volume\` does not run its larger rows at that
default: the bench sizes the event queue above the event count (512 slots for
the 52- and 252-event rows, 1,024 for the 502-event row), so it measures
streaming cost, not capacity overflow. It reports
$(extract_median "$STREAM_VOL/52_events/new/estimates.json") for 52 events, $(extract_median "$STREAM_VOL/252_events/new/estimates.json") for 252 and $(extract_median "$STREAM_VOL/502_events/new/estimates.json") for 502:
$(derive_marginal_per_event "$STREAM_VOL/52_events/new/estimates.json" "$STREAM_VOL/252_events/new/estimates.json" 200) per added event from 52 to 252 events and
$(derive_marginal_per_event "$STREAM_VOL/252_events/new/estimates.json" "$STREAM_VOL/502_events/new/estimates.json" 250) from 252 to 502.

Production deployments expecting >256 events/task can raise
\`RequestHandlerBuilder::with_event_queue_capacity()\` to match their peak volume.

SECTION

# These two sections quote measurements, so they are computed from the same
# estimates.json files the tables above are built from. Do not fold them back
# into a quoted heredoc — that is what let them drift for three minor versions.

TRANSPORT_64="$CRITERION_DIR/transport_payload_scaling/jsonrpc_send/64/new/estimates.json"
TRANSPORT_16K="$CRITERION_DIR/transport_payload_scaling/jsonrpc_send/16384/new/estimates.json"
TRANSPORT_100K="$CRITERION_DIR/transport_payload_scaling/jsonrpc_send/102400/new/estimates.json"
TRANSPORT_1M="$CRITERION_DIR/transport_payload_scaling/jsonrpc_send/1048576/new/estimates.json"
CONN_NEW="$CRITERION_DIR/realistic_connection/new_client_per_request/new/estimates.json"
CONN_REUSED="$CRITERION_DIR/realistic_connection/reused_client/new/estimates.json"
BURST_AB="$CRITERION_DIR/production_agent_burst_client_sharing"
BURST_PER_AGENT="$BURST_AB/per_agent_client_agents/100/new/estimates.json"
BURST_SHARED="$BURST_AB/shared_client_agents/100/new/estimates.json"

cat >> "$OUTPUT_FILE" <<SECTION
### Transport payload insensitivity

Transport benchmarks (64B → 16KB) show a $(derive_pct_increase "$TRANSPORT_64" "$TRANSPORT_16K") latency increase for a
256× payload increase, because the $(extract_median "$TRANSPORT_64") HTTP round-trip dominates. Serde
regressions are hard to detect via the 64B–16KB transport benchmarks; the
100KiB ($(extract_median "$TRANSPORT_100K")) and 1MiB ($(extract_median "$TRANSPORT_1M")) rows are where payload size begins to
dominate. Use the
\`protocol/payload_scaling\` isolation benchmarks (64B → 1MB, pure serde)
for serialization regression detection.

### Connection reuse impact

Connection reuse saves $(derive_saving "$CONN_NEW" "$CONN_REUSED") on loopback —
$(extract_median "$CONN_NEW") per request when the client is rebuilt each time,
versus $(extract_median "$CONN_REUSED") when it is shared. On real networks with TLS the
saving should be larger still, since a new client also pays a TLS handshake;
this page does not measure that. Best practice: create one
\`A2aClient\` at startup and share via \`Arc\` across request handlers.

Two consequences worth spelling out, because both have bitten this repo:

- A benchmark that builds a client inside its measured region is measuring
  client construction, not the thing it names. \`production_agent_burst\` does
  exactly this, and its per-agent cost tracks
  $(extract_median "$CONN_NEW") — the rebuild-every-time number — rather than
  the shared-client one.
- Quoting this saving as a small percentage (an older revision of this page
  said 9%) understates it by $(derive_saving_vs_pct "$CONN_NEW" "$CONN_REUSED" 9).
  It is a large fraction of a loopback request, not a rounding error.

### What sharing a client is actually worth under concurrency

The bullet above says \`production_agent_burst\` tracks the rebuild-every-time
number. That is true, and it invites a wrong inference: that sharing a client
would move it to the reused figure. It does not, and
\`production/agent_burst_client_sharing\` was added to measure the difference
rather than reason about it. Both arms run the same server, the same three
operations per agent, and the same burst sizes; only the client's provenance
changes.

| Burst | Client per agent | Shared \`Arc<A2aClient>\` | Saved per agent |
|---|---|---|---|
| 10 | $(derive_per_agent "$BURST_AB/per_agent_client_agents/10/new/estimates.json" 10) | $(derive_per_agent "$BURST_AB/shared_client_agents/10/new/estimates.json" 10) | $(derive_per_agent_saving "$BURST_AB/per_agent_client_agents/10/new/estimates.json" "$BURST_AB/shared_client_agents/10/new/estimates.json" 10) |
| 50 | $(derive_per_agent "$BURST_AB/per_agent_client_agents/50/new/estimates.json" 50) | $(derive_per_agent "$BURST_AB/shared_client_agents/50/new/estimates.json" 50) | $(derive_per_agent_saving "$BURST_AB/per_agent_client_agents/50/new/estimates.json" "$BURST_AB/shared_client_agents/50/new/estimates.json" 50) |
| 100 | $(derive_per_agent "$BURST_PER_AGENT" 100) | $(derive_per_agent "$BURST_SHARED" 100) | $(derive_per_agent_saving "$BURST_PER_AGENT" "$BURST_SHARED" 100) |

Sharing wins at every burst size, and the medians' 95% confidence intervals are
disjoint in all three, so the direction is not noise. But the size of the win is
only a fraction of what the single-request comparison above predicts, $(derive_saving_share "$BURST_AB/per_agent_client_agents/10/new/estimates.json" "$BURST_AB/shared_client_agents/10/new/estimates.json" 10 "$CONN_NEW" "$CONN_REUSED") of it
at 10 agents, $(derive_saving_share "$BURST_AB/per_agent_client_agents/50/new/estimates.json" "$BURST_AB/shared_client_agents/50/new/estimates.json" 50 "$CONN_NEW" "$CONN_REUSED") at 50 and $(derive_saving_share "$BURST_PER_AGENT" "$BURST_SHARED" 100 "$CONN_NEW" "$CONN_REUSED") at 100, where it is
$(derive_per_agent_saving "$BURST_PER_AGENT" "$BURST_SHARED" 100) per agent
against the $(derive_saving "$CONN_NEW" "$CONN_REUSED") that
\`reused_client\` versus \`new_client_per_request\` would lead you to expect.

The reason is that the two arms differ in two coupled ways, not one. A shared
client skips per-agent construction *and* per-agent connection setup, but it
also puts every concurrent agent on one connection pool, and that contention
gives part of the saving back. The sequential benchmark has no contention to
pay, which is why its number is the optimistic bound rather than the forecast.

The practical reading: share the client — it is free to do and wins at every
size measured — but size capacity from the burst figures, not from the
single-request saving.

SECTION

# Quoted from the tables above, so computed from the same estimates.json files.
# Counts use extract_median_ns, which is what the count-mode tables print.
PSC="$CRITERION_DIR/protocol_payload_scaling"
MEM_DESER_TASK="$CRITERION_DIR/memory_deserialize/task_alloc_count/new/estimates.json"
MEM_SER_TASK="$CRITERION_DIR/memory_serialize/task_alloc_count/new/estimates.json"
cat >> "$OUTPUT_FILE" <<SECTION
### Deserialization versus serialization cost

In the \`memory_*\` rows above, deserialization takes several times as long as
serialization (Task: $(extract_median_ns "$MEM_DESER_TASK") ns vs $(extract_median_ns "$MEM_SER_TASK") ns, $(derive_ratio "$MEM_DESER_TASK" "$MEM_SER_TASK" raw)).
This is inherent to serde_json's parsing model: every field creates an
intermediate \`String\`/\`Vec\` allocation during parsing. The
\`serde_helpers::deser_from_str()\` helper uses serde_json's \`from_str\` path,
which in the \`protocol_payload_scaling\` rows above takes
$(derive_from_str_summary).

The \`serde_helpers::SerBuffer\` provides thread-local buffer reuse for
serialization. In the same rows it takes $(extract_median "$PSC/ser_buffer/64/new/estimates.json") against \`to_vec\`'s $(extract_median "$PSC/to_vec/64/new/estimates.json") at
64 B ($(derive_ratio "$PSC/to_vec/64/new/estimates.json" "$PSC/ser_buffer/64/new/estimates.json")) and $(extract_median "$PSC/ser_buffer/256/new/estimates.json") against $(extract_median "$PSC/to_vec/256/new/estimates.json") at 256 B, and the two
converge as the payload grows: $(extract_median "$PSC/ser_buffer/1048576/new/estimates.json") against $(extract_median "$PSC/to_vec/1048576/new/estimates.json") at 1 MiB.

### History depth scaling

History depth scales roughly linearly: \`memory_history_scaling\` records
$(extract_median_ns "$CRITERION_DIR/memory_history_scaling/deserialize_allocs/1/new/estimates.json") ns to deserialize at 1 turn and $(extract_median_ns "$CRITERION_DIR/memory_history_scaling/deserialize_allocs/50/new/estimates.json") ns at 50 turns
($(extract_median_ns "$CRITERION_DIR/memory_history_scaling/serialize_allocs/1/new/estimates.json") ns and $(extract_median_ns "$CRITERION_DIR/memory_history_scaling/serialize_allocs/50/new/estimates.json") ns to serialize), so a store that parses the
stored task on every \`get()\` (the SQL stores) pays that growth per read;
\`InMemoryTaskStore::get\` clones instead. The \`serde_helpers\` module
provides optimized paths; for maximum throughput on deep histories, consider
storing pre-serialized bytes alongside parsed structs to avoid re-parsing on
every read.

### Artifact accumulation clone cost

\`task_clone_at_depth\` measures \`Task::clone\` alone:
$(extract_median "$CRITERION_DIR/advanced_artifact_accumulation/task_clone_at_depth/0/new/estimates.json") with no artifacts and $(extract_median "$CRITERION_DIR/advanced_artifact_accumulation/task_clone_at_depth/500/new/estimates.json") with 500. The background
event processor no longer clones the whole task per artifact event: it persists
artifact updates with \`TaskStore::save_artifact_delta\`, which the in-memory
store applies in place, so streaming cost per event stays flat (see
[Stream volume per-event cost](#stream-volume-per-event-cost)).
\`store_save_at_depth\` still prices a whole-task \`save\`.

### Slow consumer timer calibration

The \`backpressure/timer_calibration\` benchmarks measure actual
\`tokio::time::sleep()\` durations on the CI runner. On shared runners,
1ms sleep ≈ $(extract_median "$CRITERION_DIR/backpressure_timer_calibration/sleep_1ms_actual/new/estimates.json") actual, 5ms sleep ≈ $(extract_median "$CRITERION_DIR/backpressure_timer_calibration/sleep_5ms_actual/new/estimates.json") actual. Slow consumer
results should be interpreted against these calibrated durations, not
the nominal sleep values.

### Data volume save() at 10K tasks

\`data_volume/save/after_prefill/10000\` reports $(extract_median "$DV_SAVE_10K") with a 95%
confidence interval of $(extract_median_ci "$DV_SAVE_10K") in this run, as tight as
the other prefill levels. An earlier run reported a much wider interval; if
that recurs, an insert into the sorted \`BTreeMap\` index crossing a node split
is the suspected cause, which has not been established. The indexes are what
let \`list()\` serve a page in \`O(log n + page_size)\` without sorting every task
on each call.

### Dispatch routing: direct handler vs HTTP round-trip

\`production/dispatch_routing/direct_handler_invoke\` calls the request handler
directly and reports $(extract_median "$CRITERION_DIR/production_dispatch_routing/direct_handler_invoke/new/estimates.json"); \`full_http_roundtrip\` sends a message through
a JSON-RPC server over a warm keep-alive connection and reports
$(extract_median "$CRITERION_DIR/production_dispatch_routing/full_http_roundtrip/new/estimates.json"). The difference ($(derive_diff "$CRITERION_DIR/production_dispatch_routing/full_http_roundtrip/new/estimates.json" "$CRITERION_DIR/production_dispatch_routing/direct_handler_invoke/new/estimates.json")) is everything the direct call
bypasses: the client's request and response handling, JSON-RPC
(de)serialization, HTTP, and dispatch. \`docs/swarm-scale-findings.md\` measures
the HTTP stack itself at roughly a tenth of a send.

### Subscribe fan-out scaling

\`advanced/subscribe_fanout\` reports $(extract_median "$CRITERION_DIR/advanced_subscribe_fanout/concurrent_subscribers/1/new/estimates.json") with 1 subscriber, $(extract_median "$CRITERION_DIR/advanced_subscribe_fanout/concurrent_subscribers/5/new/estimates.json") with 5
and $(extract_median "$CRITERION_DIR/advanced_subscribe_fanout/concurrent_subscribers/10/new/estimates.json") with 10.

### Agent burst scaling

\`production/agent_burst\` times the whole burst: $(extract_median "$CRITERION_DIR/production_agent_burst/agents/10/new/estimates.json") for 10 agents,
$(extract_median "$CRITERION_DIR/production_agent_burst/agents/50/new/estimates.json") for 50 and $(extract_median "$CRITERION_DIR/production_agent_burst/agents/100/new/estimates.json") for 100 — per agent, $(derive_per_agent "$CRITERION_DIR/production_agent_burst/agents/10/new/estimates.json" 10),
$(derive_per_agent "$CRITERION_DIR/production_agent_burst/agents/50/new/estimates.json" 50) and $(derive_per_agent "$CRITERION_DIR/production_agent_burst/agents/100/new/estimates.json" 100) respectively.

### Cold start vs steady state

\`production/cold_start/first_request\` ($(extract_median "$CRITERION_DIR/production_cold_start/first_request/new/estimates.json")) creates a fresh server per
iteration (sample\\_size=20), measuring server handler initialization + first
TCP connect. \`steady_state\` ($(extract_median "$CRITERION_DIR/production_cold_start/steady_state/new/estimates.json")) reuses an existing keep-alive
connection, measuring the full HTTP round-trip with connection overhead already
amortized. The two benchmarks measure different things — they are
complementary, not comparable.

### Tenant resolver overhead

Per request, the tenant resolvers cost $(extract_median "$CRITERION_DIR/advanced_tenant_resolver/header_resolver/new/estimates.json") (header), $(extract_median "$CRITERION_DIR/advanced_tenant_resolver/header_resolver_miss/new/estimates.json") (header
miss), $(extract_median "$CRITERION_DIR/advanced_tenant_resolver/path_resolver/new/estimates.json") (path), $(extract_median "$CRITERION_DIR/advanced_tenant_resolver/bearer_resolver/new/estimates.json") (bearer) and $(extract_median "$CRITERION_DIR/advanced_tenant_resolver/bearer_resolver_with_mapper/new/estimates.json") (bearer with mapper),
against $(extract_median "$CRITERION_DIR/transport_jsonrpc_send/single_message/new/estimates.json") for a full JSON-RPC round trip
(\`transport_jsonrpc_send/single_message\`).

### Pagination context index

At 1000 tasks, the \`advanced/pagination_walk\` filtered walk takes
$(extract_median "$CRITERION_DIR/advanced_pagination_walk/filtered/1000_tasks_page_50/new/estimates.json") against $(extract_median "$CRITERION_DIR/advanced_pagination_walk/unfiltered/1000_tasks_page_50/new/estimates.json") unfiltered. The \`BTreeMap\` context index
lets a \`context_id\`-filtered walk iterate only that context's tasks (500 of the
1,000 here), which is why it takes about half as long.

SECTION

cat >> "$OUTPUT_FILE" <<'FOOTER'
---

## Methodology

All benchmarks use [Criterion.rs](https://github.com/bheisler/criterion.rs),
which provides:

- **Statistical significance testing** — detects real regressions vs noise
- **Warm-up iterations** — avoids cold-start measurement artifacts
- **Median and its 95 % confidence interval** — a central tendency resistant to outliers
- **Configurable sample sizes** — more iterations for noisy benchmarks

### Measurement rigor

Most benchmarks follow these practices; the exceptions are deliberate (the
cold-start, per-request-client and per-agent-client benchmarks build what they
measure inside `iter()`, and the `data_volume` save benchmarks need a fresh task
id per iteration):

- **Deterministic inputs**: Fixed task IDs and payloads inside `iter()` — no
  incrementing counters that change HashMap distribution across iterations
- **Setup outside measurement**: Store creation, server startup, and resource
  allocation happen before `iter()`, not inside it
- **`debug_assert!` for invariants**: Correctness checks inside measurement
  loops use `debug_assert!` to avoid string-formatting cost in release builds
- **`black_box()` on inputs and outputs**: Prevents the compiler from
  eliminating measured work through dead-code optimization
- **Tolerance-based allocation assertions**: Memory benchmarks use a 5%
  tolerance instead of exact counts to avoid spurious CI failures from
  serde_json/stdlib version changes
- **Side-effect interceptors**: The interceptor chain benchmark uses
  `CountingInterceptor` (AtomicU64) to verify interceptors are actually
  invoked during measurement — not just optimized away

### What we benchmark

The SDK's value proposition is the **A2A protocol layer and runtime efficiency**,
not agent logic. The bulk of the suite therefore benchmarks what the SDK owns:
transport overhead, serialization cost, store operations, concurrency scaling,
streaming backpressure, error handling, and memory allocation behavior.

One benchmark — `coordinator_chain_under_fault` — is deliberately a different
shape: it measures *end-to-end agent-chain latency under fault injection*, not
SDK-layer overhead. It is documented in its own section above with the
caveats for how to interpret it (in-process only, sequential delegation only,
one topology). It is not intended to substitute for a real agent-capability
benchmark suite — it closes the most obvious gap in the existing suite while
staying honest about what it is.

### What we do NOT benchmark

- **Agent intelligence** — LLM quality is an eval problem, not a perf benchmark
- **Real network faults** — the fault-injection bench simulates synthetic
  `ClientError::Timeout` responses in-process, not real packet loss or TCP
  congestion control
- **Network latency** — all benchmarks use loopback (127.0.0.1)
- **TLS handshake** — benchmarks use plaintext HTTP
- **Task completion quality** — needs human-preference evaluation
- **Multi-agent topologies beyond sequential delegation** — critic loops,
  parallel fan-out with deadline propagation, and plan-and-execute with
  replanning are out of scope for this crate

### Reproducing locally

```bash
# Run all benchmarks
cargo bench -p a2a-benchmarks

# Run a specific module
cargo bench -p a2a-benchmarks --bench transport_throughput

# Save baseline, make changes, then compare
./benches/scripts/run_benchmarks.sh --save
# ... make changes ...
./benches/scripts/run_benchmarks.sh --compare
```

Full HTML reports (with violin plots and comparison overlays) are generated
in `target/criterion/`.
FOOTER

echo "Book page generated: $OUTPUT_FILE"
