#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
#
# Drives mixed traffic at an a2a-rust agent on all four bindings with the
# `a2a` CLI, so every dashboard panel and recording rule has data: successful
# sends and streams, a GetTask that fails (task not found) and ListTasks. With
# WEBHOOK_OK and WEBHOOK_BAD set (a receiver that answers 200, and a URL
# nothing answers), it also sends JSON-RPC messages carrying an inline push
# config to each, so push deliveries are both delivered and failed; the agent
# must allow private webhook URLs for that.
#
# Usage: [WEBHOOK_OK=url WEBHOOK_BAD=url] drive.sh A2A_BIN HTTP_URL GRPC_ADDR WS_URL SECONDS
set -uo pipefail
cli=$1 http=$2 grpc=$3 ws=$4 secs=$5
end=$((SECONDS + secs))
i=0
while (( SECONDS < end )); do
    i=$((i + 1))
    for pair in "jsonrpc $http" "rest $http" "grpc $grpc" "websocket $ws"; do
        set -- $pair
        "$cli" --binding "$1" send "$2" "drive $i" >/dev/null 2>&1
        "$cli" --binding "$1" stream "$2" "drive stream $i" >/dev/null 2>&1
        "$cli" --binding "$1" task get "$2" "no-such-task-$i" >/dev/null 2>&1
        "$cli" --binding "$1" task list "$2" >/dev/null 2>&1
    done
    for hook in ${WEBHOOK_OK:-} ${WEBHOOK_BAD:-}; do
        curl -s -o /dev/null -X POST "$http" -H 'content-type: application/json' -H 'A2A-Version: 1.0' \
            -d '{"jsonrpc":"2.0","id":1,"method":"SendMessage","params":{"message":{"messageId":"drive-push-'"$i-$RANDOM"'","role":"ROLE_USER","parts":[{"text":"push"}]},"configuration":{"taskPushNotificationConfig":{"url":"'"$hook"'"}}}}'
    done
done
echo "drive.sh: $i rounds"
