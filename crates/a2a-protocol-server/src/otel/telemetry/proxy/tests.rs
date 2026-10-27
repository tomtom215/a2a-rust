// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Which destinations go through which proxy, with the environment injected.
//! The wire behaviour (a forward request, a `CONNECT` tunnel) is tested
//! against a real forward proxy in `tests/telemetry_export/proxy.rs`.

use std::collections::HashMap;

use super::*;

fn env(pairs: &[(&str, &str)]) -> Box<Env> {
    let map: HashMap<String, String> = pairs
        .iter()
        .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
        .collect();
    Box::new(move |k| map.get(k).cloned().filter(|v| !v.trim().is_empty()))
}

fn uri(s: &str) -> http::Uri {
    s.parse().expect("a URI")
}

/// The proxy URI `setting` and `pairs` choose for `dst`, as text.
fn via(pairs: &[(&str, &str)], setting: &ProxySetting, dst: &str) -> Option<String> {
    let rules = matcher(&*env(pairs), setting);
    intercept(&rules, &uri(dst)).map(|i| i.uri().to_string())
}

#[test]
fn nothing_is_proxied_when_nothing_is_set() {
    assert_eq!(
        via(&[], &ProxySetting::Environment, "http://collector:4318"),
        None
    );
}

#[test]
fn the_scheme_of_the_destination_picks_the_variable() {
    let e = [
        ("HTTP_PROXY", "http://plain.proxy:3128"),
        ("HTTPS_PROXY", "http://secure.proxy:3129"),
    ];
    assert_eq!(
        via(&e, &ProxySetting::Environment, "http://collector:4318").as_deref(),
        Some("http://plain.proxy:3128/")
    );
    assert_eq!(
        via(&e, &ProxySetting::Environment, "https://collector:4318").as_deref(),
        Some("http://secure.proxy:3129/")
    );
}

#[test]
fn all_proxy_covers_both_and_the_specific_variable_wins() {
    let e = [
        ("ALL_PROXY", "http://all.proxy:1"),
        ("HTTPS_PROXY", "http://secure.proxy:2"),
    ];
    assert_eq!(
        via(&e, &ProxySetting::Environment, "http://c:1").as_deref(),
        Some("http://all.proxy:1/")
    );
    assert_eq!(
        via(&e, &ProxySetting::Environment, "https://c:1").as_deref(),
        Some("http://secure.proxy:2/")
    );
}

#[test]
fn lowercase_variables_are_read_and_uppercase_wins_when_both_are_set() {
    assert_eq!(
        via(
            &[("https_proxy", "http://lower:1")],
            &ProxySetting::Environment,
            "https://c:1"
        )
        .as_deref(),
        Some("http://lower:1/")
    );
    assert_eq!(
        via(
            &[
                ("HTTPS_PROXY", "http://upper:1"),
                ("https_proxy", "http://lower:1")
            ],
            &ProxySetting::Environment,
            "https://c:1"
        )
        .as_deref(),
        Some("http://upper:1/")
    );
}

#[test]
fn no_proxy_exempts_hosts_suffixes_and_ranges() {
    let e = [
        ("HTTP_PROXY", "http://proxy:3128"),
        ("NO_PROXY", "localhost, .internal.example,10.0.0.0/8"),
    ];
    for exempt in [
        "http://localhost:4318",
        "http://collector.internal.example:4318",
        "http://10.1.2.3:4318",
    ] {
        assert_eq!(
            via(&e, &ProxySetting::Environment, exempt),
            None,
            "{exempt}"
        );
    }
    for proxied in ["http://collector.example:4318", "http://11.1.2.3:4318"] {
        assert!(
            via(&e, &ProxySetting::Environment, proxied).is_some(),
            "{proxied}"
        );
    }
}

#[test]
fn star_in_no_proxy_exempts_everything() {
    let e = [("HTTP_PROXY", "http://proxy:3128"), ("no_proxy", "*")];
    assert_eq!(via(&e, &ProxySetting::Environment, "http://c:1"), None);
}

#[test]
fn off_ignores_the_environment_and_a_url_in_code_ignores_it_entirely() {
    let e = [
        ("HTTP_PROXY", "http://env.proxy:1"),
        ("NO_PROXY", "exempt.example"),
    ];
    assert_eq!(via(&e, &ProxySetting::Off, "http://c:1"), None);
    let code = ProxySetting::Url("http://code.proxy:9".to_owned());
    assert_eq!(
        via(&e, &code, "https://c:1").as_deref(),
        Some("http://code.proxy:9/")
    );
    // `NO_PROXY` belongs to the proxies the environment names, not to one
    // chosen in code.
    assert_eq!(
        via(&e, &code, "http://exempt.example:1").as_deref(),
        Some("http://code.proxy:9/")
    );
}

#[test]
fn a_cgi_process_uses_no_proxy_whatever_the_variables_say() {
    let e = [
        ("REQUEST_METHOD", "GET"),
        ("HTTP_PROXY", "http://attacker.example:1"),
    ];
    assert_eq!(via(&e, &ProxySetting::Environment, "http://c:1"), None);
    assert_eq!(
        via(
            &e,
            &ProxySetting::Url("http://p:1".to_owned()),
            "http://c:1"
        ),
        None
    );
}

