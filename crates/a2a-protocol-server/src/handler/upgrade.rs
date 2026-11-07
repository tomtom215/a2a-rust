// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Admission for a WebSocket upgrade on the HTTP port (audit B-25, ADR 0055).
//!
//! The upgrade request's headers are the connection's for every call on it,
//! and the chain sees them again on each call; admitting them once before the
//! `101` is what keeps a caller the chain refuses from holding a connection
//! at all.

use std::collections::HashMap;

use crate::error::ServerError;

use super::RequestHandler;
use super::helpers::build_call_context;

/// The method name the interceptor chain sees for an upgrade.
pub const UPGRADE_METHOD: &str = "WebSocketUpgrade";

impl RequestHandler {
    /// Runs the interceptor chain on an upgrade request's `headers`, as a
    /// call to [`UPGRADE_METHOD`], in the tenant the resolver gives a request
    /// naming none: `Ok` when the chain admits it, the chain's refusal
    /// otherwise. Interceptors see `before` and then `on_complete`, as for
    /// any other call; nothing reads or writes a store.
    pub(crate) async fn admit_upgrade(
        &self,
        headers: &HashMap<String, String>,
    ) -> Result<(), ServerError> {
        let tenant = self
            .resolve_tenant_unadmitted(UPGRADE_METHOD, Some(headers), None)
            .await
            .unwrap_or_default();
        crate::store::tenant::TenantContext::scope_request(tenant, async {
            let call_ctx =
                build_call_context(UPGRADE_METHOD, Some(headers), self.inbound_trace_policy);
            let mut call = self.interceptors.begin(&call_ctx);
            let admitted = self.admit(&mut call, &call_ctx).await;
            call.finish(admitted).await
        })
        .await
    }
}
