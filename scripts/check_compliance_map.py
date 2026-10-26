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
  * a cited implementation (`code = [...]`, `path::Symbol` or
    `path::Type::method`) is not defined in that file: the file is missing,
    or no `fn`/`struct`/`enum`/`trait`/`type`/`const`/`static`/`mod`/
    `macro_rules!` of that name is declared there (for `Type::method`, the
    method is, and the type is declared or has an `impl` block there);
  * an EU AI Act row the SDK `supported` or partly supports cites no code,
    so its chain from provision to implementation to test to document is
    incomplete;
  * a cited CI step (`ci = [...]`) appears in no workflow under
    `.github/workflows/` — the string must occur verbatim in one;
  * a row cites neither a test nor a CI step, whatever its status, unless it
    is in `UNCITED` below with the reason it has nothing to cite (since
    2026-10-23: every row cites what proves its `sdk` text), or is in
    `UNCITED` and cites something after all;
  * a row cites `DOC_STEP` (this script, run by CI) without an entry in
    `DOC_CLAIMS`, or has an entry there without citing it, or a document an
    entry names no longer says, word for word, what the entry quotes;
  * a row's status and fields disagree: `supported` backed by neither a test
    nor a CI step, `documented` with no evidence file, either of them with a
    gap text, any other status without one, a `remainder` on anything but
    those two, an unknown status or source;
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

# Rows whose `sdk` text says the SDK offers nothing toward the provision, so
# there is no behaviour of the SDK's for a test to prove. Each says why. The
# maintainer decides what, if anything, such a row should cite; until then it
# is listed here rather than left to look like an oversight.
UNCITED: dict[str, str] = {}

# A row whose `sdk` text is a claim that a document says something cites
# this, the command CI runs this script with, and `DOC_CLAIMS` says what each
# document must still say. Whitespace is normalised, so rewrapping a
# paragraph does not trip it; rewording does, which is the point.
DOC_STEP = "python3 scripts/check_compliance_map.py --check"
_SEC = "SECURITY.md"
_BOOK_SEC = "book/src/deployment/security.md"
_ADR15 = "docs/adr/0015-audit-trail.md"
_LICENCE = [
    ("LICENSE", "Apache License Version 2.0, January 2004"),
    ("Cargo.toml", 'license = "Apache-2.0"'),
] + [(f"crates/{c}/Cargo.toml", "license.workspace = true") for c in (
    "a2a-protocol-types", "a2a-protocol-client", "a2a-protocol-server", "a2a-protocol-sdk")]
