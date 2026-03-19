// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Tests for the gRPC dispatcher (`dispatch::grpc`).

#![cfg(feature = "grpc")]

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use a2a_protocol_types::agent_card::{AgentCapabilities, AgentCard, AgentInterface, AgentSkill};
use a2a_protocol_types::error::A2aResult;

use a2a_protocol_server::builder::RequestHandlerBuilder;
use a2a_protocol_server::dispatch::grpc::{GrpcConfig, GrpcDispatcher};
use a2a_protocol_server::executor::AgentExecutor;
use a2a_protocol_server::request_context::RequestContext;
use a2a_protocol_server::streaming::EventQueueWriter;

// ── Test executor ───────────────────────────────────────────────────────────

struct NoopExecutor;

impl AgentExecutor for NoopExecutor {
    fn execute<'a>(
        &'a self,
        _ctx: &'a RequestContext,
        _queue: &'a dyn EventQueueWriter,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async { Ok(()) })
    }
}

// ── Helpers ─────────────────────────────────────────────────────────────────

fn minimal_agent_card() -> AgentCard {
    AgentCard {
        url: None,
        name: "gRPC Test Agent".into(),
        description: "A gRPC test agent".into(),
        version: "1.0.0".into(),
        supported_interfaces: vec![AgentInterface {
            url: "grpc://localhost:50051".into(),
            protocol_binding: "gRPC".into(),
            protocol_version: "1.0.0".into(),
            tenant: None,
        }],
        default_input_modes: vec!["text/plain".into()],
        default_output_modes: vec!["text/plain".into()],
        skills: vec![AgentSkill {
            id: "noop".into(),
            name: "Noop".into(),
            description: "Does nothing".into(),
            tags: vec!["test".into()],
            examples: None,
            input_modes: None,
            output_modes: None,
            security_requirements: None,
        }],
        capabilities: AgentCapabilities::none(),
        provider: None,
        icon_url: None,
        documentation_url: None,
        security_schemes: None,
        security_requirements: None,
        signatures: None,
    }
}

fn build_handler() -> Arc<a2a_protocol_server::handler::RequestHandler> {
    Arc::new(
        RequestHandlerBuilder::new(NoopExecutor)
            .with_agent_card(minimal_agent_card())
            .build()
            .expect("build handler"),
    )
}

// ── GrpcConfig tests ────────────────────────────────────────────────────────

#[test]
fn default_values() {
    let config = GrpcConfig::default();
    assert_eq!(config.max_message_size, 4 * 1024 * 1024);
    assert_eq!(config.concurrency_limit, 256);
    assert_eq!(config.stream_channel_capacity, 64);
}

#[test]
fn with_max_message_size() {
    let config = GrpcConfig::default().with_max_message_size(8 * 1024 * 1024);
    assert_eq!(config.max_message_size, 8 * 1024 * 1024);
    // Other fields remain at defaults.
    assert_eq!(config.concurrency_limit, 256);
    assert_eq!(config.stream_channel_capacity, 64);
}

#[test]
fn with_concurrency_limit() {
    let config = GrpcConfig::default().with_concurrency_limit(512);
    assert_eq!(config.concurrency_limit, 512);
    assert_eq!(config.max_message_size, 4 * 1024 * 1024);
    assert_eq!(config.stream_channel_capacity, 64);
}

#[test]
fn with_stream_channel_capacity() {
    let config = GrpcConfig::default().with_stream_channel_capacity(128);
    assert_eq!(config.stream_channel_capacity, 128);
    assert_eq!(config.max_message_size, 4 * 1024 * 1024);
    assert_eq!(config.concurrency_limit, 256);
}

#[test]
fn builder_chaining() {
    let config = GrpcConfig::default()
        .with_max_message_size(16 * 1024 * 1024)
        .with_concurrency_limit(1024)
        .with_stream_channel_capacity(256);
    assert_eq!(config.max_message_size, 16 * 1024 * 1024);
    assert_eq!(config.concurrency_limit, 1024);
    assert_eq!(config.stream_channel_capacity, 256);
}

