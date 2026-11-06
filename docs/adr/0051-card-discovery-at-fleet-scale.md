<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# ADR 0051: Card resolution keeps one HTTP client, makes one fetch at a time, may serve a stale card on error, and is shared across URLs and clients

**Date:** 2026-11-04
**Status:** Accepted (maintainer's instruction of 2026-11-04; the maintainer
may veto in review). Builds ADR 0032 decision 6. ADR 0031's merged source
cache stays with ADR 0031.
**Author:** Tom F.

---

## Context

Audit H8 (severity medium since c4404e8 made the default TLS configuration
once per process): a fleet that starts together fetches every agent card at
once, and `CachingCardResolver` made that worse in four ways, each read in
`discovery.rs` at this branch's head:

1. Every fetch built a new HTTP client, so a revalidation never reused the
   connection of the fetch before it (`CardFetchOptions::https_client`, called
   from `fetch_inner`).
2. Concurrent callers holding a stale card each fetched: N callers, N requests
   (no single-flight in `resolve`).
3. A revalidation that failed returned the error even while a usable card
   was held (the `?` after `fetch_card_with_metadata`).
4. One resolver covers one URL; a supervisor with many peers holds and
   configures many, and nothing shares them between its clients.

c4404e8's claim (roots and session cache built once) had no test.

## Options

| Option | For | Against |
|---|---|---|
| **A. Fix it inside `CachingCardResolver`**: one client per resolver (or the pool's), single-flight, opt-in stale-if-error, opt-in jittered expiry | Small, additive, no new type | Leaves the many-URL case to the caller |
| **B. A plus a URL-keyed `CardCache`** that hands out one shared resolver per URL, with the same options and pool, bounded | One object for a fleet's discovery; clones share it across clients; still additive | One more public type |
| **C. B plus background refresh at ~80% of freshness and a `watch()` of card changes** | Hides the revalidation latency | A task per cached card with a lifetime the SDK does not own, and a change stream nobody has asked for yet |

**Chosen: B.** C is not built.

## Decision

1. **One client per resolver.** A resolver builds its HTTP client once, when
   it is made or given new options, and every fetch of it and its clones uses
   that one: revalidations reuse the connection. `with_pool(&pool)` uses an
   `HttpPool`'s connections instead; a pool has one TLS configuration, so a
   resolver whose options also carry one fails each fetch with
   `ClientError::Config` naming both, rather than ignoring one.
2. **Single-flight.** While a fetch for a resolver is running, other
   `resolve` calls on it (and its clones) wait for that fetch and get its
   outcome, success or error; they do not fetch. The mechanism is the token
   cache's (`token_provider/flight.rs`), generalised: a leader publishes a
   `watch` channel, and a leader that is cancelled hands the attempt to the
   next caller instead of failing it.
3. **Stale-if-error (RFC 5861 §4).** When a revalidation fails because the
   server could not be reached or answered `500`, `502`, `503` or `504` (or
   the fetch timed out), the stored card is returned if it went stale no more
   than the window ago. The window is the larger of
   `with_stale_if_error(Duration)` (default: none) and the response's own
   `Cache-Control: stale-if-error=N`. A card marked `must-revalidate` or
   `no-cache` is never served stale, and neither is one past its window, nor
   one when the fetch failed for another reason (a `404`, a card that does
   not verify, a card that does not parse). **Behaviour change**: a server
   that sends `stale-if-error` gets it honoured without the caller opting in,
   which is what the directive asks of a cache. *Amended 2026-11-06*: the
   first build did not keep a card that had no `ETag`, no `Last-Modified` and
   no lifetime, so for the cards simple agents actually send, neither the
   directive nor the configured window did anything. RFC 5861 §4 makes the
   directive apply "regardless of other freshness information", and RFC 9111
   §4.2.4 permits serving stale by "configuration in accordance with an
   out-of-band contract", which `with_stale_if_error` is. Such a card is now
   kept when a window (its own or the configured one) can apply to it; with no
   window, as before, it is not kept, since nothing could use it. It is stale
   from the moment it arrives, so the window counts from then.
4. **Jittered expiry, opt-in on a resolver.** `with_refresh_jitter(f)` (a
   fraction from 0 to 0.5) shortens each stored card's freshness by a random
   part of up to `f` of its lifetime, never lengthens it, so processes that
   fetched together revalidate apart. Default 0 on `CachingCardResolver`,
   whose freshness tests pin RFC 9111 to the millisecond.
5. **`CardCache`**: a cache of resolvers keyed by card URL. Every resolver it
   makes has the cache's options, pool, freshness and stale-if-error settings;
   the same URL gives the same (shared) resolver, so every client a supervisor
   builds from it shares one card per peer. Bounded (`with_max_entries`,
   default 1024): past the bound the least recently used URL is dropped.
   Jitter defaults to 10% here, because a cache is what a fleet holds.
   Because all its resolvers have one set of request headers, a card fetched
   under one credential is never served to another (RFC 9111 §4.1): a second
   credential is a second `CardCache`.
6. **The once-per-process TLS configuration is pinned by a test**: two calls
   of `default_tls_config()` share the crypto provider and the client
   certificate resolver (`Arc::ptr_eq`), which a rebuild would not.

## Consequences

* Additive API (`with_pool`, `with_stale_if_error`, `with_refresh_jitter`,
  `CardCache`); one behaviour change (3).
* The free functions (`resolve_agent_card`, `fetch_card_from_url`, their
  `_with_options` forms) still make one client per call: they are documented
  as one-shot and cache nothing, and sharing connections behind them would
  also freeze the proxy environment they read per call.

## Not built, and why

* Background refresh and `watch()` (option C): see the table.
* ADR 0031's merged, jittered cache over discovery sources: that ADR's, with
  the discovery crate.
* The swarm measurement of 512 simultaneous fetches (`swarm_scale` card arm):
  not run here; the tests count requests and connections instead.
