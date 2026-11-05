<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Sharing connections: `HttpPool`

Every client built by `ClientBuilder` has a connection pool of its own. A
program that holds a client for each of fifty peers holds fifty pools, and
two clients that call the same host share nothing. When you can, share the
clients themselves ([Reusing and sharing a client](./reuse.md)). When you
need many clients (they differ in tenant, credentials or protocol binding)
that call the same hosts, give them one `HttpPool` and they reuse each
other's connections.

```rust,no_run
# use a2a_protocol_sdk::prelude::*;
use std::time::Duration;

# fn main() -> ClientResult<()> {
let pool = HttpPool::builder()
    .with_max_connection_lifetime(Duration::from_secs(600))
    .with_http2_keep_alive(Duration::from_secs(30), Duration::from_secs(10))
    .build()?;

let alpha = ClientBuilder::new("https://alpha.example")
    .with_pool(&pool)
    .build()?;
let beta = ClientBuilder::new("https://beta.example")
    .with_pool(&pool)
    .build()?;
# drop((alpha, beta));
# Ok(())
# }
```

A pool is cheap to clone; every clone is the same pool. `HttpPool::shared()`
returns one process-wide pool with the defaults, created on first use, for a
caller who wants one line.

## What a pool owns

One TLS identity (the roots it trusts and any client certificate it
presents), one proxy route, one set of timeouts. Connections for two TLS
identities cannot be mixed, because they cannot be put in the same pool. So a
pool is the right size for *one identity*, and a program that talks to some
peers with a client certificate and to others without needs two pools.

A client that sets its own TLS configuration (`with_tls_config`,
`without_tls`) or connect timeout (`with_connection_timeout`) **and** is given
a pool fails at `build()` with an error that names the setting. It is not
ignored. Set it on the pool instead: `HttpPool::builder().with_tls_config(..)`
or `.with_connect_timeout(..)`. Everything else stays the client's: its
request and send timeouts, retry policy, in-flight cap and interceptors.

The proxy is read from `HTTPS_PROXY`, `HTTP_PROXY`, `ALL_PROXY` and
`NO_PROXY` once, when the pool is built.

A pool serves the JSON-RPC and HTTP+JSON bindings. gRPC and WebSocket clients
keep the connections they have.

## Settings

Everything is off, or what a client without a pool has, until asked for.

| Setting | Default | Notes |
|---|---|---|
| `with_tls_config` | Mozilla's roots plus `SSL_CERT_FILE` / `SSL_CERT_DIR` | Build one with `tls::tls_config_from_pem`. |
| `with_connect_timeout` | 10 s | The TCP handshake. |
| `with_idle_timeout` | 50 s | How long an unused connection is kept. Keep it under the idle timeout of anything in between (60 s is common). |
| `with_max_idle_per_host` | no limit | Idle connections kept per host. |
| `with_max_connection_lifetime` | connections are not recycled | New connections are used once the current ones are this old, spread by up to 10% either way. |
| `with_http2_keep_alive(interval, timeout)` | no PING | An HTTP/2 PING every `interval`, idle or not; a connection that does not answer within `timeout` is closed. |
| `with_http2_prior_knowledge` | negotiate | Speak HTTP/2 without negotiating it, including over plain `http://`. For agents known to accept it. |
| `with_ejection(Ejection)` | off | A circuit breaker per origin, shared by the pool's clients: after a number of failures in a row the origin's calls are refused with `ClientError::CircuitOpen` until one probe succeeds. |
| `with_max_in_flight_per_origin(n)` | no cap | Calls waiting for a response from one origin, across the pool's clients; one more is refused with `TooManyPendingRequests`. Zero is refused. |
| `with_retry_budget(RetryBudget)` | none | One retry budget per origin for every client of the pool, besides each client's own. |

### Per-origin state

A pool keeps, for each origin (scheme, host and port) its clients call, the
calls in flight and whether the origin is failing (ADR 0050). The three
settings above use it, and each is off by default. `HttpPool::origins()`
returns a snapshot per origin: calls in flight, how much longer it is ejected,
and how many failures and ejections in a row. A
[balanced client](../load-balancers.md#balancing-in-the-client) on the pool
uses the same state for its rotation, and the pool's ejection policy.

### Recycling connections

`with_max_connection_lifetime` is for balancers that pin a long-lived
connection to one replica ([Behind a load balancer](../load-balancers.md)).
A connection is not closed under a call: when the lifetime passes, new
requests go to a new connection, and the old one finishes what it is doing
and closes when it is idle. A connection can therefore outlive the lifetime
by the length of the longest call or stream on it. Each renewal costs a TCP
and TLS handshake per host.

### Keep-alive PINGs

`with_http2_keep_alive` finds a connection that a firewall or a vanished peer
has silently dropped before the next call fails on it, and it applies only to
connections that speak HTTP/2 (over TLS, those that negotiate it; over plain
`http://`, those you opened with `with_http2_prior_knowledge`). A PING is
answered by the next hop, so it does not prove the agent is up. Servers that
limit PINGs close a connection that sends them too often; pick an `interval`
for the strictest server the pool will meet.

## Closing idle connections

An idle connection is closed in the background when its idle timeout passes.
Until 0.16.0 a client closed one only when the next request asked for it, so
a program that talked to many hosts once kept a socket open to each until it
exited. This applies to every client, with or without a pool.
