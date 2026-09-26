#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
# profile_one.sh <name> <binary> <port> <path>
set -u
cd "$(dirname "$0")"
NAME=$1; BINARY=$2; PORT=$3; P=$4
rm -f cg.$NAME.out*
AGENT_MODE=echo valgrind --tool=callgrind --instr-atstart=no --callgrind-out-file=cg.$NAME.out "$BINARY" >/dev/null 2>vg.$NAME.log &
VP=$!
for i in $(seq 1 120); do curl -sf http://127.0.0.1:$PORT/.well-known/agent-card.json >/dev/null 2>&1 && break; sleep 1; done
python3 load.py 127.0.0.1 $PORT $P 300
callgrind_control -i on $VP >/dev/null
python3 load.py 127.0.0.1 $PORT $P 2000
callgrind_control -i off $VP >/dev/null
kill $VP; wait $VP 2>/dev/null
t=$(grep -h "^totals:" cg.$NAME.out | awk '{print $2}')
m=$(python3 callers.py cg.$NAME.out memcpy_avx_unaligned_erms | head -1 | awk '{print $NF}' | tr -d ,)
echo "$NAME: $((t/2000)) Ir/req, memcpy $((m/2000)) Ir/req"
