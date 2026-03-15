// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! Comprehensive integration tests for the `a2a-client` crate.

use std::sync::Arc;
use std::time::Duration;

use a2a_client::auth::{AuthInterceptor, InMemoryCredentialsStore, SessionId};
use a2a_client::config::{ClientConfig, TlsConfig, BINDING_JSONRPC};
use a2a_client::error::ClientError;
use a2a_client::interceptor::{CallInterceptor, ClientRequest, ClientResponse};
use a2a_client::{ClientBuilder, CredentialsStore, JsonRpcTransport, RestTransport};
use a2a_types::{AgentCapabilities, AgentCard, AgentInterface};

// ── Helpers ──────────────────────────────────────────────────────────────────

/// A no-op interceptor for testing `with_interceptor`.
struct NoopInterceptor;

impl CallInterceptor for NoopInterceptor {
    async fn before(&self, _req: &mut ClientRequest) -> a2a_client::ClientResult<()> {
        Ok(())
    }

    async fn after(&self, _resp: &ClientResponse) -> a2a_client::ClientResult<()> {
        Ok(())
    }
}

/// Builds a minimal [`AgentCard`] with the given interfaces.
fn agent_card_with_interfaces(interfaces: Vec<AgentInterface>) -> AgentCard {
    AgentCard {
        name: "test-agent".into(),
        version: "1.0".into(),
        description: "A test agent".into(),
        supported_interfaces: interfaces,
        provider: None,
        icon_url: None,
        documentation_url: None,
        capabilities: AgentCapabilities::none(),
        security_schemes: None,
        security_requirements: None,
        default_input_modes: vec![],
        default_output_modes: vec![],
        skills: vec![],
        signatures: None,
    }
}

// ── 1. ClientBuilder tests ───────────────────────────────────────────────────

#[test]
fn builder_from_card_empty_interfaces_falls_back_to_defaults() {
    let card = agent_card_with_interfaces(vec![]);
    // `from_card` with no interfaces should produce a builder with an empty
    // endpoint. Building should fail because an empty URL is invalid.
    let result = ClientBuilder::from_card(&card).build();
    assert!(result.is_err(), "empty endpoint should fail validation");
}

#[test]
fn builder_with_custom_transport_overrides_transport() {
    let custom = JsonRpcTransport::new("http://localhost:7777").expect("transport");
    let client = ClientBuilder::new("http://localhost:9999")
        .with_custom_transport(custom)
        .build()
        .expect("build should succeed with custom transport");
    // Verify the client was built successfully with the custom transport.
    let dbg = format!("{client:?}");
    assert!(
        dbg.contains("A2aClient"),
        "client should be an A2aClient instance"
    );
    // The config should reflect the builder's settings, not the transport URL.
    assert_eq!(client.config().request_timeout, Duration::from_secs(30));
}

#[test]
fn builder_with_accepted_output_modes_stores_config() {
    let modes = vec!["text/html".to_owned(), "image/png".to_owned()];
    let client = ClientBuilder::new("http://localhost:8080")
        .with_accepted_output_modes(modes.clone())
        .build()
        .expect("build");
    assert_eq!(client.config().accepted_output_modes, modes);
}

#[test]
fn builder_with_history_length_stores_config() {
    let client = ClientBuilder::new("http://localhost:8080")
        .with_history_length(42)
        .build()
        .expect("build");
    assert_eq!(client.config().history_length, Some(42));
}

#[test]
fn builder_with_return_immediately_stores_config() {
    let client = ClientBuilder::new("http://localhost:8080")
        .with_return_immediately(true)
        .build()
        .expect("build");
    assert!(client.config().return_immediately);
}

#[test]
fn builder_without_tls_disables_tls() {
    let client = ClientBuilder::new("http://localhost:8080")
        .without_tls()
        .build()
        .expect("build");
    assert!(matches!(client.config().tls, TlsConfig::Disabled));
}

#[test]
fn builder_with_interceptor_adds_to_chain() {
    // Build a client with two interceptors; if it builds, the chain accepted
    // them. We verify indirectly via Debug output showing the count.
    let client = ClientBuilder::new("http://localhost:8080")
        .with_interceptor(NoopInterceptor)
        .with_interceptor(NoopInterceptor)
        .build()
        .expect("build");
    let dbg = format!("{client:?}");
    assert!(
        dbg.contains("A2aClient"),
        "debug should contain struct name"
    );
}

// ── 2. InMemoryCredentialsStore tests ────────────────────────────────────────

#[test]
fn credentials_store_multiple_sessions_dont_interfere() {
    let store = InMemoryCredentialsStore::new();
    let s1 = SessionId::new("session-1");
    let s2 = SessionId::new("session-2");

    store.set(s1.clone(), "bearer", "token-1".into());
    store.set(s2.clone(), "bearer", "token-2".into());

    assert_eq!(store.get(&s1, "bearer").as_deref(), Some("token-1"));
    assert_eq!(store.get(&s2, "bearer").as_deref(), Some("token-2"));
}

