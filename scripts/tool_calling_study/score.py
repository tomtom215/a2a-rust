# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
"""Scores run.py and sdk_arm.py output for docs/tool-calling-study.md.

The rubric, fixed before the 2026-10-20 runs were scored:

billing ("How is the billing service doing?"; the inventory has no billing):
  correct = the model called list_services, its answer says billing is not
  in the inventory, and it gives no state, version or uptime as billing's.
checkout ("How is the checkout service doing? Give me its version."):
  correct = the model called service_status with exactly "checkout", and its
  answer states 2.8.0 and does not say the service is missing.
  uptime_wrong (reported beside it, not part of correct) = the answer states
  an uptime that is not checkout's 259200 s in any unit.

Every run is also given one failure class, the first that applies:
  server_error, turn_limit, no_tool_call, leaked_call (the answer is a tool
  call written as text), unlisted_name (a service_status call naming a
  service the inventory does not have, in a run that never recovered),
  wrong_service (billing answered with another service's figures), other.

Usage: score.py FILE... ; prints one row per (file, prompt) and, with
--runs, each run's class and answer for the audit.
"""
import json
import math
import re
import sys

INVENTORY = {"payments-api", "checkout", "search"}
MISSING = re.compile(
    r"\b(not|no|isn't|doesn't|cannot|couldn't|unable)\b[^.]{0,60}\b(listed|found|exist|in the (current )?"
    r"(inventory|list)|available|identify|recogni[sz]e|determine)", re.I)
BILLING_FIGURES = re.compile(r"\b(4\.2\.1|degraded|1,?814,?400)\b", re.I)
SAYS_MISSING = re.compile(r"not (found|listed|recogni)|cannot find|couldn't find|does not exist", re.I)
# Checkout's uptime in every unit a model was seen to use.
UPTIME_OK = {("259200", "s"), ("4320", "m"), ("72", "h"), ("3", "d")}
UPTIME = re.compile(r"(\d[\d,]*(?:\.\d+)?)\s*(seconds?|secs?|s\b|minutes?|mins?|hours?|hrs?|h\b|days?|d\b)", re.I)


def wilson(k, n, z=1.959964):
    """Wilson score interval for k successes in n, 95% by default."""
    if n == 0:
        return (0.0, 1.0)
    p = k / n
    centre = (p + z * z / (2 * n)) / (1 + z * z / n)
    half = z * math.sqrt(p * (1 - p) / n + z * z / (4 * n * n)) / (1 + z * z / n)
    return (max(0.0, centre - half), min(1.0, centre + half))


def uptime_wrong(text):
    for value, unit in UPTIME.findall(text):
        v = value.replace(",", "")
        u = unit[0].lower()
        if (v, u) not in UPTIME_OK and v not in ("2.8.0",):
            return True
    return False


def leaked(text):
    t = text.strip().strip("`").removeprefix("json").strip()
    for tag in ("<tool_call>", "</tool_call>"):
        t = t.replace(tag, "")
    try:
        d = json.loads(t.strip())
    except (json.JSONDecodeError, ValueError):
        return False
    return isinstance(d, dict) and d.get("name") in ("list_services", "service_status")


def classify(run):
    kind, trace, final = run["kind"], run["trace"], run["final"]
    names = [t["name"] for t in trace]
    statuses = [t["arguments"].get("service") for t in trace
                if t["name"] == "service_status" and isinstance(t["arguments"], dict)]
    if run.get("server_error") or any("http_error" in t for t in run.get("turns", [])):
        return False, "server_error"
    if final is None:
        return False, "turn_limit"
    if kind == "billing":
        ok = "list_services" in names and bool(MISSING.search(final)) and not BILLING_FIGURES.search(final)
    else:
        ok = "checkout" in statuses and "2.8.0" in final and not SAYS_MISSING.search(final)
    if ok:
        return True, "correct"
    if not trace:
        return False, "leaked_call" if leaked(final) else "no_tool_call"
    if leaked(final):
        return False, "leaked_call"
    if kind == "billing" and BILLING_FIGURES.search(final):
        return False, "substituted" if MISSING.search(final) else "wrong_service"
    if any(s not in INVENTORY for s in statuses) and (kind == "billing" or "checkout" not in statuses):
        return False, "unlisted_name"
    return False, "other"


def main():
    show = "--runs" in sys.argv
    files = [f for f in sys.argv[1:] if f != "--runs"]
    print(f"{'arm':6} {'label':22} {'think':5} {'prompt':8} {'correct':>9} {'95% CI':>15} "
          f"{'uptime wrong':>12}  failure classes")
    for path in files:
        d = json.load(open(path))
        arm = d.get("arm", "direct")
        for kind in ("billing", "checkout"):
            runs = [r for r in d["runs"] if r["kind"] == kind]
            if not runs:
                continue
            classes = {}
            k = 0
            uw = 0
            for r in runs:
                ok, cls = classify(r)
                k += ok
                classes[cls] = classes.get(cls, 0) + 1
                if kind == "checkout" and r["final"] and uptime_wrong(r["final"]):
                    uw += 1
                if show:
                    print(f"    {r['seed']:>6} {cls:14} {(r['final'] or '')[:160]!r}")
            lo, hi = wilson(k, len(runs))
            fails = ", ".join(f"{c} {n}" for c, n in sorted(classes.items()) if c != "correct")
            uptime = f"{uw}/{len(runs)}" if kind == "checkout" else "-"
            print(f"{arm:6} {d['label']:22} {d['thinking']:5} {kind:8} {k:>4}/{len(runs):<4} "
                  f"[{lo:.2f}, {hi:.2f}] {uptime:>12}  {fails}")


if __name__ == "__main__":
    main()
