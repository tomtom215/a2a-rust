# Agent Cards & Discovery

An **Agent Card** is the machine-readable discovery document that describes an A2A agent. Clients fetch the card to learn what the agent can do, how to connect, and what security is required.

## The Agent Card

Agent cards are served at `/.well-known/agent-card.json` and contain:

```json
{
  "name": "Calculator Agent",
  "description": "Evaluates arithmetic expressions",
  "version": "1.0.0",
  "supportedInterfaces": [
    {
      "url": "https://agent.example.com/rpc",
      "protocolBinding": "JSONRPC",
      "protocolVersion": "1.0.0"
    },
    {
      "url": "https://agent.example.com/api",
      "protocolBinding": "HTTP+JSON",
      "protocolVersion": "1.0.0"
    }
  ],
  "defaultInputModes": ["text/plain"],
  "defaultOutputModes": ["text/plain"],
  "skills": [
    {
      "id": "calc",
      "name": "Calculator",
      "description": "Evaluates expressions like '3 + 5'",
      "tags": ["math", "calculator"],
      "examples": ["3 + 5", "10 * 2"]
    }
  ],
  "capabilities": {
    "streaming": true,
    "pushNotifications": false
  }
}
```

## Naming a protocol binding

`protocolBinding` names the transport a client should dial. The spec's
canonical values are `"JSONRPC"`, `"GRPC"` and `"HTTP+JSON"` (the `AgentInterface.protocolBinding` field, §4.4.6; see the §8.5 sample card), and any
binding outside those three is a *custom binding* (§12).

Custom bindings **should** be identified by a URI rather than a bare word
(§5.8). A bare name is unique only by convention: two projects can define
incompatible bindings called `"WEBSOCKET"` and nothing in the card tells a
client which one it is looking at. A URI under a domain the binding's author
controls can only mean one thing. It is an identifier, not a document — it
does not have to resolve.

The two custom bindings in this project both follow that rule:

| Binding | `protocolBinding` | Constant |
|---|---|---|
| WebSocket | `https://a2a-rust.com/bindings/websocket/v1` | `a2a_protocol_types::WEBSOCKET_BINDING_URI` |
| SLIMRPC | `https://a2a-protocol.org/bindings/experimental-slimrpc/v1` | `a2a_protocol_slimrpc::SLIMRPC_PROTOCOL_BINDING` |

The trailing `/v1` carries the binding's version, because §5.8 asks for a
*new URI* on a breaking change rather than a redefinition of the old one.

Before 0.11.0 this project advertised the bare `"WEBSOCKET"`. A client reading
cards in the wild should accept both spellings for as long as the old one is
still out there.

## Required Fields

| Field | Description |
|-------|-------------|
| `name` | Human-readable agent name |
| `description` | What the agent does |
| `version` | Semantic version of this agent |
| `supportedInterfaces` | At least one interface with URL and protocol binding |
| `defaultInputModes` | MIME types accepted (e.g., `["text/plain"]`) |
| `defaultOutputModes` | MIME types produced |
| `skills` | At least one skill describing a capability |
| `capabilities` | Capability flags (streaming, push, etc.) |

## Skills

Skills describe discrete capabilities:

```rust,no_run
use a2a_protocol_sdk::types::agent_card::AgentSkill;

let skill = AgentSkill {
    id: "summarize".into(),
    name: "Summarizer".into(),
    description: "Summarizes long documents".into(),
    tags: vec!["nlp".into(), "summarization".into()],
    examples: Some(vec![
        "Summarize this research paper".into(),
        "Give me a 3-sentence summary".into(),
    ]),
    input_modes: Some(vec!["text/plain".into(), "application/pdf".into()]),
    output_modes: Some(vec!["text/plain".into()]),
    security_requirements: None,
};
```

Skills can override the agent's default input/output modes and declare their own security requirements.

## Capabilities

The `AgentCapabilities` struct advertises what the agent supports:

