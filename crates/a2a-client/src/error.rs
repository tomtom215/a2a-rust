// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! Client error types.
//!
//! [`ClientError`] is the top-level error type for all A2A client operations.
//! Use [`ClientResult`] as the return type alias.

use std::fmt;

use a2a_protocol_types::{A2aError, TaskId};

// ── ClientError ───────────────────────────────────────────────────────────────

/// Errors that can occur during A2A client operations.
#[derive(Debug)]
#[non_exhaustive]
pub enum ClientError {
    /// A transport-level HTTP error from hyper.
    Http(hyper::Error),

    /// An HTTP-level error from the hyper-util client (connection, redirect, etc.).
    HttpClient(String),

    /// JSON serialization or deserialization error.
    Serialization(serde_json::Error),

    /// A protocol-level A2A error returned by the server.
    Protocol(A2aError),

    /// A transport configuration or connection error.
    Transport(String),

    /// The agent endpoint URL is invalid or could not be resolved.
    InvalidEndpoint(String),

    /// The server returned an unexpected HTTP status code.
    UnexpectedStatus {
        /// The HTTP status code received.
        status: u16,
        /// The response body (truncated if large).
        body: String,
    },

    /// The agent requires authentication for this task.
    AuthRequired {
        /// The ID of the task requiring authentication.
        task_id: TaskId,
    },

    /// A request or stream connection timed out.
    Timeout(String),

    /// The server appears to use a different protocol binding than the client.
    ///
    /// For example, a JSON-RPC client connected to a REST-only server (or
    /// vice-versa).  Check the agent card's `supported_interfaces` to select
    /// the correct protocol binding.
    ProtocolBindingMismatch(String),
}

impl fmt::Display for ClientError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Http(e) => write!(f, "HTTP error: {e}"),
            Self::HttpClient(msg) => write!(f, "HTTP client error: {msg}"),
            Self::Serialization(e) => write!(f, "serialization error: {e}"),
            Self::Protocol(e) => write!(f, "protocol error: {e}"),
            Self::Transport(msg) => write!(f, "transport error: {msg}"),
            Self::InvalidEndpoint(msg) => write!(f, "invalid endpoint: {msg}"),
            Self::UnexpectedStatus { status, body } => {
                write!(f, "unexpected HTTP status {status}: {body}")
            }
            Self::AuthRequired { task_id } => {
                write!(f, "authentication required for task: {task_id}")
            }
            Self::Timeout(msg) => write!(f, "timeout: {msg}"),
            Self::ProtocolBindingMismatch(msg) => {
                write!(
                    f,
                    "protocol binding mismatch: {msg}; check the agent card's supported_interfaces"
                )
            }
        }
    }
}

impl std::error::Error for ClientError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Http(e) => Some(e),
            Self::Serialization(e) => Some(e),
            Self::Protocol(e) => Some(e),
            _ => None,
        }
    }
}

impl From<A2aError> for ClientError {
    fn from(e: A2aError) -> Self {
        Self::Protocol(e)
    }
}

impl From<hyper::Error> for ClientError {
    fn from(e: hyper::Error) -> Self {
        Self::Http(e)
    }
}

impl From<serde_json::Error> for ClientError {
    fn from(e: serde_json::Error) -> Self {
        Self::Serialization(e)
    }
}

// ── ClientResult ──────────────────────────────────────────────────────────────

/// Convenience type alias: `Result<T, ClientError>`.
pub type ClientResult<T> = Result<T, ClientError>;

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use a2a_protocol_types::ErrorCode;

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
        };
        assert!(e.to_string().contains("404"));
    }
}
