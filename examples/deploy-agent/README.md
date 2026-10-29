<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# deploy-agent

The smallest A2A agent you can actually ship.

[`hello-agent`](../hello-agent) is the smallest thing that answers A2A — one
screen of code, no configuration, bound to `127.0.0.1:3000`. It is the right
first example and the wrong thing to deploy. This is the same agent with what
a container platform requires, plus a jemalloc allocator and OTLP telemetry
from the `OTEL_*` environment, and nothing else.

| Concern | Here | Why examples never show it |
|---|---|---|
| Configuration | `PORT`, `AGENT_URL`, `DATABASE_URL`, `DRAIN_DELAY_SECONDS` from the environment, checked before anything binds | Examples hardcode a port; a scheduler assigns one |
| Health checks | `GET /health` (liveness: a constant), `GET /ready` (readiness: the task store answers and the process is not draining) | Nothing probes an example, so nobody notices there is no probe endpoint |
| Graceful shutdown | `SIGTERM`: not ready at once, still serving for `DRAIN_DELAY_SECONDS`, then in-flight tasks end, then connections drain, each step bounded | Examples die to Ctrl-C and nobody minds the truncated stream |
| Bind address | every address: `[::]`, IPv6 and IPv4, or `0.0.0.0` where the host has no IPv6 | A loopback bind works locally and is unreachable in a container |
| Timeouts | the SDK's `Server`: a 30 s header-read and a 75 s idle timeout on every connection | `axum::serve`, which this example used until 2026-10-29, has neither: a client sending a byte a minute holds a connection forever |

The agent logic is deliberately trivial. Everything worth reading is the
wrapper, because the wrapper is what is missing when someone takes an example
to production and finds it answers only to itself.

## Run it

```sh
cargo run -p deploy-agent
curl localhost:8080/health
curl localhost:8080/ready
curl localhost:8080/.well-known/agent-card.json

curl -X POST localhost:8080/message:send \
  -H 'content-type: application/json' \
  -H 'A2A-Version: 1.0' \
  -d '{"message":{"messageId":"m1","role":"ROLE_USER","parts":[{"text":"Ada"}]}}'
```

`CombinedDispatcher` serves both HTTP bindings at the one URL: **HTTP+JSON**
(`/message:send`, `/tasks`, …) and **JSON-RPC** on `/`, and the agent card
advertises both. A card advertising a binding the process does not serve
fails only when a client calls it, with a 404 (a test pins the two together).

Configuration:

| Variable | Default | Notes |
|---|---|---|
| `PORT` | `8080` | Rejected at startup if it is not a port from 1 to 65535 — a process that ignores its configuration is worse than one that refuses to start |
| `AGENT_URL` | `http://localhost:$PORT` | The **externally reachable** URL, `http://` or `https://`. Goes on the agent card |
| `DRAIN_DELAY_SECONDS` | `0` | After `SIGTERM`, how long to answer `/ready` with 503 and go on serving before stopping, while load balancers learn the pod is leaving. `deployment.yaml` sets 5 |
| `DATABASE_URL` | unset: tasks in memory | A PostgreSQL URL. Tasks and push configs go there, which is what lets several replicas serve the agent; unset, run one replica (the process says so at startup). TLS to the database: `?sslmode=verify-full&sslrootcert=/path/ca.pem` (or `require`), as hosted PostgreSQL documents |
| `OTEL_EXPORTER_OTLP_ENDPOINT` | unset: nothing exported | A collector's OTLP/HTTP URL, e.g. `http://localhost:4318` for [`deploy/observability`](../../deploy/observability)'s stack; traces, metrics and logs all go there |

## Container

```sh
# From the repository root — the build needs the workspace.
docker build -f examples/deploy-agent/Dockerfile -t deploy-agent .
docker run --rm -p 8080:8080 -e AGENT_URL=http://localhost:8080 deploy-agent
```

The dependencies are built in a layer of their own (cargo-chef), which Docker
reuses until a manifest or `Cargo.lock` changes: after a change to the source
only, a rebuild took 221 s against 428 s from nothing, most of it compiling the
workspace's own crates with link-time optimisation.

With telemetry, to the stack in [`deploy/observability`](../../deploy/observability)
(`docker compose -f deploy/observability/docker-compose.yml up -d` first): on
its network the Collector is `otel-collector`, not `localhost`, which inside a
container is the container itself.

```sh
docker run --rm -p 8080:8080 --network a2a-observability_default \
  -e AGENT_URL=http://localhost:8080 \
  -e OTEL_SERVICE_NAME=deploy-agent \
  -e OTEL_EXPORTER_OTLP_ENDPOINT=http://otel-collector:4318 \
  deploy-agent
```

Two stages: build with the toolchain, ship without it. Runs as uid 10001 with
no shell in the entrypoint, so the binary is PID 1 and receives `SIGTERM`
directly — the shell form puts `/bin/sh` there, which does not forward signals,
and graceful shutdown silently stops working.

## Kubernetes

```sh
kubectl apply -f examples/deploy-agent/deployment.yaml
```

Three edits first (`kubectl apply` as shipped fails twice: the Secret is
missing, then the image cannot be pulled):

