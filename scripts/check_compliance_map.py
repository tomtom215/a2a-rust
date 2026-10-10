#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
"""Keep the regulatory control map honest.

`docs/compliance/controls.toml` maps provisions of the EU AI Act, the Cyber
Resilience Act, the GDPR, OWASP's agentic Top 10 and ISO/IEC 42001 onto what
this SDK provides. A map like that is only worth what its citations are worth:
a row that says "supported, see test X" after X was renamed or deleted is a
claim with nothing behind it, and it reads exactly like one with something
behind it. This script makes the citations load-bearing.

`--check` (CI) fails when:

  * a cited test does not exist — the file is missing, it has no `fn <name>(`,
    or that function carries no `#[test]` / `#[tokio::test]` / `#[sqlx::test]`
    attribute in the lines above it;
  * a cited evidence path does not exist;
  * a cited CI step (`ci = [...]`) appears in no workflow under
    `.github/workflows/` — the string must occur verbatim in one;
  * a row's status and fields disagree: `supported` backed by neither a test
    nor a CI step, `documented` with no evidence file, either of them with a
    gap text, any other status without one, an unknown status or source;
  * two rows share an id;
  * `docs/compliance/control-map.md` is not what the TOML renders to.

`--write` regenerates the Markdown from the TOML.

Exit 0 when the map holds, 1 when it does not, 2 on a usage or parse error.
"""

from __future__ import annotations

import argparse
import re
import sys
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SOURCE = ROOT / "docs/compliance/controls.toml"
RENDERED = ROOT / "docs/compliance/control-map.md"

STATUSES = {
    "supported": "Supported",
    "documented": "Documented",
    "partial": "Partial",
    "gap": "Gap",
    "integrator": "Integrator's duty",
}
WORKFLOWS = [p.read_text(encoding="utf-8") for p in sorted((ROOT / ".github/workflows").glob("*.yml"))]
TEST_ATTR = re.compile(r"#\[\s*(?:tokio::|sqlx::)?test\b")


def load(path: Path) -> dict:
    try:
        return tomllib.loads(path.read_text(encoding="utf-8"))
    except (OSError, tomllib.TOMLDecodeError) as e:
        print(f"check_compliance_map: cannot read {path}: {e}", file=sys.stderr)
        sys.exit(2)


def test_exists(ref: str) -> str | None:
    """None if `path::name` names a test function, else why not."""
    if "::" not in ref:
        return f"{ref!r} is not of the form path/to/file.rs::test_name"
    path, name = ref.rsplit("::", 1)
    f = ROOT / path
    if not f.is_file():
        return f"{ref}: {path} does not exist"
    lines = f.read_text(encoding="utf-8").splitlines()
    pat = re.compile(rf"\bfn {re.escape(name)}\s*[(<]")
    for i, line in enumerate(lines):
        if pat.search(line):
            window = lines[max(0, i - 8) : i]
            if any(TEST_ATTR.search(w) for w in window):
                return None
            return f"{ref}: `fn {name}` exists but carries no test attribute"
    return f"{ref}: no `fn {name}` in {path}"


def validate(doc: dict) -> list[str]:
    errors: list[str] = []
    sources = {s["id"]: s for s in doc.get("sources", [])}
    for s in sources.values():
        if s.get("verification") not in ("primary", "secondary"):
            errors.append(f"source {s['id']}: verification must be primary or secondary")
    seen: set[str] = set()
    controls = doc.get("control", [])
    if not controls:
        errors.append("no [[control]] rows")
    for c in controls:
        cid = c.get("id", "<no id>")
        if cid in seen:
            errors.append(f"{cid}: duplicate id")
        seen.add(cid)
        for key in ("source", "provision", "requirement", "sdk", "status"):
            if not c.get(key):
                errors.append(f"{cid}: missing `{key}`")
        if c.get("source") not in sources:
            errors.append(f"{cid}: unknown source {c.get('source')!r}")
        status = c.get("status")
        if status not in STATUSES:
            errors.append(f"{cid}: unknown status {status!r}")
        tests = c.get("tests", [])
        ci = c.get("ci", [])
        gap = c.get("gap", "").strip()
        if status == "supported" and not (tests or ci):
            errors.append(f"{cid}: `supported` cites neither a test nor a CI step")
        if status == "documented" and not c.get("evidence"):
            errors.append(f"{cid}: `documented` cites no evidence file")
        if status in ("supported", "documented"):
            if gap:
                errors.append(f"{cid}: `{status}` with a gap text — is it partial?")
        elif status in STATUSES and not gap:
            errors.append(f"{cid}: `{status}` must say what is missing, in `gap`")
        for step in ci:
            if not any(step in w for w in WORKFLOWS):
                errors.append(f"{cid}: CI step {step!r} appears in no workflow")
        for t in tests:
            why = test_exists(t)
            if why:
                errors.append(f"{cid}: {why}")
        for e in c.get("evidence", []):
            if not (ROOT / e).exists():
                errors.append(f"{cid}: evidence {e} does not exist")
    return errors


