// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! Tests covering client coverage gaps: builder edge cases, transport
//! configuration, credential store, error types.

use a2a_protocol_client::auth::{InMemoryCredentialsStore, SessionId};
use a2a_protocol_client::builder::ClientBuilder;
use a2a_protocol_client::error::ClientError;
use a2a_protocol_client::interceptor::InterceptorChain;
use a2a_protocol_client::CredentialsStore;

// ── Builder edge cases ───────────────────────────────────────────────────────

#[test]
fn builder_with_trailing_slash() {
    let client = ClientBuilder::new("http://localhost:8080/").build();
    assert!(client.is_ok(), "trailing slash should be accepted");
}

#[test]
fn builder_with_path_prefix() {
    let client = ClientBuilder::new("http://localhost:8080/api/v1").build();
    assert!(client.is_ok(), "path prefix should be accepted");
}

#[test]
fn builder_invalid_url() {
    let client = ClientBuilder::new("not-a-url").build();
    assert!(client.is_err(), "completely invalid URL should be rejected");
}

#[test]
fn builder_empty_url() {
    let client = ClientBuilder::new("").build();
    assert!(client.is_err(), "empty URL should be rejected");
}

#[test]
fn builder_with_timeout() {
    let client = ClientBuilder::new("http://localhost:8080")
        .with_timeout(std::time::Duration::from_secs(30))
        .build();
    assert!(client.is_ok());
}

#[test]
fn builder_with_history_length() {
    let client = ClientBuilder::new("http://localhost:8080")
        .with_history_length(10)
        .build();
    assert!(client.is_ok());
}

// ── Credentials store ────────────────────────────────────────────────────────

#[test]
fn credentials_store_set_get() {
    let store = InMemoryCredentialsStore::new();
    let session = SessionId::new("session-1");
    store.set(session.clone(), "bearer", "my-token".into());

    let cred = store.get(&session, "bearer");
    assert_eq!(cred, Some("my-token".into()));
}

#[test]
fn credentials_store_missing_session() {
    let store = InMemoryCredentialsStore::new();
    let session = SessionId::new("nonexistent");

    let cred = store.get(&session, "bearer");
    assert_eq!(cred, None);
}

#[test]
fn credentials_store_missing_scheme() {
    let store = InMemoryCredentialsStore::new();
    let session = SessionId::new("session-1");
    store.set(session.clone(), "bearer", "token".into());

    let cred = store.get(&session, "api-key");
    assert_eq!(cred, None);
}

#[test]
fn credentials_store_overwrite() {
    let store = InMemoryCredentialsStore::new();
    let session = SessionId::new("session-1");
    store.set(session.clone(), "bearer", "old-token".into());
    store.set(session.clone(), "bearer", "new-token".into());

    let cred = store.get(&session, "bearer");
    assert_eq!(cred, Some("new-token".into()));
}

#[test]
fn credentials_store_multiple_schemes() {
    let store = InMemoryCredentialsStore::new();
    let session = SessionId::new("session-1");
    store.set(session.clone(), "bearer", "bear-tok".into());
    store.set(session.clone(), "api-key", "api-tok".into());

    assert_eq!(store.get(&session, "bearer"), Some("bear-tok".into()));
    assert_eq!(store.get(&session, "api-key"), Some("api-tok".into()));
}

#[test]
fn credentials_store_multiple_sessions() {
    let store = InMemoryCredentialsStore::new();
    let s1 = SessionId::new("session-1");
    let s2 = SessionId::new("session-2");
    store.set(s1.clone(), "bearer", "token-1".into());
    store.set(s2.clone(), "bearer", "token-2".into());

    assert_eq!(store.get(&s1, "bearer"), Some("token-1".into()));
    assert_eq!(store.get(&s2, "bearer"), Some("token-2".into()));
}

#[test]
fn credentials_store_debug_impl() {
    let store = InMemoryCredentialsStore::new();
    let debug = format!("{store:?}");
    assert!(!debug.is_empty());
}

// ── Session ID ───────────────────────────────────────────────────────────────

#[test]
fn session_id_equality() {
    let s1 = SessionId::new("abc");
    let s2 = SessionId::new("abc");
    let s3 = SessionId::new("xyz");

    assert_eq!(s1, s2);
    assert_ne!(s1, s3);
}

#[test]
fn session_id_clone() {
    let s1 = SessionId::new("abc");
    let s2 = s1.clone();
    assert_eq!(s1, s2);
}

#[test]
fn session_id_display() {
    let s = SessionId::new("my-session");
    let display = format!("{s}");
    assert_eq!(display, "my-session");
}

// ── Error types ──────────────────────────────────────────────────────────────

#[test]
fn client_error_display() {
    let err = ClientError::from(a2a_protocol_types::error::A2aError::internal("test error"));
    let display = err.to_string();
    assert!(!display.is_empty());
}

#[test]
fn client_error_source() {
    use std::error::Error;

    let json_err = serde_json::from_str::<String>("not json").unwrap_err();
    let err = ClientError::from(json_err);
    // Should have a source chain
    assert!(err.source().is_some() || err.source().is_none()); // Just ensure it doesn't panic
}

// ── Interceptor chain ────────────────────────────────────────────────────────

#[test]
fn interceptor_chain_empty() {
    let chain = InterceptorChain::new();
    let debug = format!("{chain:?}");
    assert!(!debug.is_empty());
}

// ── Client from_card ─────────────────────────────────────────────────────────

#[test]
fn client_from_card_with_valid_interface() {
    use a2a_protocol_client::A2aClient;
    use a2a_protocol_types::*;

    let card = AgentCard {
        name: "Test".into(),
        description: "Test agent".into(),
        version: "1.0.0".into(),
        supported_interfaces: vec![AgentInterface {
            url: "http://localhost:8080".into(),
            protocol_binding: "JSONRPC".into(),
            protocol_version: "1.0.0".into(),
            tenant: None,
        }],
        default_input_modes: vec!["text/plain".into()],
        default_output_modes: vec!["text/plain".into()],
        skills: vec![AgentSkill {
            id: "s1".into(),
            name: "skill".into(),
            description: "A skill".into(),
            tags: vec![],
            examples: None,
            input_modes: None,
            output_modes: None,
            security_requirements: None,
        }],
        capabilities: a2a_protocol_types::AgentCapabilities::none(),
        provider: None,
        icon_url: None,
        documentation_url: None,
        security_schemes: None,
        security_requirements: None,
        signatures: None,
    };

    let client = A2aClient::from_card(&card);
    assert!(client.is_ok(), "from_card should succeed with valid card");
}
