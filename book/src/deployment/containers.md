<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Docker and Kubernetes

What the repository gives you for running an agent in a container, which
settings in it matter and why, and what it leaves to you. Everything here is
in `examples/deploy-agent`; the Rust side of that example (configuration,
probes, shutdown) is on [its page](../examples/deploy-agent.md).

## What the repository contains

| Files | What they are |
|---|---|
| `examples/deploy-agent/Dockerfile`, `Dockerfile.dockerignore` | A multi-stage image for `deploy-agent`: build with the Rust toolchain, ship on Debian slim without it. The release workflow builds it from each tag and publishes it, signed, to `ghcr.io/tomtom215/a2a-rust/deploy-agent` (from 0.16.0) |
| `examples/deploy-agent/deployment.yaml` | A Kubernetes `Deployment` (two replicas) and `Service` for that image |
| `deploy/observability/docker-compose.yml`, `docker-compose.prod.yml` | A telemetry stack (OpenTelemetry Collector, Prometheus, Tempo, Loki, Grafana) for a laptop or a test box; see [Observability](./observability.md) |
| `itk/docker-compose.yml`, `itk/Dockerfile.*`, `itk/agents/*/Dockerfile` | The integration test kit: agents built with the Python, JavaScript, Go and Java SDKs and this SDK's `echo-agent`, run against each other. Test infrastructure, not a template for deploying |

