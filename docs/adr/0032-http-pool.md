<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# ADR 0032: A shared HTTP pool is opt-in, fixed at construction, and owns per-origin state

**Date:** 2026-10-31
**Status:** Accepted, built in part (amended 2026-11-04). The pool itself,
`ClientBuilder::with_pool`, `HttpPool::shared()` and the pool-level settings
(TLS configuration, connect and idle timeouts, a cap on idle connections per
host, HTTP/2 PING, a maximum connection lifetime, HTTP/2 prior knowledge) are
built and tested; see the amendment at the end. The per-origin state of
decision 5 and the card resolver of decision 6 are not built. The part about
client-side load balancing (H12) is not built and stays conditional on a
measurement, as written below. The shape (opt-in, not the default) was
recommended to the maintainer, who decided on 2026-11-03 to build the opt-in
pool (fork F7).
**Author:** Tom F.

---

## Context

Every `ClientBuilder` builds its own `hyper_util` client, and with it its own
connection pool (`crates/a2a-protocol-client/src/transport/mod.rs:59` and
`tls.rs:307`, each with a 50 s idle timeout). Card discovery, the token
provider and the JWKS fetch build further clients of their own (`discovery.rs`,
`token_provider.rs`, `jwks.rs`). A supervisor that holds a client per child
therefore holds a pool per child: connections are not shared between children
that call the same host, and nothing is shared that could say "this origin is
failing".

This blocks the swarm items that need state shared across clients: a retry
budget, circuit breaker and in-flight cap (H6; recorded recommendation H-D2:
per-client opt-in now, a process-wide registry after the pool), card discovery
cost at fleet cold start (H8), and spreading calls over replicas of a peer
(H12).

## Decision

1. **`HttpPool`**: one `hyper_util` client together with the connector
   configuration it was built from, fixed at construction through
   `HttpPool::builder()`: TLS configuration, proxy and `NO_PROXY` handling, idle
   timeout, maximum idle connections per host, HTTP/2 settings, connect timeout.
   `Clone` is cheap (an `Arc`). A `hyper_util` client has exactly one
   connector, so a pool has exactly one TLS identity and one proxy route:
   connections for different identities cannot be mixed, because they cannot
   be put in the same pool.
2. **`ClientBuilder::with_pool(&pool)`.** A builder that also sets its own TLS
   configuration, proxy or pool-level timeout fails at `build()` with a
   configuration error naming the conflict. It does not ignore the setting.
3. **Opt-in.** `ClientBuilder::new` keeps building its own pool; nothing changes
   unless asked. Reasons: a shared default would let clients with different TLS
   identities (a client certificate for one tenant, none for another) end up
   in one pool, which is a security hazard; it makes pool state global and
   test isolation harder; and it would be a silent behaviour change.
4. **`HttpPool::shared()`**: one process-wide pool with safe defaults (system
   roots, proxy variables, certificate verification on, no insecure option),
   created on first use, for callers who want one line.
5. **Per-origin state lives in the pool.** For each origin: calls in flight,
   consecutive failures, the time of the last failure, and an "ejected until".
   Three opt-in policies read it, all off by default:
   * an **in-flight cap** per origin (a semaphore; `hyper-util` 0.1.21, the
     version in `Cargo.lock`, offers an idle timeout and a maximum of idle
     connections per host, unlimited by default, and no maximum of total
     connections);
   * a **circuit breaker** (open after N consecutive failures, half-open probe
     after a cool-down) and a **retry budget** (retries as a bounded fraction
     of calls, so retries cannot amplify an overload; H6);
   * **passive ejection** of an endpoint that keeps failing (input to the
     balancer below).
   All of it reports through the existing metrics hooks.
6. **Card discovery shares the pool** (`CachingCardResolver` takes one), with
   singleflight per URL and jittered expiry, so a fleet starting together does
   not fetch every card at once (H8).

### Client-side load balancing and hedging (H12): a decision for the maintainer

The recorded recommendation (H-D7) is to **decline in the core** and document L4
and L7 balancing and headless services. This ADR records the case for the
alternative so the choice is made knowing it:

* **For a hook, not a balancer:** a long-lived HTTP/2 connection through an L4
  balancer pins all of a client's streams to one replica. A supervisor that
  fans out to one service sees that imbalance. A client addressing a service
  name with a set of endpoints, choosing between two at random by load (power of
  two choices) and ejecting the failing ones, fixes it without a mesh.
* **Against:** it is a runtime feature; meshes and proxies already do it; every
  policy is a thing to maintain.

