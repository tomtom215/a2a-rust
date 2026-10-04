# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
"""Median across reps, with [min, max] spread, per (server, mode, connections).
Any cell with errors > 0 is flagged rather than averaged in."""
import json, sys, statistics
from collections import defaultdict
rows = [json.loads(l) for l in open(sys.argv[1])]
g = defaultdict(list)
for r in rows:
    g[(r["mode"], r["connections"], r["server"])].append(r)
print("| mode | conns | server | rps median [min–max] | p50 µs | p99 µs | p99.9 µs | RSS end MiB | errors |")
print("|---|---:|---|---:|---:|---:|---:|---:|---:|")
for (mode, c, s) in sorted(g, key=lambda k: (k[0], k[1], k[2])):
    rs = g[(mode, c, s)]
    rps = [r["rps"] for r in rs]
    med = lambda f: statistics.median(f(r) for r in rs)
    err = sum(r["errors"] for r in rs)
    print("| %s | %d | %s | %.0f [%.0f–%.0f] | %.0f | %.0f | %.0f | %.1f | %d%s |" % (
        mode, c, s, statistics.median(rps), min(rps), max(rps),
        med(lambda r: r["lat_us"]["p50"]), med(lambda r: r["lat_us"]["p99"]), med(lambda r: r["lat_us"]["p999"]),
        med(lambda r: r["rss_end_kb"]) / 1024, err, " ⚠" if err else ""))
