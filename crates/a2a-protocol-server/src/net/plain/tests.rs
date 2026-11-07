// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The plaintext push connector over real sockets: a destination dialled
//! straight or through an `http://` proxy, the connection it hands back, and
//! the one warning for a proxy of a kind it cannot use. Which destinations go
//! through which proxy is tested in `proxy/tests.rs`.

use std::collections::HashMap;
use std::future::poll_fn;
use std::sync::Arc;
use std::time::Duration;

use hyper::rt::{Read as _, ReadBuf, Write as _};
use hyper_util::client::legacy::connect::Connection as _;
use hyper_util::client::proxy::matcher::Matcher;
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use tower_service::Service as _;

use super::PlainConnector;
use crate::net::proxy::{ProxySetting, matcher};
use crate::net::resolver::system_tcp;

/// The proxy rules the variables in `pairs` give.
fn rules(pairs: &[(&str, &str)]) -> Arc<Matcher> {
    let map: HashMap<String, String> = pairs
        .iter()
        .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
        .collect();
    Arc::new(matcher(
        &move |k: &str| map.get(k).cloned(),
        &ProxySetting::Environment,
    ))
}

fn uri(s: &str) -> http::Uri {
    s.parse().expect("a URI")
}

/// Writes `ping`, flushes, shuts the write half, and reads what the peer
/// sends until it has four bytes or says it has no more.
async fn ping_and_read_the_reply(mut conn: std::pin::Pin<&mut super::PlainConn>) -> Vec<u8> {
    let sent = poll_fn(|cx| conn.as_mut().poll_write(cx, b"ping"))
        .await
        .expect("write");
    assert_eq!(sent, 4, "poll_write must report the bytes it took");
    poll_fn(|cx| conn.as_mut().poll_flush(cx))
        .await
        .expect("flush");
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
    reply
}

/// One request and its reply across a `PlainConn` to a real socket, straight
/// to the peer (`proxied` false) or as a forward request to a proxy
/// (`proxied` true).
///
/// The peer reads the request to its end before it answers, and reaches the
/// end only when the client shuts its write half. So the reply arrives only
/// if `poll_write` put the bytes on the socket and said how many,
/// `poll_shutdown` closed the write half, and `poll_read` handed back what the
/// peer sent. The deadline makes a shutdown that never reaches the socket a
/// failure and not a hang.
async fn request_and_reply(proxied: bool) {
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

    let peer = format!("http://127.0.0.1:{port}");
    let (destination, pairs) = if proxied {
        (
            "http://collector.invalid:4318".to_owned(),
            vec![("HTTP_PROXY", peer.as_str())],
        )
    } else {
        (peer.clone(), vec![])
    };
    let mut connector = PlainConnector::new(rules(&pairs), system_tcp());
    let conn = connector.call(uri(&destination)).await.expect("connect");
    assert_eq!(
        conn.connected().is_proxied(),
        proxied,
        "a forward request to a proxy is reported as proxied, a direct one is not"
    );
    let mut conn = std::pin::pin!(conn);
    let reply = tokio::time::timeout(
        Duration::from_secs(20),
        ping_and_read_the_reply(conn.as_mut()),
    )
    .await
    .expect(
        "the exchange ended within 20 s: the write half was shut and the read gave back the reply",
    );
    assert_eq!(reply, b"pong");
    assert_eq!(server.await.expect("the peer's task"), b"ping");
}

#[tokio::test]
async fn a_request_and_its_reply_cross_a_direct_connection() {
    request_and_reply(false).await;
}

#[tokio::test]
async fn a_request_and_its_reply_cross_a_connection_to_a_proxy() {
    request_and_reply(true).await;
}

/// What `tracing` recorded at `WARN`.
#[cfg(feature = "tracing")]
#[derive(Clone, Default)]
struct Warnings(Arc<std::sync::Mutex<Vec<String>>>);

#[cfg(feature = "tracing")]
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

/// A proxy of a kind this connector cannot use is named when the first
/// connection is made, and not again: the warning is for the person who
/// configured it, not one line per delivery.
#[cfg(feature = "tracing")]
#[test]
fn an_unsupported_proxy_is_named_on_the_first_connection_and_only_then() {
    use tracing_subscriber::layer::SubscriberExt as _;

    let warnings = Warnings::default();
    let _guard =
        tracing::subscriber::set_default(tracing_subscriber::registry().with(warnings.clone()));
    let mut connector =
        PlainConnector::new(rules(&[("ALL_PROXY", "socks5://p:1080")]), system_tcp());

    // The warning is said by `call` itself; the future is never polled, so
    // nothing connects.
    drop(connector.call(uri("http://c:4318")));
    let said = warnings.0.lock().expect("lock").clone();
    assert_eq!(said.len(), 1, "{said:?}");
    assert!(
        said[0].contains("http://c:4318") && said[0].contains("`socks5://`"),
        "{said:?}"
    );

    drop(connector.call(uri("http://c:4318")));
    let said = warnings.0.lock().expect("lock").clone();
    assert_eq!(said.len(), 1, "said again: {said:?}");
}
