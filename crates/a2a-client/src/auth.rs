// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! Authentication interceptor and credential storage.
//!
//! [`AuthInterceptor`] injects `Authorization` headers from a
//! [`CredentialsStore`] before each request. [`InMemoryCredentialsStore`]
//! provides a simple in-process credential store.
//!
//! # Usage
//!
//! ```rust,no_run
//! use std::sync::Arc;
//! use a2a_client::auth::{
//!     InMemoryCredentialsStore, AuthInterceptor, SessionId, CredentialsStore,
//! };
//! use a2a_client::ClientBuilder;
//!
//! let store = Arc::new(InMemoryCredentialsStore::new());
//! let session = SessionId::new("my-session");
//! store.set(session.clone(), "bearer", "my-token".into());
//!
//! let _builder = ClientBuilder::new("http://localhost:8080")
//!     .with_interceptor(AuthInterceptor::new(store, session));
//! ```

use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, RwLock};

use crate::error::ClientResult;
use crate::interceptor::{CallInterceptor, ClientRequest, ClientResponse};

// ── SessionId ─────────────────────────────────────────────────────────────────

/// Opaque identifier for a client authentication session.
///
/// Sessions scope credentials so that a single credential store can manage
/// tokens for multiple simultaneous client instances.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SessionId(String);

impl SessionId {
    /// Creates a new [`SessionId`] from any string-like value.
    #[must_use]
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }
}

impl fmt::Display for SessionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<String> for SessionId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for SessionId {
    fn from(s: &str) -> Self {
        Self(s.to_owned())
    }
}

// ── CredentialsStore ──────────────────────────────────────────────────────────

/// Persistent storage for auth credentials, keyed by session + scheme.
///
/// Schemes follow the A2A / HTTP convention: `"bearer"`, `"basic"`,
/// `"api-key"`, etc. The stored value is the raw credential (e.g. the raw
/// token string, not including the scheme prefix).
pub trait CredentialsStore: Send + Sync + 'static {
    /// Returns the credential for the given session and scheme, if present.
    fn get(&self, session: &SessionId, scheme: &str) -> Option<String>;

    /// Stores a credential for the given session and scheme.
    fn set(&self, session: SessionId, scheme: &str, credential: String);

    /// Removes the credential for the given session and scheme.
    fn remove(&self, session: &SessionId, scheme: &str);
}

// ── InMemoryCredentialsStore ──────────────────────────────────────────────────

/// An in-memory [`CredentialsStore`] backed by an `RwLock<HashMap>`.
///
/// Suitable for single-process deployments. Credentials are lost when the
/// process exits.
pub struct InMemoryCredentialsStore {
    inner: RwLock<HashMap<SessionId, HashMap<String, String>>>,
}

impl InMemoryCredentialsStore {
    /// Creates an empty credential store.
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: RwLock::new(HashMap::new()),
        }
    }
}

impl Default for InMemoryCredentialsStore {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for InMemoryCredentialsStore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Don't expose credential values in debug output.
        let count = self.inner.read().map(|g| g.len()).unwrap_or(0);
        f.debug_struct("InMemoryCredentialsStore")
            .field("sessions", &count)
            .finish()
    }
}

impl CredentialsStore for InMemoryCredentialsStore {
    fn get(&self, session: &SessionId, scheme: &str) -> Option<String> {
        self.inner.read().ok()?.get(session)?.get(scheme).cloned()
    }

    fn set(&self, session: SessionId, scheme: &str, credential: String) {
        if let Ok(mut guard) = self.inner.write() {
            guard
                .entry(session)
                .or_default()
                .insert(scheme.to_owned(), credential);
        }
    }

    fn remove(&self, session: &SessionId, scheme: &str) {
        if let Ok(mut guard) = self.inner.write() {
            if let Some(schemes) = guard.get_mut(session) {
                schemes.remove(scheme);
            }
        }
    }
}

// ── AuthInterceptor ───────────────────────────────────────────────────────────

