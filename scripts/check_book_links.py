#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
"""Every link inside the built book reaches a page, and every #fragment an
element on it.

mdbook 0.4 does not check links: a page split or a heading renamed leaves
links to the old place silently dead. This reads the built HTML (run
`mdbook build book` first), so it checks what readers follow, including
the ids mdbook derives from headings.

Links into the rustdoc tree (`api/`) and the `dev/` line are not checked:
the docs workflow adds them after the build. External links are not checked.

    mdbook build book && python3 scripts/check_book_links.py book/book
"""

from __future__ import annotations

import sys
from html.parser import HTMLParser
from pathlib import Path
from urllib.parse import unquote, urlsplit

# Trees added to the site after `mdbook build`: the rustdoc output, and the
# development line's copy that build_docs_site.py publishes under dev/.
ADDED_LATER = ("api/", "dev/")


class Page(HTMLParser):
    def __init__(self) -> None:
        super().__init__()
        self.ids: set[str] = set()
        self.links: list[tuple[str, int]] = []

    def handle_starttag(self, tag: str, attrs: list[tuple[str, str | None]]) -> None:
        values = dict(attrs)
        for key in ("id", "name"):
            if values.get(key):
                self.ids.add(values[key])
        if tag == "a" and values.get("href"):
            self.links.append((values["href"], self.getpos()[0]))


def parse(path: Path) -> Page:
    page = Page()
    page.feed(path.read_text(encoding="utf-8", errors="replace"))
    return page


def main() -> int:
    root = Path(sys.argv[1] if len(sys.argv) > 1 else "book/book").resolve()
    if not (root / "index.html").is_file():
        print(f"check_book_links: no built book at {root}; run `mdbook build book`")
        return 2
    pages = {p: parse(p) for p in root.rglob("*.html")}
    broken: list[str] = []
    for path, page in sorted(pages.items()):
        rel = path.relative_to(root)
        # The 404 page and print.html repeat every page's links with other
        # bases; the pages themselves are checked.
        if rel.name in ("404.html", "print.html") or rel.parts[0] == "api":
            continue
        for href, line in page.links:
            parts = urlsplit(href)
            if parts.scheme or parts.netloc or href.startswith(("mailto:", "javascript:")):
                continue
            if not parts.path:
                target = path
            elif parts.path.startswith("/"):
                target = (root / unquote(parts.path).lstrip("/")).resolve()
            else:
                target = (path.parent / unquote(parts.path)).resolve()
            try:
                inside = target.relative_to(root).as_posix()
            except ValueError:
                broken.append(f"{rel}:{line}: {href} leaves the book")
                continue
            if any(inside == t.rstrip("/") or inside.startswith(t) for t in ADDED_LATER):
                continue
            if target.is_dir():
                target = target / "index.html"
            if not target.exists():
                broken.append(f"{rel}:{line}: {href} -> no {inside}")
                continue
            fragment = unquote(parts.fragment)
            if fragment and target.suffix == ".html":
                ids = pages[target].ids if target in pages else parse(target).ids
                if fragment not in ids:
                    broken.append(f"{rel}:{line}: {href} -> no #{fragment} on {inside}")
    for b in broken:
        print(b)
    print(f"check_book_links: {len(pages)} pages, {len(broken)} broken links")
    return 1 if broken else 0


if __name__ == "__main__":
    sys.exit(main())
