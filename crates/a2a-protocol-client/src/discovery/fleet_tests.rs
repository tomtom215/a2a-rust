// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Card resolution at fleet scale (ADR 0051, audit H8): one client per
//! resolver, one fetch at a time, stale-if-error, jittered expiry and the
//! URL-keyed [`CardCache`](super::CardCache). Each case counts what a real server on a socket
//! saw: the connections it accepted and the requests it answered. Time moves
//! by pausing tokio's clock between resolves, never while one is in flight.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use http_body_util::Full;
use hyper::body::Bytes;
use tokio::sync::Semaphore;

use super::CachingCardResolver;
use crate::error::ClientError;

/// What the server answers with.
#[derive(Clone)]
enum Mode {
    /// `200` with the card and these extra header fields.
    Ok(Vec<(&'static str, &'static str)>),
    /// This status and no card.
    Status(u16),
    /// Closes the connection without an answer.
    Refuse,
}

struct Server {
    url: String,
    accepted: AtomicUsize,
    requests: AtomicUsize,
    mode: Mutex<Mode>,
    /// When set, each request waits for a permit before it is answered.
    gate: Mutex<Option<Arc<Semaphore>>>,
}

impl Server {
    async fn start(mode: Mode) -> Arc<Self> {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let server = Arc::new(Self {
            url: format!("http://{}", listener.local_addr().expect("addr")),
            accepted: AtomicUsize::new(0),
            requests: AtomicUsize::new(0),
            mode: Mutex::new(mode),
            gate: Mutex::new(None),
        });
        let seen = Arc::clone(&server);
        tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                seen.accepted.fetch_add(1, Ordering::SeqCst);
                let s = Arc::clone(&seen);
                tokio::spawn(async move {
                    let service = hyper::service::service_fn(move |_req| {
                        let s = Arc::clone(&s);
                        async move { s.answer().await }
                    });
                    let _ = hyper::server::conn::http1::Builder::new()
                        .serve_connection(hyper_util::rt::TokioIo::new(stream), service)
                        .await;
                });
            }
        });
        server
    }

    async fn answer(&self) -> Result<hyper::Response<Full<Bytes>>, std::io::Error> {
        self.requests.fetch_add(1, Ordering::SeqCst);
        let gate = self.gate.lock().unwrap().clone();
        if let Some(gate) = gate {
            gate.acquire().await.expect("gate").forget();
        }
        let mode = self.mode.lock().unwrap().clone();
        match mode {
            Mode::Ok(fields) => {
                let mut resp = hyper::Response::builder()
                    .header("content-type", "application/json")
                    .header("etag", "\"v1\"");
                for (name, value) in fields {
                    resp = resp.header(name, value);
                }
                Ok(resp
                    .body(Full::new(Bytes::from(card_json())))
                    .expect("response"))
            }
            Mode::Status(status) => Ok(hyper::Response::builder()
                .status(status)
                .body(Full::new(Bytes::new()))
                .expect("response")),
            Mode::Refuse => Err(std::io::Error::other("refused on command")),
        }
    }

    fn set(&self, mode: Mode) {
        *self.mode.lock().unwrap() = mode;
    }

    fn hold(&self) -> Arc<Semaphore> {
        let gate = Arc::new(Semaphore::new(0));
        *self.gate.lock().unwrap() = Some(Arc::clone(&gate));
        gate
    }

    fn accepted(&self) -> usize {
        self.accepted.load(Ordering::SeqCst)
    }

    fn requests(&self) -> usize {
        self.requests.load(Ordering::SeqCst)
    }
}

fn card_json() -> String {
    let card = a2a_protocol_types::AgentCard::new(
        "fleet",
        "1.0.0",
        a2a_protocol_types::AgentInterface::jsonrpc("http://localhost:1"),
    )
    .with_description("d")
    .with_skill(a2a_protocol_types::AgentSkill::new("s", "S", "d").with_tags(["t"]));
    serde_json::to_string(&card).expect("card")
}

fn ok(fields: &[(&'static str, &'static str)]) -> Mode {
    Mode::Ok(fields.to_vec())
}

/// Moves tokio's clock on by `by`, between requests.
async fn elapse(by: Duration) {
    tokio::time::pause();
    tokio::time::advance(by).await;
    tokio::time::resume();
}

const SECOND: Duration = Duration::from_secs(1);

// ── One client per resolver ──────────────────────────────────────────────────

