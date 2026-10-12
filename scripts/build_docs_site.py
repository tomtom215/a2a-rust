#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
"""Assemble the versioned documentation site, and check it.

The site at the book's `site-url` used to be the book built from `main`, so a
reader on the latest release was shown the next release's unreleased, and
sometimes breaking, changes. The layout this builds:

  /            the latest release's book, built from its tag: the only copy
               search engines index, with that release's canonical URLs
  /vX.Y/       every release line's book, at its latest patch release
  /dev/        the book built from main, with rustdoc for main at /dev/api/
  /api/        a redirect to docs.rs at the latest release (old deep links
               to /api/<crate>/... are mapped there by the 404 page)

Every page except the root release's is `noindex`, without a canonical link,
so a search lands on the version a reader most likely uses. Every mdBook page
gets the version selector (`book/versioning/`), which reads `versions.json`.

Release tags are `vMAJOR.MINOR.PATCH` with no suffix; a line's latest patch
represents it. Each tag's book is built from the tag's own `book/` with the
mdBook binary this runs, which is the one `docs.yml` pins; every tag from
v0.2.0 to v0.14.1 built with mdBook 0.4.40 when this was written.

Usage:
    scripts/build_docs_site.py build --dev book/book --out site [--mdbook PATH]
    scripts/build_docs_site.py check site
"""

import argparse
import html
import io
import json
import re
import shutil
import subprocess
import sys
import tarfile
import tempfile
from dataclasses import dataclass
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
ASSETS = ROOT / "book" / "versioning"
ASSET_NAMES = ("version-switcher.js", "version-switcher.css")
TAG = re.compile(r"^v(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$")
MSRV_PLACEHOLDER = "__A2A_MSRV__"
NOINDEX = '<meta name="robots" content="noindex">'
DOCS_RS = "https://docs.rs"
API_CRATES = ("a2a_protocol_types", "a2a_protocol_client", "a2a_protocol_server", "a2a_protocol_sdk")
# Root-only files: crawlers read them at the host root and nowhere else.
ROOT_ONLY = ("robots.txt", "sitemap.xml")
API_404_MARKER = "a2a-api-redirect"
# Identifiers this project mints on its own domain, each with the chapter
# that specifies it. An identifier that is a URL should lead to its
# specification; all three answered 404 until 2026-10-12.
PERMALINKS = {
    "extensions/idempotency/v1": "client/idempotency.html",
    "extensions/failure/v1": "client/failure-classes.html",
    "bindings/websocket/v1": "bindings/websocket.html",
}


@dataclass(frozen=True)
class Line:
    """One release line: `label` is MAJOR.MINOR, `release` its latest patch."""

    label: str
    release: str
    tag: str


def release_lines(tags: list[str]) -> list[Line]:
    """Each MAJOR.MINOR's latest patch release, newest line first."""
    best: dict[tuple[int, int], int] = {}
    for tag in tags:
        m = TAG.match(tag)
        if m:
            major, minor, patch = (int(g) for g in m.groups())
            best[(major, minor)] = max(patch, best.get((major, minor), -1))
    return [
        Line(f"{a}.{b}", f"{a}.{b}.{p}", f"v{a}.{b}.{p}")
        for (a, b), p in sorted(best.items(), reverse=True)
    ]


def git(*args: str) -> bytes:
    return subprocess.run(["git", *args], cwd=ROOT, check=True, capture_output=True).stdout


def export(tag: str, dest: Path, paths: list[str]) -> None:
    """Extracts `paths` as they are at `tag` into `dest`."""
    archive = git("archive", "--format=tar", tag, "--", *paths)
    with tarfile.open(fileobj=io.BytesIO(archive)) as tar:
        tar.extractall(dest, filter="data")


def has_path(tag: str, path: str) -> bool:
    return subprocess.run(["git", "cat-file", "-e", f"{tag}:{path}"], cwd=ROOT).returncode == 0


def msrv(cargo_toml: Path) -> str | None:
    m = re.search(r'(?m)^rust-version\s*=\s*"([^"]+)"', cargo_toml.read_text(encoding="utf-8"))
    return m.group(1) if m else None


def build_line(line: Line, work: Path, mdbook: str, latest: bool) -> Path:
    """Builds one release's book from its tag; returns the built site."""
    src = work / line.tag
    paths = ["book", "Cargo.toml"]
    if latest:
        # The root carries real SEO metadata, written by the release's own
        # script against the release's own template.
        if not has_path(line.tag, "scripts/seo_postprocess.py"):
            sys.exit(f"error: {line.tag} has no scripts/seo_postprocess.py for the root")
        paths.append("scripts/seo_postprocess.py")
    export(line.tag, src, paths)
    subprocess.run([mdbook, "build", "book"], cwd=src, check=True, capture_output=True)
    built = src / "book" / "book"
    static = src / "book" / "static"
    if static.is_dir():
        shutil.copytree(static, built, dirs_exist_ok=True)
    if latest:
        seo = src / "scripts" / "seo_postprocess.py"
        for extra in ([], ["--check"]):
            subprocess.run([sys.executable, str(seo), str(built), *extra], check=True)
    version = msrv(src / "Cargo.toml")
    if version:
        for page in built.rglob("*.html"):
            text = page.read_text(encoding="utf-8")
            if MSRV_PLACEHOLDER in text:
                page.write_text(text.replace(MSRV_PLACEHOLDER, version), encoding="utf-8")
    return built


