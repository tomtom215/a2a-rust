<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# A Shared Rate-Limit Counter

`RateLimitInterceptor` counts in a process-local map. That is correct for one
process and wrong for two: each replica admits the full configured rate, so
**N replicas admit N times the limit**. Two limiters configured for 5 requests
per window admit 10 — measured, not inferred.

If the limiter exists to protect an upstream with a real quota, that is the
whole point of it defeated, and it fails in the direction that matters least
when you have one replica and most when you have twenty.

`with_shared_counter` moves the count somewhere every replica can see:

```rust
# use std::sync::Arc;
# use a2a_protocol_server::{PostgresRateLimitCounter, RateLimitConfig, RateLimitInterceptor};
# async fn example(database_url: String) -> Result<(), Box<dyn std::error::Error>> {
let counter = Arc::new(PostgresRateLimitCounter::new(&database_url).await?);
let limiter = RateLimitInterceptor::new(
    RateLimitConfig::default()
        .with_requests_per_window(100)
        .with_window(std::time::Duration::from_secs(60)),
)?
.with_shared_counter(counter);
# Ok(())
# }
```

Every replica now increments the same row, and the limit is the deployment's.

## Give each caller its own key

A shared counter makes the limit global. It does not make it *per caller* —
that depends on `CallContext::caller_identity`, and only an authentication
interceptor can supply one:

- `JwtAuthInterceptor` records the validated issuer and `sub` automatically,
  as `jwt:<issuer>:<sub>` (the issuer percent-encodes `%` and `:`).
- `ApiKeyAuthInterceptor::with_labelled_keys` and
  `BearerTokenAuthInterceptor::with_labelled_tokens` record a label you
  choose, as `api-key:<label>` and `bearer:<label>`.
- `MutualTlsAuthInterceptor` records the client certificate's fingerprint (or
  a name you map it to) as `mtls:<name>`.

The label is separate from the credential on purpose. A caller key reaches the
rate-limit table your replicas share, and can reach logs and metrics; a bearer
token or API key belongs in none of those.

An identity does more than key the limiter: it also gives the caller a task
partition of its own (spec §13.1; the default with an authenticating
interceptor since 0.15), which only stores that partition by tenant can hold.
With identified callers, use `TenantAwarePostgresTaskStore` and
`TenantAwarePostgresPushConfigStore` in place of the plain stores ([Sharing the store](./shared-stores.md)), or
call `share_tasks_across_callers()` if callers are meant to see each other's
tasks. `build()` refuses the plain stores until you choose. See
[Multi-tenancy](../multi-tenancy.md#callers-within-one-tenant).

**Register authentication before the limiter.** The chain runs interceptors in
registration order over one context, so a limiter registered first reads an
identity nothing has set yet and buckets every caller together. Nothing rejects
that ordering — it just quietly stops being per-caller.

Without any of this an unidentified caller is keyed by its client address:
with `trusted_proxy_hops` set, the `trusted_proxy_hops`-th entry from the
right of `X-Forwarded-For`; with it zero, the connection's peer address (an
IPv6 client by its `/64`). A client that reaches the server without going
through the proxy can send that header itself, so name the proxies' address
ranges as well: `with_trusted_proxies(["10.0.0.0/8".parse()?])` believes the
header only when the connection's peer is in one of them, and keys any other
connection by its own address (before this, the hop count alone was believed,
and an entry with a port, `203.0.113.7:51234`, made a new key for each
connection; the port is now dropped). Hops set with no ranges named log a
warning when the limiter (or `AuthFailureThrottle`) is built. A proxy that
writes RFC 7239 `Forwarded` instead (`for=192.0.2.60;proto=https`) is read with
`with_forwarded_header(ForwardedHeader::Forwarded)`: only the configured header
is read, since a proxy that writes one passes a client's own copy of the other
through. A forwarded chain shorter than `trusted_proxy_hops`, or an element
whose `for` is missing or `unknown`, is keyed by the connection's peer (the
proxy), so the callers through that proxy share its budget and no one else's;
until 0.16.0 they shared one `anon` bucket with every such caller, which a
client connecting directly with a short header could exhaust for all of them.
Only a transport that records no peer falls back to `anon`. Keying by the peer
address is new in 0.15; before, every caller with no identity and no trusted
forwarded address shared one bucket.

## What it costs

A round trip on every request, and the figures are not small — loopback,
release build, best of three runs of 2,000 requests:

| Counter | Per request |
|---|---:|
| In-process (the default) | 0.2 µs |
| `PostgresRateLimitCounter` | 232 µs |
| The same on a durable pool | 598 µs |

For scale, a complete JSON-RPC request through this server measures ~195 µs on
the same machine. A shared counter roughly doubles the cost of a request, and a
counter across a real network costs whatever that network costs.

That is why it is opt-in. A single-replica deployment gains nothing and should
not pay it. A deployment that needs both a global limit and the last
microsecond should implement `RateLimitCounter` against Redis or another
in-memory keyspace — the trait has one method, so that is a few lines.

`PostgresRateLimitCounter::new` runs its own pool with
`synchronous_commit = off`, which is where the 598 µs → 232 µs comes from. A
rate-limit count describes one window and is swept away shortly after, so the
worst a crash can do is forget that a caller had spent part of its budget. That
setting is **not** applied by `from_pool`, because that pool is usually the task
store's, and it is not a counter's business to make a task store non-durable.

## Whose clock numbers the window

`PostgresRateLimitCounter` numbers windows by the **database's** clock
(`floor(epoch / window_secs)` evaluated by PostgreSQL), not by each replica's.
The window number the interceptor computes from its own clock is passed to the
counter and ignored by this one. Replicas whose clocks differ therefore count in
the same row, and one replica's sweep, which keeps the previous window and drops
older ones, cannot delete a window another replica is still counting in. A
`RateLimitCounter` of your own, backed by a store with a clock every replica
shares, can do the same; one with no clock of its own should count under the
`window` it is given.

The `Retry-After` on a refusal is still computed from the replica's own clock,
so with a skewed replica it can be off by the skew.

## When the counter is down

The request is counted locally instead. The failure mode is exactly the
per-replica behaviour you had before adopting a shared counter — not an outage,
and not an open door. Failing closed would make adding a shared limiter a
reliability regression; failing open would remove the limit at the moment an
attacker with database access most wants it gone.
