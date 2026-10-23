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

let handler = RequestHandlerBuilder::new(my_executor)
    .with_agent_card(card())
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

`new` authenticates without naming the caller, so the rate limiter keys
every valid caller by its address (or, behind trusted proxies, the forwarded
client address), not by who it is. To give each credential an identity
(`CallContext::caller_identity`), build the interceptor with
`BearerTokenAuthInterceptor::with_labelled_tokens([(token, "caller-a"), …])` or
`ApiKeyAuthInterceptor::with_labelled_keys(…)`; the label, not the secret, is
what reaches rate limiting, logs and metrics. The identity is the scheme and
the label, `bearer:caller-a` or `api-key:caller-a`, and a JWT's is
`jwt:<issuer>:<sub>`; `a2a_protocol_server::auth::caller_id` builds each, for
an approver allowlist or anything else that compares against one.

An identity also scopes tasks (spec §13.1): with an authenticating
interceptor, each identified caller gets a partition of its own, and another
caller's task is `TaskNotFound`. Callers `new` leaves unnamed keep sharing the
tenant's records. To let identified callers share, call
`RequestHandlerBuilder::share_tasks_across_callers()`; see
[Multi-tenancy](../deployment/multi-tenancy.md#callers-within-one-tenant).

**Rotating and revoking** (spec §13.4: credentials SHOULD be rotated
periodically, and agents SHOULD have a way to revoke them). Both interceptors
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

A JWT is revoked by removing its signing key from the JWKS (a remote set is
refetched on rotation) or by its `exp`; `JwtAuthInterceptor` keeps no list of
revoked token ids.

### JWT (HS256 / RS256 / ES256)

Enable the `auth-jwt` feature. `JwtAuthInterceptor` verifies the token's
signature and its `exp`/`nbf`/`iss`/`aud` claims. Keys come from a static
`Jwks`, a shared HS256 secret, or a remote JWKS endpoint.

```toml
a2a-protocol-server = { version = "0.15", features = ["auth-jwt", "tls-rustls"] }
```

`tls-rustls` is needed to fetch JWKS or OIDC discovery documents over
`https://`; without it the fetch is plaintext-only. Static keys and HS256 need
only `auth-jwt`.

**Validate tokens from an OIDC issuer** (discovers the issuer's JWKS, caches it,
and refetches on key rotation):

```rust,no_run
# #[tokio::main]
# async fn main() -> Result<(), Box<dyn std::error::Error>> {
use a2a_protocol_server::auth::jwt::{JwtAuthInterceptor, JwtValidator};

let validator = JwtValidator::new()
    .with_issuer("https://login.example.com")
    .with_audience("my-a2a-agent");

let interceptor =
    JwtAuthInterceptor::from_oidc_issuer("https://login.example.com", validator).await?;
# Ok(())
# }
```

**Static keys** — supply a `Jwks` directly (from a JWKS JSON document or key by
key), or a shared secret for HS256:

```rust
# fn main() -> Result<(), Box<dyn std::error::Error>> {
# let jwks_bytes: &[u8] = br#"{"keys": []}"#;
use a2a_protocol_server::auth::jwt::{Jwks, JwtAuthInterceptor, JwtValidator};

// RS256/ES256 from a JWKS document:
let jwks = Jwks::from_json(jwks_bytes)?;
let interceptor = JwtAuthInterceptor::new(
    JwtValidator::new().with_issuer("https://issuer").with_audience("agent"),
    jwks,
);

// HS256 shared secret (no JWKS). At least 32 bytes (RFC 7518 §3.2);
// `try_with_hs256_secret` refuses a shorter one here, at startup.
let secret = b"0123456789abcdef0123456789abcdef".to_vec(); // load yours from a secret store
let interceptor = JwtAuthInterceptor::new(
    JwtValidator::new().try_with_hs256_secret(secret)?,
    Jwks::new(),
);
# Ok(())
# }
```

### Mutual TLS: the client certificate names the caller

With TLS on the listener and a rustls configuration whose client verifier
trusts your CAs (`rustls::server::WebPkiClientVerifier`, passed to
`Server::with_tls` or `WebSocketDispatcher::with_tls`; for gRPC,
`ServerTlsConfig::client_ca_root`), the certificate a client presents
reaches the interceptors as `CallContext::peer_certificate`, the DER of its
leaf. `MutualTlsAuthInterceptor` (`tls-rustls` or `grpc-tls`) names the
caller from it and refuses a call without one (`401`, `WWW-Authenticate:
MutualTLS`; gRPC `UNAUTHENTICATED`):

```rust,no_run
use a2a_protocol_server::auth::MutualTlsAuthInterceptor;
# use a2a_protocol_sdk::prelude::*;
# struct MyAgent;
# agent_executor!(MyAgent, |_ctx, _queue| async { Ok(()) });
# fn main() -> Result<(), Box<dyn std::error::Error>> {

// `mtls:sha256:<fingerprint>`: exact, needs no parser, changes on renewal.
let by_fingerprint = MutualTlsAuthInterceptor::new();

// `mtls:<name>` from your own mapping; `None` refuses the certificate. To
// name callers by subject, parse `der` with the X.509 crate you trust.
let by_name = MutualTlsAuthInterceptor::with_names(|der| {
    (der.len() > 0).then(|| "billing-agent".to_owned())
});
# let _ = by_fingerprint;
let handler = RequestHandlerBuilder::new(MyAgent)
    .with_interceptor(by_name)
    .build()?;
# let _ = handler;
# Ok(())
# }
```

The handshake verifies the certificate; the interceptor only names it. A
verifier that also admits clients without one
(`allow_unauthenticated()`, `client_auth_optional(true)`) leaves the refusal
to the interceptor, so the same listener can serve the agent card to anyone.
Behind a proxy that terminates TLS, the listener sees the proxy's handshake,
not the client's, and there is no client certificate to read.

### Security properties

- **Only HS256/RS256/ES256 are accepted.** `alg: none` and any unlisted
  algorithm are rejected. HS256 is only ever verified against a configured
  secret — never a JWKS public key — so the RS256→HS256 algorithm-confusion
  downgrade is structurally impossible.
- **HS256 secrets are at least 32 bytes.** RFC 7518 §3.2 requires an HS256
  key at least as long as the hash output. `try_with_hs256_secret` returns an
  error for a shorter one; `with_hs256_secret` fails closed instead — it logs
  an error and rejects every HS256 token — so a short secret can never be
  the thing standing between a guesser and a valid token.
- **Rejections are generic.** A missing, malformed, expired, or wrong token all
  produce the same error, so the response can't be used as an oracle.
- **Constant-time comparison** for API keys and static bearer tokens.

### The card and the interceptors must agree

`RequestHandlerBuilder::build` compares the authentication the agent card
declares with the interceptors that enforce it (since 0.15):

- A card whose `securityRequirements` (or a skill's) ask for a credential is
  refused when no registered interceptor authenticates, because every request
  would be served without it (spec §3.3.2, §7.4). An empty requirement `{}`
  makes authentication optional, as in OpenAPI, and asks for nothing. When a
  gateway, mesh or sidecar in front of the process checks the credential,
  say so with `authenticated_upstream()`.
- An authenticating interceptor that checks none of the schemes the card
  declares is refused: §13.3 requires requests to be authenticated "using one
  of the schemes declared in the public `AgentCard`", and a client following
  the card could never pass it. `BearerTokenAuthInterceptor` and
  `JwtAuthInterceptor` check the HTTP `bearer` scheme and OAuth 2.0 or OpenID
  Connect schemes (whose access tokens are bearer tokens);
  `ApiKeyAuthInterceptor` checks an `apiKeySecurityScheme` in the header it
  reads, compared without regard to case. A custom interceptor answers
  through `ServerInterceptor::checks_scheme`; one that does not is not
  checked.

### Throttling guessed credentials

Spec §13.4 asks agents to rate-limit against brute force. `RateLimitInterceptor`
cannot do it: registered after the authenticator, it never sees a refused
credential; registered before, it cannot tell one unidentified caller from
another. `AuthFailureThrottle` counts, per peer, the calls an authenticator
refused as unauthenticated, and once a peer reaches `max_failures` in a window
it refuses that peer's calls as rate-limited until the window ends, the right
credential included, so a guesser cannot tell a hit from a miss. Register it
**before** the authenticator:

```rust
# fn card() -> a2a_protocol_types::agent_card::AgentCard {
#     use a2a_protocol_types::agent_card::{AgentCard, AgentInterface, AgentSkill};
#     AgentCard::new("my-agent", "1.0.0", AgentInterface::jsonrpc("http://localhost:3000"))
#         .with_description("An example agent")
#         .with_skill(AgentSkill::new("echo", "Echo", "Echoes text").with_tags(["echo"]))
# }
# use a2a_protocol_sdk::prelude::agent_executor;
# struct MyAgent;
# agent_executor!(MyAgent, |_ctx, _queue| async { Ok(()) });
# fn main() -> Result<(), Box<dyn std::error::Error>> {
use a2a_protocol_server::{
    AuthFailureThrottle, AuthFailureThrottleConfig, BearerTokenAuthInterceptor,
    RequestHandlerBuilder,
};

let throttle = AuthFailureThrottle::new(
    AuthFailureThrottleConfig::default()
        .with_max_failures(10) // refused credentials per peer per window
        .with_window_secs(300),
)?;
let handler = RequestHandlerBuilder::new(MyAgent)
    .with_agent_card(card())
    .with_interceptor(throttle)
    .with_interceptor(BearerTokenAuthInterceptor::new(["service-token-1"]))
    .build()?;
# Ok(())
# }
```

A throttled call answers `429` with `Retry-After` on HTTP+JSON, `200` with a
`-32603` body on JSON-RPC, and `RESOURCE_EXHAUSTED` on gRPC; the last two
carry a `RetryInfo` detail. Each gives the seconds until the window ends.

A peer is the IP address of the connection (`CallContext::peer_addr`), which
every binding records when the SDK serves it. The axum router has it only when
served with `into_make_service_with_connect_info::<SocketAddr>()`. Behind
reverse proxies, set `with_trusted_proxy_hops` to the number of proxies so the
client address they append to `x-forwarded-for` is counted instead. A call
with no known address is not throttled.

What to weigh:

- Everyone behind one address (a NAT, an untrusted proxy) shares a count, so a
  peer that keeps failing locks the others there out for the window.
- A failure is counted when its call ends, so calls already in flight when a
  peer reaches the limit are still checked.
- At most `max_peers` peers are tracked (10,000 by default). When the table is
  full, peers whose window has passed are dropped, and a new peer that still
  does not fit is not counted.

### Error mapping (important)

A refused credential answers with each binding's own status (since 0.14.0;
[ADR 0014](https://github.com/tomtom215/a2a-rust/blob/main/docs/adr/0014-auth-rejection-status.md)):

| Binding | No usable credential | Credential not permitted |
|---|---|---|
| HTTP+JSON | `401` + `WWW-Authenticate` | `403` |
| JSON-RPC over HTTP | `401` + `WWW-Authenticate`, body `-32600` | `403`, body `-32600` |
| gRPC | `UNAUTHENTICATED` | `PERMISSION_DENIED` |
| WebSocket | body `-32600` only (see below) | body `-32600` only |

JSON-RPC batches answer `200`, since one response answers many calls; each
refused entry's body is `-32600`.

The built-in interceptors send `Bearer realm="a2a"` (bearer and JWT) or
`ApiKey header="x-api-key"` as the challenge, and never say why a credential
was refused. A client that refreshes on `401` — this SDK's
`BearerAuthInterceptor` does — recovers from a revoked or rotated token on
its next call. A custom interceptor gets the same statuses by returning
`A2aError::unauthenticated(message, challenge)` or
`A2aError::permission_denied(message)`.

WebSocket runs interceptors per message after the connection is upgraded, so
there is no HTTP status to send; authenticate at your gateway or reverse
proxy if WebSocket clients need a `401`.

Through 0.13 every refusal answered `400` / `INVALID_ARGUMENT`; see
**Behaviour Changes** in the changelog.

### Authorization: scopes and per-task checks

Spec §13.1 requires an authorization check on every operation, and §3.3.2 a
`403` when the authenticated caller lacks a permission, naming the missing
scope where that leaks nothing. Two pieces help.

**A scope every call needs.** `JwtValidator::require_scope` refuses a token
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

**A decision per operation or per task.** An interceptor sees the method
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

## Client: acquiring and attaching tokens

### A token you already have

```rust
# fn main() -> Result<(), Box<dyn std::error::Error>> {
use std::sync::Arc;
use a2a_protocol_client::{BearerAuthInterceptor, ClientBuilder, StaticTokenProvider};

let provider = Arc::new(StaticTokenProvider::new("my-long-lived-token"));
let client = ClientBuilder::new("https://agent.example.com")
    .with_interceptor(BearerAuthInterceptor::new(provider))
    .build()?;
# Ok(())
# }
```

`BearerAuthInterceptor` asks its provider for a token before **every** request,
so a provider that refreshes keeps a long-lived client authenticated across
token rotations. When the agent answers `401` (surfaced as
`ClientError::UnexpectedStatus { status: 401 }` over JSON-RPC, REST and gRPC,
and for a WebSocket handshake refused with `401`; a refusal inside an open
WebSocket connection is a `-32600` body and does not trigger this), the
interceptor calls
`TokenProvider::invalidate` with the token it sent. The call that got the `401`
still fails, but the next one fetches a new token instead of resending the
refused one until it expires. A custom `TokenProvider` gets this only if it
overrides `invalidate`; the default does nothing.

**Plaintext.** Spec §13.4 says credentials SHOULD travel only over encrypted
connections. A client whose endpoint is `http://` or `ws://` on a host other
than loopback logs a warning, once, the first time a call carries a
credential: an `Authorization`, `Proxy-Authorization` or `Cookie` header, or
the header or query parameter an `AuthInterceptor::api_key` writes.
`ClientBuilder::require_https_credentials()` refuses such a call instead,
before sending it, with `ClientError::InsecureTransport` naming the header and
the endpoint's origin. `http://localhost` and other loopback addresses are
allowed either way.

**Declared schemes.** Spec §3.1.11 says the client MUST authenticate with one
of the schemes the agent card declares. A client built with
`ClientBuilder::from_card` checks each call, after its interceptors run, that
it meets at least one of the card's `securityRequirements`: `Authorization`
with the declared HTTP scheme (compared without case; `Bearer` for OAuth 2.0
and OpenID Connect), or the header, query parameter or cookie an API-key
scheme names, whichever interceptor wrote it. A mutual-TLS scheme counts as
met, since the certificate is not in the request. By default a mismatch is
sent and logged once per client;
`.with_credential_preflight(CredentialPreflight::Refuse)` refuses it before
sending, with `ClientError::InvalidAuthScheme` naming the card's schemes and
the request's header, cookie and parameter names, never their values, and
`CredentialPreflight::Off` turns the check off. A client built for a bare
endpoint has no card and checks nothing.

### OAuth 2.0 client credentials

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

**From the agent card** — an agent that advertises an OAuth2 client-credentials
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

**From an OIDC issuer** — discovers the `token_endpoint`:

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

When the token endpoint fails, every caller waiting on that refresh gets the
same error from the one request: a dead endpoint costs them one
`with_request_timeout` together, not one each in turn. The failure is then
remembered for `with_failure_backoff` (default 1 s; `Duration::ZERO` turns it
off), so a tight loop makes at most one attempt per backoff. A caller cancelled
mid-refresh does not strand the others; the next one starts a new attempt.

The client secret is never logged, never echoed in an error, and redacted from
`Debug`. `Basic` client authentication is the default; switch to form-body
credentials with `.with_auth_style(TokenEndpointAuthStyle::Post)`.

### Delegating for a principal: RFC 8693 token exchange

An agent serving Alice that delegates to another agent should send neither
its own credential (it names only the agent) nor Alice's (the next agent
was never meant to accept it). It exchanges the token it was called with at
the identity provider for one issued for the next agent: `sub` is still
Alice, and `act` names the agent acting for her (ADR 0021).

```rust,no_run
use a2a_protocol_client::token_provider::{ExchangeRequest, TokenExchange};
# async fn run(alices_token: String) -> Result<(), Box<dyn std::error::Error>> {
let idp = TokenExchange::new("https://idp.example/oauth/token", "planner-agent", "secret");
let delegated = idp
    .exchange(
        &ExchangeRequest::new(alices_token)
            .with_audience("https://worker.example")
            .with_scopes(["tasks:read"]),
    )
    .await?;
# let _ = delegated;
# Ok(())
# }
```

Send the call with `delegated.access_token`. The exchanged token belongs to
one principal, so give it to that call alone, for example on a client
built with `StaticTokenProvider`, not to a client every principal shares.
On the receiving agent, `JwtAuthInterceptor` makes Alice the caller
identity and lists the acting agents in `CallContext::actors`, nearest
first. With `audit`, every call record names both. The identity provider
decides what may be delegated, to whom, and with which scopes.

### Interactive flows

Authorization-code (browser redirect) and device-code grants are interactive
and out of scope for agent-to-agent auth. Implement the `TokenProvider` trait
with your own flow and pass it to `BearerAuthInterceptor`.
