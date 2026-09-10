# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

"""Reverse direction of `scripts/check_api_reference.py`: code -> page.

Imported by that script, which puts this directory on `sys.path`; not a
command on its own. Split out on 2026-09-10 when the entry point crossed the
500-line ratchet `scripts/check_file_lengths.sh` enforces.

Every name a crate exports *at its root* must appear in the first column of
some table on the page. Root-level means what a reader meets first —
`use a2a_protocol_types::X` — which is exactly what a curated entry point
exists to introduce. Concretely, from each `crates/*/src/lib.rs`:

  - `pub use path::{A, B as C}` groups and `pub use path::Item` — the leaf
    names (aliases win; `as _` exports nothing);
  - `pub use module::*` globs — resolved one level by reading the target
    module's own `pub` items when the module is a file in the same crate;
    a glob that cannot be resolved that way is reported as unresolved, never
    silently ignored;
  - `pub mod name`, `pub struct/enum/union/trait/fn/type/const/static name`;
  - `#[macro_export] macro_rules!` macros, from any file in the crate, since
    those live at the crate root wherever they are written;
  - for the `a2a-protocol-sdk` facade, additionally the named re-exports
    inside its inline `prelude` module, since that list is the facade's
    reason to exist.

Items marked `#[doc(hidden)]` and anything narrower than bare `pub`
(`pub(crate)`, `pub(super)`) are not root exports and are skipped.

There is no allowlist. Every root export is on the page or the check fails.

`collect_roots` gathers a `CrateRoot` per crate and `report_roots` prints the
per-crate summary (and the `--explain` listing), returning the names each
crate exports that the page does not list. Every message keeps the
`check_api_reference:` prefix so output is indistinguishable from before the
split.
"""

from __future__ import annotations

import re
import sys
from dataclasses import dataclass, field
from pathlib import Path


# The item a table row is *about* is the backticked token in its first cell.
# Descriptions mention other names in passing (`(`otel` feature)`), and the
# `otel` module is a real root export, so counting every cell would let a
# feature-flag aside stand in for an entry. First cell only.
#
# The leading path-and-identifier is what the row names: `serve(addr, ..)` names
# `serve`, `A2aResult<T>` names `A2aResult`, `agent_executor!` names the macro,
# `a2a_protocol_sdk::types` names `types`.
LEADING_NAME = re.compile(r"^(?:[A-Za-z_]\w*::)*([A-Za-z_]\w*)(?:!|\(|<|$)")


def names_in_first_column(text: str) -> set[str]:
    """Names the page lists, i.e. the first-cell token of each table row."""
    listed: set[str] = set()
    for line in text.splitlines():
        stripped = line.strip()
        if not stripped.startswith("|"):
            continue
        cells = stripped.strip("|").split("|")
        if not cells:
            continue
        for raw in re.findall(r"`([^`]+)`", cells[0]):
            m = LEADING_NAME.match(raw.strip())
            if m:
                listed.add(m.group(1))
    return listed


def strip_comments_and_strings(src: str) -> str:
    """Blank out comments, string and char literals, preserving length.

    A URL inside a `const` would otherwise read as a line comment and a `{`
    inside a doc comment would unbalance the brace count. Lifetimes (`'a`)
    are left alone; only `'x'` / `'\\n'` char literals are blanked.
    """
    out = []
    i, n = 0, len(src)
    while i < n:
        c = src[i]
        nxt = src[i + 1] if i + 1 < n else ""
        if c == "/" and nxt == "/":
            while i < n and src[i] != "\n":
                out.append(" ")
                i += 1
            continue
        if c == "/" and nxt == "*":
            depth = 0
            while i < n:
                if src.startswith("/*", i):
                    depth += 1
                    out.append("  ")
                    i += 2
                elif src.startswith("*/", i):
                    depth -= 1
                    out.append("  ")
                    i += 2
                    if depth == 0:
                        break
                else:
                    out.append("\n" if src[i] == "\n" else " ")
                    i += 1
            continue
        if c == "r" and (nxt == '"' or (nxt == "#" and re.match(r'r#+"', src[i:]))):
            m = re.match(r'r(#*)"', src[i:])
            hashes = m.group(1)
            end = src.find('"' + hashes, i + len(m.group(0)))
            end = n if end < 0 else end + 1 + len(hashes)
            out.append(" " * (end - i))
            i = end
            continue
        if c == '"':
            j = i + 1
            while j < n and src[j] != '"':
                j += 2 if src[j] == "\\" else 1
            j = min(j + 1, n)
            out.append(" " * (j - i))
            i = j
            continue
        if c == "'":
            m = re.match(r"'(?:\\(?:u\{[0-9a-fA-F]+\}|x[0-9a-fA-F]{2}|.)|[^\\'])'", src[i:])
            if m:
                out.append(" " * len(m.group(0)))
                i += len(m.group(0))
                continue
        out.append(c)
        i += 1
    return "".join(out)


