<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Proxies, Certificates and DNS

What changes when an agent runs behind a reverse proxy, rotates its
certificates, or resolves names inside Kubernetes. Each section names the
setting; the defaults are in the [configuration reference](../../reference/configuration.md).

## A path prefix from the reverse proxy

A proxy that forwards `https://gateway.example.com/agents/billing/...` to the
agent with the prefix kept (nginx `location /agents/billing/ { proxy_pass
http://agent:8080; }`, with no path on `proxy_pass`) sends the agent
`/agents/billing/message:send`. The HTTP+JSON binding reads a first segment
before a route as a tenant, so without configuration that request runs as
tenant `agents` on a route that does not exist, and the agent card is not
found under the prefix.

Either strip the prefix at the proxy (`proxy_pass http://agent:8080/;`, with
the trailing slash), or tell the dispatchers about it:

```rust,no_run
use std::sync::Arc;
use a2a_protocol_sdk::server::{CombinedDispatcher, DispatchConfig, RequestHandler, ServerResult};

fn dispatcher(handler: Arc<RequestHandler>) -> ServerResult<CombinedDispatcher> {
    // A path that is not `/` and plain segments is refused here.
    let config = DispatchConfig::default().with_base_path("/agents/billing")?;
    Ok(CombinedDispatcher::with_config(handler, config))
}
```

`with_base_path` is read by `RestDispatcher`, `JsonRpcDispatcher`,
`CombinedDispatcher` and the axum `RestRouter`. A request under the prefix is
routed as though the prefix were not there: the card is at
`/agents/billing/.well-known/agent-card.json`, a tenant is the segment after
the prefix, and `PathSegmentTenantResolver` sees the path without it. A
request outside the prefix (a health probe that reaches the pod directly) is
routed as before. The card's interface URLs must include the prefix, since
they are what clients call.

## Who the client is

Behind proxies, the rate limiter and `AuthFailureThrottle` key an
unidentified caller by the address the proxies record. Set both the hop count
and the proxies' address ranges:

```rust,no_run
use a2a_protocol_sdk::server::{ForwardedHeader, RateLimitConfig, TrustedProxy};

fn limits() -> Result<RateLimitConfig, Box<dyn std::error::Error>> {
    Ok(RateLimitConfig::default()
        .with_trusted_proxy_hops(1)
        .with_trusted_proxies(["10.0.0.0/8".parse::<TrustedProxy>()?])
        // The proxy writes RFC 7239 `Forwarded`; the default is X-Forwarded-For.
        .with_forwarded_header(ForwardedHeader::Forwarded))
}
```

Hops without ranges believe the header from any peer, so a client that
reaches the agent directly can choose its own key; building the interceptor
logs a warning in that case. Only the configured header is read.

## Mutual TLS behind a TLS-terminating proxy

When Envoy, Istio, nginx or an AWS Application Load Balancer terminates the
client's TLS, the agent's connection carries no client certificate and
`MutualTlsAuthInterceptor` refuses every call. Have the proxy verify the
certificate and forward it, and tell the interceptor which peers may:

```rust,no_run
use a2a_protocol_sdk::server::TrustedProxy;
use a2a_protocol_sdk::server::auth::{ForwardedClientCert, MutualTlsAuthInterceptor};

fn auth() -> Result<MutualTlsAuthInterceptor, Box<dyn std::error::Error>> {
    let proxies = ["10.0.0.0/8".parse::<TrustedProxy>()?];
    Ok(MutualTlsAuthInterceptor::new()
        // Envoy/Istio `x-forwarded-client-cert` with `Cert` in its details;
        // `url_encoded_pem("x-ssl-client-cert", ..)` for nginx's
        // `$ssl_client_escaped_cert`; `aws_alb(..)` for `x-amzn-mtls-clientcert`.
        .with_forwarded(ForwardedClientCert::envoy(proxies)))
}
```

The header is read only when the connection's own handshake presented no
certificate and its peer is in one of the ranges; from anywhere else it is
ignored, and with no ranges it is never read. The proxy must replace a header
of that name the client sent (Envoy `forward_client_cert_details:
SANITIZE_SET`; nginx `proxy_set_header` replaces), since the agent does not
verify the forwarded certificate: the proxy did.

