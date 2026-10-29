#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
"""Every `TaskStore` that writes a delta in place also says so to a wrapper.

`TaskStore::try_save_artifact_delta` and `try_save_status_delta` are how a
wrapper that transforms parts (`EncryptedTaskStore`, which seals each one)
learns that a delta was applied in place, so that it transforms only the
parts the write stores. Their defaults return `false`, which is always
correct and costs exactly what release audit F7 was: an implementation that
overrides `save_artifact_delta` but not `try_save_artifact_delta` silently
sends every encrypted artifact event down the whole-task path, sealing every
part of the task each time. CONTRIBUTING.md, "A default is not free", asks
for a gate where one is expressible; this is it.

For each `impl ... TaskStore for ...` block in the repository's Rust sources,
an override of `save_artifact_delta` requires one of
`try_save_artifact_delta`, and likewise for `save_status_delta`.

The same reasoning covers `turn_claims` (`store::claim`): its default, `None`,
means no cross-replica admission and no fence, so a SQL store several
replicas share, or a wrapper around one, that does not override it lets two
replicas run one task at once. A block whose `database` names PostgreSQL or
SQLite, or forwards an inner store's, must override `turn_claims` too.
"""

import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
PAIRS = [
    ("save_artifact_delta", "try_save_artifact_delta"),
    ("save_status_delta", "try_save_status_delta"),
]
# A `database` that names a database replicas can share, or forwards one.
SHARED_DATABASE = re.compile(
    r"StoreDatabase::(?:postgres|sqlite)\(|\.inner\.database\(\)"
)
IMPL = re.compile(r"\bimpl(?:<[^>]*>)?\s+(?:[\w:]+::)?TaskStore\s+for\s+([\w:<>]+)")


def blocks(text):
    """Yields (name, body) for each `impl TaskStore for X { ... }`."""
    for m in IMPL.finditer(text):
        start = text.find("{", m.end())
        if start < 0:
            continue
        depth = 0
        for i in range(start, len(text)):
            c = text[i]
            if c == "{":
                depth += 1
            elif c == "}":
                depth -= 1
                if depth == 0:
                    yield m.group(1), text[start : i + 1], text.count("\n", 0, m.start()) + 1
                    break


def main():
    failures = []
    checked = 0
    for path in sorted(ROOT.rglob("*.rs")):
        rel = path.relative_to(ROOT)
        if rel.parts[0] == "target" or "target" in rel.parts:
            continue
        text = path.read_text(encoding="utf-8")
        for name, body, line in blocks(text):
            checked += 1
            for write, try_write in PAIRS:
                has_write = re.search(rf"\bfn\s+{write}\s*<", body)
                has_try = re.search(rf"\bfn\s+{try_write}\s*<", body)
                if has_write and not has_try:
                    failures.append(f"{rel}:{line}: `{name}` overrides `{write}` but not `{try_write}`")
            if SHARED_DATABASE.search(body) and not re.search(r"\bfn\s+turn_claims\s*\(", body):
                failures.append(
                    f"{rel}:{line}: `{name}` reports a shared database but does not override `turn_claims`"
                )
    for f in failures:
        print(f"FAIL: {f}")
    print(f"check_store_delta_forwarding: {checked} TaskStore impl(s) checked, {len(failures)} missing")
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
