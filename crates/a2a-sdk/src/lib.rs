// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! A2A protocol v1.0 — umbrella SDK crate.
//!
//! Re-exports all three constituent crates so users who want everything can
//! depend on `a2a-sdk` alone.
//!
//! # Quick start
//!
//! Use the [`prelude`] module to pull in the most common types:
//!
//! ```rust
//! use a2a_sdk::prelude::*;
//! ```
//!
//! # Module overview
//!
//! | Module | Source crate | Contents |
//! |---|---|---|
//! | [`types`] | `a2a-types` | All A2A wire types |
//! | [`client`] | `a2a-client` | HTTP client |
//! | [`server`] | `a2a-server` | Server framework |
//! | [`prelude`] | — | Convenience re-exports for common usage |

#![deny(missing_docs)]
#![deny(unsafe_op_in_unsafe_fn)]
#![warn(clippy::all, clippy::pedantic, clippy::nursery)]
#![allow(clippy::module_name_repetitions)]

/// All A2A protocol wire types.
pub mod types {
    #[allow(unused_imports)]
    pub use a2a_types::*;
}

/// HTTP client for sending A2A requests.
pub mod client {
    #[allow(unused_imports)]
    pub use a2a_client::*;
}

/// Server framework for implementing A2A agents.
pub mod server {
    #[allow(unused_imports)]
    pub use a2a_server::*;
}

/// Convenience re-exports for common A2A usage patterns.
///
/// Import with `use a2a_sdk::prelude::*` to get the most frequently used
/// types for building agents and clients:
///
/// - **Wire types**: `Task`, `TaskState`, `TaskStatus`, `Message`, `Part`,
///   `MessageRole`, `Artifact`, `StreamResponse`, `AgentCard`, `AgentInterface`
/// - **ID newtypes**: `TaskId`, `ContextId`, `MessageId`, `ArtifactId`
/// - **Params**: `MessageSendParams`, `TaskQueryParams`, `ListTasksParams`
/// - **Responses**: `SendMessageResponse`, `TaskListResponse`
/// - **Client**: `A2aClient`, `ClientBuilder`, `EventStream`
/// - **Server**: `AgentExecutor`, `RequestHandler`, `RequestHandlerBuilder`,
///   `RequestContext`, `EventQueueWriter`, `JsonRpcDispatcher`, `RestDispatcher`
/// - **Errors**: `A2aError`, `A2aResult`, `ClientError`, `ServerError`
pub mod prelude {
    // ── Wire types ───────────────────────────────────────────────────────
    pub use a2a_types::{
        AgentCapabilities, AgentCard, AgentInterface, AgentSkill, Artifact, ArtifactId, ContextId,
        Message, MessageId, MessageRole, MessageSendParams, Part, SendMessageResponse,
        StreamResponse, Task, TaskArtifactUpdateEvent, TaskId, TaskListResponse, TaskQueryParams,
        TaskState, TaskStatus, TaskStatusUpdateEvent,
    };

    // ── Errors ───────────────────────────────────────────────────────────
    pub use a2a_types::{A2aError, A2aResult};

    // ── Client ───────────────────────────────────────────────────────────
    pub use a2a_client::{A2aClient, ClientBuilder, ClientError, ClientResult, EventStream};

    // ── Server ───────────────────────────────────────────────────────────
    pub use a2a_server::{
        AgentExecutor, EventQueueWriter, JsonRpcDispatcher, RequestContext, RequestHandler,
        RequestHandlerBuilder, RestDispatcher, ServerError, ServerResult,
    };
}
