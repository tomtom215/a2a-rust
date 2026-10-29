<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Attaching tokens to a client

Send a credential with every call a client makes. `BearerAuthInterceptor` asks a `TokenProvider` for a token before each request; this page uses `StaticTokenProvider` for a token you already hold. To fetch tokens from an authorization server, see [OAuth 2.0 client credentials](./oauth-client-credentials.md).

## A token you already have

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
`ClientError::UnexpectedStatus { status: 401 }` on every binding: over
JSON-RPC, REST and gRPC, for a WebSocket handshake refused with `401`, and
for a refusal inside an open WebSocket connection, whose `-32600` body
carries an `UNAUTHENTICATED` detail), the interceptor calls
`TokenProvider::invalidate` with the token it sent. The call that got the `401`
still fails, but the next one fetches a new token instead of resending the
refused one until it expires. A custom `TokenProvider` gets this only if it
overrides `invalidate`; the default does nothing.
Over WebSocket the credential is the one sent with the upgrade, so a new
token reaches the agent only on a new connection
([WebSocket binding §6](../../bindings/websocket.md#6-authentication-126)).

## Refusing plaintext

Spec §13.4 says credentials SHOULD travel only over encrypted
connections. A client whose endpoint is `http://` or `ws://` on a host other
than loopback logs a warning, once, the first time a call carries a
credential: an `Authorization`, `Proxy-Authorization` or `Cookie` header, or
the header or query parameter an `AuthInterceptor::api_key` writes.
`ClientBuilder::require_https_credentials()` refuses such a call instead,
before sending it, with `ClientError::InsecureTransport` naming the header and
the endpoint's origin. `http://localhost` and other loopback addresses are
allowed either way.

## Checking the card's schemes

Spec §3.1.11 says the client MUST authenticate with one
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

## Interactive flows

Authorization-code (browser redirect) and device-code grants are interactive
and out of scope for agent-to-agent auth. Implement the `TokenProvider` trait
with your own flow and pass it to `BearerAuthInterceptor`.
