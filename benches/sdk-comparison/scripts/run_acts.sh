#!/bin/bash
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
# Usage: run_acts.sh <sdk-root> <ITK_SDK_REPO> [extra build timeout]
# Mirrors the SDK's own itk/run_itk.sh shim; differences: host networking
# (sandbox proxy), prebuilt image, a2a-itk pinned to b57c5332.
set -e
SDK_ROOT="$1"; REPO="$2"
cd "$SDK_ROOT/itk"
rm -rf a2a-itk && cp -r "${ITK_CHECKOUT:?set ITK_CHECKOUT to an a2a-itk checkout at b57c5332}" a2a-itk
ITK_SDK_NAME=rust
ITK_SDK_REPO="$REPO"
ITK_SCENARIO_SET=shared
ITK_COPY_PROTO=0
ITK_EXTRA_DOCKER_ARGS=(--network host -e ITK_BUILD_TIMEOUT=2400)
export A2A_ITK_REVISION=b57c5332aa883b27c1e5c915fe61cac76d2a1de9 ITK_ACTS_RUN=1 ITK_ACTS_TRANSPORTS=jsonrpc,grpc,rest ITK_SKIP_BUILD=1 ITK_READINESS_TIMEOUT=2400
source a2a-itk/scripts/run_itk_shared.sh
