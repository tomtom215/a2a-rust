// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The delivery connects to the addresses its host was checked to resolve to,
//! over `https://` as well as `http://` (audit D-13, SEC-13, B-13).
//!
//! `deliver` takes the validated addresses as a parameter, so these tests can
//! use a name that resolves nowhere (`*.test`) and a receiver on loopback: if
//! the connection reaches the receiver, it was the pin that put it there, and
//! it was not DNS. The proxy rules are empty so a proxy in the environment
//! cannot take the delivery.

#![cfg(feature = "tls-rustls")]

use std::net::SocketAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use a2a_protocol_types::events::{StreamResponse, TaskStatusUpdateEvent};
use a2a_protocol_types::push::TaskPushNotificationConfig;
use a2a_protocol_types::task::{ContextId, TaskId, TaskState, TaskStatus};
use bytes::Bytes;
use http_body_util::Full;
use hyper::service::service_fn;
use hyper::{Request, Response};
use hyper_util::rt::{TokioExecutor, TokioIo};
use tokio_rustls::TlsAcceptor;

use super::*;
use crate::net::client::HttpsClient;
use crate::net::proxy::{self, ProxySetting};

struct Pki {
    ca: rustls::pki_types::CertificateDer<'static>,
}

/// A certificate authority and a server certificate for `name` signed by it.
fn pki(name: &str) -> (Pki, Arc<rustls::ServerConfig>) {
    let mut ca_params = rcgen::CertificateParams::new(vec![]).unwrap();
    ca_params.is_ca = rcgen::IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
    let ca_key = rcgen::KeyPair::generate().unwrap();
    let ca_cert = ca_params.self_signed(&ca_key).unwrap();
    let issuer = rcgen::Issuer::new(ca_params, ca_key);
    let key = rcgen::KeyPair::generate().unwrap();
    let cert = rcgen::CertificateParams::new(vec![name.to_owned()])
        .unwrap()
        .signed_by(&key, &issuer)
        .unwrap();
    let server = rustls::ServerConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .unwrap()
    .with_no_client_auth()
    .with_single_cert(
        vec![cert.der().clone()],
        rustls::pki_types::PrivateKeyDer::Pkcs8(rustls::pki_types::PrivatePkcs8KeyDer::from(
            key.serialize_der(),
        )),
    )
    .unwrap();
    (
        Pki {
            ca: ca_cert.der().clone(),
        },
        Arc::new(server),
    )
}

/// A sender that trusts `pki`'s authority and has no proxy, delivering once
/// per attempt without waiting between attempts.
fn sender(pki: &Pki, attempts: usize) -> HttpPushSender {
    let mut roots = rustls::RootCertStore::empty();
    roots.add(pki.ca.clone()).unwrap();
    let tls = rustls::ClientConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .unwrap()
    .with_root_certificates(roots)
    .with_no_client_auth();
    let rules = Arc::new(proxy::matcher(&|_| None, &ProxySetting::Environment));
    HttpPushSender {
        client: HttpsClient::with_rules(&tls, rules),
        request_timeout: Duration::from_secs(10),
        retry_policy: PushRetryPolicy::default()
            .with_max_attempts(attempts)
            .with_backoff(vec![Duration::from_millis(10)]),
        ..HttpPushSender::new()
    }
}

/// What a receiver saw: the number of posts and the `Host` of the last.
#[derive(Default)]
struct Seen {
    posts: AtomicUsize,
    host: Mutex<Option<String>>,
}

fn handler(
    seen: Arc<Seen>,
) -> impl hyper::service::Service<
    Request<hyper::body::Incoming>,
    Response = Response<Full<Bytes>>,
    Error = std::convert::Infallible,
    Future = impl std::future::Future<Output = Result<Response<Full<Bytes>>, std::convert::Infallible>>
             + Send,
> + Clone {
    service_fn(move |req: Request<hyper::body::Incoming>| {
        let seen = Arc::clone(&seen);
        async move {
            use http_body_util::BodyExt;
            *seen.host.lock().unwrap() = req
                .headers()
                .get("host")
                .and_then(|h| h.to_str().ok())
                .map(str::to_owned);
            let _ = req.into_body().collect().await;
            seen.posts.fetch_add(1, Ordering::SeqCst);
            Ok(Response::new(Full::new(Bytes::from_static(b"ok"))))
        }
    })
}

/// A TLS receiver on loopback. Returns its port.
async fn tls_receiver(config: Arc<rustls::ServerConfig>, seen: Arc<Seen>) -> u16 {
    let acceptor = TlsAcceptor::from(config);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        while let Ok((stream, _)) = listener.accept().await {
            let (acceptor, seen) = (acceptor.clone(), Arc::clone(&seen));
            tokio::spawn(async move {
                let Ok(tls) = acceptor.accept(stream).await else {
                    return;
                };
                let _ = hyper_util::server::conn::auto::Builder::new(TokioExecutor::new())
                    .serve_connection(TokioIo::new(tls), handler(seen))
                    .await;
            });
        }
    });
    port
}

/// A plaintext receiver on loopback. Returns its port.
async fn plain_receiver(seen: Arc<Seen>) -> u16 {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        while let Ok((stream, _)) = listener.accept().await {
            let seen = Arc::clone(&seen);
            tokio::spawn(async move {
                let _ = hyper_util::server::conn::auto::Builder::new(TokioExecutor::new())
                    .serve_connection(TokioIo::new(stream), handler(seen))
                    .await;
            });
        }
    });
    port
}

