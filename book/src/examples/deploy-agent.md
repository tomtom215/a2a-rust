# Deploy Agent

The smallest A2A agent you can actually ship.

[`hello-agent`](./hello-agent.md) is the smallest thing that answers A2A: one
screen of code, no configuration, bound to `127.0.0.1:3000`. It is the right
first example and the wrong thing to deploy. `deploy-agent` is the same agent
with the four things a container platform requires, plus a jemalloc allocator
and OTLP telemetry from the `OTEL_*` environment, and nothing else.

| Concern | Here | Why examples never show it |
|---|---|---|
| Configuration | `PORT`, `AGENT_URL` from the environment | Examples hardcode a port; a scheduler assigns one |
| Health checks | `GET /healthz`, `GET /readyz` | Nothing probes an example, so nobody notices there is no probe endpoint |
| Graceful shutdown | `SIGTERM`/`SIGINT` end in-flight tasks, then drain connections | Examples die to Ctrl-C and nobody minds the truncated stream |
| Bind address | `0.0.0.0` | A loopback bind works locally and is unreachable in a container |

## Run it

```sh
cargo run -p deploy-agent
curl localhost:8080/healthz
curl localhost:8080/.well-known/agent-card.json
```

| Variable | Default | Notes |
|---|---|---|
| `PORT` | `8080` | Rejected at startup if unparseable |
| `AGENT_URL` | `http://localhost:$PORT` | The **externally reachable** URL; goes on the agent card |

An unparseable `PORT` stops the process rather than falling back to the
default. A process that ignores its configuration is worse than one that
refuses to start, because the misconfiguration survives the deploy.

## The two traps

Both were found by running this example rather than by reading it, and both now
have tests.

**The card must advertise the public URL, not the bind address.** A container
binds `0.0.0.0:8080`; callers reach it at `https://agent.example.com`. Publish
the bind address and clients read the card, call it, and fail — with an error
that points at the client. The process cannot derive this, which is why
`AGENT_URL` is separate from `PORT`.

**The card must advertise the binding the process actually serves.**
`A2aRouter` serves HTTP+JSON — `/message:send`, `/tasks` — not JSON-RPC on `/`.
This example first shipped advertising `JSONRPC`, which looked correct
everywhere except in a client's 404.

## Container and Kubernetes

```sh
# Run both commands from the repository root; the build needs the workspace.
docker build -f examples/deploy-agent/Dockerfile -t deploy-agent .
kubectl apply -f examples/deploy-agent/deployment.yaml
```

Two edits to the manifest first. It names `deploy-agent:latest` and sets no
`imagePullPolicy`, and Kubernetes pulls a `:latest` image by default, so a
locally built image is not used: push it to a registry the cluster can reach
and set `image:` to it, or on kind run `kind load docker-image deploy-agent`
and set `imagePullPolicy: IfNotPresent`. And `AGENT_URL` there is the
in-cluster Service URL; set it to the URL callers will use.

Two details in those files matter more than they look:

- The `ENTRYPOINT` is exec form, so the binary is PID 1 and receives `SIGTERM`
  directly. The shell form puts `/bin/sh` there, which does not forward signals
  — graceful shutdown silently stops working and rollouts start cutting streams.
- `terminationGracePeriodSeconds: 45` must exceed the shutdown budget in
  `main.rs`: up to 5 s for tasks to end on their own, then cancellation and up
  to 10 s for the executors to act on it, then the HTTP drain, then one 5 s
  budget shared by the event-queue drain and the executor's cleanup hook, then,
  with telemetry on, the flush as `main` returns: up to 15 s when the collector
  accepts connections and never answers, 1.6 s when it refuses them (measured
  on `hello-agent`, 2026-10-26). A task still running after 5 s is
  cancelled, however long its stream would have been. If `SIGKILL` lands
  before the budget is spent, the graceful shutdown is cut short.

## What it does not cover

Stated because a deployment example that quietly omits these reads as an
endorsement: TLS (terminated at an ingress here), authentication (no
interceptor, so every request is anonymous), persistence (the in-memory store —
tasks do not survive a restart and replicas do not share them), and
observability beyond what `Telemetry` exports: with `OTEL_EXPORTER_OTLP_ENDPOINT`
set it sends traces, metrics and logs to that collector; there are no
dashboards, alerts or collector here. Each has a home elsewhere in this book;
none is pretended to be present.
