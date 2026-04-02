#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F.
#
# Extract criterion benchmark results into structured JSON for the dashboard.
#
# Usage:
#   python3 benches/scripts/extract_benchmark_json.py [--criterion-dir DIR] [--output FILE]
#
# Walks target/criterion/ recursively, finds all new/estimates.json files,
# extracts median point estimates and confidence intervals, and produces a
# structured JSON document consumed by the benchmark dashboard template.

"""Extract criterion benchmark results into structured JSON for the dashboard."""

import argparse
import json
import os
import platform
import subprocess
import sys
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Dict, List, Optional, Tuple


# ---------------------------------------------------------------------------
# Criterion estimates extraction
# ---------------------------------------------------------------------------

def extract_estimate(est_path: Path) -> Dict[str, float]:
    """Extract median point estimate and confidence interval from estimates.json.

    Returns a dict with keys: median_ns, lower_ns, upper_ns.
    Returns zeros if the file is missing or malformed.
    """
    try:
        with open(est_path) as f:
            data = json.load(f)
        median = data["median"]
        return {
            "median_ns": float(median["point_estimate"]),
            "lower_ns": float(median["confidence_interval"]["lower_bound"]),
            "upper_ns": float(median["confidence_interval"]["upper_bound"]),
        }
    except (FileNotFoundError, KeyError, json.JSONDecodeError, TypeError):
        return {"median_ns": 0.0, "lower_ns": 0.0, "upper_ns": 0.0}


def format_human(ns: float) -> str:
    """Format nanoseconds into a human-readable string."""
    if ns <= 0:
        return "---"
    if ns >= 1_000_000:
        return f"{ns / 1_000_000:.2f} ms"
    if ns >= 1_000:
        return f"{ns / 1_000:.1f} us"
    return f"{ns:.0f} ns"


# ---------------------------------------------------------------------------
# Collect all benchmarks from criterion directory
# ---------------------------------------------------------------------------

def collect_benchmarks(criterion_dir: Path) -> Dict[str, Dict[str, float]]:
    """Walk criterion_dir and collect all benchmark estimates.

    Returns a dict mapping bench_name (relative path before /new/estimates.json)
    to the extracted estimate dict.
    """
    benchmarks: Dict[str, Dict[str, float]] = {}
    if not criterion_dir.is_dir():
        return benchmarks

    for est_path in sorted(criterion_dir.rglob("new/estimates.json")):
        # Build bench_name from the relative path: everything before /new/estimates.json
        rel = est_path.relative_to(criterion_dir)
        parts = rel.parts
        # Find the "new" directory in the path
        try:
            new_idx = parts.index("new")
        except ValueError:
            continue
        bench_name = "/".join(parts[:new_idx])
        benchmarks[bench_name] = extract_estimate(est_path)

    return benchmarks


# ---------------------------------------------------------------------------
# Lookup helpers
# ---------------------------------------------------------------------------

def _ns(benchmarks: Dict[str, Dict[str, float]], key: str) -> float:
    """Get median_ns for a benchmark key, returning 0 if missing."""
    return benchmarks.get(key, {}).get("median_ns", 0.0)


def _ms(benchmarks: Dict[str, Dict[str, float]], key: str) -> float:
    """Get median in milliseconds for a benchmark key."""
    return _ns(benchmarks, key) / 1_000_000


def _us(benchmarks: Dict[str, Dict[str, float]], key: str) -> float:
    """Get median in microseconds for a benchmark key."""
    return _ns(benchmarks, key) / 1_000


# ---------------------------------------------------------------------------
# Size label helpers
# ---------------------------------------------------------------------------

_PAYLOAD_SIZES = [
    (64, "64B"),
    (256, "256B"),
    (1024, "1KB"),
    (4096, "4KB"),
    (16384, "16KB"),
    (102400, "100KB"),
    (1048576, "1MB"),
]

