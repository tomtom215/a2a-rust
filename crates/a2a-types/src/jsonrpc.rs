// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! JSON-RPC 2.0 envelope types.
//!
//! A2A 0.3.0 uses JSON-RPC 2.0 as its wire protocol. This module provides the
//! request/response envelope types. Protocol-method-specific parameter and
//! result types live in [`crate::params`] and the individual domain modules.
//!
//! # Key types
//!
//! - [`JsonRpcRequest`] — outbound method call.
//! - [`JsonRpcResponse`] — inbound response (success **or** error, untagged union).
//! - [`JsonRpcError`] — structured error object carried in error responses.
//! - [`JsonRpcVersion`] — newtype that always serializes/deserializes as `"2.0"`.

use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

// ── JsonRpcVersion ────────────────────────────────────────────────────────────

/// The JSON-RPC protocol version marker.
///
/// Always serializes as the string `"2.0"`. Deserialization rejects any value
/// other than `"2.0"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JsonRpcVersion;

impl Default for JsonRpcVersion {
    fn default() -> Self {
        Self
    }
}

impl fmt::Display for JsonRpcVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("2.0")
    }
}

impl Serialize for JsonRpcVersion {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str("2.0")
    }
}

impl<'de> Deserialize<'de> for JsonRpcVersion {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        if s == "2.0" {
            Ok(Self)
        } else {
            Err(serde::de::Error::custom(format!(
                "expected JSON-RPC version \"2.0\", got \"{s}\""
            )))
        }
    }
}

// ── JsonRpcId ─────────────────────────────────────────────────────────────────

/// A JSON-RPC 2.0 request/response identifier.
///
/// Per spec, valid values are a string, a number, or `null`. When the field is
/// absent entirely (notifications), represent as `None`.
pub type JsonRpcId = Option<serde_json::Value>;

// ── JsonRpcRequest ────────────────────────────────────────────────────────────

/// A JSON-RPC 2.0 request object.
///
/// When `id` is `None`, the request is a *notification* and no response is
/// expected.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcRequest {
    /// Protocol version — always `"2.0"`.
    pub jsonrpc: JsonRpcVersion,

    /// Request identifier; `None` for notifications.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: JsonRpcId,

    /// A2A method name (e.g. `"message/send"`).
    pub method: String,

    /// Method-specific parameters.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<serde_json::Value>,
}

impl JsonRpcRequest {
    /// Creates a new request with the given `id` and `method`.
    #[must_use]
    pub fn new(id: serde_json::Value, method: impl Into<String>) -> Self {
        Self {
            jsonrpc: JsonRpcVersion,
            id: Some(id),
            method: method.into(),
            params: None,
        }
    }

    /// Creates a new request with `params`.
    #[must_use]
    pub fn with_params(
        id: serde_json::Value,
        method: impl Into<String>,
        params: serde_json::Value,
    ) -> Self {
        Self {
            jsonrpc: JsonRpcVersion,
            id: Some(id),
            method: method.into(),
            params: Some(params),
        }
    }

    /// Creates a notification (no `id`, no response expected).
    #[must_use]
    pub fn notification(method: impl Into<String>, params: Option<serde_json::Value>) -> Self {
        Self {
            jsonrpc: JsonRpcVersion,
            id: None,
            method: method.into(),
            params,
        }
    }
}

// ── JsonRpcResponse ───────────────────────────────────────────────────────────

/// A JSON-RPC 2.0 response: either a success with a `result` or an error with
/// an `error` object.
///
/// The `untagged` representation tries `Success` first; if `result` is absent
/// it falls back to `Error`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum JsonRpcResponse<T> {
    /// Successful response carrying a typed result.
    Success(JsonRpcSuccessResponse<T>),
    /// Error response carrying a structured error object.
    Error(JsonRpcErrorResponse),
}

// ── JsonRpcSuccessResponse ────────────────────────────────────────────────────

/// A successful JSON-RPC 2.0 response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcSuccessResponse<T> {
    /// Protocol version — always `"2.0"`.
    pub jsonrpc: JsonRpcVersion,

    /// Matches the `id` of the corresponding request.
    pub id: JsonRpcId,

    /// The method result.
    pub result: T,
}