## Rotating certificates without a restart

`ReloadingCertResolver` serves the certificate in two PEM files and replaces
it when they change. Every listener that takes a `rustls::ServerConfig` can use
it, the gRPC one through `GrpcDispatcher::with_rustls`:

```rust,no_run
use std::sync::Arc;
use std::time::Duration;
use a2a_protocol_sdk::server::{JsonRpcDispatcher, ReloadingCertResolver, RequestHandler};
use a2a_protocol_sdk::server::serve::Server;

async fn run(
    handler: Arc<RequestHandler>,
    shutdown: impl std::future::Future<Output = ()> + Send,
) -> std::io::Result<()> {
    let certs = Arc::new(ReloadingCertResolver::from_pem_files(
        "/etc/a2a/tls/tls.crt",
        "/etc/a2a/tls/tls.key",
    )?);
    let _watch = certs.watch(Duration::from_secs(60));
    let report = Server::bind("0.0.0.0:8443")
        .await?
        .with_tls(certs.server_config()?)
        .serve_with_shutdown(JsonRpcDispatcher::new(handler), shutdown)
        .await;
    let _ = report;
    Ok(())
}
```

`watch` re-reads the files every interval; `reload()` does it once (call it
from a `SIGHUP` handler of your own). A pair that does not load, as when the
two files are caught between their writes, keeps the certificate already
served and is retried. New handshakes get the new certificate; established
connections keep theirs. For mutual TLS, build the configuration yourself with
`with_client_cert_verifier(..)` and `.with_cert_resolver(certs)`.

## The crypto provider, FIPS and revocation

Every configuration the SDK builds for you uses rustls' safe defaults (TLS 1.2
and 1.3, AEAD suites) on the `ring` provider. To use another provider (the
`aws-lc-rs` crate, or its FIPS build), or to check client certificates against
CRLs, build the `rustls::ServerConfig` yourself and pass it to `with_tls` (or
`with_rustls` for gRPC); `ReloadingCertResolver::with_provider` loads its key
with the provider you give. rustls checks CRLs through
`WebPkiClientVerifier::builder(roots).with_crls(crls)`. There is no OCSP
helper, no built-in FIPS feature, and no mode that skips verification
([ADR 0041](https://github.com/tomtom215/a2a-rust/blob/main/docs/adr/0041-server-certificate-reload-and-crypto-policy.md)).

## Idle peers on the gRPC and WebSocket listeners

Both listeners now probe idle peers by default: the WebSocket listener pings a
connection after 37.5 s of silence and closes it at 75 s if nothing answered
(`with_idle_timeout`, `without_idle_timeout`), and the gRPC listener sends an
HTTP/2 PING after 60 s of silence and closes the connection if it is not
answered within 20 s (`with_http2_keepalive`, `without_http2_keepalive`). Any
conformant client answers without its application being involved, so a quiet
subscription stays open; a peer that went away without closing does not.
Neither listener caps connections by default: set
`ServeConfig::with_max_connections` for your fan-in, through each dispatcher's
`with_serve_config` ([ADR 0042](https://github.com/tomtom215/a2a-rust/blob/main/docs/adr/0042-listener-liveness-defaults.md)).

## DNS inside Kubernetes

The client's name lookup is bounded by its connect timeout (`ClientBuilder::
with_connection_timeout`, 10 s by default); before 0.16.0 the timeout started
only after the lookup returned. A pod's default `ndots: 5` makes a name with
fewer than five dots go through every search domain before it is tried as
written, each try a round of DNS queries. For an agent that calls names
outside the cluster, set the pod's `dnsConfig.options` to `ndots: 1` (or `2`).
A trailing dot on the name (`agent.example.com.`) also skips the search, but
the dot then reaches the server in `Host` and in the TLS name check, which not
every server accepts; the SDK's tests do not cover it. The SDK does not cache lookups: the
system resolver, a node-local DNS cache, or the connection pool's reuse of an
open connection already do, and a cache here would outlive the records' TTLs.
