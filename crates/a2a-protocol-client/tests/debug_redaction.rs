// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! A debug-print of a client type that carries credentials names what it
//! holds and prints no secret (audit register R17).
//!
//! `tracing::debug!("{req:?}")` in an interceptor is the natural thing to
//! write, and the request it prints carries the `Authorization` header an
//! auth interceptor added and, for a push-config call, the webhook's token
//! and credentials. Each assertion that a secret is absent is paired with
//! one that the field is there, so empty output cannot pass.

use std::collections::HashMap;

use a2a_protocol_client::discovery::CardFetchOptions;
use a2a_protocol_client::interceptor::{ClientRequest, ClientResponse};
use serde_json::json;

/// Header names, inserted out of order: the output lists them sorted, so a
/// debug-print is the same line every time.
fn headers() -> HashMap<String, String> {
    [
        ("x-company-key", "key-s3cret"),
        ("authorization", "Bearer hdr-s3cret"),
        ("cookie", "sid=ck-s3cret"),
        ("a2a-extensions", "urn:ext-s3cret"),
        ("x-tenant", "tenant-s3cret"),
        ("b-trace", "trace-s3cret"),
    ]
    .into_iter()
    .map(|(n, v)| (n.to_owned(), v.to_owned()))
    .collect()
}

const SORTED_NAMES: &str =
    r#"["a2a-extensions", "authorization", "b-trace", "cookie", "x-company-key", "x-tenant"]"#;

/// A push config as `CreateTaskPushNotificationConfig` sends it and the
/// agent answers it.
fn push_config() -> serde_json::Value {
    json!({
        "url": "https://hook.example/cb",
        "token": "push-s3cret",
        "authentication": { "scheme": "Bearer", "credentials": "cred-s3cret" },
        "taskId": "task-1"
    })
}

fn assert_no_secret(shown: &str) {
    assert!(!shown.contains("s3cret"), "a secret was printed: {shown}");
}

const PUSH_CONFIG_KEYS: &str = r#"["authentication", "taskId", "token", "url"]"#;

#[test]
fn a_client_request_prints_header_names_and_param_keys_only() {
    let mut req = ClientRequest::new("CreateTaskPushNotificationConfig", push_config());
    req.extra_headers = headers();
    req.query_params = vec![("api_key".into(), "query-s3cret".into())];
    let shown = format!("{req:?}");
    assert_no_secret(&shown);
    assert_eq!(
        shown,
        format!(
            "ClientRequest {{ method: \"CreateTaskPushNotificationConfig\", \
             param_keys: {PUSH_CONFIG_KEYS}, extra_header_names: {SORTED_NAMES}, \
             query_param_names: [\"api_key\"] }}"
        )
    );
}

#[test]
fn a_client_response_prints_result_keys_only() {
    let resp = ClientResponse {
        method: "GetTaskPushNotificationConfig".into(),
        result: push_config(),
        status_code: 200,
    };
    let shown = format!("{resp:?}");
    assert_no_secret(&shown);
    assert_eq!(
        shown,
        format!(
            "ClientResponse {{ method: \"GetTaskPushNotificationConfig\", \
             result_keys: {PUSH_CONFIG_KEYS}, status_code: 200 }}"
        )
    );
}

/// `params` is `null` once it has moved to the transport, which is what an
/// `on_error` hook sees; a value that is not an object has no keys.
#[test]
fn a_request_whose_params_are_not_an_object_prints_no_keys() {
    for params in [serde_json::Value::Null, json!("bare-s3cret")] {
        let shown = format!("{:?}", ClientRequest::new("GetTask", params));
        assert_no_secret(&shown);
        assert_eq!(
            shown,
            r#"ClientRequest { method: "GetTask", param_keys: [], extra_header_names: [], query_param_names: [] }"#
        );
    }
}

#[test]
fn card_fetch_options_print_header_names_only() {
    let options = CardFetchOptions::default()
        .with_headers(headers())
        .with_timeout(std::time::Duration::from_secs(5));
    let shown = format!("{options:?}");
    assert_no_secret(&shown);
    // The keys a card is verified against are public; they print as before.
    let keys = if cfg!(feature = "signing") {
        ", signature_keys: None, require_signature: true"
    } else {
        ""
    };
    // Whether a TLS configuration was given, never what is in it.
    let tls = if cfg!(feature = "tls-rustls") {
        ", tls_config: false"
    } else {
        ""
    };
    assert_eq!(
        shown,
        format!("CardFetchOptions {{ header_names: {SORTED_NAMES}, timeout: 5s{keys}{tls} }}")
    );
}

/// A TLS configuration may hold a client certificate's private key; the
/// options say only that one is set.
#[cfg(feature = "tls-rustls")]
#[test]
fn card_fetch_options_say_a_tls_configuration_is_set_and_nothing_of_it() {
    let tls = a2a_protocol_client::tls::default_tls_config();
    let shown = format!("{:?}", CardFetchOptions::default().with_tls_config(tls));
    assert!(shown.ends_with("tls_config: true }"), "{shown}");
    assert!(
        !shown.contains("ClientConfig") && !shown.contains("RootCertStore"),
        "{shown}"
    );
}

#[cfg(feature = "websocket")]
#[test]
fn websocket_config_prints_header_names_only() {
    use a2a_protocol_client::transport::websocket::WebSocketTransportConfig;
    use std::time::Duration;

    let config = WebSocketTransportConfig::default()
        .with_request_timeout(Duration::from_secs(3))
        .with_extra_headers(headers())
        .with_extensions(["urn:a2a:ext:x"])
        .with_connect_timeout(Duration::from_secs(4))
        .with_max_message_size(1024)
        .with_max_pending_requests(7);
    let shown = format!("{config:?}");
    assert_no_secret(&shown);
    let tls = if cfg!(feature = "tls-rustls") {
        ", tls: None"
    } else {
        ""
    };
    assert_eq!(
        shown,
        format!(
            "WebSocketTransportConfig {{ request_timeout: 3s, send_timeout: 600s, \
             extra_header_names: {SORTED_NAMES}, extensions: [\"urn:a2a:ext:x\"], \
             connect_timeout: 4s, max_message_size: 1024, max_pending_requests: 7{tls} }}"
        )
    );
}
