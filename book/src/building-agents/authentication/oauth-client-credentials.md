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
scheme carries its token endpoint. **The card is the agent's own word, and the
client secret goes to whatever host its `tokenUrl` names.** A card fetched over
a network you do not control, or from an agent you do not fully trust, can name
an endpoint of its choosing and receive `client-id:client-secret` on the first
token request. Name the host your identity provider runs on:

```rust,no_run
# use a2a_protocol_sdk::prelude::*;
# fn f(card: AgentCard) -> Result<(), Box<dyn std::error::Error>> {
let provider = OAuth2ClientCredentials::from_agent_card_trusting(
    &card, "my-oauth-scheme", "client-id", "client-secret",
    &["login.example.com"],
)?;
# Ok(())
# }
```

The host must match exactly (case, a trailing dot, the port and the path do not
matter): `login.example.com` does not admit `login.example.com.evil.example`,
`evil-login.example.com` or `https://login.example.com@evil.example/`, and an
empty list refuses every card. `from_agent_card` is the same without the list,
for a card you have verified yourself (a signed card read with
`ClientBuilder::from_signed_card`, for example):

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

## An identity provider behind a private CA, or one that wants a certificate

The token request trusts Mozilla's roots and the certificates `SSL_CERT_FILE`
and `SSL_CERT_DIR` name, and presents no client certificate. For a private CA
set in code, or an identity provider that requires mutual TLS (a service mesh,
SPIFFE), give the provider a TLS configuration: `with_tls_config` on
`OAuth2ClientCredentials` and on `TokenExchange`, and
`discover_token_endpoint_with_tls_config` for the discovery request. A
configuration you give is exact, and `tls::tls_config_from_pem` builds one
from PEM that keeps the default roots, adds yours, and presents your
certificate and key:

```rust,no_run
# use a2a_protocol_sdk::prelude::*;
# use a2a_protocol_client::tls;
# async fn f(ca: Vec<u8>, cert: Vec<u8>, key: Vec<u8>) -> Result<(), Box<dyn std::error::Error>> {
let tls = tls::tls_config_from_pem(Some(&ca), Some((&cert, &key)))?;
let provider = OAuth2ClientCredentials::new(
    "https://idp.internal/oauth/token", "client-id", "client-secret",
)
.with_tls_config(tls);
# Ok(())
# }
```

Until 2026-11-01 the provider took no TLS configuration at all, though every
other outbound path of the client did.

## Plaintext endpoints are refused

The token request carries the client secret. A token endpoint, a `tokenUrl`
from an agent card, or an issuer for discovery that is `http://` to a host
other than loopback is refused with `ClientError::InvalidEndpoint` before any
connection is made; the error names the origin and not the path or query.
For an endpoint on a network you trust end to end, build the provider with
`OAuth2ClientCredentials::new` and call `allow_insecure_token_endpoint()`
(`TokenExchange` has the same method). Discovery has no such switch: take
the `token_endpoint` from the issuer's document yourself and pass it to
`new`. Discovery also requires the document's `issuer` to equal the issuer
you asked about (a trailing slash does not matter) and refuses a document
that names none, so a document served for another issuer is not believed.

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