- **The image.** The manifest names `deploy-agent:latest` and sets no
  `imagePullPolicy`, and Kubernetes pulls a `:latest` image by default, so a
  locally built image is not used. Push it to a registry the cluster can reach
  and set `image:` to it, or on kind run `kind load docker-image deploy-agent`
  and set `imagePullPolicy: IfNotPresent`.
- **`AGENT_URL`.** The manifest sets the in-cluster Service URL. Set it to the
  URL callers will use (see the trap below).
- **The database.** The manifest runs two replicas and takes `DATABASE_URL`
  from the Secret `deploy-agent-db`, which you create:
  `kubectl create secret generic deploy-agent-db --from-literal=url='postgres://USER:PASSWORD@HOST:5432/DB'`.
  A pod without it does not start (`CreateContainerConfigError`), on purpose:
  two replicas on in-memory stores answer `TaskNotFound` for every task the
  other one created. For one replica and no database, delete the entry and
  set `replicas: 1`.

Two replicas on one database need no session affinity: each turn of a task
takes a claim in the database, so a continuation that reaches the other pod
waits for the running turn rather than starting a second one, a cancel taken
by either pod stops the executor wherever it runs, and a pod that dies
mid-turn holds its tasks for 15 s, after which the other takes them over
([Running More Than One Replica](https://a2a-rust.com/deployment/horizontal-scaling.html)).

`deployment.yaml` wires a startup probe (the port opens only once the
database has answered, which a cold database can take tens of seconds to do),
liveness to `/health` and readiness to `/ready`; a rolling update that adds a
pod before removing one; a PodDisruptionBudget; spreading across nodes and
zones; the Pod Security "restricted" settings; and
`terminationGracePeriodSeconds: 60`. That number must exceed the shutdown
budget: `DRAIN_DELAY_SECONDS` (5), up to 5 s for tasks to end on their own,
then cancellation and up to 10 s for the executors to act on it, up to 10 s
of connection drain, up to 5 s for the cleanup hook and the release of the
turn claims, then, with telemetry on, the flush as `main` returns: up to 15 s
when the collector accepts connections and never answers, 1.8 s when it
refuses them (measured on `hello-agent`, 2026-10-26; `OTEL_EXPORTER_OTLP_TIMEOUT`
shortens the first). A task still running 5 s after the drain delay is
stopped, however long its stream would have been, and ends `failed` with
failure class `transient`, telling its caller it may send again; a send that
arrives after the drain delay is refused with a `503` and `Retry-After`, so
a client with a retry policy sends it to the other replica. If `SIGKILL` lands
before that finishes, the turn claims are left to expire and the graceful
shutdown is decorative.

## The trap this example exists to prevent

**The agent card must advertise the public URL, not the bind address.**

A container binds `0.0.0.0:8080`; callers reach it at
`https://agent.example.com`. Publish the bind address on the card and clients
read it, call it, and fail — with an error that points at the client rather than
at the card. It cannot be derived from inside the process, which is why
`AGENT_URL` is separate from `PORT`.

There is a test for exactly this
(`agent_card_advertises_the_public_url_not_the_bind_address`), asserting both
that the card carries the configured URL *and* that it does not leak
`127.0.0.1`.

## Tests

```sh
cargo test -p deploy-agent
```

Seven. Five drive the real server over a real socket rather than calling the
handler directly — the point of this example is the wiring, and asserting on
the handler would skip precisely the part that can be wrong. Two check that a
configuration it cannot use is refused and that the database URL is logged
without its password.

## Allocator

The binary runs on jemalloc (`tikv-jemallocator`), not glibc's allocator.
A long-running server's bounded task store keeps a fixed-size heap full of
freed holes, and glibc's allocator is slow there: on the echo benchmark
jemalloc took unary throughput from 7,189 to 11,862 requests/s at the same
resident memory. The book's production chapter has the measurement.

To confirm a running build uses it, start it with
`_RJEM_MALLOC_CONF=stats_print:true`; jemalloc prints its statistics when
the process exits. Windows (MSVC) builds keep the system allocator, which
`tikv-jemalloc-sys` does not replace there.

## What this does not cover

Stated rather than implied, because a deployment example that quietly omits
these reads as an endorsement:

- **TLS.** Terminated at an ingress or service mesh here. An agent doing its own
  TLS needs a certificate source and a reload path, neither of which is shown.
- **Authentication.** No interceptor is installed, so every request is
  anonymous. See [Authentication](https://a2a-rust.com/building-agents/authentication.html)
  and the `harden` act of [`incident-response`](../incident-response).
- **The database itself.** `DATABASE_URL` names a PostgreSQL you run; its
  backups, upgrades and connection limits are yours. Without it the store is
  in memory: tasks do not survive a restart, and the agent must run as one
  replica.
- **The observability stack in Kubernetes.** With `OTEL_EXPORTER_OTLP_ENDPOINT`
  set (or a per-signal endpoint) the binary sends traces, metrics and logs to
  that collector; with none set nothing is built. `deploy/observability` has a
  Docker Compose stack (Collector, Prometheus, Tempo, Loki, Grafana with the
  dashboards) for a laptop or a test box; a cluster's collector and backends
  are deployment-specific.
