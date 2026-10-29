<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# OAuth 2.0 client credentials

Fetch tokens from an authorization server with the RFC 6749 §4.4 grant and attach them to every call, refreshed before they expire.

## Fetching and attaching tokens

`OAuth2ClientCredentials` runs the RFC 6749 §4.4 grant, caches the token,
refreshes it shortly before expiry, and collapses concurrent refreshes into a
single request:

```rust
# fn main() -> Result<(), Box<dyn std::error::Error>> {
use std::sync::Arc;
use a2a_protocol_client::{BearerAuthInterceptor, ClientBuilder, OAuth2ClientCredentials};

let provider = Arc::new(
    OAuth2ClientCredentials::new(
        "https://auth.example.com/oauth/token",
        "my-client-id",
        "my-client-secret",
    )
    .with_scopes(["tasks:read", "tasks:write"]),
);

let client = ClientBuilder::new("https://agent.example.com")
    .with_interceptor(BearerAuthInterceptor::new(provider))
    .build()?;
# Ok(())
# }
```

## From the agent card

An agent that advertises an OAuth2 client-credentials
scheme carries its token endpoint:

```rust,no_run
# use a2a_protocol_sdk::prelude::*;
# fn f(card: AgentCard) -> Result<(), Box<dyn std::error::Error>> {
let provider = OAuth2ClientCredentials::from_agent_card(
    &card, "my-oauth-scheme", "client-id", "client-secret",
)?;
# Ok(())
# }
```

## From an OIDC issuer

`OAuth2ClientCredentials::from_oidc_issuer` discovers the `token_endpoint`:

```rust,no_run
# use a2a_protocol_sdk::prelude::*;
# #[tokio::main]
# async fn main() -> Result<(), Box<dyn std::error::Error>> {
let provider = OAuth2ClientCredentials::from_oidc_issuer(
    "https://login.example.com", "client-id", "client-secret",
).await?;
# Ok(())
# }
```

## When the token endpoint fails

When the token endpoint fails, every caller waiting on that refresh gets the
same error from the one request: a dead endpoint costs them one
`with_request_timeout` together, not one each in turn. The failure is then
remembered for `with_failure_backoff` (default 1 s; `Duration::ZERO` turns it
off), so a tight loop makes at most one attempt per backoff. A caller cancelled
mid-refresh does not strand the others; the next one starts a new attempt.

## The client secret

The client secret is never logged, never echoed in an error, and redacted from
`Debug`. `Basic` client authentication is the default; switch to form-body
credentials with `.with_auth_style(TokenEndpointAuthStyle::Post)`.
