// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! Fuzz target for JSON deserialization of core A2A protocol types.
//!
//! Run with: `cargo +nightly fuzz run json_deser`

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // Try to parse as each major protocol type.
    // None of these should panic — errors are fine, panics are bugs.
    let _ = serde_json::from_slice::<a2a_types::AgentCard>(data);
    let _ = serde_json::from_slice::<a2a_types::Task>(data);
    let _ = serde_json::from_slice::<a2a_types::Message>(data);
    let _ = serde_json::from_slice::<a2a_types::SendMessageResponse>(data);
    let _ = serde_json::from_slice::<a2a_types::StreamResponse>(data);
    let _ = serde_json::from_slice::<a2a_types::TaskPushNotificationConfig>(data);
    let _ = serde_json::from_slice::<a2a_types::jsonrpc::JsonRpcRequest>(data);
});