There are no Helm charts and no Kustomize overlays. The one published image is
`deploy-agent`'s, pushed to GHCR by `release.yml` under the release version
only, with a Sigstore-signed build-provenance attestation (`gh attestation
verify oci://ghcr.io/tomtom215/a2a-rust/deploy-agent:<version> --repo
tomtom215/a2a-rust`) and BuildKit's SBOM. That image is the ready-to-run
server: it runs a trivial agent on purpose, and what is ready is the wrapper
around it (configuration, probes, drain, retention, telemetry, the bind and the
card); there is no configurable gateway binary ([ADR
0040](https://github.com/tomtom215/a2a-rust/blob/main/docs/adr/0040-first-run-configuration-and-scope.md)).
An agent of your own is built from your own code with the same files; see
[Copying it into your project](#copying-it-into-your-project). The workflows that build the test images (`acts.yml`, `itk-nightly.yml`)
load them into the runner for a test and never push them.

## Building the image

Build from the repository root, because the example depends on the SDK
crates by path and the build needs the whole workspace:

```bash
docker build -f examples/deploy-agent/Dockerfile -t deploy-agent .
docker run --rm -p 8080:8080 -e AGENT_URL=http://localhost:8080 deploy-agent
```

What the Dockerfile does, and why:

- **Dependencies in their own layer.** Three build stages use `cargo-chef`
  (0.1.78, installed `--locked`): `prepare` reduces the workspace to a recipe
  of its manifests and lock file, `cook` builds only the dependencies from it,
  and the last stage builds the workspace's own crates on top. Docker reuses
  the dependency layer until a manifest or `Cargo.lock` changes: after a
  change to the source only, a rebuild took 221 s against 428 s from nothing.
- **Pinned bases.** `rust:1.94-bookworm` to build and `debian:bookworm-slim`
  to run, each pinned by digest. Debian slim rather than scratch, which has
  no glibc, and rather than `distroless/cc`, which has glibc and would run
  the binary too: slim keeps a shell, which `docker exec`, the health check
  below and an incident want, at the cost of more packages to patch. Nothing
  is installed at build time: the CA store is copied from the pinned build
  image, so the layer does not depend on the day's Debian index.
- **A health check for Docker and Compose.** `HEALTHCHECK` sends `GET
  /health` to `127.0.0.1:$PORT` with bash's `/dev/tcp` (the image has no
  curl) and passes on `HTTP/1.1 200`; it needs nothing from the agent but
  that endpoint, so a copied Dockerfile keeps working. Kubernetes ignores it
  and uses the manifest's probes.
- **A small build context.** `Dockerfile.dockerignore`, beside the
  Dockerfile, is what BuildKit reads for it instead of the root
  `.dockerignore`: no `.env`, keys or `.git`, and none of the book, the docs
  or the other languages' agents, so editing those does not rebuild the
  workspace's crates.
- **Certificates.** The SDK verifies TLS against its bundled Mozilla roots,
  so a public CA needs nothing from the image. A private CA (or a
  TLS-inspecting proxy's root) reaches, through `SSL_CERT_FILE` or
  `SSL_CERT_DIR` (a mounted PEM file or directory, trusted beside Mozilla's
  roots), the client crate's HTTP and WebSocket bindings, card discovery,
  JWKS and the token provider, and this crate's push delivery, JWKS and OIDC
  discovery, and the tool loop's model client (until 2026-10-31 the server's
  three did not read it); gRPC does not. In code the roots come through
  `ClientBuilder::with_tls_config` and `CardFetchOptions::with_tls_config`
  on the client, and `HttpPushSender::with_tls_config`,
  `JwtAuthInterceptor::from_oidc_issuer_with_tls_config` and
  `OpenAiChatModel::with_tls_config` on the server. It reaches telemetry through
  `OTEL_EXPORTER_OTLP_CERTIFICATE`, and the PostgreSQL store through the
  URL's `sslrootcert`. `ca-certificates` is installed for tools run inside
  the container; `SSL_CERT_FILE=/etc/ssl/certs/ca-certificates.crt` makes the
  client trust it too.
- **Unprivileged.** It runs as uid 10001; many cluster admission policies
  refuse a container that runs as root.
- **The binary is PID 1.** `ENTRYPOINT` is in exec form, so the process
  receives `SIGTERM` directly. The shell form (`CMD deploy-agent`) puts
  `/bin/sh` at PID 1, which does not forward signals: graceful shutdown stops
  working and every rollout cuts streams mid-flight.
- **`PORT`.** `EXPOSE 8080` documents the port and `ENV PORT=8080` sets it;
  the process reads `PORT` and defaults to 8080. Keep the two in step.

## Copying it into your project

The Dockerfile builds whichever package `--build-arg PACKAGE=` names
(`deploy-agent` by default; `BIN=` when the binary is named otherwise) and
installs it as `/usr/local/bin/agent`, so it needs no edit. What a copy into
a project of its own does need:

1. **Both files beside each other:** `Dockerfile` and
   `Dockerfile.dockerignore`. Without the second, Docker sends the whole
   directory, `.env` files, keys and `.git` included, into every build stage
   that copies the tree.
2. **A committed `Cargo.lock`**, because the build runs `--locked`.
3. **Dependencies from crates.io**, not the workspace: replace
   `a2a-protocol-sdk = { path = ... }` with a version, and the
   `*.workspace = true` keys (`edition`, `rust-version`, `license`, `tokio`,
   `tracing`) with values; cargo refuses a manifest that inherits from a
   workspace it is not in. `deploy-agent` needs 0.16.0 or later
   (`ServeConfig::with_drain_delay`, `new_with_pool_size`).
4. **The release profile** if you want the same binary: the workspace's
   `[profile.release]` (link-time optimisation, one codegen unit, stripped)
   is not copied with the crate.
5. **Your own `/health`** on `PORT`, which the health check and the
   manifest's probes call; the HTTP+JSON dispatcher, `CombinedDispatcher` and
   the axum router serve it, the JSON-RPC dispatcher alone does not.

Then, from your project's root:

```bash
docker build --build-arg PACKAGE=my-agent -t my-agent .
```

With telemetry, start the stack in `deploy/observability`
(`docker compose -f deploy/observability/docker-compose.yml up -d`) and join
its network. On that network the Collector is `otel-collector`, not
`localhost`, which inside a container is the container itself:

```bash
docker run --rm -p 8080:8080 --network a2a-observability_default \
  -e AGENT_URL=http://localhost:8080 \
  -e OTEL_SERVICE_NAME=deploy-agent \
  -e OTEL_EXPORTER_OTLP_ENDPOINT=http://otel-collector:4318 \
  deploy-agent
```

## Running it on Kubernetes

```bash
kubectl create secret generic deploy-agent-db \
  --from-literal=url='postgres://USER:PASSWORD@HOST:5432/DB?sslmode=verify-full&sslrootcert=/path/ca.pem'
kubectl apply -f examples/deploy-agent/deployment.yaml
```

Keep the `sslmode` in that URL. The driver's default, `prefer`, falls back to
plaintext and does not check who answered; `verify-full` checks the server's
certificate and name. `sslrootcert` is the CA file as the pod sees it, so mount
it there.

Two edits first, and one for an image of your own:

- **The image.** From 0.16.0 the manifest names the published image,
  `ghcr.io/tomtom215/a2a-rust/deploy-agent:0.16.0`, with
  `imagePullPolicy: IfNotPresent`; nothing to set. (Until 2026-11-01 it named
  `deploy-agent:latest`, which Kubernetes pulls from Docker Hub, where nothing
  was published.) For a build of your own, push it to a registry the cluster
  can reach and set `image:` to it, or on kind run
  `kind load docker-image deploy-agent:dev` and set `image: deploy-agent:dev`.
- **`AGENT_URL`.** The manifest sets the in-cluster Service URL. Set it to the
  URL callers will use: it is what goes on the agent card, and it cannot be
  derived from inside the pod. A card that advertises the bind address sends
  clients to an address they cannot reach.
- **The database.** Two replicas take `A2A_DATABASE_URL` from the Secret
  `deploy-agent-db`. The entry is not optional, on purpose: a pod without it
  fails with `CreateContainerConfigError` rather than starting on in-memory
  stores, where each replica answers `TaskNotFound` for the tasks the other
  created. For one replica and no database, delete the entry and set
  `replicas: 1`.

What each part of the manifest is for:

| Field | Value | Why |
|---|---|---|
| `replicas` | `2` | Both share PostgreSQL, whose turn claims let one pod at a time run a task, so the `Service` needs no session affinity ([Running More Than One Replica](./horizontal-scaling.md)) |
| `strategy` | `RollingUpdate`, `maxSurge: 1`, `maxUnavailable: 0` | A new pod is ready before an old one goes |
| `terminationGracePeriodSeconds` | `60` | Must exceed the shutdown budget in `main.rs` plus the telemetry flush, or `SIGKILL` cuts the shutdown short ([Graceful shutdown](./production/graceful-shutdown.md)) |
| `startupProbe` | `GET /health` every 2 s, 45 tries | The port opens once the database has answered; liveness waits for it, so a slow first connection is not killed as a hang |
| `livenessProbe` | `GET /health` every 10 s | Checks nothing, so a database outage does not become a restart loop ([Health checks](./health-checks.md)) |
| `readinessProbe` | `GET /ready` every 5 s | `503` while the task store fails its health check or the pod is draining |
| `DRAIN_DELAY_SECONDS` | `5` | After `SIGTERM`, not ready and still serving while the endpoints and proxies learn the pod is leaving |
| `automountServiceAccountToken` | `false` | The agent calls no Kubernetes API |
| `topologySpreadConstraints` | by node and by zone, `ScheduleAnyway` | Two replicas on one node are one failure |
| `securityContext` (pod) | `runAsNonRoot: true`, `runAsUser: 10001`, `seccompProfile: RuntimeDefault` | Matches the image's user; Pod Security "restricted" refuses a pod without the seccomp profile |
| `securityContext` (container) | `allowPrivilegeEscalation: false`, `readOnlyRootFilesystem: true`, `capabilities.drop: ["ALL"]` | The agent needs no privileges and writes nothing to the image |
| `resources` | requests `cpu: 100m`, `memory: 64Mi`; limit `memory: 256Mi` | The example's values; size your own agent from its metrics |
| `TOKIO_WORKER_THREADS`, `_RJEM_MALLOC_CONF` | `2`; `narenas:2,background_thread:true` | Without them the runtime starts a worker per CPU of the *node*, and jemalloc sizes its arenas from the node's CPUs even under a CPU limit (it reads the CPU affinity, not the cgroup quota), so a pod's memory grows with the node it lands on. Raise both with the pod's CPU (audit A26) |
| `env` | `PORT`, `AGENT_URL`, `DRAIN_DELAY_SECONDS`, `TOKIO_WORKER_THREADS`, `_RJEM_MALLOC_CONF`, `A2A_DATABASE_URL`, and commented `OTEL_*` entries | Uncomment the telemetry entries to export to an in-cluster Collector |
| `Service` | port 80 to the container's `http` port | No session affinity |
| `PodDisruptionBudget` | `maxUnavailable: 1` | A node drain takes one replica at a time |

The commented telemetry entries set `OTEL_EXPORTER_OTLP_ENDPOINT` to a
Collector's OTLP/HTTP port (4318; port 4317 is gRPC and also needs
`OTEL_EXPORTER_OTLP_PROTOCOL=grpc`) and put the pod's name in
`OTEL_RESOURCE_ATTRIBUTES` as `service.instance.id`, which is what tells the
replicas apart in the metrics; without it each process makes up a random one.

## What is not included

Stated because a deployment example that quietly omits these reads as an
endorsement:

- **TLS.** Terminated at an ingress or a service mesh in front of the pods.
  The server can terminate it itself ([HTTPS](./production.md#https)), but
  the example does not show a certificate source or a reload path.
- **Authentication.** No interceptor is installed, so every request is
  anonymous. See [Authentication](../building-agents/authentication.md).
- **The database.** `A2A_DATABASE_URL` names a PostgreSQL you run; its backups,
  upgrades and connection limits are yours.
- **Network policy, ingress and scraping, applied by default.**
  `examples/deploy-agent/network.yaml` has a `NetworkPolicy` that lets only
  the ingress controller's namespace reach port 8080 (the agent is
  unauthenticated) and an `Ingress` that ends TLS for the host `AGENT_URL`
  names, with buffering off and a one-hour read timeout for streams; edit the
  two marked values and apply it after the manifest. Egress is not
  restricted. There is no `ServiceMonitor`: the binary is built without
  `otel-prometheus`, so its metrics reach Prometheus through a Collector.
- **Telemetry backends in the cluster.** `deploy/observability` is a Docker
  Compose stack; a cluster's Collector and backends are specific to the
  cluster.
- **Packaging.** No Helm chart and no operator; the one published image is
  `deploy-agent`'s, as above.
