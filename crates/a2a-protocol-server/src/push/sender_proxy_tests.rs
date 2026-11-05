// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! `HttpPushSender` behind a real forward proxy: a tiny HTTP proxy on
//! loopback that records each request line and forwards the request to one
//! receiver (the names in these tests resolve nowhere, so the proxy is told
//! where they go). In both builds: the plaintext one (audit NEW-b-3) uses the
//! same proxy rules and pins as the TLS one.
//!
//! Each test gives the sender its own variable table, never the process
//! environment, so the tests cannot see each other's or the host's proxy.

use std::sync::{Arc, Mutex};

use a2a_protocol_types::events::TaskStatusUpdateEvent;
use a2a_protocol_types::task::{ContextId, TaskId, TaskState, TaskStatus};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

use super::*;
use crate::net::proxy::{self, ProxySetting};

/// Reads one HTTP/1.1 request: its head and its body, by `Content-Length`.
async fn read_request(stream: &mut TcpStream) -> (String, Vec<u8>) {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    let head_end = loop {
        let n = stream.read(&mut chunk).await.unwrap();
        assert!(n > 0, "the connection closed inside a request head");
        buf.extend_from_slice(&chunk[..n]);
        if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break i + 4;
        }
    };
    let head = String::from_utf8_lossy(&buf[..head_end]).into_owned();
    let length = head
        .lines()
        .find_map(|l| {
            let (name, value) = l.split_once(':')?;
            name.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse::<usize>().ok())?
        })
        .unwrap_or(0);
    let mut body = buf[head_end..].to_vec();
    while body.len() < length {
        let n = stream.read(&mut chunk).await.unwrap();
        assert!(n > 0, "the connection closed inside a request body");
        body.extend_from_slice(&chunk[..n]);
    }
    (head, body)
}

fn header<'a>(head: &'a str, name: &str) -> Option<&'a str> {
    head.lines().find_map(|l| {
        let (n, v) = l.split_once(':')?;
        n.eq_ignore_ascii_case(name).then(|| v.trim())
    })
}

/// A receiver on loopback that answers `200` and records each `Host`.
async fn receiver() -> (u16, Arc<Mutex<Vec<String>>>) {
    let (port, hosts, _) = receiver_with_lines().await;
    (port, hosts)
}

/// [`receiver`], also recording each request line.
async fn receiver_with_lines() -> (u16, Arc<Mutex<Vec<String>>>, Arc<Mutex<Vec<String>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let hosts = Arc::new(Mutex::new(Vec::new()));
    let lines = Arc::new(Mutex::new(Vec::new()));
    let (seen, seen_lines) = (Arc::clone(&hosts), Arc::clone(&lines));
    tokio::spawn(async move {
        while let Ok((mut stream, _)) = listener.accept().await {
            let (head, _) = read_request(&mut stream).await;
            seen_lines
                .lock()
                .unwrap()
                .push(head.lines().next().unwrap_or_default().to_owned());
            seen.lock()
                .unwrap()
                .push(header(&head, "host").unwrap_or_default().to_owned());
            let _ = stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                .await;
        }
    });
    (port, hosts, lines)
}

/// A forward proxy on loopback: records each request line, sends the request
/// on to `upstream` in origin form, and relays the answer.
async fn forward_proxy(upstream: u16) -> (String, Arc<Mutex<Vec<String>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let lines = Arc::new(Mutex::new(Vec::new()));
    let seen = Arc::clone(&lines);
    tokio::spawn(async move {
        while let Ok((mut client, _)) = listener.accept().await {
            let seen = Arc::clone(&seen);
            tokio::spawn(async move {
                let (head, body) = read_request(&mut client).await;
                let (line, rest) = head.split_once("\r\n").unwrap();
                seen.lock().unwrap().push(line.to_owned());
                let mut parts = line.split(' ');
                let (method, target) = (parts.next().unwrap(), parts.next().unwrap());
                let path = target
                    .strip_prefix("http://")
                    .and_then(|t| t.find('/').map(|i| &t[i..]))
                    .unwrap_or(target);
                let mut up = TcpStream::connect(("127.0.0.1", upstream)).await.unwrap();
                up.write_all(format!("{method} {path} HTTP/1.1\r\n{rest}").as_bytes())
                    .await
                    .unwrap();
                up.write_all(&body).await.unwrap();
                let mut answer = Vec::new();
                let _ = up.read_to_end(&mut answer).await;
                let _ = client.write_all(&answer).await;
            });
        }
    });
    (url, lines)
}

