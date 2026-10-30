// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Tests for [`Conn`]'s writes and [`ProxyConnector`]'s warning, from the
//! incremental mutation run on pull request #184.

use std::future::poll_fn;
use std::task::{Context, Poll, Waker};

use tokio::io::AsyncReadExt as _;
use tower_service::Service as _;

use super::*;

fn uri(s: &str) -> http::Uri {
    s.parse().unwrap()
}

/// A connection shut down is closed to its peer, which reads its end, while
/// the connection itself is still held.
#[tokio::test]
async fn a_connection_shut_down_is_closed_to_its_peer() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let tcp = TcpStream::connect(listener.local_addr().unwrap())
        .await
        .unwrap();
    let (mut peer, _) = listener.accept().await.unwrap();
    let mut conn = Conn::Forwarded {
        inner: TokioIo::new(tcp),
    };
    poll_fn(|cx| Pin::new(&mut conn).poll_shutdown(cx))
        .await
        .unwrap();
    let mut byte = [0u8; 1];
    let read = tokio::time::timeout(std::time::Duration::from_secs(2), peer.read(&mut byte))
        .await
        .expect("the peer sees the close")
        .unwrap();
    assert_eq!(read, 0, "end of stream");
    drop(conn);
}

/// What rustls has taken but not yet sent is sent by a flush: a TLS write
/// that met a full socket returns with records still buffered, and only
/// `poll_flush` writes them. The writes run with a waker that never wakes,
/// until the socket is full and nothing more is taken; then the peer reads,
/// and must receive every byte that was taken.
#[tokio::test]
async fn a_flush_sends_what_tls_still_holds() {
    let key = rcgen::KeyPair::generate().unwrap();
    let cert = rcgen::CertificateParams::new(vec!["localhost".into()])
        .unwrap()
        .self_signed(&key)
        .unwrap();
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let server = rustls::ServerConfig::builder_with_provider(Arc::clone(&provider))
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(
            vec![cert.der().clone()],
            rustls_pki_types::PrivatePkcs8KeyDer::from(key.serialize_der()).into(),
        )
        .unwrap();
    let mut roots = rustls::RootCertStore::empty();
    roots.add(cert.der().clone()).unwrap();
    let client = rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_root_certificates(roots)
        .with_no_client_auth();

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (start, started) = tokio::sync::oneshot::channel::<usize>();
    let reader = tokio::spawn(async move {
        let (tcp, _) = listener.accept().await.unwrap();
        let mut tls = tokio_rustls::TlsAcceptor::from(Arc::new(server))
            .accept(tcp)
            .await
            .unwrap();
        // Nothing is read until the writer has filled the socket.
        let expected = started.await.unwrap();
        let mut got = vec![0u8; expected];
        tokio::time::timeout(std::time::Duration::from_secs(5), tls.read_exact(&mut got))
            .await
            .map(|r| r.map(|_| expected))
    });

    let tcp = TcpStream::connect(addr).await.unwrap();
    let tls = tokio_rustls::TlsConnector::from(Arc::new(client))
        .connect(
            rustls_pki_types::ServerName::try_from("localhost").unwrap(),
            TokioIo::new(TokioIo::new(tcp)),
        )
        .await
        .unwrap();
    let mut conn = Conn::Stream {
        inner: hyper_rustls::MaybeHttpsStream::Https(TokioIo::new(tls)),
    };

    let chunk = vec![7u8; 64 * 1024];
    let mut taken = 0usize;
    let mut cx = Context::from_waker(Waker::noop());
    while let Poll::Ready(n) = Pin::new(&mut conn).poll_write(&mut cx, &chunk) {
        taken += n.unwrap();
        assert!(taken < 1 << 30, "the socket never filled");
    }
    start.send(taken).unwrap();
    poll_fn(|cx| Pin::new(&mut conn).poll_flush(cx))
        .await
        .unwrap();
    let received = reader.await.unwrap();
    assert!(
        matches!(received, Ok(Ok(n)) if n == taken),
        "{taken} bytes taken; the peer read {received:?}"
    );
}

/// A proxy of a kind not supported is warned about once per connector, not
/// once per connection; the connector's `Debug` says what it is.
#[cfg(feature = "tracing")]
#[test]
fn an_unsupported_proxy_is_warned_about_once() {
    let rules = Arc::new(matcher_from(&|name: &str| {
        (name == "HTTPS_PROXY").then(|| "socks5://proxy:1080".to_owned())
    }));
    let mut connector = ProxyConnector::new(
        crate::tls::default_tls_config(),
        rules,
        HttpConnector::new(),
    );
    assert!(format!("{connector:?}").starts_with("ProxyConnector"));
    let first =
        crate::test_tracing::warnings_in(|| drop(connector.call(uri("https://agent.example/"))));
    let second =
        crate::test_tracing::warnings_in(|| drop(connector.call(uri("https://agent.example/"))));
    assert_eq!((first, second), (1, 0));
}
