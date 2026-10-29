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
| `examples/deploy-agent/Dockerfile` | A multi-stage image for `deploy-agent`: build with the Rust toolchain, ship on Debian slim without it |
| `examples/deploy-agent/deployment.yaml` | A Kubernetes `Deployment` (two replicas) and `Service` for that image |
| `deploy/observability/docker-compose.yml`, `docker-compose.prod.yml` | A telemetry stack (OpenTelemetry Collector, Prometheus, Tempo, Loki, Grafana) for a laptop or a test box; see [Observability](./observability.md) |
| `itk/docker-compose.yml`, `itk/Dockerfile.*`, `itk/agents/*/Dockerfile` | The integration test kit: agents built with the Python, JavaScript, Go and Java SDKs and this SDK's `echo-agent`, run against each other. Test infrastructure, not a template for deploying |

There are no Helm charts, no Kustomize overlays and no published container
images. The workflows that build images (`acts.yml`, `itk-nightly.yml`) load
them into the runner for a test and never push them. Copy the two
`deploy-agent` files into your own project and adapt them.

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
  to run, each pinned by digest. Debian slim rather than distroless or
  scratch, because the binary links glibc.
- **No certificate store needed by the agent.** The SDK verifies TLS (push
  notifications, the client, the telemetry exporters) against its bundled
  Mozilla roots and never reads the image's store; `ca-certificates` is
  installed for tools run inside the container. A private CA reaches the
  agent through the SDK's CA options, and reaches telemetry through
  `OTEL_EXPORTER_OTLP_CERTIFICATE` (a mounted PEM).
- **Unprivileged.** It runs as uid 10001; many cluster admission policies
  refuse a container that runs as root.
- **The binary is PID 1.** `ENTRYPOINT` is in exec form, so the process
  receives `SIGTERM` directly. The shell form (`CMD deploy-agent`) puts
  `/bin/sh` at PID 1, which does not forward signals: graceful shutdown stops
  working and every rollout cuts streams mid-flight.
- **`PORT`.** `EXPOSE 8080` documents the port and `ENV PORT=8080` sets it;
  the process reads `PORT` and defaults to 8080. Keep the two in step.

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
  --from-literal=url='postgres://USER:PASSWORD@HOST:5432/DB'
kubectl apply -f examples/deploy-agent/deployment.yaml
```

Three edits first:

- **The image.** The manifest names `deploy-agent:latest` and sets no
  `imagePullPolicy`, and Kubernetes always pulls a `:latest` image, so a
  locally built one is not used. Push the image to a registry the cluster can
  reach and set `image:` to it, or on kind run
  `kind load docker-image deploy-agent` and set
  `imagePullPolicy: IfNotPresent`.
- **`AGENT_URL`.** The manifest sets the in-cluster Service URL. Set it to the
  URL callers will use: it is what goes on the agent card, and it cannot be
  derived from inside the pod. A card that advertises the bind address sends
  clients to an address they cannot reach.
- **The database.** Two replicas take `DATABASE_URL` from the Secret
  `deploy-agent-db`. The entry is not optional, on purpose: a pod without it
  fails with `CreateContainerConfigError` rather than starting on in-memory
  stores, where each replica answers `TaskNotFound` for the tasks the other
  created. For one replica and no database, delete the entry and set
  `replicas: 1`.

What each part of the manifest is for:

| Field | Value | Why |
|---|---|---|
| `replicas` | `2` | Both share PostgreSQL, whose turn claims let one pod at a time run a task, so the `Service` needs no session affinity ([Running More Than One Replica](./horizontal-scaling.md)) |
| `terminationGracePeriodSeconds` | `45` | Must exceed the shutdown budget in `main.rs` plus the telemetry flush, or `SIGKILL` cuts the shutdown short ([Graceful shutdown](./production/graceful-shutdown.md)) |
| `livenessProbe` | `GET /healthz` every 10 s | Checks nothing, so a database outage does not become a restart loop ([Health checks](./health-checks.md)) |
| `readinessProbe` | `GET /readyz` every 5 s | Reaches the task store, so a pod that cannot stops receiving traffic |
| `securityContext` (pod) | `runAsNonRoot: true`, `runAsUser: 10001` | Matches the image's user |
| `securityContext` (container) | `allowPrivilegeEscalation: false`, `readOnlyRootFilesystem: true`, `capabilities.drop: ["ALL"]` | The agent needs no privileges and writes nothing to the image |
| `resources` | requests `cpu: 50m`, `memory: 64Mi`; limit `memory: 256Mi` | The example's values; size your own agent from its metrics |
| `env` | `PORT`, `AGENT_URL`, `DATABASE_URL`, and commented `OTEL_*` entries | Uncomment the telemetry entries to export to an in-cluster Collector |
| `Service` | port 80 to the container's `http` port | No session affinity |

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
- **The database.** `DATABASE_URL` names a PostgreSQL you run; its backups,
  upgrades and connection limits are yours.
- **Telemetry backends in the cluster.** `deploy/observability` is a Docker
  Compose stack; a cluster's Collector and backends are specific to the
  cluster.
- **Packaging.** No Helm chart, operator or published image, as above.
