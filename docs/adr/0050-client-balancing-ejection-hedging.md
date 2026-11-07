<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# ADR 0050: Clients balance over a set of endpoints, eject the failing ones, and hedge idempotent reads, all opt-in; per-origin state lives in the pool

**Date:** 2026-11-04
**Status:** Accepted (maintainer's instruction of 2026-11-04: every open plan
item is in 0.16.0, built to the best engineering option; the maintainer may
veto in review). Amends ADR 0032: its decision 5 (per-origin in-flight cap,
circuit breaker, retry budget, passive ejection) and its H12 section
(load balancing and hedging), which was conditional on a measurement.
**Author:** Tom F.

---

## Context

Audit H12: a client targets one URL (`ClientBuilder::new(endpoint)`), and over
`https://` its calls to one peer multiplex on one HTTP/2 connection (ALPN in
`proxy.rs`). A layer-4 balancer (a Kubernetes `ClusterIP` Service, HAProxy
`mode tcp`) picks a replica per *connection*, so every call and stream of a
long-lived client lands on one replica. A supervisor that fans out to a
service sees one replica loaded and the others idle; one replica that fails
fails every call, and nothing on the client notices it is the replica and not
the service. ADR 0032 added `with_max_connection_lifetime`, which spreads
connections over time, not calls at any moment.

ADR 0032 decision 5 put per-origin state in the pool (calls in flight,
consecutive failures, an "ejected until") for three opt-in policies (an
in-flight cap, a circuit breaker with a retry budget, passive ejection) and
left all of it unbuilt. The per-client forms exist (`with_max_in_flight`,
`RetryPolicy::with_budget`, PR #193). ADR 0032 also asked for a measurement
before building a balancer: that layer-4 balancing leaves a fan-out uneven.

That measurement is a property of HTTP/2, not of a deployment, and it can be
shown on one host: a layer-4 forwarder that sends each new TCP connection to
the next of three backends, and one client speaking HTTP/2 through it, puts
every call on one backend (`tests/balancing/premise.rs`; the same calls over
the balancer of this ADR spread over all three). The maintainer's instruction
of 2026-11-04 supersedes the recorded "decline in core" (H-D7).

## Options

| Option | For | Against |
|---|---|---|
| **A. Decline; document layer-7 balancing and headless Services** (H-D7) | No code to keep; meshes already do it | Leaves every client without a mesh pinned to one replica over HTTPS, and with no notion of a failing replica; the docs say what to buy, not what the SDK does |
| **B. An opt-in balancer in the client over an `EndpointSource`**: endpoints from a list, a card, or DNS; the fewest calls in flight; passive ejection with a half-open probe; hedging for idempotent calls only, capped by a budget | Fixes the pinning and the single failing replica where the client is; additive; small policies, each with a test | A runtime feature to maintain; overlaps with meshes for those who have one |
| **C. B plus active health checks and DNS SRV/TTL** | Finds a dead replica before a call does | A background task per client per endpoint, a health endpoint A2A does not define, and SRV/TTL need a resolver crate; "never in core" in the triage |

**Chosen: B.** C is not built (see the end).

## Decision

1. **Endpoints.** `balance::Endpoint` is a URL, optionally *pinned* to one
   socket address: the URL's host still names the peer (the `Host` header,
   TLS SNI and certificate verification), and only the TCP connection goes to
   the address. `balance::EndpointSource` yields the set:
   * `StaticEndpoints::new([..])`: a list of replica URLs.
   * `StaticEndpoints::from_card(card, binding)`: the card's interfaces of one
     binding (and one tenant), when the agent publishes one URL per replica.
   * `DnsEndpoints::new(url)`: every address the URL's host resolves to (the
     system resolver), one pinned endpoint each, re-resolved every 30 s
     (`with_refresh_every`). This is the Kubernetes headless Service case: one
     name, one address per pod, one certificate for the name.
   A source is asked on the first call and again once its interval has passed,
   by one caller at a time (single-flight) while the others use the set they
   have. A refresh that fails or returns nothing keeps the set it had.
2. **Selection: fewest calls in flight.** The endpoint in rotation with the
   fewest calls in flight is used; a tie goes to the one chosen longest ago,
   so calls one after another go round the set. A stream counts as in flight
   until it is dropped, because the H12 load is long streams. ADR 0032
   proposed power of two choices (draw two at random, take the less loaded);
   that pays where the load figures are stale or shared by many choosers. Here
   the counts are the client's own (or, on a pool, an atomic count every
   client of the pool updates), read at the moment of choosing, and sets are
   small (replicas of one agent), so a full scan is exact, costs one pass, and
   gives a deterministic order a test can assert. Cumulative counts are not
   used to break ties: an endpoint back from an ejection would otherwise take
   every call until it caught up.
