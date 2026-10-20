// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Admission for a call whose params a dispatcher could not read.
//!
//! Every handler method authenticates before it looks at its params, but a
//! dispatcher that must deserialize them first never reaches the handler
//! when they are malformed. JSON-RPC answered such a call `-32602`, naming
//! the field at fault, to a caller with no credential, where the same caller
//! with well-formed params was answered `401`.

use std::collections::HashMap;
use std::convert::Infallible;

use crate::error::ServerError;

use super::RequestHandler;
use super::helpers::build_call_context;

impl RequestHandler {
    /// The error to answer a `method` call with when its params could not be
    /// read: the refusal of the interceptor chain, when it does not admit
    /// the caller, and `unreadable` otherwise.
    ///
    /// The call is admitted as a handler method admits one, in the tenant
    /// the resolver gives a request naming none, so the interceptors see it
    /// as any other — `before`, then `on_complete` with `unreadable` — with
    /// no task id, since none could be read. When strict tenancy resolves no
    /// tenant the chain still runs, in the default partition, so the answer
    /// does not depend on the tenant configuration: nothing here reads or
    /// writes a store.
    pub(crate) async fn refuse_unreadable(
        &self,
        method: &str,
        headers: Option<&HashMap<String, String>>,
        unreadable: ServerError,
    ) -> ServerError {
        let tenant = self
            .resolve_tenant(method, headers, None)
            .await
            .unwrap_or_default();
        let answered = crate::store::tenant::TenantContext::scope_request(tenant, async {
            let call_ctx = build_call_context(method, headers, self.inbound_trace_policy);
            let mut call = self.interceptors.begin(&call_ctx);
            let refused = match self.admit(&mut call, &call_ctx).await {
                Ok(()) => unreadable,
                Err(refused) => refused,
            };
            call.finish::<Infallible>(Err(refused)).await
        })
        .await;
        match answered {
            Err(e) => e,
            Ok(never) => match never {},
        }
    }
}
