// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Tests for [`PinResolver`](super::PinResolver): a pinned host resolves to
//! the pinned addresses and nothing else, for as long as the guard lives.

use std::net::SocketAddr;

use hyper_util::client::legacy::connect::dns::Name;
use tower_service::Service;

use super::{PinResolver, Tcp, system_tcp};

fn name(host: &str) -> Name {
    host.parse().expect("a valid name")
}

fn addr(text: &str) -> SocketAddr {
    text.parse().expect("an address")
}

async fn resolve(resolver: &PinResolver, host: &str) -> Vec<SocketAddr> {
    resolver
        .clone()
        .call(name(host))
        .await
        .expect("resolves")
        .collect()
}

/// The point of the module: whatever DNS says for the name, the pinned
/// answer is the only one. `localhost` stands in for a name that answers
/// differently on the second lookup; it needs no network to resolve.
#[tokio::test]
async fn a_pinned_host_resolves_to_the_pin_and_not_to_dns() {
    let resolver = PinResolver::default();
    let pin = resolver.pin("localhost", vec![addr("192.0.2.7:0")]);
    assert_eq!(resolve(&resolver, "localhost").await, [addr("192.0.2.7:0")]);

    drop(pin);
    let after = resolve(&resolver, "localhost").await;
    assert!(
        after.iter().all(|a| a.ip().is_loopback()) && !after.is_empty(),
        "with the pin gone, the system answers: {after:?}"
    );
}

/// DNS names are case-insensitive and a trailing dot names the same host; a
/// pin that missed on either would let the second lookup through.
#[tokio::test]
async fn a_pin_matches_the_name_however_it_is_written() {
    let resolver = PinResolver::default();
    let _pin = resolver.pin("WWW.Example.COM.", vec![addr("203.0.113.9:0")]);
    for spelling in ["www.example.com", "WWW.EXAMPLE.COM", "www.example.com."] {
        assert_eq!(
            resolve(&resolver, spelling).await,
            [addr("203.0.113.9:0")],
            "{spelling}"
        );
    }
}

/// A pin names one host. Another host, or a name that only contains it, is
/// not pinned.
#[test]
fn a_pin_does_not_reach_another_host() {
    let resolver = PinResolver::default();
    let _pin = resolver.pin("a.example", vec![addr("203.0.113.1:0")]);
    assert!(resolver.pinned("a.example").is_some());
    assert!(resolver.pinned("b.example").is_none());
    assert!(resolver.pinned("xa.example").is_none());
    assert!(resolver.pinned("a.example.org").is_none());
}

/// Two deliveries to one host each hold a pin; one finishing must not take
/// the other's addresses away, and the host is free again after both.
#[test]
fn pins_stack_and_each_guard_removes_only_its_own() {
    let resolver = PinResolver::default();
    let first = resolver.pin("h.example", vec![addr("203.0.113.1:443")]);
    let second = resolver.pin(
        "h.example",
        vec![addr("203.0.113.2:443"), addr("203.0.113.1:443")],
    );
    assert_eq!(
        resolver.pinned("h.example"),
        Some(vec![addr("203.0.113.1:443"), addr("203.0.113.2:443")]),
        "both sets, in pinned order, without repeats"
    );

    drop(first);
    assert_eq!(
        resolver.pinned("h.example"),
        Some(vec![addr("203.0.113.2:443"), addr("203.0.113.1:443")]),
        "the second delivery's own addresses stay"
    );

    drop(second);
    assert_eq!(resolver.pinned("h.example"), None);
    assert!(
        resolver.pins.lock().expect("lock").is_empty(),
        "no entry is left behind for a host nobody is delivering to"
    );
}

/// Clones of a resolver (the connector clones it per request) see one set of
/// pins.
#[test]
fn clones_share_their_pins() {
    let resolver = PinResolver::default();
    let clone = resolver.clone();
    let _pin = resolver.pin("h.example", vec![addr("203.0.113.1:443")]);
    assert!(clone.pinned("h.example").is_some());
}

async fn listener() -> (tokio::net::TcpListener, SocketAddr) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let local = listener.local_addr().expect("addr");
    (listener, local)
}

/// The wiring, not just the table: a connector built on the resolver dials
/// the pinned address for a name that resolves nowhere, and the same name
/// fails once the pin is gone. `.invalid` never resolves (RFC 6761).
#[tokio::test]
async fn the_connector_dials_the_pinned_address() {
    let resolver = PinResolver::default();
    let mut tcp: Tcp =
        hyper_util::client::legacy::connect::HttpConnector::new_with_resolver(resolver.clone());
    let (listener, local) = listener().await;
    let uri: http::Uri = format!("http://pinned.invalid:{}/", local.port())
        .parse()
        .expect("uri");

    let pin = resolver.pin("pinned.invalid", vec![local]);
    let accepted = tokio::spawn(async move { listener.accept().await.is_ok() });
    tcp.call(uri.clone()).await.expect("connects to the pin");
    assert!(
        accepted.await.expect("accept task"),
        "the listener was dialled"
    );

    drop(pin);
    let refused = tcp.call(uri).await;
    assert!(refused.is_err(), "unpinned, the name resolves nowhere");
}

/// `system_tcp` is a connector with no pins: it dials what the system
/// resolver gives, here a literal.
#[tokio::test]
async fn system_tcp_dials_an_ordinary_address() {
    let (listener, local) = listener().await;
    let mut tcp = system_tcp();
    let accepted = tokio::spawn(async move { listener.accept().await.is_ok() });
    let uri: http::Uri = format!("http://{local}/").parse().expect("uri");
    tcp.call(uri).await.expect("connects");
    assert!(accepted.await.expect("accept task"));
}

/// The resolver prints its type and nothing of its pins. A host and the
/// addresses it was checked against are a delivery's business; they do not
/// belong in a `{:?}` of the client that holds the resolver.
#[test]
fn debug_names_the_type_and_hides_the_pins() {
    let resolver = PinResolver::default();
    let _pin = resolver.pin("hooks.example", vec![addr("192.0.2.7:0")]);
    assert_eq!(format!("{resolver:?}"), "PinResolver { .. }");
}