// ── GrpcDispatcher tests ────────────────────────────────────────────────────

#[test]
fn debug_format() {
    let handler = build_handler();
    let dispatcher = GrpcDispatcher::new(handler, GrpcConfig::default());
    let debug_str = format!("{dispatcher:?}");
    assert!(
        debug_str.contains("GrpcDispatcher"),
        "Debug output should contain 'GrpcDispatcher', got: {debug_str}"
    );
    assert!(
        debug_str.contains("config"),
        "Debug output should contain 'config', got: {debug_str}"
    );
}

#[test]
fn into_service_creates_server() {
    let handler = build_handler();
    let dispatcher = GrpcDispatcher::new(handler, GrpcConfig::default());
    // Calling into_service() should not panic and should return a valid
    // A2aServiceServer that can be added to a tonic Server.
    let _svc = dispatcher.into_service();
}

// ── serve_with_listener tests ───────────────────────────────────────────────

#[tokio::test]
async fn serve_with_listener_returns_correct_address() {
    let handler = build_handler();
    let dispatcher = GrpcDispatcher::new(handler, GrpcConfig::default());

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind listener");
    let expected_addr = listener.local_addr().expect("local addr");

    let addr = dispatcher
        .serve_with_listener(listener)
        .expect("serve_with_listener");

    assert_eq!(addr, expected_addr);
    assert!(addr.port() > 0, "should have a non-zero port");
}

#[tokio::test]
async fn serve_with_addr_returns_bound_address() {
    let handler = build_handler();
    let dispatcher = GrpcDispatcher::new(handler, GrpcConfig::default());

    let addr = dispatcher
        .serve_with_addr("127.0.0.1:0")
        .await
        .expect("serve_with_addr");

    assert_eq!(addr.ip(), std::net::Ipv4Addr::LOCALHOST);
    assert!(addr.port() > 0, "should have a non-zero port");
}

// ── GrpcConfig Clone & Debug ────────────────────────────────────────────────

#[test]
fn config_clone() {
    let config = GrpcConfig::default()
        .with_max_message_size(10)
        .with_concurrency_limit(20)
        .with_stream_channel_capacity(30);
    let cloned = config.clone();
    assert_eq!(cloned.max_message_size, 10);
    assert_eq!(cloned.concurrency_limit, 20);
    assert_eq!(cloned.stream_channel_capacity, 30);
}

#[test]
fn config_debug() {
    let config = GrpcConfig::default();
    let debug_str = format!("{config:?}");
    assert!(
        debug_str.contains("GrpcConfig"),
        "Debug output should contain 'GrpcConfig', got: {debug_str}"
    );
    assert!(
        debug_str.contains("max_message_size"),
        "Debug output should contain 'max_message_size', got: {debug_str}"
    );
}

// ── into_service with custom config ─────────────────────────────────────────

#[test]
fn into_service_respects_custom_config() {
    let handler = build_handler();
    let config = GrpcConfig::default()
        .with_max_message_size(1024)
        .with_concurrency_limit(8)
        .with_stream_channel_capacity(4);
    let dispatcher = GrpcDispatcher::new(handler, config);
    // Should not panic even with small limits.
    let _svc = dispatcher.into_service();
}

// ── Multiple dispatchers from same handler ──────────────────────────────────

#[tokio::test]
async fn multiple_dispatchers_from_same_handler() {
    let handler = build_handler();

    let d1 = GrpcDispatcher::new(Arc::clone(&handler), GrpcConfig::default());
    let d2 = GrpcDispatcher::new(
        Arc::clone(&handler),
        GrpcConfig::default().with_concurrency_limit(32),
    );

    let listener1 = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind listener 1");
    let listener2 = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind listener 2");

    let addr1 = d1.serve_with_listener(listener1).expect("serve 1");
    let addr2 = d2.serve_with_listener(listener2).expect("serve 2");

    assert_ne!(addr1.port(), addr2.port(), "should bind to different ports");
}