/// Three revalidations of one resolver, and of a clone of it, reuse one
/// connection. Before ADR 0051 each fetch built a client, so the server
/// accepted one connection per fetch.
#[tokio::test]
async fn revalidations_reuse_the_resolvers_connection() {
    let server = Server::start(ok(&[])).await;
    let resolver = CachingCardResolver::new(&server.url)
        .unwrap()
        .with_always_revalidate(true);
    let clone = resolver.clone();
    for r in [&resolver, &clone, &resolver] {
        assert_eq!(r.resolve().await.expect("card").name, "fleet");
    }
    assert_eq!(server.requests(), 3, "always revalidates");
    assert_eq!(server.accepted(), 1, "one connection for every fetch");
}

/// New options build a new client (they may carry another TLS
/// configuration); the cache starts over too, as before.
#[tokio::test]
async fn new_options_start_a_new_client() {
    let server = Server::start(ok(&[])).await;
    let resolver = CachingCardResolver::new(&server.url)
        .unwrap()
        .with_always_revalidate(true);
    resolver.resolve().await.expect("first");
    let resolver = resolver.with_options(super::CardFetchOptions::default());
    resolver.resolve().await.expect("second");
    assert_eq!(server.accepted(), 2);
}

/// c4404e8 (audit N-B-6): the default TLS configuration is built once per
/// process. A rebuild would make a new crypto provider and a new client
/// certificate resolver; clones of one configuration share both.
#[cfg(feature = "tls-rustls")]
#[test]
fn the_default_tls_configuration_is_built_once() {
    let a = crate::tls::default_tls_config();
    let b = crate::tls::default_tls_config();
    assert!(Arc::ptr_eq(a.crypto_provider(), b.crypto_provider()));
    assert!(Arc::ptr_eq(
        &a.client_auth_cert_resolver,
        &b.client_auth_cert_resolver
    ));
}

// ── Single-flight ────────────────────────────────────────────────────────────

/// Ten callers with nothing fresh stored make one request between them, and
/// all get its card.
#[tokio::test]
async fn concurrent_resolves_make_one_request() {
    let server = Server::start(ok(&[("cache-control", "max-age=60")])).await;
    let gate = server.hold();
    let resolver = CachingCardResolver::new(&server.url).unwrap();
    let calls: Vec<_> = (0..10)
        .map(|_| {
            let r = resolver.clone();
            tokio::spawn(async move { r.resolve().await })
        })
        .collect();
    while server.requests() == 0 || resolver.flight.waiters() < 9 {
        tokio::task::yield_now().await;
    }
    gate.add_permits(1);
    for call in calls {
        assert_eq!(call.await.unwrap().expect("card").name, "fleet");
    }
    assert_eq!(server.requests(), 1);
}

/// A failed fetch is shared too: five callers, one request, five errors.
#[tokio::test]
async fn a_failed_fetch_is_shared_not_repeated() {
    let server = Server::start(Mode::Status(503)).await;
    let gate = server.hold();
    let resolver = CachingCardResolver::new(&server.url).unwrap();
    let calls: Vec<_> = (0..5)
        .map(|_| {
            let r = resolver.clone();
            tokio::spawn(async move { r.resolve().await })
        })
        .collect();
    while server.requests() == 0 || resolver.flight.waiters() < 4 {
        tokio::task::yield_now().await;
    }
    gate.add_permits(1);
    for call in calls {
        let err = call.await.unwrap().expect_err("503");
        assert!(
            matches!(err, ClientError::UnexpectedStatus { status: 503, .. }),
            "{err:?}"
        );
    }
    assert_eq!(server.requests(), 1);
}

// ── Stale-if-error ───────────────────────────────────────────────────────────

/// A resolver holding a card that went stale `stale_by` ago, whose agent
/// now answers `then`.
///
/// A case that expects the card to be served must leave a second of its window
/// to spare, because the server's `Date` arrives up to a second old
/// (`stale_window.rs` says why).
async fn stale_resolver(
    fields: &[(&'static str, &'static str)],
    window: Option<Duration>,
    then: Mode,
    stale_by: Duration,
) -> (Arc<Server>, CachingCardResolver) {
    let server = Server::start(ok(fields)).await;
    let resolver = CachingCardResolver::new(&server.url).unwrap();
    let resolver = match window {
        Some(w) => resolver.with_stale_if_error(w),
        None => resolver,
    };
    resolver.resolve().await.expect("first fetch");
    server.set(then);
    // max-age=1 in every case: stale from one second on.
    elapse(SECOND + stale_by).await;
    (server, resolver)
}

