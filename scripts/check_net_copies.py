#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
"""Fails when the client's and the server's copies of the outbound-HTTP code drift.

`a2a-protocol-client` and `a2a-protocol-server` each dial out over HTTP, and
share no crate that could hold the code both need: the proxy rules and the
connector that tunnels or forwards by them, and the loader of `SSL_CERT_FILE`
and `SSL_CERT_DIR`. They are written twice:

  * `crates/a2a-protocol-client/src/proxy.rs` and `crates/a2a-protocol-server/src/net/proxy.rs`
  * `crates/a2a-protocol-client/src/tls.rs`   and `crates/a2a-protocol-server/src/net/roots.rs`

Until 2026-10-31 the server's push sender, its JWKS and OIDC client and its
model client had neither, while the client had both (audit N-B-1): the two
crates were fixed two days apart, because a fix in one did not reach the other.
A copy whose functions differ is that fault waiting to recur, so the functions
below must stay text-identical, whitespace and visibility keywords ignored.
Functions that differ for a stated reason (the client's `ProxyConnector::new`
takes a TCP connector, the server's `matcher` takes an injected environment)
are not listed.

Usage:
    python3 scripts/check_net_copies.py
    python3 scripts/check_net_copies.py --self-test

Exit codes: 0 the copies agree, 1 a function differs or is missing.
"""

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent

# (client file, server file, functions that must be identical)
PAIRS = [
    (
        "crates/a2a-protocol-client/src/proxy.rs",
        "crates/a2a-protocol-server/src/net/proxy.rs",
        [
            "unbracketed",
            "intercept",
            "unsupported",
            "tunnel",
            "tunnelled_tls",
            "connected",
            "poll_read",
            "poll_write",
            "poll_flush",
            "poll_shutdown",
            "poll_ready",
        ],
    ),
    (
        "crates/a2a-protocol-client/src/tls.rs",
        "crates/a2a-protocol-server/src/net/roots.rs",
        [
            "ring_provider",
            "config_with_roots",
            "default_tls_config",
            "root_cert_store",
            "env_roots",
            "read_env_roots",
        ],
    ),
]

FN = re.compile(
    r"^(?: {0,4})(?:pub(?:\([a-z]+\))? )?(?:async )?fn (\w+)\b", re.MULTILINE
)


def functions(source):
    """Name -> normalised text of each function, first occurrence of a name."""
    found = {}
    for m in FN.finditer(source):
        name = m.group(1)
        if name in found:
            continue
        open_brace = source.index("{", m.end())
        depth, k = 0, open_brace
        while True:
            c = source[k]
            if c == "{":
                depth += 1
            elif c == "}":
                depth -= 1
                if depth == 0:
                    break
            k += 1
        text = source[m.start() : k + 1]
        text = re.sub(r"\bpub\([a-z]+\) ", "", text)
        text = re.sub(r"^(\s*)pub ", r"\1", text, flags=re.MULTILINE)
        found[name] = re.sub(r"\s+", " ", text).strip()
    return found


def check(client_src, server_src, names, label):
    problems = []
    client, server = functions(client_src), functions(server_src)
    for name in names:
        if name not in client or name not in server:
            where = "client" if name not in client else "server"
            problems.append(f"{label}: `{name}` is missing from the {where} copy")
        elif client[name] != server[name]:
            problems.append(
                f"{label}: `{name}` differs between the client's and the server's copy"
            )
    return problems


def run():
    problems = []
    for client_path, server_path, names in PAIRS:
        problems += check(
            (ROOT / client_path).read_text(encoding="utf-8"),
            (ROOT / server_path).read_text(encoding="utf-8"),
            names,
            f"{client_path} / {server_path}",
        )
    for p in problems:
        print(f"check_net_copies: {p}", file=sys.stderr)
    if problems:
        print(
            "check_net_copies: change both copies, or take the function off the "
            "list in scripts/check_net_copies.py with the reason it differs",
            file=sys.stderr,
        )
        return 1
    total = sum(len(names) for _, _, names in PAIRS)
    print(f"check_net_copies: {total} functions identical across {len(PAIRS)} file pairs")
    return 0


def self_test():
    a = "pub fn f(x: u8) -> u8 {\n    x + 1\n}\n"
    same = "pub(crate) fn f(x: u8)  -> u8 {\n  x + 1 }\n"
    other = "pub fn f(x: u8) -> u8 {\n    x + 2\n}\n"
    assert check(a, same, ["f"], "t") == [], "visibility and whitespace are ignored"
    assert len(check(a, other, ["f"], "t")) == 1, "a changed body is reported"
    assert "missing from the server" in check(a, "fn g() {}\n", ["f"], "t")[0]
    assert "missing from the client" in check("fn g() {}\n", a, ["f"], "t")[0]
    nested = "fn outer() {\n    if true { let _ = 1; }\n}\nfn f() {}\n"
    assert functions(nested)["f"] == "fn f() {}", "braces nest"
    print("check_net_copies: self-test passed")
    return 0


if __name__ == "__main__":
    sys.exit(self_test() if "--self-test" in sys.argv else run())
