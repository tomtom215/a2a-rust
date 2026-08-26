#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
"""Check that `docs/provenance-manifest.md` still describes this commit.

That document is written for a downstream project's counsel — the A2A project
and the Linux Foundation are named in it by name — and its whole value is that
its figures are true. Nothing forced it to be regenerated, and between
`c008ab0` (2026-08-11) and `7093af3` (2026-08-26) it drifted by 228 commits.

The drift ran in the project's *favour*, which is the part worth stating: the
share of history passing the project's own DCO gate had gone from 19.4% to
39.2% and the document still said 19.4%. A counsel-facing document that
understates the project is as much a defect as one that overstates it, and it
is the harder one to notice, because nobody is motivated to check it.

This runs in `release.yml`'s validate job rather than on every commit. Every
commit changes the counts, so a per-commit gate would fail constantly and be
routed around within a week — the argument `check_file_lengths.sh` makes about
its own ratchet. Tying it to the release instead means the manifest is exactly
true at each published version, which is the only moment anyone cites it.

Exit 0 if the manifest's pinned commit is the one being released and every
headline figure matches a fresh measurement. Non-zero otherwise.
"""

from __future__ import annotations

import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
DOC = ROOT / "docs" / "provenance-manifest.md"
GENERATOR = ROOT / "scripts" / "provenance_manifest.sh"

PINNED = re.compile(r"\*\*Measured (\d{4}-\d{2}-\d{2}) at `([0-9a-f]{7,40})`")

# Each headline figure, as the document writes it and as the generator reports it.
DOC_ROWS = {
    "total":     re.compile(r"^\| Total reachable \| (\d+) \|", re.M),
    "merges":    re.compile(r"^\| Merge commits \(`dco\.yml` does not examine these\) \| (\d+) \|", re.M),
    "nonmerge":  re.compile(r"^\| \*\*Non-merge commits[^|]*\| \*\*(\d+)\*\* \|", re.M),
    "pass":      re.compile(r"^\| \*\*Would pass\*\*[^|]*\| \*\*(\d+)\*\* \| ([\d.]+)% \|", re.M),
    "ai":        re.compile(r"^\| Fail — author `noreply@anthropic\.com` \| (\d+) \| ([\d.]+)% \|", re.M),
    "bot":       re.compile(r"^\| Fail — author `github-actions\[bot\]` \| (\d+) \| ([\d.]+)% \|", re.M),
    "nosignoff": re.compile(r"^\| Fail — human author, no matching `Signed-off-by` \| (\d+) \| ([\d.]+)% \|", re.M),
}

GEN_ROWS = {
    "total":     re.compile(r"^commits reachable\s+(\d+)", re.M),
    "merges":    re.compile(r"^\s+merge commits\s+(\d+)", re.M),
    "nonmerge":  re.compile(r"^\s+non-merge commits\s+(\d+)", re.M),
    "pass":      re.compile(r"^\s+would pass\s+(\d+)", re.M),
    "ai":        re.compile(r"^\s+fail — author noreply@anthropic\.com\s+(\d+)", re.M),
    "bot":       re.compile(r"^\s+fail — author \*\[bot\]@users\.noreply\s+(\d+)", re.M),
    "nosignoff": re.compile(r"^\s+fail — human author, no sign-off\s+(\d+)", re.M),
}


def fail(*lines: str) -> int:
    print("check_provenance_manifest: FAILED\n", file=sys.stderr)
    for line in lines:
        print(f"  {line}", file=sys.stderr)
    print(
        "\n  Regenerate and update the document in the release-prep commit:\n"
        "      scripts/provenance_manifest.sh HEAD\n",
        file=sys.stderr,
    )
    return 1


def main() -> int:
    rev = sys.argv[1] if len(sys.argv) > 1 else "HEAD"

    if subprocess.run(["git", "rev-parse", "--is-shallow-repository"],
                      capture_output=True, text=True,
                      cwd=ROOT).stdout.strip() == "true":
        # The generator has its own guard; this one exists so the *reason* is
        # named here too. A shallow clone truncates the oldest history, which
        # is exactly where the non-compliant commits are, so it does not
        # produce a smaller number — it produces a flattering one.
        return fail("this is a shallow clone; the figures it would produce are wrong",
                    "and wrong in the project's favour. Check out with fetch-depth: 0.")

    doc = DOC.read_text()
    pin = PINNED.search(doc)
    if not pin:
        return fail(f"{DOC.name} has no `**Measured <date> at <sha>`** header to check against")
    _, pinned_sha = pin.groups()

    head = subprocess.run(["git", "rev-parse", rev], capture_output=True,
                          text=True, cwd=ROOT).stdout.strip()
    if not head.startswith(pinned_sha):
        return fail(
            f"the manifest is pinned at {pinned_sha}, but this release is {head[:len(pinned_sha)]}",
            "so its figures describe a different commit than the one being published",
        )

    gen = subprocess.run([str(GENERATOR), rev], capture_output=True, text=True, cwd=ROOT)
    if gen.returncode != 0:
        return fail(f"{GENERATOR.name} exited {gen.returncode}", *gen.stderr.strip().splitlines()[:4])

    problems = []
    figures = {}
    for key, doc_re in DOC_ROWS.items():
        dm, gm = doc_re.search(doc), GEN_ROWS[key].search(gen.stdout)
        if not dm:
            problems.append(f"{key}: the manifest no longer has a row this check can read")
            continue
        if not gm:
            problems.append(f"{key}: the generator's output no longer has a line this check can read")
            continue
        figures[key] = int(gm.group(1))
        if int(dm.group(1)) != figures[key]:
            problems.append(f"{key}: manifest says {dm.group(1)}, measured {gm.group(1)}")
        # Where the document also states a share, it must follow from the counts.
        if dm.lastindex and dm.lastindex >= 2 and figures.get("nonmerge"):
            want = round(100 * figures[key] / figures["nonmerge"], 1)
            if abs(float(dm.group(2)) - want) > 0.05:
                problems.append(f"{key}: manifest says {dm.group(2)}%, counts give {want}%")

    if not problems and figures.get("nonmerge") is not None:
        parts = sum(figures[k] for k in ("pass", "ai", "bot", "nosignoff") if k in figures)
        if parts != figures["nonmerge"]:
            problems.append(f"the four verdict counts sum to {parts}, not {figures['nonmerge']}")

    if problems:
        return fail(*problems)

    print(f"check_provenance_manifest: {DOC.name} is measured at {head[:7]} and all "
          f"{len(DOC_ROWS)} headline figures match a fresh run")
    return 0


if __name__ == "__main__":
    sys.exit(main())