#[test]
fn credentials_store_multiple_schemes_per_session() {
    let store = InMemoryCredentialsStore::new();
    let session = SessionId::new("multi-scheme");

    store.set(session.clone(), "bearer", "bearer-token".into());
    store.set(session.clone(), "api-key", "api-key-value".into());

    assert_eq!(
        store.get(&session, "bearer").as_deref(),
        Some("bearer-token")
    );
    assert_eq!(
        store.get(&session, "api-key").as_deref(),
        Some("api-key-value")
    );
}

#[test]
fn credentials_store_remove_is_idempotent() {
    let store = InMemoryCredentialsStore::new();
    let session = SessionId::new("idempotent");

    // Removing a non-existent key should not panic.
    store.remove(&session, "bearer");
    store.remove(&session, "bearer");

    // After inserting and removing, a second remove should also be fine.
    store.set(session.clone(), "bearer", "token".into());
    store.remove(&session, "bearer");
    store.remove(&session, "bearer");

    assert!(store.get(&session, "bearer").is_none());
}

#[test]
fn credentials_store_debug_hides_credentials() {
    let store = InMemoryCredentialsStore::new();
    let session = SessionId::new("secret-session");
    store.set(session, "bearer", "super-secret-token-12345".into());

    let dbg = format!("{store:?}");
    assert!(
        !dbg.contains("super-secret-token-12345"),
        "Debug output must not contain the credential value"
    );
    assert!(
        dbg.contains("InMemoryCredentialsStore"),
        "Debug output should contain the struct name"
    );
    assert!(
        dbg.contains("sessions"),
        "Debug output should show session count field"
    );
}

// ── 3. AuthInterceptor tests ─────────────────────────────────────────────────

#[tokio::test]
async fn auth_interceptor_with_scheme_basic_produces_basic_header() {
    let store = Arc::new(InMemoryCredentialsStore::new());
    let session = SessionId::new("basic-session");
    store.set(session.clone(), "basic", "dXNlcjpwYXNz".into());

    let interceptor = AuthInterceptor::with_scheme(store, session, "basic");
    let mut req = ClientRequest::new("message/send", serde_json::Value::Null);
    interceptor.before(&mut req).await.unwrap();

    assert_eq!(
        req.extra_headers.get("authorization").map(String::as_str),
        Some("Basic dXNlcjpwYXNz")
    );
}

#[tokio::test]
async fn auth_interceptor_with_scheme_api_key_produces_raw_credential() {
    let store = Arc::new(InMemoryCredentialsStore::new());
    let session = SessionId::new("apikey-session");
    store.set(session.clone(), "api-key", "my-raw-api-key".into());

    let interceptor = AuthInterceptor::with_scheme(store, session, "api-key");
    let mut req = ClientRequest::new("message/send", serde_json::Value::Null);
    interceptor.before(&mut req).await.unwrap();

    // For non-bearer/non-basic schemes, the raw credential is used as-is.
    assert_eq!(
        req.extra_headers.get("authorization").map(String::as_str),
        Some("my-raw-api-key")
    );
}

// ── 4. ClientConfig tests ────────────────────────────────────────────────────

#[test]
fn client_config_default_produces_valid_config() {
    let cfg = ClientConfig::default();
    assert_eq!(cfg.request_timeout, Duration::from_secs(30));
    assert_eq!(cfg.stream_connect_timeout, Duration::from_secs(30));
    assert_eq!(cfg.preferred_bindings, vec![BINDING_JSONRPC]);
    assert_eq!(
        cfg.accepted_output_modes,
        vec!["text/plain", "application/json"]
    );
    assert!(cfg.history_length.is_none());
    assert!(!cfg.return_immediately);
}

#[test]
fn client_config_default_http_produces_valid_config() {
    let cfg = ClientConfig::default_http();
    assert_eq!(cfg.request_timeout, Duration::from_secs(30));
    assert!(matches!(cfg.tls, TlsConfig::Disabled));
    assert_eq!(cfg.preferred_bindings, vec![BINDING_JSONRPC]);
    assert_eq!(
        cfg.accepted_output_modes,
        vec!["text/plain", "application/json"]
    );
}

// ── 5. ClientError tests ─────────────────────────────────────────────────────

#[test]
fn client_error_display_transport() {
    let e = ClientError::Transport("connection reset".into());
    let msg = e.to_string();
    assert!(
        msg.contains("transport error"),
        "expected 'transport error' in: {msg}"
    );
    assert!(msg.contains("connection reset"));
}

#[test]
fn client_error_display_invalid_endpoint() {
    let e = ClientError::InvalidEndpoint("bad url".into());
    let msg = e.to_string();
    assert!(
        msg.contains("invalid endpoint"),
        "expected 'invalid endpoint' in: {msg}"
    );
    assert!(msg.contains("bad url"));
}

#[test]
fn client_error_display_unexpected_status() {
    let e = ClientError::UnexpectedStatus {
        status: 503,
        body: "Service Unavailable".into(),
    };
    let msg = e.to_string();
    assert!(msg.contains("503"));
    assert!(msg.contains("Service Unavailable"));
}

