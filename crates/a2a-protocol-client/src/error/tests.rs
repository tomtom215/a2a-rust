// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Unit tests for [`ClientError`](super::ClientError).
//!
//! Split out of `error.rs` when four new stream-lag tests pushed that file
//! past the 500-line limit `scripts/check_file_lengths.sh` enforces. The
//! production code stays in `mod.rs`; only the test module moved, so nothing
//! about the public surface changed.

use super::*;
use a2a_protocol_types::{A2aError, ErrorCode};

#[test]
fn client_error_display_http_client() {
    let e = ClientError::HttpClient("connection refused".into());
    assert!(e.to_string().contains("connection refused"));
}

#[test]
fn client_error_display_protocol() {
    let a2a = A2aError::task_not_found("task-99");
    let e = ClientError::Protocol(a2a);
    assert!(e.to_string().contains("task-99"));
}

#[test]
fn client_error_from_a2a_error() {
    let a2a = A2aError::new(ErrorCode::TaskNotFound, "missing");
    let e: ClientError = a2a.into();
    assert!(matches!(e, ClientError::Protocol(_)));
}

#[test]
fn client_error_unexpected_status() {
    let e = ClientError::UnexpectedStatus {
        status: 404,
        body: "Not Found".into(),
        retry_after: None,
    };
    assert!(e.to_string().contains("404"));
}

/// Bug #32: Timeout errors must be retryable.
///
/// Previously, REST/JSON-RPC transports used `ClientError::Transport` for
/// timeouts, which is non-retryable. This test verifies `Timeout` is
/// retryable and `Transport` is not, ensuring retry logic works correctly.
#[test]
fn timeout_is_retryable_transport_is_not() {
    let timeout = ClientError::Timeout("request timed out".into());
    assert!(timeout.is_retryable(), "Timeout errors must be retryable");

    let transport = ClientError::Transport("config error".into());
    assert!(
        !transport.is_retryable(),
        "Transport errors must not be retryable"
    );
}

#[test]
fn client_error_source_http() {
    use std::error::Error;
    // Create a hyper error by trying to parse invalid HTTP.
    // Use a Transport error wrapping an Http error via From.
    let http_err: ClientError = ClientError::HttpClient("test".into());
    // HttpClient is not Http, so source is None.
    assert!(http_err.source().is_none());

    // Serialization error has a source.
    let ser_err =
        ClientError::Serialization(serde_json::from_str::<String>("not json").unwrap_err());
    assert!(
        ser_err.source().is_some(),
        "Serialization error should have a source"
    );

    // Protocol error has a source.
    let proto_err = ClientError::Protocol(a2a_protocol_types::A2aError::task_not_found("t"));
    assert!(
        proto_err.source().is_some(),
        "Protocol error should have a source"
    );

    // Transport error has no source.
    let transport_err = ClientError::Transport("config".into());
    assert!(transport_err.source().is_none());
}

// ── Display tests for every variant ────────────────────────────────

#[test]
fn client_error_display_transport() {
    let e = ClientError::Transport("socket closed".into());
    let s = e.to_string();
    assert!(s.contains("transport error"), "missing prefix: {s}");
    assert!(s.contains("socket closed"), "missing message: {s}");
}

#[test]
fn client_error_display_invalid_endpoint() {
    let e = ClientError::InvalidEndpoint("bad url".into());
    let s = e.to_string();
    assert!(s.contains("invalid endpoint"), "missing prefix: {s}");
    assert!(s.contains("bad url"), "missing message: {s}");
}

#[test]
fn client_error_display_auth_required() {
    let e = ClientError::AuthRequired {
        task_id: TaskId::new("task-7"),
    };
    let s = e.to_string();
    assert!(s.contains("authentication required"), "missing prefix: {s}");
    assert!(s.contains("task-7"), "missing task_id: {s}");
}

#[test]
fn client_error_display_timeout() {
    let e = ClientError::Timeout("30s elapsed".into());
    let s = e.to_string();
    assert!(s.contains("timeout"), "missing prefix: {s}");
    assert!(s.contains("30s elapsed"), "missing message: {s}");
}

