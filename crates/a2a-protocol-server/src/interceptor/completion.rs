// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! One call through the interceptor chain, from `before` to `on_complete`.
//!
//! Every [`RequestHandler`](crate::RequestHandler) method runs its body as
//!
//! ```text
//! let mut call = self.interceptors.begin(&call_ctx);
//! let result = async { call.before().await?; /* body */ }.await;
//! call.finish(result).await
//! ```
//!
//! so that [`ServerInterceptor::on_complete`] sees every outcome. A drop
//! guard is not enough on its own: a call that fails at a `?` returns
//! through the same drop as one whose client went away. So `finish` is
//! handed the result when there is one, and the guard is left only the case
//! where there is none.
//!
//! The body is awaited in place by the handler, never passed in. An
//! `intercept(ctx, body)` taking the body as an argument stored it twice —
//! as the argument, and again where it was awaited — and grew every send
//! dispatch future, to about 30 KB, past clippy's `large_futures` bound.
//! `rpc_span::ServerSpan::call` documents the same trap.

use super::{CallOutcome, ServerInterceptorChain};
use crate::call_context::CallContext;
use crate::error::ServerResult;

impl ServerInterceptorChain {
    /// Starts one call through the chain. Nothing runs until
    /// [`InterceptedCall::before`].
    pub const fn begin<'a>(&'a self, ctx: &'a CallContext) -> InterceptedCall<'a> {
        InterceptedCall {
            chain: self,
            ctx,
            entered: 0,
        }
    }
}

/// A call whose interceptors have not all been told how it ended.
pub struct InterceptedCall<'a> {
    chain: &'a ServerInterceptorChain,
    ctx: &'a CallContext,
    /// How many interceptors, from the front of the chain, have had
    /// `before` called and not yet had `on_complete` started. They are
    /// completed from the back, so this is also the next one to complete.
    entered: usize,
}

impl InterceptedCall<'_> {
    /// Runs every `before` hook in insertion order, stopping at the first
    /// that refuses the call.
    pub async fn before(&mut self) -> a2a_protocol_types::error::A2aResult<()> {
        let chain = self.chain;
        for interceptor in &chain.interceptors {
            // Counted before the await: an interceptor whose `before` was
            // called is owed `on_complete`, whether it then refuses the call
            // or the call is dropped inside it.
            self.entered += 1;
            interceptor.before(self.ctx).await?;
        }
        Ok(())
    }

    /// Runs every `after` hook if the call succeeded, then `on_complete` on
    /// every interceptor whose `before` ran, and returns the call's result —
    /// an `after` error in place of a success.
    pub async fn finish<T>(mut self, result: ServerResult<T>) -> ServerResult<T> {
        let result = match result {
            Ok(out) => match self.chain.run_after(self.ctx).await {
                Ok(()) => Ok(out),
                Err(e) => Err(e.into()),
            },
            Err(e) => Err(e),
        };
        let outcome = match &result {
            Ok(_) => CallOutcome::Succeeded,
            Err(e) => CallOutcome::Failed(e),
        };
        let chain = self.chain;
        for interceptor in chain.interceptors.iter().take(self.entered).rev() {
            // Decremented before the await, so a drop during this hook does
            // not start it a second time.
            self.entered -= 1;
            interceptor.on_complete(self.ctx, outcome).await;
        }
        result
    }
}

impl Drop for InterceptedCall<'_> {
    fn drop(&mut self) {
        if self.entered == 0 {
            return;
        }
        // Dropped with interceptors still owed their `on_complete`: the call
        // was cancelled. The hooks are async and this is not, so they run in
        // a task of their own, in this call's span and tenant.
        let Ok(runtime) = tokio::runtime::Handle::try_current() else {
            return;
        };
        let owed: Vec<_> = self
            .chain
            .interceptors
            .iter()
            .take(self.entered)
            .rev()
            .cloned()
            .collect();
        let ctx = self.ctx.clone();
        let tenant = crate::store::tenant::TenantContext::current();
        drop(runtime.spawn(crate::rpc_span::in_current_span(
            crate::store::tenant::TenantContext::scope(tenant, async move {
                for interceptor in owed {
                    interceptor.on_complete(&ctx, CallOutcome::Cancelled).await;
                }
            }),
        )));
    }
}
