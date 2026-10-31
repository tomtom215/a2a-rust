<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Health Checks

What to point a load balancer or a Kubernetes probe at, and what each
endpoint actually checks. The short version: liveness should check nothing,
readiness should reach the task store, and which of those you get for free
depends on the dispatcher.

## What each dispatcher answers

| Dispatcher | `GET /health` | `GET /ready` |
|---|---|---|
| `A2aRouter` (Axum, feature `axum`) | `200 {"status":"ok"}`, a constant | `200 {"status":"ready"}`, or `503 {"status":"not_ready","reason":…}` when the task store cannot be reached |
| `RestDispatcher`, `CombinedDispatcher` | `200 {"status":"ok"}`, a constant | the same constant: says nothing about the store |
| `JsonRpcDispatcher` | not served | not served |
| gRPC and WebSocket listeners | not served | not served |

`A2aRouter` answers `GET /health` (liveness, a constant) and `GET /ready`,
which reaches the task store and answers `503` with a `not_ready` body when it
cannot. `RestDispatcher` (and `CombinedDispatcher`, through its REST half)
answers both paths with a constant `{"status":"ok"}`, so its `/ready` says
nothing about the store; call `RequestHandler::task_store_health()` from a
route of your own if a probe needs that (`examples/deploy-agent` does). The
JSON-RPC dispatcher answers neither; route the path yourself in front of it,
answering with the dispatcher's own body type:

```rust
use std::convert::Infallible;
use std::sync::Arc;

use a2a_protocol_sdk::server::JsonRpcDispatcher;
use http_body_util::{BodyExt, Full, combinators::BoxBody};
use hyper::body::{Bytes, Incoming};

async fn route(
    dispatcher: Arc<JsonRpcDispatcher>,
    req: hyper::Request<Incoming>,
) -> hyper::Response<BoxBody<Bytes, Infallible>> {
    if req.uri().path() == "/health" {
        hyper::Response::new(Full::new(Bytes::from("ok")).boxed())
    } else {
        dispatcher.dispatch(req).await
    }
}
```

## What readiness checks

`RequestHandler::task_store_health()` asks the task store's `health()`. The
SQL stores answer with one statement whose cost does not depend on how many
tasks they hold (`SELECT pg_is_in_recovery()` on PostgreSQL, `SELECT 1` on
SQLite); a PostgreSQL standby, which takes no writes, is not ready. A store
at its capacity limit still reports ready: capacity is not reachability. A
custom store that does not implement `health()` is asked for its `count()`.
Until 0.16.0 the probe was `count()`, a `SELECT COUNT(*)` over every task,
which slowed as the table grew until every replica's probe timed out at once. It
records no span: a probe every few seconds per replica would otherwise be a
trace each. On failure, expose
`A2aError::metric_label` rather than the
error's message, which can name a host or a connection string; `A2aRouter`'s
`/ready` does that in its `reason` field.

## Kubernetes probes

`examples/deploy-agent` points its `deployment.yaml` at the two endpoints the
HTTP+JSON dispatcher serves:

```yaml
startupProbe:
  httpGet:
    path: /health
    port: http
  periodSeconds: 2
  failureThreshold: 45
livenessProbe:
  httpGet:
    path: /health
    port: http
  periodSeconds: 10
  timeoutSeconds: 2
readinessProbe:
  httpGet:
    path: /ready
    port: http
  periodSeconds: 5
  timeoutSeconds: 3
```

`/health` checks nothing, on purpose: a liveness probe that follows a
dependency turns that dependency's outage into a restart loop on every
replica. `/ready` calls `RequestHandler::readiness()`: `503` while the task
store fails `health()`, so a replica that cannot reach its database stops
taking traffic without being restarted, and `503 draining` once the process
is shutting down (`begin_draining`, which `Server` calls when its signal
fires and `finish_in_flight` calls too). The startup probe holds the other
two off while the database is first reached and its schema is applied. The
rest of that manifest is on [Docker and Kubernetes](./containers.md).

Until 0.16.0 the HTTP+JSON dispatcher's `/ready` (and so
`CombinedDispatcher`'s) answered the same constant `200` as `/health`, and
only the axum router's asked the store.
