#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
#
# Build llama.cpp's `llama-server` at its newest release tag, and fetch two
# small GGUFs with their Hugging Face LFS hashes checked.
#
# Needs: cmake, ninja, a C++ toolchain, git; network to github.com (git over
# the proxy) and huggingface.co; about 6 GB while building, 4 GB of models.
# Writes: $LLAMA_DIR (default /opt/llama/latest) and $MODELS_DIR (/opt/models).
# Takes about 6 minutes on 4 vCPUs for the build; the downloads depend on the link.
set -euo pipefail
LLAMA_DIR="${LLAMA_DIR:-/opt/llama/latest}"
MODELS_DIR="${MODELS_DIR:-/opt/models}"
JOBS="${JOBS:-2}"

if [ ! -d "$LLAMA_DIR/.git" ]; then
  tag=$(git ls-remote --tags https://github.com/ggml-org/llama.cpp \
        | awk '{print $2}' | sed 's|refs/tags/||' | grep -E '^b[0-9]+$' \
        | sort -t b -k2 -n | tail -1)
  echo "newest tag: $tag"
  git clone --depth 1 --branch "$tag" https://github.com/ggml-org/llama.cpp "$LLAMA_DIR"
fi
git -C "$LLAMA_DIR" log -1 --format='commit %H (%cd)'

cmake -S "$LLAMA_DIR" -B "$LLAMA_DIR/build" -G Ninja -DCMAKE_BUILD_TYPE=Release \
  -DLLAMA_CURL=OFF -DGGML_NATIVE=ON -DLLAMA_BUILD_EXAMPLES=OFF -DLLAMA_BUILD_TESTS=OFF
cmake --build "$LLAMA_DIR/build" --target llama-server -j"$JOBS"
"$LLAMA_DIR/build/bin/llama-server" --version 2>&1 | head -3

fetch() { # repo file
  local repo=$1 file=$2
  local want
  local tree
  tree=$(mktemp)
  # Saved, then read: the listing is data, never piped into an interpreter.
  curl -sS "https://huggingface.co/api/models/$repo/tree/main" -o "$tree"
  want=$(python3 -I -c 'import json,sys; f=sys.argv[1]; print(next(x["lfs"]["oid"] for x in json.load(open(sys.argv[2])) if x["path"]==f))' "$file" "$tree")
  rm -f "$tree"
  if [ ! -s "$MODELS_DIR/$file" ]; then
    curl -sS -L -o "$MODELS_DIR/$file" "https://huggingface.co/$repo/resolve/main/$file"
  fi
  local got
  got=$(sha256sum "$MODELS_DIR/$file" | awk '{print $1}')
  if [ "$got" = "$want" ]; then echo "OK   $file $(stat -c %s "$MODELS_DIR/$file") bytes sha256 $got"
  else echo "BAD  $file: sha256 $got, Hugging Face says $want"; exit 1; fi
}
mkdir -p "$MODELS_DIR"
fetch unsloth/Qwen3.5-2B-GGUF Qwen3.5-2B-Q4_K_M.gguf
fetch ggml-org/gemma-4-E2B-it-GGUF gemma-4-E2B-it-Q4_0.gguf
