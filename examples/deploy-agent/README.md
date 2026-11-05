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
| Configuration | `PORT`, `AGENT_URL`, `A2A_DATABASE_URL`, `DRAIN_DELAY_SECONDS` from the environment, checked before anything binds | Examples hardcode a port; a scheduler assigns one |
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
| `A2A_DATABASE_URL` | unset: tasks in memory | A PostgreSQL URL. Tasks and push configs go there, which is what lets several replicas serve the agent; unset, run one replica (the process says so at startup). TLS to the database: `?sslmode=verify-full&sslrootcert=/path/ca.pem` (or `require`), as hosted PostgreSQL documents. A schema of its own: `&options=-c%20search_path%3Da2a` (below) |
| `DATABASE_URL` | unset | Read only when `A2A_DATABASE_URL` is unset, because platforms (Heroku, Render, Railway, Fly) inject it; the start then warns, since sqlx-cli, Diesel, Rails and Django read the same name and a shell that exports it for another application would otherwise send this agent's tables there (audit A14, [ADR 0040](../../docs/adr/0040-first-run-configuration-and-scope.md)). Until 0.16.0 it was the only name |
| `AGENT_VERSION` | this crate's own version | What the agent card reports as its `version`, so rollouts can be told apart from the card alone. The published image sets it to the release; refused at startup if it contains whitespace |
| `TASK_RETENTION_HOURS` | `168` | With a database, how long a finished task (completed, failed, canceled, rejected) is kept after its last update: the agent deletes older ones, and the idempotency keys that outlived them, every hour, in bounded batches, from every replica (safe: each batch picks its own rows). A task still working or waiting for input is never deleted. `0` keeps everything, and says so at startup: then schedule your own `PostgresTaskStore::purge_expired`. Until 2026-11-01 nothing deleted, so a database-backed deployment grew without bound where the in-memory one kept an hour. Refused at startup unless a whole number of hours |
| `DATABASE_POOL_SIZE` | `10` | Connections in the task store's pool; a replica uses this plus `DATABASE_PUSH_POOL_SIZE`, logged at startup. Replicas × that, with a surge pod during a rollout, must stay under PostgreSQL's `max_connections` (97 usable by default). Refused at startup unless a whole number from 1 |
| `DATABASE_PUSH_POOL_SIZE` | `2` | Connections in the push-config store's pool, which reads a task's configs once per delivery. A pool of its own, not a share of the task store's: the task store's pool skips a ping that the push-config store's statements need (`PostgresTaskStore::pool_options`). `/ready` probes the task store's pool only: both reach the same database, and a push pool that is merely busy should not take the replica out of the balancer. Refused at startup unless a whole number from 1 |
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
and graceful shutdown silently stops working. A `HEALTHCHECK` (`GET /health`
over bash's `/dev/tcp`; the image has no curl) gives `docker ps` and Compose a
health status; Kubernetes uses the manifest's probes instead. The build context
is what [`Dockerfile.dockerignore`](Dockerfile.dockerignore) leaves in: no
secrets or history, and not the book or the docs, whose edits no longer
rebuild the agent. `--build-arg PACKAGE=my-agent` builds another crate with
the same file (the book's *Docker and Kubernetes* page lists what a copy
needs).

## Kubernetes

```sh
kubectl create secret generic deploy-agent-db \
  --from-literal=url='postgres://USER:PASSWORD@HOST:5432/DB?sslmode=verify-full&sslrootcert=/path/ca.pem'
kubectl apply -f examples/deploy-agent/deployment.yaml
# Optional: only the ingress controller may reach the pods, and an Ingress with
# TLS for AGENT_URL's host. Edit the two marked values first.
kubectl apply -f examples/deploy-agent/network.yaml
```

The manifest names the image the release workflow publishes,
`ghcr.io/tomtom215/a2a-rust/deploy-agent:0.16.0` (from 0.16.0 on; tagged with
the release version only, there is no `latest`), with
`imagePullPolicy: IfNotPresent`. Until 2026-11-01 it named `deploy-agent:latest`,
which Kubernetes pulls from Docker Hub, where nothing was published, so
`kubectl apply` as shipped failed twice: the Secret was missing, then the
image could not be pulled. Two things to set:

- **`AGENT_URL`.** The manifest sets the in-cluster Service URL. Set it to the
  URL callers will use (see the trap below).
- **The database.** The manifest runs two replicas and takes `A2A_DATABASE_URL`
  from the Secret `deploy-agent-db`, which the first command creates.
  Keep the `sslmode`: the driver's default, `prefer`, falls back to plaintext
  and does not check who answered, while `verify-full` checks the server's
  certificate and name. `sslrootcert` is the CA file as the pod sees it, so
  mount it there. A pod without the Secret does not start (`CreateContainerConfigError`), on purpose:
  two replicas on in-memory stores answer `TaskNotFound` for every task the
  other one created. For one replica and no database, delete the entry and
  set `replicas: 1`.

For an image of your own, build it as above, then push it to a registry the
cluster can reach and set `image:` to it, or on kind run
`kind load docker-image deploy-agent:dev` (after `docker build ... -t
deploy-agent:dev`) and set `image: deploy-agent:dev`. A built-from-source
image reports the crate's version on its agent card; the published one
reports the release it was built for (`AGENT_VERSION`).

### A schema of its own

The stores create unprefixed tables (`tasks`, `task_events`,
`idempotency_keys`, `push_configs`, `schema_versions`, beside the prefixed
`a2a_*` ones) in the first schema of the connection's `search_path`. In a
database another application also uses, give the agent a schema and put it on
the path in the URL:

```sql
CREATE SCHEMA a2a AUTHORIZATION agent;
```

```text
postgres://agent:PASSWORD@HOST:5432/DB?sslmode=verify-full&options=-c%20search_path%3Da2a
```

Every table, index and function the agent creates then lands in `a2a`, and
`search_path=a2a,public` works as well beside another application's
`public.tasks`. Until 0.16.0 that second form did not: the start looked for
`tasks` along the whole path, found the other application's, and failed
("foreign key constraint ... cannot be implemented") or wrote into it (audit
NEW-a-l-2; `pg_schema::search_path_tests` pins both forms).

### Verify the image

Each published image carries a Sigstore-signed SLSA build-provenance
attestation, stored beside it in the registry, and BuildKit's SBOM and
provenance in the image index:

```sh
gh attestation verify oci://ghcr.io/tomtom215/a2a-rust/deploy-agent:0.16.0 \
  --repo tomtom215/a2a-rust
```

The release workflow runs the same command on the image it has just pushed
and fails if it cannot verify it. The attestation says which workflow, at
which commit, built that digest; it does not say the code is free of defects.

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

A container binds every address (`[::]:8080`); callers reach it at
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

`deployment.yaml` sets `_RJEM_MALLOC_CONF=narenas:2,background_thread:true`
and `TOKIO_WORKER_THREADS=2`: without them both follow the CPUs of the node,
not of the pod, and jemalloc does so even under a CPU limit. Raise them with
the CPU the pod is given.

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
- **The database itself.** `A2A_DATABASE_URL` names a PostgreSQL you run; its
  backups, upgrades and connection limits are yours. A server certificate
  signed by a CA outside Mozilla's roots (an internal CA, or a provider's own)
  needs `sslrootcert` and the CA file mounted in the pod; `deployment.yaml`
  carries the ConfigMap volume as a commented example. Without it the store is
  in memory: tasks do not survive a restart, and the agent must run as one
  replica.
- **Network policy, ingress and scraping, by default.** `deployment.yaml`
  ships none of them; [`network.yaml`](network.yaml) has a `NetworkPolicy`
  admitting only the ingress controller's namespace to port 8080 and an
  `Ingress` ending TLS for `AGENT_URL`'s host, to edit (two marked values) and
  apply after it. Egress is not restricted. There is no `ServiceMonitor`:
  metrics reach a Prometheus Operator only through a Collector,
  because this binary is built without `otel-prometheus` (asking for it makes
  the process start without it and say so at `WARN`).
- **The observability stack in Kubernetes.** With `OTEL_EXPORTER_OTLP_ENDPOINT`
  set (or a per-signal endpoint) the binary sends traces, metrics and logs to
  that collector; with none set nothing is built. `deploy/observability` has a
  Docker Compose stack (Collector, Prometheus, Tempo, Loki, Grafana with the
  dashboards) for a laptop or a test box; a cluster's collector and backends
  are deployment-specific.
