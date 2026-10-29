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
| `Last-Modified` | Static handler only: when it loaded the card |
| `Cache-Control` | `public, max-age=3600` (configurable: `DispatchConfig::with_agent_card_max_age`, and `with_agent_card_max_age` on `WebSocketDispatcher` and `GrpcDispatcher`) |

Clients revalidate with `If-None-Match`, or with `If-Modified-Since` against the static handler. If the card hasn't changed, the server returns `304 Not Modified` with no body. The dynamic handler sends no `Last-Modified` and ignores `If-Modified-Since`: a card produced per request has no modification date, so the `ETag` is its only validator.

## Client cache

On the client, `discovery::CachingCardResolver` is a private HTTP cache of one card (spec §8.6.2, RFC 9111). `resolve()` returns the stored card without a request while it is fresh, for its `max-age`, or else its `Expires`, less the age it arrived with (`Age`, `Date`), and revalidates it with `If-None-Match` / `If-Modified-Since` once it is stale. A card marked `no-cache` is revalidated on every resolve, and a `no-store` card is not stored.

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
