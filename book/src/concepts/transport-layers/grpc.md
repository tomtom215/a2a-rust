<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# gRPC

The **gRPC** transport (`grpc` feature flag) provides high-performance RPC via protocol buffers and HTTP/2. As of 0.7 it is **protobuf-native**: the transport speaks the canonical `lf.a2a.v1.A2AService` service with fully-typed messages generated from the A2A specification's protobuf schema, making it wire-compatible with the official Go, Python, and Java SDKs.

> **History:** Through 0.6 it tunneled JSON inside a protobuf `bytes` field on a non-standard service. The tunnel **client** was removed in 0.7; the tunnel **service** and the `grpc-legacy-json` feature that served it alongside the canonical one were removed in 0.8. A 0.6 client must now upgrade rather than be tunneled for.

```toml
# Server
a2a-protocol-server = { version = "0.16", features = ["grpc"] }

# Client
a2a-protocol-client = { version = "0.16", features = ["grpc"] }
```

## Serving it

Serve the binding with `GrpcDispatcher`: its `GrpcConfig`, connection
bounds, pre-bound listeners and `into_service()` are in
[Dispatchers](../../building-agents/dispatchers/grpc.md).

Declare the binding on the card you give the handler
(`AgentInterface::grpc("agent.example.com:50051")`). The listener logs a
warning when it builds its router, with the `tracing` feature, if the card
declares no `GRPC` interface (spec §5.2).

## Client

This section covers how a client dials the target a card advertises. Building
a gRPC client and `GrpcTransportConfig` are covered in
[Building a Client](../../client/builder/grpc.md).

An Agent Card's gRPC interface advertises a gRPC **target** — `host:port`, as
the A2A proto's `AgentInterface.url` comment has said since upstream commit
`cfc9d34` (2026-07-21); the v1.0.1 copy this repository ships still says "a
valid absolute HTTPS URL" — not a URL, because gRPC names carry no scheme.
`GrpcTransport::connect` and `ClientBuilder::build_grpc` accept that form, and
decide how to dial it with a `GrpcBareAddressScheme`:

| Policy | Bare `host:port` is dialled… | When |
|---|---|---|
| `HttpsExceptLoopback` (default) | with TLS, except `localhost` / `127.0.0.0/8` / `::1` in plaintext | the spec requires TLS in production (§13); a loopback peer is this machine |
| `Https` | always with TLS | a loopback TLS terminator, or to remove the exception |
| `Http` | always in plaintext | a private network whose agents advertise `agent:50051` and terminate TLS in a mesh or sidecar |

An address that already carries `http://` or `https://` is used as-is.
`https://` — explicit or chosen by the policy — needs the `grpc-tls` feature;
without it the connect fails with a message naming the feature rather than
attempting a plaintext handshake against a TLS port.

```rust,no_run
# use a2a_protocol_sdk::prelude::AgentCard;
# async fn f(card: AgentCard, ca_pem: Vec<u8>) -> Result<(), Box<dyn std::error::Error>> {
use a2a_protocol_client::{ClientBuilder, GrpcBareAddressScheme};

// From a card whose gRPC interface says "grpc.example.com:443": TLS,
// verified against the bundled Mozilla roots (needs `grpc-tls`).
let client = ClientBuilder::from_card(&card)?.build_grpc().await?;

// A Compose network where the card says "analyzer:50051" and the mesh
// terminates TLS: plaintext, on purpose.
let client = ClientBuilder::from_card(&card)?
    .with_grpc_bare_address_scheme(GrpcBareAddressScheme::Http)
    .build_grpc()
    .await?;

// A private CA, pinned (needs `grpc-tls`). The TLS types are re-exported
// from `transport::grpc`, so no tonic dependency of your own.
use a2a_protocol_client::transport::grpc::{Certificate, ClientTlsConfig};

let tls = ClientTlsConfig::new()
    .ca_certificate(Certificate::from_pem(ca_pem))
    .domain_name("agent.internal");
let client = ClientBuilder::from_card(&card)?
    .with_grpc_tls_config(tls)
    .build_grpc()
    .await?;
# Ok(())
# }
```

## Serving TLS

The server's gRPC listener is plaintext with the `grpc` feature — the shape
the official SDKs' gRPC servers expect, with TLS terminated in a proxy or
mesh. With `grpc-tls` on `a2a-protocol-server` it can serve TLS itself:
`GrpcDispatcher::with_tls` takes a `ServerTlsConfig` carrying the server
certificate and key and, for mutual TLS, the CA that client certificates
must chain to. The types are re-exported from `dispatch::grpc`, so no tonic
dependency of your own. The listener then speaks TLS only; a plaintext
client is refused at the handshake rather than served.

```rust
# use std::sync::Arc;
# use a2a_protocol_server::RequestHandler;
use a2a_protocol_server::dispatch::grpc::{
    Certificate, GrpcConfig, GrpcDispatcher, Identity, ServerTlsConfig,
};

# async fn example(
#     handler: Arc<RequestHandler>,
#     cert_pem: &str,
#     key_pem: &str,
#     client_ca_pem: &str,
# ) -> std::io::Result<()> {
let tls = ServerTlsConfig::new()
    .identity(Identity::from_pem(cert_pem, key_pem))
    // Mutual TLS: clients must present a certificate this CA signed.
    // Add `.client_auth_optional(true)` to admit clients that present none.
    .client_ca_root(Certificate::from_pem(client_ca_pem));

GrpcDispatcher::new(handler, GrpcConfig::default())
    .with_tls(tls)
    .serve("0.0.0.0:50051")
    .await?;
# Ok(())
# }
```

A configuration tonic rejects — a key that does not match its certificate —
comes back from the `serve*` call as an `std::io::Error`, not a panic. tonic
builds its acceptor from the process-level rustls crypto provider; when none
is installed and more than one is linked, the dispatcher installs `ring`
rather than letting rustls panic, and never overrides a provider the
application installed first.

## Protocol

- All 11 A2A methods are mapped to gRPC RPCs on the canonical `lf.a2a.v1.A2AService`
- Streaming methods (`SendStreamingMessage`, `SubscribeToTask`) use gRPC server streaming
- Messages are the fully-typed `lf.a2a.v1` protobuf types, converted to/from the serde domain types via a bidirectional `TryFrom` layer (ProtoJSON semantics)
- The canonical schema lives at `proto/a2a_v1/a2a.proto`, kept byte-identical to the specification copy

## When to use gRPC

- **Service mesh integration** — gRPC is native to Kubernetes, Istio, Envoy
- **Language interop** — gRPC has code generation for 10+ languages
- **HTTP/2 multiplexing** — Multiple RPCs over a single connection
- **Streaming** — Native server streaming without SSE
