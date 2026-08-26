#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
"""Catches a doc comment that contains a literal `\\n` where a newline was meant.

Why this exists
---------------
On 2026-08-20 five field doc comments in `tenant_config.rs` were committed and
pushed looking like this — one physical line, with the line breaks present as
two-character escapes:

    /// Maximum tasks stored. `None` = use store default.\\n    ///\\n    /// Not
    enforced, and no store consults it.

They were written by a script that used `"\\\\n"` in a Python string where it
meant `"\\n"`. Nothing in 54 CI gates noticed: `cargo fmt` does not reformat
inside a comment, clippy does not read prose, and `cargo doc -D warnings` is
happy because the text is *valid* — it just renders as one run-on paragraph
with `\\n` printed in it.

What it checks
--------------
A doc line (`///` or `//!`) that contains the two characters `\\` `n` followed
by whitespace and another doc marker. That is the signature of a doc comment
that was assembled as one string and never split:

    /// something.\\n    /// more            <- flagged
    /// data lines are joined with `\\n`     <- not flagged

The second form is why the check is not simply "no `\\n` in a doc comment".
This repository has four legitimate ones, all in SSE parsing, where the docs
are *about* newline characters — `parser.rs` explaining that a `\\r` may already
have terminated a line, and `types.rs` describing `data:` values joined by
`\\n`. A check that flagged those would be turned off within a week.

Exit codes: 0 clean, 1 at least one mangled doc comment.
"""

import pathlib
import re
import sys

REPO = pathlib.Path(__file__).resolve().parents[1]

# `\n` followed by whitespace and a doc marker: a line break that was meant to
# start the next doc line and instead became text.
MANGLED = re.compile(r"\\n\s*//[/!]")


def main() -> int:
    paths = sorted(
        list(REPO.glob("crates/*/src/**/*.rs"))
        + list(REPO.glob("bindings/*/src/**/*.rs"))
        + list(REPO.glob("examples/*/src/**/*.rs"))
    )
    if not paths:
        sys.exit(f"check_doc_escapes: no sources under {REPO} — wrong repo root?")

    findings = []
    for path in paths:
        for number, line in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
            stripped = line.lstrip()
            if not (stripped.startswith("///") or stripped.startswith("//!")):
                continue
            if MANGLED.search(line):
                findings.append((path.relative_to(REPO), number, stripped[:78]))

    if findings:
        print("check_doc_escapes: doc comment(s) containing a literal \\n\n")
        for rel, number, text in findings:
            print(f"  {rel}:{number}")
            print(f"      {text}")
        print(
            "\n  A line break was written as the two characters \\ and n. Split the\n"
            "  comment into real lines. If the docs are genuinely *about* the\n"
            "  newline character, keep it away from a following `///`."
        )
        return 1

    print(f"check_doc_escapes: {len(paths)} sources, no mangled doc comments")
    return 0


if __name__ == "__main__":
    sys.exit(main())
