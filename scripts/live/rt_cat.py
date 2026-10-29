#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
"""Splits the round trips rt_measure.py recorded between its marks into
statements, prepares, pings and the rest. Usage: rt_cat.py RT.jsonl"""
import json, sys
recs = [json.loads(l) for l in open(sys.argv[1])]
m = [i for i, r in enumerate(recs) if "mark" in r]
w = recs[m[0] + 1:m[1]]
ping = sum(1 for r in w if r["msgs"] == "S")
prep = sum(1 for r in w if r["msgs"].startswith("P"))
stmt = sum(1 for r in w if "E" in r["msgs"])
print(f"{sys.argv[1].rsplit('/',1)[-1]}: total {len(w)} = statements {stmt} + prepares {prep} + pings {ping} + other {len(w)-stmt-prep-ping}")
