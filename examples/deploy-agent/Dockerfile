# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
#
# Build against the full toolchain, ship without it. The runtime
# image carries no compiler, no cargo registry and no source — a build image
# shipped as a runtime is the most common way an agent's attack surface ends up
# larger than its code.
#
# Build from the repository root, not this directory:
#   docker build -f examples/deploy-agent/Dockerfile -t deploy-agent .

# Three build stages, so a change to the source does not rebuild the
# dependencies. cargo-chef's `prepare` reduces the workspace to a recipe of
# its manifests and lock file; `cook` builds only the dependencies from that
# recipe, in a layer Docker reuses until a manifest or Cargo.lock changes;
# the last stage builds the workspace's own crates over it. The workspace is
# copied whole because this example depends on the SDK crates by path.
FROM rust:1.94-bookworm@sha256:6ae102bdbf528294bc79ad6e1fae682f6f7c2a6e6621506ba959f9685b308a55 AS chef
# An exact version from crates.io, built with its own lock file.
RUN cargo install cargo-chef --version 0.1.78 --locked
WORKDIR /src

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS build
COPY --from=planner /src/recipe.json recipe.json
RUN cargo chef cook --release --locked -p deploy-agent --recipe-path recipe.json
COPY . .
RUN cargo build --release --locked -p deploy-agent

# Debian slim rather than distroless or scratch: the binary links glibc. The
# SDK verifies TLS (push notifications, the client, the telemetry exporters)
# against its bundled Mozilla roots and never reads this image's certificate
# store, so `ca-certificates` below is for tools run inside the container, not
# for the agent. A private CA reaches the agent through the SDK's CA options,
# and reaches telemetry through OTEL_EXPORTER_OTLP_CERTIFICATE (a mounted PEM).
FROM debian:bookworm-slim@sha256:7c7b2c966bc9ee8cedfeef67e0e279108992c77681fa595db4a9d65c06ccc587 AS runtime

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# Unprivileged: nothing here needs root, and a container that runs as root is
# refused outright by many cluster admission policies.
RUN useradd --system --uid 10001 --no-create-home agent
USER 10001

COPY --from=build /src/target/release/deploy-agent /usr/local/bin/deploy-agent

# Documentation only — publishing a port does not bind it. The process reads
# PORT and defaults to 8080; keep the two in step.
EXPOSE 8080
ENV PORT=8080

# No shell form, so the binary is PID 1 and receives SIGTERM directly. The
# shell form (`CMD deploy-agent`) puts /bin/sh at PID 1, which does not forward
# signals — graceful shutdown silently stops working and rollouts start cutting
# streams mid-flight.
ENTRYPOINT ["/usr/local/bin/deploy-agent"]
