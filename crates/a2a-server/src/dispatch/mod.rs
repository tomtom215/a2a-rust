// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! HTTP dispatch layer — JSON-RPC and REST routing.

pub mod jsonrpc;
pub mod rest;

pub use jsonrpc::JsonRpcDispatcher;
pub use rest::RestDispatcher;
