# Production Hardening

What changes between an agent that runs on your laptop and one that serves
traffic. The checklist is the whole job in one list, and each line links to
the page that explains it. This page also covers the two settings everything
else assumes: the production profile and TLS.

| You want to | Read |
|---|---|
| Bound how long work runs, and how much one caller can take | [Timeouts and limits](./production/limits.md) |
| Stop or roll a replica without cutting its streams | [Graceful shutdown](./production/graceful-shutdown.md) |
| Give a load balancer or Kubernetes something to probe | [Health checks](./health-checks.md) |
| Serve browsers, or deliver push notifications safely | [Browsers and webhooks](./production/browsers-and-webhooks.md) |
| Get more requests out of each core | [Performance tuning](./production/performance.md) |
| Build an image and run it on Kubernetes | [Docker and Kubernetes](./containers.md) |
| Run more than one replica | [Running More Than One Replica](./horizontal-scaling.md) |
| Know what the SDK guarantees, and what it does not | [Security](./security.md) |

## Deployment checklist

- [ ] `Profile::Production` and `ServeConfig::production()` applied, or each refusal they make accounted for ([below](#the-production-profile))
- [ ] HTTPS termination configured ([below](#https))
- [ ] CORS origins restricted to known clients ([CORS](./production/browsers-and-webhooks.md#cors))
- [ ] Executor timeout set ([Timeouts and limits](./production/limits.md#executor-timeout))
- [ ] Max concurrent streams limited ([Timeouts and limits](./production/limits.md#concurrent-stream-limits))
- [ ] `ServeConfig::with_max_connections` set (connections are unbounded by default); header-read (30 s) and idle (75 s) timeouts left on ([Connections](./production/limits.md#connections))
- [ ] Task store TTL and capacity configured ([Timeouts and limits](./production/limits.md#task-store-limits))
- [ ] Rate limiting enabled for public-facing agents ([Rate limiting](./production/limits.md#rate-limiting))
- [ ] A2A clients built once and reused (not per-request) ([Performance tuning](./production/performance.md#client-retry--reuse))
- [ ] Structured logging enabled ([below](#logs-metrics-and-traces))
- [ ] Health check endpoint available ([Health checks](./health-checks.md))
- [ ] Push notification URLs restricted to HTTPS (`HttpPushSender::https_only`) ([Push notification security](./production/browsers-and-webhooks.md#push-notification-security))
- [ ] Body/query size limits verified ([Input handling](./security.md#input-handling))
- [ ] Graceful shutdown implemented ([Graceful shutdown](./production/graceful-shutdown.md))
- [ ] Server runs on jemalloc or mimalloc, not glibc's default allocator ([Allocator](./production/performance.md#allocator))

## Security

### The production profile

Two presets turn the startup warnings and the safety settings a delegated
deployment needs into refusals (ADR 0025). Neither changes a default.
`Profile::Production` makes `build()` refuse a handler with no
authenticating interceptor, no halt store, no lease store, or no content
signer (ADR 0027, feature `signing`).
`ServeConfig::production()` makes the server return at once, accepting
nothing, when it would serve plaintext beyond loopback or a binding the agent
card does not declare; `ServeReport::refused` says why.

```rust,no_run
use std::sync::Arc;
use a2a_protocol_server::dispatch::JsonRpcDispatcher;
use a2a_protocol_server::profile::Profile;
use a2a_protocol_server::serve::{ServeConfig, Server, shutdown_signal};
use a2a_protocol_server::store::{InMemoryHaltStore, InMemoryLeaseStore};
use a2a_protocol_server::{BearerTokenAuthInterceptor, RequestHandlerBuilder, agent_executor};
use a2a_protocol_types::agent_card::{AgentCard, AgentInterface, AgentSkill};
use a2a_protocol_types::provenance::ContentSigner;

struct Agent;
agent_executor!(Agent, |_ctx, _queue| async { Ok(()) });

async fn run(signing_key_pkcs8: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
    let card = AgentCard::new("agent", "1.0.0", AgentInterface::jsonrpc("https://agent.example/a2a"))
        .with_description("an agent other agents delegate to")
        .with_skill(AgentSkill::new("s", "S", "d").with_tags(["t"]))
        // Spec §13.3: the scheme the bearer interceptor checks.
        .with_security_schemes(std::collections::HashMap::from([(
            "bearer".to_owned(),
            a2a_protocol_types::security::SecurityScheme::Http(
                a2a_protocol_types::security::HttpAuthSecurityScheme {
                    scheme: "Bearer".to_owned(),
                    description: None,
                    bearer_format: None,
                },
            ),
        )]));
    let handler = RequestHandlerBuilder::new(Agent)
        .with_agent_card(card)
        .with_profile(Profile::Production)
        .with_interceptor(BearerTokenAuthInterceptor::with_labelled_tokens([("token", "caller")]))
        .with_halt_store(InMemoryHaltStore::new())
        .with_lease_store(InMemoryLeaseStore::new())
        .with_content_signer(ContentSigner::ed25519("agent-2026-10", signing_key_pkcs8)?)
        .build()?; // refuses, naming what is missing
    // TLS ends at the ingress in front of this pod.
    let config = ServeConfig::production().with_tls_terminated_in_front();
    let report = Server::bind("0.0.0.0:3000")
        .await?
        .with_config(config)
        .serve_with_shutdown(JsonRpcDispatcher::new(Arc::new(handler)), shutdown_signal())
        .await;
    if let Some(why) = report.refused {
        return Err(why.into());
    }
    Ok(())
}
```

Use `PostgresHaltStore` and `PostgresLeaseStore` (feature `postgres`) when
more than one replica serves the agent: both are shared through the database.
`SqliteHaltStore` keeps a halt across a restart of one process, but a SQLite
file is not shared between pods, and there is no SQLite lease store. In 0.15
several replicas are supported only when every turn of a task is routed to one
replica; see [Running More Than One Replica](./horizontal-scaling.md). The
gRPC and WebSocket dispatchers serve on their own listeners, which
`ServeConfig` does not reach.

### HTTPS

Always use HTTPS in production. The **client** (and, through
`a2a-protocol-sdk`, the server's push sender) ship TLS out of the box via the
default `tls-rustls` feature — `https://` agents and webhooks work with no
extra setup. A direct dependency on `a2a-protocol-server` needs
`features = ["tls-rustls"]`.

The **server** terminates TLS itself on `serve::Server` (spec §14:
"Implementations SHOULD support HTTPS"), with the same `tls-rustls` feature,
which `a2a-protocol-sdk` enables by default. Give it a certificate chain and
key in PEM, or a `rustls::ServerConfig` you built (client certificates,
another crypto provider) with `with_tls`:

```rust,no_run
# use std::sync::Arc;
# async fn run(
#     handler: Arc<a2a_protocol_sdk::server::RequestHandler>,
#     shutdown: impl std::future::Future<Output = ()> + Send,
# ) -> std::io::Result<()> {
use a2a_protocol_sdk::server::JsonRpcDispatcher;
use a2a_protocol_sdk::server::serve::Server;

let cert_chain = std::fs::read("/etc/a2a/cert.pem")?;
let key = std::fs::read("/etc/a2a/key.pem")?;
let report = Server::bind("0.0.0.0:443")
    .await?
    .with_tls_pem(&cert_chain, &key)?
    .serve_with_shutdown(JsonRpcDispatcher::new(handler), shutdown)
    .await;
# let _ = report;
# Ok(())
# }
```

The listener then speaks TLS only, offering HTTP/2 and HTTP/1.1 by ALPN. A
handshake must finish within `ServeConfig::header_read_timeout`; one that
fails closes the connection and counts as failed in the connection-pool
metrics. The WebSocket listener terminates TLS itself with
`WebSocketDispatcher::with_tls` or `with_tls_pem` (`tls-rustls`), and the gRPC
listener with `GrpcDispatcher::with_tls` (`grpc-tls`); without those calls they
are plaintext too. So are the quick-start `serve` and `serve_with_addr`: run
those behind a TLS-terminating reverse proxy (nginx, Caddy, a cloud load
balancer), as is also the usual choice when certificates are managed outside
the process:

```text
Client ──HTTPS──→ [nginx/Caddy] ──HTTP──→ [a2a-rust agent]
```

Authentication, outbound-request (SSRF) checks, data at rest, input limits
and the known gaps are on [Security](./security.md), which states what holds
by construction; this chapter is the configuration that turns it on.

## Logs, metrics and traces

The client, the server and the SDK log through `tracing` by default (the
`tracing` feature). Install a subscriber to see the output:

```toml
[dependencies]
a2a-protocol-server = "0.15"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
```

```rust
use tracing_subscriber::EnvFilter;

tracing_subscriber::fmt()
    .with_env_filter(
        EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| EnvFilter::new("info"))
    )
    // For log aggregation, enable tracing-subscriber's `json` feature and
    // add `.json()` here.
    .init();
```

`RUST_LOG=debug` gives verbose output, `RUST_LOG=a2a_protocol_server=debug`
the server's alone. Metrics are recorded by default and traces are spans of
the same subscriber; `Telemetry` exports all three over OTLP from the
standard `OTEL_*` variables. [Observability](./observability.md) has the
setup for a laptop and for Kubernetes, the metric catalogue, and the
dashboards and alerts that ship.

## Next steps

- **[GitHub Pages & CI/CD](./cicd.md)** — Continuous deployment
- **[Configuration Reference](../reference/configuration.md)** — All configuration options
