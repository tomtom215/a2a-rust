#!/bin/bash
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
# usage: probe.sh NAME URL [extra curl header args...]
NAME=$1; URL=$2; shift 2
OUT=/opt/bench/results/deep/params/raw/$NAME.server.txt
: > $OUT
declare -A B
B[a_omitted]='{"jsonrpc":"2.0","id":1,"method":"GetExtendedAgentCard"}'
B[b_null]='{"jsonrpc":"2.0","id":2,"method":"GetExtendedAgentCard","params":null}'
B[c_empty_obj]='{"jsonrpc":"2.0","id":3,"method":"GetExtendedAgentCard","params":{}}'
B[d_empty_arr]='{"jsonrpc":"2.0","id":4,"method":"GetExtendedAgentCard","params":[]}'
B[e_tenant]='{"jsonrpc":"2.0","id":5,"method":"GetExtendedAgentCard","params":{"tenant":""}}'
for k in a_omitted b_null c_empty_obj d_empty_arr e_tenant; do
  echo "=== $k" >> $OUT
  echo "REQ: POST $URL  $*" >> $OUT
  echo "BODY: ${B[$k]}" >> $OUT
  curl -sS -m 10 -o /tmp/probe_body.$$ -w 'HTTP %{http_code}\n' -X POST "$URL" -H 'A2A-Version: 1.0' -H 'Content-Type: application/json' "$@" --data-raw "${B[$k]}" >> $OUT 2>&1
  echo "RESP: $(cat /tmp/probe_body.$$)" >> $OUT
  echo >> $OUT
done
rm -f /tmp/probe_body.$$
cat $OUT | sed -E 's/^(RESP: .{0,300}).*/\1/'
