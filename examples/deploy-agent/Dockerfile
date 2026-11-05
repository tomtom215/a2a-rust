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
#
# What reaches the build is set by `Dockerfile.dockerignore` beside this file,
# which BuildKit reads in place of the root `.dockerignore` and which travels
# with this file when it is copied into another project: no secrets, no
# history, and nothing the build does not read (the book, the docs, the other
# languages' agents), so an edit there does not rebuild the agent.
#
# Another crate, in a project this file was copied into: name its package
# (and, when the binary is named otherwise, its binary). The rest of the file
# needs no edit; the book's "Docker and Kubernetes" page lists what else a
# copy needs (a committed Cargo.lock, for `--locked`).
#   docker build -f Dockerfile --build-arg PACKAGE=my-agent -t my-agent .
ARG PACKAGE=deploy-agent
ARG BIN=${PACKAGE}

# Three build stages, so a change to the source does not rebuild the
# dependencies. cargo-chef's `prepare` reduces the workspace to a recipe of
# its manifests and lock file; `cook` builds only the dependencies from that
# recipe, in a layer Docker reuses until a manifest or Cargo.lock changes;
# the last stage builds the workspace's own crates over it. The workspace is
# copied whole because this example depends on the SDK crates by path.
#
# Behind a TLS-inspecting proxy the build cannot reach crates.io: the proxy
# signs it with its own CA, and `cargo` fails with "SSL certificate problem:
# self-signed certificate in certificate chain" (reproduced 2026-11-01, audit
# L18). Pass that CA as a BuildKit secret and every step that downloads trusts
# it, without the file reaching a layer or the image:
#
#   docker build --secret id=cacert,src=/path/to/proxy-ca.pem \
#     -f examples/deploy-agent/Dockerfile -t deploy-agent .
#
# Without the secret nothing changes. (`HTTP_PROXY`/`HTTPS_PROXY` given as
# `--build-arg`s are passed through by Docker as they are.)
FROM rust:1.94-bookworm@sha256:6ae102bdbf528294bc79ad6e1fae682f6f7c2a6e6621506ba959f9685b308a55 AS chef
# An exact version from crates.io, built with its own lock file.
RUN --mount=type=secret,id=cacert,required=false \
    if [ -s /run/secrets/cacert ]; then export CARGO_HTTP_CAINFO=/run/secrets/cacert; fi \
    && cargo install cargo-chef --version 0.1.78 --locked
WORKDIR /src

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS build
ARG PACKAGE
ARG BIN
COPY --from=planner /src/recipe.json recipe.json
RUN --mount=type=secret,id=cacert,required=false \
    if [ -s /run/secrets/cacert ]; then export CARGO_HTTP_CAINFO=/run/secrets/cacert; fi \
    && cargo chef cook --release --locked -p "$PACKAGE" --recipe-path recipe.json
COPY . .
RUN --mount=type=secret,id=cacert,required=false \
    if [ -s /run/secrets/cacert ]; then export CARGO_HTTP_CAINFO=/run/secrets/cacert; fi \
    && cargo build --release --locked -p "$PACKAGE" --bin "$BIN" \
    && cp "target/release/$BIN" /src/agent

# Debian slim rather than scratch, and rather than distroless for a reason that
# is a choice, not a necessity: distroless/cc carries glibc too, so the binary
# would run there. Slim keeps a shell, which is what `docker exec`, the
# HEALTHCHECK below and an incident at 3 a.m. want, at the cost of a larger
# image and more packages to patch (the base is pinned by digest, and
# Dependabot's docker ecosystem refreshes the digest weekly). The SDK verifies
# TLS against its bundled Mozilla roots, so a public CA needs nothing here. A
# private CA reaches the A2A client through SSL_CERT_FILE or SSL_CERT_DIR (a
# mounted PEM; set SSL_CERT_FILE=/etc/ssl/certs/ca-certificates.crt to trust
# this image's store), telemetry through OTEL_EXPORTER_OTLP_CERTIFICATE, and
# PostgreSQL through the URL's sslrootcert.
FROM debian:bookworm-slim@sha256:7c7b2c966bc9ee8cedfeef67e0e279108992c77681fa595db4a9d65c06ccc587 AS runtime

# The CA store, taken from the digest-pinned build image rather than from an
# `apt-get install` against the day's Debian index, which made this layer
# differ from one build to the next though both bases are pinned (audit A19).
COPY --from=chef /etc/ssl/certs/ca-certificates.crt /etc/ssl/certs/ca-certificates.crt

# Unprivileged: nothing here needs root, and a container that runs as root is
# refused outright by many cluster admission policies.
RUN useradd --system --uid 10001 --no-create-home agent
USER 10001

COPY --from=build /src/agent /usr/local/bin/agent

# The version the agent card reports, set from the release tag by the image
# job (release.yml). Empty, as from a local `docker build`, the binary reports
# its own crate version.
ARG AGENT_VERSION=
ENV AGENT_VERSION=${AGENT_VERSION}

# Documentation only — publishing a port does not bind it. The process reads
# PORT and defaults to 8080; keep the two in step.
EXPOSE 8080
ENV PORT=8080

# For `docker run` and Compose, which act on it; Kubernetes ignores it and uses
# the manifest's probes. GET /health over bash's /dev/tcp, because the image
# has no curl and the check must not need the agent's own code: any agent that
# serves /health on PORT passes it. The first line of the answer must be
# `HTTP/1.1 200`.
HEALTHCHECK --interval=10s --timeout=3s --start-period=60s --retries=3 \
  CMD ["bash", "-c", "exec 3<>/dev/tcp/127.0.0.1/${PORT} && printf 'GET /health HTTP/1.1\\r\\nHost: localhost\\r\\nConnection: close\\r\\n\\r\\n' >&3 && read -r -t 2 status <&3 && [[ $status == 'HTTP/1.1 200'* ]]"]

# No shell form, so the binary is PID 1 and receives SIGTERM directly. The
# shell form (`CMD deploy-agent`) puts /bin/sh at PID 1, which does not forward
# signals — graceful shutdown silently stops working and rollouts start cutting
# streams mid-flight.
ENTRYPOINT ["/usr/local/bin/agent"]
