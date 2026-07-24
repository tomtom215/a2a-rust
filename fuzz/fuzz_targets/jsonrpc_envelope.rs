// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Fuzz target for the JSON-RPC envelope and every method's params type.
//!
//! The JSON-RPC dispatcher parses an untrusted request envelope, then the
//! `params` of whichever method was named. This target exercises both the
//! envelope and the per-method params/response types that cross the wire —
//! none may panic on arbitrary bytes.
//!
//! Run with: `cargo +nightly fuzz run jsonrpc_envelope`

#![no_main]

use a2a_protocol_types as t;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // Envelope forms.
    let _ = serde_json::from_slice::<t::jsonrpc::JsonRpcRequest>(data);
    let _ = serde_json::from_slice::<t::jsonrpc::JsonRpcResponse<serde_json::Value>>(data);
    let _ = serde_json::from_slice::<t::jsonrpc::JsonRpcErrorResponse>(data);

    // Per-method params.
    let _ = serde_json::from_slice::<t::params::MessageSendParams>(data);
    let _ = serde_json::from_slice::<t::params::TaskQueryParams>(data);
    let _ = serde_json::from_slice::<t::params::ListTasksParams>(data);
    let _ = serde_json::from_slice::<t::params::CancelTaskParams>(data);
    let _ = serde_json::from_slice::<t::params::TaskIdParams>(data);
    let _ = serde_json::from_slice::<t::params::GetPushConfigParams>(data);
    let _ = serde_json::from_slice::<t::params::ListPushConfigsParams>(data);
    let _ = serde_json::from_slice::<t::params::DeletePushConfigParams>(data);
    let _ = serde_json::from_slice::<t::params::GetExtendedAgentCardParams>(data);

    // Responses / results that the client parses from an untrusted server.
    let _ = serde_json::from_slice::<t::responses::SendMessageResponse>(data);
    let _ = serde_json::from_slice::<t::responses::TaskListResponse>(data);
    let _ = serde_json::from_slice::<t::responses::ListPushConfigsResponse>(data);
    let _ = serde_json::from_slice::<t::push::TaskPushNotificationConfig>(data);
});