fn event() -> StreamResponse {
    StreamResponse::StatusUpdate(TaskStatusUpdateEvent::new(
        TaskId::new("t"),
        ContextId::new("c"),
        TaskStatus::with_timestamp(TaskState::Completed),
    ))
}

fn config(url: &str) -> TaskPushNotificationConfig {
    TaskPushNotificationConfig::new("t".to_owned(), url.to_owned()).with_id("cfg".to_owned())
}

fn loopback(port: u16) -> SocketAddr {
    SocketAddr::from(([127, 0, 0, 1], port))
}

/// An address on this machine that nothing listens on and that fails fast on
/// every platform: `::1` is refused when IPv6 is up (the receivers listen on
/// `127.0.0.1` only) and fails at once when it is not.
///
/// This was `127.0.0.2`, which Linux treats as loopback and refuses, but macOS
/// does not configure it, so a connect to it waited out the request timeout
/// and the macOS job of the pull request failed with "timed out after 2s". The
/// request timeout is 10 s here because Windows can take about 2 s to refuse a
/// closed loopback port.
fn down_address(port: u16) -> SocketAddr {
    SocketAddr::from((std::net::Ipv6Addr::LOCALHOST, port))
}

/// The fix, end to end. The name resolves nowhere, so the connection can only
/// have gone where the pin said, and the handshake succeeded, so the
/// certificate was checked against the name in the URL.
#[tokio::test]
async fn an_https_delivery_connects_to_the_pinned_address_and_checks_the_name() {
    let (pki, server) = pki("pinned.test");
    let seen = Arc::new(Seen::default());
    let port = tls_receiver(server, Arc::clone(&seen)).await;
    let sender = sender(&pki, 1);
    let url = format!("https://pinned.test:{port}/hook");

    sender
        .deliver(&url, &event(), &config(&url), Some(vec![loopback(port)]))
        .await
        .expect("delivered to the pinned address");
    assert_eq!(seen.posts.load(Ordering::SeqCst), 1);
}

/// The control: with no pin the same URL goes to the system resolver, which
/// does not know the name. Without this the test above could be passing
/// because something else resolves it.
#[tokio::test]
async fn without_a_pin_the_same_name_goes_to_the_system_resolver() {
    let (pki, server) = pki("pinned.test");
    let seen = Arc::new(Seen::default());
    let port = tls_receiver(server, Arc::clone(&seen)).await;
    let sender = sender(&pki, 1);
    let url = format!("https://pinned.test:{port}/hook");

    sender
        .deliver(&url, &event(), &config(&url), None)
        .await
        .expect_err("the name resolves nowhere");
    assert_eq!(seen.posts.load(Ordering::SeqCst), 0);
}

/// Pinning the address does not excuse the certificate: a server at the pinned
/// address that cannot prove it is the named host gets nothing.
#[tokio::test]
async fn a_pin_does_not_excuse_a_certificate_for_another_name() {
    let (pki, server) = pki("someone-else.test");
    let seen = Arc::new(Seen::default());
    let port = tls_receiver(server, Arc::clone(&seen)).await;
    let sender = sender(&pki, 1);
    let url = format!("https://pinned.test:{port}/hook");

    sender
        .deliver(&url, &event(), &config(&url), Some(vec![loopback(port)]))
        .await
        .expect_err("the certificate is for another name");
    assert_eq!(seen.posts.load(Ordering::SeqCst), 0);
}

/// The pin lasts for the delivery and no longer: a host that is pinned
/// forever would keep a stale address after its owner moved it.
#[tokio::test]
async fn the_pin_ends_with_the_delivery() {
    let (pki, server) = pki("pinned.test");
    let port = tls_receiver(server, Arc::default()).await;
    let sender = sender(&pki, 1);
    let url = format!("https://pinned.test:{port}/hook");

    sender
        .deliver(&url, &event(), &config(&url), Some(vec![loopback(port)]))
        .await
        .expect("delivered");
    assert!(!sender.client.is_pinned("pinned.test"));
}

/// A host with an address that is down and one that is up: the connector goes
/// on to the next address of the pin.
#[tokio::test]
async fn an_https_delivery_goes_on_to_the_next_validated_address() {
    let (pki, server) = pki("pinned.test");
    let seen = Arc::new(Seen::default());
    let port = tls_receiver(server, Arc::clone(&seen)).await;
    let sender = sender(&pki, 1);
    let url = format!("https://pinned.test:{port}/hook");
    let down = down_address(port);

    sender
        .deliver(
            &url,
            &event(),
            &config(&url),
            Some(vec![down, loopback(port)]),
        )
        .await
        .expect("the second address answered");
    assert_eq!(seen.posts.load(Ordering::SeqCst), 1);
}

/// `http://` is rewritten to one literal address per attempt: the first
/// attempt dials the address that is down, the second the next, and the
/// receiver is told the host the URL named.
#[tokio::test]
async fn an_http_delivery_tries_the_next_validated_address_on_each_attempt() {
    let (pki, _) = pki("unused.test");
    let seen = Arc::new(Seen::default());
    let port = plain_receiver(Arc::clone(&seen)).await;
    let sender = sender(&pki, 2);
    let url = format!("http://down.test:{port}/hook");
    let down = down_address(port);

    sender
        .deliver(
            &url,
            &event(),
            &config(&url),
            Some(vec![down, loopback(port)]),
        )
        .await
        .expect("the second attempt reached the second address");
    assert_eq!(seen.posts.load(Ordering::SeqCst), 1);
    assert_eq!(
        seen.host.lock().unwrap().as_deref(),
        Some(format!("down.test:{port}").as_str()),
        "the receiver sees the host the URL named, not the address"
    );
}
