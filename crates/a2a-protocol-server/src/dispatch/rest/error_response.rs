// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The REST dispatcher's error responses: AIP-193 `google.rpc.Status` bodies
//! (spec §11.6). The axum adapter answers through these too, so this
//! crate's two HTTP+JSON dispatchers send one error shape (audit N37).

use std::convert::Infallible;

use bytes::Bytes;
use http_body_util::combinators::BoxBody;

use super::params::Unreadable;
use super::response::{build_json_response, internal_error_response};
use crate::error::ServerError;

/// A 400 whose AIP-193 body carries a `google.rpc.BadRequest` detail with
/// one field violation (spec §11.6: implementations SHOULD use well-known
/// types such as `BadRequest` for validation errors). `field` is the path of
/// the offending field, `""` when the request itself is the problem.
/// The same body as [`server_error_to_response`] of the
/// [`ServerError::InvalidField`] it is, `INVALID_PARAMS` `ErrorInfo` first.
pub fn bad_request_response(
    field: &str,
    description: &str,
) -> hyper::Response<BoxBody<Bytes, Infallible>> {
    server_error_to_response(&ServerError::InvalidField {
        field: field.to_owned(),
        message: description.to_owned(),
    })
}

/// The [`Unreadable::violation`] for a value that did not deserialize: the
/// field `serde_path_to_error` names (with a missing key appended, see
/// [`violation_field`](crate::streaming::sse::violation_field)) and the
/// error's text.
fn violation<E: std::fmt::Display>(path: &serde_path_to_error::Path, inner: &E) -> Unreadable {
    let description = inner.to_string();
    let field = crate::streaming::sse::violation_field(&path.to_string(), &description);
    Unreadable::violation(&field, &description)
}

/// Deserializes request parameters from a parsed body. A value of the wrong
/// shape is [`Unreadable::violation`], a [`bad_request_response`] naming the
/// field once the caller is admitted.
#[allow(clippy::result_large_err)] // the error holds the response, as in `RestDispatcher::read_params`
pub fn params_from_value<T: serde::de::DeserializeOwned>(
    value: serde_json::Value,
) -> Result<T, Unreadable> {
    serde_path_to_error::deserialize(value).map_err(|e| violation(e.path(), e.inner()))
}

/// Deserializes request parameters from raw body bytes. Malformed JSON is
/// [`Unreadable::not_json`]; well-formed JSON of the wrong shape is read
/// again as a value through [`params_from_value`], on the failure path only,
/// to name the field. As a value, as `RestDispatcher` reads it: re-reading the
/// bytes put `at line 1 column N` into the message, so the two HTTP+JSON
/// dispatchers answered one bad body with two messages.
#[cfg(feature = "axum")]
#[allow(clippy::result_large_err)] // the error holds the response, as in `RestDispatcher::read_params`
pub fn params_from_slice<T: serde::de::DeserializeOwned>(body: &[u8]) -> Result<T, Unreadable> {
    match serde_json::from_slice(body) {
        Ok(v) => Ok(v),
        Err(e) if e.classify() != serde_json::error::Category::Data => {
            Err(Unreadable::not_json(&e))
        }
        Err(e) => match serde_json::from_slice::<serde_json::Value>(body) {
            Ok(value) => {
                params_from_value::<T>(value).and(Err(Unreadable::violation("", &e.to_string())))
            }
            Err(e) => Err(Unreadable::not_json(&e)),
        },
    }
}

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

/// The `google.rpc.ErrorInfo` reason of a JSON-RPC-standard error on this
/// binding: the reasons a2a-go (v2.7.0 `a2a/errors.go`) and a2a-python
/// (`a2a-sdk` 1.2.3 `a2a/utils/errors.py`) send and map by, in the
/// `a2a-protocol.org` domain. v1.0.1 §11.6 requires an `ErrorInfo` only for the
/// A2A-specific errors (§3.3.2), and names no reason for these; without one,
/// a2a-go read a `400 INVALID_ARGUMENT` as an internal error and a2a-python
/// as a bare HTTP 400 (ADR 0049). a2a-python reads no `PARSE_ERROR`; a2a-go
/// does.
const fn standard_reason(code: a2a_protocol_types::ErrorCode) -> Option<&'static str> {
    use a2a_protocol_types::ErrorCode;
    match code {
        ErrorCode::ParseError => Some("PARSE_ERROR"),
        ErrorCode::InvalidRequest => Some("INVALID_REQUEST"),
        ErrorCode::MethodNotFound => Some("METHOD_NOT_FOUND"),
        ErrorCode::InvalidParams => Some("INVALID_PARAMS"),
        ErrorCode::InternalError => Some("INTERNAL_ERROR"),
        _ => None,
    }
}

