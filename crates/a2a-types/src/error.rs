// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! A2A protocol error types.
//!
//! This module defines [`A2aError`], the canonical error type for all A2A
//! protocol operations, along with [`ErrorCode`] carrying every standard error
//! code defined by A2A v1.0 and the underlying JSON-RPC 2.0 specification.

use std::fmt;

use serde::{Deserialize, Serialize};

// ── Error codes ──────────────────────────────────────────────────────────────

/// Numeric error codes defined by JSON-RPC 2.0 and the A2A v1.0 specification.
///
/// JSON-RPC standard codes occupy the `-32700` to `-32600` range.
/// A2A-specific codes occupy `-32001` to `-32099`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(into = "i32", try_from = "i32")]
#[non_exhaustive]
pub enum ErrorCode {
    // ── JSON-RPC 2.0 standard ─────────────────────────────────────────────
    /// Invalid JSON was received by the server (`-32700`).
    ParseError = -32700,
    /// The JSON sent is not a valid Request object (`-32600`).
    InvalidRequest = -32600,
    /// The method does not exist or is not available (`-32601`).
    MethodNotFound = -32601,
    /// Invalid method parameters (`-32602`).
    InvalidParams = -32602,
    /// Internal JSON-RPC error (`-32603`).
    InternalError = -32603,

    // ── A2A-specific ──────────────────────────────────────────────────────
    /// The requested task was not found (`-32001`).
    TaskNotFound = -32001,
    /// The task cannot be canceled in its current state (`-32002`).
    TaskNotCancelable = -32002,
    /// The agent does not support push notifications (`-32003`).
    PushNotificationNotSupported = -32003,
    /// The requested operation is not supported by this agent (`-32004`).
    UnsupportedOperation = -32004,
    /// The requested content type is not supported (`-32005`).
    ContentTypeNotSupported = -32005,
    /// The agent returned an invalid response (`-32006`).
    InvalidAgentResponse = -32006,
    /// Extended agent card not configured (`-32007`).
    ExtendedAgentCardNotConfigured = -32007,
    /// A required extension is not supported (`-32008`).
    ExtensionSupportRequired = -32008,
    /// The requested protocol version is not supported (`-32009`).
    VersionNotSupported = -32009,
}

impl ErrorCode {
    /// Returns the numeric value of this error code.
    #[must_use]
    pub const fn as_i32(self) -> i32 {
        self as i32
    }

    /// Returns a short human-readable description of the code.
    #[must_use]
    pub const fn default_message(self) -> &'static str {
        match self {
            Self::ParseError => "Parse error",
            Self::InvalidRequest => "Invalid request",
            Self::MethodNotFound => "Method not found",
            Self::InvalidParams => "Invalid params",
            Self::InternalError => "Internal error",
            Self::TaskNotFound => "Task not found",
            Self::TaskNotCancelable => "Task not cancelable",
            Self::PushNotificationNotSupported => "Push notification not supported",
            Self::UnsupportedOperation => "Unsupported operation",
            Self::ContentTypeNotSupported => "Content type not supported",
            Self::InvalidAgentResponse => "Invalid agent response",
            Self::ExtendedAgentCardNotConfigured => "Extended agent card not configured",
            Self::ExtensionSupportRequired => "Extension support required",
            Self::VersionNotSupported => "Version not supported",
        }
    }
}

impl From<ErrorCode> for i32 {
    fn from(code: ErrorCode) -> Self {
        code as Self
    }
}

impl TryFrom<i32> for ErrorCode {
    type Error = i32;

    fn try_from(v: i32) -> Result<Self, Self::Error> {
        match v {
            -32700 => Ok(Self::ParseError),
            -32600 => Ok(Self::InvalidRequest),
            -32601 => Ok(Self::MethodNotFound),
            -32602 => Ok(Self::InvalidParams),
            -32603 => Ok(Self::InternalError),
            -32001 => Ok(Self::TaskNotFound),
            -32002 => Ok(Self::TaskNotCancelable),
            -32003 => Ok(Self::PushNotificationNotSupported),
            -32004 => Ok(Self::UnsupportedOperation),
            -32005 => Ok(Self::ContentTypeNotSupported),
            -32006 => Ok(Self::InvalidAgentResponse),
            -32007 => Ok(Self::ExtendedAgentCardNotConfigured),
            -32008 => Ok(Self::ExtensionSupportRequired),
            -32009 => Ok(Self::VersionNotSupported),
            other => Err(other),
        }
    }
}

