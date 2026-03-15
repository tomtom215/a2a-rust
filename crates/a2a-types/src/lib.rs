// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! A2A protocol v1.0 — pure data types with serde support.
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
//! | [`message`] | [`message::Message`], [`message::Part`], [`message::PartContent`] |
//! | [`artifact`] | [`artifact::Artifact`], [`artifact::ArtifactId`] |
//! | [`agent_card`] | [`agent_card::AgentCard`], capabilities, skills |
//! | [`security`] | [`security::SecurityScheme`] variants, OAuth flows |
//! | [`events`] | [`events::StreamResponse`], status/artifact update events |
//! | [`jsonrpc`] | [`jsonrpc::JsonRpcRequest`], [`jsonrpc::JsonRpcResponse`] |
//! | [`params`] | Method parameter structs |
//! | [`push`] | [`push::TaskPushNotificationConfig`] |
//! | [`extensions`] | [`extensions::AgentExtension`], [`extensions::AgentCardSignature`] |
//! | [`responses`] | [`responses::SendMessageResponse`], [`responses::TaskListResponse`] |

#![deny(missing_docs)]
#![deny(unsafe_op_in_unsafe_fn)]
#![warn(clippy::all, clippy::pedantic, clippy::nursery)]
#![allow(clippy::module_name_repetitions)]

// ── Protocol constants ────────────────────────────────────────────────────────

/// A2A protocol version string.
pub const A2A_VERSION: &str = "1.0.0";

/// A2A-specific content type for JSON payloads.
pub const A2A_CONTENT_TYPE: &str = "application/a2a+json";

/// HTTP header name for the A2A protocol version.
pub const A2A_VERSION_HEADER: &str = "A2A-Version";

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
#[cfg(feature = "signing")]
pub mod signing;
pub mod task;

// ── Flat re-exports ───────────────────────────────────────────────────────────

pub use agent_card::{AgentCapabilities, AgentCard, AgentInterface, AgentProvider, AgentSkill};
pub use artifact::{Artifact, ArtifactId};
pub use error::{A2aError, A2aResult, ErrorCode};
pub use events::{StreamResponse, TaskArtifactUpdateEvent, TaskStatusUpdateEvent};
pub use extensions::{AgentCardSignature, AgentExtension};
pub use jsonrpc::{
    JsonRpcError, JsonRpcErrorResponse, JsonRpcId, JsonRpcRequest, JsonRpcResponse,
    JsonRpcSuccessResponse, JsonRpcVersion,
};
pub use message::{Message, MessageId, MessageRole, Part, PartContent};
pub use params::{
    CancelTaskParams, DeletePushConfigParams, GetExtendedAgentCardParams, GetPushConfigParams,
    ListPushConfigsParams, ListTasksParams, MessageSendParams, SendMessageConfiguration,
    TaskIdParams, TaskQueryParams,
};
pub use push::{AuthenticationInfo, TaskPushNotificationConfig};
pub use responses::{
    AuthenticatedExtendedCardResponse, ListPushConfigsResponse, SendMessageResponse,
    TaskListResponse,
};
pub use security::{
    ApiKeyLocation, ApiKeySecurityScheme, AuthorizationCodeFlow, ClientCredentialsFlow,
    DeviceCodeFlow, HttpAuthSecurityScheme, ImplicitFlow, MutualTlsSecurityScheme,
    NamedSecuritySchemes, OAuth2SecurityScheme, OAuthFlows, OpenIdConnectSecurityScheme,
    PasswordOAuthFlow, SecurityRequirement, SecurityScheme, StringList,
};
pub use task::{ContextId, Task, TaskId, TaskState, TaskStatus, TaskVersion};

// ── Utilities ─────────────────────────────────────────────────────────────

/// Returns the current UTC time as an ISO 8601 string (e.g. `"2026-03-15T12:00:00Z"`).
///
/// Uses [`std::time::SystemTime`] — no external dependency required.
#[must_use]
pub fn utc_now_iso8601() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    // Decompose seconds into y/m/d H:M:S — simplified UTC-only implementation.
    let (y, m, d, hh, mm, ss) = secs_to_ymd_hms(secs);
    format!("{y:04}-{m:02}-{d:02}T{hh:02}:{mm:02}:{ss:02}Z")
}

/// Converts UNIX epoch seconds to (year, month, day, hour, minute, second).
const fn secs_to_ymd_hms(epoch: u64) -> (u64, u64, u64, u64, u64, u64) {
    let secs_per_day = 86400_u64;
    let mut days = epoch / secs_per_day;
    let time_of_day = epoch % secs_per_day;
    let hh = time_of_day / 3600;
    let mm = (time_of_day % 3600) / 60;
    let ss = time_of_day % 60;

    // Civil date from day count (days since 1970-01-01).
    // Algorithm from Howard Hinnant.
    days += 719_468;
    let era = days / 146_097;
    let doe = days - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d, hh, mm, ss)
}
