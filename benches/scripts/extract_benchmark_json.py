#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
#
# Extract criterion benchmark results into structured JSON for the dashboard.
#
# Reads: target/criterion/*/new/estimates.json
# Writes: structured JSON to stdout or file (--output)
#
# Usage:
#   python3 extract_benchmark_json.py --criterion-dir target/criterion
#   python3 extract_benchmark_json.py --criterion-dir target/criterion --output data.json

"""Criterion benchmark data extractor for the a2a-rust dashboard.

Walks the criterion output directory, extracts median point estimates and
confidence intervals from estimates.json files, and assembles a structured
JSON object consumed by the interactive benchmark dashboard template.
"""

import argparse
import json
import subprocess
import sys
from datetime import datetime, timezone
from pathlib import Path
import platform as plat


def extract_estimate(est_path: Path) -> dict | None:
    """Extract median point estimate and CI from a criterion estimates.json."""
    try:
        with open(est_path) as f:
            data = json.load(f)
        median = data["median"]
        ns = median["point_estimate"]
        ci = median.get("confidence_interval", {})
        return {
            "median_ns": ns,
            "ci_lower_ns": ci.get("lower_bound", ns),
            "ci_upper_ns": ci.get("upper_bound", ns),
        }
    except (json.JSONDecodeError, KeyError, FileNotFoundError):
        return None


def format_human(ns: float) -> str:
    """Convert nanoseconds to human-readable string."""
    if ns >= 1_000_000:
        return f"{ns / 1_000_000:.2f} ms"
    elif ns >= 1_000:
        return f"{ns / 1_000:.1f} \u00b5s"
    else:
        return f"{ns:.0f} ns"


def collect_benchmarks(criterion_dir: Path) -> list[dict]:
    """Walk criterion output and collect all benchmark results."""
    results = []
    if not criterion_dir.is_dir():
        return results
    for est_path in sorted(criterion_dir.rglob("new/estimates.json")):
        rel = est_path.relative_to(criterion_dir)
        bench_name = str(rel.parent.parent)
        if bench_name == "." or bench_name.startswith("report"):
            continue
        estimate = extract_estimate(est_path)
        if estimate is None:
            continue
        results.append({
            "name": bench_name,
            "median_ns": estimate["median_ns"],
            "ci_lower_ns": estimate["ci_lower_ns"],
            "ci_upper_ns": estimate["ci_upper_ns"],
            "human": format_human(estimate["median_ns"]),
        })
    return results


def categorize(benchmarks: list[dict]) -> dict:
    """Group benchmarks by category prefix."""
    cats = {p: [] for p in [
        "transport", "protocol", "lifecycle", "concurrent", "realistic",
        "errors", "backpressure", "data_volume", "memory",
        "cross_language", "enterprise", "production", "advanced",
    ]}
    cats["uncategorized"] = []
    for b in benchmarks:
        matched = False
        for prefix in cats:
            if prefix != "uncategorized" and b["name"].startswith(prefix + "_"):
                cats[prefix].append(b)
                matched = True
                break
        if not matched:
            cats["uncategorized"].append(b)
    return {k: v for k, v in cats.items() if v}