_CHANNELS = [
    (_SEC, "1. **GitHub Security Advisories (preferred):** Open a draft advisory"),
    (_SEC, "2. **Email:** Send a detailed report to **tomf@tomtomtech.net**"),
]
_MANUFACTURERS = [
    (_SEC, "## For manufacturers integrating these crates (EU Cyber Resilience Act)"),
    (_SEC, "Say in the report that it is an Article 13(6) report; it gets the same "
           "3-business-day acknowledgement as any other. If you have a fix, send it"),
    (_SEC, "**Your legal deadlines win over our embargo.**"),
    (_SEC, "do so on the law's timetable, and tell us at the same time"),
]
DOC_CLAIMS = {
    "AIA-4": [
        ("book/src/SUMMARY.md", "[Audit Trail](./deployment/audit.md)"),
        ("book/src/SUMMARY.md", "[Human Oversight](./deployment/oversight.md)"),
        ("book/src/SUMMARY.md", "[Provenance and Signatures](./deployment/provenance.md)"),
        (_BOOK_SEC, "What this SDK guarantees, what it leaves to you, and what it does not do at all."),
    ],
    "AIA-13-3f": [
        ("book/src/deployment/audit.md", "## What is recorded"),
        ("book/src/deployment/audit.md", "Stores: `InMemoryAuditStore`"),
        ("book/src/deployment/audit.md", "## The checkpoint key"),
        ("book/src/deployment/audit.md", "An auditor should verify an export"),
        ("book/src/deployment/audit.md", "## Following one task across agents"),
        ("book/src/deployment/audit.md", "## Retention and legal holds"),
        ("book/src/deployment/audit.md", "The design, its threat model and its measured cost are in"),
        (_ADR15, "**What tamper evidence does not cover.**"),
    ],
    "AIA-2-12": _LICENCE,
    "AIA-6": [
        ("book/src/SUMMARY.md", "[EU AI Act](./deployment/ai-act-classification.md)"),
        ("book/src/deployment/ai-act-classification.md", "## The two routes to high-risk (Article 6)"),
        ("book/src/deployment/ai-act-classification.md", "**An Annex III system that profiles natural persons is always high-risk.**"),
        ("book/src/deployment/ai-act-classification.md", "Article 6(4): if you decide an Annex III system is not high-risk, you document that assessment before placing it on the market, register under Article 49(2)"),
        ("book/src/deployment/ai-act-classification.md", "## If it is high-risk: what the SDK gives you"),
        ("book/src/deployment/ai-act-classification.md", "## What stays yours"),
    ],
    "AIA-25-2-4": _LICENCE,
    "CRA-13-6": _CHANNELS + _MANUFACTURERS,
    "CRA-AI-II-2-4-8": [
        (_SEC, "A fix for a vulnerability ships as a patch release of the current line "
               "— `0.14.1` for GHSA-hr9h-6jvf-wvg6 is the precedent."),
        (_SEC, "We follow a **90-day coordinated disclosure** timeline"),
        ("CHANGELOG.md", "## [0.14.1] - 2026-09-30 ### Security - **`PathSegmentTenantResolver` "
                         "let a caller choose its own tenant (GHSA-hr9h-6jvf-wvg6).**"),
        ("CHANGELOG.md", "**If you cannot upgrade yet,** strip the inbound `path` header"),
    ] + _LICENCE[:2],
    "CRA-AI-II-5-6": _CHANNELS + _MANUFACTURERS[:2] + [
        (_SEC, "This policy covers **every crate published from this repository**"),
        (_SEC, "**`bindings/a2a-protocol-slimrpc`**, which is publishable and **is covered by this policy**"),
        (_SEC, "we acknowledge within 3 business days"),
        (_SEC, "**Day 1-14** -- We triage the issue"),
        (_SEC, "We follow a **90-day coordinated disclosure** timeline"),
        (_SEC, "We gratefully credit reporters"),
        (_SEC, "There is no published PGP key for this project"),
    ],
    "CRA-AI-I-1": [
        (_BOOK_SEC, "## What holds by construction"),
        (_BOOK_SEC, "what it leaves to you"),
        (_BOOK_SEC, "## Known gaps"),
        ("book/src/deployment/production.md", "# Production Hardening"),
    ],
    "CRA-13-16-18": _CHANNELS + _MANUFACTURERS[:1] + [
        (_SEC, "## Supported Versions"),
        (_SEC, "## Disclosure Timeline"),
        (_BOOK_SEC, "## What holds by construction"),
        (_BOOK_SEC, "## Known gaps"),
        ("book/src/SUMMARY.md", "[Production Hardening](./deployment/production.md)"),
        ("book/src/SUMMARY.md", "[Audit Trail](./deployment/audit.md)"),
        ("book/src/SUMMARY.md", "[Multi-Tenancy](./deployment/multi-tenancy.md)"),
        ("book/src/SUMMARY.md", "[Provenance and Signatures](./deployment/provenance.md)"),
    ],
    "CRA-14": _MANUFACTURERS[2:],
    "CRA-24": [
        (_SEC, "These crates are free and open-source software published by an individual and not monetised"),
        (_SEC, "an open-source software steward (Article 24) is a legal person, which an individual maintainer is not"),
        (_SEC, "## Reporting a Vulnerability"),
        (_SEC, "## Disclosure Timeline"),
        (_SEC, "## Credit"),
    ],
    "GDPR-30": [
        (_BOOK_SEC, "What it does not protect:"),
        (_ADR15, "**What tamper evidence does not cover.**"),
    ],
    "GDPR-35": [
        (_BOOK_SEC, "## Data at rest"),
        (_BOOK_SEC, "What it does not protect:"),
        (_BOOK_SEC, "## Known gaps"),
        (_ADR15, "### What is recorded"),
        (_ADR15, "### How records are made tamper-evident"),
        (_ADR15, "**What tamper evidence does not cover.**"),
        (_ADR15, "**What it does not do.** It does not unlink"),
    ],
}


def normalised(text: str) -> str:
    return " ".join(text.split())


def doc_claims_hold(cid: str) -> list[str]:
    """Each document `DOC_CLAIMS[cid]` names still says what it quotes."""
    errors = []
    for path, phrase in DOC_CLAIMS[cid]:
        f = ROOT / path
        if not f.is_file():
            errors.append(f"{cid}: {path}, whose wording the row relies on, does not exist")
        elif normalised(phrase) not in normalised(f.read_text(encoding="utf-8")):
            errors.append(f"{cid}: {path} no longer says {phrase!r}")
    return errors
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


DEFINES = r"\b(?:fn|struct|enum|trait|type|const|static|mod|union)\s+{name}\b|macro_rules!\s*{name}\b"