#[tokio::test]
async fn the_configured_window_serves_the_stored_card_on_a_503() {
    let (server, resolver) = stale_resolver(
        &[("cache-control", "max-age=1")],
        Some(60 * SECOND),
        Mode::Status(503),
        58 * SECOND,
    )
    .await;
    assert_eq!(resolver.resolve().await.expect("stale card").name, "fleet");
    assert_eq!(server.requests(), 2, "the revalidation was tried");
}

#[tokio::test]
async fn past_the_window_the_error_is_returned() {
    let (_server, resolver) = stale_resolver(
        &[("cache-control", "max-age=1")],
        Some(60 * SECOND),
        Mode::Status(503),
        60 * SECOND + Duration::from_millis(1),
    )
    .await;
    let err = resolver.resolve().await.expect_err("too stale");
    assert!(matches!(
        err,
        ClientError::UnexpectedStatus { status: 503, .. }
    ));
}

#[tokio::test]
async fn without_a_window_a_failed_revalidation_is_an_error() {
    let (_server, resolver) = stale_resolver(
        &[("cache-control", "max-age=1")],
        None,
        Mode::Status(503),
        SECOND,
    )
    .await;
    resolver.resolve().await.expect_err("no window");
}

/// RFC 5861 §4: the response's own directive grants the window.
#[tokio::test]
async fn the_cards_stale_if_error_directive_is_honoured() {
    let (_server, resolver) = stale_resolver(
        &[("cache-control", "max-age=1, stale-if-error=30")],
        None,
        Mode::Status(502),
        28 * SECOND,
    )
    .await;
    assert_eq!(resolver.resolve().await.expect("stale card").name, "fleet");
    elapse(2 * SECOND + Duration::from_millis(1)).await;
    resolver
        .resolve()
        .await
        .expect_err("past the directive's window");
}

/// The larger of the two windows applies.
#[tokio::test]
async fn the_larger_window_wins() {
    let (_server, resolver) = stale_resolver(
        &[("cache-control", "max-age=1, stale-if-error=5")],
        Some(20 * SECOND),
        Mode::Status(504),
        18 * SECOND,
    )
    .await;
    resolver.resolve().await.expect("the configured 20 s");
}

/// A connection the agent drops is a want of the agent, as a `503` is.
#[tokio::test]
async fn a_dropped_connection_serves_the_stored_card() {
    let (_server, resolver) = stale_resolver(
        &[("cache-control", "max-age=1")],
        Some(60 * SECOND),
        Mode::Refuse,
        SECOND,
    )
    .await;
    assert_eq!(resolver.resolve().await.expect("stale card").name, "fleet");
}

#[tokio::test]
async fn must_revalidate_and_no_cache_are_never_served_stale() {
    for directive in ["max-age=1, must-revalidate", "max-age=1, no-cache"] {
        let server = Server::start(Mode::Ok(vec![("cache-control", directive)])).await;
        let resolver = CachingCardResolver::new(&server.url)
            .unwrap()
            .with_stale_if_error(60 * SECOND);
        resolver.resolve().await.expect("first");
        server.set(Mode::Status(503));
        elapse(2 * SECOND).await;
        resolver.resolve().await.expect_err(directive);
    }
}

/// A `404` is an answer: the card is gone, not unreachable.
#[tokio::test]
async fn a_404_is_returned_whatever_the_window() {
    let (_server, resolver) = stale_resolver(
        &[("cache-control", "max-age=1")],
        Some(60 * SECOND),
        Mode::Status(404),
        SECOND,
    )
    .await;
    let err = resolver.resolve().await.expect_err("404");
    assert!(matches!(
        err,
        ClientError::UnexpectedStatus { status: 404, .. }
    ));
}

// ── Jitter ───────────────────────────────────────────────────────────────────

