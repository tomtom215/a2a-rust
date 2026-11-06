<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# HTTP caching and client checks

This page covers how a served card is cached and revalidated, the client's
own card cache, and the checks a client applies to a card it receives.

## Server headers

Agent card responses include standard HTTP caching headers (spec §8.6, RFC 9110):

| Header | Purpose |
|--------|---------|
| `ETag` | Content hash for cache validation |
| `Last-Modified` | Static handler only: when the running executable was last modified (not when the process started), or `StaticAgentCardHandler::with_last_modified` |
| `Cache-Control` | `public, max-age=3600` (configurable: `DispatchConfig::with_agent_card_max_age`, and `with_agent_card_max_age` on `WebSocketDispatcher` and `GrpcDispatcher`) |

Clients revalidate with `If-None-Match`, or with `If-Modified-Since` against the static handler. If the card hasn't changed, the server returns `304 Not Modified` with no body. The dynamic handler sends no `Last-Modified` and ignores `If-Modified-Since`: a card produced per request has no modification date, so the `ETag` is its only validator.

### Behind several replicas

Replicas of one build should send one card, with one validator. Two things decide it:

- **`Last-Modified`** is the running executable's modification time, so replicas of one build agree whatever time each started. (Before 0.16.0 it was the time each process started, so an `If-Modified-Since` revalidation missed on every replica but the one that issued the date; `If-None-Match` was never affected, and a client that sends it is decided by the `ETag` alone.) Where the executable's time cannot be read, or lies ahead of the clock, it is the time of construction. If you know the card's real date, `StaticAgentCardHandler::with_last_modified` sets it.
- **The `ETag`** is a hash of the card's JSON, so replicas agree while the card is the same bytes. A card **signed with ES256 at start-up** is not: each signature differs, so each replica's card, and its `ETag`, differs and a revalidation that reaches another replica is a miss (about `(N-1)/N` of them). Sign the card **once, offline**, and ship the signed card to every replica, or sign with Ed25519, whose signatures are deterministic. This is a cache-efficiency and signature-pinning concern; no replica serves stale data.

## Client cache

On the client, `discovery::CachingCardResolver` is a private HTTP cache of one card (spec §8.6.2, RFC 9111). `resolve()` returns the stored card without a request while it is fresh, for its `max-age`, or else its `Expires`, less the age it arrived with (`Age`, `Date`), and revalidates it with `If-None-Match` / `If-Modified-Since` once it is stale. A `304` freshens it: the `Cache-Control`, `Expires` or `Vary` the `304` carries replace the stored ones, a field it leaves out stays in force, and its own `Date` and `Age` are the ones that count (RFC 9111 §4.3.4). A card marked `no-cache` is revalidated on every resolve, and a `no-store` card is not stored.

```rust,no_run
use std::time::Duration;
use a2a_protocol_client::discovery::CachingCardResolver;

# async fn example() -> Result<(), a2a_protocol_client::error::ClientError> {
let resolver = CachingCardResolver::new("https://agent.example.com")?
    // A card sent with no `max-age` and no `Expires` stays fresh this long
    // (off by default: such a card is revalidated on every resolve).
    .with_default_freshness(Duration::from_secs(300));
let card = resolver.resolve().await?; // fetched
let card = resolver.resolve().await?; // from the cache while fresh
# let _ = card;
# Ok(())
# }
```

`with_always_revalidate(true)` sends a request on every resolve, as the resolver did before 0.15, for an agent whose card changes sooner than its `max-age` says; a hot-reloaded card served with the default `max-age=3600` is the usual case. `resolve_agent_card` and `fetch_card_from_url` cache nothing: each call fetches.

### Many agents, many processes

A fleet that starts together asks for every card at once. Since 0.16.0 (ADR 0051) the resolver is built for that:

- **One connection.** A resolver builds its HTTP client once, so a revalidation reuses the connection of the fetch before it. `with_pool(&pool)` uses an [`HttpPool`](../../client/builder/pool.md)'s connections, shared with the clients on that pool; a pool has one TLS configuration, so options that carry their own make every resolve fail with a configuration error.
- **One fetch at a time.** While a fetch is running, other `resolve` calls on the resolver and its clones wait for it and share its outcome, error included: a hundred tasks holding a stale card make one request.
- **Stale if error** (RFC 5861). When a revalidation fails because the agent cannot be reached, timed out, or answered `500`, `502`, `503` or `504`, the stored card is returned if it went stale no longer ago than the window: the larger of `with_stale_if_error(duration)` and the card's own `Cache-Control: stale-if-error=N`. A card marked `must-revalidate` or `no-cache` is never returned stale, and a `404`, or a card that does not parse or verify, is returned as the error it is. The card's own directive is honoured without opting in. This holds for a card with no `ETag`, no `Last-Modified` and no `max-age`, which most hand-written agents send: it is stale as it arrives, so every resolve fetches it whole, and the window counts from its arrival.
- **Spread revalidations.** `with_refresh_jitter(0.1)` shortens each card's freshness by a random share of up to 10% of its lifetime (never lengthens it), so processes that fetched together revalidate apart. Off by default on a resolver.

`discovery::CardCache` holds one resolver per card URL, with one set of options, and hands the same resolver to every caller of one URL; clones share it. Its jitter is 10% by default, and it keeps at most 1024 URLs (`with_max_entries`), dropping the least recently used.

```rust,no_run
use std::time::Duration;
use a2a_protocol_client::ClientBuilder;
use a2a_protocol_client::discovery::CardCache;

# async fn example() -> Result<(), a2a_protocol_client::error::ClientError> {
let cards = CardCache::new()
    .with_default_freshness(Duration::from_secs(300))
    .with_stale_if_error(Duration::from_secs(600));
for peer in ["https://alpha.example", "https://beta.example"] {
    let client = ClientBuilder::from_card(&cards.resolve(peer).await?)?.build()?;
    # drop(client);
}
# Ok(())
# }
```

Every resolver of one `CardCache` sends the same request headers, so a card fetched under one credential is never served for another: use one cache per credential.

## What the client checks

A card the client receives is held to the rules the server applies before it
publishes one. `resolve_agent_card`, `fetch_card_from_url`,
`CachingCardResolver::resolve` and `get_extended_agent_card` fail with
`ClientError::InvalidAgentResponse` for a card that `AgentCard::validate` or
`validate_security_schemes` refuses, and a refused extended card is not
adopted. `ClientBuilder::from_card` does not validate the card it is handed,
but its `build()` fails with `ExtensionSupportRequired` when the card marks an
extension `required` that the client has not asked for with `with_extensions`
(call `skip_required_extension_check()` if an interceptor sends the
`A2A-Extensions` header).
