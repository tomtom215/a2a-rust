#!/bin/bash
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F.
#
# ITK harness for a2a-rust — a shim over a2a-itk's shared driver.
#
# The upstream Integration Testing Kit (a2aproject/a2a-itk) mounts this
# repository into its service container as the "current" agent, builds the
# agent under ./itk (see README-current-agent.md), and runs the shared
# role-based scenario sets against every peer SDK line in its matrix.yaml:
# the official Python, JavaScript, Go and Java SDKs, and a2a-rs. Everything
# else — image build, container start, readiness poll, the /run request,
# response validation, result reporting and the nightly metrics file — lives
# in a2a-itk/scripts/run_itk_shared.sh, which every SDK repository shares.
#
# Usage (from anywhere; Docker or Podman required):
#
#   A2A_ITK_REVISION=main bash itk/run_itk.sh                     # PR set
#   A2A_ITK_REVISION=main ITK_NIGHTLY_RUN=true bash itk/run_itk.sh  # nightly set
#
# The nightly workflow (.github/workflows/itk-nightly.yml) sets
# ITK_NIGHTLY_RUN and publishes the resulting itk_rust.json.
set -e
cd "$(dirname "${BASH_SOURCE[0]}")"

ITK_SDK_NAME=rust
# The matrix's `rust` line is a2a-rs; this repository is the SUT ("current")
# and is graded against that line like any other peer. The repo name only
# labels the nightly metrics.
ITK_SDK_REPO=a2a-rust
ITK_SCENARIO_SET=shared

# No codegen step: build.rs compiles the vendored copy at protos/
# instruction.proto (the nightly workflow diffs it against the a2a-itk
# checkout), so the driver's copy into this directory is not needed.
ITK_COPY_PROTO=0

# The agent is built inside the container with `cargo build --locked
# --release` against a cold registry cache; the launcher's default 10-minute
# build timeout is too short for the server crate with gRPC enabled.
ITK_EXTRA_DOCKER_ARGS=(-e ITK_BUILD_TIMEOUT="${ITK_BUILD_TIMEOUT:-2400}")

# --- bootstrap -------------------------------------------------------------
# The shared driver lives in a2a-itk, so the checkout has to exist before it
# can be sourced. CI places it here via actions/checkout; locally it is
# cloned at the requested revision.
: "${A2A_ITK_REVISION:?A2A_ITK_REVISION environment variable must be set}"
if [ ! -d a2a-itk ]; then
  git clone https://github.com/a2aproject/a2a-itk.git a2a-itk
fi
(cd a2a-itk && git fetch origin && git checkout "$A2A_ITK_REVISION" \
  && { git symbolic-ref -q HEAD > /dev/null && git pull origin "$A2A_ITK_REVISION" || true; })

source a2a-itk/scripts/run_itk_shared.sh