#[test]
fn only_http_proxies_are_used_and_the_others_are_named() {
    for (proxy, scheme) in [("socks5://p:1080", "socks5"), ("https://p:443", "https")] {
        let rules = matcher(&*env(&[("ALL_PROXY", proxy)]), &ProxySetting::Environment);
        let dst = uri("http://c:4318");
        assert!(intercept(&rules, &dst).is_none(), "{proxy}");
        assert_eq!(unsupported(&rules, &dst).as_deref(), Some(scheme));
    }
    let rules = matcher(
        &*env(&[("ALL_PROXY", "http://p:1")]),
        &ProxySetting::Environment,
    );
    assert_eq!(unsupported(&rules, &uri("http://c:4318")), None);
}

#[test]
fn userinfo_in_the_proxy_url_becomes_basic_proxy_authorization() {
    let rules = matcher(
        &*env(&[("ALL_PROXY", "http://user:p%40ss@p:3128")]),
        &ProxySetting::Environment,
    );
    let found = intercept(&rules, &uri("http://c:1")).expect("proxied");
    assert_eq!(found.uri().to_string(), "http://p:3128/");
    // base64("user:p@ss"): the password was percent-decoded first.
    assert_eq!(
        found.basic_auth().expect("auth").to_str().expect("ascii"),
        "Basic dXNlcjpwQHNz"
    );
}

/// What `tracing` recorded at `WARN`.
#[derive(Clone, Default)]
struct Warnings(std::sync::Arc<std::sync::Mutex<Vec<String>>>);

impl<S: tracing::Subscriber> tracing_subscriber::Layer<S> for Warnings {
    fn on_event(&self, event: &tracing::Event<'_>, _: tracing_subscriber::layer::Context<'_, S>) {
        struct MessageText(String);
        impl tracing::field::Visit for MessageText {
            fn record_debug(&mut self, f: &tracing::field::Field, v: &dyn std::fmt::Debug) {
                if f.name() == "message" {
                    self.0 = format!("{v:?}");
                }
            }
        }
        if *event.metadata().level() == tracing::Level::WARN {
            let mut message = MessageText(String::new());
            event.record(&mut message);
            self.0.lock().expect("lock").push(message.0);
        }
    }
}