/// The arithmetic, to the nanosecond: a 10 s card stored with a share of
/// 0.5 is fresh until 5 s after it arrived, and with no share until 10 s.
#[test]
fn jitter_brings_the_expiry_forward_by_its_share_of_the_lifetime() {
    let t0 = tokio::time::Instant::now();
    let mut headers = hyper::HeaderMap::new();
    headers.insert("cache-control", "max-age=10".parse().unwrap());
    let freshness = super::Freshness::of(&headers, t0, t0, std::time::SystemTime::now());
    let card: a2a_protocol_types::AgentCard = serde_json::from_str(&card_json()).unwrap();
    let cached = |jitter| super::CachedCard {
        card: card.clone(),
        etag: None,
        last_modified: None,
        freshness: freshness.clone(),
        jitter,
    };
    let resolver = CachingCardResolver::new("http://a.example").unwrap();
    assert_eq!(resolver.fresh_until(&cached(0.5)), Some(t0 + 5 * SECOND));
    assert_eq!(
        resolver.fresh_until(&cached(0.25)),
        Some(t0 + 7500 * Duration::from_millis(1))
    );
    assert_eq!(resolver.fresh_until(&cached(0.0)), Some(t0 + 10 * SECOND));
    let always = resolver.with_always_revalidate(true);
    assert_eq!(always.fresh_until(&cached(0.5)), None);
}

/// Over a socket: the largest draw takes the whole `fraction` off, so a 10 s
/// card with a jitter of 0.5 is revalidated after 5 s, not 10.
#[tokio::test]
async fn jitter_shortens_freshness_by_the_drawn_share() {
    let server = Server::start(ok(&[("cache-control", "max-age=10")])).await;
    let resolver = CachingCardResolver::new(&server.url)
        .unwrap()
        .with_refresh_jitter(0.5)
        .with_draw(|| u64::MAX);
    resolver.resolve().await.expect("first");
    elapse(4 * SECOND).await;
    resolver.resolve().await.expect("fresh");
    assert_eq!(server.requests(), 1);
    elapse(2 * SECOND).await;
    resolver.resolve().await.expect("revalidated");
    assert_eq!(server.requests(), 2);
}

/// The smallest draw changes nothing.
#[tokio::test]
async fn a_zero_draw_leaves_the_lifetime_whole() {
    let server = Server::start(ok(&[("cache-control", "max-age=10")])).await;
    let resolver = CachingCardResolver::new(&server.url)
        .unwrap()
        .with_refresh_jitter(0.5)
        .with_draw(|| 0);
    resolver.resolve().await.expect("first");
    elapse(9 * SECOND).await;
    resolver.resolve().await.expect("fresh");
    assert_eq!(server.requests(), 1);
}

/// The leader looks at the cache again before it fetches: a fetch that
/// ended between a caller's look and its taking the lead stored a card that
/// is fresh, and fetching again would be the stampede single-flight exists to
/// stop. Called on the leader's own function, as the race cannot be
/// scheduled from outside.
#[tokio::test]
async fn the_leader_looks_again_before_fetching() {
    let server = Server::start(ok(&[("cache-control", "max-age=60")])).await;
    let resolver = CachingCardResolver::new(&server.url).unwrap();
    resolver.resolve().await.expect("first");
    assert_eq!(resolver.refresh().await.expect("fresh").name, "fleet");
    assert_eq!(server.requests(), 1);
}

/// A resolver given new options does not join a fetch made under the old
/// ones: that card answers other request headers (RFC 9111 §4.1).
#[tokio::test]
async fn new_options_do_not_share_the_old_fetch() {
    let server = Server::start(ok(&[("cache-control", "max-age=60")])).await;
    let gate = server.hold();
    let old = CachingCardResolver::new(&server.url).unwrap();
    let first = {
        let old = old.clone();
        tokio::spawn(async move { old.resolve().await })
    };
    while server.requests() == 0 {
        tokio::task::yield_now().await;
    }
    let new = old
        .clone()
        .with_options(super::CardFetchOptions::default().with_header("x-tenant", "b"));
    let second = {
        let new = new.clone();
        tokio::spawn(async move { new.resolve().await })
    };
    // Either it sent its own request, or (the defect) it is waiting on the
    // old fetch.
    while server.requests() < 2 && old.flight.waiters() == 0 {
        tokio::task::yield_now().await;
    }
    assert_eq!(server.requests(), 2, "its own request");
    assert_eq!(old.flight.waiters(), 0);
    gate.add_permits(2);
    first.await.unwrap().expect("old");
    second.await.unwrap().expect("new");
}

// The `CardCache` and pool cases.
mod card_cache;

// What the resolver does not store.
mod stored_cards;

// How a stale-if-error window is counted, and what a `304` leaves, end to end.
mod revalidation;
mod stale_window;