def make_noindex(page: str) -> str:
    page = re.sub(r'\s*<link rel="canonical"[^>]*>', "", page)
    page = re.sub(r'\s*<meta name="(?:robots|googlebot)" content="[^"]*">', "", page)
    return page.replace("</head>", f"    {NOINDEX}\n</head>", 1)


def switcher_tags(label: str, root: str, page: str) -> str:
    def q(value: str) -> str:
        return html.escape(value, quote=True)

    return (
        f'    <link rel="stylesheet" href="{q(root)}version-switcher.css">\n'
        f'    <script defer src="{q(root)}version-switcher.js" data-a2a-version="{q(label)}" '
        f'data-a2a-root="{q(root)}" data-a2a-page="{q(page)}"></script>\n'
    )


def redirect_page(url: str) -> str:
    u = html.escape(url, quote=True)
    return (
        '<!doctype html>\n<meta charset="utf-8">\n<title>a2a-rust API documentation</title>\n'
        f'{NOINDEX}\n<meta http-equiv="refresh" content="0; url={u}">\n'
        f'<p>Redirecting to <a href="{u}">{u}</a>.</p>\n'
    )


def api_404_script(release: str) -> str:
    crates = "|".join(c.removeprefix("a2a_protocol_") for c in API_CRATES)
    return (
        f'<script id="{API_404_MARKER}">(function () {{\n'
        f'  var m = location.pathname.match(/^\\/api\\/(a2a_protocol_(?:{crates}))\\/(.*)$/);\n'
        f'  if (m) location.replace("{DOCS_RS}/" + m[1].replace(/_/g, "-") + "/{release}/" +\n'
        '    m[1] + "/" + m[2] + location.hash);\n'
        "})();</script>\n"
    )


def sdk_docs(release: str) -> str:
    return f"{DOCS_RS}/a2a-protocol-sdk/{release}/a2a_protocol_sdk/"


def assemble(dev: Path, builds: list[tuple[Line, Path]], out: Path) -> None:
    """Lays `builds` (newest first) and `dev` out as the site at `out`."""
    if out.exists():
        shutil.rmtree(out)
    latest = builds[0][0]
    shutil.copytree(builds[0][1], out)
    roles: dict[str, str] = {"dev": "dev"}
    for line, built in builds:
        shutil.copytree(built, out / f"v{line.label}")
        roles[f"v{line.label}"] = line.label
    shutil.copytree(dev, out / "dev")
    for sub in roles:
        for name in ROOT_ONLY:
            (out / sub / name).unlink(missing_ok=True)

    for page in sorted(out.rglob("*.html")):
        rel = page.relative_to(out)
        top = rel.parts[0] if len(rel.parts) > 1 else ""
        label = roles.get(top, latest.label)
        within = rel.relative_to(top).as_posix() if top in roles else rel.as_posix()
        root = "../" * (len(rel.parts) - 1)
        text = page.read_text(encoding="utf-8")
        if top in roles:
            text = make_noindex(text)
        if 'id="menu-bar"' in text:
            text = text.replace("</head>", switcher_tags(label, root, within) + "</head>", 1)
        page.write_text(text, encoding="utf-8")

    for name in ASSET_NAMES:
        shutil.copyfile(ASSETS / name, out / name)
    (out / "api").mkdir(exist_ok=True)
    (out / "api" / "index.html").write_text(redirect_page(sdk_docs(latest.release)), encoding="utf-8")
    for line, _ in builds:
        api = out / f"v{line.label}" / "api"
        api.mkdir(exist_ok=True)
        (api / "index.html").write_text(redirect_page(sdk_docs(line.release)), encoding="utf-8")
    for uri, chapter in PERMALINKS.items():
        # The latest release's chapter where it has one, else main's.
        target = chapter if (out / chapter).is_file() else f"dev/{chapter}"
        if not (out / target).is_file():
            sys.exit(f"error: {chapter} is in neither the latest release nor dev, for /{uri}")
        (out / uri).mkdir(parents=True, exist_ok=True)
        up = "../" * len(Path(uri).parts)
        (out / uri / "index.html").write_text(redirect_page(up + target), encoding="utf-8")
    not_found = out / "404.html"
    text = not_found.read_text(encoding="utf-8")
    not_found.write_text(text.replace("</head>", api_404_script(latest.release) + "</head>", 1), encoding="utf-8")

    index = {
        "latest": latest.label,
        "versions": [
            {"label": line.label, "release": line.release, "path": "" if i == 0 else f"v{line.label}/"}
            for i, (line, _) in enumerate(builds)
        ],
        "dev": {"label": "dev", "path": "dev/"},
    }
    (out / "versions.json").write_text(json.dumps(index, indent=2) + "\n", encoding="utf-8")


