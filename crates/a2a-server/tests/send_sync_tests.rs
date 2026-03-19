// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Compile-time assertions that all public types implement Send + Sync.
//!
//! These tests don't run at runtime — they verify at compile time that the
//! types can be shared across threads, which is essential for async runtimes.

fn assert_send_sync<T: Send + Sync>() {}

#[test]
fn server_types_are_send_sync() {
    // Core handler types
    assert_send_sync::<a2a_protocol_server::RequestHandler>();
    assert_send_sync::<a2a_protocol_server::RequestHandlerBuilder>();
    assert_send_sync::<a2a_protocol_server::JsonRpcDispatcher>();
    assert_send_sync::<a2a_protocol_server::RestDispatcher>();

    // Agent card handlers
    assert_send_sync::<a2a_protocol_server::StaticAgentCardHandler>();

    // Store types
    assert_send_sync::<a2a_protocol_server::InMemoryTaskStore>();
    assert_send_sync::<a2a_protocol_server::InMemoryPushConfigStore>();

    // Error types
    assert_send_sync::<a2a_protocol_server::ServerError>();

    // Config types
    assert_send_sync::<a2a_protocol_server::CorsConfig>();
    assert_send_sync::<a2a_protocol_server::TaskStoreConfig>();

    // Streaming types
    assert_send_sync::<a2a_protocol_server::EventQueueManager>();
    assert_send_sync::<a2a_protocol_server::InMemoryQueueReader>();
    assert_send_sync::<a2a_protocol_server::InMemoryQueueWriter>();
}

#[test]
fn types_types_are_send_sync() {
    // Core protocol types
    assert_send_sync::<a2a_protocol_types::task::Task>();
    assert_send_sync::<a2a_protocol_types::task::TaskStatus>();
    assert_send_sync::<a2a_protocol_types::task::TaskState>();
    assert_send_sync::<a2a_protocol_types::task::TaskId>();
    assert_send_sync::<a2a_protocol_types::task::ContextId>();

    // Message types
    assert_send_sync::<a2a_protocol_types::message::Message>();
    assert_send_sync::<a2a_protocol_types::message::Part>();
    assert_send_sync::<a2a_protocol_types::message::MessageRole>();

    // Agent card types
    assert_send_sync::<a2a_protocol_types::agent_card::AgentCard>();
    assert_send_sync::<a2a_protocol_types::agent_card::AgentCapabilities>();
    assert_send_sync::<a2a_protocol_types::agent_card::AgentInterface>();
    assert_send_sync::<a2a_protocol_types::agent_card::AgentSkill>();

    // Event types
    assert_send_sync::<a2a_protocol_types::events::StreamResponse>();
    assert_send_sync::<a2a_protocol_types::events::TaskStatusUpdateEvent>();
    assert_send_sync::<a2a_protocol_types::events::TaskArtifactUpdateEvent>();

    // JSON-RPC types
    assert_send_sync::<a2a_protocol_types::jsonrpc::JsonRpcRequest>();
    assert_send_sync::<a2a_protocol_types::jsonrpc::JsonRpcError>();
    assert_send_sync::<a2a_protocol_types::jsonrpc::JsonRpcVersion>();

    // Error types
    assert_send_sync::<a2a_protocol_types::error::A2aError>();
    assert_send_sync::<a2a_protocol_types::error::ErrorCode>();

    // Push types
    assert_send_sync::<a2a_protocol_types::push::TaskPushNotificationConfig>();
    assert_send_sync::<a2a_protocol_types::push::AuthenticationInfo>();

    // Param types
    assert_send_sync::<a2a_protocol_types::params::MessageSendParams>();
    assert_send_sync::<a2a_protocol_types::params::TaskQueryParams>();
    assert_send_sync::<a2a_protocol_types::params::ListTasksParams>();
    assert_send_sync::<a2a_protocol_types::params::CancelTaskParams>();
}
