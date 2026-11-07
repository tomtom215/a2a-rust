// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The URL-keyed [`CardCache`] and resolvers on an `HttpPool`, against the
//! counting server of the parent module.

use super::super::{CardCache, CardFetchOptions};
use super::{SECOND, Server, ok};
#[cfg(feature = "tls-rustls")]
use {super::super::CachingCardResolver, crate::error::ClientError};

// ── CardCache ────────────────────────────────────────────────────────────────

#[tokio::test]
async fn one_url_is_one_shared_resolver() {
    let cache = CardCache::new();
    let a = cache.resolver("http://a.example").unwrap();
    let again = cache.resolver("http://a.example/").unwrap();
    let b = cache.resolver("http://b.example").unwrap();
    assert!(a.shares_cache_with(&again));
    assert!(!a.shares_cache_with(&b));
    assert_eq!(cache.len(), 2);
    assert!(!cache.is_empty());
    assert!(CardCache::new().is_empty());
}

/// Past the bound the least recently used URL goes; a recently used one
/// stays.
#[tokio::test]
async fn the_least_recently_used_url_is_dropped() {
    let cache = CardCache::new().with_max_entries(2);
    let a = cache.resolver("http://a.example").unwrap();
    let b = cache.resolver("http://b.example").unwrap();
    let _ = cache.resolver("http://a.example").unwrap();
    let _ = cache.resolver("http://c.example").unwrap();
    assert_eq!(cache.len(), 2);
    assert!(a.shares_cache_with(&cache.resolver("http://a.example").unwrap()));
    assert!(!b.shares_cache_with(&cache.resolver("http://b.example").unwrap()));
}

#[tokio::test]
async fn a_bound_of_zero_keeps_one() {
    let cache = CardCache::new().with_max_entries(0);
    let _ = cache.resolver("http://a.example").unwrap();
    assert_eq!(cache.len(), 1);
}

/// Clients resolving one peer through clones of one cache make one request.
#[tokio::test]
async fn clones_of_a_cache_share_one_fetch() {
    let server = Server::start(ok(&[("cache-control", "max-age=60")])).await;
    let gate = server.hold();
    let cache = CardCache::new();
    let calls: Vec<_> = (0..6)
        .map(|_| {
            let (cache, url) = (cache.clone(), server.url.clone());
            tokio::spawn(async move { cache.resolve(&url).await })
        })
        .collect();
    let resolver = cache.resolver(&server.url).unwrap();
    while server.requests() == 0 || resolver.flight.waiters() < 5 {
        tokio::task::yield_now().await;
    }
    gate.add_permits(1);
    for call in calls {
        call.await.unwrap().expect("card");
    }
    cache
        .resolve(&server.url)
        .await
        .expect("fresh, from the cache");
    assert_eq!(server.requests(), 1);
}

/// Every resolver of a cache carries its settings.
#[tokio::test]
async fn a_caches_resolvers_carry_its_settings() {
    let cache = CardCache::new()
        .with_path("/card.json")
        .with_always_revalidate(true)
        .with_default_freshness(7 * SECOND)
        .with_stale_if_error(9 * SECOND)
        .with_refresh_jitter(0.2);
    let r = cache.resolver("http://a.example").unwrap();
    assert_eq!(r.url(), "http://a.example/card.json");
    assert!(r.always_revalidate);
    assert_eq!(r.default_freshness, Some(7 * SECOND));
    assert_eq!(r.stale_if_error, Some(9 * SECOND));
    assert!((r.jitter - 0.2).abs() < f64::EPSILON);
    let plain = CardCache::new().resolver("http://a.example").unwrap();
    assert_eq!(plain.url(), "http://a.example/.well-known/agent-card.json");
    assert!(!plain.always_revalidate);
    assert_eq!(plain.default_freshness, None);
    assert_eq!(plain.stale_if_error, None);
    assert!((plain.jitter - 0.1).abs() < f64::EPSILON);
}

/// The options a cache is given are the ones it keeps, and so the ones its
/// resolvers send: its `Debug` shows the header names and the budget.
#[test]
fn a_cache_keeps_the_options_it_was_given() {
    let options = CardFetchOptions::default()
        .with_timeout(3 * SECOND)
        .with_header("x-tenant", "t-1");
    let text = format!("{:?}", CardCache::new().with_options(options));
    assert!(text.contains("x-tenant"), "{text}");
    assert!(text.contains("timeout: 3s"), "{text}");
    let plain = format!("{:?}", CardCache::new());
    assert!(
        !plain.contains("x-tenant") && !plain.contains("timeout: 3s"),
        "{plain}"
    );
}

