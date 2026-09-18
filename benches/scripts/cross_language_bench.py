#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F.
"""Measure A2A server-side request handling cost across SDK implementations.

The comparison this makes, and the one it does not
--------------------------------------------------
Every target receives **byte-identical HTTP request bytes** from **one shared
client**, over a warm keep-alive connection, on loopback. Only the server
differs. So the *difference* between two targets is attributable to the server;
the *absolute* numbers include client and kernel cost that is common to all of
them, which is why this script also measures a ``floor`` target — a server that
returns a pre-baked constant and does no A2A work at all.

Because that shared cost sits in every number, the ratio of two end-to-end
medians **understates** the ratio of the servers' own handling cost. Subtract
the floor before forming a ratio, or quote the absolute difference instead.

The client is a raw socket that writes a pre-serialized request and reads a
Content-Length-framed response. It parses nothing and allocates almost nothing,
so it adds as little of its own cost as a Python client can. It is deliberately
*not* either SDK's client: using an SDK's own client would measure that SDK
twice and make the two legs incomparable.

Statistics
----------
Each trial is an independent process-level run against a freshly started
server. Within a trial we discard ``--warmup`` requests, then time
``--iterations``. We report per-trial percentiles and, across trials, the
median of medians with the observed min/max spread — a shared container is a
noisy host and a single trial cannot be distinguished from a scheduling artifact.
"""

from __future__ import annotations

import argparse
import json
import os
import platform
import socket
import statistics
import subprocess
import sys
import threading
import time
from typing import Any

# ── Request construction ────────────────────────────────────────────────────
#
# `historyLength: 0` is sent because the two reference agents otherwise return
# different amounts of data: the official Python agent echoes the inbound
# message back in `history`, the Rust one does not. Suppressing history removes
# the largest payload-size difference between them. It does not remove all of
# it — the residual sizes are recorded in the result file so a reader can see
# exactly how much uncontrolled difference remains.

PAYLOAD = json.dumps(
    {
        "jsonrpc": "2.0",
        "id": 1,
        "method": "SendMessage",
        "params": {
            "configuration": {"historyLength": 0},
            "message": {
                "messageId": "bench-fixed-id",
                "role": "ROLE_USER",
                "parts": [{"text": "hello"}],
            },
        },
    },
    separators=(",", ":"),
).encode()


def build_request(host: str, port: int) -> bytes:
    """The exact bytes every target receives. Identical modulo the Host header."""
    return (
        b"POST / HTTP/1.1\r\n"
        b"Host: " + f"{host}:{port}".encode() + b"\r\n"
        b"Content-Type: application/json\r\n"
        b"A2A-Version: 1.0\r\n"
        b"Accept: application/json\r\n"
        b"Content-Length: " + str(len(PAYLOAD)).encode() + b"\r\n"
        b"\r\n" + PAYLOAD
    )


class Conn:
    """One keep-alive connection, reused for every request in a trial."""

    def __init__(self, host: str, port: int) -> None:
        self.sock = socket.create_connection((host, port), timeout=30)
        self.sock.setsockopt(socket.IPPROTO_TCP, socket.TCP_NODELAY, 1)
        self.request = build_request(host, port)
        self.buf = b""

    def round_trip(self) -> int:
        """Send one request, read the whole response, return its byte length."""
        self.sock.sendall(self.request)
        while True:
            split = self.buf.find(b"\r\n\r\n")
            if split != -1:
                head = self.buf[:split]
                length = None
                for line in head.split(b"\r\n"):
                    if line.lower().startswith(b"content-length:"):
                        length = int(line.split(b":", 1)[1])
                        break
                if length is None:
                    raise RuntimeError(f"no Content-Length in response: {head!r}")
                total = split + 4 + length
                if len(self.buf) >= total:
                    body_len = length
                    self.buf = self.buf[total:]
                    return body_len
            chunk = self.sock.recv(65536)
            if not chunk:
                raise RuntimeError("connection closed by server mid-response")
            self.buf += chunk

    def close(self) -> None:
        try:
            self.sock.close()
        except OSError:
            pass


