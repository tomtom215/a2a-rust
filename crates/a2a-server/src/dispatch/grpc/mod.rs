// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! gRPC dispatcher for the A2A server.
//!
//! [`GrpcDispatcher`] implements the tonic-generated `A2aService` trait,
//! routing gRPC calls to the underlying [`RequestHandler`]. JSON payloads
//! are carried inside protobuf `bytes` fields, reusing the same serde types
//! as the JSON-RPC and REST bindings.
//!
//! # Configuration
//!
//! Use [`GrpcConfig`] to control message size limits and compression.
//!
//! # Example
//!
//! ```rust,no_run
//! use std::sync::Arc;
//! use a2a_protocol_server::dispatch::grpc::{GrpcDispatcher, GrpcConfig};
//! use a2a_protocol_server::RequestHandlerBuilder;
//! # struct MyExec;
//! # impl a2a_protocol_server::AgentExecutor for MyExec {
//! #     fn execute<'a>(&'a self, _: &'a a2a_protocol_server::RequestContext,
//! #         _: &'a dyn a2a_protocol_server::EventQueueWriter,
//! #     ) -> std::pin::Pin<Box<dyn std::future::Future<
//! #         Output = a2a_protocol_types::error::A2aResult<()>
//! #     > + Send + 'a>> { Box::pin(async { Ok(()) }) }
//! # }
//! # async fn example() -> std::io::Result<()> {
//! let handler = Arc::new(
//!     RequestHandlerBuilder::new(MyExec).build().unwrap()
//! );
//! let config = GrpcConfig::default();
//! let dispatcher = GrpcDispatcher::new(handler, config);
//! dispatcher.serve("127.0.0.1:50051").await?;
//! # Ok(())
//! # }
//! ```

mod config;
mod dispatcher;
mod helpers;
mod service;

// Include the generated protobuf code.
pub(crate) mod proto {
    #![allow(
        clippy::all,
        clippy::pedantic,
        clippy::nursery,
        missing_docs,
        unused_qualifications
    )]
    tonic::include_proto!("a2a.v1");
}

pub use config::GrpcConfig;
pub use dispatcher::GrpcDispatcher;
pub use proto::a2a_service_server::A2aServiceServer;

use proto::a2a_service_server::A2aService;
use proto::JsonPayload;