def code_exists(ref: str) -> str | None:
    """None if `path::Symbol` (or `path::Type::method`) is defined there."""
    if ".rs::" not in ref:
        return f"{ref!r} is not of the form path/to/file.rs::Symbol"
    path, symbol = ref.split(".rs::", 1)
    path += ".rs"
    f = ROOT / path
    if not f.is_file():
        return f"{ref}: {path} does not exist"
    text = f.read_text(encoding="utf-8")
    *owners, item = symbol.split("::")
    for name in owners:
        # A method's type may be declared elsewhere and implemented here.
        impl = rf"\bimpl\b[^{{;]*\b{re.escape(name)}\b"
        if not (re.search(DEFINES.format(name=re.escape(name)), text) or re.search(impl, text)):
            return f"{ref}: `{name}` is neither defined nor implemented in {path}"
    if not re.search(DEFINES.format(name=re.escape(item)), text):
        return f"{ref}: nothing named `{item}` is defined in {path}"
    return None


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
        elif not (tests or ci) and cid not in UNCITED:
            errors.append(
                f"{cid}: cites neither a test nor a CI step that proves its `sdk` text; "
                "cite one, or list the row in UNCITED with the reason it has none"
            )
        if cid in UNCITED and (tests or ci):
            errors.append(f"{cid}: listed in UNCITED but cites evidence; remove it from UNCITED")
        if DOC_STEP in ci:
            if cid in DOC_CLAIMS:
                errors += doc_claims_hold(cid)
            else:
                errors.append(f"{cid}: cites {DOC_STEP!r} but DOC_CLAIMS says nothing it checks")
        elif cid in DOC_CLAIMS:
            errors.append(f"{cid}: has DOC_CLAIMS but does not cite {DOC_STEP!r}")
        if status == "documented" and not c.get("evidence"):
            errors.append(f"{cid}: `documented` cites no evidence file")
        # `gap` is SDK work not done; `remainder` is what stays with the
        # provider or deployer once the SDK has done all a library can. A
        # finished row with a remainder is `supported`, not `partial`.
        if status in ("supported", "documented"):
            if gap:
                errors.append(f"{cid}: `{status}` with a gap text — is it partial?")
        elif status in STATUSES and not gap:
            errors.append(f"{cid}: `{status}` must say what is missing, in `gap`")
        if c.get("remainder") and status not in ("supported", "documented"):
            errors.append(f"{cid}: `remainder` is for a finished row; `{status}` says it in `gap`")
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
        code = c.get("code", [])
        if c.get("source") == "ai-act" and status in ("supported", "partial") and not code:
            errors.append(f"{cid}: `{status}` cites no code; name what implements it in `code`")
        for ref in code:
            why = code_exists(ref)
            if why:
                errors.append(f"{cid}: {why}")
    for cid in sorted((set(UNCITED) | set(DOC_CLAIMS)) - seen):
        errors.append(f"{cid}: named in UNCITED or DOC_CLAIMS but no such row")
    return errors


def cell(text: str) -> str:
    return " ".join(text.split()).replace("|", "\\|")


def reason(c: dict) -> str:
    if c.get("remainder"):
        return "Left to the integrator: " + cell(c["remainder"])
    return cell(c.get("gap", "")) or "—"


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
            ev = [f"code: `{r.split('.rs::', 1)[1]}`" for r in c.get("code", [])]
            ev += [f"`{t.rsplit('::', 1)[1]}`" for t in c.get("tests", [])]
            ev += [f"CI: {cell(x)}" for x in c.get("ci", [])]
            ev += [f"[{Path(e).name}](../../{e})" for e in c.get("evidence", [])]
            tests = "<br>".join(ev) or "—"
            out.append(
                f"| {c['id']} | {cell(c['provision'])} | {cell(c['requirement'])} | "
                f"{cell(c['sdk'])} | {STATUSES[c['status']]} | {tests} | "
                f"{reason(c)} |"
            )
        out.append("")
    out += ["## What each cited document is checked to say", ""]
    out += [
        "Rows citing `" + DOC_STEP + "` rely on these documents saying, word for word"
        " (whitespace aside), what is quoted here; CI fails when one stops.",
        "",
    ]
    for c in doc["control"]:
        for path, phrase in DOC_CLAIMS.get(c["id"], []):
            out.append(f"- {c['id']}: `{path}` — `` {cell(phrase)} ``")
    out.append("")
    out += ["## Rows that cite nothing", ""]
    for cid, why in UNCITED.items():
        out.append(f"- {cid}: {why}")
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
    k = sum(len(c.get("code", [])) for c in doc["control"])
    print(f"check_compliance_map: {n} control(s), {t} cited test(s), {k} cited implementation(s), "
          "all present; map current")
    return 0


if __name__ == "__main__":
    sys.exit(main())
