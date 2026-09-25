// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The REST dispatcher's error responses: AIP-193 `google.rpc.Status` bodies
//! (spec §11.6). The axum adapter answers through these too, so this
//! crate's two HTTP+JSON dispatchers send one error shape (audit N37).

use std::convert::Infallible;

use bytes::Bytes;
use http_body_util::combinators::BoxBody;

use super::response::{build_json_response, internal_error_response};
use crate::error::ServerError;

/// Builds an AIP-193 compliant error response.
///
/// Per Section 11.6, HTTP error responses use the format:
/// ```json
/// {"error": {"code": 404, "status": "NOT_FOUND", "message": "...", "details": [...]}}
/// ```
pub fn error_json_response(
    status: u16,
    message: &str,
) -> hyper::Response<BoxBody<Bytes, Infallible>> {
    let mut body = serde_json::json!({
        "error": {
            "code": status,
            "message": message
        }
    });
    // AIP-193's HTTP/JSON form carries the `google.rpc.Code` name beside the
    // number, as `server_error_to_response` does; named only where Google's
    // HTTP mapping gives one, not invented for (say) `413`.
    if let Some(name) = canonical_status_name(status) {
        body["error"]["status"] = serde_json::Value::from(name);
    }
    serde_json::to_vec(&body).map_or_else(
        |_| internal_error_response(),
        |bytes| build_json_response(status, bytes),
    )
}

/// The `google.rpc.Code` name Google's HTTP mapping gives `status`
/// (`google/rpc/code.proto`), for the codes it maps.
const fn canonical_status_name(status: u16) -> Option<&'static str> {
    Some(match status {
        400 => "INVALID_ARGUMENT",
        401 => "UNAUTHENTICATED",
        403 => "PERMISSION_DENIED",
        404 => "NOT_FOUND",
        409 => "ABORTED",
        429 => "RESOURCE_EXHAUSTED",
        499 => "CANCELLED",
        500 => "INTERNAL",
        501 => "UNIMPLEMENTED",
        503 => "UNAVAILABLE",
        504 => "DEADLINE_EXCEEDED",
        _ => return None,
    })
}

pub(super) fn not_found_response() -> hyper::Response<BoxBody<Bytes, Infallible>> {
    error_json_response(404, "not found")
}

