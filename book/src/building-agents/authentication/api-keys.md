<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# API keys and bearer tokens

Guard an agent with a fixed set of shared secrets: API keys in a header, or static bearer tokens. This page covers the card declaration, naming each caller, how identity partitions tasks, and rotating credentials without a restart. For tokens an identity provider issues, see [JWT and OIDC](./jwt.md).

## Guarding an agent

For a fixed set of accepted credentials, `ApiKeyAuthInterceptor` and
`BearerTokenAuthInterceptor` compare in constant time and need no feature flag:

```rust
# fn card() -> a2a_protocol_types::agent_card::AgentCard {
#     use a2a_protocol_types::agent_card::{AgentCapabilities, AgentCard, AgentInterface, AgentSkill};
#     AgentCard::new("my-agent", "1.0.0", AgentInterface::jsonrpc("http://localhost:3000"))
#         .with_description("An example agent")
#         .with_skill(AgentSkill::new("echo", "Echo", "Echoes text").with_tags(["echo"]))
#         .with_capabilities(AgentCapabilities::none().with_streaming(true))
# }
# use a2a_protocol_sdk::prelude::agent_executor;
# struct MyAgent;
# agent_executor!(MyAgent, |_ctx, _queue| async { Ok(()) });
# fn main() -> Result<(), Box<dyn std::error::Error>> {
# let my_executor = MyAgent;
use a2a_protocol_server::{BearerTokenAuthInterceptor, RequestHandlerBuilder};
use a2a_protocol_types::security::{
    HttpAuthSecurityScheme, SecurityRequirement, SecurityScheme, StringList,
};
use std::collections::HashMap;

// Spec §13.3: the public card declares the scheme the interceptor checks.
let card = card()
    .with_security_schemes(HashMap::from([(
        "bearer".to_owned(),
        SecurityScheme::Http(HttpAuthSecurityScheme {
            scheme: "Bearer".to_owned(),
            description: None,
            bearer_format: None,
        }),
    )]))
    .with_security_requirements(vec![SecurityRequirement {
        schemes: HashMap::from([("bearer".to_owned(), StringList { list: vec![] })]),
    }]);

let handler = RequestHandlerBuilder::new(my_executor)
    .with_agent_card(card)
    .with_interceptor(BearerTokenAuthInterceptor::new([
        "service-token-1",
        "service-token-2",
    ]))
    .build()?;
# Ok(())
# }
```

`ApiKeyAuthInterceptor::new([...])` reads `x-api-key` by default; change the
header with `.with_header("X-Company-Key")`.

## Naming the caller

`new` authenticates without naming the caller, so the rate limiter keys
every valid caller by its address (or, behind trusted proxies, the forwarded
client address), not by who it is. To give each credential an identity
(`CallContext::caller_identity`), build the interceptor with
`BearerTokenAuthInterceptor::with_labelled_tokens([(token, "caller-a"), …])` or
`ApiKeyAuthInterceptor::with_labelled_keys(…)`; the label, not the secret, is
what reaches rate limiting, logs and metrics. The identity is the scheme and
the label, `bearer:caller-a` or `api-key:caller-a`, and a JWT's is
`jwt:<issuer>:<sub>` with the issuer percent-encoded (`%` as `%25`, `:` as
`%3A`; a token with no `iss` has an empty issuer), so a token from
`https://idp.example` for `alice` is `jwt:https%3A//idp.example:alice`. Build
every one with `a2a_protocol_server::auth::caller_id` (`bearer`, `api_key`,
`jwt`, `mtls`) rather than by hand, for an approver allowlist or anything else
that compares against one. A bare name such as `alice` in an approver
allowlist matches nobody.

## Each caller gets its own tasks

An identity also scopes tasks (spec §13.1): with an authenticating
interceptor, each identified caller gets a partition of its own, and another
caller's task is `TaskNotFound`. An unlabelled bearer token or API key is the
exception: it authenticates but names nobody, so it keeps the tenant's shared
records. A JWT without a `sub` claim does not: it is refused with
`PermissionDenied` (`403`), because the tenant's shared records are never
handed to an anonymous token. Make the issuer put a `sub` in the token, or
call `share_tasks_across_callers()`.

Isolation needs a task store and a push-config store that partition by
tenant. Left unset they default to the tenant-aware in-memory stores, whose
capacity (`with_task_store_config`, 10,000 tasks by default) then applies per
caller. A store you supply must be a `TenantAware…` store (or a custom one
whose `isolates_tenants()` is `true`); otherwise `build()` refuses it. To let
identified callers share, call
`RequestHandlerBuilder::share_tasks_across_callers()`, which keeps one set of
records per tenant; see
[Multi-tenancy](../../deployment/multi-tenancy.md#callers-within-one-tenant).

## Rotating and revoking

Spec §13.4 says credentials SHOULD be rotated periodically, and agents SHOULD
have a way to revoke them. Both interceptors
hand out a `CredentialSet` from `credentials()`, a handle you keep after
giving the interceptor to the builder. What you add or revoke applies from the
next request on, without a restart:

```rust
# fn main() -> Result<(), Box<dyn std::error::Error>> {
use a2a_protocol_server::BearerTokenAuthInterceptor;

let auth = BearerTokenAuthInterceptor::with_labelled_tokens([("token-2026-09", "billing")]);
let tokens = auth.credentials(); // keep this; pass `auth` to `with_interceptor`

// Rotation: accept the new token, move the caller over, retire the old one.
tokens.add_labelled("token-2026-10", "billing");
assert!(tokens.revoke("token-2026-09"));
// Revocation of a leaked token is the same call; its next request gets 401.
# Ok(())
# }
```


A JWT is revoked differently, by its expiry or its signing key; see
[Revoking a token](./jwt.md#revoking-a-token).
