#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
#
# The README's "HTTP caching: ETag, Last-Modified, 304 Not Modified for agent card
# endpoints", and the other card-endpoint behaviour a client or a CDN relies on,
# against a running deploy-agent: conditional GETs, HEAD, OPTIONS (CORS), a wrong
# method, a trailing slash, and an Accept header the card endpoint cannot satisfy.
#
# Needs: 03-build-examples.sh done; port 8080 free; curl.
set -uo pipefail
. "$(dirname "${BASH_SOURCE[0]}")/env.sh"
repo_env
export NO_PROXY='*' no_proxy='*'
env -i PATH="$PATH" "$CARGO_TARGET_DIR/debug/deploy-agent" >/dev/null 2>&1 & pid=$!
trap 'kill $pid 2>/dev/null' EXIT
for _ in $(seq 1 100); do curl -s -o /dev/null http://127.0.0.1:8080/health && break; sleep 0.2; done
C=http://127.0.0.1:8080/.well-known/agent-card.json
echo "--- GET"; curl -s -D - -o /dev/null $C
etag=$(curl -s -D - -o /dev/null $C | awk -F': ' 'tolower($1)=="etag"{print $2}' | tr -d '\r')
lm=$(curl -s -D - -o /dev/null $C | awk -F': ' 'tolower($1)=="last-modified"{print $2}' | tr -d '\r')
echo "ETag=$etag  Last-Modified=$lm"
echo "--- If-None-Match: the ETag"; curl -s -o /dev/null -w 'HTTP %{http_code}, %{size_download} body bytes\n' -H "If-None-Match: $etag" $C
echo "--- If-None-Match: another tag"; curl -s -o /dev/null -w 'HTTP %{http_code}, %{size_download} body bytes\n' -H 'If-None-Match: "other"' $C
echo "--- If-None-Match: *"; curl -s -o /dev/null -w 'HTTP %{http_code}\n' -H 'If-None-Match: *' $C
echo "--- If-Modified-Since: Last-Modified"; curl -s -o /dev/null -w 'HTTP %{http_code}\n' -H "If-Modified-Since: $lm" $C
echo "--- If-Modified-Since: 1970"; curl -s -o /dev/null -w 'HTTP %{http_code}\n' -H 'If-Modified-Since: Thu, 01 Jan 1970 00:00:00 GMT' $C
echo "--- HEAD"; curl -s -I $C | head -8
echo "--- OPTIONS (CORS preflight from a browser origin)"; curl -s -i -X OPTIONS -H 'Origin: https://example.org' -H 'Access-Control-Request-Method: GET' $C | head -12
echo "--- POST to the card"; curl -s -i -X POST $C | head -6
echo "--- trailing slash"; curl -s -o /dev/null -w 'HTTP %{http_code}\n' $C/
echo "--- Accept: text/html"; curl -s -o /dev/null -w 'HTTP %{http_code} %{content_type}\n' -H 'Accept: text/html' $C
echo "--- the deprecated path /.well-known/agent.json"; curl -s -o /dev/null -w 'HTTP %{http_code}\n' http://127.0.0.1:8080/.well-known/agent.json
echo "--- extended card without credentials"; curl -s -i -H 'A2A-Version: 1.0' http://127.0.0.1:8080/extendedAgentCard | head -8
