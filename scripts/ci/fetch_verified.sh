#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
#
# Downloads one file over HTTPS, retries a failed or corrupt transfer, and
# refuses to leave anything at DEST unless its SHA-256 is the pinned one.
#
# usage: fetch_verified.sh URL SHA256 DEST
#
# Why this exists. On 2026-11-09 the `static-checks` job of `ci.yml` failed on
# `curl -sSL <mdBook release> | tar -xz` with "not in gzip format", and every
# gate after it did not run. That line had three faults, and each is a rule here:
#
#   1. No `--fail`: curl exits 0 on an HTTP error and writes the error page, so
#      tar was handed HTML (or a truncated body) as if it were the archive.
#   2. No retry: one bad response from the CDN ended the job. The
#      observability job's promtool download died the same way on an HTTP 504.
#   3. No checksum: an archive that did arrive was run unverified. A pinned
#      digest is what makes "the bytes are the ones we reviewed" a check.
#
# A download that fails, or that arrives with the wrong digest, is fetched
# again, up to FETCH_ATTEMPTS times (default 5) with a growing pause
# (FETCH_RETRY_DELAY seconds times the attempt number, default 5; curl's own
# three retries inside one attempt pause FETCH_CURL_RETRY_DELAY, default 2). A digest that
# is still wrong on the last attempt is a hard failure that names both digests
# and what the bytes look like: it is either a release that was replaced, which
# the maintainer must look at, or a proxy that keeps serving an error page.
# Nothing is written to DEST before the digest matches; a partial file is
# removed. HTTPS only (plain HTTP is accepted for a loopback host, which is how
# the self-test serves the failures this script must survive).
#
# Exit status: 0 verified; 1 every attempt failed or mismatched; 2 bad usage.

set -euo pipefail

if [ "$#" -ne 3 ]; then
  echo "usage: $0 URL SHA256 DEST" >&2
  exit 2
fi
url=$1
want=$2
dest=$3

if ! [[ "$want" =~ ^[0-9a-f]{64}$ ]]; then
  echo "fetch_verified: SHA256 must be 64 lowercase hex digits, got '$want'" >&2
  exit 2
fi
case "$url" in
  https://*) ;;
  http://127.0.0.1[:/]* | http://localhost[:/]* | http://\[::1\][:/]*) ;;
  *)
    echo "fetch_verified: refusing '$url': HTTPS only (plain HTTP only to loopback)" >&2
    exit 2
    ;;
esac

attempts=${FETCH_ATTEMPTS:-5}
delay=${FETCH_RETRY_DELAY:-5}
if ! [[ "$attempts" =~ ^[1-9][0-9]*$ ]] || ! [[ "$delay" =~ ^[0-9]+$ ]]; then
  echo "fetch_verified: FETCH_ATTEMPTS must be a positive integer and FETCH_RETRY_DELAY a non-negative one" >&2
  exit 2
fi

mkdir -p "$(dirname "$dest")"
part=$(mktemp "$dest.XXXXXX.part")
trap 'rm -f "$part"' EXIT

proto=(--proto '=https')
case "$url" in http://*) proto=(--proto '=http,https') ;; esac

got=none
for ((n = 1; n <= attempts; n++)); do
  # curl's own retry covers a refused connection or a 5xx inside one attempt;
  # this loop covers what curl cannot see: bytes that arrive and are wrong.
  if curl --fail --silent --show-error --location "${proto[@]}" \
    --connect-timeout 20 --max-time 300 \
    --retry 3 --retry-all-errors --retry-delay "${FETCH_CURL_RETRY_DELAY:-2}" \
    --output "$part" "$url"; then
    got=$(sha256sum "$part" | cut -d' ' -f1)
    if [ "$got" = "$want" ]; then
      mv "$part" "$dest"
      trap - EXIT
      echo "fetch_verified: $dest ($(wc -c <"$dest") bytes) matches sha256 $want"
      exit 0
    fi
    kind=$(file -b "$part" 2>/dev/null | cut -c1-60 || echo unknown)
    echo "fetch_verified: attempt $n/$attempts: sha256 of the download is $got, expected $want ($(wc -c <"$part") bytes, $kind)" >&2
  else
    status=$?
    echo "fetch_verified: attempt $n/$attempts: curl exited $status for $url" >&2
  fi
  if [ "$n" -lt "$attempts" ]; then
    sleep $((delay * n))
  fi
done

echo "fetch_verified: giving up on $url after $attempts attempts (last digest $got, expected $want)" >&2
exit 1