3. **Ejection, with a half-open probe** (`Ejection`). An endpoint that fails
   `consecutive_failures` (5) calls in a row is ejected for `base` (30 s); the
   *n*-th consecutive ejection lasts `base * 2^(n-1)`, capped at `max`
   (300 s). When the ejection ends, one call is let through as a probe: its
   success returns the endpoint to rotation and resets the count, its failure
   ejects it again for the next duration. A failure is what says the replica,
   not the request, is wrong: a connection error, a timeout, `502`, `503`,
   `504`. Any other answer, an error status or a protocol error included, is
   a success for this purpose (the replica answered). A call refused locally
   (an in-flight cap, an open breaker) or cancelled records nothing.
4. **Never eject the set empty** (`BalancePolicy::with_max_ejected_percent`,
   default 50). No more than that share of the set (rounded down) is kept out
   of rotation at once; past it, the endpoints whose ejection ends soonest are
   used as probes. A blip that fails every replica therefore does not become a
   client-side outage. `100` makes the balancer a circuit breaker over the
   set: with every endpoint ejected, calls fail fast with
   `ClientError::CircuitOpen`.
5. **Hedging, opt-in, for idempotent calls only** (`Hedge`). A unary call that
   has had no answer after `delay` is sent to a second endpoint; the first
   success is returned and the other call is dropped (cancelled). Only the
   methods the retry layer already treats as idempotent (`GetTask`,
   `ListTasks`, `CancelTask`, `GetExtendedAgentCard`, the push-config reads
   and delete) and a `SendMessage` that carries an idempotency key to a peer
   known to honour it are hedged; **a send without a key never is**. Streams
   are not hedged. Each hedge is paid from a `RetryBudget` (default: 5% of
   calls, a reserve of one), so hedging adds a bounded fraction of load.
6. **Per-origin state lives in the pool** (ADR 0032 decision 5, built). An
   origin is a scheme, host and port, plus the pinned address if any. For each
   it holds the calls in flight, the ejection state above, an optional
   in-flight cap and an optional retry-budget ledger.
   * `HttpPoolBuilder::with_ejection(Ejection)`: the **circuit breaker**. A
     client on the pool that is not balanced refuses calls to an ejected
     origin with `ClientError::CircuitOpen` (retryable, and safe to retry for
     any method: nothing was sent) until the probe; a balanced client uses
     the same state for its rotation. Clients of one pool share it, so one
     client's failures eject the origin for all of them.
   * `HttpPoolBuilder::with_max_in_flight_per_origin(n)`: a call that would
     be the `n+1`-th in flight to an origin is refused with
     `TooManyPendingRequests`, across every client of the pool.
   * `HttpPoolBuilder::with_retry_budget(RetryBudget)`: one ledger per
     origin of the client's configured endpoint, shared by every client of
     the pool. A retry is made only when the client's own budget (if any) and
     the pool's can both pay for it, so a thousand clients retrying towards
     one peer are bounded together (H6).
   * `HttpPool::origins()`: a snapshot per origin (calls in flight, ejected
     or not, how many times in a row), for gauges.
   A balanced client without a pool keeps the same state for itself. A
   balanced client given a pool uses the pool's ejection policy; calling
   `BalancePolicy::with_ejection` as well fails at `build()` (ADR 0032's rule:
   the setting is the pool's, refused rather than ignored).
7. **API.** `ClientBuilder::with_balancing(source, policy)`; the builder's own
   endpoint stays the client's name (spans, the plaintext check), and every
   endpoint of the set must have its scheme. The HTTP bindings (JSON-RPC,
   HTTP+JSON) only: `build_grpc` and a custom transport refuse it (tonic has
   its own balancer). Nothing changes for a client that does not ask.

## Consequences

* Additive, except one new `ClientError` variant (`CircuitOpen`; the enum is
  `#[non_exhaustive]`).
* The retry layer sits above the balancer, so a retry is a new selection and
  usually reaches another replica.
* Over a proxy, a pinned endpoint is reached through the proxy's `CONNECT` to
  the host name; the proxy resolves it, so DNS balancing through a proxy
  reaches whatever the proxy chooses. Documented.
* Pinned endpoints need `tls-rustls` (the connector that pins is that
  feature's); without it, a pinned endpoint fails to build with a
  configuration error.

## Not built, and why

* **Active health checks** (option C): A2A defines no health endpoint, and a
  probe call per endpoint per interval is load the passive policy does not add.
* **DNS SRV and record TTLs**: the system resolver gives neither; a fixed
  re-resolution interval is what it allows. A caller who has a better source
  implements `EndpointSource`.
* **Load from the server** (`A2A-Load`, swarm report gap 7): no server sends
  it yet; calls in flight are the client's own measure.
* **Metrics through OpenTelemetry**: the state is exposed by
  `HttpPool::origins()` and transitions are logged (`tracing`), not exported
  as instruments.
* **gRPC and WebSocket**: one long-lived connection each, own connectors.
