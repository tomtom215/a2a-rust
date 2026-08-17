# Deploy Agent

The smallest A2A agent you can actually ship.

[`hello-agent`](./hello-agent.md) is the smallest thing that answers A2A: 23
lines, no configuration, bound to `127.0.0.1:3000`. It is the right first
example and the wrong thing to deploy. `deploy-agent` is the same agent with
the four things a container platform requires, and nothing else.

| Concern | Here | Why examples never show it |
|---|---|---|
| Configuration | `PORT`, `AGENT_URL` from the environment | Examples hardcode a port; a scheduler assigns one |
| Health checks | `GET /healthz`, `GET /readyz` | Nothing probes an example, so nobody notices there is no probe endpoint |
| Graceful shutdown | `SIGTERM`/`SIGINT` drain in-flight work | Examples die to Ctrl-C and nobody minds the truncated stream |
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
docker build -f examples/deploy-agent/Dockerfile -t deploy-agent .
kubectl apply -f examples/deploy-agent/deployment.yaml
```

Two details in those files matter more than they look:

- The `ENTRYPOINT` is exec form, so the binary is PID 1 and receives `SIGTERM`
  directly. The shell form puts `/bin/sh` there, which does not forward signals
  — graceful shutdown silently stops working and rollouts start cutting streams.
- `terminationGracePeriodSeconds: 30` must exceed your longest in-flight
  stream. If `SIGKILL` lands first, the drain is decorative.

## What it does not cover

Stated because a deployment example that quietly omits these reads as an
endorsement: TLS (terminated at an ingress here), authentication (no
interceptor, so every request is anonymous), persistence (the in-memory store —
tasks do not survive a restart and replicas do not share them), and
observability exporters. Each has a home elsewhere in this book; none is
pretended to be present.