impl<T> JsonRpcSuccessResponse<T> {
    /// Creates a success response for the given request `id`.
    #[must_use]
    pub const fn new(id: JsonRpcId, result: T) -> Self {
        Self {
            jsonrpc: JsonRpcVersion,
            id,
            result,
        }
    }
}

// ── JsonRpcErrorResponse ──────────────────────────────────────────────────────

/// An error JSON-RPC 2.0 response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcErrorResponse {
    /// Protocol version — always `"2.0"`.
    pub jsonrpc: JsonRpcVersion,

    /// Matches the `id` of the corresponding request, or `null` if the id
    /// could not be determined.
    pub id: JsonRpcId,

    /// Structured error object.
    pub error: JsonRpcError,
}

impl JsonRpcErrorResponse {
    /// Creates an error response for the given request `id`.
    #[must_use]
    pub const fn new(id: JsonRpcId, error: JsonRpcError) -> Self {
        Self {
            jsonrpc: JsonRpcVersion,
            id,
            error,
        }
    }
}

// ── JsonRpcError ──────────────────────────────────────────────────────────────

/// The error object within a JSON-RPC 2.0 error response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcError {
    /// Numeric error code.
    pub code: i32,

    /// Short human-readable error message.
    pub message: String,

    /// Optional additional error details.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

impl JsonRpcError {
    /// Creates a new error object.
    #[must_use]
    pub fn new(code: i32, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            data: None,
        }
    }

    /// Creates a new error object with additional data.
    #[must_use]
    pub fn with_data(code: i32, message: impl Into<String>, data: serde_json::Value) -> Self {
        Self {
            code,
            message: message.into(),
            data: Some(data),
        }
    }
}

impl fmt::Display for JsonRpcError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{}] {}", self.code, self.message)
    }
}

impl std::error::Error for JsonRpcError {}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_serializes_as_2_0() {
        let v = JsonRpcVersion;
        let s = serde_json::to_string(&v).expect("serialize");
        assert_eq!(s, "\"2.0\"");
    }

    #[test]
    fn version_rejects_wrong_version() {
        let result: Result<JsonRpcVersion, _> = serde_json::from_str("\"1.0\"");
        assert!(result.is_err(), "should reject non-2.0 version");
    }

    #[test]
    fn version_accepts_2_0() {
        let v: JsonRpcVersion = serde_json::from_str("\"2.0\"").expect("deserialize");
        assert_eq!(v, JsonRpcVersion);
    }

    #[test]
    fn request_roundtrip() {
        let req = JsonRpcRequest::with_params(
            serde_json::json!(1),
            "message/send",
            serde_json::json!({"message": {}}),
        );
        let json = serde_json::to_string(&req).expect("serialize");
        assert!(json.contains("\"jsonrpc\":\"2.0\""));
        assert!(json.contains("\"method\":\"message/send\""));

        let back: JsonRpcRequest = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.method, "message/send");
    }

    #[test]
    fn success_response_roundtrip() {
        let resp: JsonRpcResponse<serde_json::Value> =
            JsonRpcResponse::Success(JsonRpcSuccessResponse::new(
                Some(serde_json::json!(42)),
                serde_json::json!({"status": "ok"}),
            ));
        let json = serde_json::to_string(&resp).expect("serialize");
        assert!(json.contains("\"result\""));
        assert!(!json.contains("\"error\""));
    }

    #[test]
    fn error_response_roundtrip() {
        let resp: JsonRpcResponse<serde_json::Value> =
            JsonRpcResponse::Error(JsonRpcErrorResponse::new(
                Some(serde_json::json!(1)),
                JsonRpcError::new(-32601, "Method not found"),
            ));
        let json = serde_json::to_string(&resp).expect("serialize");
        assert!(json.contains("\"error\""));
        assert!(json.contains("-32601"));
    }

    #[test]
    fn notification_has_no_id() {
        let n = JsonRpcRequest::notification("task/cancel", None);
        let json = serde_json::to_string(&n).expect("serialize");
        assert!(
            !json.contains("\"id\""),
            "notification must omit id: {json}"
        );
    }
}