@dataclass
class RootItems:
    """What one source text declares at its top level."""

    names: dict[str, str] = field(default_factory=dict)  # name -> how it got there
    globs: list[str] = field(default_factory=list)  # `pub use x::*` prefixes
    mods: set[str] = field(default_factory=set)  # every `mod` declared, pub or not
    inline_mods: dict[str, str] = field(default_factory=dict)  # pub mod x { body }


def top_level_items(cleaned: str) -> list[tuple[str, str]]:
    """Split cleaned source into (header, block_body) at brace depth zero.

    A `use` statement owns its braces (`pub use a::{B, C};`); every other item
    ends at its first `{`, whose block is returned separately, or at `;`.
    """
    items: list[tuple[str, str]] = []
    buf: list[str] = []
    i, n = 0, len(cleaned)

    def flush(body: str = "") -> None:
        text = " ".join("".join(buf).split())
        buf.clear()
        if text:
            items.append((text, body))

    while i < n:
        c = cleaned[i]
        if c == ";":
            flush()
            i += 1
            continue
        if c == "{":
            head = "".join(buf)
            is_use = re.search(r"\buse\s", head) is not None
            # Find the matching brace.
            depth, j = 0, i
            while j < n:
                if cleaned[j] == "{":
                    depth += 1
                elif cleaned[j] == "}":
                    depth -= 1
                    if depth == 0:
                        break
                j += 1
            if is_use:
                buf.append(cleaned[i : j + 1])
                i = j + 1
                continue
            body = cleaned[i + 1 : j]
            flush(body)
            i = j + 1
            continue
        buf.append(c)
        i += 1
    flush()
    return items


def parse_use_tree(tree: str, prefix: list[str], out: RootItems, origin: str) -> None:
    """Walk `a::b::{C, D as E, f::*}` and record every leaf name."""
    tree = tree.strip()
    if tree.startswith("{") and tree.endswith("}"):
        inner = tree[1:-1]
        depth, start = 0, 0
        parts: list[str] = []
        for k, ch in enumerate(inner):
            if ch == "{":
                depth += 1
            elif ch == "}":
                depth -= 1
            elif ch == "," and depth == 0:
                parts.append(inner[start:k])
                start = k + 1
        parts.append(inner[start:])
        for part in parts:
            if part.strip():
                parse_use_tree(part, prefix, out, origin)
        return
    # `path::rest` where rest may itself be a group or `*`.
    segs: list[str] = []
    rest = tree
    while True:
        m = re.match(r"\s*([A-Za-z_]\w*|\*|\{)", rest)
        if not m:
            return
        tok = m.group(1)
        if tok == "{":
            parse_use_tree(rest.strip(), prefix + segs, out, origin)
            return
        if tok == "*":
            out.globs.append("::".join(prefix + segs))
            return
        after = rest[m.end() :].lstrip()
        if after.startswith("::"):
            segs.append(tok)
            rest = after[2:]
            continue
        # Leaf: `Name`, `Name as Alias`, `self`, `self as Alias`.
        alias_m = re.match(r"as\s+([A-Za-z_]\w*)", after)
        name = alias_m.group(1) if alias_m else tok
        if name == "_":
            return
        if tok == "self" and not alias_m:
            if not (prefix + segs):
                return
            name = (prefix + segs)[-1]
        out.names[name] = origin
        return


ITEM_KW = re.compile(
    r"^pub\s+(?:(?:unsafe|async|const|extern\s+\S+)\s+)*"
    r"(struct|enum|union|trait|type|const|static|fn|mod)\s+([A-Za-z_]\w*)"
)


def parse_root_items(cleaned: str, origin: str, macros_too: bool) -> RootItems:
    """Public items declared at the top level of one cleaned source text."""
    out = RootItems()
    for head, body in top_level_items(cleaned):
        attrs = re.findall(r"#\s*!?\s*\[([^\]]*(?:\([^\]]*\))?[^\]]*)\]", head)
        stmt = re.sub(r"#\s*!?\s*\[[^\]]*\]", " ", head)
        stmt = " ".join(stmt.split())
        hidden = any("doc(hidden)" in a.replace(" ", "") for a in attrs)
        exported_macro = any(a.strip() == "macro_export" for a in attrs)

        m = re.match(r"^(?:pub\s+)?mod\s+([A-Za-z_]\w*)$", stmt)
        if m:
            out.mods.add(m.group(1))
            if stmt.startswith("pub ") and body:
                out.inline_mods[m.group(1)] = body

        if exported_macro and macros_too:
            m = re.match(r"^macro_rules!\s*([A-Za-z_]\w*)", stmt)
            if m and not hidden:
                out.names[m.group(1)] = f"{origin}: #[macro_export] macro_rules!"
            continue

        if hidden or not stmt.startswith("pub "):
            continue
        if stmt.startswith("pub("):  # pub(crate), pub(super), pub(in ...)
            continue

        m = re.match(r"^pub\s+use\s+(.*)$", stmt)
        if m:
            parse_use_tree(m.group(1), [], out, f"{origin}: pub use")
            continue
        m = ITEM_KW.match(stmt)
        if m:
            kind, name = m.groups()
            out.names[name] = f"{origin}: pub {kind}"
    return out