def build_dashboard_data(benchmarks: list[dict], categories: dict) -> dict:
    """Build the structured data object consumed by the HTML dashboard."""
    lookup = {b["name"]: b["median_ns"] for b in benchmarks}

    def find(name: str) -> float:
        return lookup.get(name, 0)

    def ms(name: str) -> float:
        return round(find(name) / 1_000_000, 2)

    def us(name: str) -> float:
        return round(find(name) / 1_000, 1)

    def ns(name: str) -> int:
        return int(find(name))

    # Metadata
    rust_version = "unknown"
    try:
        r = subprocess.run(["rustc", "--version"], capture_output=True, text=True)
        if r.returncode == 0:
            rust_version = r.stdout.strip()
    except FileNotFoundError:
        pass

    return {
        "metadata": {
            "timestamp": datetime.now(timezone.utc).strftime("%Y-%m-%d %H:%M UTC"),
            "rust_version": rust_version,
            "platform": f"{plat.system()}-{plat.machine()}",
            "total_benchmarks": len(benchmarks),
            "total_categories": len(categories),
        },
        "highlights": {
            "serde_floor_ns": ns("protocol_stream_events/status_update/serialize"),
            "roundtrip_reused_ms": ms("realistic_connection/reused_client"),
            "roundtrip_new_ms": ms("realistic_connection/new_client_per_request"),
            "concurrent_64_sends_ms": ms("concurrent_sends/jsonrpc/64"),
            "concurrent_1_send_ms": ms("concurrent_sends/jsonrpc/1"),
            "error_path_ms": ms("errors_happy_vs_error/error_path"),
            "happy_path_ms": ms("errors_happy_vs_error/happy_path"),
            "agent_burst_100_ms": ms("production_agent_burst/agents/100"),
            "agent_burst_10_ms": ms("production_agent_burst/agents/10"),
        },
        "transport": {
            "jsonrpc_send_ms": ms("transport_jsonrpc_send/single_message"),
            "jsonrpc_stream_ms": ms("transport_jsonrpc_stream/stream_drain"),
            "rest_send_ms": ms("transport_rest_send/single_message"),
            "rest_stream_ms": ms("transport_rest_stream/stream_drain"),
            "payload_scaling": [
                {"size": s, "ms": ms(f"transport_payload_scaling/jsonrpc_send/{n}")}
                for s, n in [("64B", 64), ("256B", 256), ("1KB", 1024), ("4KB", 4096),
                             ("16KB", 16384), ("100KB", 102400), ("1MB", 1048576)]
            ],
        },
        "connection_reuse": {
            "reused_ms": ms("realistic_connection/reused_client"),
            "new_per_request_ms": ms("realistic_connection/new_client_per_request"),
        },
        "concurrency": [
            {"c": c,
             "sends": ms(f"concurrent_sends/jsonrpc/{c}"),
             "streams": ms(f"concurrent_streams/jsonrpc/{c}"),
             "store": us(f"concurrent_store/save_and_get/{c}")}
            for c in [1, 4, 16, 64]
        ],
        "serde": {
            "types": [
                {"type": t, "ns": ns(n)} for t, n in [
                    ("AgentCard ser", "protocol_type_serde/agent_card/serialize"),
                    ("AgentCard de", "protocol_type_serde/agent_card/deserialize"),
                    ("Task ser", "protocol_type_serde/task/serialize/278"),
                    ("Task de", "protocol_type_serde/task/deserialize/278"),
                    ("Message ser", "protocol_type_serde/message/serialize/217"),
                    ("Message de", "protocol_type_serde/message/deserialize/217"),
                    ("status_update ser", "protocol_stream_events/status_update/serialize"),
                    ("status_update de", "protocol_stream_events/status_update/deserialize"),
                    ("artifact_update ser", "protocol_stream_events/artifact_update/serialize"),
                    ("artifact_update de", "protocol_stream_events/artifact_update/deserialize"),
                    ("request envelope ser", "protocol_jsonrpc_envelope/serialize_request"),
                    ("request envelope de", "protocol_jsonrpc_envelope/deserialize_request"),
                    ("response envelope ser", "protocol_jsonrpc_envelope/serialize_response"),
                    ("response envelope de", "protocol_jsonrpc_envelope/deserialize_response"),
                ]
            ],
            "batch": [
                {"count": c,
                 "ser_us": us(f"protocol_batch/serialize_tasks/{c}"),
                 "de_us": us(f"protocol_batch/deserialize_tasks/{c}")}
                for c in [1, 10, 50, 100]
            ],
            "interceptors": [
                {"n": n, "us": us(f"realistic_interceptor_chain/interceptors/{n}")}
                for n in [0, 1, 5, 10]
            ],
            "payload_scaling": [
                {"size": s,
                 "to_vec_ns": ns(f"protocol_payload_scaling/to_vec/{n}"),
                 "ser_buffer_ns": ns(f"protocol_payload_scaling/ser_buffer/{n}"),
                 "from_slice_ns": ns(f"protocol_payload_scaling/from_slice/{n}"),
                 "from_str_ns": ns(f"protocol_payload_scaling/from_str/{n}")}
                for s, n in [("64B", 64), ("256B", 256), ("1KB", 1024), ("4KB", 4096),
                             ("16KB", 16384), ("100KB", 102400), ("1MB", 1048576)]
            ],
        },
        "backpressure": {
            "stream_volume": [
                {"events": e, "ms": ms(f"backpressure_stream_volume/{l}")}
                for e, l in [("3", "3_events"), ("7", "7_events"), ("27", "27_events"),
                             ("52", "52_events"), ("252", "252_events"), ("502", "502_events")]
            ],
            "slow_consumer": {
                "fast_ms": ms("backpressure_slow_consumer/fast_consumer"),
                "delay_1ms_ms": ms("backpressure_slow_consumer/1ms_delay"),
                "delay_5ms_ms": ms("backpressure_slow_consumer/5ms_delay"),
            },
            "concurrent_streams": [
                {"streams": s, "ms": ms(f"backpressure_concurrent_streams/streams/{s}")}
                for s in [1, 4, 16]
            ],
            "timer_cal": {
                "sleep_1ms_actual_ms": ms("backpressure_timer_calibration/sleep_1ms_actual"),
                "sleep_5ms_actual_ms": ms("backpressure_timer_calibration/sleep_5ms_actual"),
            },
        },
        "data_volume": {
            "get": [{"volume": v, "ns": ns(f"data_volume_get/lookup/{n}")}
                    for v, n in [("1K", 1000), ("10K", 10000), ("100K", 100000)]],
            "list": [{"volume": v, "us": us(f"data_volume_list/filtered_page_50/{n}")}
                     for v, n in [("1K", 1000), ("10K", 10000), ("100K", 100000)]],
            "save": [{"prefill": p, "us": us(f"data_volume_save/after_prefill/{n}")}
                     for p, n in [("0", 0), ("1K", 1000), ("10K", 10000), ("50K", 50000)]],
            "history_depth": [{"turns": t, "us": us(f"data_volume_history_depth/save_with_turns/{t}")}
                              for t in [1, 5, 10, 20, 50]],
        },
        "memory": {
            "alloc_counts": {
                "task_ser": ns("memory_serialize/task_alloc_count"),
                "task_de": ns("memory_deserialize/task_alloc_count"),
                "agent_card_ser": ns("memory_serialize/agent_card_alloc_count"),
                "agent_card_de": ns("memory_deserialize/agent_card_alloc_count"),
            },
            "bytes_per_payload": [
                {"payload": s, "bytes": ns(f"memory_bytes_per_payload/serialize_bytes/{n}")}
                for s, n in [("64B", 64), ("256B", 256), ("1KB", 1024), ("4KB", 4096), ("16KB", 16384)]
            ],
            "history_allocs": [
                {"turns": t,
                 "ser": ns(f"memory_history_scaling/serialize_allocs/{t}"),
                 "de": ns(f"memory_history_scaling/deserialize_allocs/{t}")}
                for t in [1, 5, 10, 20, 50]
            ],
        },
        "cross_language": {
            "echo_roundtrip_ms": ms("cross_language_echo_roundtrip/rust"),
            "stream_events_ms": ms("cross_language_stream_events/rust"),
            "serialize_ns": ns("cross_language_serialize_agent_card/rust_serialize"),
            "concurrent_50_ms": ms("cross_language_concurrent_50/rust"),
            "minimal_overhead_ms": ms("cross_language_minimal_overhead/rust"),
        },
        "enterprise": {
            "tenant_isolation": [
                {"tenants": t, "ns": ns(f"enterprise_multi_tenant/tenant_isolation_check/{t}")}
                for t in [1, 10, 50, 100]
            ],
            "rw_mix": [
                {"mix": m, "us": us(f"enterprise_rw_mix/{k}")}
                for m, k in [("100R/0W", "100r_0w"), ("75R/25W", "75r_25w"),
                             ("50R/50W", "50r_50w"), ("25R/75W", "25r_75w"), ("0R/100W", "0r_100w")]
            ],
            "cors_preflight_us": us("enterprise_cors/options_preflight"),
            "cancel_task_ms": ms("enterprise_cancel_task/send_then_cancel"),
            "rate_limit_ms": ms("enterprise_rate_limiting/with_rate_limit"),
            "no_rate_limit_ms": ms("enterprise_rate_limiting/no_rate_limit"),
            "metadata_rejection_us": us("enterprise_handler_limits/metadata_rejection"),
            "eviction": [
                {"size": s,
                 "save_ns": ns(f"enterprise_eviction/save_at_capacity/{n}"),
                 "sweep_ns": ns(f"enterprise_eviction/sweep_duration/{n}")}
                for s, n in [("100", 100), ("1K", 1000), ("10K", 10000)]
            ],
            "large_history": [
                {"turns": t,
                 "ser_us": us(f"enterprise_large_history/serialize/{t}"),
                 "de_us": us(f"enterprise_large_history/deserialize/{t}"),
                 "save_us": us(f"enterprise_large_history/store_save/{t}")}
                for t in [100, 200, 500]
            ],
        },
        "production": {
            "agent_burst": [
                {"agents": a, "ms": ms(f"production_agent_burst/agents/{a}")}
                for a in [10, 50, 100]
            ],
            "orchestration_7step_ms": ms("production_e2e_orchestration/7_step_workflow"),
            "subscribe_reconnect_ms": ms("production_subscribe_to_task/send_then_subscribe"),
            "cold_start_us": us("production_cold_start/first_request"),
            "steady_state_ms": ms("production_cold_start/steady_state"),
            "cancel_subscribe_race_us": us("production_cancel_subscribe_race/concurrent_cancel_and_subscribe"),
            "dispatch_direct_ms": ms("production_dispatch_routing/direct_handler_invoke"),
            "dispatch_http_ms": ms("production_dispatch_routing/full_http_roundtrip"),
            "push_config": {
                "set_us": us("production_push_config/set_roundtrip"),
                "get_us": us("production_push_config/get_roundtrip"),
                "list_us": us("production_push_config/list_roundtrip"),
                "delete_us": us("production_push_config/delete_roundtrip"),
            },
        },
        "advanced": {
            "tenant_resolvers": [
                {"resolver": r, "ns": ns(f"advanced_tenant_resolver/{k}")}
                for r, k in [("header miss", "header_resolver_miss"), ("bearer", "bearer_resolver"),
                             ("header", "header_resolver"), ("bearer+map", "bearer_resolver_with_mapper"),
                             ("path", "path_resolver")]
            ],
            "agent_card_hot_reload": {
                "read_ns": ns("advanced_agent_card_hot_reload/read_current_card"),
                "swap_read_ns": ns("advanced_agent_card_hot_reload/swap_and_read"),
                "swap_complex_us": us("advanced_agent_card_hot_reload/swap_complex_card"),
            },
            "discovery_us": us("advanced_agent_card_discovery/well_known_endpoint"),
            "extended_card_us": us("advanced_extended_agent_card/get_extended_card_roundtrip"),
            "subscribe_fanout": [
                {"subscribers": s, "ms": ms(f"advanced_subscribe_fanout/concurrent_subscribers/{s}")}
                for s in [1, 5, 10]
            ],
            "artifact_accumulation": [
                {"depth": d,
                 "clone_us": us(f"advanced_artifact_accumulation/task_clone_at_depth/{d}"),
                 "save_us": us(f"advanced_artifact_accumulation/store_save_at_depth/{d}")}
                for d in [0, 10, 50, 100, 500]
            ],
            "pagination_walk": [
                {"config": c, "us": us(f"advanced_pagination_walk/{k}")}
                for c, k in [("100 unfiltered", "unfiltered/100_tasks_page_25"),
                             ("100 filtered", "filtered/100_tasks_page_25"),
                             ("1K unfiltered", "unfiltered/1000_tasks_page_50"),
                             ("1K filtered", "filtered/1000_tasks_page_50")]
            ],
        },
        "errors": {
            "happy_path_ms": ms("errors_happy_vs_error/happy_path"),
            "error_path_ms": ms("errors_happy_vs_error/error_path"),
            "invalid_json_us": us("errors_malformed_request/invalid_json"),
            "wrong_content_type_us": us("errors_malformed_request/wrong_content_type"),
            "task_not_found_us": us("errors_task_not_found/get_nonexistent_task"),
        },
        "lifecycle": {
            "send_complete_ms": ms("lifecycle_e2e/send_and_complete"),
            "stream_drain_ms": ms("lifecycle_e2e/stream_and_drain"),
            "store_save_ns": ns("lifecycle_store_save/single_task"),
            "store_get_ns": ns("lifecycle_store_get/lookup_in_1000"),
            "store_list_us": us("lifecycle_store_list/filtered_page_50_of_250"),
            "queue_write_read": [
                {"n": n, "us": us(f"lifecycle_queue/write_read/{n}")}
                for n in [1, 10, 50, 100]
            ],
        },
        "all_benchmarks": benchmarks,
    }


