// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Where `--header` values come from and how they are spelled (audit SEC-09,
//! G/D-05): from the environment, without case, and on `a2a card`.

use super::card_origin::{
    A_JSON_RPC_ERROR, Seen, a2a_env, card_that_names_another_host, serve_fixed,
};
use super::*;

/// `--header-env` reads the value from the environment, so a credential is not
/// in the process list (audit SEC-09).
#[tokio::test(flavor = "multi_thread")]
async fn a_header_can_come_from_the_environment() {
    let seen: Seen = Arc::default();
    let addr = serve_fixed(A_JSON_RPC_ERROR.to_vec(), Arc::clone(&seen)).await;
    let url = format!("http://127.0.0.1:{}", addr.port());
    let out = a2a_env(
        &[
            "--binding",
            "jsonrpc",
            "--header-env",
            "Authorization=A2A_TEST_AUTH",
            "send",
            &url,
            "hi",
        ],
        &[("A2A_TEST_AUTH", "Bearer from-the-environment")],
    )
    .await;
    assert_eq!(code(&out), 1, "B answers with an error: {}", stdout(&out));
    assert_eq!(
        seen.lock().unwrap().as_slice(),
        ["Bearer from-the-environment"]
    );
    assert!(
        !stderr(&out).contains("from-the-environment"),
        "{}",
        stderr(&out)
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn an_unset_or_empty_variable_is_a_usage_error_that_names_it() {
    for env in [&[][..], &[("A2A_TEST_AUTH", "")]] {
        let out = a2a_env(
            &[
                "--header-env",
                "Authorization=A2A_TEST_AUTH",
                "card",
                "http://127.0.0.1:1",
            ],
            env,
        )
        .await;
        assert_eq!(code(&out), 2, "{}", stderr(&out));
        assert!(stderr(&out).contains("A2A_TEST_AUTH"), "{}", stderr(&out));
    }
}

/// A header from the environment is a credential like any other: it does not
/// go to a host the card names.
#[tokio::test(flavor = "multi_thread")]
async fn a_header_from_the_environment_is_held_to_the_same_host_rule() {
    let (url, seen_b) = card_that_names_another_host().await;
    let out = a2a_env(
        &[
            "--header-env",
            "Authorization=A2A_TEST_AUTH",
            "send",
            &url,
            "hi",
        ],
        &[("A2A_TEST_AUTH", "Bearer s3cret")],
    )
    .await;
    assert_eq!(code(&out), 1, "stdout: {}", stdout(&out));
    assert!(
        stderr(&out).contains("--follow-card-origin"),
        "{}",
        stderr(&out)
    );
    assert!(
        seen_b.lock().unwrap().is_empty(),
        "{:?}",
        seen_b.lock().unwrap()
    );
}

/// HTTP header names do not depend on case: one given twice is sent once, with
/// the last value, and the environment wins over `--header`.
#[tokio::test(flavor = "multi_thread")]
async fn a_repeated_header_name_is_sent_once_with_the_last_value() {
    let seen: Seen = Arc::default();
    let addr = serve_fixed(A_JSON_RPC_ERROR.to_vec(), Arc::clone(&seen)).await;
    let url = format!("http://127.0.0.1:{}", addr.port());
    let out = a2a(&[
        "--binding",
        "jsonrpc",
        "--header",
        "Authorization=one",
        "--header",
        "authorization=two",
        "send",
        &url,
        "hi",
    ])
    .await;
    assert_eq!(code(&out), 1, "{}", stdout(&out));
    assert_eq!(
        seen.lock().unwrap().as_slice(),
        ["two"],
        "one value, the last"
    );

    seen.lock().unwrap().clear();
    let out = a2a_env(
        &[
            "--binding",
            "jsonrpc",
            "--header",
            "Authorization=from-the-flag",
            "--header-env",
            "AUTHORIZATION=A2A_TEST_AUTH",
            "send",
            &url,
            "hi",
        ],
        &[("A2A_TEST_AUTH", "from-the-environment")],
    )
    .await;
    assert_eq!(code(&out), 1, "{}", stdout(&out));
    assert_eq!(seen.lock().unwrap().as_slice(), ["from-the-environment"]);
}

/// `a2a card` carries `--header` (a card behind authentication is reachable)
/// and is bounded by `--timeout`, as discovery is for every other command.
#[tokio::test(flavor = "multi_thread")]
async fn the_card_command_carries_the_header_and_honours_the_timeout() {
    let seen: Seen = Arc::default();
    let card = serde_json::to_vec(&card_for("127.0.0.1:1".parse().unwrap())).expect("card json");
    let addr = serve_fixed(card, Arc::clone(&seen)).await;
    let out = a2a(&[
        "--header",
        "Authorization=Bearer for-the-card",
        "card",
        &format!("http://127.0.0.1:{}", addr.port()),
    ])
    .await;
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    assert_eq!(seen.lock().unwrap().as_slice(), ["Bearer for-the-card"]);

    // A server that accepts and never answers: `--timeout 1` ends it at about a
    // second, not at the 30 s a card fetch had when the flag was ignored.
    let silent = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let silent_url = format!("http://127.0.0.1:{}", silent.local_addr().unwrap().port());
    tokio::spawn(async move {
        let mut held = Vec::new();
        while let Ok((stream, _)) = silent.accept().await {
            held.push(stream);
        }
    });
    let started = std::time::Instant::now();
    let out = a2a(&["--timeout", "1", "card", &silent_url]).await;
    assert_eq!(code(&out), 1, "{}", stdout(&out));
    assert!(
        started.elapsed() < std::time::Duration::from_secs(15),
        "the flag bounded the fetch: {:?}",
        started.elapsed()
    );
}
