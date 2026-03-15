// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! A2A protocol 0.3.0 — pure data types with serde support.
//!
//! This crate provides all wire types for the A2A protocol with zero I/O
//! dependencies. Add `a2a-client` or `a2a-server` for HTTP transport.

#![warn(missing_docs)]
#![deny(unsafe_op_in_unsafe_fn)]
#![warn(clippy::all, clippy::pedantic, clippy::nursery)]
#![allow(clippy::module_name_repetitions)]
