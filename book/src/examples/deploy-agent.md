# Deploy Agent

The smallest A2A agent you can actually ship.

[`hello-agent`](./hello-agent.md) is the smallest thing that answers A2A: one
screen of code, no configuration, bound to `127.0.0.1:3000`. It is the right
first example and the wrong thing to deploy. `deploy-agent` is the same agent
with the four things a container platform requires, plus a jemalloc allocator,
an optional PostgreSQL store for running more than one replica, and OTLP
telemetry from the `OTEL_*` environment, and nothing else.

**Source:** [`examples/deploy-agent/`](https://github.com/tomtom215/a2a-rust/tree/main/examples/deploy-agent)

## What it shows

| Concern | Here | Why examples never show it |
|---|---|---|
| Configuration | `PORT`, `AGENT_URL` from the environment | Examples hardcode a port; a scheduler assigns one |
| Health checks | `GET /healthz`, `GET /readyz` | Nothing probes an example, so nobody notices there is no probe endpoint |
| Graceful shutdown | `SIGTERM`/`SIGINT` end in-flight tasks, then drain connections | Examples die to Ctrl-C and nobody minds the truncated stream |
| Bind address | `0.0.0.0` | A loopback bind works locally and is unreachable in a container |
| Replicas | `DATABASE_URL` puts tasks and push configs in PostgreSQL | Behind a load balancer, a task created on one replica's memory is not found on the next |

The agent logic is deliberately trivial. Everything worth reading is the
wrapper, because the wrapper is what is missing when someone takes an example
to production and finds it answers only to itself.

## Run it

Locally it needs nothing but Rust; Docker, Kubernetes and PostgreSQL are for
the deployment steps further down.

```bash
cargo run -p deploy-agent
curl localhost:8080/healthz
curl localhost:8080/.well-known/agent-card.json

curl -X POST localhost:8080/message:send \
  -H 'content-type: application/json' \
  -H 'A2A-Version: 1.0' \
  -d '{"message":{"messageId":"m1","role":"ROLE_USER","parts":[{"text":"Ada"}]}}'
```

`A2aRouter` serves the **HTTP+JSON** binding (`/message:send`, `/tasks`, …),
which is what the agent card advertises. It is not JSON-RPC on `/`.

| Variable | Default | Notes |
|---|---|---|
| `PORT` | `8080` | Rejected at startup if unparseable |
| `AGENT_URL` | `http://localhost:$PORT` | The **externally reachable** URL; goes on the agent card |
| `DATABASE_URL` | unset: tasks in memory | A PostgreSQL URL. Tasks and push configs go there, which is what lets several replicas serve the agent; unset, run one replica |
| `OTEL_EXPORTER_OTLP_ENDPOINT` | unset: nothing exported | A collector's OTLP/HTTP URL, such as `http://localhost:4318`; traces, metrics and logs all go there |

An unparseable `PORT` stops the process rather than falling back to the
default. A process that ignores its configuration is worse than one that
refuses to start, because the misconfiguration survives the deploy.

## What you'll see

`main.rs` prints where it is listening and what it advertises. With the
defaults those lines read as follows, and the second goes to standard error
because `DATABASE_URL` is unset:

```text
deploy-agent listening on 0.0.0.0:8080 (advertising http://localhost:8080)
warning: DATABASE_URL is unset: tasks are kept in this process's memory, so run one replica, or set DATABASE_URL to a PostgreSQL database every replica shares
```

`/healthz` answers `ok` and `/readyz` answers `ready` (or `503 not ready` when
the task store does not answer). On `SIGTERM` or Ctrl-C it prints
`SIGTERM received, draining` or `SIGINT received, draining`, and
`drained cleanly, exiting` when the shutdown was graceful.

## Key code

The probes are two routes beside the A2A ones. `/healthz` checks nothing on
purpose: a liveness probe with a dependency turns that dependency's outage
into a restart loop. `/readyz` asks the task store
(`RequestHandler::task_store_health`), so a replica that cannot persist stops
taking traffic.

Shutdown is where the order matters. In-flight work lives in the handler, not
in axum, and has to end *before* axum drains, because an open SSE stream is a
connection that stays open until its task ends:

```rust,no_run
# use std::sync::Arc;
# use std::time::Duration;
# use a2a_protocol_sdk::prelude::*;
# async fn shutdown_signal() {}
# async fn run(
#     listener: tokio::net::TcpListener,
#     router: axum::Router,
#     handler: Arc<RequestHandler>,
# ) -> Result<(), Box<dyn std::error::Error>> {
let tasks = Arc::clone(&handler);
axum::serve(listener, router)
    .with_graceful_shutdown(async move {
        shutdown_signal().await;
        let report = tasks
            .finish_in_flight(Duration::from_secs(5), Duration::from_secs(10))
            .await;
        if !report.finished {
            eprintln!(
                "{} of {} task(s) did not end within the grace period",
                report.still_running, report.cancelled
            );
        }
    })
    .await?;

// Then the executor's cleanup hook, with its outcome reported.
let report = handler.shutdown_with_timeout(Duration::from_secs(5)).await;
if report.is_graceful() {
    println!("drained cleanly, exiting");
}
# Ok(())
# }
```

`finish_in_flight` gives tasks 5 s to end on their own, then fires their
cancellation tokens and gives the executors 10 s to act on it. Only then does
the future resolve and axum stop accepting and drain.

## The two traps

Both were found by running this example rather than by reading it, and both now
have tests.

**The card must advertise the public URL, not the bind address.** A container
binds `0.0.0.0:8080`; callers reach it at `https://agent.example.com`. Publish
the bind address and clients read the card, call it, and fail — with an error
that points at the client. The process cannot derive this, which is why
`AGENT_URL` is separate from `PORT`. The test
`agent_card_advertises_the_public_url_not_the_bind_address` asserts both that
the card carries the configured URL and that it does not leak `127.0.0.1`.

**The card must advertise the binding the process actually serves.**
`A2aRouter` serves HTTP+JSON — `/message:send`, `/tasks` — not JSON-RPC on `/`.
This example first shipped advertising `JSONRPC`, which looked correct
everywhere except in a client's 404.

## Container

```bash
# Run from the repository root; the build needs the workspace.
docker build -f examples/deploy-agent/Dockerfile -t deploy-agent .
docker run --rm -p 8080:8080 -e AGENT_URL=http://localhost:8080 deploy-agent
```

Two stages: build with the toolchain, ship without it. The dependencies are
built in a layer of their own (cargo-chef), which Docker reuses until a
manifest or `Cargo.lock` changes: after a change to the source only, a rebuild
took 221 s against 428 s from nothing. The runtime image is Debian slim and
runs as uid 10001.

The `ENTRYPOINT` is exec form, so the binary is PID 1 and receives `SIGTERM`
directly. The shell form puts `/bin/sh` there, which does not forward signals
— graceful shutdown silently stops working and rollouts start cutting streams.

To send telemetry to the stack in `deploy/observability` (start it first with
`docker compose -f deploy/observability/docker-compose.yml up -d`), join its
network. There the Collector is `otel-collector`, not `localhost`, which
inside a container is the container itself:

```bash
docker run --rm -p 8080:8080 --network a2a-observability_default \
  -e AGENT_URL=http://localhost:8080 \
  -e OTEL_SERVICE_NAME=deploy-agent \
  -e OTEL_EXPORTER_OTLP_ENDPOINT=http://otel-collector:4318 \
  deploy-agent
```

## Kubernetes

```bash
kubectl apply -f examples/deploy-agent/deployment.yaml
```

<figure class="diagram">
<a href="../images/diagrams/ex-deploy-topology.light.svg"><img class="diagram-light" src="../images/diagrams/ex-deploy-topology.light.svg" alt="Callers reach a Service on port 80, which balances across two deploy-agent pods listening on 0.0.0.0:8080; the kubelet probes each pod's /healthz and /readyz, and both pods share one PostgreSQL database."></a>
<a href="../images/diagrams/ex-deploy-topology.dark.svg"><img class="diagram-dark" src="../images/diagrams/ex-deploy-topology.dark.svg" alt=""></a>
<figcaption>What <code>deployment.yaml</code> sets up: two replicas, two probes, one shared store.</figcaption>
</figure>

Three edits first:

- **The image.** The manifest names `deploy-agent:latest` and sets no
  `imagePullPolicy`, and Kubernetes pulls a `:latest` image by default, so a
  locally built image is not used. Push it to a registry the cluster can reach
  and set `image:` to it, or on kind run `kind load docker-image deploy-agent`
  and set `imagePullPolicy: IfNotPresent`.
- **`AGENT_URL`.** The manifest sets the in-cluster Service URL
  (`http://deploy-agent.default.svc.cluster.local`). Set it to the URL callers
  will use.
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
([Running More Than One Replica](../deployment/horizontal-scaling.md)).

The liveness probe points at `/healthz` (first after 3 s, every 10 s) and the
readiness probe at `/readyz` (first after 1 s, every 5 s). The pods run as a
non-root user with a read-only root filesystem and every capability dropped.
The OTLP variables are in the manifest, commented out; uncommented, they set
`service.instance.id` from the pod name, which is what tells the replicas
apart.

## Shutdown timeline

`terminationGracePeriodSeconds: 45` must exceed the whole shutdown budget in
`main.rs`. If `SIGKILL` lands before the budget is spent, the graceful
shutdown is cut short.

<figure class="diagram">
<a href="../images/diagrams/ex-deploy-shutdown.light.svg"><img class="diagram-light" src="../images/diagrams/ex-deploy-shutdown.light.svg" alt="After SIGTERM, in-flight tasks get up to 5 seconds to finish, the rest are cancelled with up to 10 seconds to act on it, axum stops accepting and drains, the queue drain and cleanup hook share a 5 second budget, and the telemetry flush takes up to 15 seconds; all of it must finish before Kubernetes sends SIGKILL 45 seconds after SIGTERM."></a>
<a href="../images/diagrams/ex-deploy-shutdown.dark.svg"><img class="diagram-dark" src="../images/diagrams/ex-deploy-shutdown.dark.svg" alt=""></a>
<figcaption>The shutdown budget that <code>terminationGracePeriodSeconds: 45</code> has to cover.</figcaption>
</figure>

The budget is: up to 5 s for tasks to end on their own, then cancellation and
up to 10 s for the executors to act on it, then the HTTP drain, then one 5 s
budget shared by the event-queue drain and the executor's cleanup hook, then,
with telemetry on, the flush as `main` returns: up to 15 s when the collector
accepts connections and never answers, 1.8 s when it refuses them (measured
on `hello-agent`, 2026-10-26; `OTEL_EXPORTER_OTLP_TIMEOUT` shortens the
first). A task still running after 5 s is cancelled, however long its stream
would have been.

## Tests and allocator

```bash
cargo test -p deploy-agent
```

Five tests. Four drive the real router over a real socket rather than calling
the handler directly — the point of this example is the wiring, and asserting
on the handler would skip precisely the part that can be wrong. The fifth
checks that an invalid `PORT` is rejected.

The binary runs on jemalloc (`tikv-jemallocator`), not glibc's allocator. A
long-running server's bounded task store keeps a fixed-size heap full of freed
holes, and glibc's allocator is slow there: on the echo benchmark jemalloc
took unary throughput from 7,189 to 11,862 requests/s at the same resident
memory ([Production Hardening](../deployment/production.md) has the
measurement). To confirm a running build uses it, start it with
`_RJEM_MALLOC_CONF=stats_print:true`; jemalloc prints its statistics when the
process exits. Windows (MSVC) builds keep the system allocator, which
`tikv-jemalloc-sys` does not replace there.

## What it does not cover

Stated because a deployment example that quietly omits these reads as an
endorsement:

- **TLS.** Terminated at an ingress or service mesh here. An agent doing its
  own TLS needs a certificate source and a reload path, neither of which is
  shown.
- **Authentication.** No interceptor is installed, so every request is
  anonymous. See [Authentication](../building-agents/authentication.md) and
  the hardening act of the [Incident-Response Team](./incident-response.md).
- **The database itself.** `DATABASE_URL` names a PostgreSQL you run; its
  backups, upgrades and connection limits are yours. Without it the store is
  in memory: tasks do not survive a restart, and the agent must run as one
  replica.
- **The observability stack in Kubernetes.** With `OTEL_EXPORTER_OTLP_ENDPOINT`
  set, the binary sends traces, metrics and logs to that collector; with none
  set nothing is built. `deploy/observability` has a Docker Compose stack
  (Collector, Prometheus, Tempo, Loki, Grafana with the dashboards) for a
  laptop or a test box; a cluster's collector and backends are
  deployment-specific.

Where next: [Resilient Agent](./resilient-agent.md) asserts what a restart, a
failure and a second replica do to a task.
