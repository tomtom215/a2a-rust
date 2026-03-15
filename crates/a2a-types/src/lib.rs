// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! A2A protocol 0.3.0 — pure data types with serde support.
//!
//! This crate provides all wire types for the A2A protocol with zero I/O
//! dependencies. Add `a2a-client` or `a2a-server` for HTTP transport.
//!
//! # Module overview
//!
//! | Module | Contents |
//! |---|---|
//! | [`error`] | [`error::A2aError`], [`error::ErrorCode`], [`error::A2aResult`] |
//! | [`task`] | [`task::Task`], [`task::TaskStatus`], [`task::TaskState`], ID newtypes |
//! | [`message`] | [`message::Message`], [`message::Part`], file/text/data parts |
//! | [`artifact`] | [`artifact::Artifact`], [`artifact::ArtifactId`] |
//! | [`agent_card`] | [`agent_card::AgentCard`], capabilities, skills |
//! | [`security`] | [`security::SecurityScheme`] variants, OAuth flows |
//! | [`events`] | [`events::StreamResponse`], status/artifact update events |
//! | [`jsonrpc`] | [`jsonrpc::JsonRpcRequest`], [`jsonrpc::JsonRpcResponse`] |
//! | [`params`] | Method parameter structs |
//! | [`push`] | [`push::PushNotificationConfig`] |
//! | [`extensions`] | [`extensions::AgentExtension`], [`extensions::AgentCardSignature`] |
//! | [`responses`] | [`responses::SendMessageResponse`], [`responses::TaskListResponse`] |

#![warn(missing_docs)]
#![deny(unsafe_op_in_unsafe_fn)]
#![warn(clippy::all, clippy::pedantic, clippy::nursery)]
#![allow(clippy::module_name_repetitions)]

pub mod agent_card;
pub mod artifact;
pub mod error;
pub mod events;
pub mod extensions;
pub mod jsonrpc;
pub mod message;
pub mod params;
pub mod push;
pub mod responses;
pub mod security;
pub mod task;

// ── Flat re-exports ───────────────────────────────────────────────────────────

pub use agent_card::{
    AgentCapabilities, AgentCard, AgentInterface, AgentProvider, AgentSkill, TransportProtocol,
};
pub use artifact::{Artifact, ArtifactId};
pub use error::{A2aError, A2aResult, ErrorCode};
pub use events::{StreamResponse, TaskArtifactUpdateEvent, TaskStatusUpdateEvent};
pub use extensions::{AgentCardSignature, AgentExtension};
pub use jsonrpc::{
    JsonRpcError, JsonRpcErrorResponse, JsonRpcId, JsonRpcRequest, JsonRpcResponse,
    JsonRpcSuccessResponse, JsonRpcVersion,
};
pub use message::{
    DataPart, FileContent, FilePart, FileWithBytes, FileWithUri, Message, MessageId, MessageRole,
    Part, TextPart,
};
pub use params::{
    DeletePushConfigParams, GetPushConfigParams, ListTasksParams, MessageSendParams,
    SendMessageConfiguration, TaskIdParams, TaskQueryParams,
};
pub use push::{PushNotificationAuthInfo, PushNotificationConfig, TaskPushNotificationConfig};
pub use responses::{AuthenticatedExtendedCardResponse, SendMessageResponse, TaskListResponse};
pub use security::{
    ApiKeyLocation, ApiKeySecurityScheme, AuthorizationCodeFlow, ClientCredentialsFlow,
    DeviceCodeFlow, HttpAuthSecurityScheme, ImplicitFlow, MutualTlsSecurityScheme,
    NamedSecuritySchemes, OAuth2SecurityScheme, OAuthFlows, OpenIdConnectSecurityScheme,
    SecurityRequirements, SecurityScheme,
};
pub use task::{ContextId, Task, TaskId, TaskState, TaskStatus, TaskVersion};
