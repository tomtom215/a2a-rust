<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# deploy-agent

The smallest A2A agent you can actually ship.

[`hello-agent`](../hello-agent) is the smallest thing that answers A2A — one
screen of code, no configuration, bound to `127.0.0.1:3000`. It is the right
first example and the wrong thing to deploy. This is the same agent with the
four things a container platform requires, plus a jemalloc allocator and OTLP
telemetry from the `OTEL_*` environment, and nothing else.

| Concern | Here | Why examples never show it |
|---|---|---|
| Configuration | `PORT`, `AGENT_URL` from the environment | Examples hardcode a port; a scheduler assigns one |
| Health checks | `GET /healthz`, `GET /readyz` | Nothing probes an example, so nobody notices there is no probe endpoint |
| Graceful shutdown | `SIGTERM`/`SIGINT` end in-flight tasks, then drain connections | Examples die to Ctrl-C and nobody minds the truncated stream |
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
  -H 'A2A-Version: 1.0' \
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

Two edits first:

- **The image.** The manifest names `deploy-agent:latest` and sets no
  `imagePullPolicy`, and Kubernetes pulls a `:latest` image by default, so a
  locally built image is not used. Push it to a registry the cluster can reach
  and set `image:` to it, or on kind run `kind load docker-image deploy-agent`
  and set `imagePullPolicy: IfNotPresent`.
- **`AGENT_URL`.** The manifest sets the in-cluster Service URL. Set it to the
  URL callers will use (see the trap below).

`deployment.yaml` wires the probes to the endpoints above and sets
`terminationGracePeriodSeconds: 30`. That number must exceed the shutdown
budget in `main.rs`: up to 5 s for tasks to end on their own, then
cancellation and up to 10 s for the executors to act on it, then the HTTP
drain, then one 5 s budget shared by the event-queue drain and the executor's
cleanup hook. A task still running after 5 s is cancelled, however long its
stream would have been. If `SIGKILL` lands before that finishes, the graceful
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

Five. Four drive the real router over a real socket rather than calling the
handler directly — the point of this example is the wiring, and asserting on
the handler would skip precisely the part that can be wrong. The fifth checks
that an invalid `PORT` is rejected.

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
- **Persistence.** The default in-memory store: tasks do not survive a restart,
  and two replicas do not share them. `replicas: 2` is safe here only because
  the agent is stateless per request — a real deployment wants the SQLite or
  Postgres store.
- **Observability beyond `Telemetry`.** With `OTEL_EXPORTER_OTLP_ENDPOINT` set
  (or a per-signal endpoint) the binary sends traces, metrics and logs to that
  collector; with none set nothing is built. There are no dashboards, alerts or
  collector here, and wiring those is deployment-specific.
