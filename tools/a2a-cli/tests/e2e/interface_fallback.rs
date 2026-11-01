// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! A first interface that cannot be built must not move `--header` to another
//! host (audit SEC-09, NEW-f-1).

use super::card_origin::{Seen, serve_fixed, serve_on};
use super::*;

// -- a first interface that cannot be built (audit NEW-f-1) -------------------
//
// The check above looks at the interface `from_card` chose. `build()` moves
// on to the card's later interfaces when the chosen one cannot be built, so a
// card whose first interface is on the typed host but unusable, and whose
// second is elsewhere, used to carry `--header` to the second.

/// The URL to type (server A, by the name `localhost`) and what server B has
/// been sent. A's card lists `first(A's port)` and then B, which is another
/// host by name (`127.0.0.1`).
async fn card_whose_first_interface_cannot_be_built(
    first: impl FnOnce(u16) -> AgentInterface,
) -> (String, Seen) {
    let (seen_b, seen_a): (Seen, Seen) = (Arc::default(), Arc::default());
    let error = br#"{"jsonrpc":"2.0","id":1,"error":{"code":-32601,"message":"no"}}"#.to_vec();
    let b = serve_fixed(error, Arc::clone(&seen_b)).await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let a_port = listener.local_addr().expect("addr").port();
    let mut card = card_for(b);
    card.supported_interfaces = vec![
        first(a_port),
        AgentInterface::jsonrpc(format!("http://127.0.0.1:{}", b.port())),
    ];
    let card = serde_json::to_vec(&card).expect("card json");
    serve_on(listener, card, seen_a);
    (format!("http://localhost:{a_port}"), seen_b)
}

/// A binding the client does not implement, on the typed host.
fn unknown_binding(port: u16) -> AgentInterface {
    AgentInterface::new(format!("http://localhost:{port}/"), "NOPE")
}

/// A scheme the HTTP transports refuse, on the typed host.
fn unusable_scheme(port: u16) -> AgentInterface {
    AgentInterface::new(format!("ftp://localhost:{port}/"), "JSONRPC")
}

async fn assert_the_header_stays_home(first: impl FnOnce(u16) -> AgentInterface) {
    let (url, seen_b) = card_whose_first_interface_cannot_be_built(first).await;
    let out = a2a(&[
        "--header",
        "Authorization=Bearer s3cret",
        "send",
        &url,
        "hi",
    ])
    .await;
    assert_eq!(code(&out), 1, "stdout: {}", stdout(&out));
    assert!(
        seen_b.lock().unwrap().is_empty(),
        "the credential reached another host through the card's fallback: {:?}",
        seen_b.lock().unwrap()
    );
    assert!(
        stderr(&out).contains("--follow-card-origin"),
        "the refusal names the way to opt in: {}",
        stderr(&out)
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_header_does_not_follow_the_fallback_past_an_unknown_binding() {
    assert_the_header_stays_home(unknown_binding).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_header_does_not_follow_the_fallback_past_an_unusable_scheme() {
    assert_the_header_stays_home(unusable_scheme).await;
}

/// Pinning is for credentials only: with no `--header` the card's later
/// interfaces are still tried, as they always were.
#[tokio::test(flavor = "multi_thread")]
async fn without_a_header_the_fallback_still_reaches_the_next_interface() {
    let (url, seen_b) = card_whose_first_interface_cannot_be_built(unknown_binding).await;
    let out = a2a(&["send", &url, "hi"]).await;
    assert_eq!(code(&out), 1, "B answers with an error: {}", stdout(&out));
    assert!(!seen_b.lock().unwrap().is_empty(), "the call reached B");
}

/// `--follow-card-origin` is the opt-in: the caller said the card may steer
/// the credential, so the fallback is allowed too.
#[tokio::test(flavor = "multi_thread")]
async fn the_flag_also_allows_the_fallback() {
    let (url, seen_b) = card_whose_first_interface_cannot_be_built(unknown_binding).await;
    let out = a2a(&[
        "--header",
        "Authorization=Bearer s3cret",
        "--follow-card-origin",
        "send",
        &url,
        "hi",
    ])
    .await;
    assert_eq!(code(&out), 1, "B answers with an error: {}", stdout(&out));
    assert!(
        seen_b.lock().unwrap().iter().any(|a| a == "Bearer s3cret"),
        "{:?}",
        seen_b.lock().unwrap()
    );
}