/// A sender whose proxy rules come from `vars`, delivering once.
fn sender(vars: &[(&str, String)]) -> HttpPushSender {
    let vars: Vec<(String, String)> = vars
        .iter()
        .map(|(k, v)| ((*k).to_owned(), v.clone()))
        .collect();
    let env = move |name: &str| vars.iter().find(|(k, _)| k == name).map(|(_, v)| v.clone());
    let rules = Arc::new(proxy::matcher(&env, &ProxySetting::Environment));
    HttpPushSender {
        client: push_client_with_rules(rules),
        retry_policy: PushRetryPolicy::default().with_max_attempts(1),
        ..HttpPushSender::new()
    }
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

/// NEW-b-4: `NO_PROXY` names the webhook's host, so the delivery goes direct,
/// to the checked address, with the host the URL named. Until 0.16.0 the URI
/// was rewritten to the address first, `NO_PROXY` was matched against
/// `127.0.0.1`, and the request went to the proxy. (The loopback exemption is
/// off, as it is for any address that is not loopback.)
#[tokio::test]
async fn no_proxy_is_matched_against_the_webhooks_name() {
    let (port, hosts) = receiver().await;
    let (proxy, lines) = forward_proxy(port).await;
    let sender = sender(&[
        ("HTTP_PROXY", proxy),
        ("NO_PROXY", ".partner.test".to_owned()),
        ("A2A_PROXY_LOOPBACK", "1".to_owned()),
    ]);
    let url = format!("http://hooks.partner.test:{port}/hook");

    sender
        .deliver(&url, &event(), &config(&url), Some(vec![loopback(port)]))
        .await
        .expect("delivered directly");
    assert_eq!(
        *lines.lock().unwrap(),
        Vec::<String>::new(),
        "nothing reached the proxy"
    );
    assert_eq!(
        *hosts.lock().unwrap(),
        [format!("hooks.partner.test:{port}")]
    );
}

/// NEW-b-4: a proxied `http://` delivery is sent to the proxy by name, so a
/// proxy that allows destinations by name can allow it. Until 0.16.0 the
/// proxy was asked for `http://127.0.0.1:<port>/hook`.
#[tokio::test]
async fn a_proxied_http_delivery_asks_the_proxy_for_the_name() {
    let (port, hosts) = receiver().await;
    let (proxy, lines) = forward_proxy(port).await;
    let sender = sender(&[
        ("HTTP_PROXY", proxy),
        ("A2A_PROXY_LOOPBACK", "1".to_owned()),
    ]);
    let url = format!("http://hooks.partner.test:{port}/hook");

    sender
        .deliver(&url, &event(), &config(&url), Some(vec![loopback(port)]))
        .await
        .expect("delivered through the proxy");
    assert_eq!(
        *lines.lock().unwrap(),
        [format!(
            "POST http://hooks.partner.test:{port}/hook HTTP/1.1"
        )]
    );
    assert_eq!(
        *hosts.lock().unwrap(),
        [format!("hooks.partner.test:{port}")]
    );
}

/// NEW-b-1: a name this process cannot resolve, behind a proxy that can, is
/// delivered through the proxy when the sender is told the proxy resolves
/// names. `.invalid` never resolves (RFC 6761).
#[tokio::test]
async fn with_proxy_resolved_names_a_name_only_the_proxy_knows_is_delivered() {
    let (port, hosts) = receiver().await;
    let (proxy, lines) = forward_proxy(port).await;
    let sender = sender(&[("HTTP_PROXY", proxy)]).with_proxy_resolved_names();
    let url = format!("http://hooks.example.invalid:{port}/hook");

    sender
        .send(&url, &event(), &config(&url))
        .await
        .expect("the proxy resolved the name");
    assert_eq!(
        *lines.lock().unwrap(),
        [format!(
            "POST http://hooks.example.invalid:{port}/hook HTTP/1.1"
        )]
    );
    assert_eq!(hosts.lock().unwrap().len(), 1);
}

/// NEW-b-1, the default: the lookup is still required, and the error says
/// what would let the proxy resolve it. Nothing reaches the proxy.
#[tokio::test]
async fn by_default_a_name_this_process_cannot_resolve_fails_and_names_the_option() {
    let (port, _) = receiver().await;
    let (proxy, lines) = forward_proxy(port).await;
    let sender = sender(&[("HTTP_PROXY", proxy)]);
    let url = format!("http://hooks.example.invalid:{port}/hook");

    let err = sender
        .send(&url, &event(), &config(&url))
        .await
        .expect_err("not resolvable here");
    assert!(
        err.to_string().contains("with_proxy_resolved_names"),
        "{err}"
    );
    assert!(lines.lock().unwrap().is_empty());
}

/// NEW-b-1: the option applies only where a proxy carries the request. A
/// destination `NO_PROXY` exempts goes direct, and must resolve here.
#[tokio::test]
async fn with_proxy_resolved_names_a_direct_destination_must_still_resolve() {
    let (port, _) = receiver().await;
    let (proxy, lines) = forward_proxy(port).await;
    let sender = sender(&[("HTTP_PROXY", proxy), ("NO_PROXY", ".invalid".to_owned())])
        .with_proxy_resolved_names();
    let url = format!("http://hooks.example.invalid:{port}/hook");

    let err = sender
        .send(&url, &event(), &config(&url))
        .await
        .expect_err("direct, and not resolvable");
    assert!(
        !err.to_string().contains("with_proxy_resolved_names"),
        "the hint is for a proxied destination only: {err}"
    );
    assert!(lines.lock().unwrap().is_empty());
}

// Until 0.16.0 an `http://` delivery was rewritten to one checked address per
// attempt (`rewrite_uri_with_pinned_addr`, `host_header_from_url`,
// `pin_target`, `pick_addr`, each with unit tests). It is now pinned in the
// client's resolver, as `https://` was, and those helpers are gone (audit
// NEW-b-4). The tests below are those unit tests rewritten against the
// behaviour that replaced them, end to end: what the receiver is sent.

/// Was `rewrite_uri_preserves_scheme_path_and_query` and
/// `rewrite_uri_default_path_when_missing`: the receiver is sent the URL's
/// own path and query, and `/` for a URL with no path.
#[tokio::test]
async fn a_pinned_http_delivery_sends_the_urls_path_and_query() {
    let (port, _, lines) = receiver_with_lines().await;
    let sender = sender(&[]);
    for url in [
        format!("http://hooks.example.test:{port}/webhook?x=1"),
        format!("http://hooks.example.test:{port}"),
    ] {
        sender
            .deliver(&url, &event(), &config(&url), Some(vec![loopback(port)]))
            .await
            .expect("delivered to the pinned address");
    }
    assert_eq!(
        *lines.lock().unwrap(),
        ["POST /webhook?x=1 HTTP/1.1", "POST / HTTP/1.1"]
    );
}

/// Was `host_header_includes_port_when_present`,
/// `host_header_omits_default_port` and `pin_target_pins_http_but_not_https`:
/// a pinned `http://` delivery presents the URL's host, with the port the URL
/// names, and never the address it was pinned to.
#[tokio::test]
async fn a_pinned_http_delivery_presents_the_urls_host_not_the_address() {
    let (port, hosts) = receiver().await;
    let sender = sender(&[]);
    let url = format!("http://hooks.example.test:{port}/webhook");

    sender
        .deliver(&url, &event(), &config(&url), Some(vec![loopback(port)]))
        .await
        .expect("delivered to the pinned address");
    assert_eq!(
        *hosts.lock().unwrap(),
        [format!("hooks.example.test:{port}")]
    );
    assert!(
        !sender.client.is_pinned("hooks.example.test"),
        "the pin ends with the delivery"
    );
}

/// Was `rewrite_uri_uses_ipv6_brackets`, which checked that an IPv6 address
/// was bracketed when written into a rewritten URI. Nothing writes an address
/// into a URI now: the pin hands the connector the address itself, and an
/// IPv6 one comes back from the resolver as it went in.
#[tokio::test]
async fn an_ipv6_address_is_pinned_as_it_is() {
    use hyper_util::client::legacy::connect::dns::Name;
    use std::str::FromStr as _;
    use tower_service::Service as _;

    let mut resolver = crate::net::resolver::PinResolver::default();
    let pinned: SocketAddr = "[2001:db8::1]:8080".parse().unwrap();
    let _pin = resolver.pin("v6.example.test", vec![pinned]);
    let answer: Vec<SocketAddr> = resolver
        .call(Name::from_str("v6.example.test").unwrap())
        .await
        .unwrap()
        .collect();
    assert_eq!(answer, [pinned]);
}

/// The form check ahead of it: the refusal of an internal name is not taken
/// for a name that does not resolve, so it is not left to the proxy.
#[tokio::test]
async fn behind_a_proxy_a_refused_form_is_not_left_to_the_proxy() {
    let looked = std::sync::atomic::AtomicBool::new(false);
    let err = preflight_with("http://metadata.internal/h", TEN, true, true, |_| {
        looked.store(true, std::sync::atomic::Ordering::SeqCst);
        answers(&[])
    })
    .await
    .expect_err("an internal name");
    assert!(err.to_string().contains("internal"), "{err}");
    assert!(
        !looked.load(std::sync::atomic::Ordering::SeqCst),
        "no lookup was made"
    );
}

/// Was `each_attempt_takes_the_next_address_and_the_schedule_wraps` and
/// `one_address_serves_every_attempt_and_none_gives_none` (audit B-13): an
/// address that is down is followed by the next, now within one attempt, by
/// the connector, rather than on the next attempt.
#[tokio::test]
async fn one_attempt_goes_on_to_the_next_pinned_address() {
    let (port, hosts) = receiver().await;
    let sender = sender(&[]);
    let url = format!("http://down.example.test:{port}/hook");
    // Refused at once: the receiver listens on 127.0.0.1 only.
    let down = SocketAddr::from((std::net::Ipv6Addr::LOCALHOST, port));

    sender
        .deliver(
            &url,
            &event(),
            &config(&url),
            Some(vec![down, loopback(port)]),
        )
        .await
        .expect("the second address answered within the one attempt");
    assert_eq!(hosts.lock().unwrap().len(), 1);
}

// `preflight_with`, the decision NEW-b-1 adds, with the resolver injected:
// the cases a real resolver cannot be made to produce on demand.

const TEN: std::time::Duration = std::time::Duration::from_secs(10);

fn answers(addrs: &'static [&'static str]) -> std::future::Ready<std::io::Result<Vec<SocketAddr>>> {
    std::future::ready(Ok(addrs.iter().map(|a| a.parse().unwrap()).collect()))
}