/// An HTTP+JSON error's details: [`error_details`](crate::streaming::sse::error_details),
/// led by the `ErrorInfo` of a JSON-RPC-standard error ([`standard_reason`]).
/// First, because a2a-python reads only the first `ErrorInfo` in `details`:
/// a halt's `HALTED` and a validation error's `BadRequest` follow it. A
/// credential refusal gets none: it answers `401`/`403` whatever its code
/// (`InvalidRequest`), and both SDKs would read a reason over that status.
/// The JSON-RPC and gRPC bindings are unchanged.
pub fn rest_error_details(err: &a2a_protocol_types::A2aError) -> Vec<serde_json::Value> {
    let mut details = crate::streaming::sse::error_details(err);
    if err.auth_rejection().is_some() {
        return details;
    }
    if let Some(reason) = standard_reason(err.code) {
        details.insert(
            0,
            serde_json::json!({
                "@type": "type.googleapis.com/google.rpc.ErrorInfo",
                "reason": reason,
                "domain": a2a_protocol_types::error::A2A_ERROR_DOMAIN,
            }),
        );
    }
    details
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
    let details = rest_error_details(&a2a_err);
    let mut error_obj = serde_json::json!({
        "error": {
            "code": status,
            "status": grpc_status,
            "message": a2a_err.message
        }
    });
    // Empty only for a credential refusal with nothing to say.
    if !details.is_empty() {
        error_obj["error"]["details"] = serde_json::Value::Array(details);
    }

    serde_json::to_vec(&error_obj).map_or_else(
        |_| internal_error_response(),
        |body| {
            let mut resp = build_json_response(status, body);
            crate::dispatch::add_auth_challenge(resp.headers_mut(), err);
            if let Some(delay) = err.retry_after() {
                // Whole seconds: `A2aError::rate_limited` rounds up to them.
                resp.headers_mut().insert(
                    hyper::header::RETRY_AFTER,
                    hyper::header::HeaderValue::from(delay.as_secs()),
                );
            }
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

    /// Every code `canonical_status_name` maps reaches the body under
    /// `error.status`, and a code Google's mapping does not name gets none.
    #[tokio::test]
    async fn error_json_response_names_each_mapped_code() {
        let table = [
            (400, "INVALID_ARGUMENT"),
            (401, "UNAUTHENTICATED"),
            (403, "PERMISSION_DENIED"),
            (404, "NOT_FOUND"),
            (409, "ABORTED"),
            (429, "RESOURCE_EXHAUSTED"),
            (499, "CANCELLED"),
            (500, "INTERNAL"),
            (501, "UNIMPLEMENTED"),
            (503, "UNAVAILABLE"),
            (504, "DEADLINE_EXCEEDED"),
        ];
        for (code, name) in table {
            let body = error_json_response(code, "x")
                .into_body()
                .collect()
                .await
                .unwrap()
                .to_bytes();
            let val: serde_json::Value = serde_json::from_slice(&body).unwrap();
            assert_eq!(val["error"]["status"], name, "status {code}");
        }
        let body = error_json_response(413, "x")
            .into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes();
        let val: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(
            val["error"].get("status").is_none(),
            "413 has no name: {val}"
        );
    }

    #[tokio::test]
    async fn error_json_response_has_a2a_content_type() {
        let resp = error_json_response(404, "not found");
        assert_eq!(
            resp.headers()
                .get("content-type")
                .and_then(|v| v.to_str().ok()),
            Some(a2a_protocol_types::JSON_CONTENT_TYPE),
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

    /// Each JSON-RPC-standard code leads its details with the reason a2a-go
    /// and a2a-python map by; an A2A code and a credential refusal do not get
    /// one (ADR 0049). The parity test drives the reachable ones through both
    /// dispatchers; parse error, method not found and invalid request are
    /// answered by this builder alone on this binding.
    #[test]
    fn each_standard_code_leads_with_its_reason() {
        use a2a_protocol_types::{A2aError, ErrorCode};
        let first = |e: &A2aError| rest_error_details(e).first().map(|d| d["reason"].clone());
        for (code, reason) in [
            (ErrorCode::ParseError, "PARSE_ERROR"),
            (ErrorCode::InvalidRequest, "INVALID_REQUEST"),
            (ErrorCode::MethodNotFound, "METHOD_NOT_FOUND"),
            (ErrorCode::InvalidParams, "INVALID_PARAMS"),
            (ErrorCode::InternalError, "INTERNAL_ERROR"),
        ] {
            let details = rest_error_details(&A2aError::new(code, "x"));
            assert_eq!(details.len(), 1, "{code:?}");
            assert_eq!(details[0]["reason"], reason);
            assert_eq!(details[0]["domain"], "a2a-protocol.org");
        }
        assert_eq!(
            first(&A2aError::task_not_found("t")),
            Some("TASK_NOT_FOUND".into())
        );
        assert_eq!(rest_error_details(&A2aError::task_not_found("t")).len(), 1);
        let refused = A2aError::unauthenticated("no credential", "Bearer");
        assert!(
            rest_error_details(&refused)
                .iter()
                .all(|d| d["domain"] != "a2a-protocol.org")
        );
    }
}
