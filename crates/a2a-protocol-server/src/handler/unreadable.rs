// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Admission for a call whose params a dispatcher could not read.
//!
//! Every handler method authenticates before it looks at its params, but a
//! dispatcher that must deserialize them first never reaches the handler
//! when they are malformed. JSON-RPC answered such a call `-32602`, naming
//! the field at fault, to a caller with no credential, where the same caller
//! with well-formed params was answered `401`; HTTP+JSON, gRPC and WebSocket
//! did the same with their own codes until 2026-10-20. Every binding now
//! admits such a call here before it answers.

use std::collections::HashMap;
use std::convert::Infallible;

use crate::error::ServerError;

use super::RequestHandler;
use super::helpers::build_call_context;

impl RequestHandler {
    /// The error to answer a `method` call with when its params could not be
    /// read: the refusal of the interceptor chain, when it does not admit
    /// the caller, and `unreadable` otherwise.
    pub(crate) async fn refuse_unreadable(
        &self,
        method: &str,
        headers: Option<&HashMap<String, String>>,
        unreadable: ServerError,
    ) -> ServerError {
        match self.admit_unreadable(method, headers, unreadable).await {
            Ok(unreadable) | Err(unreadable) => unreadable,
        }
    }

    /// Admits a `method` call whose params could not be read: `Ok` with
    /// `unreadable`, for the binding to answer as it always has, when the
    /// interceptor chain admits the caller, and `Err` with the chain's
    /// refusal when it does not.
    ///
    /// The call is admitted as a handler method admits one, in the tenant
    /// the resolver gives a request naming none, so the interceptors see it
    /// as any other — `before`, then `on_complete` with `unreadable` — with
    /// no task id, since none could be read. When strict tenancy resolves no
    /// tenant the chain still runs, in the default partition, so the answer
    /// does not depend on the tenant configuration: nothing here reads or
    /// writes a store.
    ///
    /// Split from [`refuse_unreadable`](Self::refuse_unreadable) 2026-10-20
    /// for the bindings whose answer to an unreadable request is not the
    /// error alone: HTTP+JSON answers it with a `400` naming the field, and
    /// gRPC with an `INVALID_ARGUMENT` carrying a `BadRequest`.
    pub(crate) async fn admit_unreadable(
        &self,
        method: &str,
        headers: Option<&HashMap<String, String>>,
        unreadable: ServerError,
    ) -> Result<ServerError, ServerError> {
        let tenant = self
            .resolve_tenant_unadmitted(method, headers, None)
            .await
            .unwrap_or_default();
        crate::store::tenant::TenantContext::scope_request(tenant, async {
            let call_ctx = build_call_context(method, headers, self.inbound_trace_policy);
            let mut call = self.interceptors.begin(&call_ctx);
            let (admitted, answer) = match self.admit(&mut call, &call_ctx).await {
                Ok(()) => (true, unreadable),
                Err(refused) => (false, refused),
            };
            let answered = match call.finish::<Infallible>(Err(answer)).await {
                Err(e) => e,
                Ok(never) => match never {},
            };
            if admitted {
                Ok(answered)
            } else {
                Err(answered)
            }
        })
        .await
    }
}