/// Behind a proxy, with the option, a private answer is still refused: the
/// option is for names that do not resolve here, not for names that resolve
/// somewhere they must not.
#[tokio::test]
async fn behind_a_proxy_a_private_answer_is_still_refused() {
    let err = preflight_with("http://hooks.example.com/h", TEN, true, true, |_| {
        answers(&["10.0.0.7:80"])
    })
    .await
    .expect_err("private");
    assert!(err.to_string().contains("private"), "{err}");
}

/// Behind a proxy, with the option, a public answer is checked and pinned as
/// always.
#[tokio::test]
async fn behind_a_proxy_a_public_answer_is_checked_as_always() {
    let got = preflight_with("http://hooks.example.com/h", TEN, true, true, |_| {
        answers(&["203.0.113.7:80"])
    })
    .await
    .unwrap();
    assert_eq!(got, Some(vec!["203.0.113.7:80".parse().unwrap()]));
}

/// Behind a proxy, with the option, no answer and an empty answer both leave
/// the name to the proxy.
#[tokio::test]
async fn behind_a_proxy_no_answer_leaves_the_name_to_the_proxy() {
    let refused = preflight_with("http://hooks.example.com/h", TEN, true, true, |_| async {
        Err(std::io::Error::other("NXDOMAIN"))
    })
    .await;
    assert_eq!(refused.unwrap(), None);
    let empty = preflight_with("http://hooks.example.com/h", TEN, true, true, |_| {
        answers(&[])
    })
    .await;
    assert_eq!(empty.unwrap(), None);
}

