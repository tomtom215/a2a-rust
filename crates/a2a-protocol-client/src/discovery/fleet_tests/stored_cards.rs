// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! What the resolver stores, for a card with no validator and no lifetime. The
//! counting server of the parent module always sends an `ETag`, so it cannot
//! serve one; this module's server sends exactly the headers it is given.

use std::sync::Arc;
use std::sync::atomic::{AtomicU16, Ordering};

use http_body_util::Full;
use hyper::body::Bytes;

use super::super::CachingCardResolver;
use super::{SECOND, card_json, elapse};
use crate::error::ClientError;

/// A server for the card, answering with `headers` and the status in the
/// returned cell (`200` at first; any other status has no body).
async fn bare_server(headers: &'static [(&'static str, &'static str)]) -> (String, Arc<AtomicU16>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let url = format!("http://{}", listener.local_addr().expect("addr"));
    let status = Arc::new(AtomicU16::new(200));
    let current = Arc::clone(&status);
    tokio::spawn(async move {
        while let Ok((stream, _)) = listener.accept().await {
            let current = Arc::clone(&current);
            tokio::spawn(async move {
                let service = hyper::service::service_fn(move |_req| {
                    let status = current.load(Ordering::SeqCst);
                    async move {
                        let mut response = hyper::Response::builder()
                            .status(status)
                            .header("content-type", "application/json");
                        for (name, value) in headers {
                            response = response.header(*name, *value);
                        }
                        let body = if status == 200 {
                            card_json()
                        } else {
                            String::new()
                        };
                        Ok::<_, std::io::Error>(
                            response
                                .body(Full::new(Bytes::from(body)))
                                .expect("response"),
                        )
                    }
                });
                let _ = hyper::server::conn::http1::Builder::new()
                    .serve_connection(hyper_util::rt::TokioIo::new(stream), service)
                    .await;
            });
        }
    });
    (url, status)
}

/// A card with no `ETag`, no `Last-Modified` and no lifetime is stale as it
/// arrives. When it carries `stale-if-error`, RFC 5861 §4 lets a cache serve
/// it on a `5xx` "regardless of other freshness information", so a `503` is
/// answered with the card, until the window (counted from its arrival) ends.
#[tokio::test]
async fn a_card_with_no_validator_and_no_lifetime_is_served_stale_by_its_own_directive() {
    let (url, status) = bare_server(&[("cache-control", "stale-if-error=30")]).await;
    let resolver = CachingCardResolver::new(&url).unwrap();
    resolver.resolve().await.expect("first fetch");
    status.store(503, Ordering::SeqCst);
    elapse(29 * SECOND).await;
    assert_eq!(
        resolver.resolve().await.expect("served stale").name,
        "fleet"
    );
    elapse(2 * SECOND).await;
    let err = resolver.resolve().await.expect_err("past the window");
    assert!(
        matches!(err, ClientError::UnexpectedStatus { status: 503, .. }),
        "{err}"
    );
}

/// The window the caller configured is "configuration in accordance with an
/// out-of-band contract" (RFC 9111 §4.2.4), and covers a card that says
/// nothing about caching at all.
#[tokio::test]
async fn a_configured_window_covers_a_card_that_says_nothing_about_caching() {
    let (url, status) = bare_server(&[]).await;
    let resolver = CachingCardResolver::new(&url)
        .unwrap()
        .with_stale_if_error(60 * SECOND);
    resolver.resolve().await.expect("first fetch");
    status.store(502, Ordering::SeqCst);
    elapse(SECOND).await;
    assert_eq!(
        resolver.resolve().await.expect("served stale").name,
        "fleet"
    );
}

/// With no window, neither the card's nor the caller's, nothing could use a
/// card that has no validator and no lifetime: it is not kept, and a `503` is
/// the error it is. The stored state is read directly, because the answer a
/// caller sees is the same whether the card was kept or not.
#[tokio::test]
async fn a_card_nothing_could_use_is_not_kept() {
    let (url, status) = bare_server(&[]).await;
    let resolver = CachingCardResolver::new(&url).unwrap();
    resolver.resolve().await.expect("first fetch");
    assert!(
        resolver.cache.read().await.is_none(),
        "no validator, no lifetime, no window: nothing to keep it for"
    );
    status.store(503, Ordering::SeqCst);
    elapse(SECOND).await;
    resolver
        .resolve()
        .await
        .expect_err("no stored card to serve");
}

/// `must-revalidate` and `no-cache` forbid serving the card stale
/// (RFC 9111 §4.2.4), so such a card with no validator is not kept for a
/// window that could never apply, and a `503` is an error whatever the window.
#[tokio::test]
async fn a_card_that_forbids_stale_service_is_not_kept_for_a_window() {
    for forbidding in ["must-revalidate", "no-cache"] {
        let headers: &'static [(&'static str, &'static str)] = match forbidding {
            "must-revalidate" => &[("cache-control", "must-revalidate, stale-if-error=30")],
            _ => &[("cache-control", "no-cache, stale-if-error=30")],
        };
        let (url, status) = bare_server(headers).await;
        let resolver = CachingCardResolver::new(&url)
            .unwrap()
            .with_stale_if_error(60 * SECOND);
        resolver.resolve().await.expect("first fetch");
        assert!(resolver.cache.read().await.is_none(), "{forbidding}");
        status.store(503, Ordering::SeqCst);
        elapse(SECOND).await;
        resolver.resolve().await.expect_err(forbidding);
    }
}

/// The control: the same card with an `ETag` is kept and served stale, so the
/// cases above differ from this one in whether the card could be used.
#[tokio::test]
async fn the_same_card_with_a_validator_is_stored_and_served_stale() {
    let (url, status) =
        bare_server(&[("cache-control", "stale-if-error=30"), ("etag", "\"v1\"")]).await;
    let resolver = CachingCardResolver::new(&url).unwrap();
    resolver.resolve().await.expect("first fetch");
    status.store(503, Ordering::SeqCst);
    elapse(SECOND).await;
    assert_eq!(
        resolver.resolve().await.expect("served stale").name,
        "fleet"
    );
}
