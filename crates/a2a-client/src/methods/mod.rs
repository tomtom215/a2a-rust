// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! Per-method client helpers.
//!
//! Each sub-module adds `impl A2aClient` blocks for a related group of A2A
//! protocol methods. The modules are declared here; the `A2aClient` struct
//! itself is defined in [`crate::client`].

pub mod extended_card;
pub mod push_config;
pub mod send_message;
pub mod tasks;
