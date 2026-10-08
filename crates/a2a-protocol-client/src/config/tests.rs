// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Tests for [`ClientConfig`](super::ClientConfig) and its defaults.

use super::*;

#[test]
fn default_config_has_jsonrpc_binding() {
    let cfg = ClientConfig::default();
    assert_eq!(cfg.preferred_bindings, vec![BINDING_JSONRPC]);
}

#[test]
fn default_config_timeout() {
    let cfg = ClientConfig::default();
    assert_eq!(cfg.request_timeout, Duration::from_secs(30));
}

#[test]
fn default_http_config_is_disabled_tls() {
    let cfg = ClientConfig::default_http();
    assert!(matches!(cfg.tls, TlsConfig::Disabled));
}

#[test]
fn default_http_config_field_values() {
    let cfg = ClientConfig::default_http();
    assert_eq!(cfg.preferred_bindings, vec![BINDING_JSONRPC]);
    assert_eq!(
        cfg.accepted_output_modes,
        vec!["text/plain", "application/json"]
    );
    assert!(cfg.history_length.is_none());
    assert!(!cfg.return_immediately);
    assert_eq!(cfg.request_timeout, Duration::from_secs(30));
    assert_eq!(cfg.stream_connect_timeout, Duration::from_secs(30));
    assert_eq!(cfg.connection_timeout, Duration::from_secs(10));
}

#[test]
fn default_config_field_values() {
    let cfg = ClientConfig::default();
    assert_eq!(cfg.preferred_bindings, vec![BINDING_JSONRPC]);
    assert_eq!(
        cfg.accepted_output_modes,
        vec!["text/plain", "application/json"]
    );
    assert!(cfg.history_length.is_none());
    assert!(!cfg.return_immediately);
    assert_eq!(cfg.request_timeout, Duration::from_secs(30));
    assert_eq!(cfg.stream_connect_timeout, Duration::from_secs(30));
    assert_eq!(cfg.connection_timeout, Duration::from_secs(10));
    assert_eq!(cfg.stream_idle_timeout, Some(DEFAULT_STREAM_IDLE_TIMEOUT));
    assert_eq!(
        ClientConfig::default_http().stream_idle_timeout,
        Some(DEFAULT_STREAM_IDLE_TIMEOUT)
    );
    assert_eq!(DEFAULT_STREAM_IDLE_TIMEOUT, Duration::from_secs(300));
    assert_eq!(
        cfg.stream_first_event_timeout,
        DEFAULT_STREAM_FIRST_EVENT_TIMEOUT
    );
    assert_eq!(
        ClientConfig::default_http().stream_first_event_timeout,
        DEFAULT_STREAM_FIRST_EVENT_TIMEOUT
    );
    assert_eq!(DEFAULT_STREAM_FIRST_EVENT_TIMEOUT, Duration::from_secs(300));
    assert_eq!(cfg.max_event_size, 16 * 1024 * 1024);
    assert_eq!(
        ClientConfig::default_http().max_event_size,
        16 * 1024 * 1024
    );
}

#[test]
fn binding_constants_values() {
    assert_eq!(BINDING_JSONRPC, "JSONRPC");
    assert_eq!(BINDING_HTTP_JSON, "HTTP+JSON");
    assert_eq!(BINDING_REST, "REST");
    assert_eq!(BINDING_GRPC, "GRPC");
}

/// Every setter writes its own field and nothing else. One assertion per
/// field against a value that differs from the default, so a setter whose
/// body became `Default::default()` (cargo-mutants' replacement) fails
/// here; the incremental gate found three such setters unpinned on
/// 2026-09-10.
#[test]
fn every_setter_sets_its_field() {
    let cfg = ClientConfig::default()
        .with_preferred_bindings(vec!["GRPC".to_owned()])
        .with_accepted_output_modes(vec!["image/png".to_owned()])
        .with_history_length(Some(7))
        .with_return_immediately(true)
        .with_request_timeout(Duration::from_secs(1))
        .with_stream_connect_timeout(Duration::from_secs(2))
        .with_stream_first_event_timeout(Duration::from_secs(6))
        .with_connection_timeout(Duration::from_secs(3))
        .with_stream_idle_timeout(Some(Duration::from_secs(5)))
        .with_max_response_size(4)
        .with_max_event_size(9)
        .with_tls(TlsConfig::Disabled)
        .with_tenant(Some("acme".to_owned()));
    assert_eq!(cfg.preferred_bindings, vec!["GRPC".to_owned()]);
    assert_eq!(cfg.accepted_output_modes, vec!["image/png".to_owned()]);
    assert_eq!(cfg.history_length, Some(7));
    assert!(cfg.return_immediately);
    assert_eq!(cfg.request_timeout, Duration::from_secs(1));
    assert_eq!(cfg.stream_connect_timeout, Duration::from_secs(2));
    assert_eq!(cfg.stream_first_event_timeout, Duration::from_secs(6));
    assert_eq!(cfg.connection_timeout, Duration::from_secs(3));
    assert_eq!(cfg.stream_idle_timeout, Some(Duration::from_secs(5)));
    assert_eq!(cfg.max_response_size, 4);
    assert_eq!(cfg.max_event_size, 9);
    assert!(matches!(cfg.tls, TlsConfig::Disabled));
    assert_eq!(cfg.tenant.as_deref(), Some("acme"));
}
