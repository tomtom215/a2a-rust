# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
"""Site discovery: which files are read, and every place a bound is applied.

Over every non-test `.rs` file under `crates/*/src` and `bindings/*/src`
(`sources`), with comments, strings and `#[cfg(test)]` items blanked (see
`scrub`), it finds every bound:

    tokio::time::timeout(<bound>, <future>)
    tokio::time::timeout_at(<deadline>, <future>)
    <sender>.send_timeout(<value>, <bound>)
    let <name> = tokio::time::Instant::now() + <bound>;
    <helper>(.., <bound>, ..)   — a same-crate fn whose `Duration` parameter
                                  is what it hands to one of the above

Each `<bound>` is resolved by `resolve`. `tokio::time::sleep` is recorded as
a site too, but only so `--explain` can list it as a timer: keep-alives and
backoffs are not budgets, and no pairing reads a sleep.
"""

from __future__ import annotations

import pathlib
import re

from . import REPO
from .entities import Fn, Site, SourceFile
from .scrub import split_args


def sources() -> list[pathlib.Path]:
    out = []
    for pattern in ("crates/*/src/**/*.rs", "bindings/*/src/**/*.rs"):
        for p in REPO.glob(pattern):
            rel = p.relative_to(REPO).as_posix()
            if "/tests/" in rel or rel.endswith("tests.rs") or rel.endswith("_tests.rs"):
                continue
            out.append(p)
    return sorted(out)


class DiscoverMixin:
    """The site-discovery half of `model.Model`; expects `self.files`,
    `self.by_crate`, `self.sites`, `self.helpers` and the `resolve` methods."""

    files: list[SourceFile]
    by_crate: dict[str, list[SourceFile]]
    sites: list[Site]
    helpers: dict[str, tuple[SourceFile, Fn, int]]  # name -> (file, fn, arg index)

    SITE = re.compile(r"(?<![\w:])(?:tokio::)?time::(timeout_at|timeout|sleep)\s*\(|\.(send_timeout)\s*\(")
    DEADLINE = re.compile(r"\blet\s+(?:mut\s+)?([A-Za-z_]\w*)\s*(?::[^=]+?)?=\s*((?:tokio::time::|std::time::)?Instant::now\(\)\s*\+\s*[^;]+);")

    def discover(self) -> None:
        for sf in self.files:
            for m in self.SITE.finditer(sf.code):
                kind = m.group(1) or m.group(2)
                popen = m.end() - 1
                spans, pclose = split_args(sf.code, popen)
                if not spans:
                    continue
                if kind == "send_timeout":
                    if len(spans) < 2:
                        continue
                    a, b = spans[1]
                    fut = None
                else:
                    a, b = spans[0]
                    fut = (spans[1][0], pclose) if len(spans) > 1 else None
                fn = sf.fn_at(m.start())
                expr = sf.code[a:b]
                bound = self.resolve(expr, sf, fn, m.start())
                self.sites.append(Site(sf.rel, sf.line_of(m.start()), m.start(), kind, self.strip(expr), bound, fn, fut))
            for m in self.DEADLINE.finditer(sf.code):
                fn = sf.fn_at(m.start())
                bound = self.resolve(m.group(2), sf, fn, m.start())
                self.sites.append(Site(sf.rel, sf.line_of(m.start()), m.start(), "deadline", m.group(1), bound, fn, note=m.group(1)))
        self._discover_helpers()

    def _discover_helpers(self) -> None:
        """A fn whose `Duration` parameter is what it hands to a timeout site
        is a bound by delegation; each call to it is a site in the caller."""
        # To a fixpoint: a fn that hands its parameter to a helper is a helper too
        # (`validate_webhook_url_with_dns` -> `validate_webhook_url_with_resolver`).
        while True:
            new: dict[str, tuple[SourceFile, Fn, int]] = {}
            for sf in self.files:
                for fn in sf.fns:
                    if fn.name in self.helpers:
                        continue
                    for i, p in enumerate(fn.params):
                        if "Duration" not in fn.param_types.get(p, "") or "Option" in fn.param_types.get(p, ""):
                            continue
                        if any(s.rel == sf.rel and s.fn is fn and s.kind != "sleep" and s.bound.kind == "param" and s.bound.name == p for s in self.sites):
                            new[fn.name] = (sf, fn, i)
            if not new:
                break
            self.helpers.update(new)
            for name, (hf, hfn, idx) in new.items():
                pat = re.compile(rf"(?<![\w])(?:[\w]+::)*{re.escape(name)}\s*\(")
                for sf in self.by_crate[hf.crate]:
                    for m in pat.finditer(sf.code):
                        if sf.code[max(0, m.start() - 3) : m.start()].strip().endswith("fn"):
                            continue
                        spans, _ = split_args(sf.code, m.end() - 1)
                        if len(spans) <= idx:
                            continue
                        a, b = spans[idx]
                        fn = sf.fn_at(m.start())
                        if fn is hfn:
                            continue
                        expr = sf.code[a:b]
                        bound = self.resolve(expr, sf, fn, m.start())
                        self.sites.append(Site(sf.rel, sf.line_of(m.start()), m.start(), f"via {name}", self.strip(expr), bound, fn))
        self.sites.sort(key=lambda s: (s.rel, s.offset))

    def bound_sites(self) -> list[Site]:
        return [s for s in self.sites if s.kind != "sleep" and s.kind != "deadline"]
