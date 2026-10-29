#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
"""Fails when a statement on a PostgreSQL pool runs outside `pg_retry`.

The task stores' pools do not ping a connection before every statement
(`crates/a2a-protocol-server/src/store/pg_retry.rs` says why and what
replaces it): a statement there must run inside `pg_retry::retry` (a second
run is harmless) or `pg_retry::checked` (pinged first). One left out works
until the server ends its connection, then fails where sqlx's ping used to
replace the connection first. Review was the only guard; this is the check.

What it reads: every non-test `.rs` file under `crates/*/src` that runs sqlx
statements and is about PostgreSQL (its path says `postgres` or `pg_`, or it
names `PgPool` or `Postgres`). Each is checked unless [`EXCLUDED`] says why
its statements never run on such a pool. A new PostgreSQL module is checked
by default, so it fails until it wraps its statements or is excluded with a
reason.

What counts as a statement on a pool: `.execute(…)`, `.fetch…(…)` whose
argument is not a transaction or a held connection (`tx`, `conn`), and
`….begin()`. It is wrapped when some call enclosing it is
`pg_retry::retry(…)` or `pg_retry::checked(…)`. Strings and comments are
blanked first, so the SQL's own parentheses do not count; `#[cfg(test)]`
modules are skipped.

Usage
-----
    python3 scripts/check_pg_retry.py             # the gate
    python3 scripts/check_pg_retry.py --self-test # prove the rules, offline

Exit codes: 0 clean, 1 a statement outside `pg_retry`, 2 the tree could not
be read.
"""

from __future__ import annotations

import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent

# Files that run statements against PostgreSQL on pools of their own, opened
# with sqlx's default (a ping before every statement), never on a task
# store's.
EXCLUDED = {
    "crates/a2a-protocol-server/src/audit/postgres.rs":
        "the audit store's pool, `pg_pool`, keeps sqlx's ping",
    "crates/a2a-protocol-server/src/push/postgres_config_store.rs":
        "opens its own pool with sqlx's defaults",
    "crates/a2a-protocol-server/src/push/tenant_postgres_config_store.rs":
        "opens its own pool with sqlx's defaults",
    "crates/a2a-protocol-server/src/push/tenant_postgres_config_store/caller_keys.rs":
        "runs on its parent push store's pool",
    "crates/a2a-protocol-server/src/rate_limit/shared.rs":
        "opens its own pool with sqlx's defaults",
}

STATEMENT = re.compile(
    r"\.(execute|fetch_one|fetch_optional|fetch_all|fetch_many|fetch)\s*\("
    r"|\.begin\s*\(\s*\)"
)
STATEMENTS_IN_FILE = re.compile(r"sqlx::(query|raw_sql)|\.begin\s*\(\s*\)")
ABOUT_POSTGRES = re.compile(r"PgPool|Postgres")
WRAPPER = re.compile(r"pg_retry::(retry|checked)\s*$")
ON_A_CONNECTION = re.compile(r"\b(tx|conn)\b")
TEST_MODULE = re.compile(r"#\[cfg\(test\)\]\s*(pub(\([^)]*\))?\s+)?mod\s+\w+\s*\{")
OPENERS, CLOSERS = "([{", ")]}"


def blank(src: str) -> str:
    """`src` with every comment, string and char literal replaced by spaces
    (newlines kept, so offsets and line numbers still hold)."""
    out = list(src)
    i, n = 0, len(src)

    def wipe(a: int, b: int) -> None:
        for k in range(a, b):
            if out[k] != "\n":
                out[k] = " "

    while i < n:
        c = src[i]
        if src.startswith("//", i):
            j = src.find("\n", i)
            j = n if j < 0 else j
            wipe(i, j)
            i = j
        elif src.startswith("/*", i):
            depth, j = 1, i + 2
            while j < n and depth:
                if src.startswith("/*", j):
                    depth, j = depth + 1, j + 2
                elif src.startswith("*/", j):
                    depth, j = depth - 1, j + 2
                else:
                    j += 1
            wipe(i, j)
            i = j
        elif (m := re.match(r'b?r(#*)"', src[i:])) and (i == 0 or not (src[i - 1].isalnum() or src[i - 1] == "_")):
            end = '"' + m.group(1)
            j = src.find(end, i + m.end())
            j = n if j < 0 else j + len(end)
            wipe(i, j)
            i = j
        elif c == '"':
            j = i + 1
            while j < n and src[j] != '"':
                j += 2 if src[j] == "\\" else 1
            wipe(i, j + 1)
            i = j + 1
        elif c == "'":
            m = re.match(r"'(\\.[^']*|[^\\'])'", src[i:])
            if m:  # a char literal; otherwise a lifetime, left as it is
                wipe(i, i + m.end())
                i += m.end()
            else:
                i += 1
        else:
            i += 1
    return "".join(out)


def test_spans(text: str) -> list[tuple[int, int]]:
    """Byte ranges of `#[cfg(test)] mod … { … }` in blanked `text`."""
    spans = []
    for m in TEST_MODULE.finditer(text):
        depth, j = 1, m.end()
        while j < len(text) and depth:
            depth += {"{": 1, "}": -1}.get(text[j], 0)
            j += 1
        spans.append((m.start(), j))
    return spans