**Proposed:** an `EndpointResolver` hook on the pool (a service name resolves to
a set of endpoints, fed by ADR 0031's sources or DNS), with power-of-two
selection and the passive ejection above as an opt-in policy, and hedging only
for calls the caller marks idempotent (a `SendMessage` with an idempotency
key, `GetTask`, `ListTasks`), capped to a small fraction of extra load.
**Build it only if a measurement shows that L4/L7 balancing leaves a fan-out
unevenly loaded**; otherwise take H-D7 as recorded.

## Alternatives

| Option | Why not |
|---|---|
| Shared by default | See 3. |
| One global pool, no handle | Hidden state; cannot hold two TLS identities; tests share it. |
| Keep per-client pools, document them | Leaves H6, H8 and H12 without a place to keep their state. |

## Consequences

* Additive: the default is unchanged.
* The pool is where connection counts and per-origin health become visible: a
  gauge of open and idle connections, and per-origin in-flight and ejection.
* A pool built with a TLS configuration cannot be used by a builder that sets
  another; the error names which.

## What would settle it (nothing is run)

* Two clients with different TLS configurations given one pool fail at
  `build()`; the same configuration shares connections (a counting acceptor
  sees one TCP connection for N clients, not N).
* Breaker, retry budget and ejection against a fake clock and a server that
  fails on command.
* The swarm example with 500 and 2,000 clients on one host, counting file
  descriptors and connections with and without the pool, hardware and build
  profile stated.
* For H12 only: a fan-out through an L4 balancer measuring per-replica load
  with and without the hook.

## Amendment, 2026-11-04: what was built, and the two open questions

**Decided.**

* The configuration error is the existing `ClientError::Config`, with a
  message that names the two calls in conflict and says where the setting
  belongs. A new variant would have been a second way to say "the client
  cannot be built as configured".
* `HttpPool` lives in the client crate. The server's own clients
  (`net/client.rs`, the push sender, the JWT key fetch, the telemetry
  exporter) keep theirs: they differ in what they resolve and pin, and
  `scripts/check_net_copies.py` still holds the duplicated proxy and root-store
  code identical. A crate below both is still possible later and would end the
  duplication; nothing here makes it harder.
* A pool is available with `tls-rustls`, the default. Without it each client
  keeps its own plain-HTTP pool.
* Conflicts are detected by what the caller *called*, not by comparing values:
  `with_connection_timeout(10 s)` conflicts with a pool though 10 s is the
  default, because the caller asked for a value the pool owns.
* A maximum connection lifetime is a *generation*: past it, new requests go to
  a new hyper client and the old one is dropped when nothing uses it, so no call
  is cut. hyper-util has no per-connection age limit to configure. Each
  lifetime is spread by up to 10% either way.
* Both of hyper-util's timers are installed for every client, pooled or not:
  hyper panics if an HTTP/2 keep-alive or timeout is configured without one,
  and without the pool timer the idle sweeper never starts (found while
  building this; see the CHANGELOG, Fixed).

**Built, with the test that shows it** (`crates/a2a-protocol-client/tests/http_pool/`):

* Ten clients on one pool make one connection; ten without one make ten
  (`sharing`). Two clients with different TLS configurations given one pool
  fail at `build()` (`conflicts`); a pool uses its own TLS configuration and
  client certificate (`tls`).
* The maximum lifetime, to the nanosecond under a paused clock, and no call
  cut by it (`pool::tests`, `lifetime`). HTTP/2 PINGs only when asked, and an
  unanswered one closes the connection and the next call reconnects
  (`keep_alive`). The idle sweeper and the idle cap (`idle`). The connect
  timeout (`connect`).

**Not built, and why.**

* Decision 5: the per-origin in-flight cap, circuit breaker, retry budget
  and passive ejection. They are the next change. The per-client forms
  (`with_max_in_flight`, `RetryPolicy::with_budget`) exist already.
  Passive ejection has no consumer until the balancer below exists, so it is
  the breaker's open state and nothing more until then.
* Decision 6: `CachingCardResolver` sharing the pool, with singleflight per URL
  and jittered expiry (H8). `CardFetchOptions` still builds a client per fetch.
* H12, the `EndpointResolver` and hedging: the condition above (a measurement
  showing that layer-4/7 balancing leaves a fan-out uneven) has not been met,
  so H-D7 stands as recorded.
* The swarm measurement ("500 and 2,000 clients on one host") and the
  per-replica measurement through a layer-4 balancer from "What would settle
  it": not run.
* Gauges of open and idle connections: hyper-util does not expose the pool's
  counts, so this needs a counting connector; with the per-origin state.