/// The URL dropped is the one used longest ago, whatever order the map keeps
/// them in: seven URLs through a cache that keeps six, the first not used
/// again. A fresh cache each time, because a map's order is random per map,
/// and a cache that dropped an arbitrary entry would be right one time in seven.
#[tokio::test]
async fn the_url_dropped_is_the_one_used_longest_ago() {
    for _ in 0..6 {
        let cache = CardCache::new().with_max_entries(6);
        let urls: Vec<String> = (0..7).map(|i| format!("http://u{i}.example")).collect();
        let held: Vec<_> = urls[..6]
            .iter()
            .map(|u| cache.resolver(u).unwrap())
            .collect();
        for url in &urls[1..6] {
            let _ = cache.resolver(url).unwrap();
        }
        let _ = cache.resolver(&urls[6]).unwrap();
        assert_eq!(cache.len(), 6);
        for (resolver, url) in held.iter().zip(&urls).skip(1) {
            assert!(
                resolver.shares_cache_with(&cache.resolver(url).unwrap()),
                "{url} was used after u0 and must still be cached"
            );
        }
        assert!(
            !held[0].shares_cache_with(&cache.resolver(&urls[0]).unwrap()),
            "u0, the least recently used, must be the one dropped"
        );
    }
}

/// The `Debug` of a cache names no URL (a card URL may carry a key).
#[test]
fn a_caches_debug_names_no_url() {
    let cache = CardCache::default();
    let _ = cache.resolver("http://a.example/?key=s3cret").unwrap();
    let text = format!("{cache:?}");
    assert!(
        text.starts_with("CardCache { entries: 1, max_entries: 1024"),
        "{text}"
    );
    assert!(!text.contains("s3cret"), "{text}");
}

// ── The pool ─────────────────────────────────────────────────────────────────

/// Two resolvers for two paths on one host, given one pool, use one
/// connection.
#[cfg(feature = "tls-rustls")]
#[tokio::test]
async fn resolvers_on_one_pool_share_its_connections() {
    let server = Server::start(ok(&[])).await;
    let pool = crate::HttpPool::builder().build().unwrap();
    let a = CachingCardResolver::with_path(&server.url, "/a.json")
        .unwrap()
        .with_pool(&pool);
    let cache = CardCache::new().with_pool(&pool).with_path("/b.json");
    a.resolve().await.expect("a");
    cache.resolve(&server.url).await.expect("b");
    assert_eq!(server.requests(), 2);
    assert_eq!(server.accepted(), 1);
}

/// A pool has one TLS configuration: options with another are refused at
/// the fetch, before anything is sent.
#[cfg(feature = "tls-rustls")]
#[tokio::test]
async fn a_pool_and_a_tls_configuration_conflict() {
    let server = Server::start(ok(&[])).await;
    let pool = crate::HttpPool::builder().build().unwrap();
    let resolver = CachingCardResolver::new(&server.url)
        .unwrap()
        .with_pool(&pool)
        .with_options(
            super::super::CardFetchOptions::default()
                .with_tls_config(crate::tls::default_tls_config()),
        );
    let err = resolver.resolve().await.expect_err("conflict");
    assert!(
        matches!(&err, ClientError::Config(m) if m.contains("with_pool") && m.contains("with_tls_config")),
        "{err:?}"
    );
    assert_eq!(server.requests(), 0);
}

/// New options on a pooled resolver keep the pool's connections.
#[cfg(feature = "tls-rustls")]
#[tokio::test]
async fn new_options_keep_the_pool() {
    let server = Server::start(ok(&[])).await;
    let pool = crate::HttpPool::builder().build().unwrap();
    let a = CachingCardResolver::with_path(&server.url, "/a.json")
        .unwrap()
        .with_pool(&pool)
        .with_options(super::super::CardFetchOptions::default().with_header("x-k", "v"));
    let b = CachingCardResolver::with_path(&server.url, "/b.json")
        .unwrap()
        .with_pool(&pool);
    a.resolve().await.expect("a");
    b.resolve().await.expect("b");
    assert_eq!(server.requests(), 2);
    assert_eq!(server.accepted(), 1, "one pool connection for both");
}