def percentiles(samples_us: list[float]) -> dict[str, float]:
    s = sorted(samples_us)
    n = len(s)

    def at(q: float) -> float:
        # Nearest-rank, clamped. Stated explicitly so the numbers are reproducible.
        idx = min(n - 1, max(0, int(q * n)))
        return s[idx]

    return {
        "min_us": round(s[0], 2),
        "p50_us": round(statistics.median(s), 2),
        "p90_us": round(at(0.90), 2),
        "p95_us": round(at(0.95), 2),
        "p99_us": round(at(0.99), 2),
        "max_us": round(s[-1], 2),
        "mean_us": round(statistics.fmean(s), 2),
        "stdev_us": round(statistics.stdev(s), 2) if n > 1 else 0.0,
    }


def run_sequential(host: str, port: int, warmup: int, iterations: int) -> dict[str, Any]:
    """Latency of one in-flight request at a time on a warm connection."""
    conn = Conn(host, port)
    try:
        body_len = 0
        for _ in range(warmup):
            body_len = conn.round_trip()
        samples = []
        for _ in range(iterations):
            start = time.perf_counter_ns()
            conn.round_trip()
            samples.append((time.perf_counter_ns() - start) / 1000.0)
        out = percentiles(samples)
        out["response_bytes"] = body_len
        out["iterations"] = iterations
        return out
    finally:
        conn.close()


def run_concurrent(
    host: str, port: int, connections: int, per_conn: int, warmup: int
) -> dict[str, Any]:
    """Aggregate throughput with `connections` requests in flight at once.

    Each worker owns its own connection; no connection is shared between
    threads. Throughput is total completed requests divided by the wall time
    from first start to last finish, so it includes ramp-up and drain.
    """
    latencies: list[list[float]] = [[] for _ in range(connections)]
    errors: list[str] = []
    barrier = threading.Barrier(connections)

    def worker(idx: int) -> None:
        try:
            conn = Conn(host, port)
        except OSError as exc:  # pragma: no cover - surfaced in the result file
            errors.append(f"connect: {exc}")
            barrier.wait()
            return
        try:
            for _ in range(warmup):
                conn.round_trip()
            barrier.wait()
            mine = latencies[idx]
            for _ in range(per_conn):
                start = time.perf_counter_ns()
                conn.round_trip()
                mine.append((time.perf_counter_ns() - start) / 1000.0)
        except (OSError, RuntimeError) as exc:
            errors.append(f"worker {idx}: {exc}")
        finally:
            conn.close()

    threads = [threading.Thread(target=worker, args=(i,)) for i in range(connections)]
    for t in threads:
        t.start()
    # Workers synchronise after warmup, so wall time covers the measured phase only.
    wall_start = time.perf_counter()
    for t in threads:
        t.join()
    wall = time.perf_counter() - wall_start

    flat = [x for sub in latencies for x in sub]
    if not flat:
        return {"error": "; ".join(errors) or "no samples"}
    out = percentiles(flat)
    out["completed"] = len(flat)
    out["wall_s"] = round(wall, 4)
    out["throughput_rps"] = round(len(flat) / wall, 1)
    out["connections"] = connections
    if errors:
        out["errors"] = errors[:10]
    return out


# ── CPU cost probe ──────────────────────────────────────────────────────────


def read_cpu_seconds(pid: int) -> float:
    """utime + stime for `pid`, in seconds, from /proc.

    Latency alone cannot distinguish work from waiting: a server that sleeps
    on a poll interval and one that burns CPU can post the same round-trip
    time. For the question this benchmark exists to answer — what does the A2A
    layer cost to run — the CPU actually consumed is the honest measure, and it
    is unaffected by loopback noise or by how the client is scheduled.
    """
    with open(f"/proc/{pid}/stat", encoding="utf-8") as fh:
        fields = fh.read().rsplit(")", 1)[1].split()
    # After the comm field, index 11 is utime and 12 is stime (proc(5) fields
    # 14 and 15, one-based, minus the two consumed by pid and comm).
    ticks = int(fields[11]) + int(fields[12])
    return ticks / os.sysconf("SC_CLK_TCK")