/// A [`CallInterceptor`] that injects `Authorization` headers from a
/// [`CredentialsStore`].
///
/// On each `before()` call it looks up the credential for the current session
/// using the configured scheme (default: `"bearer"`). If found, it adds:
///
/// ```text
/// Authorization: Bearer <token>
/// ```
///
/// to `req.extra_headers`.
pub struct AuthInterceptor {
    store: Arc<dyn CredentialsStore>,
    session: SessionId,
    /// The auth scheme to look up (e.g. `"bearer"`, `"api-key"`).
    scheme: String,
}

impl AuthInterceptor {
    /// Creates an [`AuthInterceptor`] that injects bearer tokens.
    #[must_use]
    pub fn new(store: Arc<dyn CredentialsStore>, session: SessionId) -> Self {
        Self {
            store,
            session,
            scheme: "bearer".to_owned(),
        }
    }

    /// Creates an [`AuthInterceptor`] with a custom auth scheme.
    #[must_use]
    pub fn with_scheme(
        store: Arc<dyn CredentialsStore>,
        session: SessionId,
        scheme: impl Into<String>,
    ) -> Self {
        Self {
            store,
            session,
            scheme: scheme.into(),
        }
    }
}

#[allow(clippy::missing_fields_in_debug)]
impl fmt::Debug for AuthInterceptor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Intentionally omit `store` to avoid exposing credential internals.
        f.debug_struct("AuthInterceptor")
            .field("session", &self.session)
            .field("scheme", &self.scheme)
            .finish()
    }
}

impl CallInterceptor for AuthInterceptor {
    #[allow(clippy::manual_async_fn)]
    fn before<'a>(
        &'a self,
        req: &'a mut ClientRequest,
    ) -> impl std::future::Future<Output = ClientResult<()>> + Send + 'a {
        async move {
            if let Some(credential) = self.store.get(&self.session, &self.scheme) {
                let header_value = if self.scheme.eq_ignore_ascii_case("bearer") {
                    format!("Bearer {credential}")
                } else if self.scheme.eq_ignore_ascii_case("basic") {
                    format!("Basic {credential}")
                } else {
                    credential
                };
                req.extra_headers
                    .insert("authorization".to_owned(), header_value);
            }
            Ok(())
        }
    }

    #[allow(clippy::manual_async_fn)]
    fn after<'a>(
        &'a self,
        _resp: &'a ClientResponse,
    ) -> impl std::future::Future<Output = ClientResult<()>> + Send + 'a {
        async move { Ok(()) }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credentials_store_set_get_remove() {
        let store = InMemoryCredentialsStore::new();
        let session = SessionId::new("sess-1");

        assert!(store.get(&session, "bearer").is_none());

        store.set(session.clone(), "bearer", "my-token".into());
        assert_eq!(store.get(&session, "bearer").as_deref(), Some("my-token"));

        store.remove(&session, "bearer");
        assert!(store.get(&session, "bearer").is_none());
    }

    #[tokio::test]
    async fn auth_interceptor_injects_bearer() {
        let store = Arc::new(InMemoryCredentialsStore::new());
        let session = SessionId::new("test");
        store.set(session.clone(), "bearer", "my-secret-token".into());

        let interceptor = AuthInterceptor::new(store, session);
        let mut req = ClientRequest::new("message/send", serde_json::Value::Null);

        interceptor.before(&mut req).await.unwrap();

        assert_eq!(
            req.extra_headers.get("authorization").map(String::as_str),
            Some("Bearer my-secret-token")
        );
    }

    #[tokio::test]
    async fn auth_interceptor_no_credential_no_header() {
        let store = Arc::new(InMemoryCredentialsStore::new());
        let session = SessionId::new("empty");
        let interceptor = AuthInterceptor::new(store, session);

        let mut req = ClientRequest::new("message/send", serde_json::Value::Null);
        interceptor.before(&mut req).await.unwrap();

        assert!(!req.extra_headers.contains_key("authorization"));
    }
}