```rust,no_run
use a2a_protocol_sdk::prelude::AgentCapabilities;

let caps = AgentCapabilities::none()
    .with_streaming(true)           // Supports SendStreamingMessage
    .with_push_notifications(true)  // Supports push notification configs
    .with_extended_agent_card(true); // Has an authenticated extended card
```

> **Note:** `AgentCapabilities` is `#[non_exhaustive]` — always construct it via `AgentCapabilities::none()` and the builder methods, never with a struct literal.

> **The server enforces these flags (spec §3.3.4).** When you configure an
> agent card on the handler (`RequestHandlerBuilder::with_agent_card`), the
> declared capabilities become a contract the server honors:
> - If `streaming` is not `true`, `SendStreamingMessage` and `SubscribeToTask`
>   return `UnsupportedOperationError`.
> - If `pushNotifications` is not `true`, the push-config operations
>   (Create/Get/List/Delete) return `PushNotificationNotSupportedError`.
>   They also need a push sender (`RequestHandlerBuilder::with_push_sender`):
>   without one they return that error whatever the card says, and `build()`
>   refuses a card that declares `pushNotifications` with no sender.
> - If `extendedAgentCard` is not `true`, `GetExtendedAgentCard` returns
>   `UnsupportedOperationError`.
>
> So if your agent serves streaming or push, **set the matching flag to `true`**
> — otherwise clients are told the operation is unsupported. A handler with no
> agent card configured publishes no contract and is not gated by one; push
> still needs a sender.
>
> **The client reads the same flags (spec §3.3.4).** A client built with
> `ClientBuilder::from_card` refuses those calls itself, before sending, when
> the card does not set the flag to `true`, with
> `ClientError::CapabilityNotDeclared`; a send carrying an inline push config
> counts as a push call. `ClientBuilder::without_capability_checks` sends them
> anyway, for an agent that serves a capability without declaring it.

## Interfaces

Each interface describes a transport endpoint:

```rust,no_run
use a2a_protocol_sdk::types::agent_card::AgentInterface;

let interface = AgentInterface {
    url: "https://agent.example.com/rpc".into(),
    protocol_binding: "JSONRPC".into(),  // or "HTTP+JSON", "GRPC"
    protocol_version: "1.0.0".into(),
    tenant: None, // Optional: fixed tenant for this interface
};
```