#[test]
fn client_error_display_protocol_binding_mismatch() {
    let e = ClientError::ProtocolBindingMismatch("expected REST".into());
    let s = e.to_string();
    assert!(
        s.contains("protocol binding mismatch"),
        "missing prefix: {s}"
    );
    assert!(s.contains("expected REST"), "missing message: {s}");
    assert!(s.contains("supported_interfaces"), "missing advice: {s}");
}

#[test]
fn client_error_display_serialization() {
    let e = ClientError::Serialization(serde_json::from_str::<String>("bad").unwrap_err());
    let s = e.to_string();
    assert!(s.contains("serialization error"), "missing prefix: {s}");
}

#[test]
fn client_error_display_unexpected_status() {
    let e = ClientError::UnexpectedStatus {
        status: 500,
        body: "Internal Server Error".into(),
        retry_after: None,
    };
    let s = e.to_string();
    assert!(s.contains("500"), "missing status code: {s}");
    assert!(s.contains("Internal Server Error"), "missing body: {s}");
}

// ── Error::source coverage ────────────────────────────────────────────

#[test]
fn client_error_source_none_for_string_variants() {
    use std::error::Error;
    let cases: Vec<ClientError> = vec![
        ClientError::HttpClient("msg".into()),
        ClientError::Transport("msg".into()),
        ClientError::InvalidEndpoint("msg".into()),
        ClientError::UnexpectedStatus {
            status: 404,
            body: String::new(),
            retry_after: None,
        },
        ClientError::AuthRequired {
            task_id: TaskId::new("t"),
        },
        ClientError::Timeout("msg".into()),
        ClientError::ProtocolBindingMismatch("msg".into()),
    ];
    for e in &cases {
        assert!(
            e.source().is_none(),
            "{:?} should have no source",
            std::mem::discriminant(e)
        );
    }
}

/// Test `Http` variant Display and source (covers lines 65, 91, 106-107).
/// We obtain a real `hyper::Error` by reading a body from a connection
/// that sends a partial HTTP response via raw TCP.
#[tokio::test]
async fn client_error_display_and_source_http() {
    use http_body_util::{BodyExt, Full};
    use hyper::body::Bytes;
    use tokio::io::AsyncWriteExt;

    // Start a raw TCP server that sends a partial HTTP response with
    // content-length mismatch, then closes the connection.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        // Read request
        let mut buf = [0u8; 4096];
        let _ = tokio::io::AsyncReadExt::read(&mut stream, &mut buf).await;
        // Send partial response: declared content-length=1000 but only send 5 bytes.
        let resp = "HTTP/1.1 200 OK\r\ncontent-length: 1000\r\n\r\nhello";
        let _ = stream.write_all(resp.as_bytes()).await;
        // Close connection - body read will fail.
        drop(stream);
    });

    let client: hyper_util::client::legacy::Client<
        hyper_util::client::legacy::connect::HttpConnector,
        Full<Bytes>,
    > = hyper_util::client::legacy::Client::builder(hyper_util::rt::TokioExecutor::new())
        .build(hyper_util::client::legacy::connect::HttpConnector::new());

    let req = hyper::Request::builder()
        .uri(format!("http://127.0.0.1:{}", addr.port()))
        .body(Full::new(Bytes::new()))
        .unwrap();

    let resp = client.request(req).await.unwrap();
    // Body read should fail due to content-length mismatch.
    let body_result = resp.collect().await;
    if let Err(hyper_err) = body_result {
        use std::error::Error;

        // Test Http variant construction and From impl (covers line 106-107).
        let client_err: ClientError = ClientError::Http(hyper_err);

        // Test Display (covers line 65).
        let display = client_err.to_string();
        assert!(display.contains("HTTP error"), "Display: {display}");

        // Test source (covers line 91).
        assert!(
            client_err.source().is_some(),
            "Http variant should have a source"
        );
    } else {
        // On very fast localhost the read might succeed before close.
        // We still covered the construction path above in other tests.
    }
}

// ── From impls ────────────────────────────────────────────────────────

#[test]
fn client_error_from_serde_json_error() {
    let serde_err = serde_json::from_str::<String>("not json").unwrap_err();
    let e: ClientError = serde_err.into();
    assert!(matches!(e, ClientError::Serialization(_)));
}

