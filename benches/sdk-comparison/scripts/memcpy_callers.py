# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
"""memcpy callers of a callgrind run, per request: memcpy_callers.py FILE.out REQUESTS"""
import re, subprocess, sys
out = sys.argv[1]; reqs = int(sys.argv[2])
txt = subprocess.run(["callgrind_annotate", "--tree=caller", "--inclusive=yes", "--threshold=100", out], capture_output=True, text=True).stdout.splitlines()
idx = [i for i, l in enumerate(txt) if "*" in l and "__memcpy_avx_unaligned_erms" in l]
i = idx[0]; blk = []; j = i - 1
while j >= 0 and txt[j].strip() and not txt[j].lstrip().startswith("*"):
    blk.append(txt[j]); j -= 1
rows = []
for l in blk:
    m = re.match(r"\s*([\d,]+)\s+\(\s*[\d.]+%\)\s+<\s*(.*?)\s+\(([\d,]+)x\)", l)
    if m: rows.append((int(m.group(1).replace(",", "")), int(m.group(3).replace(",", "")), re.sub(r"::h[0-9a-f]{16}", "", m.group(2))))
rows.sort(reverse=True)
print(f"memcpy per request, by caller (Ir/request, calls/request); run had ~{reqs} requests")
for ir, calls, name in rows[:10]:
    print(f"{ir/reqs:>10,.0f} {calls/reqs:>6.1f}  {name[:160]}")
