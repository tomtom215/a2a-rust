// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! A2A protocol 0.3.0 — HTTP client (hyper-backed).
//!
//! This crate provides [`A2aClient`], a full-featured client for communicating
//! with any A2A-compliant agent over HTTP.
//!
//! # Quick start
//!
//! ```rust,no_run
//! use a2a_client::ClientBuilder;
//! use a2a_types::{MessageSendParams, Message, MessageRole, Part, TextPart, MessageId};
//!
//! # async fn example() -> Result<(), a2a_client::error::ClientError> {
//! let client = ClientBuilder::new("http://localhost:8080").build()?;
//!
//! let params = MessageSendParams {
//!     message: Message {
//!         id: MessageId::new("msg-1"),
//!         role: MessageRole::User,
//!         parts: vec![Part::Text(TextPart::new("Hello, agent!"))],
//!         task_id: None,
//!         context_id: None,
//!         reference_task_ids: None,
//!         extensions: None,
//!         metadata: None,
//!     },
//!     configuration: None,
//!     metadata: None,
//! };
//!
//! let response = client.send_message(params).await?;
//! println!("{response:?}");
//! # Ok(())
//! # }
//! ```
//!
//! # Streaming
//!
//! ```rust,no_run
//! # use a2a_client::ClientBuilder;
//! # use a2a_types::{MessageSendParams, Message, MessageRole, Part, TextPart, MessageId, StreamResponse};
//! # async fn example() -> Result<(), a2a_client::error::ClientError> {
//! # let client = ClientBuilder::new("http://localhost:8080").build()?;
//! # let params = MessageSendParams {
//! #     message: Message { id: MessageId::new("m"), role: MessageRole::User,
//! #         parts: vec![], task_id: None, context_id: None,
//! #         reference_task_ids: None, extensions: None, metadata: None },
//! #     configuration: None, metadata: None,
//! # };
//! let mut stream = client.stream_message(params).await?;
//! while let Some(event) = stream.next().await {
//!     match event? {
//!         StreamResponse::StatusUpdate(ev) => {
//!             println!("State: {:?}", ev.state);
//!             if ev.r#final { break; }
//!         }
//!         _ => {}
//!     }
//! }
//! # Ok(())
//! # }
//! ```
//!
//! # Authentication
//!
//! ```rust,no_run
//! use a2a_client::{ClientBuilder, CredentialsStore};
//! use a2a_client::auth::{AuthInterceptor, InMemoryCredentialsStore, SessionId};
//! use std::sync::Arc;
//!
//! # fn example() -> Result<(), a2a_client::error::ClientError> {
//! let store = Arc::new(InMemoryCredentialsStore::new());
//! let session = SessionId::new("session-1");
//! store.set(session.clone(), "bearer", "my-token".into());
//!
//! let client = ClientBuilder::new("http://localhost:8080")
//!     .with_interceptor(AuthInterceptor::new(store, session))
//!     .build()?;
//! # Ok(())
//! # }
//! ```
//!
//! # Agent card discovery
//!
//! ```rust,no_run
//! use a2a_client::discovery::resolve_agent_card;
//! use a2a_client::A2aClient;
//!
//! # async fn example() -> Result<(), a2a_client::error::ClientError> {
//! let card = resolve_agent_card("http://localhost:8080").await?;
//! let client = A2aClient::from_card(&card)?;
//! # Ok(())
//! # }
//! ```

#![warn(missing_docs)]
#![deny(unsafe_op_in_unsafe_fn)]
#![warn(clippy::all, clippy::pedantic, clippy::nursery)]
#![allow(clippy::module_name_repetitions)]

// ── Modules ───────────────────────────────────────────────────────────────────

pub mod auth;
pub mod builder;
pub mod client;
pub mod config;
pub mod discovery;
pub mod error;
pub mod interceptor;
pub mod methods;
pub mod streaming;
pub mod transport;

// ── Flat re-exports ───────────────────────────────────────────────────────────

pub use auth::{AuthInterceptor, CredentialsStore, InMemoryCredentialsStore, SessionId};
pub use builder::ClientBuilder;
pub use client::A2aClient;
pub use config::ClientConfig;
pub use discovery::resolve_agent_card;
pub use error::{ClientError, ClientResult};
pub use interceptor::{CallInterceptor, ClientRequest, ClientResponse, InterceptorChain};
pub use streaming::EventStream;
pub use transport::{JsonRpcTransport, RestTransport, Transport};
