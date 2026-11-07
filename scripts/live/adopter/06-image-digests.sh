#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
#
# Every image the deployment artifacts pin by digest (the deploy-agent Dockerfile's two
# bases, the observability compose file's six services): does the registry still serve
# that digest? Reads the manifest only (`docker buildx imagetools inspect`), pulls no layer.
#
# Needs: a running Docker daemon (`dockerd &`) and a registry route (Docker Hub may answer 429).
set -uo pipefail
. "$(dirname "${BASH_SOURCE[0]}")/env.sh"
cd "$REPO"
refs=$( { grep -hoE '^FROM [^ ]+@sha256:[0-9a-f]{64}' examples/deploy-agent/Dockerfile | awk '{print $2}'
          grep -hoE 'image: [^ ]+@sha256:[0-9a-f]{64}' deploy/observability/docker-compose.yml | awk '{print $2}'; } | sort -u)
ok=0; bad=0
for r in $refs; do
  out=$(docker buildx imagetools inspect "$r" 2>&1); rc=$?
  if [ $rc -eq 0 ]; then
    echo "OK   $r"; ok=$((ok+1))
  else
    echo "FAIL $r :: $(echo "$out" | head -1)"; bad=$((bad+1))
  fi
done
echo "digests present: $ok, failing: $bad"
