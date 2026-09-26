// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Why a request was refused on authentication or authorization grounds.
//!
//! A2A defines no error code for either (spec §3.3.2 gives HTTP `401`/`403`,
//! gRPC `UNAUTHENTICATED`/`PERMISSION_DENIED` and "a JSON-RPC custom error" as
//! examples), so an [`A2aError`](crate::error::A2aError) that refuses a
//! credential keeps the code it would otherwise carry and records the refusal
//! here. Each binding reads it to answer with its own status: that is what
//! lets a client tell "your credential was refused" from "your request was
//! malformed", and so refresh a token instead of giving up. An OAuth client
//! discards a cached token on `401`, and on nothing else.
//!
//! It is not part of the error's wire form: the binding's status says it.

/// Which of the two refusals spec §3.3.2 distinguishes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum AuthRejectionKind {
    /// No usable credential: absent, malformed, expired or unknown. HTTP
    /// `401`, gRPC `UNAUTHENTICATED`.
    Unauthenticated,
    /// A valid credential that does not permit this operation. HTTP `403`,
    /// gRPC `PERMISSION_DENIED`.
    PermissionDenied,
}

/// A refused credential, as carried by
/// [`A2aError::auth_rejection`](crate::error::A2aError::auth_rejection).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthRejection {
    kind: AuthRejectionKind,
    challenge: Option<String>,
}

impl AuthRejection {
    pub(crate) const fn unauthenticated(challenge: String) -> Self {
        Self {
            kind: AuthRejectionKind::Unauthenticated,
            challenge: Some(challenge),
        }
    }

    pub(crate) const fn permission_denied() -> Self {
        Self {
            kind: AuthRejectionKind::PermissionDenied,
            challenge: None,
        }
    }

    /// Which refusal this is.
    #[must_use]
    pub const fn kind(&self) -> AuthRejectionKind {
        self.kind
    }

    /// The `WWW-Authenticate` challenge an HTTP binding sends with its `401`
    /// (RFC 9110 §15.5.2 requires one; §11.6.1 defines it), e.g.
    /// `Bearer realm="a2a"`.
    /// `None` for [`AuthRejectionKind::PermissionDenied`].
    #[must_use]
    pub fn challenge(&self) -> Option<&str> {
        self.challenge.as_deref()
    }
}

#[cfg(test)]
mod tests {
    use super::AuthRejectionKind;
    use crate::error::{A2aError, ErrorCode};

    #[test]
    fn unauthenticated_carries_its_challenge_and_keeps_the_json_rpc_code() {
        let err = A2aError::unauthenticated("authentication required", "Bearer realm=\"a2a\"");
        assert_eq!(err.code, ErrorCode::InvalidRequest);
        let rejection = err.auth_rejection().expect("a rejection");
        assert_eq!(rejection.kind(), AuthRejectionKind::Unauthenticated);
        assert_eq!(rejection.challenge(), Some("Bearer realm=\"a2a\""));
    }

    #[test]
    fn permission_denied_has_no_challenge() {
        let err = A2aError::permission_denied("not permitted");
        let rejection = err.auth_rejection().expect("a rejection");
        assert_eq!(rejection.kind(), AuthRejectionKind::PermissionDenied);
        assert_eq!(rejection.challenge(), None);
    }

    #[test]
    fn an_ordinary_error_is_no_rejection() {
        assert!(
            A2aError::new(ErrorCode::InvalidRequest, "bad")
                .auth_rejection()
                .is_none()
        );
    }

    /// The rejection is the binding's to express: the error's JSON is what it
    /// was before, and nothing on the wire can claim one.
    #[test]
    fn the_rejection_is_not_serialized_or_deserialized() {
        let err = A2aError::unauthenticated("authentication required", "Bearer");
        let json = serde_json::to_value(&err).expect("serializes");
        assert_eq!(
            json,
            serde_json::json!({"code": -32600, "message": "authentication required"})
        );
        let back: A2aError = serde_json::from_value(json).expect("deserializes");
        assert!(back.auth_rejection().is_none());
    }
}