/// A proxy of a kind the export cannot use is named when the first connection
/// is made, and not again: the warning is for the person who configured it,
/// not one line per batch.
#[test]
fn an_unsupported_proxy_is_named_on_the_first_connection_and_only_then() {
    use tower_service::Service as _;
    use tracing_subscriber::layer::SubscriberExt as _;

    let warnings = Warnings::default();
    let _guard =
        tracing::subscriber::set_default(tracing_subscriber::registry().with(warnings.clone()));
    let tls = rustls::ClientConfig::builder_with_provider(std::sync::Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .expect("protocol versions")
    .with_root_certificates(rustls::RootCertStore::empty())
    .with_no_client_auth();
    let rules = std::sync::Arc::new(matcher(
        &*env(&[("ALL_PROXY", "https://p:443")]),
        &ProxySetting::Environment,
    ));
    let mut connector = ProxyConnector::new(&tls, rules);

    // The warning is said by `call` itself; the future is never polled, so
    // nothing connects.
    drop(connector.call(uri("http://c:4318")));
    let said = warnings.0.lock().expect("lock").clone();
    assert_eq!(said.len(), 1, "{said:?}");
    assert!(
        said[0].contains("http://c:4318") && said[0].contains("`https://`"),
        "{said:?}"
    );

    drop(connector.call(uri("http://c:4318")));
    let said = warnings.0.lock().expect("lock").clone();
    assert_eq!(said.len(), 1, "said again: {said:?}");
}

/// A server config presenting a `localhost` certificate from a throwaway CA,
/// and a client config that trusts that CA.
fn tls_pair() -> (rustls::ServerConfig, rustls::ClientConfig) {
    let mut ca_params = rcgen::CertificateParams::new(vec![]).expect("ca params");
    ca_params.is_ca = rcgen::IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
    let ca_key = rcgen::KeyPair::generate().expect("ca key");
    let ca_cert = ca_params.self_signed(&ca_key).expect("ca cert");
    let issuer = rcgen::Issuer::new(ca_params, ca_key);
    let server_key = rcgen::KeyPair::generate().expect("server key");
    let server_cert = rcgen::CertificateParams::new(vec!["localhost".into()])
        .expect("server params")
        .signed_by(&server_key, &issuer)
        .expect("server cert");

    let provider = std::sync::Arc::new(rustls::crypto::ring::default_provider());
    let server = rustls::ServerConfig::builder_with_provider(std::sync::Arc::clone(&provider))
        .with_safe_default_protocol_versions()
        .expect("protocol versions")
        .with_no_client_auth()
        .with_single_cert(
            vec![server_cert.der().clone()],
            rustls_pki_types::PrivateKeyDer::try_from(server_key.serialize_der()).expect("key"),
        )
        .expect("server config");
    let mut roots = rustls::RootCertStore::empty();
    roots.add(ca_cert.der().clone()).expect("root");
    let client = rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .expect("protocol versions")
        .with_root_certificates(roots)
        .with_no_client_auth();
    (server, client)
}

/// `poll_flush` must not say "done" while bytes the socket has not taken are
/// still inside the TLS layer. The peer completes the handshake and then never
/// reads, so the socket fills, `poll_write` reports `Pending`, and some
/// ciphertext is still unsent; a flush that answers `Ready` at once would let
/// a caller go on believing a request had left the process.
#[tokio::test]
async fn flush_waits_for_the_tls_bytes_the_socket_has_not_taken() {
    use hyper::rt::Write as _;
    use std::task::{Context, Poll, Waker};
    use tower_service::Service as _;

    let (server_tls, client_tls) = tls_pair();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let port = listener.local_addr().expect("address").port();
    let acceptor = tokio_rustls::TlsAcceptor::from(std::sync::Arc::new(server_tls));
    let server = tokio::spawn(async move {
        let (tcp, _) = listener.accept().await.expect("accept");
        let _held = acceptor.accept(tcp).await.expect("handshake");
        std::future::pending::<()>().await;
    });

    let rules = std::sync::Arc::new(matcher(&*env(&[]), &ProxySetting::Environment));
    let mut connector = ProxyConnector::new(&client_tls, rules);
    let conn = connector
        .call(uri(&format!("https://localhost:{port}")))
        .await
        .expect("connect");
    let mut conn = std::pin::pin!(conn);
    let mut cx = Context::from_waker(Waker::noop());

    let chunk = vec![0_u8; 64 * 1024];
    let mut full = false;
    for _ in 0..4_096 {
        match conn.as_mut().poll_write(&mut cx, &chunk) {
            Poll::Ready(written) => {
                written.expect("write");
            }
            Poll::Pending => {
                full = true;
                break;
            }
        }
    }
    assert!(full, "the socket never filled");
    assert!(
        conn.as_mut().poll_flush(&mut cx).is_pending(),
        "flush said done with TLS bytes still unsent"
    );
    server.abort();
}

/// One request and its reply across a `Conn` to a real socket, to the
/// collector (`proxied` false) or to a forward proxy (`proxied` true).
///
/// The peer reads the request to its end before it answers, and reaches the
/// end only when the client shuts its write half. So the reply arrives only if
/// `poll_write` put the bytes on the socket and said how many, `poll_shutdown`
/// closed the write half, and `poll_read` handed back what the peer sent.
async fn request_and_reply(proxied: bool) {
    use hyper::rt::{Read as _, ReadBuf, Write as _};
    use std::future::poll_fn;
    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
    use tower_service::Service as _;

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let port = listener.local_addr().expect("address").port();
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.expect("accept");
        let mut request = Vec::new();
        socket
            .read_to_end(&mut request)
            .await
            .expect("read the request to its end");
        socket.write_all(b"pong").await.expect("reply");
        request
    });

    let proxy = format!("http://127.0.0.1:{port}");
    let pairs = if proxied {
        vec![("HTTP_PROXY", proxy.as_str())]
    } else {
        vec![]
    };
    let destination = if proxied {
        "http://collector.invalid:4318".to_owned()
    } else {
        proxy.clone()
    };
    let rules = std::sync::Arc::new(matcher(&*env(&pairs), &ProxySetting::Environment));
    let mut connector = ProxyConnector::new(&tls_pair().1, rules);
    let conn = connector.call(uri(&destination)).await.expect("connect");
    assert_eq!(matches!(conn, Conn::Forwarded { .. }), proxied);
    let mut conn = std::pin::pin!(conn);

    let sent = poll_fn(|cx| conn.as_mut().poll_write(cx, b"ping"))
        .await
        .expect("write");
    assert_eq!(sent, 4, "poll_write must report the bytes it took");
    poll_fn(|cx| conn.as_mut().poll_shutdown(cx))
        .await
        .expect("shutdown");
    let mut reply = Vec::new();
    while reply.len() < 4 {
        let mut chunk = [0_u8; 4];
        let mut buf = ReadBuf::new(&mut chunk);
        poll_fn(|cx| conn.as_mut().poll_read(cx, buf.unfilled()))
            .await
            .expect("read");
        if buf.filled().is_empty() {
            break;
        }
        reply.extend_from_slice(buf.filled());
    }
    assert_eq!(reply, b"pong");
    assert_eq!(server.await.expect("server"), b"ping");
}

#[tokio::test]
async fn a_direct_connection_carries_a_request_and_its_reply() {
    tokio::time::timeout(std::time::Duration::from_secs(10), request_and_reply(false))
        .await
        .expect("the request and its reply crossed within 10 s");
}

#[tokio::test]
async fn a_connection_to_a_forward_proxy_carries_a_request_and_its_reply() {
    tokio::time::timeout(std::time::Duration::from_secs(10), request_and_reply(true))
        .await
        .expect("the request and its reply crossed within 10 s");
}
