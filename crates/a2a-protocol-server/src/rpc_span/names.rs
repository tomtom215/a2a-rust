// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! What a call is called and how it failed: the binding, the method, and
//! the status each binding puts on the wire.

use std::borrow::Cow;

use a2a_protocol_types::error::A2aError;

use crate::error::ServerError;

/// The binding a call arrived on, as the semantic conventions' `rpc.system.name`.
#[derive(Debug, Clone, Copy)]
pub enum RpcSystem {
    /// JSON-RPC over HTTP, and over WebSocket, which carries the same frames.
    JsonRpc,
    /// The HTTP+JSON binding. Not in the conventions' list, so a custom value,
    /// as they allow ("otherwise, a custom value MAY be used").
    HttpJson,
    /// The gRPC binding.
    #[cfg_attr(not(feature = "grpc"), allow(dead_code))]
    Grpc,
    /// A binding outside this crate, through
    /// [`BindingSpan`](super::BindingSpan).
    Custom(super::CustomBinding),
}

impl RpcSystem {
    /// The `rpc.system.name` attribute value.
    pub const fn name(self) -> &'static str {
        match self {
            Self::JsonRpc => "jsonrpc",
            Self::HttpJson => "a2a_http_json",
            Self::Grpc => "grpc",
            Self::Custom(binding) => binding.name,
        }
    }

    /// Whether a `SERVER` span that ended with wire status `status` failed,
    /// as each binding's semantic conventions count it — which decides
    /// `error.type` and `Error` status, while `rpc.status_code` keeps the
    /// status whatever it is. At `open-telemetry/semantic-conventions@55c98cf`:
    ///
    /// * **JSON-RPC**: every error code. `docs/rpc/json-rpc.md`: "All JSON
    ///   RPC error codes SHOULD be considered errors."
    /// * **gRPC**: only the server-side failures. `docs/rpc/grpc.md`, server
    ///   span, `rpc.status_code` note: "The following status codes SHOULD be
    ///   considered errors: `UNKNOWN`, `DEADLINE_EXCEEDED`, `UNIMPLEMENTED`,
    ///   `INTERNAL`, `UNAVAILABLE`, `DATA_LOSS`". A `NOT_FOUND` `GetTask` is
    ///   the caller's mistake, answered correctly.
    /// * **HTTP+JSON**: a 5xx, not a 4xx. Its status *is* an HTTP status, and
    ///   `docs/http/http-spans.md` says "For HTTP status codes in the 4xx
    ///   range span status MUST be left unset in case of `SpanKind.SERVER`",
    ///   with `error.type` set only for a status that "indicates an error
    ///   according to HTTP span status definition". `rpc-spans.md` leaves
    ///   which codes are errors to each system's conventions, and this
    ///   custom system's are HTTP's.
    /// * **A custom binding**: every status, unless it asked for gRPC's rule
    ///   ([`CustomBinding::with_grpc_server_errors`](super::CustomBinding::with_grpc_server_errors)).
    pub fn is_server_error(self, status: &str) -> bool {
        match self {
            Self::JsonRpc => true,
            Self::HttpJson => status.parse::<u16>().ok().is_none_or(|code| code >= 500),
            Self::Grpc => grpc_server_error(status),
            Self::Custom(binding) => (binding.server_error)(status),
        }
    }
}

/// gRPC's server-side failures (`docs/rpc/grpc.md`): the statuses a server
/// span counts as errors.
pub fn grpc_server_error(status: &str) -> bool {
    matches!(
        status,
        "UNKNOWN"
            | "DEADLINE_EXCEEDED"
            | "UNIMPLEMENTED"
            | "INTERNAL"
            | "UNAVAILABLE"
            | "DATA_LOSS"
    )
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
            RpcSystem::Custom(binding) => (binding.error_status)(self),
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
        // Not the v0.3 `message/stream`: no binding routes it — the WebSocket
        // binding stopped on 2026-10-12, the JSON-RPC one never did — so a
        // call of it is refused `MethodNotFound` and recorded as `_OTHER`,
        // with `rpc.method_original`, like every other method not served.
        "SendStreamingMessage" => "lf.a2a.v1.A2AService/SendStreamingMessage",
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
