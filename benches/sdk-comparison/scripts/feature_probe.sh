#!/bin/bash
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
# a2a-rust persistence + bearer auth black-box probe. Needs agent-a2a-rust built with --features sqlite.
B=${B:?set B to an agent-a2a-rust binary built with --features sqlite}; DB=$(mktemp -u /tmp/probe-XXXX.db); V='A2A-Version: 1.0'
req='{"jsonrpc":"2.0","id":1,"method":"SendMessage","params":{"message":{"role":"ROLE_USER","messageId":"pm-1","parts":[{"text":"persist me"}]}}}'
(PORT=3010 SQLITE_URL="sqlite://$DB?mode=rwc" $B > /dev/null 2>&1 &); sleep 1
TID=$(curl -s -XPOST 127.0.0.1:3010/ -H "$V" -H 'content-type: application/json' -d "$req" | python3 -c 'import sys,json;print(json.load(sys.stdin)["result"]["task"]["id"])')
pkill -9 -x agent-a2a-rust; sleep 0.5
(PORT=3010 SQLITE_URL="sqlite://$DB?mode=rwc" $B > /dev/null 2>&1 &); sleep 1
R=$(curl -s -XPOST 127.0.0.1:3010/ -H "$V" -H 'content-type: application/json' -d "{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"GetTask\",\"params\":{\"id\":\"$TID\"}}")
echo "$R" | grep -q TASK_STATE_COMPLETED && echo "PASS persistence: task $TID readable after kill -9 + restart" || echo "FAIL persistence: $R"
pkill -x agent-a2a-rust; sleep 0.5; rm -f $DB*
(PORT=3011 AUTH_TOKEN=s3cret $B > /dev/null 2>&1 &); sleep 1
for h in "" "Authorization: Bearer wrong" "Authorization: Bearer s3cret"; do
  c=$(curl -s -o /dev/null -w '%{http_code}' -XPOST 127.0.0.1:3011/ -H "$V" -H 'content-type: application/json' ${h:+-H "$h"} -d "$req")
  echo "auth ${h:-<no header>}: http $c"
done
echo "auth card without token: http $(curl -s -o /dev/null -w '%{http_code}' 127.0.0.1:3011/.well-known/agent-card.json)"
pkill -x agent-a2a-rust