def check(site: Path) -> list[str]:
    """Every invariant the layout promises; returns the faults found."""
    faults: list[str] = []
    try:
        index = json.loads((site / "versions.json").read_text(encoding="utf-8"))
        versions, dev = index["versions"], index["dev"]
    except (OSError, ValueError, KeyError, TypeError) as e:
        return [f"versions.json unreadable: {e}"]
    if not versions or index.get("latest") != versions[0]["label"] or versions[0]["path"] != "":
        faults.append("versions.json: the first version must be the latest, served at the root")
    roles = {f"v{v['label']}": v["label"] for v in versions}
    roles["dev"] = "dev"
    for sub in [*roles, ""]:
        if not (site / sub / "index.html").is_file():
            faults.append(f"/{sub}/index.html is missing" if sub else "/index.html is missing")
    for name in ROOT_ONLY:
        if not (site / name).is_file():
            faults.append(f"/{name} is missing")
        for sub in roles:
            if (site / sub / name).exists():
                faults.append(f"/{sub}/{name} exists; crawlers read it only at the root")
    for name in ASSET_NAMES:
        if not (site / name).is_file():
            faults.append(f"/{name} is missing")
    latest_release = versions[0]["release"] if versions else ""
    for v in versions:
        api = (site / "" if v["path"] == "" else site / f"v{v['label']}") / "api" / "index.html"
        if not api.is_file() or sdk_docs(v["release"]) not in api.read_text(encoding="utf-8"):
            faults.append(f"{api.relative_to(site)} does not redirect to docs.rs at {v['release']}")
    for uri in PERMALINKS:
        page = site / uri / "index.html"
        m = re.search(r'url=([^"]+)"', page.read_text(encoding="utf-8")) if page.is_file() else None
        if not m or not (page.parent / html.unescape(m.group(1))).resolve().is_file():
            faults.append(f"/{uri} does not lead to its chapter")
    try:
        if API_404_MARKER not in (site / "404.html").read_text(encoding="utf-8"):
            faults.append("/404.html does not map /api/ deep links to docs.rs")
    except OSError:
        faults.append("/404.html is missing")

    pages = 0
    for page in sorted(site.rglob("*.html")):
        rel = page.relative_to(site)
        top = rel.parts[0] if len(rel.parts) > 1 else ""
        text = page.read_text(encoding="utf-8")
        canon = re.findall(r'<link rel="canonical"[^>]*>', text)
        robots = re.findall(r'<meta name="robots" content="([^"]*)"', text)
        if top in roles:
            if canon or robots != ["noindex"]:
                faults.append(f"{rel}: a non-root page must be noindex with no canonical")
        elif 'id="menu-bar"' in text and rel.name not in ("print.html", "404.html"):
            if len(canon) != 1 or len(robots) != 1 or not robots[0].startswith("index"):
                faults.append(f"{rel}: a root page needs one canonical and an index robots tag")
        if 'id="menu-bar"' not in text:
            continue
        pages += 1
        want = roles.get(top, versions[0]["label"] if versions else "")
        tags = re.findall(r'<script defer src="([^"]*)version-switcher\.js" data-a2a-version="([^"]*)"', text)
        root = "../" * (len(rel.parts) - 1)
        if tags != [(root, want)]:
            faults.append(f"{rel}: switcher {tags}, want [({root!r}, {want!r})]")
    if pages == 0:
        faults.append("no mdBook pages in the site")
    print(f"build_docs_site: {len(versions)} release line(s), latest {latest_release}; "
          f"{pages} book pages checked, {len(faults)} fault(s)")
    return faults


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n", 1)[0])
    sub = ap.add_subparsers(dest="cmd", required=True)
    b = sub.add_parser("build", help="build every release line and assemble the site")
    b.add_argument("--dev", type=Path, required=True, help="the book built from main, with api/")
    b.add_argument("--out", type=Path, required=True)
    b.add_argument("--mdbook", default="mdbook")
    c = sub.add_parser("check", help="verify an assembled site")
    c.add_argument("site", type=Path)
    args = ap.parse_args()

    if args.cmd == "check":
        faults = check(args.site)
        for f in faults:
            print(f"::error::{f}")
        return 1 if faults else 0

    lines = release_lines(git("tag", "--list", "v*").decode().split())
    if not lines:
        print("error: no release tags; fetch with full history (fetch-depth: 0)", file=sys.stderr)
        return 1
    with tempfile.TemporaryDirectory() as tmp:
        builds = [(line, build_line(line, Path(tmp), args.mdbook, i == 0)) for i, line in enumerate(lines)]
        assemble(args.dev, builds, args.out)
    print(f"build_docs_site: assembled {len(lines)} release line(s) and dev into {args.out}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
