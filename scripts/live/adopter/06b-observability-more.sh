#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
#
# The dashboards' remaining panels, which 06-observability.sh's traffic cannot fill:
#   * GenAI: `tool-loop-agent` against model_stub.py's scripted model (a tool call, then an
#     answer, and some failing turns), exporting OTLP, so model calls, tokens, tool calls and
#     agent runs exist;
#   * store operations: a second deploy-agent on port 8081 over a real PostgreSQL
#     (A2A_DATABASE_URL), exporting OTLP, driven with a2a;
# then the repository's panel checker again. Push deliveries and persistence errors stay
# unexercised (the first needs a webhook receiver the agent may reach; the second needs a store that fails).
#
#   06b-observability-more.sh BIN_DIR [PG_URL]
#
# Needs: the compose stack up; BIN_DIR with a2a, deploy-agent, tool-loop-agent; PostgreSQL;
# ports 8081, 3501-3512.
set -uo pipefail
. "$(dirname "${BASH_SOURCE[0]}")/env.sh"
BIN=${1:?bin dir}; PG=${2:-postgres://postgres:postgres@localhost:5432/postgres}
export NO_PROXY='*' no_proxy='*'
OTEL=http://localhost:4318
W=$(mktemp -d)
MODEL_STUB_LOG="$W/stub.log" python3 -I "$REPO/scripts/live/adopter/model_stub.py" 3500 >"$W/stub.out" 2>&1 & stub=$!
env -i PATH="$PATH" PORT=8081 AGENT_URL=http://127.0.0.1:8081 A2A_DATABASE_URL="$PG" OTEL_SERVICE_NAME=deploy-agent-pg OTEL_RESOURCE_ATTRIBUTES=service.instance.id=pg-1 OTEL_EXPORTER_OTLP_ENDPOINT=$OTEL "$BIN/deploy-agent" >"$W/pg-agent.log" 2>&1 & pg=$!
trap 'kill $stub $pg 2>/dev/null' EXIT
for _ in $(seq 1 100); do curl -s -o /dev/null http://127.0.0.1:8081/ready && break; sleep 0.3; done
echo "--- deploy-agent over PostgreSQL: /ready -> $(curl -s -o /dev/null -w '%{http_code}' http://127.0.0.1:8081/ready)"
head -6 "$W/pg-agent.log" | cut -c1-250

echo "--- GenAI: tool-loop-agent x15 (tool call then answer) and x3 (a 500 from the provider), exporting OTLP"
for i in $(seq 1 15); do
  OTEL_SERVICE_NAME=tool-loop-agent OTEL_RESOURCE_ATTRIBUTES=service.instance.id=tla-$i OTEL_EXPORTER_OTLP_ENDPOINT=$OTEL \
    TOOL_LOOP_BASE_URL=http://127.0.0.1:3511/v1 timeout 60 "$BIN/tool-loop-agent" "How is the checkout service doing?" >/dev/null 2>&1
done
for i in 1 2 3; do
  OTEL_SERVICE_NAME=tool-loop-agent OTEL_RESOURCE_ATTRIBUTES=service.instance.id=tlb-$i OTEL_EXPORTER_OTLP_ENDPOINT=$OTEL \
    TOOL_LOOP_BASE_URL=http://127.0.0.1:3505/v1 timeout 60 "$BIN/tool-loop-agent" "How is the checkout service doing?" >/dev/null 2>&1
done

echo "--- store operations: 40 rounds of send / get / list over PostgreSQL"
export OTEL_EXPORTER_OTLP_ENDPOINT=$OTEL OTEL_SERVICE_NAME=a2a-cli OTEL_RESOURCE_ATTRIBUTES=service.instance.id=cli-pg
for i in $(seq 1 40); do
  out=$("$BIN/a2a" --binding jsonrpc send http://127.0.0.1:8081 "pg $i" 2>/dev/null)
  tid=$(printf '%s' "$out" | python3 -I -c 'import json,sys; print(json.load(sys.stdin)["task"]["id"])' 2>/dev/null)
  [ -n "$tid" ] && "$BIN/a2a" --binding jsonrpc task get http://127.0.0.1:8081 "$tid" >/dev/null 2>&1
  "$BIN/a2a" --binding jsonrpc task list http://127.0.0.1:8081 --page-size 5 >/dev/null 2>&1
done
unset OTEL_EXPORTER_OTLP_ENDPOINT OTEL_SERVICE_NAME OTEL_RESOURCE_ATTRIBUTES
sleep 25
echo "--- the panel checker again"
python3 "$REPO/deploy/observability/scripts/check_dashboards.py" --grafana http://localhost:3001 --prometheus http://localhost:9090 "$REPO/deploy/observability/grafana/dashboards" --lookback 15 2>&1 | grep -E 'FAIL|check_dashboards|queries|checked'
