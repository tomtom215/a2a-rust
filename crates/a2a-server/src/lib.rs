// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! A2A protocol 0.3.0 — server framework.
//!
//! Provides `RequestHandler` and `AgentExecutor` for implementing A2A
//! agents over HTTP/1.1 and HTTP/2 using hyper 1.x.
//!
//! Full implementation arrives in Phase 3.

#![warn(missing_docs)]
#![deny(unsafe_op_in_unsafe_fn)]
#![warn(clippy::all, clippy::pedantic, clippy::nursery)]
#![allow(clippy::module_name_repetitions)]
