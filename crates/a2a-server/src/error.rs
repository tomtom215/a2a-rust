// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! Server-specific error types.
//!
//! [`ServerError`] wraps lower-level errors and A2A protocol errors into a
//! unified enum for the server framework. Use [`ServerError::to_a2a_error`]
//! to convert back to a protocol-level [`A2aError`] for wire responses.

use std::fmt;

use a2a_types::error::{A2aError, ErrorCode};
use a2a_types::task::TaskId;

// ── ServerError ──────────────────────────────────────────────────────────────

/// Server framework error type.
///
/// Each variant maps to a specific A2A [`ErrorCode`] via [`to_a2a_error`](Self::to_a2a_error).
#[derive(Debug)]
#[non_exhaustive]
pub enum ServerError {
    /// The requested task was not found.
    TaskNotFound(TaskId),
    /// The task is in a terminal state and cannot be canceled.
    TaskNotCancelable(TaskId),
    /// Invalid method parameters.
    InvalidParams(String),
    /// JSON serialization/deserialization failure.
    Serialization(serde_json::Error),
    /// Hyper HTTP error.
    Http(hyper::Error),
    /// HTTP client-side error (e.g. push notification delivery).
    HttpClient(String),
    /// Transport-layer error.
    Transport(String),
    /// The agent does not support push notifications.
    PushNotSupported,
    /// An internal server error.
    Internal(String),
    /// The requested JSON-RPC method was not found.
    MethodNotFound(String),
    /// An A2A protocol error propagated from the executor.
    Protocol(A2aError),
    /// The request body exceeds the configured size limit.
    PayloadTooLarge(String),
    /// An invalid task state transition was attempted.
    InvalidStateTransition {
        /// The task ID.
        task_id: TaskId,
        /// The current state.
        from: a2a_types::task::TaskState,
        /// The attempted target state.
        to: a2a_types::task::TaskState,
    },
}

impl fmt::Display for ServerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TaskNotFound(id) => write!(f, "task not found: {id}"),
            Self::TaskNotCancelable(id) => write!(f, "task not cancelable: {id}"),
            Self::InvalidParams(msg) => write!(f, "invalid params: {msg}"),
            Self::Serialization(e) => write!(f, "serialization error: {e}"),
            Self::Http(e) => write!(f, "HTTP error: {e}"),
            Self::HttpClient(msg) => write!(f, "HTTP client error: {msg}"),
            Self::Transport(msg) => write!(f, "transport error: {msg}"),
            Self::PushNotSupported => f.write_str("push notifications not supported"),
            Self::Internal(msg) => write!(f, "internal error: {msg}"),
            Self::MethodNotFound(m) => write!(f, "method not found: {m}"),
            Self::Protocol(e) => write!(f, "protocol error: {e}"),
            Self::PayloadTooLarge(msg) => write!(f, "payload too large: {msg}"),
            Self::InvalidStateTransition { task_id, from, to } => {
                write!(
                    f,
                    "invalid state transition for task {task_id}: {from} → {to}"
                )
            }
        }
    }
}

impl std::error::Error for ServerError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Serialization(e) => Some(e),
            Self::Http(e) => Some(e),
            Self::Protocol(e) => Some(e),
            _ => None,
        }
    }
}

impl ServerError {
    /// Converts this server error into an [`A2aError`] suitable for wire responses.
    ///
    /// # Mapping
    ///
    /// | Variant | [`ErrorCode`] |
    /// |---|---|
    /// | `TaskNotFound` | `TaskNotFound` |
    /// | `TaskNotCancelable` | `TaskNotCancelable` |
    /// | `InvalidParams` | `InvalidParams` |
    /// | `Serialization` | `ParseError` |
    /// | `MethodNotFound` | `MethodNotFound` |
    /// | `PushNotSupported` | `PushNotificationNotSupported` |
    /// | everything else | `InternalError` |
    #[must_use]
    pub fn to_a2a_error(&self) -> A2aError {
        match self {
            Self::TaskNotFound(id) => A2aError::task_not_found(id),
            Self::TaskNotCancelable(id) => A2aError::task_not_cancelable(id),
            Self::InvalidParams(msg) => A2aError::invalid_params(msg.clone()),
            Self::Serialization(e) => A2aError::parse_error(e.to_string()),
            Self::MethodNotFound(m) => {
                A2aError::new(ErrorCode::MethodNotFound, format!("Method not found: {m}"))
            }
            Self::PushNotSupported => A2aError::new(
                ErrorCode::PushNotificationNotSupported,
                "Push notifications not supported",
            ),
            Self::Protocol(e) => e.clone(),
            Self::Http(e) => A2aError::internal(e.to_string()),
            Self::HttpClient(msg)
            | Self::Transport(msg)
            | Self::Internal(msg)
            | Self::PayloadTooLarge(msg) => A2aError::internal(msg.clone()),
            Self::InvalidStateTransition { task_id, from, to } => A2aError::invalid_params(
                format!("invalid state transition for task {task_id}: {from} → {to}"),
            ),
        }
    }
}

// ── From impls ───────────────────────────────────────────────────────────────

impl From<A2aError> for ServerError {
    fn from(e: A2aError) -> Self {
        Self::Protocol(e)
    }
}

impl From<serde_json::Error> for ServerError {
    fn from(e: serde_json::Error) -> Self {
        Self::Serialization(e)
    }
}

impl From<hyper::Error> for ServerError {
    fn from(e: hyper::Error) -> Self {
        Self::Http(e)
    }
}

// ── ServerResult ─────────────────────────────────────────────────────────────

/// Convenience type alias: `Result<T, ServerError>`.
pub type ServerResult<T> = Result<T, ServerError>;
