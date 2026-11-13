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

/// The token counts are the provider's own report and only a budget reads
/// them: a count that is not a whole non-negative number is no usage, and does
/// not cost the completion (hostile campaign, wave 2, W2-LIVE-10: a float, a
/// negative number or a string made a good answer "unreadable").
#[test]
fn a_malformed_token_count_is_no_usage_and_not_a_refused_completion() {
    let with = |usage: &str| {
        let body =
            format!(r#"{{"choices":[{{"message":{{"content":"hello"}}}}],"usage":{usage}}}"#);
        parse_completion(body.as_bytes()).expect("the completion is read")
    };
    let counts = |usage: &str| {
        let turn = with(usage);
        assert_eq!(turn.text, "hello", "{usage}");
        turn.usage.map(|u| (u.input_tokens, u.output_tokens))
    };
    // Whole numbers, a float that is one, and the largest `u64`.
    assert_eq!(
        counts(r#"{"prompt_tokens":12,"completion_tokens":5}"#),
        Some((Some(12), Some(5)))
    );
    assert_eq!(
        counts(r#"{"prompt_tokens":12.0,"completion_tokens":0}"#),
        Some((Some(12), Some(0)))
    );
    assert_eq!(
        counts(r#"{"prompt_tokens":18446744073709551615}"#),
        Some((Some(u64::MAX), None))
    );
    // The edge of what a float can say: the largest whole float below 2^64 is
    // itself, and 2^64 is no count (the cast would have made it `u64::MAX`).
    assert_eq!(
        counts(r#"{"prompt_tokens":18446744073709549568.0}"#),
        Some((Some(18_446_744_073_709_549_568), None))
    );
    assert_eq!(
        counts(r#"{"prompt_tokens":18446744073709551616.0}"#),
        Some((None, None))
    );
    assert_eq!(counts(r#"{"prompt_tokens":-0.0}"#), Some((Some(0), None)));
    // Anything else is a missing count, the other count unaffected.
    for bad in [
        "1.5", "-3", "\"12\"", "null", "true", "[1]", "{}", "1e300", "-0.5",
    ] {
        assert_eq!(
            counts(&format!(
                r#"{{"prompt_tokens":{bad},"completion_tokens":7}}"#
            )),
            Some((None, Some(7))),
            "prompt_tokens: {bad}"
        );
    }
    // Not an object at all is still refused: that is not a usage.
    assert!(parse_completion(br#"{"choices":[{"message":{"content":"x"}}],"usage":5}"#).is_err());
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

/// A local endpoint that answers one streamed turn and returns the request
/// it was sent, headers lowercased.
async fn one_streamed_turn(model: impl FnOnce(&str) -> OpenAiChatModel) -> String {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}/v1", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut buf = vec![0_u8; 16 * 1024];
        let n = stream.read(&mut buf).await.unwrap();
        let body = "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"hi\"},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n";
        let resp = format!(
            "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncontent-length: {}\r\n\r\n{body}",
            body.len()
        );
        stream.write_all(resp.as_bytes()).await.unwrap();
        String::from_utf8_lossy(&buf[..n]).to_ascii_lowercase()
    });
    let model = model(&base);
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let messages = vec![crate::tool_loop::ChatMessage::User("go".to_owned())];
    let turn = model
        .complete_streaming(
            crate::tool_loop::ChatRequest::new("", &messages, &[]),
            &crate::tool_loop::TextSink::new(1, tx),
        )
        .await
        .unwrap();
    assert_eq!(turn.text, "hi");
    server.await.unwrap()
}

/// By default a streamed turn asks for its connection to be closed, for
/// llama-server; `with_stream_connection_reuse` leaves it to the pool
/// (adversarial review N16, 2026-11-13).
#[tokio::test]
async fn a_streamed_turn_closes_its_connection_unless_reuse_is_asked_for() {
    let sent = one_streamed_turn(|base| OpenAiChatModel::new(base, "m").unwrap()).await;
    assert!(sent.contains("connection: close"), "{sent}");
    let sent = one_streamed_turn(|base| {
        OpenAiChatModel::new(base, "m")
            .unwrap()
            .with_stream_connection_reuse()
    })
    .await;
    assert!(!sent.contains("connection: close"), "{sent}");
}