impl fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} ({})", self.default_message(), self.as_i32())
    }
}

// ── A2aError ──────────────────────────────────────────────────────────────────

/// The canonical error type for A2A protocol operations.
///
/// Carries an [`ErrorCode`], a human-readable `message`, and an optional
/// `data` payload (arbitrary JSON) for additional diagnostics.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct A2aError {
    /// Machine-readable error code.
    pub code: ErrorCode,
    /// Human-readable error message.
    pub message: String,
    /// Optional structured error details.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

impl A2aError {
    /// Creates a new `A2aError` with the given code and message.
    #[must_use]
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            data: None,
        }
    }

    /// Creates a new `A2aError` with the given code, message, and data.
    #[must_use]
    pub fn with_data(code: ErrorCode, message: impl Into<String>, data: serde_json::Value) -> Self {
        Self {
            code,
            message: message.into(),
            data: Some(data),
        }
    }

    // ── Named constructors ────────────────────────────────────────────────

    /// Creates a "Task not found" error for the given task ID string.
    #[must_use]
    pub fn task_not_found(task_id: impl fmt::Display) -> Self {
        Self::new(
            ErrorCode::TaskNotFound,
            format!("Task not found: {task_id}"),
        )
    }

    /// Creates a "Task not cancelable" error.
    #[must_use]
    pub fn task_not_cancelable(task_id: impl fmt::Display) -> Self {
        Self::new(
            ErrorCode::TaskNotCancelable,
            format!("Task cannot be canceled: {task_id}"),
        )
    }

    /// Creates an internal error with the provided message.
    #[must_use]
    pub fn internal(msg: impl Into<String>) -> Self {
        Self::new(ErrorCode::InternalError, msg)
    }

    /// Creates an "Invalid params" error.
    #[must_use]
    pub fn invalid_params(msg: impl Into<String>) -> Self {
        Self::new(ErrorCode::InvalidParams, msg)
    }

    /// Creates an "Unsupported operation" error.
    #[must_use]
    pub fn unsupported_operation(msg: impl Into<String>) -> Self {
        Self::new(ErrorCode::UnsupportedOperation, msg)
    }

    /// Creates a "Parse error" error.
    #[must_use]
    pub fn parse_error(msg: impl Into<String>) -> Self {
        Self::new(ErrorCode::ParseError, msg)
    }

    /// Creates an "Invalid agent response" error.
    #[must_use]
    pub fn invalid_agent_response(msg: impl Into<String>) -> Self {
        Self::new(ErrorCode::InvalidAgentResponse, msg)
    }

    /// Creates an "Extended agent card not configured" error.
    #[must_use]
    pub fn extended_card_not_configured(msg: impl Into<String>) -> Self {
        Self::new(ErrorCode::ExtendedAgentCardNotConfigured, msg)
    }
}

impl fmt::Display for A2aError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{}] {}", self.code.as_i32(), self.message)
    }
}

impl std::error::Error for A2aError {}

// ── A2aResult ─────────────────────────────────────────────────────────────────

/// Convenience type alias: `Result<T, A2aError>`.
pub type A2aResult<T> = Result<T, A2aError>;

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_code_roundtrip() {
        let code = ErrorCode::TaskNotFound;
        let n: i32 = code.into();
        assert_eq!(n, -32001);
        assert_eq!(ErrorCode::try_from(n), Ok(ErrorCode::TaskNotFound));
    }

    #[test]
    fn error_code_unknown_value() {
        assert!(ErrorCode::try_from(-99999).is_err());
    }

    #[test]
    fn a2a_error_display() {
        let err = A2aError::task_not_found("abc123");
        let s = err.to_string();
        assert!(s.contains("-32001"), "expected code in display: {s}");
        assert!(s.contains("abc123"), "expected task id in display: {s}");
    }

    #[test]
    fn a2a_error_serialization() {
        let err = A2aError::internal("something went wrong");
        let json = serde_json::to_string(&err).expect("serialize");
        let back: A2aError = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.code, ErrorCode::InternalError);
        assert_eq!(back.message, "something went wrong");
        assert!(back.data.is_none());
    }

    #[test]
    fn a2a_error_with_data() {
        let data = serde_json::json!({"detail": "extra info"});
        let err = A2aError::with_data(ErrorCode::InvalidParams, "bad input", data.clone());
        let json = serde_json::to_string(&err).expect("serialize");
        assert!(json.contains("\"data\""), "data field should be present");
        let back: A2aError = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.data, Some(data));
    }
}
