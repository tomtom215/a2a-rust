#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
#
# The observability walkthrough page, with the agents that were already built:
# two long-lived agents export OTLP to the compose stack's Collector (deploy-agent,
# JSON-RPC + HTTP+JSON on 8080; hello-agent, JSON-RPC on 3000), the `a2a` CLI drives
# traffic at them and exports its own client spans, then Prometheus, Tempo, Loki and
# Grafana are asked what arrived, and the repository's own checker
# (deploy/observability/scripts/check_dashboards.py) runs every panel's queries through
# Grafana's query API.
#
#   06-observability.sh BIN_DIR SECONDS [OUT_DIR]
#
# Needs: `docker compose -f deploy/observability/docker-compose.yml up -d` done and the
# six services healthy; BIN_DIR holding a2a, deploy-agent, hello-agent; ports 3000,
# 8080, 4318 (the Collector), 9090, 3200, 3100, 3001; curl, python3 with PyYAML.
set -uo pipefail
. "$(dirname "${BASH_SOURCE[0]}")/env.sh"
BIN=${1:?bin dir}; SECS=${2:-150}; OUT=${3:-/tmp/obs-out}
mkdir -p "$OUT"
export NO_PROXY='*' no_proxy='*'
OTEL=http://localhost:4318

echo "--- stack health"
for u in http://localhost:9090/-/ready http://localhost:3200/ready http://localhost:3100/ready http://localhost:3001/api/health http://localhost:9093/-/ready; do
  printf '%s -> ' "$u"; curl -s -o /dev/null -w '%{http_code}\n' "$u"
done

echo "--- start the agents"
env -i PATH="$PATH" OTEL_SERVICE_NAME=deploy-agent OTEL_RESOURCE_ATTRIBUTES=service.instance.id=local-1 OTEL_EXPORTER_OTLP_ENDPOINT=$OTEL AGENT_URL=http://127.0.0.1:8080 "$BIN/deploy-agent" >"$OUT/deploy-agent.log" 2>&1 & p1=$!
env -i PATH="$PATH" OTEL_SERVICE_NAME=hello-agent OTEL_RESOURCE_ATTRIBUTES=service.instance.id=local-1 OTEL_EXPORTER_OTLP_ENDPOINT=$OTEL "$BIN/hello-agent" >"$OUT/hello-agent.log" 2>&1 & p2=$!
# Each response is saved to a file, then read: data, not code, so nothing
# downloaded is ever piped into an interpreter (OpenSSF Scorecard, Pinned-Dependencies).
J=$(mktemp)
trap 'kill $p1 $p2 2>/dev/null; rm -f "$J"' EXIT
for _ in $(seq 1 100); do curl -s -o /dev/null http://127.0.0.1:8080/health && curl -s -o /dev/null http://127.0.0.1:3000/.well-known/agent-card.json && break; sleep 0.2; done
head -2 "$OUT/deploy-agent.log" | cut -c1-260; head -2 "$OUT/hello-agent.log" | cut -c1-260

echo "--- drive traffic for $SECS s (a2a CLI over jsonrpc and rest; a GetTask that fails; ListTasks)"
export OTEL_EXPORTER_OTLP_ENDPOINT=$OTEL OTEL_SERVICE_NAME=a2a-cli OTEL_RESOURCE_ATTRIBUTES=service.instance.id=cli-1
end=$((SECONDS + SECS)); i=0
while (( SECONDS < end )); do
  i=$((i + 1))
  for b in jsonrpc rest; do
    "$BIN/a2a" --binding $b send http://127.0.0.1:8080 "drive $i" >/dev/null 2>&1
    "$BIN/a2a" --binding $b stream http://127.0.0.1:8080 "drive stream $i" >/dev/null 2>&1
    "$BIN/a2a" --binding $b task get http://127.0.0.1:8080 "no-such-task-$i" >/dev/null 2>&1
    "$BIN/a2a" --binding $b task list http://127.0.0.1:8080 >/dev/null 2>&1
  done
  "$BIN/a2a" --binding jsonrpc send http://127.0.0.1:3000 "hello $i" >/dev/null 2>&1
done
echo "$i rounds"
unset OTEL_EXPORTER_OTLP_ENDPOINT OTEL_SERVICE_NAME OTEL_RESOURCE_ATTRIBUTES
sleep 20   # one more export interval

echo "--- Prometheus: which jobs and instances exported (count by (job, instance))"
curl -sG http://localhost:9090/api/v1/query --data-urlencode 'query=count by (job, instance) ({__name__=~".+"})' -o "$J" && python3 -I -c 'import json,sys; d=json.load(open(sys.argv[1])); [print(" ", r["metric"].get("job"), r["metric"].get("instance"), r["value"][1]) for r in d["data"]["result"]]' "$J"
echo "--- a recording rule and an alert listing"
curl -sG http://localhost:9090/api/v1/query --data-urlencode 'query=a2a:rpc_server_calls:rate5m' -o "$J" && python3 -I -c 'import json,sys; d=json.load(open(sys.argv[1])); print("  a2a:rpc_server_calls:rate5m series:", len(d["data"]["result"]))' "$J"
curl -s http://localhost:9090/api/v1/rules -o "$J" && python3 -I -c 'import json,sys; d=json.load(open(sys.argv[1])); g=d["data"]["groups"]; print("  rule groups:", len(g), "rules:", sum(len(x["rules"]) for x in g), "alerts firing:", sum(1 for x in g for r in x["rules"] if r.get("type")=="alerting" and r.get("state")=="firing"))' "$J"
echo "--- Tempo: traces by service"
for svc in deploy-agent hello-agent a2a-cli; do
  printf '  %s: ' $svc; curl -sG http://localhost:3200/api/search --data-urlencode "tags=service.name=$svc" --data-urlencode limit=5 -o "$J" && python3 -I -c 'import json,sys; d=json.load(open(sys.argv[1])); print(len(d.get("traces", [])), "trace(s) returned (limit 5)")' "$J"
done
echo "--- Loki: log streams by service"
curl -sG http://localhost:3100/loki/api/v1/label/service_name/values -o "$J" && python3 -I -c 'import json,sys; print(" ", json.load(open(sys.argv[1])).get("data"))' "$J"
echo "--- the repository's own panel checker, through Grafana"
python3 "$REPO/deploy/observability/scripts/check_dashboards.py" --grafana http://localhost:3001 --prometheus http://localhost:9090 "$REPO/deploy/observability/grafana/dashboards" --lookback 10 2>&1 | tail -60
