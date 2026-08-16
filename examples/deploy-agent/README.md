<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# deploy-agent

The smallest A2A agent you can actually ship.

[`hello-agent`](../hello-agent) is the smallest thing that answers A2A — 23
lines, no configuration, bound to `127.0.0.1:3000`. It is the right first
example and the wrong thing to deploy. This is the same agent with the four
things a container platform requires, and nothing else.

| Concern | Here | Why examples never show it |
|---|---|---|
| Configuration | `PORT`, `AGENT_URL` from the environment | Examples hardcode a port; a scheduler assigns one |
| Health checks | `GET /healthz`, `GET /readyz` | Nothing probes an example, so nobody notices there is no probe endpoint |
| Graceful shutdown | `SIGTERM`/`SIGINT` drain in-flight work | Examples die to Ctrl-C and nobody minds the truncated stream |
| Bind address | `0.0.0.0` | A loopback bind works locally and is unreachable in a container |

The agent logic is deliberately trivial. Everything worth reading is the
wrapper, because the wrapper is what is missing when someone takes an example
to production and finds it answers only to itself.

## Run it

```sh
cargo run -p deploy-agent
curl localhost:8080/healthz
curl localhost:8080/.well-known/agent-card.json

curl -X POST localhost:8080/message:send \
  -H 'content-type: application/json' \
  -d '{"message":{"messageId":"m1","role":"ROLE_USER","parts":[{"text":"Ada"}]}}'
```

`A2aRouter` serves the **HTTP+JSON** binding (`/message:send`, `/tasks`, …),
which is what the agent card advertises. It is not JSON-RPC on `/` — a card
claiming `JSONRPC` here would describe a binding this process does not serve,
and the only symptom is clients getting a 404.

Configuration:

| Variable | Default | Notes |
|---|---|---|
| `PORT` | `8080` | Rejected at startup if unparseable — a process that ignores its configuration is worse than one that refuses to start |
| `AGENT_URL` | `http://localhost:$PORT` | The **externally reachable** URL. Goes on the agent card |

## Container

```sh
# From the repository root — the build needs the workspace.
docker build -f examples/deploy-agent/Dockerfile -t deploy-agent .
docker run --rm -p 8080:8080 -e AGENT_URL=http://localhost:8080 deploy-agent
```

Two stages: build with the toolchain, ship without it. Runs as uid 10001 with
no shell in the entrypoint, so the binary is PID 1 and receives `SIGTERM`
directly — the shell form puts `/bin/sh` there, which does not forward signals,
and graceful shutdown silently stops working.

## Kubernetes

```sh
kubectl apply -f examples/deploy-agent/deployment.yaml
```

`deployment.yaml` wires the probes to the endpoints above and sets
`terminationGracePeriodSeconds: 30`. That number must exceed your longest
in-flight stream: if `SIGKILL` lands before the drain finishes, the graceful
shutdown in `main.rs` is decorative.

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

Three, all driving the real router over a real socket rather than calling the
handler directly — the point of this example is the wiring, and asserting on
the handler would skip precisely the part that can be wrong.

## What this does not cover

Stated rather than implied, because a deployment example that quietly omits
these reads as an endorsement:

- **TLS.** Terminated at an ingress or service mesh here. An agent doing its own
  TLS needs a certificate source and a reload path, neither of which is shown.
- **Authentication.** No interceptor is installed, so every request is
  anonymous. See [Authentication](https://a2a-rust.com/building-agents/authentication.html)
  and the `harden` act of [`incident-response`](../incident-response).
- **Persistence.** The default in-memory store: tasks do not survive a restart,
  and two replicas do not share them. `replicas: 2` is safe here only because
  the agent is stateless per request — a real deployment wants the SQLite or
  Postgres store.
- **Observability.** No metrics or tracing exporter. The SDK has both hooks;
  wiring them is deployment-specific.
