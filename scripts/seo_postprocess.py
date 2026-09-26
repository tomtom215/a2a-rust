#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
"""Write each built book page's own SEO metadata into its HTML.

`book/theme/head.hbs` can only emit values common to every page: mdBook's
Handlebars exposes no string helpers, so a template cannot turn a page's
path into its URL. Until 2026-09-25 a script in the page rewrote the
canonical link, og:url, the share titles and the description after load.
That left the served HTML claiming, on every page, that its canonical URL
was the site root, and carrying two identical site-wide descriptions (one
from mdBook's `book.toml`, one from `head.hbs`). Google's guidance is not
to change a canonical with JavaScript when the served HTML already names a
different one: every chapter read as a duplicate of the home page.

This runs after `mdbook build`, before deploy, and makes the served HTML
right without JavaScript:

* canonical, og:url and twitter:url are the page's own URL. `introduction.html`
  is byte-identical to `index.html` (mdBook copies the first chapter), so
  its canonical is the root;
* og:title and twitter:title are the page's `<title>`;
* one description per page, from its first paragraph, replacing both
  site-wide ones;
* `print.html` and `404.html` are `noindex` with no canonical (mdBook marks
  `print.html` noindex, and the template used to add `index,follow` beside
  it);
* the JSON-LD `runtimePlatform` names the MSRV from the workspace manifest.

Usage:
    python3 scripts/seo_postprocess.py book/book          # rewrite in place
    python3 scripts/seo_postprocess.py book/book --check  # verify, exit 1 on a fault
"""

import argparse
import html
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
MSRV_PLACEHOLDER = "__A2A_MSRV__"
NOINDEX = {"print.html", "404.html"}
# Pages whose content is another URL's: canonical points there instead.
SAME_AS = {"introduction.html": ""}
DESC_MAX = 155


def site_origin() -> str:
    text = (ROOT / "book" / "book.toml").read_text()
    m = re.search(r'(?m)^site-url\s*=\s*"(https?://[^"/]+)/?"', text)
    if not m:
        sys.exit("error: book.toml has no site-url")
    return m.group(1)


def msrv() -> str:
    text = (ROOT / "Cargo.toml").read_text()
    m = re.search(r'(?m)^rust-version\s*=\s*"([^"]+)"', text)
    if not m:
        sys.exit("error: the workspace Cargo.toml has no rust-version")
    return m.group(1)


def page_url(origin: str, rel: str) -> str:
    rel = SAME_AS.get(rel, rel)
    if rel in ("", "index.html"):
        return origin + "/"
    return f"{origin}/{rel}"


def first_paragraph(page: str) -> str | None:
    main = page.split("<main>", 1)
    if len(main) < 2:
        return None
    for m in re.finditer(r"<p>(.*?)</p>", main[1], re.S):
        text = html.unescape(re.sub(r"<[^>]+>", "", m.group(1)))
        text = re.sub(r"\s+", " ", text).strip()
        if len(text) >= 40:
            if len(text) > DESC_MAX:
                text = re.sub(r"\s+\S*$", "", text[: DESC_MAX - 3]) + "…"
            return text
    return None


def set_attr(page: str, elem_id: str, attr: str, value: str) -> str:
    pat = re.compile(r'(<[^>]*\bid="%s"[^>]*\b%s=")[^"]*(")' % (elem_id, attr))
    page, n = pat.subn(lambda m: m.group(1) + html.escape(value, quote=True) + m.group(2), page)
    if n != 1:
        raise ValueError(f"expected one #{elem_id}, found {n}")
    return page


def process(page: str, rel: str, origin: str, version: str) -> str:
    page = page.replace(MSRV_PLACEHOLDER, version)
    # mdBook's own description (book.toml) carries no id; ours does.
    page = re.sub(r'\s*<meta name="description" content="[^"]*">', "", page)
    if rel in NOINDEX:
        page = re.sub(r'\s*<meta name="(?:robots|googlebot)" content="index[^"]*">', "", page)
        if '<meta name="robots" content="noindex">' not in page:
            page = page.replace("</head>", '    <meta name="robots" content="noindex">\n</head>', 1)
        page = re.sub(r'\s*<link rel="canonical"[^>]*>', "", page)
        return page
    url = page_url(origin, rel)
    page = set_attr(page, "a2a-canonical", "href", url)
    page = set_attr(page, "a2a-og-url", "content", url)
    page = set_attr(page, "a2a-twitter-url", "content", url)
    title = re.search(r"<title>(.*?)</title>", page, re.S)
    if title:
        text = html.unescape(title.group(1)).strip()
        page = set_attr(page, "a2a-og-title", "content", text)
        page = set_attr(page, "a2a-tw-title", "content", text)
    desc = first_paragraph(page)
    if desc:
        for elem_id in ("a2a-desc", "a2a-og-desc", "a2a-tw-desc"):
            page = set_attr(page, elem_id, "content", desc)
    return page


def check(page: str, rel: str, origin: str) -> list[str]:
    faults = []
    if MSRV_PLACEHOLDER in page:
        faults.append("MSRV placeholder left in the page")
    canon = re.findall(r'<link rel="canonical"[^>]*href="([^"]*)"', page)
    descs = re.findall(r'<meta name="description"', page)
    robots = re.findall(r'<meta name="robots" content="([^"]*)"', page)
    if rel in NOINDEX:
        if canon:
            faults.append(f"noindex page declares canonical {canon}")
        if robots != ["noindex"]:
            faults.append(f"robots should be exactly ['noindex'], is {robots}")
        return faults
    want = page_url(origin, rel)
    if canon != [want]:
        faults.append(f"canonical {canon}, want [{want!r}]")
    if len(descs) != 1:
        faults.append(f"{len(descs)} meta descriptions, want 1")
    for elem_id in ("a2a-og-url", "a2a-twitter-url"):
        m = re.search(r'id="%s"[^>]*content="([^"]*)"' % elem_id, page)
        if not m or m.group(1) != want:
            faults.append(f"#{elem_id} is {m.group(1) if m else None!r}, want {want!r}")
    if len(robots) != 1 or not robots[0].startswith("index"):
        faults.append(f"robots {robots}, want one index,... tag")
    return faults


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n", 1)[0])
    ap.add_argument("site", type=Path, help="mdbook build output, e.g. book/book")
    ap.add_argument("--check", action="store_true", help="verify only; exit 1 on a fault")
    args = ap.parse_args()
    origin, version = site_origin(), msrv()
    pages = sorted(
        p for p in args.site.rglob("*.html")
        # A marker every templated page keeps; the canonical link is removed
        # from noindex pages, so it cannot select them for --check.
        if 'id="a2a-og-url"' in p.read_text(encoding="utf-8", errors="replace")
    )
    if not pages:
        print(f"error: no book pages under {args.site}", file=sys.stderr)
        return 1
    faults = 0
    for p in pages:
        rel = p.relative_to(args.site).as_posix()
        text = p.read_text(encoding="utf-8")
        if args.check:
            for f in check(text, rel, origin):
                print(f"::error file={rel}::{f}")
                faults += 1
        else:
            try:
                p.write_text(process(text, rel, origin, version), encoding="utf-8")
            except ValueError as e:
                print(f"::error file={rel}::{e}")
                faults += 1
    verb = "checked" if args.check else "wrote"
    print(f"seo_postprocess: {verb} {len(pages)} pages, {faults} fault(s)")
    return 1 if faults else 0


if __name__ == "__main__":
    sys.exit(main())
