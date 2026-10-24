# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
"""Compares the direct and SDK arms cell by cell, for docs/tool-calling-study.md.

For each (model, thinking, prompt) it takes the audited verdicts (score.py's,
overridden by audit.json where an audit changed them) and reports the SDK
arm's correct rate minus the direct arm's, with a Newcombe hybrid-score 95%
interval for the difference, a two-sided Fisher exact p-value, and the
Holm-adjusted p-value across the 16 comparisons (the family the study's
question defines). Two secondary measures follow, each with its own Holm
family: checkout answers stating a wrong uptime (8 comparisons), and, in the
thinking-on cells, answers that came back empty (4 comparisons). Those two
were not planned before the runs were scored; read them as exploratory.

Usage: compare.py RUNS_DIR AUDIT_JSON
"""
import json
import math
import sys
from math import comb
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import score  # noqa: E402

MODELS = ["qwen3.5-0.8b-q4_0", "qwen3.5-0.8b-q4_k_m", "qwen3-1.7b-q4_k_m", "qwen3.5-2b-q4_k_m"]


def fisher(a, n1, b, n2):
    """Two-sided Fisher exact p for a/n1 against b/n2 (summing tables no likelier)."""
    k, n = a + b, n1 + n2

    def p(x):
        return comb(n1, x) * comb(n2, k - x) / comb(n, k)

    p0 = p(a)
    return min(1.0, sum(p(x) for x in range(max(0, k - n2), min(k, n1) + 1) if p(x) <= p0 * (1 + 1e-9)))


def newcombe(a, n1, b, n2):
    """Newcombe's hybrid score interval for b/n2 - a/n1, from Wilson intervals."""
    l1, u1 = score.wilson(a, n1)
    l2, u2 = score.wilson(b, n2)
    p1, p2 = a / n1, b / n2
    d = p2 - p1
    return d - math.sqrt((p2 - l2) ** 2 + (u1 - p1) ** 2), d + math.sqrt((u2 - p2) ** 2 + (p1 - l1) ** 2)


def holm(ps):
    """Holm step-down adjusted p-values, in the input order."""
    order = sorted(range(len(ps)), key=lambda i: ps[i])
    out, running = [0.0] * len(ps), 0.0
    for rank, i in enumerate(order):
        running = max(running, min(1.0, (len(ps) - rank) * ps[i]))
        out[i] = running
    return out


def verdicts(runs_dir, audit, cell, kind):
    for r in json.load(open(runs_dir / f"{cell}.json"))["runs"]:
        if r["kind"] == kind:
            ok, cls = score.classify(r)
            yield r, audit.get((cell, kind, r["seed"]), (ok, cls))


def count(runs_dir, audit, cell, kind, pred):
    rows = list(verdicts(runs_dir, audit, cell, kind))
    return sum(1 for r, v in rows if pred(r, v)), len(rows)


def table(title, rows):
    adj = holm([r[-1] for r in rows])
    print(f"## {title}")
    print("| Model | Thinking | Prompt | Direct | SDK | SDK − direct [95% CI] | Fisher p | Holm p |")
    print("|---|---|---|---:|---:|---|---:|---:|")
    for (m, t, kind, a, n1, b, n2, p), h in zip(rows, adj):
        lo, hi = newcombe(a, n1, b, n2)
        print(f"| {m} | {t} | {kind} | {a}/{n1} | {b}/{n2} | {(b / n2 - a / n1):+.2f} [{lo:+.2f}, {hi:+.2f}] | {p:.3g} | {h:.3g} |")
    print()


def main():
    runs_dir, audit_path = Path(sys.argv[1]), sys.argv[2]
    audit = {(o["cell"], o["kind"], o["seed"]): tuple(o["audited"])
             for o in json.load(open(audit_path))["overrides"]}
    correct, uptime, empty = [], [], []
    for m in MODELS:
        for t in ("off", "on"):
            for kind in ("billing", "checkout"):
                a, n1 = count(runs_dir, audit, f"direct_{m}_{t}", kind, lambda r, v: v[0])
                b, n2 = count(runs_dir, audit, f"sdk_{m}_{t}", kind, lambda r, v: v[0])
                correct.append((m, t, kind, a, n1, b, n2, fisher(a, n1, b, n2)))

            def wrong(r, v):
                return bool(r["final"]) and score.uptime_wrong(r["final"])
            a, n1 = count(runs_dir, audit, f"direct_{m}_{t}", "checkout", wrong)
            b, n2 = count(runs_dir, audit, f"sdk_{m}_{t}", "checkout", wrong)
            uptime.append((m, t, "checkout", a, n1, b, n2, fisher(a, n1, b, n2)))
        a = b = n1 = n2 = 0
        for kind in ("billing", "checkout"):
            x, y = count(runs_dir, audit, f"direct_{m}_on", kind, lambda r, v: v[1] == "empty_answer")
            a, n1 = a + x, n1 + y
            x, y = count(runs_dir, audit, f"sdk_{m}_on", kind, lambda r, v: v[1] == "empty_answer")
            b, n2 = b + x, n2 + y
        empty.append((m, "on", "both", a, n1, b, n2, fisher(a, n1, b, n2)))
    table("Correct answers (the study's question; Holm over 16)", correct)
    table("Checkout answers stating a wrong uptime (exploratory; Holm over 8)", uptime)
    table("Empty answers, thinking on, both prompts (exploratory; Holm over 4)", empty)


if __name__ == "__main__":
    main()
