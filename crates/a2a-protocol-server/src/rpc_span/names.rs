// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! What a call is called and how it failed: the binding, the method, and
//! the status each binding puts on the wire.

use std::borrow::Cow;

use a2a_protocol_types::error::A2aError;

use crate::error::ServerError;

/// The binding a call arrived on, as the semantic conventions' `rpc.system.name`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RpcSystem {
    /// JSON-RPC over HTTP, and over WebSocket, which carries the same frames.
    JsonRpc,
    /// The HTTP+JSON binding. Not in the conventions' list, so a custom value,
    /// as they allow ("otherwise, a custom value MAY be used").
    HttpJson,
    /// The gRPC binding.
    #[cfg_attr(not(feature = "grpc"), allow(dead_code))]
    Grpc,
}

impl RpcSystem {
    /// The `rpc.system.name` attribute value.
    pub const fn name(self) -> &'static str {
        match self {
            Self::JsonRpc => "jsonrpc",
            Self::HttpJson => "a2a_http_json",
            Self::Grpc => "grpc",
        }
    }
}

/// An error a binding sends, as the status code that binding puts on the wire.
pub trait WireStatus {
    /// The `rpc.status_code` — and so the `error.type` — of a call that failed
    /// with this error on `system`.
    fn wire_status(&self, system: RpcSystem) -> Cow<'static, str>;
}

impl WireStatus for ServerError {
    fn wire_status(&self, system: RpcSystem) -> Cow<'static, str> {
        match system {
            // What `error_response` and `error_response_bytes` send.
            RpcSystem::JsonRpc => Cow::Owned(self.to_a2a_error().code.as_i32().to_string()),
            RpcSystem::HttpJson => Cow::Owned(self.http_status().to_string()),
            #[cfg(feature = "grpc")]
            RpcSystem::Grpc => Cow::Borrowed(crate::dispatch::grpc::grpc_status_name(self)),
            #[cfg(not(feature = "grpc"))]
            RpcSystem::Grpc => Cow::Borrowed("INTERNAL"),
        }
    }
}

impl WireStatus for A2aError {
    fn wire_status(&self, system: RpcSystem) -> Cow<'static, str> {
        // `Protocol` carries the error unchanged, so on JSON-RPC this is the
        // code as is — the WebSocket binding's `send_error` path. A separate
        // arm for it said the same thing and was an equivalent mutant.
        ServerError::Protocol(self.clone()).wire_status(system)
    }
}

/// The A2A method a call names, fully qualified as the gRPC service spells
/// it, or `_OTHER` for one this server does not serve — the conventions'
/// value for a method the server does not recognise, which also keeps an
/// attacker's method string out of span names and metric attributes.
pub fn a2a_method(method: &str) -> &'static str {
    match method {
        "SendMessage" => "lf.a2a.v1.A2AService/SendMessage",
        // The WebSocket binding also routes this v0.3 spelling.
        "SendStreamingMessage" | "message/stream" => "lf.a2a.v1.A2AService/SendStreamingMessage",
        "GetTask" => "lf.a2a.v1.A2AService/GetTask",
        "ListTasks" => "lf.a2a.v1.A2AService/ListTasks",
        "CancelTask" => "lf.a2a.v1.A2AService/CancelTask",
        "SubscribeToTask" => "lf.a2a.v1.A2AService/SubscribeToTask",
        "CreateTaskPushNotificationConfig" => {
            "lf.a2a.v1.A2AService/CreateTaskPushNotificationConfig"
        }
        "GetTaskPushNotificationConfig" => "lf.a2a.v1.A2AService/GetTaskPushNotificationConfig",
        "ListTaskPushNotificationConfigs" => "lf.a2a.v1.A2AService/ListTaskPushNotificationConfigs",
        "DeleteTaskPushNotificationConfig" => {
            "lf.a2a.v1.A2AService/DeleteTaskPushNotificationConfig"
        }
        "GetExtendedAgentCard" => "lf.a2a.v1.A2AService/GetExtendedAgentCard",
        _ => OTHER,
    }
}

/// The conventions' `rpc.method` for a method the server does not recognise.
pub const OTHER: &str = "_OTHER";
