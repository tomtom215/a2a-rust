#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

"""Converts `ignore`d book code blocks into compiled `no_run` doctests.

A maintenance tool, not a gate. `check_book_code.sh` is the ratchet that stops
the ignored count growing; this is what shrinks it.

Most ignored blocks are not uncompilable — they are fragments that reference a
`url`, a `client` or a `params` the page established in prose. rustdoc's hidden
`# ` lines can supply those without showing them to the reader, so the block
compiles while the page still reads as a fragment.

Blocks naming a type the reader is meant to write themselves (`MyExecutor`,
`LoggingInterceptor`) are left alone: making those compile would mean inventing
a definition the page does not show, which trades an uncompiled block for a
misleading one.

# What this does not do

It does not convert most files, and that is a property of the problem rather
than a missing feature. A blanket preamble was tried across every page in
`.book-ignore-baseline`: it converted nothing, because each page's fragments
lean on a different set of bindings, and one page's preamble is another page's
unused-variable error. Adding bindings until everything compiles does not
converge either — the binding that fixes `client/error-handling.md` broke
`client/task-management.md`, which had already been converted.

So the useful unit of work is one page at a time: run this on a single file,
run the doctests, read the errors, add what that page needs to `BINDINGS`
locally, and keep it only if it compiles. That is what took
`client/task-management.md` from 3 ignored blocks to 0.

Usage:
    convert_book_ignores.py book/src/client/task-management.md

Always re-run `cargo test -p a2a-book-tests --doc` afterwards and revert the
file if it does not compile — conversion is a guess about what a fragment
needs, and the compiler is the only thing that settles it.
"""

from __future__ import annotations

import pathlib
import re
import sys

# Identifiers the book invents in prose. A block naming one cannot compile
# without a definition the page never shows.
UNDEFINED = re.compile(
    r"\b(My[A-Z]\w*|Your[A-Z]\w*|Custom[A-Z]\w*|LoggingInterceptor"
    r"|AuthInterceptor|TracingInterceptor|RedisStore)\b"
)

BINDINGS = [
    '# let url = "http://agent.example.com";',
    "# let message = Message {",
    '#     id: MessageId::new("m1"),',
    "#     role: MessageRole::User,",
    '#     parts: vec![Part::text("hi")],',
    "#     task_id: None,",
    "#     context_id: None,",
    "#     reference_task_ids: None,",
    "#     extensions: None,",
    "#     metadata: None,",
    "# };",
    "# let params = MessageSendParams {",
    "#     tenant: None,",
    "#     message,",
    "#     configuration: None,",
    "#     metadata: None,",
    "# };",
    "# let (params1, params2) = (params.clone(), params.clone());",
    "# let client = ClientBuilder::new(url).build()?;",
    '# let task_id = "task-abc";',
]

IMPORTS = [
    "# use a2a_protocol_sdk::prelude::*;",
    "# use a2a_protocol_types::message::{MessageId, MessageRole};",
    "# use std::sync::Arc;",
    "# use std::time::Duration;",
]

SYNC_OPEN = "# fn doc() -> Result<(), Box<dyn std::error::Error>> {"
ASYNC_OPEN = "# async fn doc() -> Result<(), Box<dyn std::error::Error>> {"
EPILOGUE = "# Ok(())\n# }\n"

SKIP_PREFIXES = ("impl ", "#[", "trait ", "struct ", "enum ", "//!", "mod ", "pub ")


def convert(path: str) -> tuple[int, int]:
    p = pathlib.Path(path)
    src = p.read_text(encoding="utf-8")
    total = len(re.findall(r"```rust[^\n]*ignore", src))
    converted = 0

    def repl(match: re.Match[str]) -> str:
        nonlocal converted
        attrs, body = match.group(1), match.group(2)
        if "ignore" not in attrs:
            return match.group(0)
        stripped = body.lstrip()
        if UNDEFINED.search(body) or "fn main" in body or stripped.startswith(SKIP_PREFIXES):
            return match.group(0)

        opener = ASYNC_OPEN if ".await" in body else SYNC_OPEN
        preamble = "\n".join(IMPORTS + [opener] + BINDINGS) + "\n"
        converted += 1
        new_attrs = attrs.replace("ignore", "no_run")
        return "```rust" + new_attrs + "\n" + preamble + body + EPILOGUE + "```"

    out = re.sub(r"```rust([^\n]*)\n(.*?)```", repl, src, flags=re.S)
    p.write_text(out, encoding="utf-8")
    return converted, total


def main() -> int:
    if len(sys.argv) < 2:
        print(__doc__)
        return 2
    for f in sys.argv[1:]:
        done, total = convert(f)
        print(f"  {f}: converted {done} of {total}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
