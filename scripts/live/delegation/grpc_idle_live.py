#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
"""Audit H4 (b), live: a quiet child over gRPC, cut by the client's idle timeout.

Runs a2a-live-grpc-idle (this repository's gRPC server and client in one
process, see src/bin/grpc_idle.rs) `--runs` times. Each run: the child says
`working` and then nothing for `--quiet-secs`; the client's stream idle timeout
is `--idle-secs`, so the stream is cut about quiet/idle times. Expects the
delegation to end `completed` with no CancelTask, after at least `--min-resumes`
SubscribeToTask calls (the old limit was six cuts). Exit status 1 if not.

    grpc_idle_live.py --bin B [--runs 3] [--quiet-secs 90] [--idle-secs 10] [--out DIR]
"""

from __future__ import annotations

import argparse
import os
import pathlib
import re
import statistics
import subprocess
import sys
import time

ROOT = pathlib.Path(__file__).resolve().parents[3]
RESULT = re.compile(r"RESULT outcome=(\S+) elapsed_s=([\d.]+) quiet_s=(\d+) idle_s=(\d+) calls=\{(.*)\}")


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--bin", required=True, help="path to a2a-live-grpc-idle")
    ap.add_argument("--runs", type=int, default=3)
    ap.add_argument("--quiet-secs", type=int, default=90)
    ap.add_argument("--idle-secs", type=int, default=10)
    ap.add_argument("--min-resumes", type=int, default=7, help="SubscribeToTask calls required (old limit: 6 cuts)")
    ap.add_argument("--out", default=str(pathlib.Path(os.environ.get("A2A_LIVE_OUT", ROOT / "target" / "live")) / "grpc-idle"))
    cfg = ap.parse_args()
    out = pathlib.Path(cfg.out) / time.strftime("%Y%m%dT%H%M%S")
    out.mkdir(parents=True, exist_ok=True)
    rows, failed = [], []
    for run in range(1, cfg.runs + 1):
        env = {"PATH": os.environ["PATH"], "QUIET_SECS": str(cfg.quiet_secs), "IDLE_SECS": str(cfg.idle_secs)}
        t0 = time.time()
        p = subprocess.run([cfg.bin], env=env, capture_output=True, text=True, timeout=cfg.quiet_secs + 120)
        (out / f"run{run}.log").write_text(p.stdout + p.stderr)
        m = RESULT.search(p.stdout)
        calls = {k: int(v) for k, v in re.findall(r"\"([^\"]+)\": (\d+)", m[5])} if m else {}
        resumes = sum(v for k, v in calls.items() if "subscribe" in k.lower())
        cancels = sum(v for k, v in calls.items() if "cancel" in k.lower())
        ok = bool(m) and m[1] == "completed" and cancels == 0 and resumes >= cfg.min_resumes and p.returncode == 0
        rows.append((run, m[1] if m else "no RESULT", m[2] if m else "-", resumes, cancels, calls, "PASS" if ok else "FAIL"))
        if not ok:
            failed.append(run)
        print(f"run {run}: exit {p.returncode} {rows[-1][1:]} wall {time.time() - t0:.1f}s", flush=True)
    print("\nrun  outcome    elapsed_s  SubscribeToTask  CancelTask  result")
    for r in rows:
        print(f"{r[0]:>3}  {r[1]:<9}  {r[2]:>9}  {r[3]:>15}  {r[4]:>10}  {r[6]}")
    el = [float(r[2]) for r in rows if r[2] != "-"]
    rs = [r[3] for r in rows]
    if el:
        print(f"elapsed_s min {min(el)} / median {statistics.median(el)} / max {max(el)};"
              f" resumes min {min(rs)} / median {statistics.median(rs)} / max {max(rs)}")
    print(f"quiet {cfg.quiet_secs}s, client stream_idle_timeout {cfg.idle_secs}s; logs {out}")
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
