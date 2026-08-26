#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
"""Ratchet on panicking constructs in the published crates' runtime code.

The question an adopter of a protocol library actually has is whether a
malformed peer can take their process down. Until this existed the answer here
was unquotable, and that was not laziness — a `grep` for `.unwrap()` counts
matches inside doc comments, inside string literals, and inside `#[cfg(test)]`
modules, and in this repository the test hits outnumber the real ones by more
than thirty to one. A number nobody can defend is worse than no number, so B5
sat open across four review passes.

What makes the count defensible:

  * comments and string/char literals are blanked by a state machine, so
    `/// call .unwrap() only when...` and `"expected .expect("` do not count;
  * inline `#[cfg(test)]` items are excised by brace matching;
  * a file declared by its parent as `#[cfg(test)] mod name;` is dropped
    entirely. This is the one that matters. The first version of this scan
    reported 178 `.unwrap()` and every one of its top hits was a `*tests.rs`
    file, because such a file carries no `#[cfg(test)]` of its own — the
    attribute is in the parent module. `--self-test` pins that case;
  * `build.rs` is counted separately. A panic there fails somebody's build,
    loudly, at a moment they are already watching; it is not a runtime hazard
    and lumping it in would overstate the count.

It is a ratchet, not a ban. Several of the surviving sites are correct — a
rustls provider that cannot fail, a loop bound that makes an `Option` provably
`Some`, and a deliberate fail-fast on a poisoned credentials lock where a silent
`None` would be an auth downgrade. Freezing the exact set means adding one is a
reviewed act rather than an accident, and removing one has to be recorded too,
so the baseline cannot quietly loosen.

Scope is `crates/*/src` — the four published crates. The SLIMRPC binding is not
covered; it is a separate, unpublished crate with its own CI job.

Usage:
    scripts/check_panic_paths.py              check against the baseline
    scripts/check_panic_paths.py --update     rewrite the baseline
    scripts/check_panic_paths.py --self-test  check the exclusion logic alone

Exit 0 if every file matches its baseline exactly, non-zero otherwise.
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
BASELINE = ROOT / "scripts" / "panic_paths_baseline.txt"

PATTERNS = {
    "unwrap": re.compile(r"\.unwrap\(\)"),
    "expect": re.compile(r"\.expect\("),
    "panic": re.compile(r"\bpanic!\("),
    "todo": re.compile(r"\b(?:todo|unimplemented)!\("),
}
CFG_TEST_ITEM = re.compile(r"#\[cfg\(test\)\]")
CFG_TEST_MOD = re.compile(r"#\[cfg\(test\)\]\s*(?:pub\s+)?mod\s+(\w+)\s*;")


def strip_noise(src: str) -> str:
    """Blank comments and string/char literals, preserving offsets and newlines."""
    out = list(src)
    i, n = 0, len(src)

    def blank(a: int, b: int) -> None:
        for k in range(a, min(b, n)):
            if out[k] != "\n":
                out[k] = " "

    while i < n:
        c = src[i]
        if c == "/" and src.startswith("//", i):
            j = src.find("\n", i)
            j = n if j < 0 else j
            blank(i, j); i = j
        elif c == "/" and src.startswith("/*", i):
            depth, j = 1, i + 2
            while j < n and depth:
                if src.startswith("/*", j): depth += 1; j += 2
                elif src.startswith("*/", j): depth -= 1; j += 2
                else: j += 1
            blank(i, j); i = j
        elif c == "r" and (m := re.match(r'r(#*)"', src[i:])):
            close = '"' + m.group(1)
            j = src.find(close, i + len(m.group(0)))
            j = n if j < 0 else j + len(close)
            blank(i, j); i = j
        elif c == '"':
            j = i + 1
            while j < n:
                if src[j] == "\\": j += 2; continue
                if src[j] == '"': j += 1; break
                j += 1
            blank(i, j); i = j
        elif c == "'":
            m = re.match(r"'(?:\\.|[^\\'])'", src[i:])
            if m: blank(i, i + m.end()); i += m.end()
            else: i += 1          # a lifetime, not a literal
        else:
            i += 1
    return "".join(out)


def excise_cfg_test(src: str) -> str:
    """Blank every `#[cfg(test)]` item by matching its braces."""
    out = list(src)
    for m in CFG_TEST_ITEM.finditer(src):
        brace = src.find("{", m.end())
        if brace < 0:
            continue
        depth, k = 0, brace
        while k < len(src):
            if src[k] == "{": depth += 1
            elif src[k] == "}":
                depth -= 1
                if depth == 0:
                    k += 1
                    break
            k += 1
        for x in range(m.start(), min(k, len(src))):
            if out[x] != "\n":
                out[x] = " "
    return "".join(out)


ANY_MOD = re.compile(r"(?:^|\n)\s*(?:#\[[^\]]*\]\s*)*(?:pub(?:\([^)]*\))?\s+)?mod\s+(\w+)\s*;")


def _module_file(parent: Path, name: str) -> Path | None:
    base = parent.parent if parent.name in ("mod.rs", "lib.rs") else parent.parent / parent.stem
    for cand in (base / f"{name}.rs", base / name / "mod.rs",
                 parent.parent / f"{name}.rs", parent.parent / name / "mod.rs"):
        if cand.exists():
            return cand.resolve()
    return None


def test_gated_files() -> set[Path]:
    """Files reachable only under `#[cfg(test)]`, following the tree transitively.

    Gating is inherited: `#[cfg(test)] mod tests;` makes `tests/mod.rs` test-only,
    and *everything `tests/mod.rs` declares* along with it. Marking only the
    directly-declared file left `handler/shutdown/tests/warning.rs` — seven
    `.expect(` calls in a file whose own first line says it is a test — counted
    as runtime code. It carries no `#[cfg(test)]` of its own and its name does
    not contain "test", so neither the attribute scan nor the name self-check
    would have caught it.
    """
    roots: set[Path] = set()
    for f in ROOT.joinpath("crates").rglob("*.rs"):
        for name in CFG_TEST_MOD.findall(f.read_text(encoding="utf-8", errors="replace")):
            if (hit := _module_file(f, name)) is not None:
                roots.add(hit)

    gated: set[Path] = set()
    pending = list(roots)
    while pending:
        f = pending.pop()
        if f in gated or not f.exists():
            continue
        gated.add(f)
        for name in ANY_MOD.findall(f.read_text(encoding="utf-8", errors="replace")):
            if (child := _module_file(f, name)) is not None and child not in gated:
                pending.append(child)
    return gated


def scan() -> tuple[dict[str, dict[str, int]], list[str]]:
    """Return {relative path: {kind: count}} and any self-check complaints."""
    gated = test_gated_files()
    counts: dict[str, dict[str, int]] = {}
    for crate in sorted(ROOT.joinpath("crates").iterdir()):
        srcs = list(crate.joinpath("src").rglob("*.rs")) if crate.joinpath("src").is_dir() else []
        if crate.joinpath("build.rs").exists():
            srcs.append(crate / "build.rs")
        for f in sorted(srcs):
            if f.resolve() in gated:
                continue
            body = excise_cfg_test(strip_noise(f.read_text(encoding="utf-8", errors="replace")))
            found = {k: len(r.findall(body)) for k, r in PATTERNS.items()}
            if any(found.values()):
                counts[str(f.relative_to(ROOT))] = found

    # If a file whose name says "test" survived every filter, the filters are
    # what should be doubted first — that is how the first version of this
    # scan produced a confident, wrong number.
    complaints = [
        f"{p}: counted despite a test-like name; check the exclusion logic"
        for p in counts
        if "test" in Path(p).name and "vector" not in Path(p).name
    ]
    return counts, complaints


def render(counts: dict[str, dict[str, int]]) -> str:
    lines = [
        "# Generated by scripts/check_panic_paths.py --update. Do not edit by hand.",
        "# One line per file: unwrap expect panic todo  path",
    ]
    lines += [
        f"{c['unwrap']} {c['expect']} {c['panic']} {c['todo']}  {p}"
        for p, c in sorted(counts.items())
    ]
    return "\n".join(lines) + "\n"


def load() -> dict[str, dict[str, int]]:
    if not BASELINE.exists():
        return {}
    out: dict[str, dict[str, int]] = {}
    for line in BASELINE.read_text().splitlines():
        if not line.strip() or line.startswith("#"):
            continue
        u, e, p, t, path = line.split(None, 4)
        out[path.strip()] = {"unwrap": int(u), "expect": int(e), "panic": int(p), "todo": int(t)}
    return out


SELF_TEST_SRC = '''
fn real() { let a = x.unwrap(); }
/// Doc comment mentioning .unwrap() which must not count.
// Line comment with .expect("no").
/* block .unwrap() */
fn strings() { let s = ".unwrap()"; let r = r#"and .expect("x")"#; }
#[cfg(test)]
mod tests {
    #[test] fn t() { y.unwrap(); z.expect("nope"); panic!("no"); }
}
fn second_real() { b.expect("yes"); }
'''


def self_test() -> int:
    body = excise_cfg_test(strip_noise(SELF_TEST_SRC))
    got = {k: len(r.findall(body)) for k, r in PATTERNS.items()}
    want = {"unwrap": 1, "expect": 1, "panic": 0, "todo": 0}
    if got != want:
        print(f"check_panic_paths --self-test: FAILED\n\n  counted {got}, expected {want}",
              file=sys.stderr)
        return 1
    print("check_panic_paths --self-test: comments, literals and cfg(test) all excluded")
    return 0


def main() -> int:
    if "--self-test" in sys.argv[1:]:
        return self_test()
    if self_test() != 0:
        return 1

    counts, complaints = scan()
    if complaints:
        print("check_panic_paths: the scan does not trust its own result\n", file=sys.stderr)
        for c in complaints:
            print(f"  {c}", file=sys.stderr)
        return 1

    if "--update" in sys.argv[1:]:
        BASELINE.write_text(render(counts))
        print(f"check_panic_paths: baseline rewritten — {len(counts)} file(s)")
        return 0

    base = load()
    problems = []
    for path in sorted(set(counts) | set(base)):
        now, was = counts.get(path), base.get(path)
        if was is None:
            problems.append(f"{path}: new panicking construct(s) {now}")
        elif now is None:
            problems.append(f"{path}: no longer present in the tree; run --update")
        elif now != was:
            deltas = ", ".join(f"{k} {was[k]}->{now[k]}" for k in PATTERNS if was[k] != now[k])
            problems.append(f"{path}: {deltas}")

    if problems:
        print("check_panic_paths: the panic surface of the published crates changed\n",
              file=sys.stderr)
        for p in problems:
            print(f"  {p}", file=sys.stderr)
        print(
            "\nThis is a ratchet, not a ban. Several existing sites are correct.\n"
            "If the new one is too, say why where it is, then run:\n"
            "    scripts/check_panic_paths.py --update\n"
            "and commit the baseline with the change, so it is a reviewed act.",
            file=sys.stderr,
        )
        return 1

    runtime = {k: sum(c[k] for p, c in counts.items() if not p.endswith("build.rs"))
               for k in PATTERNS}
    build = {k: sum(c[k] for p, c in counts.items() if p.endswith("build.rs")) for k in PATTERNS}
    print(
        f"check_panic_paths: runtime library code has {runtime['unwrap']} unwrap, "
        f"{runtime['expect']} expect, {runtime['panic']} panic!, {runtime['todo']} todo!; "
        f"build scripts {build['unwrap']} unwrap, {build['expect']} expect "
        f"(across {len(counts)} file(s), matching the baseline)"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
