#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
#
# Renders the book's diagrams: each book/diagrams/<name>.mmd (Mermaid) to
# book/src/images/diagrams/<name>.light.svg and <name>.dark.svg. A page shows
# both through the `diagram` figure markup, and custom.css hides the one that
# does not match the reader's mdBook theme: an <img> cannot read the page's
# CSS variables, so one SVG cannot follow the theme.
#
# The SVGs are committed, so neither CI nor a reader needs Node or a browser.
# Re-run after editing a .mmd and commit both outputs:
#
#   scripts/render_book_diagrams.sh            # every diagram
#   scripts/render_book_diagrams.sh name ...   # just these
#
# Needs Node 18+ and a Chromium. MERMAID_CLI pins the renderer; CHROMIUM
# overrides the browser it drives (default: Playwright's under /opt).
set -euo pipefail

cd "$(dirname "$0")/.."
MERMAID_CLI="${MERMAID_CLI:-@mermaid-js/mermaid-cli@11.4.2}"
CHROMIUM="${CHROMIUM:-$(ls -d /opt/pw-browsers/chromium-*/chrome-linux64/chrome 2>/dev/null | head -1)}"
SRC=book/diagrams
OUT=book/src/images/diagrams
mkdir -p "$OUT"

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
printf '{"executablePath":"%s","args":["--no-sandbox"]}\n' "$CHROMIUM" > "$work/puppeteer.json"
# The book's palette (book/theme/custom.css: amber accents, warm neutrals)
# on Mermaid's `base` theme, one set per mdBook theme family. One font
# stack for both, near the book's body face: an SVG in an <img> cannot load
# the page's web fonts.
common='"fontFamily":"Inter, Segoe UI, Helvetica, Arial, sans-serif","fontSize":"15px"'
cat > "$work/light.json" <<JSON
{"theme":"base","themeVariables":{$common,
 "background":"transparent","primaryColor":"#fff7ed","primaryBorderColor":"#D97706",
 "primaryTextColor":"#1c1917","secondaryColor":"#f5f5f4","secondaryBorderColor":"#a8a29e",
 "tertiaryColor":"#faf7f1","tertiaryBorderColor":"#d6d3d1","lineColor":"#57534e",
 "textColor":"#1c1917","noteBkgColor":"#fef3c7","noteBorderColor":"#D97706",
 "actorBkg":"#fff7ed","actorBorder":"#D97706","signalColor":"#44403c","signalTextColor":"#1c1917",
 "clusterBkg":"#faf7f1","clusterBorder":"#d6d3d1","edgeLabelBackground":"#faf7f1"},
 "flowchart":{"htmlLabels":false,"curve":"basis"},"sequence":{"mirrorActors":false}}
JSON
cat > "$work/dark.json" <<JSON
{"theme":"base","themeVariables":{$common,"darkMode":true,
 "background":"transparent","primaryColor":"#1f2630","primaryBorderColor":"#F0A93B",
 "primaryTextColor":"#e6e1cf","secondaryColor":"#232a33","secondaryBorderColor":"#5c6773",
 "tertiaryColor":"#161b22","tertiaryBorderColor":"#3d4651","lineColor":"#aab2bc",
 "textColor":"#e6e1cf","noteBkgColor":"#2d2414","noteBorderColor":"#F0A93B",
 "actorBkg":"#1f2630","actorBorder":"#F0A93B","signalColor":"#c5ccd3","signalTextColor":"#e6e1cf",
 "clusterBkg":"#161b22","clusterBorder":"#3d4651","edgeLabelBackground":"#161b22"},
 "flowchart":{"htmlLabels":false,"curve":"basis"},"sequence":{"mirrorActors":false}}
JSON

if [ "$#" -gt 0 ]; then
    names=("$@")
else
    names=()
    for f in "$SRC"/*.mmd; do names+=("$(basename "$f" .mmd)"); done
fi

for name in "${names[@]}"; do
    for theme in light dark; do
        npx -y "$MERMAID_CLI" --quiet -p "$work/puppeteer.json" -c "$work/$theme.json" \
            -b transparent -i "$SRC/$name.mmd" -o "$OUT/$name.$theme.svg"
    done
    echo "rendered $name"
done