/// A lookup that never answers is cut off at the lookup bound, and behind a
/// proxy with the option the name goes to the proxy, 10 s later exactly.
#[tokio::test(start_paused = true)]
async fn behind_a_proxy_a_lookup_that_hangs_leaves_the_name_to_the_proxy() {
    let started = tokio::time::Instant::now();
    let got = preflight_with("http://hooks.example.com/h", TEN, true, true, |_| {
        std::future::pending::<std::io::Result<Vec<SocketAddr>>>()
    })
    .await;
    assert_eq!(got.unwrap(), None);
    assert_eq!(started.elapsed(), TEN);
}

/// Without the option the error names it; going direct (the pre-flight
/// `HttpPushSender` uses when no proxy applies), it does not, and a failed
/// lookup fails as it always did.
#[tokio::test]
async fn the_hint_is_given_only_behind_a_proxy_and_without_the_option() {
    let fail = |_| async { Err(std::io::Error::other("NXDOMAIN")) };
    let behind = preflight_with("http://hooks.example.com/h", TEN, true, false, fail)
        .await
        .expect_err("not resolvable");
    assert!(
        behind.to_string().contains("with_proxy_resolved_names"),
        "{behind}"
    );
    let direct = preflight_with("http://hooks.example.com/h", TEN, false, true, fail)
        .await
        .expect_err("not resolvable");
    assert!(
        !direct.to_string().contains("with_proxy_resolved_names"),
        "{direct}"
    );
}
