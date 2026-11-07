#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
#
# Installs promtool and/or amtool into DIR from the Prometheus and
# Alertmanager release tarballs, at the versions the compose stack runs, each
# checked against the SHA-256 that release's sha256sums.txt lists (copied
# here on 2026-11-04). The one place these pins live: observability.yml and
# ci.yml (which proves the promtool gate can fail) both call this. Change a
# version and its digest together, and the image pins in docker-compose.yml
# with them.
#
# usage: install_tools.sh DIR promtool|amtool...
set -euo pipefail

PROMETHEUS_VERSION=3.15.1
PROMETHEUS_SHA256=6a726885328ad9efebbf36e876ce2fb6225468a71fd1e7ce3ba666d5f4d97a84
ALERTMANAGER_VERSION=0.34.2
ALERTMANAGER_SHA256=17ee14acd0f6ba6e25a436ff223d9c17ddd5cf484f543ad12f0932eaa89ba186

if [ "$#" -lt 2 ]; then
  echo "usage: $0 DIR promtool|amtool..." >&2
  exit 2
fi
dest=$1; shift
mkdir -p "$dest"
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

fetch() { # project version sha256 tool
  local name="$1-$2.linux-amd64"
  # Already there at this version (a local re-run): nothing to download.
  if [ -x "$dest/$4" ] && "$dest/$4" --version 2>&1 | grep -q "version $2 "; then
    echo "install_tools: $dest/$4 is already $2"
    return
  fi
  curl -fsSL -o "$work/$1.tar.gz" "https://github.com/prometheus/$1/releases/download/v$2/$name.tar.gz"
  echo "$3  $work/$1.tar.gz" | sha256sum -c -
  tar xzf "$work/$1.tar.gz" -C "$work" "$name/$4"
  mv "$work/$name/$4" "$dest/"
}

for tool in "$@"; do
  case "$tool" in
    promtool) fetch prometheus "$PROMETHEUS_VERSION" "$PROMETHEUS_SHA256" promtool ;;
    amtool) fetch alertmanager "$ALERTMANAGER_VERSION" "$ALERTMANAGER_SHA256" amtool ;;
    *) echo "install_tools: unknown tool '$tool'" >&2; exit 2 ;;
  esac
done
