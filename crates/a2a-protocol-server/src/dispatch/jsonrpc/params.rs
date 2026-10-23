// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Reading a call's `params`, which happens before the handler — and so
//! before the interceptor chain — sees the call.
//!
//! Until 2026-10-20 a call whose params did not parse was answered `-32602`
//! there and then, so a caller with no credential learned which field it got
//! wrong where a well-formed call would have been refused `401`
//! (`docs/handoff.md`, "Open engineering"). Such a call is now admitted
//! first, and answered `-32602` only when the chain admits it.

use std::collections::HashMap;

use a2a_protocol_types::jsonrpc::JsonRpcRequest;
use a2a_protocol_types::params::AcceptedFields;

use super::JsonRpcDispatcher;
use super::response::parse_params;
use crate::error::ServerError;

impl JsonRpcDispatcher {
    /// [`parse_params`], with the interceptor chain's refusal in place of
    /// the parse error when the chain does not admit the caller.
    pub(super) async fn params<T>(
        &self,
        rpc_req: &JsonRpcRequest,
        headers: Option<&HashMap<String, String>>,
    ) -> Result<T, ServerError>
    where
        T: serde::de::DeserializeOwned + AcceptedFields,
    {
        match parse_params::<T>(rpc_req) {
            Ok(params) => Ok(params),
            Err(unreadable) => Err(self
                .handler
                .refuse_unreadable(&rpc_req.method, headers, unreadable)
                .await),
        }
    }
}