def run_cpu_probe(host: str, port: int, pid: int, budget_s: float) -> dict[str, Any]:
    """Drive the server flat out for `budget_s` and divide CPU used by requests.

    A wall-clock budget rather than a fixed request count, because the two
    servers differ by more than an order of magnitude in throughput and a count
    that gives one of them a usable sample size would give the other either a
    handful of clock ticks or a very long run.
    """
    conn = Conn(host, port)
    try:
        for _ in range(200):
            conn.round_trip()
        cpu_before = read_cpu_seconds(pid)
        wall_before = time.perf_counter()
        count = 0
        while time.perf_counter() - wall_before < budget_s:
            conn.round_trip()
            count += 1
        wall = time.perf_counter() - wall_before
        cpu = read_cpu_seconds(pid) - cpu_before
        tick = 1.0 / os.sysconf("SC_CLK_TCK")
        return {
            "requests": count,
            "wall_s": round(wall, 3),
            "server_cpu_s": round(cpu, 3),
            "server_cpu_us_per_request": round(cpu * 1e6 / count, 2) if count else None,
            "clock_tick_s": tick,
            "quantisation_error_pct": round(100 * tick / cpu, 2) if cpu > 0 else None,
        }
    finally:
        conn.close()


# ── Floor target ────────────────────────────────────────────────────────────


class FloorServer(threading.Thread):
    """A server that returns one pre-baked response and does no A2A work.

    Its purpose is to put a number on everything that is *not* the SDK: the
    client loop, the loopback stack, and the kernel. No real server can beat
    it, so it is a floor, not a competitor. It is intentionally the crudest
    possible implementation — one thread per connection, a constant reply —
    because anything smarter would start measuring the floor server instead.
    """

    daemon = True

    def __init__(self, response_len: int) -> None:
        super().__init__()
        body = b'{"jsonrpc":"2.0","id":1,"result":{}}'
        body = body + b" " * max(0, response_len - len(body))
        self.response = (
            b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n"
            b"Content-Length: " + str(len(body)).encode() + b"\r\n\r\n" + body
        )
        self.sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        self.sock.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        self.sock.bind(("127.0.0.1", 0))
        self.sock.listen(128)
        self.port = self.sock.getsockname()[1]

    def run(self) -> None:
        while True:
            try:
                client, _ = self.sock.accept()
            except OSError:
                return
            threading.Thread(target=self._serve, args=(client,), daemon=True).start()

    def _serve(self, client: socket.socket) -> None:
        client.setsockopt(socket.IPPROTO_TCP, socket.TCP_NODELAY, 1)
        buf = b""
        try:
            while True:
                chunk = client.recv(65536)
                if not chunk:
                    return
                buf += chunk
                # Count complete requests by their terminator; the body is a
                # fixed size so this is exact for our single fixed payload.
                while b"\r\n\r\n" in buf:
                    head, rest = buf.split(b"\r\n\r\n", 1)
                    want = 0
                    for line in head.split(b"\r\n"):
                        if line.lower().startswith(b"content-length:"):
                            want = int(line.split(b":", 1)[1])
                    if len(rest) < want:
                        break
                    buf = rest[want:]
                    client.sendall(self.response)
        except OSError:
            return
        finally:
            client.close()


# ── Provenance ──────────────────────────────────────────────────────────────


def sh(cmd: list[str]) -> str:
    try:
        return subprocess.run(
            cmd, capture_output=True, text=True, timeout=30, check=False
        ).stdout.strip()
    except (OSError, subprocess.SubprocessError):
        return "unavailable"


