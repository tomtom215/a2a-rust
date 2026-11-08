// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! What `OpenAiChatModel` reads from a completion and its endpoint.

use super::*;
use crate::tool_loop::ToolCall;

#[test]
fn an_error_body_is_cut_on_a_character_boundary() {
    assert_eq!(truncated(b"short", 512), "short");
    let long = "é".repeat(400); // 800 bytes
    let cut = truncated(long.as_bytes(), 512);
    assert_eq!(cut.len(), 512);
    assert_eq!(truncated("aé".as_bytes(), 2), "a");
}

/// The body is the provider's text, and it reaches the operator's log through
/// `ModelError`'s `Display`: a newline in it would start a log line the
/// provider wrote, and an escape sequence would reach the operator's terminal
/// raw (found by the live campaign, 2026-11-08). Control characters are
/// written as the escapes Rust gives them, so the body is one printable line,
/// and the bound is on what is shown.
#[test]
fn an_error_body_carries_no_control_character() {
    assert_eq!(
        truncated(b"a\nb\r\x1b[31mc\x07\x00d\te", 512),
        r"a\nb\r\u{1b}[31mc\u{7}\u{0}d\te"
    );
    assert_eq!(
        truncated(
            b"ok\n2026-11-08T00:00:00Z ERROR forged: admin session opened",
            512
        ),
        r"ok\n2026-11-08T00:00:00Z ERROR forged: admin session opened"
    );
    // C1 controls too (U+0085 NEL ends a line on some terminals), but not
    // ordinary text, spaces or letters outside ASCII.
    assert_eq!(truncated("a\u{85}b é ü".as_bytes(), 512), r"a\u{85}b é ü");
    // The bound is on what is shown: 200 escapes of 6 bytes are cut at 512.
    let shown = truncated(&[0x1b; 200], 512);
    assert!(shown.len() <= 512, "{} bytes", shown.len());
    assert!(shown.starts_with(r"\u{1b}\u{1b}"));
    assert!(!shown.chars().any(char::is_control));
}

#[test]
fn content_parts_and_missing_fields_are_read() {
    let turn = parse_completion(
        br#"{"choices":[{"message":{"content":[{"type":"text","text":"a"},{"type":"text","text":"b"}]}}]}"#,
    )
    .expect("parses");
    assert_eq!(turn.text, "ab");
    assert!(turn.usage.is_none());
    let turn = parse_completion(
        br#"{"choices":[{"message":{"content":null,"tool_calls":[{"function":{"name":"t"}}]}}]}"#,
    )
    .expect("parses");
    assert_eq!(turn.tool_calls, vec![ToolCall::new("", "t", "")]);
    assert!(matches!(
        parse_completion(br#"{"choices":[]}"#),
        Err(ModelError::InvalidResponse(_))
    ));
    assert!(matches!(
        parse_completion(b"<html>"),
        Err(ModelError::InvalidResponse(_))
    ));
}

/// The completion's id and the first choice's finish reason, as the
/// provider spelled it, for `gen_ai.response.id` and
/// `gen_ai.response.finish_reasons`; absent, neither is claimed.
#[test]
fn the_response_id_and_finish_reason_are_read() {
    let turn = parse_completion(
        br#"{"id":"chatcmpl-9","choices":[{"message":{"content":"a"},"finish_reason":"tool_calls"}]}"#,
    )
    .expect("parses");
    assert_eq!(turn.response_id.as_deref(), Some("chatcmpl-9"));
    assert_eq!(turn.finish_reason.as_deref(), Some("tool_calls"));
    let bare = parse_completion(br#"{"choices":[{"message":{"content":"a"}}]}"#).expect("parses");
    assert_eq!((bare.response_id, bare.finish_reason), (None, None));
}

/// `server.address` and `server.port`: the endpoint's host, an IPv6
/// address without its brackets, and the scheme's port when none is
/// written.
#[test]
fn the_server_is_the_endpoints_host_and_port() {
    let server = |url: &str| {
        let model = OpenAiChatModel::new(url, "m").expect("valid");
        model.server().map(|(h, p)| (h.to_owned(), p))
    };
    assert_eq!(
        server("http://127.0.0.1:8080/v1"),
        Some(("127.0.0.1".into(), 8080))
    );
    assert_eq!(server("http://[::1]:9/v1"), Some(("::1".into(), 9)));
    assert_eq!(
        server("http://models.internal/v1"),
        Some(("models.internal".into(), 80))
    );
    #[cfg(feature = "tls-rustls")]
    assert_eq!(
        server("https://api.example/v1"),
        Some(("api.example".into(), 443))
    );
}

#[test]
fn a_base_url_that_cannot_work_is_refused_at_construction() {
    assert!(matches!(
        OpenAiChatModel::new("ftp://x/v1", "m"),
        Err(ModelError::Config(_))
    ));
    assert!(matches!(
        OpenAiChatModel::new("not a url", "m"),
        Err(ModelError::Config(_))
    ));
    let ok = OpenAiChatModel::new("http://127.0.0.1:1/v1/", "m").expect("http is fine");
    assert_eq!(
        ok.endpoint.to_string(),
        "http://127.0.0.1:1/v1/chat/completions"
    );
    #[cfg(not(feature = "tls-rustls"))]
    assert!(matches!(
        OpenAiChatModel::new("https://api.example/v1", "m"),
        Err(ModelError::Config(e)) if e.contains("tls-rustls")
    ));
    #[cfg(feature = "tls-rustls")]
    assert!(OpenAiChatModel::new("https://api.example/v1", "m").is_ok());
}

#[test]
fn the_api_key_is_not_in_debug_output() {
    let model = OpenAiChatModel::new("http://127.0.0.1:1/v1", "m")
        .expect("valid")
        .with_api_key("sk-secret");
    let debug = format!("{model:?}");
    assert!(
        !debug.contains("sk-secret"),
        "Debug output carries the API key"
    );
    assert!(
        debug.contains("[redacted]"),
        "Debug output does not mark the key redacted"
    );
}
