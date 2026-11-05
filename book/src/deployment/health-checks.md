<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Health Checks

What to point a load balancer or a Kubernetes probe at, and what each
endpoint actually checks. The short version: liveness should check nothing,
readiness should reach the task store, and which of those you get for free
depends on the dispatcher.

## What each dispatcher answers

| Dispatcher | `GET /health` | `GET /ready` | `GET /load` |
|---|---|---|---|
| `RestRouter` (Axum, feature `axum`) | `200 {"status":"ok"}`, a constant | `200 {"status":"ready"}`, or `503 {"status":"not_ready","reason":…}` from `RequestHandler::readiness()` | with `with_load_reporting()`: `200` and the load as JSON |
| `RestDispatcher`, `CombinedDispatcher` | `200 {"status":"ok"}`, a constant | the same as `RestRouter` | the same as `RestRouter` |
| `JsonRpcDispatcher` | not served | not served | not served |
| gRPC and WebSocket listeners | not served | not served | not served |

`GET /health` is liveness, a constant. `GET /ready` asks
`RequestHandler::readiness()` and answers `503` with a bounded `reason`
(`draining`, `saturated`, `timeout`, `halt_store`, or the store error's metric
label). Either body carries `"halted":true` while a halt of every tenant is
held: a halted replica stays ready, because it still serves reads and cancels.
The JSON-RPC dispatcher answers none of the three; route the path yourself in
front of it, answering with the dispatcher's own body type:

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
error's message, which can name a host or a connection string; `RestRouter`'s
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

## Load, for balancers

Readiness says whether a replica can serve; it does not say how busy it is.
Until 0.16.0 nothing did: a replica holding all 1,024 of its streams answered
ready, and a balancer kept sending it sends it refused `Overloaded` (audit H7,
ADR 0046). `RequestHandlerBuilder::with_load_reporting()` reports load two
ways, and changes nothing else:

- `GET /load` on the HTTP+JSON binding and the axum router:
  `{"running":3,"queued":0,"capacity":4,"utilization":0.75,"draining":false,"halted":false}`.
  `running` is the turns running, `queued` the sends waiting in the admission
  gate (0 without one), `capacity` the gate's capacity, else the stream cap,
  else `null`; `utilization` is `(running + queued) / capacity`, past 1 when
  sends are queued. It touches no store, so it can be scraped every second.
- The `endpoint-load-metrics` header on every response the crate's server
  (`serve`, `Server`) or the axum router sends:
  `TEXT application_utilization=0.750, named_metrics.running=3, named_metrics.queued=0`.
  This is the text form of an ORCA load report, which a client-side weighted
  balancer such as Envoy's reads from ordinary responses. The format follows
  Envoy's documentation and was not tested against Envoy here. gRPC responses
  do not carry it. `RequestHandler::load()` returns the same numbers.

### Saturation readiness, if you must

`HandlerLimits::with_saturation_readiness(unready_at, ready_at)` makes
`/ready` answer `503 saturated` once utilization reaches `unready_at` percent,
until it is back at or below `ready_at` percent. The band between the two
keeps one noisy probe from flapping it, and the build refuses a band that is
not one. It is off by default for a reason: replicas of a fleet saturate
together, so readiness taken away under load takes capacity away when it is
needed most, and probe periods of seconds are too slow to balance with.
Prefer the load signal where the balancer can read it.

`ServeConfig::production()` warns at startup when `max_connections` is unset:
nothing then bounds the connections a listener accepts.