_VOLUME_MAP = {
    1000: "1K",
    10000: "10K",
    100000: "100K",
}


# ---------------------------------------------------------------------------
# Build the structured dashboard data
# ---------------------------------------------------------------------------

def build_dashboard_data(benchmarks: Dict[str, Dict[str, float]]) -> Dict[str, Any]:
    """Build the complete structured JSON object for the dashboard."""

    # -- metadata ----------------------------------------------------------
    try:
        rust_version = subprocess.check_output(
            ["rustc", "--version"], stderr=subprocess.DEVNULL, text=True
        ).strip()
    except (FileNotFoundError, subprocess.CalledProcessError):
        rust_version = "unknown"

    plat = f"{platform.system()}-{platform.machine()}"
    total = len(benchmarks)

    # Categorize benchmarks by top-level prefix
    categories: set = set()
    for name in benchmarks:
        prefix = name.split("/")[0].split("_")[0]
        categories.add(prefix)

    metadata = {
        "timestamp": datetime.now(timezone.utc).strftime("%Y-%m-%d %H:%M UTC"),
        "rust_version": rust_version,
        "platform": plat,
        "total_benchmarks": total,
        "total_categories": len(categories),
    }

    # -- highlights --------------------------------------------------------
    highlights = {
        "serde_floor_ns": _ns(benchmarks, "protocol_type_serde/agent_card_serialize"),
        "roundtrip_reused_ms": _ms(benchmarks, "realistic_connection/reused_client"),
        "roundtrip_new_ms": _ms(benchmarks, "realistic_connection/new_client_per_request"),
        "concurrent_64_sends_ms": _ms(benchmarks, "concurrent_sends/jsonrpc/64"),
        "concurrent_1_send_ms": _ms(benchmarks, "concurrent_sends/jsonrpc/1"),
        "error_path_ms": _ms(benchmarks, "errors_happy_vs_error/error_path"),
        "happy_path_ms": _ms(benchmarks, "errors_happy_vs_error/happy_path"),
        "agent_burst_100_ms": _ms(benchmarks, "production_agent_burst/agents/100"),
    }

    # -- transport ---------------------------------------------------------
    transport_payload_scaling = []
    for size_bytes, size_label in _PAYLOAD_SIZES:
        transport_payload_scaling.append({
            "size": size_label,
            "ms": _ms(benchmarks, f"transport_payload_scaling/jsonrpc_send/{size_bytes}"),
        })

    transport = {
        "jsonrpc_send_ms": _ms(benchmarks, "transport_jsonrpc_send/single_message"),
        "jsonrpc_stream_ms": _ms(benchmarks, "transport_jsonrpc_stream/stream_drain"),
        "rest_send_ms": _ms(benchmarks, "transport_rest_send/single_message"),
        "rest_stream_ms": _ms(benchmarks, "transport_rest_stream/stream_drain"),
        "payload_scaling": transport_payload_scaling,
    }

    # -- connection_reuse --------------------------------------------------
    connection_reuse = {
        "reused_ms": _ms(benchmarks, "realistic_connection/reused_client"),
        "new_per_request_ms": _ms(benchmarks, "realistic_connection/new_client_per_request"),
    }

    # -- concurrency -------------------------------------------------------
    concurrency = []
    for c_level in [1, 4, 16, 64]:
        concurrency.append({
            "c": c_level,
            "sends": _ms(benchmarks, f"concurrent_sends/jsonrpc/{c_level}"),
            "streams": _ms(benchmarks, f"concurrent_streams/jsonrpc/{c_level}"),
            "store": _ms(benchmarks, f"concurrent_store/save_and_get/{c_level}"),
        })

    # -- serde -------------------------------------------------------------
    # Type-level ser/de
    # protocol_type_serde uses bench_function("agent_card/serialize") -> agent_card_serialize
    # and BenchmarkId::new("task/serialize", len) -> task_serialize/<len>
    # We need to find the actual task_bytes length from the benchmarks
    task_ser_key = _find_prefix(benchmarks, "protocol_type_serde/task_serialize/")
    task_de_key = _find_prefix(benchmarks, "protocol_type_serde/task_deserialize/")
    msg_ser_key = _find_prefix(benchmarks, "protocol_type_serde/message_serialize/")
    msg_de_key = _find_prefix(benchmarks, "protocol_type_serde/message_deserialize/")

    serde_types = [
        {"type": "AgentCard ser", "ns": _ns(benchmarks, "protocol_type_serde/agent_card_serialize")},
        {"type": "AgentCard de", "ns": _ns(benchmarks, "protocol_type_serde/agent_card_deserialize")},
        {"type": "Task ser", "ns": _ns(benchmarks, task_ser_key) if task_ser_key else 0.0},
        {"type": "Task de", "ns": _ns(benchmarks, task_de_key) if task_de_key else 0.0},
        {"type": "Message ser", "ns": _ns(benchmarks, msg_ser_key) if msg_ser_key else 0.0},
        {"type": "Message de", "ns": _ns(benchmarks, msg_de_key) if msg_de_key else 0.0},
        {"type": "status_update ser", "ns": _ns(benchmarks, "protocol_stream_events/status_update_serialize")},
        {"type": "status_update de", "ns": _ns(benchmarks, "protocol_stream_events/status_update_deserialize")},
        {"type": "artifact_update ser", "ns": _ns(benchmarks, "protocol_stream_events/artifact_update_serialize")},
        {"type": "artifact_update de", "ns": _ns(benchmarks, "protocol_stream_events/artifact_update_deserialize")},
        {"type": "request envelope ser", "ns": _ns(benchmarks, "protocol_jsonrpc_envelope/serialize_request")},
        {"type": "request envelope de", "ns": _ns(benchmarks, "protocol_jsonrpc_envelope/deserialize_request")},
        {"type": "response envelope ser", "ns": _ns(benchmarks, "protocol_jsonrpc_envelope/serialize_response")},
        {"type": "response envelope de", "ns": _ns(benchmarks, "protocol_jsonrpc_envelope/deserialize_response")},
    ]

    serde_batch = []
    for count in [1, 10, 50, 100]:
        serde_batch.append({
            "count": count,
            "ser_us": _us(benchmarks, f"protocol_batch/serialize_tasks/{count}"),
            "de_us": _us(benchmarks, f"protocol_batch/deserialize_tasks/{count}"),
        })

    serde_interceptors = []
    for n in [0, 1, 5, 10]:
        serde_interceptors.append({
            "n": n,
            "us": _us(benchmarks, f"realistic_interceptor_chain/interceptors/{n}"),
        })

    serde_payload_scaling = []
    for size_bytes, size_label in _PAYLOAD_SIZES:
        serde_payload_scaling.append({
            "size": size_label,
            "to_vec_ns": _ns(benchmarks, f"protocol_payload_scaling/to_vec/{size_bytes}"),
            "ser_buffer_ns": _ns(benchmarks, f"protocol_payload_scaling/ser_buffer/{size_bytes}"),
            "from_slice_ns": _ns(benchmarks, f"protocol_payload_scaling/from_slice/{size_bytes}"),
            "from_str_ns": _ns(benchmarks, f"protocol_payload_scaling/from_str/{size_bytes}"),
        })

    serde = {
        "types": serde_types,
        "batch": serde_batch,
        "interceptors": serde_interceptors,
        "payload_scaling": serde_payload_scaling,
    }

    # -- backpressure ------------------------------------------------------
    stream_volume = []
    for label in ["3_events", "7_events", "27_events", "52_events", "252_events", "502_events"]:
        stream_volume.append({
            "events": label.replace("_events", ""),
            "ms": _ms(benchmarks, f"backpressure_stream_volume/{label}"),
        })

    slow_consumer = {
        "fast_ms": _ms(benchmarks, "backpressure_slow_consumer/fast_consumer"),
        "delay_1ms_ms": _ms(benchmarks, "backpressure_slow_consumer/1ms_delay"),
        "delay_5ms_ms": _ms(benchmarks, "backpressure_slow_consumer/5ms_delay"),
    }

    concurrent_streams_bp = []
    for n in [1, 4, 16]:
        concurrent_streams_bp.append({
            "streams": n,
            "ms": _ms(benchmarks, f"backpressure_concurrent_streams/streams/{n}"),
        })

    timer_cal = {
        "sleep_1ms_actual_ms": _ms(benchmarks, "backpressure_timer_calibration/sleep_1ms_actual"),
        "sleep_5ms_actual_ms": _ms(benchmarks, "backpressure_timer_calibration/sleep_5ms_actual"),
    }

    backpressure = {
        "stream_volume": stream_volume,
        "slow_consumer": slow_consumer,
        "concurrent_streams": concurrent_streams_bp,
        "timer_cal": timer_cal,
    }

    # -- data_volume -------------------------------------------------------
    data_volume_get = []
    for vol, vol_label in [(1000, "1K"), (10000, "10K"), (100000, "100K")]:
        data_volume_get.append({
            "volume": vol_label,
            "ns": _ns(benchmarks, f"data_volume_get/lookup/{vol}"),
        })

    data_volume_list = []
    for vol, vol_label in [(1000, "1K"), (10000, "10K"), (100000, "100K")]:
        data_volume_list.append({
            "volume": vol_label,
            "us": _us(benchmarks, f"data_volume_list/filtered_page_50/{vol}"),
        })

    data_volume_save = []
    for prefill in [0, 1000, 10000, 50000]:
        label = "0" if prefill == 0 else _VOLUME_MAP.get(prefill, str(prefill))
        if prefill == 50000:
            label = "50K"
        data_volume_save.append({
            "prefill": label,
            "us": _us(benchmarks, f"data_volume_save/after_prefill/{prefill}"),
        })

    data_volume_history = []
    for turns in [1, 5, 10, 20, 50]:
        data_volume_history.append({
            "turns": turns,
            "us": _us(benchmarks, f"data_volume_history_depth/save_with_turns/{turns}"),
        })

    data_volume = {
        "get": data_volume_get,
        "list": data_volume_list,
        "save": data_volume_save,
        "history_depth": data_volume_history,
    }

    # -- memory ------------------------------------------------------------
    # Note: These benchmarks use iter_custom() which returns wall-clock time.
    # Criterion reports the timing (ns), not allocation counts. Allocation
    # counts are verified internally via assertions. The values here are
    # timing in nanoseconds under the counting allocator overhead.
    alloc_timing = {
        "task_ser": _ns(benchmarks, "memory_serialize/task_alloc_count"),
        "task_de": _ns(benchmarks, "memory_deserialize/task_alloc_count"),
        "agent_card_ser": _ns(benchmarks, "memory_serialize/agent_card_alloc_count"),
        "agent_card_de": _ns(benchmarks, "memory_deserialize/agent_card_alloc_count"),
    }

    bytes_per_payload = []
    for size_bytes, size_label in _PAYLOAD_SIZES[:5]:  # 64 to 16384
        bytes_per_payload.append({
            "payload": size_label,
            "bytes": _ns(benchmarks, f"memory_bytes_per_payload/serialize_bytes/{size_bytes}"),
        })

    history_allocs = []
    for turns in [1, 5, 10, 20, 50]:
        history_allocs.append({
            "turns": turns,
            "ser": _ns(benchmarks, f"memory_history_scaling/serialize_allocs/{turns}"),
            "de": _ns(benchmarks, f"memory_history_scaling/deserialize_allocs/{turns}"),
        })

    memory = {
        "alloc_timing": alloc_timing,
        "bytes_per_payload": bytes_per_payload,
        "history_allocs": history_allocs,
    }

    # -- cross_language ----------------------------------------------------
    cross_language = {
        "echo_roundtrip_ms": _ms(benchmarks, "cross_language_echo_roundtrip/rust"),
        "stream_events_ms": _ms(benchmarks, "cross_language_stream_events/rust"),
        "serialize_ns": _ns(benchmarks, "cross_language_serialize_agent_card/rust_serialize"),
        "concurrent_50_ms": _ms(benchmarks, "cross_language_concurrent_50/rust"),
        "minimal_overhead_ms": _ms(benchmarks, "cross_language_minimal_overhead/rust"),
    }

    # -- enterprise --------------------------------------------------------
    tenant_isolation = []
    for n in [1, 10, 50, 100]:
        tenant_isolation.append({
            "tenants": n,
            "ns": _ns(benchmarks, f"enterprise_multi_tenant/concurrent_tenant_saves/{n}"),
        })

    rw_mix = []
    for label, display in [("100r_0w", "100R/0W"), ("75r_25w", "75R/25W"),
                           ("50r_50w", "50R/50W"), ("25r_75w", "25R/75W"),
                           ("0r_100w", "0R/100W")]:
        rw_mix.append({
            "mix": display,
            "us": _us(benchmarks, f"enterprise_rw_mix/{label}"),
        })

    eviction = []
    for size in [100, 1000, 10000]:
        eviction.append({
            "size": str(size),
            "sweep_ns": _ns(benchmarks, f"enterprise_eviction/sweep_duration/{size}"),
        })

    large_history = []
    for turns in [100, 200, 500]:
        large_history.append({
            "turns": turns,
            "ser_us": _us(benchmarks, f"enterprise_large_history/serialize/{turns}"),
            "de_us": _us(benchmarks, f"enterprise_large_history/deserialize/{turns}"),
            "save_us": _us(benchmarks, f"enterprise_large_history/store_save/{turns}"),
        })

    enterprise = {
        "tenant_isolation": tenant_isolation,
        "rw_mix": rw_mix,
        "cors_preflight_us": _us(benchmarks, "enterprise_cors/options_preflight"),
        "cancel_task_ms": _ms(benchmarks, "enterprise_cancel_task/send_then_cancel"),
        "rate_limit_ms": _ms(benchmarks, "enterprise_rate_limiting/with_rate_limit"),
        "no_rate_limit_ms": _ms(benchmarks, "enterprise_rate_limiting/no_rate_limit"),
        "metadata_rejection_us": _us(benchmarks, "enterprise_handler_limits/metadata_rejection"),
        "eviction": eviction,
        "large_history": large_history,
    }

    # -- production --------------------------------------------------------
    agent_burst = []
    for n in [10, 50, 100]:
        agent_burst.append({
            "agents": n,
            "ms": _ms(benchmarks, f"production_agent_burst/agents/{n}"),
        })

    production = {
        "agent_burst": agent_burst,
        "orchestration_7step_ms": _ms(benchmarks, "production_e2e_orchestration/7_step_workflow"),
        "subscribe_reconnect_ms": _ms(benchmarks, "production_subscribe_to_task/send_then_subscribe"),
        "cold_start_us": _us(benchmarks, "production_cold_start/first_request"),
        "steady_state_ms": _ms(benchmarks, "production_cold_start/steady_state"),
        "cancel_subscribe_race_us": _us(benchmarks, "production_cancel_subscribe_race/concurrent_cancel_and_subscribe"),
        "dispatch_direct_ms": _ms(benchmarks, "production_dispatch_routing/direct_handler_invoke"),
        "dispatch_http_ms": _ms(benchmarks, "production_dispatch_routing/full_http_roundtrip"),
        "push_config": {
            "set_us": _us(benchmarks, "production_push_config/set_roundtrip"),
            "get_us": _us(benchmarks, "production_push_config/get_roundtrip"),
            "list_us": _us(benchmarks, "production_push_config/list_roundtrip"),
            "delete_us": _us(benchmarks, "production_push_config/delete_roundtrip"),
        },
    }

    # -- advanced ----------------------------------------------------------
    tenant_resolvers = []
    for resolver, label in [
        ("header_resolver_miss", "header_miss"),
        ("header_resolver", "header"),
        ("bearer_resolver", "bearer"),
        ("bearer_resolver_with_mapper", "bearer_with_mapper"),
        ("path_resolver", "path"),
    ]:
        tenant_resolvers.append({
            "resolver": label,
            "ns": _ns(benchmarks, f"advanced_tenant_resolver/{resolver}"),
        })

    agent_card_hot_reload = {
        "read_ns": _ns(benchmarks, "advanced_agent_card_hot_reload/read_current_card"),
        "swap_read_ns": _ns(benchmarks, "advanced_agent_card_hot_reload/swap_and_read"),
        "swap_complex_us": _us(benchmarks, "advanced_agent_card_hot_reload/swap_complex_card"),
    }

    subscribe_fanout = []
    for n in [1, 5, 10]:
        subscribe_fanout.append({
            "subscribers": n,
            "ms": _ms(benchmarks, f"advanced_subscribe_fanout/concurrent_subscribers/{n}"),
        })

    artifact_accumulation = []
    for depth in [0, 10, 50, 100, 500]:
        artifact_accumulation.append({
            "depth": depth,
            "clone_us": _us(benchmarks, f"advanced_artifact_accumulation/task_clone_at_depth/{depth}"),
            "save_us": _us(benchmarks, f"advanced_artifact_accumulation/store_save_at_depth/{depth}"),
        })

    pagination_walk = []
    for config_key, config_label in [
        ("unfiltered/100_tasks_page_25", "100 unfiltered"),
        ("filtered/100_tasks_page_25", "100 filtered"),
        ("unfiltered/1000_tasks_page_50", "1000 unfiltered"),
        ("filtered/1000_tasks_page_50", "1000 filtered"),
    ]:
        pagination_walk.append({
            "config": config_label,
            "us": _us(benchmarks, f"advanced_pagination_walk/{config_key}"),
        })

    advanced = {
        "tenant_resolvers": tenant_resolvers,
        "agent_card_hot_reload": agent_card_hot_reload,
        "discovery_us": _us(benchmarks, "advanced_agent_card_discovery/well_known_endpoint"),
        "extended_card_us": _us(benchmarks, "advanced_extended_agent_card/get_extended_card_roundtrip"),
        "subscribe_fanout": subscribe_fanout,
        "artifact_accumulation": artifact_accumulation,
        "pagination_walk": pagination_walk,
    }

    # -- concurrent_mixed --------------------------------------------------
    concurrent_mixed = {
        "send_then_get_ms": _ms(benchmarks, "concurrent_mixed/send_then_get"),
    }

    # -- errors ------------------------------------------------------------
    errors = {
        "happy_path_ms": _ms(benchmarks, "errors_happy_vs_error/happy_path"),
        "error_path_ms": _ms(benchmarks, "errors_happy_vs_error/error_path"),
        "invalid_json_us": _us(benchmarks, "errors_malformed_request/invalid_json"),
        "wrong_content_type_us": _us(benchmarks, "errors_malformed_request/wrong_content_type"),
        "task_not_found_us": _us(benchmarks, "errors_task_not_found/get_nonexistent_task"),
    }

    # -- lifecycle ---------------------------------------------------------
    queue_write_read = []
    for n in [1, 10, 50, 100]:
        queue_write_read.append({
            "n": n,
            "us": _us(benchmarks, f"lifecycle_queue/write_read/{n}"),
        })

    lifecycle = {
        "send_complete_ms": _ms(benchmarks, "lifecycle_e2e/send_and_complete"),
        "stream_drain_ms": _ms(benchmarks, "lifecycle_e2e/stream_and_drain"),
        "store_save_ns": _ns(benchmarks, "lifecycle_store_save/single_task"),
        "store_get_ns": _ns(benchmarks, "lifecycle_store_get/lookup_in_1000"),
        "store_list_us": _us(benchmarks, "lifecycle_store_list/filtered_page_50_of_250"),
        "queue_write_read": queue_write_read,
    }

    # -- all_benchmarks (raw list) -----------------------------------------
    all_benchmarks = []
    for name in sorted(benchmarks):
        est = benchmarks[name]
        all_benchmarks.append({
            "name": name,
            "median_ns": est["median_ns"],
            "lower_ns": est["lower_ns"],
            "upper_ns": est["upper_ns"],
            "human": format_human(est["median_ns"]),
        })

    # -- assemble ----------------------------------------------------------
    return {
        "metadata": metadata,
        "highlights": highlights,
        "transport": transport,
        "connection_reuse": connection_reuse,
        "concurrency": concurrency,
        "serde": serde,
        "backpressure": backpressure,
        "data_volume": data_volume,
        "memory": memory,
        "cross_language": cross_language,
        "enterprise": enterprise,
        "production": production,
        "advanced": advanced,
        "concurrent_mixed": concurrent_mixed,
        "errors": errors,
        "lifecycle": lifecycle,
        "all_benchmarks": all_benchmarks,
    }


