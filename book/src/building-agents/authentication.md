# Authentication

a2a-rust ships first-party helpers for both sides of authentication:

- **Server** — reject unauthenticated requests with an interceptor that
  guards *every* transport (JSON-RPC, REST, gRPC, WebSocket), because they all
  populate the same `CallContext` headers.
- **Client** — acquire and attach bearer tokens, including a full OAuth 2.0
  client-credentials flow with caching and refresh.

All of it is built on the crate's existing `ring` + `hyper` stack — no
OAuth-ecosystem dependencies (see [ADR 0010](../reference/adrs.md)).

## Server: guarding an agent

### API key / static bearer token

For a fixed set of accepted credentials, `ApiKeyAuthInterceptor` and
`BearerTokenAuthInterceptor` compare in constant time and need no feature flag:

```rust
use a2a_protocol_server::{BearerTokenAuthInterceptor, RequestHandlerBuilder};

let handler = RequestHandlerBuilder::new(my_executor)
    .with_interceptor(BearerTokenAuthInterceptor::new([
        "service-token-1",
        "service-token-2",
    ]))
    .build()?;
```

`ApiKeyAuthInterceptor::new([...])` reads `x-api-key` by default; change the
header with `.with_header("X-Company-Key")`.

### JWT (HS256 / RS256 / ES256)

Enable the `auth-jwt` feature. `JwtAuthInterceptor` verifies the token's
signature and its `exp`/`nbf`/`iss`/`aud` claims. Keys come from a static
`Jwks`, a shared HS256 secret, or a remote JWKS endpoint.

```toml
a2a-protocol-server = { version = "0.7", features = ["auth-jwt"] }
```

**Validate tokens from an OIDC issuer** (discovers the issuer's JWKS, caches it,
and refetches on key rotation):

```rust
use a2a_protocol_server::auth::jwt::{JwtAuthInterceptor, JwtValidator};

let validator = JwtValidator::new()
    .with_issuer("https://login.example.com")
    .with_audience("my-a2a-agent");

let interceptor =
    JwtAuthInterceptor::from_oidc_issuer("https://login.example.com", validator).await?;
```

**Static keys** — supply a `Jwks` directly (from a JWKS JSON document or key by
key), or a shared secret for HS256:

```rust
use a2a_protocol_server::auth::jwt::{Jwks, JwtAuthInterceptor, JwtValidator};

// RS256/ES256 from a JWKS document:
let jwks = Jwks::from_json(jwks_bytes)?;
let interceptor = JwtAuthInterceptor::new(
    JwtValidator::new().with_issuer("https://issuer").with_audience("agent"),
    jwks,
);

// HS256 shared secret (no JWKS):
let interceptor = JwtAuthInterceptor::new(
    JwtValidator::new().with_hs256_secret(b"shared-secret".to_vec()),
    Jwks::new(),
);
```

### Security properties

- **Only HS256/RS256/ES256 are accepted.** `alg: none` and any unlisted
  algorithm are rejected. HS256 is only ever verified against a configured
  secret — never a JWKS public key — so the RS256→HS256 algorithm-confusion
  downgrade is structurally impossible.
- **Rejections are generic.** A missing, malformed, expired, or wrong token all
  produce the same error, so the response can't be used as an oracle.
- **Constant-time comparison** for API keys and static bearer tokens.

### Error mapping (important)

An interceptor rejection surfaces as A2A `InvalidRequest`
(**HTTP 400** / gRPC `INVALID_ARGUMENT`), *not* `401`. The A2A protocol has no
dedicated "unauthenticated" error code — the spec models authentication at the
transport/security-scheme layer. When you need real `401` semantics with a
`WWW-Authenticate` challenge, terminate authentication at a gateway in front of
the agent; these interceptors are the self-contained, defense-in-depth option.

## Client: acquiring and attaching tokens

### A token you already have

```rust
use std::sync::Arc;
use a2a_protocol_client::{BearerAuthInterceptor, ClientBuilder, StaticTokenProvider};

let provider = Arc::new(StaticTokenProvider::new("my-long-lived-token"));
let client = ClientBuilder::new("https://agent.example.com")
    .with_interceptor(BearerAuthInterceptor::new(provider))
    .build()?;
```

`BearerAuthInterceptor` asks its provider for a token before **every** request,
so a provider that refreshes keeps a long-lived client authenticated across
token rotations.

### OAuth 2.0 client credentials

`OAuth2ClientCredentials` runs the RFC 6749 §4.4 grant, caches the token,
refreshes it shortly before expiry, and collapses concurrent refreshes into a
single request:

```rust
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
```

**From the agent card** — an agent that advertises an OAuth2 client-credentials
scheme carries its token endpoint:

```rust
let provider = OAuth2ClientCredentials::from_agent_card(
    &card, "my-oauth-scheme", "client-id", "client-secret",
)?;
```

**From an OIDC issuer** — discovers the `token_endpoint`:

```rust
let provider = OAuth2ClientCredentials::from_oidc_issuer(
    "https://login.example.com", "client-id", "client-secret",
).await?;
```

The client secret is never logged, never echoed in an error, and redacted from
`Debug`. `Basic` client authentication is the default; switch to form-body
credentials with `.with_auth_style(TokenEndpointAuthStyle::Post)`.

### Interactive flows

Authorization-code (browser redirect) and device-code grants are interactive
and out of scope for agent-to-agent auth. Implement the `TokenProvider` trait
with your own flow and pass it to `BearerAuthInterceptor`.