def module_file(crate_src: Path, segments: list[str]) -> Path | None:
    """`a::b` -> src/a/b.rs or src/a/b/mod.rs, if either exists."""
    if not segments:
        return None
    base = crate_src.joinpath(*segments)
    for candidate in (base.with_suffix(".rs"), base / "mod.rs"):
        if candidate.is_file():
            return candidate
    return None


def read_text(path: Path) -> str:
    try:
        return path.read_text(encoding="utf-8")
    except OSError as e:
        print(f"check_api_reference: cannot read {path}: {e}", file=sys.stderr)
        raise SystemExit(2) from e


@dataclass
class CrateRoot:
    crate: str
    names: dict[str, str]
    unresolved_globs: list[str]


def crate_root_exports(crate_dir: Path) -> CrateRoot:
    """Every name reachable as `crate_name::NAME`, and any glob left unresolved."""
    src = crate_dir / "src"
    lib = src / "lib.rs"
    root = parse_root_items(strip_comments_and_strings(read_text(lib)), "lib.rs", macros_too=True)

    # `#[macro_export]` puts a macro at the crate root wherever it is written.
    for path in sorted(src.rglob("*.rs")):
        if path == lib:
            continue
        rel = path.relative_to(src).as_posix()
        found = parse_root_items(strip_comments_and_strings(read_text(path)), rel, macros_too=True)
        for name, how in found.names.items():
            if "macro_export" in how:
                root.names[name] = how

    # The sdk facade's inline `prelude` is the one module-depth list this
    # check reads: it is the facade's stated purpose.
    if crate_dir.name == "a2a-protocol-sdk" and "prelude" in root.inline_mods:
        prelude = parse_root_items(root.inline_mods["prelude"], "lib.rs prelude", macros_too=False)
        for name, how in prelude.names.items():
            root.names[name] = how
        root.globs.extend(f"prelude::{g}" for g in prelude.globs)

    # Globs: one level, same crate only.
    unresolved: list[str] = []
    for glob in root.globs:
        segs = [s for s in glob.split("::") if s not in ("crate", "self")]
        target = module_file(src, segs) if segs and segs[0] in root.mods else None
        if target is None:
            unresolved.append(f"{glob}::*")
            continue
        rel = target.relative_to(src).as_posix()
        found = parse_root_items(strip_comments_and_strings(read_text(target)), rel, macros_too=False)
        for name, how in found.names.items():
            root.names.setdefault(name, f"lib.rs: pub use {glob}::* -> {how}")
        unresolved.extend(f"{glob}::{g}::* (nested glob, not followed)" for g in found.globs)

    return CrateRoot(crate_dir.name, dict(sorted(root.names.items())), unresolved)


# ── Per-crate collection and reporting ───────────────────────────────────────


def collect_roots(crates: Path, crate_dirs: tuple[str, ...]) -> list[CrateRoot]:
    """One `CrateRoot` per crate, in the order given; exit 2 if a lib.rs is missing."""
    roots: list[CrateRoot] = []
    for crate in crate_dirs:
        crate_dir = crates / crate
        if not (crate_dir / "src" / "lib.rs").is_file():
            print(f"check_api_reference: {crate_dir}/src/lib.rs not found", file=sys.stderr)
            raise SystemExit(2)
        roots.append(crate_root_exports(crate_dir))
    return roots


def report_roots(
    roots: list[CrateRoot], listed: set[str], explain: bool
) -> dict[str, list[str]]:
    """Print the per-crate summary; return crate -> root names absent from the page."""
    unlisted: dict[str, list[str]] = {}
    for root in roots:
        absent = [n for n in root.names if n not in listed]
        on_page = len(root.names) - len(absent)
        print(
            f"check_api_reference: {root.crate}: {len(root.names)} root name(s), "
            f"{on_page} on the page, {len(absent)} missing, "
            f"{len(root.unresolved_globs)} unresolved glob(s)"
        )
        if absent:
            unlisted[root.crate] = absent
        if root.unresolved_globs and not explain:
            for g in root.unresolved_globs:
                print(f"    unresolved glob: {g}")

    if explain:
        for root in roots:
            print(f"\n== {root.crate}: {len(root.names)} root name(s)")
            for name, how in root.names.items():
                mark = " " if name in listed else "!"
                print(f"  {mark} {name:<40} {how}")
            for g in root.unresolved_globs:
                print(f"  ? unresolved glob: {g}")
    return unlisted
