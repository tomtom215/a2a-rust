// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! `--header` goes to the host the user typed, not to wherever the agent card
//! points (audit SEC-09).
//!
//! Server A is the one the user types (`localhost`); its card names server B
//! (`127.0.0.1`), another host by name. B records the `Authorization` header of
//! every request it receives.

use super::*;

pub(super) type Seen = Arc<std::sync::Mutex<Vec<String>>>;

/// A server that answers every request `200` with `body` and records each
/// request's `authorization` header (`""` when absent).
pub(super) async fn serve_fixed(body: Vec<u8>, seen: Seen) -> SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    serve_on(listener, body, seen)
}

/// [`serve_fixed`] on a listener the caller already holds, for a card that has
/// to name its own server's port.
pub(super) fn serve_on(listener: tokio::net::TcpListener, body: Vec<u8>, seen: Seen) -> SocketAddr {
    let addr = listener.local_addr().expect("addr");
    tokio::spawn(async move {
        while let Ok((stream, _)) = listener.accept().await {
            let (body, seen) = (body.clone(), Arc::clone(&seen));
            tokio::spawn(async move {
                let io = hyper_util::rt::TokioIo::new(stream);
                let service = hyper::service::service_fn(move |req: hyper::Request<_>| {
                    // Every value, in order, joined: a header sent twice must
                    // show as two values, not as the first of them.
                    let auth = req
                        .headers()
                        .get_all("authorization")
                        .iter()
                        .filter_map(|v| v.to_str().ok())
                        .collect::<Vec<_>>()
                        .join(", ");
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
pub(super) async fn card_that_names_another_host() -> (String, Seen) {
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

/// [`a2a`] with environment variables set for the child.
pub(super) async fn a2a_env(args: &[&str], env: &[(&str, &str)]) -> Output {
    let args: Vec<String> = args.iter().map(|s| (*s).to_owned()).collect();
    let env: Vec<(String, String)> = env
        .iter()
        .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
        .collect();
    tokio::task::spawn_blocking(move || {
        Command::new(env!("CARGO_BIN_EXE_a2a"))
            .args(&args)
            .envs(env)
            .output()
            .expect("spawn a2a")
    })
    .await
    .expect("join")
}

/// A JSON-RPC error answer, which makes `send` fail after the request was
/// recorded.
pub(super) const A_JSON_RPC_ERROR: &[u8] =
    br#"{"jsonrpc":"2.0","id":1,"error":{"code":-32601,"message":"no"}}"#;

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