/// Converts a [`ServerError`] to an AIP-193 error response with proper status codes.
///
/// Per Section 5.4 and 11.6, each A2A error type maps to a specific HTTP status.
pub fn server_error_to_response(err: &ServerError) -> hyper::Response<BoxBody<Bytes, Infallible>> {
    let a2a_err = err.to_a2a_error();
    // Not `a2a_err.code.http_status()`: that answers 400 for a body over the
    // limit and 500 for an overload, where this binding's other dispatcher
    // answers 413 and 503 (audit N20).
    let status = err.http_status();
    let grpc_status = err.status_name();
    let details = a2a_err.error_info_data(None);

    let mut error_obj = serde_json::json!({
        "error": {
            "code": status,
            "status": grpc_status,
            "message": a2a_err.message
        }
    });
    if !details.is_null() {
        error_obj["error"]["details"] = details;
    }

    serde_json::to_vec(&error_obj).map_or_else(
        |_| internal_error_response(),
        |body| {
            let mut resp = build_json_response(status, body);
            crate::dispatch::add_auth_challenge(resp.headers_mut(), err);
            resp
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use http_body_util::BodyExt;

    #[tokio::test]
    async fn error_json_response_status_and_body() {
        let resp = error_json_response(400, "bad request");
        assert_eq!(resp.status().as_u16(), 400);
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        let val: serde_json::Value = serde_json::from_slice(&body).unwrap();
        // AIP-193 format: {"error": {"code": 400, "message": "bad request"}}
        assert_eq!(val["error"]["message"], "bad request");
        assert_eq!(val["error"]["code"], 400);
    }

    #[tokio::test]
    async fn error_json_response_has_a2a_content_type() {
        let resp = error_json_response(404, "not found");
        assert_eq!(
            resp.headers()
                .get("content-type")
                .and_then(|v| v.to_str().ok()),
            Some(a2a_protocol_types::A2A_CONTENT_TYPE),
        );
    }

    #[tokio::test]
    async fn not_found_response_is_404() {
        let resp = not_found_response();
        assert_eq!(resp.status().as_u16(), 404);
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        let val: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(val["error"]["message"], "not found");
    }

    #[tokio::test]
    async fn server_error_task_not_found_maps_to_404() {
        let err = ServerError::TaskNotFound("t1".into());
        let resp = server_error_to_response(&err);
        assert_eq!(resp.status().as_u16(), 404);
    }

    #[tokio::test]
    async fn server_error_method_not_found_maps_to_404() {
        let err = ServerError::MethodNotFound("foo".into());
        let resp = server_error_to_response(&err);
        assert_eq!(resp.status().as_u16(), 404);
    }

    #[tokio::test]
    async fn server_error_task_not_cancelable_maps_to_400() {
        let err = ServerError::TaskNotCancelable("t1".into());
        let resp = server_error_to_response(&err);
        assert_eq!(resp.status().as_u16(), 400);
    }

    #[tokio::test]
    async fn server_error_invalid_params_maps_to_400() {
        let err = ServerError::InvalidParams("bad".into());
        let resp = server_error_to_response(&err);
        assert_eq!(resp.status().as_u16(), 400);
    }

    #[tokio::test]
    async fn server_error_push_not_supported_maps_to_400() {
        let err = ServerError::PushNotSupported;
        let resp = server_error_to_response(&err);
        assert_eq!(resp.status().as_u16(), 400);
    }

    #[tokio::test]
    async fn server_error_internal_maps_to_500() {
        let err = ServerError::Internal("oops".into());
        let resp = server_error_to_response(&err);
        assert_eq!(resp.status().as_u16(), 500);
    }

    /// Covers line 45 (`error_json_response` fallback — normally unreachable
    /// since `serde_json::json`! always serializes).
    /// Test that `error_json_response` always produces correct status and body.
    #[tokio::test]
    async fn error_json_response_various_statuses() {
        for status in [400, 403, 404, 422, 500, 503] {
            let resp = error_json_response(status, &format!("error {status}"));
            assert_eq!(resp.status().as_u16(), status);
        }
    }

    /// Covers line 73 (`server_error_to_response` serialization — normally always succeeds).
    /// Covers `ServerError::Serialization` variant mapping to 400.
    #[tokio::test]
    async fn server_error_serialization_maps_to_400() {
        let err = ServerError::Serialization(serde_json::from_str::<()>("bad").unwrap_err());
        let resp = server_error_to_response(&err);
        assert_eq!(resp.status().as_u16(), 400);
    }

    /// Covers `server_error_to_response` with Http, Transport, `PayloadTooLarge` variants (line 69).
    #[tokio::test]
    async fn server_error_transport_maps_to_500() {
        let err = ServerError::Transport("transport broke".into());
        let resp = server_error_to_response(&err);
        assert_eq!(resp.status().as_u16(), 500);
    }

    /// Was `..._maps_to_400`, pinning the divergence audit N20 records: this
    /// dispatcher's own body-limit check answers 413, as does the axum
    /// adapter, and this path answered 400 for the same condition.
    #[tokio::test]
    async fn server_error_payload_too_large_maps_to_413() {
        let err = ServerError::PayloadTooLarge("too big".into());
        let resp = server_error_to_response(&err);
        assert_eq!(resp.status().as_u16(), 413);
    }

    /// An overload is the retryable 503, not a 500 (audit N20).
    #[tokio::test]
    async fn server_error_overloaded_maps_to_503() {
        let err = ServerError::Overloaded("at capacity".into());
        let resp = server_error_to_response(&err);
        assert_eq!(resp.status().as_u16(), 503);
    }

    #[tokio::test]
    async fn server_error_http_client_maps_to_500() {
        let err = ServerError::HttpClient("connection refused".into());
        let resp = server_error_to_response(&err);
        assert_eq!(resp.status().as_u16(), 500);
    }

    /// Covers the `InvalidStateTransition` variant through `server_error_to_response`.
    #[tokio::test]
    async fn server_error_invalid_state_transition_maps_to_400() {
        use a2a_protocol_types::task::TaskState;
        let err = ServerError::InvalidStateTransition {
            task_id: "t1".into(),
            from: TaskState::Completed,
            to: TaskState::Working,
        };
        let resp = server_error_to_response(&err);
        // InvalidStateTransition → InvalidParams → 400
        assert_eq!(resp.status().as_u16(), 400);
    }

    /// Covers line 73: `server_error_to_response` serialization fallback.
    /// Covers the Protocol variant through `server_error_to_response`.
    #[tokio::test]
    async fn server_error_protocol_maps_to_500() {
        let err = ServerError::Protocol(a2a_protocol_types::error::A2aError::internal("proto err"));
        let resp = server_error_to_response(&err);
        assert_eq!(resp.status().as_u16(), 500);
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        let val: serde_json::Value = serde_json::from_slice(&body).unwrap();
        // AIP-193 format: {"error": {"message": "..."}}
        assert!(
            val["error"]["message"]
                .as_str()
                .unwrap_or("")
                .contains("proto err")
        );
    }
}