def wrapped(text: str, at: int) -> bool:
    """Whether some call enclosing offset `at` is `pg_retry::retry(` or
    `pg_retry::checked(`."""
    depth = 0
    for k in range(at - 1, -1, -1):
        c = text[k]
        if c in CLOSERS:
            depth += 1
        elif c in OPENERS:
            if depth:
                depth -= 1
            elif c == "(" and WRAPPER.search(text[max(0, k - 120):k].rstrip()):
                return True
    return False


def call_argument(text: str, open_paren: int) -> str:
    depth, j = 0, open_paren
    while j < len(text):
        depth += {"(": 1, ")": -1}.get(text[j], 0)
        if depth == 0:
            return text[open_paren + 1:j]
        j += 1
    return text[open_paren + 1:]


def findings(src: str) -> list[tuple[int, str]]:
    """(line, statement) for each statement on a pool outside `pg_retry`."""
    text = blank(src)
    skip = test_spans(text)
    found = []
    for m in STATEMENT.finditer(text):
        if any(a <= m.start() < b for a, b in skip):
            continue
        if m.group(1) is not None:
            argument = call_argument(text, m.end() - 1)
            if ON_A_CONNECTION.search(argument):
                continue
        if not wrapped(text, m.start()):
            line = text.count("\n", 0, m.start()) + 1
            found.append((line, src[m.start():m.end()].strip() + ("…)" if m.group(1) else "")))
    return found


def in_scope(rel: str, src: str) -> bool:
    if rel.endswith("/tests.rs") or "/tests/" in rel:
        return False
    if not STATEMENTS_IN_FILE.search(blank(src)):
        return False
    return "postgres" in rel or "/pg_" in rel or bool(ABOUT_POSTGRES.search(src))


def main() -> int:
    if "--self-test" in sys.argv[1:]:
        return self_test()
    try:
        files = sorted((ROOT / "crates").glob("*/src/**/*.rs"))
    except OSError as e:
        print(f"check_pg_retry: cannot read the tree: {e}")
        return 2
    bad, checked = [], 0
    for path in files:
        rel = path.relative_to(ROOT).as_posix()
        src = path.read_text(encoding="utf-8")
        if rel in EXCLUDED or not in_scope(rel, src):
            continue
        checked += 1
        bad += [f"{rel}:{line}: {what}" for line, what in findings(src)]
    stale = [r for r in EXCLUDED if not (ROOT / r).is_file()]
    for r in stale:
        bad.append(f"{r}: listed in EXCLUDED but not found; remove it")
    if bad:
        print("Statements on a PostgreSQL pool outside pg_retry::retry or checked")
        print("(see crates/a2a-protocol-server/src/store/pg_retry.rs):\n")
        print("\n".join(bad))
        return 1
    print(f"check_pg_retry: {checked} file(s), every statement on a pool inside pg_retry; "
          f"{len(EXCLUDED)} excluded with a reason")
    return 0


CASES = [
    ("a plain statement on the pool", 'fn f() { sqlx::query("SELECT 1").execute(&self.pool).await; }', 1),
    ("inside retry", 'fn f() { crate::store::pg_retry::retry(|| sqlx::query("x").execute(&self.pool)).await; }', 0),
    ("inside checked", 'fn f() { pg_retry::checked(sqlx::query("x").execute(&pool)).await; }', 0),
    ("a closure with its own statements",
     "fn f() { pg_retry::retry(|| { let mut q = sqlx::query(&s); for v in &vs { q = q.bind(v); }"
     " q.fetch_all(&self.pool) }).await; }", 0),
    ("a retry from elsewhere is not pg_retry's", 'fn f() { backoff::retry(|| q.execute(&self.pool)).await; }', 1),
    ("on a transaction", 'fn f() { sqlx::query("x").execute(&mut *tx).await; }', 0),
    ("on a held connection", 'fn f() { q.fetch_one(&mut conn).await; }', 0),
    ("begin unwrapped", "fn f() { let tx = self.pool.begin().await?; }", 1),
    ("begin wrapped", "fn f() { let tx = super::pg_retry::retry(|| pool.begin()).await?; }", 0),
    ("SQL parentheses do not close the wrapper",
     'fn f() { pg_retry::retry(|| sqlx::query("SELECT count(*) FROM (SELECT 1) AS t)").fetch_one(&self.pool)).await; }', 0),
    ("a raw string with a quote and parentheses",
     'fn f() { pg_retry::retry(|| sqlx::query(r#"SELECT ")" (1)"#).fetch_one(&self.pool)).await; }', 0),
    ("a statement in a string or a comment is none",
     'fn f() { let s = ".execute(&self.pool)"; // .fetch_one(&self.pool)\n}', 0),
    ("a char literal parenthesis",
     "fn f() { let c = '('; sqlx::query(\"x\").execute(&self.pool).await; }", 1),
    ("a lifetime is not a char", "fn f<'a>(x: &'a str) { q.execute(&self.pool).await; }", 1),
    ("a test module is skipped",
     '#[cfg(test)]\nmod tests { fn f() { sqlx::query("x").execute(&pool).await; } }', 0),
]


def self_test() -> int:
    failed = 0
    for name, src, want in CASES:
        got = len(findings(src))
        if got != want:
            failed += 1
            print(f"FAIL {name}: {got} finding(s), want {want}")
    print(f"check_pg_retry self-test: {len(CASES) - failed} of {len(CASES)} passed")
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
