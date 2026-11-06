// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! What the resolver keeps after a `304` (RFC 9111 §4.3.4), end to end: a
//! server that answers a conditional request with a `304` carrying exactly the
//! header fields it is given. The merge itself, including `Date` and `Age`, is
//! tested field by field in `freshness/after_304_tests.rs`.

use std::sync::Arc;
use std::sync::atomic::{AtomicU16, AtomicUsize, Ordering};

use http_body_util::Full;
use hyper::body::Bytes;

use super::super::CachingCardResolver;
use super::{SECOND, card_json, elapse};
use crate::error::ClientError;

type Fields = &'static [(&'static str, &'static str)];

/// A server for the card: `200` with `first` and an `ETag`; to a request that
/// carries `If-None-Match`, `304` with `not_modified` and the same `ETag`; and
/// whatever status is stored in the returned cell when it is not `200`.
struct Validating {
    url: String,
    requests: Arc<AtomicUsize>,
    status: Arc<AtomicU16>,
}

impl Validating {
    async fn start(first: Fields, not_modified: Fields) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let url = format!("http://{}", listener.local_addr().expect("addr"));
        let requests = Arc::new(AtomicUsize::new(0));
        let status = Arc::new(AtomicU16::new(200));
        let (seen, current) = (Arc::clone(&requests), Arc::clone(&status));
        tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                let (seen, current) = (Arc::clone(&seen), Arc::clone(&current));
                tokio::spawn(async move {
                    let service = hyper::service::service_fn(move |req: hyper::Request<_>| {
                        seen.fetch_add(1, Ordering::SeqCst);
                        let status = current.load(Ordering::SeqCst);
                        let conditional = req.headers().contains_key("if-none-match");
                        async move {
                            let (code, fields, body) = match (status, conditional) {
                                (200, true) => (304, not_modified, String::new()),
                                (200, false) => (200, first, card_json()),
                                (other, _) => (other, &[][..], String::new()),
                            };
                            let mut response = hyper::Response::builder().status(code);
                            if code == 200 || code == 304 {
                                response = response.header("etag", "\"v1\"");
                            }
                            for (name, value) in fields {
                                response = response.header(*name, *value);
                            }
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
        Self {
            url,
            requests,
            status,
        }
    }

    fn requests(&self) -> usize {
        self.requests.load(Ordering::SeqCst)
    }

    fn fail_with(&self, status: u16) {
        self.status.store(status, Ordering::SeqCst);
    }
}

/// A `304` that leaves `Cache-Control` out, as the stored response's lifetime
/// is not the `304`'s to forget: the card is fresh again for the stored `60 s`,
/// counted from the `304`. Before 2026-11-06 it was revalidated on every use.
#[tokio::test]
async fn a_304_without_cache_control_keeps_the_card_fresh_for_its_stored_lifetime() {
    let server = Validating::start(&[("cache-control", "max-age=60")], &[]).await;
    let resolver = CachingCardResolver::new(&server.url).unwrap();
    resolver.resolve().await.expect("first fetch");
    elapse(61 * SECOND).await;
    resolver.resolve().await.expect("revalidated by a 304");
    assert_eq!(server.requests(), 2);
    elapse(30 * SECOND).await;
    assert_eq!(resolver.resolve().await.expect("fresh").name, "fleet");
    assert_eq!(server.requests(), 2, "fresh from the 304 on: no request");
    elapse(31 * SECOND).await;
    resolver.resolve().await.expect("stale again, revalidated");
    assert_eq!(server.requests(), 3, "and stale once the 60 s are used");
}

/// A `Cache-Control` on the `304` replaces the stored one as a field: its
/// `max-age=10` is the new lifetime, and the stored `stale-if-error=300` is
/// gone with the rest of the stored field, so a `503` is an error.
#[tokio::test]
async fn a_304s_cache_control_replaces_the_stored_field_whole() {
    let server = Validating::start(
        &[("cache-control", "max-age=60, stale-if-error=300")],
        &[("cache-control", "max-age=10")],
    )
    .await;
    let resolver = CachingCardResolver::new(&server.url).unwrap();
    resolver.resolve().await.expect("first fetch");
    elapse(61 * SECOND).await;
    resolver.resolve().await.expect("revalidated by a 304");
    assert_eq!(server.requests(), 2);
    elapse(5 * SECOND).await;
    resolver.resolve().await.expect("fresh for the 304's 10 s");
    assert_eq!(server.requests(), 2);
    elapse(6 * SECOND).await;
    server.fail_with(503);
    let err = resolver.resolve().await.expect_err("no stale window left");
    assert!(
        matches!(err, ClientError::UnexpectedStatus { status: 503, .. }),
        "{err}"
    );
}

/// The control for the case above: a `304` that carries no `Cache-Control`
/// leaves the stored `stale-if-error` in force, so the same `503` is answered
/// with the card.
#[tokio::test]
async fn a_304_without_cache_control_keeps_the_stored_stale_window() {
    let server =
        Validating::start(&[("cache-control", "max-age=60, stale-if-error=300")], &[]).await;
    let resolver = CachingCardResolver::new(&server.url).unwrap();
    resolver.resolve().await.expect("first fetch");
    elapse(61 * SECOND).await;
    resolver.resolve().await.expect("revalidated by a 304");
    elapse(61 * SECOND).await;
    server.fail_with(503);
    assert_eq!(
        resolver.resolve().await.expect("served stale").name,
        "fleet"
    );
}
