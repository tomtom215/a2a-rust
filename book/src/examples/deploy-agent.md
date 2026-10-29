# Deploy Agent

The smallest A2A agent you can actually ship.

[`hello-agent`](./hello-agent.md) is the smallest thing that answers A2A: one
screen of code, no configuration, bound to `127.0.0.1:3000`. It is the right
first example and the wrong thing to deploy. `deploy-agent` is the same agent
with what a container platform requires, plus a jemalloc allocator,
an optional PostgreSQL store for running more than one replica, and OTLP
telemetry from the `OTEL_*` environment, and nothing else.

**Source:** [`examples/deploy-agent/`](https://github.com/tomtom215/a2a-rust/tree/main/examples/deploy-agent)

## What it shows

| Concern | Here | Why examples never show it |
|---|---|---|
| Configuration | `PORT`, `AGENT_URL`, `DATABASE_URL`, `DRAIN_DELAY_SECONDS` from the environment, checked before anything binds | Examples hardcode a port; a scheduler assigns one |
| Health checks | `GET /health` (liveness, a constant) and `GET /ready` (readiness: the task store answers and the process is not draining) | Nothing probes an example, so nobody notices there is no probe endpoint |
| Graceful shutdown | `SIGTERM`: not ready at once, still serving for `DRAIN_DELAY_SECONDS`, then in-flight tasks end, then connections drain, each step bounded | Examples die to Ctrl-C and nobody minds the truncated stream |
| Bind address | every address: `[::]` (IPv6 and IPv4), or `0.0.0.0` where the host has no IPv6 | A loopback bind works locally and is unreachable in a container |
| Timeouts | the SDK's `Server`: a header-read and an idle timeout on every connection | `axum::serve` has neither, so a client sending a byte a minute holds a connection forever |
| Replicas | `DATABASE_URL` puts tasks and push configs in PostgreSQL | Behind a load balancer, a task created on one replica's memory is not found on the next |

The agent logic is deliberately trivial. Everything worth reading is the
wrapper, because the wrapper is what is missing when someone takes an example
to production and finds it answers only to itself.

## Run it

Locally it needs nothing but Rust; Docker, Kubernetes and PostgreSQL are for
the deployment steps further down.

```bash
cargo run -p deploy-agent
curl localhost:8080/health
curl localhost:8080/ready
curl localhost:8080/.well-known/agent-card.json

curl -X POST localhost:8080/message:send \
  -H 'content-type: application/json' \
  -H 'A2A-Version: 1.0' \
  -d '{"message":{"messageId":"m1","role":"ROLE_USER","parts":[{"text":"Ada"}]}}'
```

`CombinedDispatcher` serves both HTTP bindings at the one URL: HTTP+JSON
(`/message:send`, `/tasks`, …) and JSON-RPC on `/`. The agent card advertises
both.

| Variable | Default | Notes |
|---|---|---|
| `PORT` | `8080` | Rejected at startup unless a port from 1 to 65535 |
| `AGENT_URL` | `http://localhost:$PORT` | The **externally reachable** `http://` or `https://` URL; goes on the agent card |
| `DATABASE_URL` | unset: tasks in memory | A PostgreSQL URL. Tasks and push configs go there, which is what lets several replicas serve the agent; unset, run one replica. Add `?sslmode=verify-full&sslrootcert=/path/ca.pem` (or `require`) for TLS to the database |
| `DRAIN_DELAY_SECONDS` | `0` | After `SIGTERM`, how long `/ready` answers 503 while the server goes on serving, so load balancers stop routing to it before it stops accepting. The manifest sets 5 |
| `OTEL_EXPORTER_OTLP_ENDPOINT` | unset: nothing exported | A collector's OTLP/HTTP URL, such as `http://localhost:4318`; traces, metrics and logs all go there |

A value the process cannot use stops it rather than falling back to the
default. A process that ignores its configuration is worse than one that
refuses to start, because the misconfiguration survives the deploy.

## What you'll see

With the defaults, the log reads (the first line is the telemetry setup's):

```text
INFO a2a_protocol_server::otel::telemetry: telemetry: nothing is exported; set OTEL_EXPORTER_OTLP_ENDPOINT to send it to a collector, or OTEL_TRACES_EXPORTER=console to print spans
WARN deploy_agent: DATABASE_URL is unset: tasks are kept in this process's memory, so run one replica, or set DATABASE_URL to a PostgreSQL database every replica shares
INFO deploy_agent: deploy-agent serving JSON-RPC and HTTP+JSON listening=[::]:8080 advertising=http://localhost:8080
```

With `DATABASE_URL` set it says which database it connects to, without the
password, and a store it cannot open stops it with one line naming the store
and the reason. `/health` answers `200`, and `/ready` answers
`{"status":"ready"}`, or `503` with `{"status":"not_ready","reason":…}` when
the task store does not answer or the process is draining. On `SIGTERM` or
Ctrl-C it logs each step of the shutdown and `drained cleanly, exiting` when
the shutdown was graceful.

## Key code

The server is the SDK's own `Server`, which brings the timeouts and the
ordered, bounded shutdown; the rest is configuration:

```rust,no_run
# use std::sync::Arc;
# use std::time::Duration;
# use a2a_protocol_sdk::prelude::*;
# use a2a_protocol_sdk::server::dispatch::CombinedDispatcher;
# use a2a_protocol_sdk::server::serve::ServeConfig;
# async fn run(handler: Arc<RequestHandler>) -> Result<(), Box<dyn std::error::Error>> {
let config = ServeConfig::default().with_drain_delay(Duration::from_secs(5));
let server = Server::bind("[::]:8080").await?.with_config(config);
let report = server
    .serve_with_shutdown(CombinedDispatcher::new(Arc::clone(&handler)), shutdown_signal())
    .await;
if let Some(why) = &report.refused {
    return Err(format!("the server refused to start: {why}").into());
}
// Then the executor's cleanup hook and the release of the turn claims.
let shutdown = handler.shutdown_with_timeout(Duration::from_secs(5)).await;
if shutdown.is_graceful() {
    println!("drained cleanly, exiting");
}
# Ok(())
# }
```

When the signal fires, `/ready` turns to `503 draining` at once and the server
goes on accepting for the drain delay, while the Service's endpoints and the
proxies in front learn the pod is leaving. Then it stops accepting; in-flight
tasks get 5 s to end on their own, are cancelled, and get 10 s to act on it;
then connections drain for up to 10 s.

## The two traps

Both were found by running this example rather than by reading it, and both now
have tests.

**The card must advertise the public URL, not the bind address.** A container
binds `[::]:8080`; callers reach it at `https://agent.example.com`. Publish
the bind address and clients read the card, call it, and fail — with an error
that points at the client. The process cannot derive this, which is why
`AGENT_URL` is separate from `PORT`. The test
`agent_card_advertises_the_public_url_not_the_bind_address` asserts both that
the card carries the configured URL and that it does not leak `127.0.0.1`.

**The card must advertise the bindings the process actually serves.** This
example first shipped advertising `JSONRPC` while it served only HTTP+JSON,
which looked correct everywhere except in a client's 404. The test
`card_advertises_the_bindings_the_server_actually_serves` calls each binding
the card names.

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
<a href="../images/diagrams/ex-deploy-topology.light.svg"><img class="diagram-light" src="../images/diagrams/ex-deploy-topology.light.svg" alt="Callers reach a Service on port 80, which balances across two deploy-agent pods listening on port 8080; the kubelet probes each pod's /health and /ready, and both pods share one PostgreSQL database."></a>
<a href="../images/diagrams/ex-deploy-topology.dark.svg"><img class="diagram-dark" src="../images/diagrams/ex-deploy-topology.dark.svg" alt=""></a>
<figcaption>What <code>deployment.yaml</code> sets up: two replicas, their probes, one shared store.</figcaption>
</figure>

Three edits first (as shipped, `kubectl apply` fails twice: the Secret is
missing, then the image cannot be pulled):

- **The image.** The manifest names `deploy-agent:latest` and sets no
  `imagePullPolicy`, and Kubernetes pulls a `:latest` image by default, so a
  locally built image is not used. Push it to a registry the cluster can reach
  and set `image:` to it, or on kind run `kind load docker-image deploy-agent`
  and set `imagePullPolicy: IfNotPresent`.
- **`AGENT_URL`.** The manifest sets the in-cluster Service URL in the pod's
  own namespace. Set it to the URL callers will use.
- **The database.** The manifest runs two replicas and takes `DATABASE_URL`
  from the Secret `deploy-agent-db`, which you create:
  `kubectl create secret generic deploy-agent-db --from-literal=url='postgres://USER:PASSWORD@HOST:5432/DB?sslmode=verify-full&sslrootcert=/path/ca.pem'`.
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

The manifest also sets:

- a **startup probe** on `/health` (every 2 s, 45 tries): the port opens only
  once the database has answered and its schema is in place, which a cold
  database can take tens of seconds to do, and liveness waits for it;
- **liveness** on `/health` (every 10 s) and **readiness** on `/ready`
  (every 5 s);
- a rolling update that adds a pod before it removes one
  (`maxSurge: 1`, `maxUnavailable: 0`), a **PodDisruptionBudget** of one,
  and spreading across nodes and zones;
- the Pod Security **restricted** settings: non-root, `seccompProfile:
  RuntimeDefault`, a read-only root filesystem, every capability dropped,
  and no service-account token;
- `DRAIN_DELAY_SECONDS=5`.

The OTLP variables are in the manifest, commented out; uncommented, they set
`service.instance.id` from the pod name, which is what tells the replicas
apart.

## Shutdown timeline

`terminationGracePeriodSeconds: 60` must exceed the whole shutdown budget. If
`SIGKILL` lands before the budget is spent, the turn claims are left to expire
and the graceful shutdown is cut short.

<figure class="diagram">
<a href="../images/diagrams/ex-deploy-shutdown.light.svg"><img class="diagram-light" src="../images/diagrams/ex-deploy-shutdown.light.svg" alt="After SIGTERM, readiness turns off and the pod keeps serving for the 5 second drain delay; in-flight tasks then get up to 5 seconds to finish, the rest are cancelled with up to 10 seconds to act on it, connections drain for up to 10 seconds, the cleanup hook and claim release take up to 5 seconds, and the telemetry flush up to 15 seconds; all of it must finish before Kubernetes sends SIGKILL 60 seconds after SIGTERM."></a>
<a href="../images/diagrams/ex-deploy-shutdown.dark.svg"><img class="diagram-dark" src="../images/diagrams/ex-deploy-shutdown.dark.svg" alt=""></a>
<figcaption>The shutdown budget that <code>terminationGracePeriodSeconds: 60</code> has to cover.</figcaption>
</figure>

The budget is: the drain delay (5 s), up to 5 s for tasks to end on their
own, then cancellation and up to 10 s for the executors to act on it, up to
10 s of connection drain, up to 5 s for the cleanup hook and the release of
the turn claims, then, with telemetry on, the flush as `main` returns: up to
15 s when the collector accepts connections and never answers, 1.8 s when it
refuses them (measured on `hello-agent`, 2026-10-26;
`OTEL_EXPORTER_OTLP_TIMEOUT` shortens the first). A task still running 5 s
after the drain delay is stopped, however long its stream would have been,
and ends `failed` with failure class `transient`, telling its caller it may
send again; a send that arrives after the drain delay is refused with a
`503` and `Retry-After`, so a client with a retry policy sends it to the
other replica.

## Tests and allocator

```bash
cargo test -p deploy-agent
```

Seven tests. Five drive the real server over a real socket rather than
calling the handler directly — the point of this example is the wiring, and
asserting on the handler would skip precisely the part that can be wrong. Two
check that a configuration it cannot use is refused and that the database URL
is logged without its password.

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
