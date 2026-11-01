// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! `--header` goes to the host the user typed, not to wherever the agent card
//! points (audit SEC-09).
//!
//! Server A is the one the user types (`localhost`); its card names server B
//! (`127.0.0.1`), another host by name. B records the `Authorization` header of
//! every request it receives.

use super::*;

type Seen = Arc<std::sync::Mutex<Vec<String>>>;

/// A server that answers every request `200` with `body` and records each
/// request's `authorization` header (`""` when absent).
async fn serve_fixed(body: Vec<u8>, seen: Seen) -> SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    serve_on(listener, body, seen)
}

/// [`serve_fixed`] on a listener the caller already holds, for a card that has
/// to name its own server's port.
fn serve_on(listener: tokio::net::TcpListener, body: Vec<u8>, seen: Seen) -> SocketAddr {
    let addr = listener.local_addr().expect("addr");
    tokio::spawn(async move {
        while let Ok((stream, _)) = listener.accept().await {
            let (body, seen) = (body.clone(), Arc::clone(&seen));
            tokio::spawn(async move {
                let io = hyper_util::rt::TokioIo::new(stream);
                let service = hyper::service::service_fn(move |req: hyper::Request<_>| {
                    let auth = req
                        .headers()
                        .get("authorization")
                        .and_then(|v| v.to_str().ok())
                        .unwrap_or_default()
                        .to_owned();
                    seen.lock().unwrap().push(auth);
                    let body = body.clone();
                    async move {
                        Ok::<_, std::convert::Infallible>(
                            hyper::Response::builder()
                                .header("content-type", "application/json")
                                .body(http_body_util::Full::new(hyper::body::Bytes::from(body)))
                                .unwrap(),
                        )
                    }
                });
                let _ = hyper_util::server::conn::auto::Builder::new(
                    hyper_util::rt::TokioExecutor::new(),
                )
                .serve_connection(io, service)
                .await;
            });
        }
    });
    addr
}

/// The pair: returns the URL to type (server A, by the name `localhost`) and
/// what server B has been sent.
async fn card_that_names_another_host() -> (String, Seen) {
    let (seen_b, seen_a): (Seen, Seen) = (Arc::default(), Arc::default());
    let error = br#"{"jsonrpc":"2.0","id":1,"error":{"code":-32601,"message":"no"}}"#.to_vec();
    let b = serve_fixed(error, Arc::clone(&seen_b)).await;
    // The card names B at `127.0.0.1:<port>`.
    let card = serde_json::to_vec(&card_for(b)).expect("card json");
    let a = serve_fixed(card, seen_a).await;
    (format!("http://localhost:{}", a.port()), seen_b)
}

