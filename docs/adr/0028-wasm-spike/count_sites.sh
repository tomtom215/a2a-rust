#!/bin/bash
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
# Lines matching each pattern in a crate's non-test source: every .rs under src/
# except files or directories with `test` in the path. Inline `#[cfg(test)]`
# modules are NOT excluded, so these are upper bounds.
# usage: count_sites.sh CRATE_SRC_DIR
cd "$1" || exit 1
files=$(find . -name '*.rs' | grep -vi test)
n() { printf '  %-52s %s\n' "$1" "$(grep -hE "$2" $files | grep -vE '^\s*//' | wc -l)"; }
echo "$1: $(echo "$files" | wc -l) source files"
n 'tokio::spawn / rt::spawn'              'tokio::spawn\(|rt::spawn\('
n 'TaskTracker (spawn goes through tokio::spawn)' 'TaskTracker'
n 'spawn_blocking'                         'spawn_blocking'
n 'tokio::time::* (sleep/timeout/interval/Instant)' 'tokio::time::'
n 'std::time Instant::now / SystemTime::now' 'Instant::now\(|SystemTime::now\('
n 'tokio::runtime::Handle'                 'runtime::Handle'
n 'tokio::net / TcpStream / TcpListener'   'tokio::net|TcpStream|TcpListener'
n 'std::fs / tokio::fs'                    'std::fs::|tokio::fs::'
n 'hyper::body::Incoming'                  'body::Incoming|Incoming'