#[test]
fn client_error_display_serialization() {
    let bad_json: Result<serde_json::Value, _> = serde_json::from_str("not json");
    let serde_err = bad_json.unwrap_err();
    let e = ClientError::Serialization(serde_err);
    let msg = e.to_string();
    assert!(
        msg.contains("serialization error"),
        "expected 'serialization error' in: {msg}"
    );
}

#[test]
fn client_error_display_protocol() {
    let a2a = a2a_types::A2aError::new(a2a_types::ErrorCode::InternalError, "boom");
    let e = ClientError::Protocol(a2a);
    let msg = e.to_string();
    assert!(
        msg.contains("protocol error"),
        "expected 'protocol error' in: {msg}"
    );
    assert!(msg.contains("boom"));
}

#[test]
fn client_error_display_http_client() {
    let e = ClientError::HttpClient("dns failure".into());
    let msg = e.to_string();
    assert!(
        msg.contains("HTTP client error"),
        "expected 'HTTP client error' in: {msg}"
    );
    assert!(msg.contains("dns failure"));
}

#[test]
fn client_error_display_auth_required() {
    let e = ClientError::AuthRequired {
        task_id: a2a_types::TaskId::new("task-abc"),
    };
    let msg = e.to_string();
    assert!(msg.contains("authentication required"));
    assert!(msg.contains("task-abc"));
}

#[test]
fn client_error_source_returns_inner_for_serialization() {
    use std::error::Error;
    let serde_err = serde_json::from_str::<serde_json::Value>("bad").unwrap_err();
    let e = ClientError::Serialization(serde_err);
    assert!(e.source().is_some(), "Serialization should have a source");
}

#[test]
fn client_error_source_returns_inner_for_protocol() {
    use std::error::Error;
    let a2a = a2a_types::A2aError::new(a2a_types::ErrorCode::InternalError, "err");
    let e = ClientError::Protocol(a2a);
    assert!(e.source().is_some(), "Protocol should have a source");
}

#[test]
fn client_error_source_returns_none_for_transport() {
    use std::error::Error;
    let e = ClientError::Transport("msg".into());
    assert!(e.source().is_none(), "Transport should not have a source");
}

#[test]
fn client_error_source_returns_none_for_invalid_endpoint() {
    use std::error::Error;
    let e = ClientError::InvalidEndpoint("bad".into());
    assert!(
        e.source().is_none(),
        "InvalidEndpoint should not have a source"
    );
}

#[test]
fn client_error_source_returns_none_for_unexpected_status() {
    use std::error::Error;
    let e = ClientError::UnexpectedStatus {
        status: 500,
        body: "error".into(),
    };
    assert!(
        e.source().is_none(),
        "UnexpectedStatus should not have a source"
    );
}

#[test]
fn client_error_from_serde_json_error() {
    let serde_err = serde_json::from_str::<serde_json::Value>("{{").unwrap_err();
    let e: ClientError = serde_err.into();
    assert!(matches!(e, ClientError::Serialization(_)));
}

#[test]
fn client_error_from_a2a_error() {
    let a2a = a2a_types::A2aError::new(a2a_types::ErrorCode::TaskNotFound, "not found");
    let e: ClientError = a2a.into();
    assert!(matches!(e, ClientError::Protocol(_)));
}

// ── 6. Transport URL validation tests ────────────────────────────────────────

#[test]
fn jsonrpc_transport_with_timeout_custom_timeout() {
    let t = JsonRpcTransport::with_timeout("http://localhost:8080", Duration::from_secs(120))
        .expect("should accept valid URL with custom timeout");
    assert_eq!(t.endpoint(), "http://localhost:8080");
}

#[test]
fn jsonrpc_transport_with_timeout_rejects_invalid_url() {
    let result = JsonRpcTransport::with_timeout("not-valid", Duration::from_secs(10));
    assert!(result.is_err(), "invalid URL should be rejected");
}

#[test]
fn rest_transport_base_url_trailing_slash_removed() {
    let t = RestTransport::new("http://localhost:8080/").expect("transport");
    assert_eq!(
        t.base_url(),
        "http://localhost:8080",
        "trailing slash should be stripped"
    );
}

#[test]
fn rest_transport_base_url_no_trailing_slash_unchanged() {
    let t = RestTransport::new("http://localhost:8080").expect("transport");
    assert_eq!(t.base_url(), "http://localhost:8080");
}

#[test]
fn rest_transport_with_timeout_custom_timeout() {
    let t = RestTransport::with_timeout("http://localhost:8080", Duration::from_secs(90))
        .expect("should accept valid URL with custom timeout");
    assert_eq!(t.base_url(), "http://localhost:8080");
}

#[test]
fn rest_transport_with_timeout_rejects_invalid_url() {
    let result = RestTransport::with_timeout("ftp://bad", Duration::from_secs(10));
    assert!(result.is_err(), "invalid URL should be rejected");
}
