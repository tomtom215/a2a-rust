<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# gRPC client

A client for an agent that serves the gRPC binding, built from the same `ClientBuilder` with `build_grpc()`, or from a `GrpcTransport` you construct yourself.

> Requires the `grpc` feature: `a2a-protocol-client = { version = "0.15", features = ["grpc"] }`
>
> An `https://` endpoint, or a card's bare `host:port` on a host other than
> loopback (dialled with TLS by default), also needs the `grpc-tls` feature
> (`features = ["grpc-tls"]` includes `grpc`); without it `build_grpc` fails with
> "gRPC over TLS needs the `grpc-tls` feature".

`ClientBuilder::new(url).build_grpc().await?` (or `from_card(&card)?.build_grpc()`)
builds a gRPC client that uses this builder's timeouts and limits; a transport
passed to `with_custom_transport` keeps only its own `GrpcTransportConfig`.

To construct the transport yourself, use `GrpcTransport::connect()` with `with_custom_transport()`:

```rust,no_run
# use a2a_protocol_sdk::client::ClientBuilder;
# #[tokio::main]
# async fn main() -> Result<(), Box<dyn std::error::Error>> {
use a2a_protocol_client::GrpcTransport;

let transport = GrpcTransport::connect("http://agent.example.com:50051").await?;
let client = ClientBuilder::new("http://agent.example.com:50051")
    .with_custom_transport(transport)
    .build()?;
# Ok(())
# }
```

Configure with `GrpcTransportConfig`:

```rust,no_run
# #[tokio::main]
# async fn main() -> Result<(), Box<dyn std::error::Error>> {
use a2a_protocol_client::transport::grpc::{GrpcTransport, GrpcTransportConfig};
use std::time::Duration;

let config = GrpcTransportConfig::default()
    .with_timeout(Duration::from_secs(60))
    .with_max_message_size(8 * 1024 * 1024);

let transport = GrpcTransport::connect_with_config(
    "http://agent.example.com:50051",
    config,
).await?;
# Ok(())
# }
```