def cell(text: str) -> str:
    return " ".join(text.split()).replace("|", "\\|")


def render(doc: dict) -> str:
    meta = doc["meta"]
    sources = {s["id"]: s for s in doc["sources"]}
    out = [
        "<!-- SPDX-License-Identifier: Apache-2.0 -->",
        "<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->",
        "<!-- GENERATED from controls.toml by scripts/check_compliance_map.py --write. Do not edit. -->",
        "",
        "# Regulatory control map",
        "",
        cell(meta["preamble"]),
        "",
        f"As of {meta['as_of']}. Every cited test is checked to exist, as a test, by"
        " `scripts/check_compliance_map.py` in CI; a row cannot outlive its evidence.",
        "",
    ]
    counts = {k: 0 for k in STATUSES}
    for c in doc["control"]:
        counts[c["status"]] += 1
    out.append(
        "| " + " | ".join(STATUSES[k] for k in STATUSES) + " |\n|"
        + "---|" * len(STATUSES) + "\n| "
        + " | ".join(str(counts[k]) for k in STATUSES) + " |"
    )
    out.append("")
    for sid, s in sources.items():
        rows = [c for c in doc["control"] if c["source"] == sid]
        if not rows:
            continue
        out += [f"## {s['title']}", ""]
        label = "read in the primary text" if s["verification"] == "primary" else (
            "**identifiers and wording from secondary sources; check against the"
            " published text before relying on them**"
        )
        out += [f"Source: <{s['url']}> — {label}.", ""]
        if s.get("note"):
            out += [cell(s["note"]), ""]
        out += [
            "| ID | Provision | Requirement | What the SDK provides | Status | Evidence | Gap or reason |",
            "|---|---|---|---|---|---|---|",
        ]
        for c in rows:
            ev = [f"`{t.rsplit('::', 1)[1]}`" for t in c.get("tests", [])]
            ev += [f"CI: {cell(x)}" for x in c.get("ci", [])]
            ev += [f"[{Path(e).name}](../../{e})" for e in c.get("evidence", [])]
            tests = "<br>".join(ev) or "—"
            out.append(
                f"| {c['id']} | {cell(c['provision'])} | {cell(c['requirement'])} | "
                f"{cell(c['sdk'])} | {STATUSES[c['status']]} | {tests} | "
                f"{cell(c.get('gap', '')) or '—'} |"
            )
        out.append("")
    out += ["## Where each cited test lives", ""]
    for c in doc["control"]:
        for t in c.get("tests", []):
            path, name = t.rsplit("::", 1)
            out.append(f"- `{name}` — `{path}` ({c['id']})")
    out.append("")
    return "\n".join(out)


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    g = ap.add_mutually_exclusive_group()
    g.add_argument("--check", action="store_true", help="validate (default)")
    g.add_argument("--write", action="store_true", help="regenerate control-map.md")
    args = ap.parse_args()

    doc = load(SOURCE)
    errors = validate(doc)
    if errors:
        for e in errors:
            print(f"check_compliance_map: {e}", file=sys.stderr)
        return 1
    text = render(doc)
    if args.write:
        RENDERED.write_text(text, encoding="utf-8")
        print(f"check_compliance_map: wrote {RENDERED.relative_to(ROOT)}")
        return 0
    if not RENDERED.is_file() or RENDERED.read_text(encoding="utf-8") != text:
        print(
            "check_compliance_map: docs/compliance/control-map.md is stale; "
            "run scripts/check_compliance_map.py --write",
            file=sys.stderr,
        )
        return 1
    n = len(doc["control"])
    t = sum(len(c.get("tests", [])) for c in doc["control"])
    print(f"check_compliance_map: {n} control(s), {t} cited test(s), all present; map current")
    return 0


if __name__ == "__main__":
    sys.exit(main())
