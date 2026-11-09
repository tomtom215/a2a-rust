#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
#
# Records what a live-model run ran on: the llama-server build, the model files and the start of
# their SHA-256, the command lines used, the machine and the load, the Rust toolchain and the tree.
# Needs: /opt/llama/latest/build/bin/llama-server and the GGUFs under /opt/models (see /opt/llama/README.md).
# Proves nothing; it is the header every recorded run refers to.
set -uo pipefail
echo "date: $(date -u +%FT%TZ)"
echo "uname: $(uname -srm)"
echo "cpus: $(nproc); load: $(cut -d' ' -f1-3 /proc/loadavg); memory (MiB): $(free -m | awk '/Mem:/ {print "total " $2 ", available " $7}')"
echo "tree: $(git -C "$(dirname "$0")/../../../.." rev-parse HEAD) on $(git -C "$(dirname "$0")/../../../.." rev-parse --abbrev-ref HEAD)"
echo "rustc: $(rustc --version)"
echo "llama-server --version:"
/opt/llama/latest/build/bin/llama-server --version 2>&1 | sed 's/^/  /'
for m in /opt/models/*.gguf; do
  echo "model: $(basename "$m") $(stat -c %s "$m") bytes sha256 $(sha256sum "$m" | cut -c1-16)..."
done
cat <<'EOF'
command lines (one model at a time, port 11434):
  llama-server -m /opt/models/Qwen3.5-2B-Q4_K_M.gguf --host 127.0.0.1 --port 11434 --jinja -c 8192 -np 1 -t 2
  llama-server -m /opt/models/gemma-4-E2B-it-Q4_0.gguf --host 127.0.0.1 --port 11434 --jinja -c 8192 -np 1 -t 2
  (the genai-agent runs add --reasoning off, as its README does; the other runs send
   chat_template_kwargs {"enable_thinking": false} per request unless the run says thinking is on)
EOF