def _find_prefix(benchmarks: Dict[str, Dict[str, float]], prefix: str) -> Optional[str]:
    """Find the first benchmark key that starts with the given prefix.

    Criterion uses BenchmarkId::new("task/serialize", byte_len) which produces
    a directory like protocol_type_serde/task_serialize/<byte_len>. Since the
    byte length varies, we search by prefix.
    """
    for key in sorted(benchmarks):
        if key.startswith(prefix):
            return key
    return None


# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------

def main() -> None:
    parser = argparse.ArgumentParser(
        description="Extract criterion benchmark results into structured JSON."
    )
    parser.add_argument(
        "--criterion-dir",
        type=Path,
        default=None,
        help="Path to criterion output directory (default: target/criterion/)",
    )
    parser.add_argument(
        "--output",
        "-o",
        type=Path,
        default=None,
        help="Output JSON file path (default: stdout)",
    )
    parser.add_argument(
        "--pretty",
        action="store_true",
        default=True,
        help="Pretty-print JSON output (default: true)",
    )
    parser.add_argument(
        "--compact",
        action="store_true",
        help="Compact JSON output (overrides --pretty)",
    )
    args = parser.parse_args()

    # Resolve criterion directory
    if args.criterion_dir:
        criterion_dir = args.criterion_dir.resolve()
    else:
        # Walk up from script location to find repo root
        script_dir = Path(__file__).resolve().parent
        repo_root = script_dir.parent.parent
        criterion_dir = repo_root / "target" / "criterion"

    if not criterion_dir.is_dir():
        print(
            f"Error: No criterion results found at {criterion_dir}\n"
            "Run benchmarks first: cargo bench -p a2a-benchmarks",
            file=sys.stderr,
        )
        sys.exit(1)

    # Collect and build
    benchmarks = collect_benchmarks(criterion_dir)
    if not benchmarks:
        print(
            f"Warning: No estimates.json files found in {criterion_dir}",
            file=sys.stderr,
        )

    data = build_dashboard_data(benchmarks)

    # Output
    indent = None if args.compact else 2
    json_str = json.dumps(data, indent=indent, ensure_ascii=False)

    if args.output:
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(json_str + "\n", encoding="utf-8")
        print(f"Wrote {len(benchmarks)} benchmarks to {args.output}", file=sys.stderr)
    else:
        print(json_str)


if __name__ == "__main__":
    main()