For HTTP-based bindings `url` is an absolute URL (HTTPS in production). For
the gRPC binding it is a gRPC target, `"grpc.example.com:443"` — no scheme,
because gRPC names have none; whether the channel uses TLS is the client's
decision (see [gRPC](transport-layers.md#grpc)).

An agent must have at least one interface. List them in the order you want clients to use them: the order is the agent's preference.

Spec §8.3.2 makes that order binding on clients: a client MUST select the first interface it supports and prefer earlier entries. `ClientBuilder::from_card()` does exactly that, skipping an interface for another protocol major and, at `build()`, one it cannot construct (gRPC under the synchronous `build()`, a binding it does not implement). Until 0.15 it preferred JSON-RPC wherever the card listed it. To rank bindings yourself, which the spec leaves to the client only when you ask for it, pass your own order with `ClientBuilder::from_card_preferring(&card, &["GRPC".into()])`; interfaces you did not rank follow in the card's order.

Because a card gives each binding its own URL, the endpoint follows the binding: selecting `GRPC` selects that interface's `url` and `tenant` as well. The `tenant` field from the selected interface is preserved in `ClientConfig::tenant` and sent on every request — all eleven methods, `GetExtendedAgentCard` included — as spec §8.3.2 rule 4 requires. Methods whose params carry a `tenant` (send, get, list, push-config) let a per-request value override it.

## Extended Agent Card

An agent can expose a richer card via `GetExtendedAgentCard` for authenticated
clients. This requires setting `capabilities.extended_agent_card = true`:

```rust
# use a2a_protocol_sdk::prelude::*;
let capabilities = AgentCapabilities::none()
    .with_extended_agent_card(true);
```

Give the handler the card to serve with `with_extended_agent_card`: the public
card plus what only clients you know should see, such as a further skill.

```rust
# use a2a_protocol_sdk::prelude::*;
# struct MyAgent;
# agent_executor!(MyAgent, |_ctx, _queue| async { Ok(()) });
# fn main() {
use a2a_protocol_sdk::server::{BearerTokenAuthInterceptor, RequestHandlerBuilder};

let public = AgentCard::new("my-agent", "1.0.0", AgentInterface::jsonrpc("http://localhost:3000"))
    .with_description("An example agent")
    .with_skill(AgentSkill::new("echo", "Echo", "Echoes text").with_tags(["echo"]))
    .with_capabilities(AgentCapabilities::none().with_extended_agent_card(true))
    // Spec §13.3: requests are authenticated with a scheme the public card
    // declares, here the one the bearer interceptor checks.
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
let extended = public
    .clone()
    .with_skill(AgentSkill::new("billing", "Billing", "Account details").with_tags(["billing"]));

let handler = RequestHandlerBuilder::new(MyAgent)
    .with_agent_card(public)
    .with_extended_agent_card(extended)
    .with_interceptor(BearerTokenAuthInterceptor::new(["s3cret"]))
    .build()
    .unwrap();
# let _ = handler;
# }
```

`build()` checks the extended card as it checks the public one, and advertises
the same extensions on it, since a client that fetches it uses its capabilities
from then on.

The answers follow §3.1.11 and §13.3:

| Public card | `with_extended_agent_card` | `GetExtendedAgentCard` answers |
|---|---|---|
| does not declare `extendedAgentCard` | — | `UnsupportedOperationError` (and `build()` refuses an extended card it would never serve) |
| declares it | set | the extended card |
| declares it | not set | `ExtendedAgentCardNotConfiguredError` |
| none (`without_agent_card()`) | set | the extended card |
| none (`without_agent_card()`) | not set | `UnsupportedOperationError` |

Until 0.15 a declared card with no extended card served the public card in its
place. To keep serving that content, pass the public card to
`with_extended_agent_card` as well.

The operation must be authenticated (§13.3): without an authenticating
interceptor (e.g. `BearerTokenAuthInterceptor`, `JwtAuthInterceptor`) the server
refuses it with `InvalidRequest`, unless you opt in with
`RequestHandlerBuilder::allow_unauthenticated_extended_card()`.

### A card per caller

§13.3 lets an agent "return different extended card content based on the
authenticated client's identity or authorization level". An
`ExtendedCardProvider` chooses the card for each request. It runs after the
interceptor chain has authenticated the caller, so
`CallContext::caller_identity()` names them (`auth::caller_id` builds the
identity to compare against). It is given the card set with
`with_extended_agent_card`, which `build()` then requires, and may return it
changed, or refuse the caller with an error.

```rust
# use a2a_protocol_sdk::prelude::*;
# struct MyAgent;
# agent_executor!(MyAgent, |_ctx, _queue| async { Ok(()) });
# fn main() {
use std::future::Future;
use std::pin::Pin;

use a2a_protocol_sdk::server::agent_card::ExtendedCardProvider;
use a2a_protocol_sdk::server::auth::caller_id;
use a2a_protocol_sdk::server::{
    BearerTokenAuthInterceptor, CallContext, RequestHandlerBuilder, ServerResult,
};

/// Only the caller labelled `finance` sees the billing skill.
struct BillingForFinance;

impl ExtendedCardProvider for BillingForFinance {
    fn card_for<'a>(
        &'a self,
        ctx: &'a CallContext,
        configured: &'a AgentCard,
    ) -> Pin<Box<dyn Future<Output = ServerResult<AgentCard>> + Send + 'a>> {
        let mut card = configured.clone();
        if ctx.caller_identity() != Some(caller_id::bearer("finance").as_str()) {
            card.skills.retain(|skill| skill.id != "billing");
        }
        Box::pin(async move { Ok(card) })
    }
}

let public = AgentCard::new("my-agent", "1.0.0", AgentInterface::jsonrpc("http://localhost:3000"))
    .with_description("An example agent")
    .with_skill(AgentSkill::new("echo", "Echo", "Echoes text").with_tags(["echo"]))
    .with_capabilities(AgentCapabilities::none().with_extended_agent_card(true))
    // Spec §13.3: requests are authenticated with a scheme the public card
    // declares, here the one the bearer interceptor checks.
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
let extended = public
    .clone()
    .with_skill(AgentSkill::new("billing", "Billing", "Account details").with_tags(["billing"]));

let handler = RequestHandlerBuilder::new(MyAgent)
    .with_agent_card(public)
    .with_extended_agent_card(extended)
    .with_extended_card_provider(BillingForFinance)
    .with_interceptor(BearerTokenAuthInterceptor::with_labelled_tokens([
        ("f-token", "finance"),
        ("s-token", "support"),
    ]))
    .build()
    .unwrap();
# let _ = handler;
# }
```

The card a provider returns is checked as the configured one is: one that is
not valid, or that declares push notifications with no push sender wired, is
not served, and the request gets an internal error saying why. The extensions
this server advertises are added to it if the provider left them out. Over
HTTP+JSON each caller's card has its own `ETag`, and the response is
`Cache-Control: private, no-cache`, so a client that revalidates gets its
own card.

## Serving Agent Cards

### Static Handler

For agent cards that don't change at runtime:

```rust
# use a2a_protocol_sdk::prelude::*;
# struct MyAgent;
# agent_executor!(MyAgent, |_ctx, _queue| async { Ok(()) });
# fn make_agent_card() -> AgentCard { AgentCard::new("my-agent", "1.0.0", AgentInterface::jsonrpc("http://localhost:3000")).with_description("An example agent").with_skill(AgentSkill::new("echo", "Echo", "Echoes text").with_tags(["echo"])) }
# fn main() {
# let my_executor = MyAgent;
use a2a_protocol_sdk::server::RequestHandlerBuilder;

let handler = RequestHandlerBuilder::new(my_executor)
    .with_agent_card(make_agent_card())
    .build()
    .unwrap();
# }
```

The static handler automatically provides:
- **ETag** headers for cache validation
- **Last-Modified** timestamps
- **Cache-Control** directives
- **304 Not Modified** responses for conditional requests

### Dynamic Handler

For agent cards that change (e.g., based on feature flags, load, or authentication):

```rust
# use std::future::Future;
# use std::pin::Pin;
# use a2a_protocol_sdk::prelude::{A2aResult, AgentInterface, AgentSkill};
# fn make_agent_card() -> AgentCard { AgentCard::new("my-agent", "1.0.0", AgentInterface::jsonrpc("http://localhost:3000")).with_description("An example agent").with_skill(AgentSkill::new("echo", "Echo", "Echoes text").with_tags(["echo"])) }
use a2a_protocol_sdk::server::{AgentCardProducer, DynamicAgentCardHandler};
use a2a_protocol_sdk::types::agent_card::AgentCard;

struct MyCardProducer;

impl AgentCardProducer for MyCardProducer {
    fn produce<'a>(&'a self) -> Pin<Box<dyn Future<Output = A2aResult<AgentCard>> + Send + 'a>> {
        Box::pin(async move {
            // Generate card dynamically
            Ok(make_agent_card())
        })
    }
}
```

The dynamic handler calls the producer on every request, computes a fresh ETag, and handles conditional caching.

### Hot-Reload Handler

For agent cards loaded from a JSON file that may change at runtime:

```rust,no_run
# use a2a_protocol_sdk::prelude::*;
# fn make_agent_card() -> AgentCard { AgentCard::new("my-agent", "1.0.0", AgentInterface::jsonrpc("http://localhost:3000")).with_description("An example agent").with_skill(AgentSkill::new("echo", "Echo", "Echoes text").with_tags(["echo"])) }
# #[tokio::main]
# async fn main() {
# let initial_card = make_agent_card();
use a2a_protocol_sdk::server::HotReloadAgentCardHandler;
use std::path::Path;
use std::time::Duration;

let handler = HotReloadAgentCardHandler::new(initial_card);

// Cross-platform: poll the file every 30 seconds
let watcher = handler.spawn_poll_watcher(
    Path::new("/etc/a2a/agent.json"),
    Duration::from_secs(30),
);

// Unix only: reload on SIGHUP
#[cfg(unix)]
let signal_watcher = handler.spawn_signal_watcher(
    Path::new("/etc/a2a/agent.json"),
);
# }
```

`HotReloadAgentCardHandler` implements `AgentCardProducer`, so it plugs directly into `DynamicAgentCardHandler` for full HTTP caching support. The internal `Arc<RwLock<AgentCard>>` ensures updates are atomic with low contention for concurrent readers.

## HTTP Caching

Agent card responses include standard HTTP caching headers (spec §8.6, RFC 9110):

| Header | Purpose |
|--------|---------|
| `ETag` | Content hash for cache validation |
| `Last-Modified` | Static handler only: when it loaded the card |
| `Cache-Control` | `public, max-age=3600` (configurable: `DispatchConfig::with_agent_card_max_age`, and `with_agent_card_max_age` on `WebSocketDispatcher` and `GrpcDispatcher`) |

Clients revalidate with `If-None-Match`, or with `If-Modified-Since` against the static handler. If the card hasn't changed, the server returns `304 Not Modified` with no body. The dynamic handler sends no `Last-Modified` and ignores `If-Modified-Since`: a card produced per request has no modification date, so the `ETag` is its only validator.

On the client, `discovery::CachingCardResolver` is a private HTTP cache of one card (spec §8.6.2, RFC 9111). `resolve()` returns the stored card without a request while it is fresh, for its `max-age`, or else its `Expires`, less the age it arrived with (`Age`, `Date`), and revalidates it with `If-None-Match` / `If-Modified-Since` once it is stale. A card marked `no-cache` is revalidated on every resolve, and a `no-store` card is not stored.

```rust,no_run
use std::time::Duration;
use a2a_protocol_client::discovery::CachingCardResolver;

# async fn example() -> Result<(), a2a_protocol_client::error::ClientError> {
let resolver = CachingCardResolver::new("https://agent.example.com")?
    // A card sent with no `max-age` and no `Expires` stays fresh this long
    // (off by default: such a card is revalidated on every resolve).
    .with_default_freshness(Duration::from_secs(300));
let card = resolver.resolve().await?; // fetched
let card = resolver.resolve().await?; // from the cache while fresh
# let _ = card;
# Ok(())
# }
```

`with_always_revalidate(true)` sends a request on every resolve, as the resolver did before 0.15, for an agent whose card changes sooner than its `max-age` says; a hot-reloaded card served with the default `max-age=3600` is the usual case. `resolve_agent_card` and `fetch_card_from_url` cache nothing: each call fetches.

## Security

Agent cards declare security schemes and the requirements that reference
them. This is the v1.0 wire form, which is what this SDK emits:

```json
{
  "securitySchemes": {
    "bearer": { "httpAuthSecurityScheme": { "scheme": "bearer" } },
    "oauth": { "oauth2SecurityScheme": { "flows": { "clientCredentials": {
      "tokenUrl": "https://auth.example.com/token", "scopes": { "read": "Read" } } } } }
  },
  "securityRequirements": [
    { "schemes": { "bearer": { "list": [] } } },
    { "schemes": { "oauth": { "list": ["read"] } } }
  ]
}
```

Each scope list is a `StringList` object (`{"list": [...]}`), because the spec
defines the JSON as the ProtoJSON of `map<string, StringList>`. When reading a
card, a bare array (`"oauth": ["read"]`, as a2a-go v2.5.0 writes it) and `null`
are accepted too, so a Go agent's card parses; only the `{"list": [...]}` form
is ever written.

Individual skills can also declare their own security requirements, overriding the global ones.

## Extended Agent Cards

If the agent supports `extendedAgentCard` capability, clients can fetch an authenticated version with additional details via the `GetExtendedAgentCard` method.

## Next Steps

- **[Tasks & Messages](./tasks-and-messages.md)** — The data model for agent communication
- **[Streaming with SSE](./streaming.md)** — Real-time event delivery