def cpu_model() -> str:
    try:
        with open("/proc/cpuinfo", encoding="utf-8") as fh:
            for line in fh:
                if line.startswith("model name"):
                    return line.split(":", 1)[1].strip()
    except OSError:
        pass
    return platform.processor() or "unknown"


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument(
        "--target",
        action="append",
        default=[],
        metavar="LABEL=HOST:PORT",
        help="a server to measure; repeatable",
    )
    ap.add_argument("--warmup", type=int, default=500)
    ap.add_argument("--iterations", type=int, default=3000)
    ap.add_argument("--trials", type=int, default=3)
    ap.add_argument("--concurrency", type=int, default=50)
    ap.add_argument("--concurrent-per-conn", type=int, default=200)
    ap.add_argument(
        "--server-pid",
        action="append",
        default=[],
        metavar="LABEL=PID",
        help="enables the CPU probe for that target; repeatable",
    )
    ap.add_argument("--cpu-probe-seconds", type=float, default=3.0)
    ap.add_argument("--pinned", default="", help="recorded verbatim in provenance")
    ap.add_argument("--note", default="", help="recorded verbatim in provenance")
    ap.add_argument(
        "--env-file",
        default="",
        help="pip freeze output to embed verbatim, so the Python side is pinned",
    )
    ap.add_argument("--out", required=True)
    args = ap.parse_args()

    targets: list[tuple[str, str, int]] = []
    for spec in args.target:
        label, _, hostport = spec.partition("=")
        host, _, port = hostport.rpartition(":")
        targets.append((label, host, int(port)))

    pids: dict[str, int] = {}
    for spec in args.server_pid:
        label, _, pid = spec.partition("=")
        pids[label] = int(pid)

    floor = FloorServer(response_len=259)
    floor.start()
    targets.append(("floor", "127.0.0.1", floor.port))

    results: dict[str, Any] = {}
    for label, host, port in targets:
        print(f"  {label:<24} ", end="", flush=True)
        seq_trials, con_trials = [], []
        for _ in range(args.trials):
            seq_trials.append(run_sequential(host, port, args.warmup, args.iterations))
            con_trials.append(
                run_concurrent(
                    host, port, args.concurrency, args.concurrent_per_conn, 50
                )
            )
            print(".", end="", flush=True)
        cpu = (
            run_cpu_probe(host, port, pids[label], args.cpu_probe_seconds)
            if label in pids
            else None
        )
        seq_medians = [t["p50_us"] for t in seq_trials]
        rps = [t.get("throughput_rps", 0.0) for t in con_trials]
        results[label] = {
            "sequential": {
                "trials": seq_trials,
                "median_of_trial_medians_us": round(statistics.median(seq_medians), 2),
                "trial_median_min_us": round(min(seq_medians), 2),
                "trial_median_max_us": round(max(seq_medians), 2),
            },
            "concurrent": {
                "trials": con_trials,
                "median_throughput_rps": round(statistics.median(rps), 1),
                "throughput_min_rps": round(min(rps), 1),
                "throughput_max_rps": round(max(rps), 1),
            },
            "cpu": cpu,
        }
        print(f" p50={results[label]['sequential']['median_of_trial_medians_us']}us")

    doc = {
        "schema": "a2a-cross-language-benchmark/1",
        "generated_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "method": {
            "description": (
                "One shared raw-socket client sends byte-identical HTTP requests "
                "over a warm keep-alive loopback connection to each target. Only "
                "the server differs."
            ),
            "request_bytes": len(PAYLOAD),
            "request_body": PAYLOAD.decode(),
            "warmup_per_trial": args.warmup,
            "iterations_per_trial": args.iterations,
            "trials": args.trials,
            "concurrency": args.concurrency,
            "concurrent_requests_per_connection": args.concurrent_per_conn,
            "percentile_rule": "nearest-rank on the sorted sample, index=int(q*n)",
            "cpu_probe": (
                "Server utime+stime from /proc, divided by requests served, over a "
                "fixed wall-clock budget. Latency cannot tell work from waiting; this "
                "can, and it is what a per-request cost argument actually rests on."
            ),
            "floor_target": (
                "A canned-response server that does no A2A work. It bounds what "
                "the client, loopback and kernel cost; it is not an A2A "
                "implementation and is not a competitor."
            ),
            "floor_scope": (
                "SEQUENTIAL LATENCY ONLY. Under concurrency the floor server "
                "measures its own thread-per-connection ceiling, not a floor, so "
                "its throughput number bounds nothing and must not be compared "
                "against the real servers."
            ),
        },
        "environment": {
            "cpu_model": cpu_model(),
            "logical_cpus": os.cpu_count(),
            "client_affinity_cpus": len(os.sched_getaffinity(0)),
            "kernel": platform.release(),
            "platform": f"{platform.system()}-{platform.machine()}",
            "python": sys.version.split()[0],
            "rustc": sh(["rustc", "--version"]),
            "git_commit": sh(["git", "rev-parse", "HEAD"]),
            "git_dirty": bool(sh(["git", "status", "--porcelain"])),
            "cpu_pinning": args.pinned or "none",
            "loadavg": list(os.getloadavg()),
            "note": args.note,
            "python_packages": (
                open(args.env_file, encoding="utf-8").read().splitlines()
                if args.env_file
                else []
            ),
        },
        "results": results,
    }
    with open(args.out, "w", encoding="utf-8") as fh:
        json.dump(doc, fh, indent=2)
        fh.write("\n")
    print(f"\nwrote {args.out}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
