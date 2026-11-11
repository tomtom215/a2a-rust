#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
#
# Runs a command, and runs it again while it fails, up to COUNT times.
#
# usage: retry.sh COUNT -- command [args...]
#
# For a command whose failure can be a transient network answer and whose
# repetition is harmless: `git clone` (git removes the directory it was
# creating when it fails, so the next attempt starts clean) and `git fetch`.
# The pause before attempt N+1 is RETRY_DELAY seconds (default 5) times N.
# The command's own output is not hidden: every attempt's stderr is in the log.
#
# Not for a command that changes state a second run would double (a push, a
# publish, an upload): those are not retried blindly.
#
# Exit status: the command's, from the last attempt; 2 on bad usage.

set -uo pipefail

if [ "$#" -lt 3 ] || ! [[ "$1" =~ ^[1-9][0-9]*$ ]] || [ "$2" != "--" ]; then
  echo "usage: $0 COUNT -- command [args...]" >&2
  exit 2
fi
count=$1
shift 2
delay=${RETRY_DELAY:-5}

status=0
for ((n = 1; n <= count; n++)); do
  "$@" && exit 0
  status=$?
  echo "retry: attempt $n/$count of '$1' exited $status" >&2
  if [ "$n" -lt "$count" ]; then
    sleep $((delay * n))
  fi
done
exit "$status"
