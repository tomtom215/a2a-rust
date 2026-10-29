<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Server interceptors

A server interceptor runs around every call the handler answers: `before` can
refuse it, `after` sees a success, and `on_complete` sees every outcome. This
page shows the trait, how to register an interceptor, and how to write one
that authenticates.

## The trait

Server interceptors run before and after the handler processes a request:

```rust
# use std::future::Future;
# use std::pin::Pin;
# use a2a_protocol_sdk::prelude::*;
# use a2a_protocol_sdk::server::CallContext;
use a2a_protocol_sdk::server::ServerInterceptor;

struct LoggingInterceptor;

impl ServerInterceptor for LoggingInterceptor {
    fn before<'a>(
        &'a self,
        ctx: &'a CallContext,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async move {
            println!("Request: {}", ctx.method());
            Ok(())
        })
    }

    fn after<'a>(
        &'a self,
        ctx: &'a CallContext,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async move {
            println!("Answered: {}", ctx.method());
            Ok(())
        })
    }
}
```

## Adding interceptors

```rust
# fn card() -> a2a_protocol_types::agent_card::AgentCard {
#     use a2a_protocol_types::agent_card::{AgentCapabilities, AgentCard, AgentInterface, AgentSkill};
#     AgentCard::new("my-agent", "1.0.0", AgentInterface::jsonrpc("http://localhost:3000"))
#         .with_description("An example agent")
#         .with_skill(AgentSkill::new("echo", "Echo", "Echoes text").with_tags(["echo"]))
#         .with_capabilities(AgentCapabilities::none().with_streaming(true))
#         .with_security_schemes(std::collections::HashMap::from([(
#             "bearer".to_owned(),
#             a2a_protocol_types::security::SecurityScheme::Http(
#                 a2a_protocol_types::security::HttpAuthSecurityScheme {
#                     scheme: "Bearer".to_owned(),
#                     description: None,
#                     bearer_format: None,
#                 },
#             ),
#         )]))
# }
# use std::future::Future;
# use std::pin::Pin;
# use a2a_protocol_sdk::prelude::*;
# use a2a_protocol_sdk::server::CallContext;
# use a2a_protocol_sdk::server::ServerInterceptor;
# struct MyAgent;
# agent_executor!(MyAgent, |_ctx, _queue| async { Ok(()) });
# struct LoggingInterceptor;
# impl ServerInterceptor for LoggingInterceptor {
#     fn before<'a>(&'a self, _: &'a CallContext) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> { Box::pin(async { Ok(()) }) }
#     fn after<'a>(&'a self, _: &'a CallContext) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> { Box::pin(async { Ok(()) }) }
# }
# fn main() -> Result<(), Box<dyn std::error::Error>> {
# let my_executor = MyAgent;
let handler = RequestHandlerBuilder::new(my_executor)
    .with_agent_card(card())
    .with_interceptor(BearerTokenAuthInterceptor::new(["s3cret-token"]))
    .with_interceptor(LoggingInterceptor)
    .with_interceptor(RateLimitInterceptor::new(RateLimitConfig::default())?)
    .build()?;
# Ok(())
# }
```

An authenticating interceptor needs a scheme declared in the public agent card
(spec §13.3), or `build()` refuses it. For the bundled bearer interceptor that
is an HTTP `Bearer` scheme, set with `.with_security_schemes(...)` on the card
passed to `with_agent_card`. See
[The card and the interceptors must agree](../authentication.md#the-card-and-the-interceptors-must-agree).


The order the chain runs them in is on the
[chapter's landing page](../interceptors.md#interceptor-chain).

## Seeing every outcome: `on_complete`

`after` runs only when the call succeeds, and an error it returns replaces
the response. For work that must happen however the call ends — releasing
what `before` acquired, closing an audit record, counting failures by
caller — override `on_complete`. It is called once per call, in reverse
order, on every interceptor whose `before` ran, with how the call ended:

```rust
# use std::future::Future;
# use std::pin::Pin;
# use a2a_protocol_sdk::prelude::*;
use a2a_protocol_sdk::server::{CallContext, CallOutcome, ServerInterceptor};

struct AuditInterceptor;

impl ServerInterceptor for AuditInterceptor {
    fn before<'a>(
        &'a self,
        _ctx: &'a CallContext,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async { Ok(()) })
    }

    fn after<'a>(
        &'a self,
        _ctx: &'a CallContext,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async { Ok(()) })
    }

    fn on_complete<'a>(
        &'a self,
        ctx: &'a CallContext,
        outcome: CallOutcome<'a>,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + 'a>> {
        Box::pin(async move {
            match outcome {
                CallOutcome::Succeeded => println!("{}: ok", ctx.method()),
                CallOutcome::Failed(e) => println!("{}: {}", ctx.method(), e.metric_label()),
                CallOutcome::Cancelled => println!("{}: client went away", ctx.method()),
                _ => {}
            }
        })
    }
}
```

`Failed` carries the error the caller is sent, whether a `before` hook, the
handler or an `after` hook produced it. `Cancelled` means the call's future
was dropped unanswered — the client disconnected, or a timeout above the
handler gave up — and the hook then runs in a task of its own. It returns
nothing, so it cannot change the response. For `SendStreamingMessage` and
`SubscribeToTask` the call ends when the stream is established, not when it
closes. The trait method's documentation states the full contract.

## Example: authentication

For a fixed set of API keys or bearer tokens, use the built-in
`ApiKeyAuthInterceptor` or `BearerTokenAuthInterceptor`: they compare
credentials in constant time, which a `HashSet` lookup does not. A custom
interceptor is for credentials you verify some other way — a session service,
say. It rejects in `before`, records who the caller is, and says that it
authenticates:

```rust
# use std::future::Future;
# use std::pin::Pin;
# use a2a_protocol_sdk::prelude::*;
# use a2a_protocol_sdk::server::CallContext;
# use a2a_protocol_sdk::server::ServerInterceptor;
# fn verify_session(_token: &str) -> Option<String> { None }
/// Accepts a request whose bearer token `verify_session` maps to a caller.
struct SessionAuthInterceptor;

impl ServerInterceptor for SessionAuthInterceptor {
    fn before<'a>(
        &'a self,
        ctx: &'a CallContext,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async move {
            let caller = ctx
                .http_headers()
                .get("authorization")
                .and_then(|h| h.strip_prefix("Bearer "))
                .and_then(verify_session)
                .ok_or_else(|| {
                    // 401 + WWW-Authenticate on HTTP, UNAUTHENTICATED on gRPC (ADR 0014).
                    A2aError::unauthenticated("authentication required", "Bearer realm=\"a2a\"")
                })?;
            // Rate limiting and executors (`ctx.caller_identity()`) key on this.
            ctx.set_caller_identity(caller);
            ctx.set_auth_scheme("session");
            Ok(())
        })
    }

    fn after<'a>(
        &'a self,
        _ctx: &'a CallContext,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async move { Ok(()) })
    }

    // Marks this interceptor as an authenticator. That lets the extended
    // agent card be served (spec §13.3), requires the public card to declare
    // a scheme it checks (override `checks_scheme` to say which), and makes
    // the handler give each caller named by `set_caller_identity` its own
    // task partition.
    fn authenticates(&self) -> bool {
        true
    }
}
```

Refuse an authenticated caller who lacks permission with
`A2aError::permission_denied(message)` (`403` / `PERMISSION_DENIED`). Any other
error keeps its usual status, so a client never learns to refresh its token.