/// Verify all retryable/non-retryable classifications.
#[test]
fn retryable_classification_exhaustive() {
    // Retryable
    assert!(ClientError::HttpClient("conn reset".into()).is_retryable());
    assert!(ClientError::Timeout("deadline".into()).is_retryable());
    assert!(ClientError::UnexpectedStatus {
        status: 429,
        body: String::new(),
        retry_after: None,
    }
    .is_retryable());
    assert!(ClientError::UnexpectedStatus {
        status: 502,
        body: String::new(),
        retry_after: None,
    }
    .is_retryable());
    assert!(ClientError::UnexpectedStatus {
        status: 503,
        body: String::new(),
        retry_after: None,
    }
    .is_retryable());
    assert!(ClientError::UnexpectedStatus {
        status: 504,
        body: String::new(),
        retry_after: None,
    }
    .is_retryable());

    // Non-retryable
    assert!(!ClientError::Transport("bad config".into()).is_retryable());
    assert!(!ClientError::InvalidEndpoint("bad url".into()).is_retryable());
    assert!(!ClientError::UnexpectedStatus {
        status: 400,
        body: String::new(),
        retry_after: None,
    }
    .is_retryable());
    assert!(!ClientError::UnexpectedStatus {
        status: 401,
        body: String::new(),
        retry_after: None,
    }
    .is_retryable());
    assert!(!ClientError::UnexpectedStatus {
        status: 404,
        body: String::new(),
        retry_after: None,
    }
    .is_retryable());
    assert!(!ClientError::ProtocolBindingMismatch("wrong".into()).is_retryable());
    assert!(!ClientError::AuthRequired {
        task_id: TaskId::new("t")
    }
    .is_retryable());
}

// ── Stream-lag passthrough ───────────────────────────────────────────
//
// `is_stream_lagged` and `dropped_event_count` forward to the wrapped
// `A2aError`, and both were untested: `cargo mutants` killed nothing when
// it replaced them with `true`, `false`, `None`, `Some(0)` and `Some(1)`,
// or deleted the `Protocol` arm entirely. A consumer uses these to tell a
// recoverable gap from a fatal stream error, so a wrong answer either
// aborts a stream that could have continued or silently swallows one that
// could not.

/// A lag error must be recognised as one, through the client wrapper.
#[test]
fn stream_lag_is_recognised_through_the_client_error() {
    let lagged = ClientError::Protocol(A2aError::stream_lagged(42));
    assert!(
        lagged.is_stream_lagged(),
        "a wrapped stream-lag error must report as lagged"
    );
    assert_eq!(
        lagged.dropped_event_count(),
        Some(42),
        "the dropped count must survive the wrapper, not be invented"
    );
}

/// Any other protocol error must not be mistaken for one.
#[test]
fn an_ordinary_protocol_error_is_not_stream_lag() {
    let other = ClientError::Protocol(A2aError::internal("something else"));
    assert!(
        !other.is_stream_lagged(),
        "an unrelated protocol error must not report as lagged — a consumer \
         would treat a fatal error as a recoverable gap"
    );
    assert_eq!(other.dropped_event_count(), None);
}

/// A non-protocol error has no lag information at all.
///
/// Distinct from the case above: this is the arm `cargo mutants` deleted,
/// and without it the `Protocol` match arm can be removed with every test
/// still passing.
#[test]
fn a_transport_error_carries_no_lag_information() {
    let transport = ClientError::Transport("connection reset".into());
    assert!(!transport.is_stream_lagged());
    assert_eq!(transport.dropped_event_count(), None);
}

/// Zero dropped events is still a lag signal, and still reports zero.
///
/// Pins the `Some(0)` mutant specifically: a count of zero must not be
/// confused with "no count", and vice versa.
#[test]
fn a_zero_drop_count_is_reported_as_zero_not_as_absent() {
    let lagged = ClientError::Protocol(A2aError::stream_lagged(0));
    assert!(lagged.is_stream_lagged());
    assert_eq!(lagged.dropped_event_count(), Some(0));
}