def main():
    parser = argparse.ArgumentParser(description="Extract criterion benchmarks to JSON")
    parser.add_argument("--criterion-dir", default="target/criterion",
                        help="Path to criterion output directory")
    parser.add_argument("--output", "-o", default=None,
                        help="Output file (default: stdout)")
    parser.add_argument("--pretty", action="store_true", default=True,
                        help="Pretty-print JSON output")
    args = parser.parse_args()

    criterion_dir = Path(args.criterion_dir)
    if not criterion_dir.is_dir():
        print(f"Error: criterion directory not found: {criterion_dir}", file=sys.stderr)
        print("Run benchmarks first: cargo bench -p a2a-benchmarks", file=sys.stderr)
        sys.exit(1)

    benchmarks = collect_benchmarks(criterion_dir)
    if not benchmarks:
        print(f"Error: no benchmark results found in {criterion_dir}", file=sys.stderr)
        sys.exit(1)

    categories = categorize(benchmarks)
    data = build_dashboard_data(benchmarks, categories)
    json_str = json.dumps(data, indent=2 if args.pretty else None, ensure_ascii=False)

    if args.output:
        Path(args.output).write_text(json_str + "\n")
        print(f"Wrote {len(benchmarks)} benchmarks to {args.output}", file=sys.stderr)
    else:
        print(json_str)


if __name__ == "__main__":
    main()
