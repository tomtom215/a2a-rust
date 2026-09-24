// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Entry points for the fuzz targets in `fuzz/`, compiled only for them.
//!
//! The client parses whatever a remote agent sends, and four of those
//! parsers had no fuzz target: an error met mid-stream, an AIP-193 error
//! body, a `Retry-After` header, and the id a WebSocket frame is routed by.
//! Three are private, so a fuzz target — an external crate — cannot reach
//! them; this module forwards to each unchanged. It exists under
//! `cfg(fuzzing)`, which `cargo fuzz` sets, and under `cfg(test)`, so the
//! forwarding itself is tested; it is in no build an adopter makes and in no
//! published API. The server's `fuzzing` module is the same arrangement.

/// A stream data frame that carries an error rather than an event.
#[must_use]
pub fn stream_error_frame(data: &str) -> Option<a2a_protocol_types::A2aError> {
    crate::transport::rest::error_frame::decode_stream_error_frame(data)
}

/// An HTTP+JSON error body in the AIP-193 shape.
#[must_use]
pub fn aip193_error(body: &[u8]) -> Option<a2a_protocol_types::A2aError> {
    crate::transport::rest::request::parse_aip193_error(body)
}

/// A `Retry-After` header value, as the retry policy reads it.
#[must_use]
pub fn retry_after(value: &[u8]) -> Option<std::time::Duration> {
    let value = hyper::header::HeaderValue::from_bytes(value).ok()?;
    let mut headers = hyper::HeaderMap::new();
    headers.insert(hyper::header::RETRY_AFTER, value);
    crate::error::parse_retry_after(&headers)
}

/// The JSON-RPC id a WebSocket text frame is routed by.
#[cfg(feature = "websocket")]
#[must_use]
pub fn websocket_frame_id(text: &str) -> Option<String> {
    crate::transport::websocket::extract_jsonrpc_id(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forwards_the_stream_error_frame() {
        let err = stream_error_frame(r#"{"code":-32001,"message":"gone"}"#).expect("error frame");
        assert_eq!(err.message, "gone");
        assert!(stream_error_frame(r#"{"statusUpdate":{}}"#).is_none());
    }

    #[test]
    fn forwards_the_aip193_body() {
        let body = br#"{"error":{"message":"m","details":[{"@type":"type.googleapis.com/google.rpc.ErrorInfo","reason":"TASK_NOT_FOUND"}]}}"#;
        assert_eq!(
            aip193_error(body).expect("aip-193").code,
            a2a_protocol_types::ErrorCode::TaskNotFound
        );
    }

    #[test]
    fn forwards_the_retry_after_header() {
        assert_eq!(retry_after(b"7"), Some(std::time::Duration::from_secs(7)));
        assert_eq!(
            retry_after(b"999999"),
            Some(std::time::Duration::from_secs(3600))
        );
        assert_eq!(retry_after(b"Wed, 21 Oct 2015 07:28:00 GMT"), None);
    }

    #[cfg(feature = "websocket")]
    #[test]
    fn forwards_the_websocket_frame_id() {
        assert_eq!(websocket_frame_id(r#"{"id":"a"}"#).as_deref(), Some("a"));
        assert_eq!(websocket_frame_id(r#"{"id":7}"#).as_deref(), Some("7"));
    }
}
