#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
#
# Screenshot the three provisioned Grafana dashboards with headless Chrome (device scale
# factor 1) so a person can look at whether the panels have data. For the walker's own
# checking: the PNGs go to OUT_DIR and are not committed.
#
#   06-screenshots.sh OUT_DIR [CHROME]
#
# Needs: the compose stack up (Grafana on 3001, anonymous access), traffic already sent
# (06-observability.sh); google-chrome or a Playwright Chromium.
set -uo pipefail
OUT=${1:?out dir}; mkdir -p "$OUT"
CHROME=${2:-$(command -v google-chrome || ls /opt/pw-browsers/chromium-*/chrome-linux64/chrome | head -1)}
export NO_PROXY='*' no_proxy='*'
for d in a2a-server a2a-client a2a-genai; do
  "$CHROME" --headless=new --no-sandbox --disable-gpu --hide-scrollbars --force-device-scale-factor=1 \
    --window-size=1600,5200 --virtual-time-budget=40000 \
    --screenshot="$OUT/$d.png" "http://localhost:3001/d/$d?orgId=1&kiosk=1&from=now-15m&to=now" >/dev/null 2>"$OUT/$d.err"
  ls -la "$OUT/$d.png" 2>&1 | awk '{print $5, $9}'
done