#[tokio::test(flavor = "multi_thread")]
async fn a_header_is_not_sent_to_a_host_the_card_names() {
    let (url, seen_b) = card_that_names_another_host().await;
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
        stderr(&out).contains("--follow-card-origin"),
        "{}",
        stderr(&out)
    );
    assert!(
        seen_b.lock().unwrap().is_empty(),
        "the second host was sent nothing: {:?}",
        seen_b.lock().unwrap()
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn the_flag_sends_it_where_the_card_says() {
    let (url, seen_b) = card_that_names_another_host().await;
    let out = a2a(&[
        "--header",
        "Authorization=Bearer s3cret",
        "--follow-card-origin",
        "send",
        &url,
        "hi",
    ])
    .await;
    // B answers with a JSON-RPC error, so the call fails; what matters is
    // that the request reached B with the header.
    assert_eq!(code(&out), 1, "stdout: {}", stdout(&out));
    assert!(
        seen_b.lock().unwrap().iter().any(|a| a == "Bearer s3cret"),
        "{:?}",
        seen_b.lock().unwrap()
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn without_a_header_the_card_is_followed_as_before() {
    let (url, seen_b) = card_that_names_another_host().await;
    let out = a2a(&["send", &url, "hi"]).await;
    assert_eq!(code(&out), 1, "B answers with an error: {}", stdout(&out));
    assert!(
        !stderr(&out).contains("--follow-card-origin"),
        "{}",
        stderr(&out)
    );
    assert!(!seen_b.lock().unwrap().is_empty(), "the call reached B");
}

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

// -- `card verify`: the JWK Set is fetched from a URL of its own --------------

/// A card server (by the name `localhost`) and a JWK Set server, which records
/// what it is sent and is reachable under two host names.
struct Servers {
    /// The card's URL, host `localhost`.
    card: String,
    /// The JWK Set by `127.0.0.1`: another host than the card's, by name.
    jwks_elsewhere: String,
    /// The same JWK Set by `localhost`: the card's host.
    jwks_same_host: String,
    /// What the JWK Set server was sent: each request's `authorization`.
    seen: Seen,
}

async fn card_and_keys_on_two_hosts() -> Servers {
    let seen: Seen = Arc::default();
    let keys = serve_fixed(br#"{"keys":[]}"#.to_vec(), Arc::clone(&seen)).await;
    let card = serde_json::to_vec(&card_for(keys)).expect("card json");
    let card_server = serve_fixed(card, Arc::default()).await;
    Servers {
        card: format!("http://localhost:{}", card_server.port()),
        jwks_elsewhere: format!("http://127.0.0.1:{}/jwks.json", keys.port()),
        jwks_same_host: format!("http://localhost:{}/jwks.json", keys.port()),
        seen,
    }
}

const NOTE: &str = "--header is not sent to the JWK Set's host";

/// `a2a --header Authorization=Bearer s3cret <extra> card verify <card> --jwks
/// <jwks>`. The card has no signature, so it never verifies (exit 1); what the
/// tests read is what reached the JWK Set server.
async fn verify_with_header(extra: &[&str], card: &str, jwks: &str) -> Output {
    let mut args = vec!["--header", "Authorization=Bearer s3cret"];
    args.extend_from_slice(extra);
    args.extend_from_slice(&["card", "verify", card, "--jwks", jwks]);
    a2a(&args).await
}

#[tokio::test(flavor = "multi_thread")]
async fn a_header_is_not_sent_to_a_jwk_set_on_another_host_than_the_card() {
    let s = card_and_keys_on_two_hosts().await;
    let out = verify_with_header(&[], &s.card, &s.jwks_elsewhere).await;
    assert_eq!(code(&out), 1, "stdout: {}", stdout(&out));
    assert!(stderr(&out).contains(NOTE), "{}", stderr(&out));
    let seen = s.seen.lock().unwrap();
    assert!(!seen.is_empty(), "the keys were still fetched");
    assert!(
        seen.iter().all(String::is_empty),
        "and without the header: {seen:?}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn the_flag_sends_the_header_to_a_jwk_set_on_another_host() {
    let s = card_and_keys_on_two_hosts().await;
    let out = verify_with_header(&["--follow-card-origin"], &s.card, &s.jwks_elsewhere).await;
    assert_eq!(code(&out), 1, "stdout: {}", stdout(&out));
    assert!(!stderr(&out).contains(NOTE), "{}", stderr(&out));
    assert!(
        s.seen.lock().unwrap().iter().any(|a| a == "Bearer s3cret"),
        "{:?}",
        s.seen.lock().unwrap()
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_jwk_set_on_the_cards_own_host_gets_the_header() {
    let s = card_and_keys_on_two_hosts().await;
    let out = verify_with_header(&[], &s.card, &s.jwks_same_host).await;
    assert_eq!(code(&out), 1, "stdout: {}", stdout(&out));
    assert!(!stderr(&out).contains(NOTE), "{}", stderr(&out));
    assert!(
        s.seen.lock().unwrap().iter().any(|a| a == "Bearer s3cret"),
        "{:?}",
        s.seen.lock().unwrap()
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_card_read_from_a_file_has_no_host_so_the_jwk_set_gets_the_header() {
    let s = card_and_keys_on_two_hosts().await;
    // The card as a file; the JWK Set is still a URL, on another host.
    let path = std::env::temp_dir().join(format!("a2a-card-origin-{}.json", std::process::id()));
    let card =
        serde_json::to_vec(&card_for("127.0.0.1:9".parse().expect("addr"))).expect("card json");
    std::fs::write(&path, card).expect("write the card");
    let out = verify_with_header(&[], path.to_str().expect("utf-8 path"), &s.jwks_elsewhere).await;
    let _ = std::fs::remove_file(&path);
    assert_eq!(code(&out), 1, "stdout: {}", stdout(&out));
    assert!(!stderr(&out).contains(NOTE), "{}", stderr(&out));
    assert!(
        s.seen.lock().unwrap().iter().any(|a| a == "Bearer s3cret"),
        "{:?}",
        s.seen.lock().unwrap()
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn with_no_header_card_verify_says_nothing_about_one() {
    let s = card_and_keys_on_two_hosts().await;
    let out = a2a(&["card", "verify", &s.card, "--jwks", &s.jwks_elsewhere]).await;
    assert_eq!(code(&out), 1, "stdout: {}", stdout(&out));
    assert!(!stderr(&out).contains(NOTE), "{}", stderr(&out));
    assert!(!s.seen.lock().unwrap().is_empty(), "the keys were fetched");
}
