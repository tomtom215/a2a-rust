<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Authorization: scopes and per-task checks

Authentication says who the caller is; authorization says what it may do. This page covers refusing a token that lacks a scope, and refusing a call per operation or per task.

Spec §13.1 requires an authorization check on every operation, and §3.3.2 a
`403` when the authenticated caller lacks a permission, naming the missing
scope where that leaks nothing. Two pieces help.

## A scope every call needs

`JwtValidator::require_scope` refuses a token
that verifies but does not grant the scope in its `scope` claim (a
space-separated string, or an array). The answer is `403` with the RFC 6750
challenge `Bearer error="insufficient_scope", scope="tasks:write"`, naming only
the scopes the token lacks. A token that fails any other check gets `401`
before its scopes are looked at.

```rust
# fn main() -> Result<(), Box<dyn std::error::Error>> {
use a2a_protocol_server::auth::jwt::{Jwks, JwtAuthInterceptor, JwtValidator};

let validator = JwtValidator::new()
    .with_issuer("https://issuer")
    .with_audience("agent")
    .require_scope("a2a:invoke");
let interceptor = JwtAuthInterceptor::new(validator, Jwks::new());
# Ok(())
# }
```

## A decision per operation or per task

An interceptor sees the method
(`CallContext::method`), who the caller is (`caller_identity`, set by the
authenticating interceptor before it) and the task the request names
(`task_id`: the `id` of `GetTask`, `CancelTask` and `SubscribeToTask`, the
`taskId` of a push-config call or of a message continuing a task). To refuse,
return `A2aError::permission_denied` (`403`), or
`A2aError::insufficient_scope(message, scopes)` (`403` with the challenge
naming the scopes) when a scope would let the caller in:

```rust
use std::future::Future;
use std::pin::Pin;

use a2a_protocol_server::{CallContext, ServerInterceptor};
use a2a_protocol_types::error::{A2aError, A2aResult};

/// Only callers on the task's allow-list may touch it.
struct TaskAcl;

impl TaskAcl {
    fn may(&self, caller: Option<&str>, task: &str) -> bool {
        // Look the pair up in your own records; refuse what is not listed.
        caller.is_some() && !task.is_empty()
    }
}

impl ServerInterceptor for TaskAcl {
    fn before<'a>(
        &'a self,
        ctx: &'a CallContext,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async move {
            match ctx.task_id() {
                Some(task) if !self.may(ctx.caller_identity(), task) => {
                    Err(A2aError::permission_denied("not permitted"))
                }
                _ => Ok(()),
            }
        })
    }

    fn after<'a>(
        &'a self,
        _ctx: &'a CallContext,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async { Ok(()) })
    }
}
```

Register it after the authenticating interceptor, so the identity is set when
it runs. `task_id` is what the caller sent, before the handler looks the task
up, so decide from your own records and not from whether the task exists: an
allow-list that refuses every id it does not list answers `403` for an unknown
task and a forbidden one alike, and reveals nothing (spec §3.3.2). With caller
isolation on (the default once callers are identified) another caller's task
is already `TaskNotFound`.
