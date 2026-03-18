// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! Server-specific error types.
//!
//! [`ServerError`] wraps lower-level errors and A2A protocol errors into a
//! unified enum for the server framework. Use [`ServerError::to_a2a_error`]
//! to convert back to a protocol-level [`A2aError`] for wire responses.

use std::fmt;

use a2a_protocol_types::error::{A2aError, ErrorCode};
use a2a_protocol_types::task::TaskId;

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
        from: a2a_protocol_types::task::TaskState,
        /// The attempted target state.
        to: a2a_protocol_types::task::TaskState,
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::error::Error;

    #[test]
    fn source_serialization_returns_some() {
        let err = ServerError::Serialization(serde_json::from_str::<String>("x").unwrap_err());
        assert!(err.source().is_some());
    }

    #[test]
    fn source_protocol_returns_some() {
        let err = ServerError::Protocol(A2aError::task_not_found("t"));
        assert!(err.source().is_some());
    }

    #[tokio::test]
    async fn source_http_returns_some() {
        // Get a hyper::Error by feeding invalid HTTP data to the server parser.
        use tokio::io::AsyncWriteExt;
        let (mut client, server) = tokio::io::duplex(256);
        // Write invalid HTTP data and close.
        let client_task = tokio::spawn(async move {
            client.write_all(b"NOT VALID HTTP\r\n\r\n").await.unwrap();
            client.shutdown().await.unwrap();
        });
        let hyper_err = hyper::server::conn::http1::Builder::new()
            .serve_connection(
                hyper_util::rt::TokioIo::new(server),
                hyper::service::service_fn(|_req: hyper::Request<hyper::body::Incoming>| async {
                    Ok::<_, hyper::Error>(hyper::Response::new(http_body_util::Full::new(
                        hyper::body::Bytes::new(),
                    )))
                }),
            )
            .await
            .unwrap_err();
        client_task.await.unwrap();
        let err = ServerError::Http(hyper_err);
        assert!(err.source().is_some());
    }

    #[test]
    fn source_transport_returns_none() {
        let err = ServerError::Transport("test".into());
        assert!(err.source().is_none());
    }

    #[test]
    fn source_task_not_found_returns_none() {
        let err = ServerError::TaskNotFound("t".into());
        assert!(err.source().is_none());
    }

    #[test]
    fn source_internal_returns_none() {
        let err = ServerError::Internal("oops".into());
        assert!(err.source().is_none());
    }

    // ── Display tests for all variants ────────────────────────────────────

    #[test]
    fn display_all_variants() {
        assert!(ServerError::TaskNotFound("t1".into())
            .to_string()
            .contains("t1"));
        assert!(ServerError::TaskNotCancelable("t2".into())
            .to_string()
            .contains("t2"));
        assert!(ServerError::InvalidParams("bad".into())
            .to_string()
            .contains("bad"));
        assert!(ServerError::HttpClient("conn".into())
            .to_string()
            .contains("conn"));
        assert!(ServerError::Transport("tcp".into())
            .to_string()
            .contains("tcp"));
        assert_eq!(
            ServerError::PushNotSupported.to_string(),
            "push notifications not supported"
        );
        assert!(ServerError::Internal("oops".into())
            .to_string()
            .contains("oops"));
        assert!(ServerError::MethodNotFound("foo/bar".into())
            .to_string()
            .contains("foo/bar"));
        assert!(ServerError::Protocol(A2aError::task_not_found("t"))
            .to_string()
            .contains("protocol error"));
        assert!(ServerError::PayloadTooLarge("too big".into())
            .to_string()
            .contains("too big"));
        let ist = ServerError::InvalidStateTransition {
            task_id: "t3".into(),
            from: a2a_protocol_types::task::TaskState::Working,
            to: a2a_protocol_types::task::TaskState::Submitted,
        };
        let s = ist.to_string();
        assert!(s.contains("t3"), "missing task_id: {s}");
        assert!(
            s.contains("working") || s.contains("WORKING") || s.contains("Working"),
            "missing from state: {s}"
        );
    }

    // ── to_a2a_error mapping tests ────────────────────────────────────────

    #[test]
    fn to_a2a_error_all_variants() {
        assert_eq!(
            ServerError::TaskNotFound("t".into()).to_a2a_error().code,
            ErrorCode::TaskNotFound
        );
        assert_eq!(
            ServerError::TaskNotCancelable("t".into())
                .to_a2a_error()
                .code,
            ErrorCode::TaskNotCancelable
        );
        assert_eq!(
            ServerError::InvalidParams("x".into()).to_a2a_error().code,
            ErrorCode::InvalidParams
        );
        assert_eq!(
            ServerError::Serialization(serde_json::from_str::<String>("x").unwrap_err())
                .to_a2a_error()
                .code,
            ErrorCode::ParseError
        );
        assert_eq!(
            ServerError::MethodNotFound("m".into()).to_a2a_error().code,
            ErrorCode::MethodNotFound
        );
        assert_eq!(
            ServerError::PushNotSupported.to_a2a_error().code,
            ErrorCode::PushNotificationNotSupported
        );
        assert_eq!(
            ServerError::Protocol(A2aError::task_not_found("t"))
                .to_a2a_error()
                .code,
            ErrorCode::TaskNotFound
        );
        assert_eq!(
            ServerError::HttpClient("x".into()).to_a2a_error().code,
            ErrorCode::InternalError
        );
        assert_eq!(
            ServerError::Transport("x".into()).to_a2a_error().code,
            ErrorCode::InternalError
        );
        assert_eq!(
            ServerError::Internal("x".into()).to_a2a_error().code,
            ErrorCode::InternalError
        );
        assert_eq!(
            ServerError::PayloadTooLarge("x".into()).to_a2a_error().code,
            ErrorCode::InternalError
        );
        let ist = ServerError::InvalidStateTransition {
            task_id: "t".into(),
            from: a2a_protocol_types::task::TaskState::Working,
            to: a2a_protocol_types::task::TaskState::Submitted,
        };
        assert_eq!(ist.to_a2a_error().code, ErrorCode::InvalidParams);
    }

    // ── From impls ────────────────────────────────────────────────────────

    #[test]
    fn from_a2a_error() {
        let e: ServerError = A2aError::internal("test").into();
        assert!(matches!(e, ServerError::Protocol(_)));
    }

    #[test]
    fn from_serde_error() {
        let e: ServerError = serde_json::from_str::<String>("bad").unwrap_err().into();
        assert!(matches!(e, ServerError::Serialization(_)));
    }

    /// Covers lines 65: Display for Http variant.
    #[tokio::test]
    async fn display_http_variant() {
        use tokio::io::AsyncWriteExt;
        let (mut client, server) = tokio::io::duplex(256);
        let client_task = tokio::spawn(async move {
            client.write_all(b"NOT VALID HTTP\r\n\r\n").await.unwrap();
            client.shutdown().await.unwrap();
        });
        let hyper_err = hyper::server::conn::http1::Builder::new()
            .serve_connection(
                hyper_util::rt::TokioIo::new(server),
                hyper::service::service_fn(|_req: hyper::Request<hyper::body::Incoming>| async {
                    Ok::<_, hyper::Error>(hyper::Response::new(http_body_util::Full::new(
                        hyper::body::Bytes::new(),
                    )))
                }),
            )
            .await
            .unwrap_err();
        client_task.await.unwrap();
        let err = ServerError::Http(hyper_err);
        let display = err.to_string();
        assert!(
            display.contains("HTTP error"),
            "Display for Http variant should contain 'HTTP error', got: {display}"
        );
    }

    /// Covers line 150-152: From<hyper::Error> impl.
    #[tokio::test]
    async fn from_hyper_error() {
        use tokio::io::AsyncWriteExt;
        let (mut client, server) = tokio::io::duplex(256);
        let client_task = tokio::spawn(async move {
            client.write_all(b"NOT VALID HTTP\r\n\r\n").await.unwrap();
            client.shutdown().await.unwrap();
        });
        let hyper_err = hyper::server::conn::http1::Builder::new()
            .serve_connection(
                hyper_util::rt::TokioIo::new(server),
                hyper::service::service_fn(|_req: hyper::Request<hyper::body::Incoming>| async {
                    Ok::<_, hyper::Error>(hyper::Response::new(http_body_util::Full::new(
                        hyper::body::Bytes::new(),
                    )))
                }),
            )
            .await
            .unwrap_err();
        client_task.await.unwrap();
        let e: ServerError = hyper_err.into();
        assert!(matches!(e, ServerError::Http(_)));
    }

    /// Covers line 64: Display for Serialization variant.
    #[test]
    fn display_serialization_variant() {
        let err = ServerError::Serialization(serde_json::from_str::<String>("x").unwrap_err());
        let display = err.to_string();
        assert!(
            display.contains("serialization error"),
            "Display for Serialization should contain 'serialization error', got: {display}"
        );
    }

    /// Covers line 123: `to_a2a_error` for Http variant.
    #[tokio::test]
    async fn to_a2a_error_http_variant() {
        use tokio::io::AsyncWriteExt;
        let (mut client, server) = tokio::io::duplex(256);
        let client_task = tokio::spawn(async move {
            client.write_all(b"NOT VALID HTTP\r\n\r\n").await.unwrap();
            client.shutdown().await.unwrap();
        });
        let hyper_err = hyper::server::conn::http1::Builder::new()
            .serve_connection(
                hyper_util::rt::TokioIo::new(server),
                hyper::service::service_fn(|_req: hyper::Request<hyper::body::Incoming>| async {
                    Ok::<_, hyper::Error>(hyper::Response::new(http_body_util::Full::new(
                        hyper::body::Bytes::new(),
                    )))
                }),
            )
            .await
            .unwrap_err();
        client_task.await.unwrap();
        let err = ServerError::Http(hyper_err);
        let a2a_err = err.to_a2a_error();
        assert_eq!(a2a_err.code, ErrorCode::InternalError);
    }
}
