// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The shared outbound client. Its behaviour (a proxy tunnelled, a private CA
//! trusted, the proxy's login sent) is tested end to end in
//! `tests/outbound_proxy_and_roots.rs`; this is what a unit test can see.

#![cfg(feature = "tls-rustls")]

use super::*;

/// `HttpsClient` sits inside `HttpPushSender`, which derives `Debug` and is
/// printed in logs: the client says what it is and nothing of its pool or its
/// proxy rules (which may carry a proxy login).
#[test]
fn debug_names_the_type_and_nothing_else() {
    let client = HttpsClient::new(&crate::net::roots::default_tls_config());
    assert_eq!(format!("{client:?}"), "HttpsClient { .. }");
}

/// The proxy rules `env` and `setting` give.
fn rules(
    env: &(dyn Fn(&str) -> Option<String> + Send + Sync + 'static),
    setting: &ProxySetting,
) -> Arc<Matcher> {
    Arc::new(proxy::matcher(env, setting))
}

/// Whether a request is proxied is what the client's rules say of its
/// destination: a proxy named for `https://` destinations is none for an
/// `http://` one, and a client given no proxy has none for either.
#[test]
fn a_request_is_proxied_when_the_rules_name_a_proxy_for_it() {
    let tls = crate::net::roots::default_tls_config();
    let http: hyper::Uri = "http://a.example/".parse().unwrap();
    let https: hyper::Uri = "https://a.example/".parse().unwrap();
    let via_https_proxy = HttpsClient::with_rules(
        &tls,
        rules(
            &|k| (k == "HTTPS_PROXY").then(|| "http://proxy.example:3128".to_owned()),
            &ProxySetting::Environment,
        ),
    );
    assert!(via_https_proxy.proxied(&https));
    assert!(
        !via_https_proxy.proxied(&http),
        "no proxy is named for http"
    );
    let direct = HttpsClient::with_rules(&tls, rules(&|_| None, &ProxySetting::Off));
    assert!(!direct.proxied(&http));
    assert!(!direct.proxied(&https));
}

/// The client's connections get the TCP settings every outbound client has
/// (audit B-30): a connection the connector opens has `TCP_NODELAY` and
/// `SO_KEEPALIVE`, which an untuned connector does not set.
#[tokio::test]
async fn the_connector_the_client_dials_with_is_tuned() {
    use hyper_util::rt::TokioIo;
    use tower_service::Service;

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let uri: hyper::Uri = format!("http://{}/", listener.local_addr().unwrap())
        .parse()
        .unwrap();
    let accept = tokio::spawn(async move { listener.accept().await });
    let mut tcp = tcp_connector(PinResolver::default());
    let io: TokioIo<tokio::net::TcpStream> = tcp.call(uri).await.expect("connects");
    let _ = accept.await;
    let stream = io.into_inner();
    assert!(stream.nodelay().unwrap());
    let socket = tokio::net::TcpSocket::from_std_stream(stream.into_std().unwrap());
    assert!(socket.keepalive().unwrap());
}
