<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Interceptors, API keys and extensions

What the client adds to every request: hooks that run around each call, credentials, and the extensions it asks the agent to use. Authentication beyond a static token is covered from the agent's side in [Authentication](../../building-agents/authentication.md).

## Interceptors

Add request/response hooks:

```rust
# use a2a_protocol_sdk::client::ClientBuilder;
# let url = "http://agent.example.com";
use std::sync::Arc;
use a2a_protocol_client::{
    BearerAuthInterceptor, StaticTokenProvider, TracePropagationInterceptor,
};

let token = Arc::new(StaticTokenProvider::new("my-token"));
let client = ClientBuilder::new(url)
    .with_interceptor(BearerAuthInterceptor::new(token))
    .with_interceptor(TracePropagationInterceptor)
    .build()
    .unwrap();
```

Your own hooks implement `CallInterceptor` and are added the same way. A hook
receives each call as a `ClientRequest`: `method`, `params`, `extra_headers` and,
since 0.15.0, `query_params`, which the JSON-RPC and HTTP+JSON transports append
to the request URL; the gRPC and WebSocket transports refuse a call that carries
any, with `InvalidAuthScheme`. Build one with `ClientRequest::new`: a struct
literal naming every field no longer compiles. Override `on_error` to observe a
call the transport failed.

## API keys

An agent whose card declares an `APIKeySecurityScheme` (spec §4.5.2) reads
its key from the header, cookie or query parameter the scheme names, so
`AuthInterceptor::api_key` sends it there; `AuthInterceptor::with_scheme(..,
"api-key")` writes `Authorization`, which such an agent never reads:

```rust
# use a2a_protocol_sdk::client::ClientBuilder;
# let url = "http://agent.example.com";
use std::sync::Arc;
use a2a_protocol_client::auth::{
    AuthInterceptor, CredentialsStore, InMemoryCredentialsStore, SessionId,
};
use a2a_protocol_types::security::{ApiKeyLocation, ApiKeySecurityScheme};

// As the agent's card declares it in `securitySchemes`.
let scheme = ApiKeySecurityScheme {
    location: ApiKeyLocation::Header,
    name: "X-API-Key".into(),
    description: None,
};
let store = Arc::new(InMemoryCredentialsStore::new());
let session = SessionId::new("me");
store.set(session.clone(), "api-key", "my-key".into());
let client = ClientBuilder::new(url)
    .with_interceptor(AuthInterceptor::api_key(store, session, "api-key", &scheme).unwrap())
    .build()
    .unwrap();
```

A scheme that places the key in the query string is sent as a query parameter
by the JSON-RPC and HTTP+JSON transports. The gRPC and WebSocket transports
have no query string and refuse such a call with
`ClientError::InvalidAuthScheme`. A URL can reach server and proxy access
logs, so prefer the header when the agent offers both. `api_key` itself
returns `InvalidAuthScheme` only for a name that is not an HTTP token.

## Extensions

Ask the agent to use extensions (spec §3.2.6, `A2A-Extensions`):

```rust
# use a2a_protocol_sdk::client::ClientBuilder;
# let url = "http://agent.example.com";
let client = ClientBuilder::new(url)
    .with_extensions([
        "https://example.com/extensions/geolocation/v1",
        "https://standards.org/extensions/citations/v1",
    ])
    .build()
    .unwrap();
```

Every request carries the URIs in one comma-separated `A2A-Extensions` header
(gRPC metadata on gRPC). An agent uses those it supports; one that declares an
extension `required` refuses a request that does not ask for it with
`ExtensionSupportRequired` (§3.3.4). A client built with `from_card` checks
this itself: if the card marks an extension `required` that `with_extensions`
does not list, `build()` fails with a `ClientError::Protocol` whose code is
`ExtensionSupportRequired` and whose message names the extension, instead of
having every call refused. Ask for the extension with `with_extensions`, or call
`skip_required_extension_check()` when an interceptor of yours sends the
`A2A-Extensions` header. A client built with `new(url)` has no card and is not
checked. An interceptor that sets the header itself decides for that request.
A `WebSocketTransport` sends headers only with its upgrade, so give it the URIs
with `WebSocketTransportConfig::with_extensions`; another set needs another
connection. The build refuses a URI that is empty or holds a comma,
whitespace or anything but visible ASCII.
