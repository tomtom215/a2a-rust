# Agent Cards & Discovery

An **Agent Card** is the machine-readable discovery document that describes an A2A agent. Clients fetch the card to learn what the agent can do, how to connect, and what security is required.

## The Agent Card

Agent cards are served at `/.well-known/agent.json` and contain:

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
      "protocolBinding": "REST",
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

```rust
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

```rust
use a2a_protocol_sdk::prelude::AgentCapabilities;

let caps = AgentCapabilities::none()
    .with_streaming(true)           // Supports SendStreamingMessage
    .with_push_notifications(true)  // Supports push notification configs
    .with_extended_agent_card(true); // Has an authenticated extended card
```

> **Note:** `AgentCapabilities` is `#[non_exhaustive]` — always construct it via `AgentCapabilities::none()` and the builder methods, never with a struct literal.

## Interfaces

Each interface describes a transport endpoint:

```rust
use a2a_protocol_sdk::types::agent_card::AgentInterface;

let interface = AgentInterface {
    url: "https://agent.example.com/rpc".into(),
    protocol_binding: "JSONRPC".into(),  // or "REST", "GRPC"
    protocol_version: "1.0.0".into(),
    tenant: None, // Optional: fixed tenant for this interface
};
```

An agent must have at least one interface. Having multiple interfaces (e.g., JSON-RPC and REST) lets clients choose their preferred transport.

## Serving Agent Cards

### Static Handler

For agent cards that don't change at runtime:

```rust
use a2a_protocol_sdk::server::RequestHandlerBuilder;

let handler = RequestHandlerBuilder::new(my_executor)
    .with_agent_card(make_agent_card())
    .build()
    .unwrap();
```

The static handler automatically provides:
- **ETag** headers for cache validation
- **Last-Modified** timestamps
- **Cache-Control** directives
- **304 Not Modified** responses for conditional requests

### Dynamic Handler

For agent cards that change (e.g., based on feature flags, load, or authentication):

```rust
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

```rust
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
```

`HotReloadAgentCardHandler` implements `AgentCardProducer`, so it plugs directly into `DynamicAgentCardHandler` for full HTTP caching support. The internal `Arc<RwLock<AgentCard>>` ensures updates are atomic with low contention for concurrent readers.

## HTTP Caching

Agent card responses include standard HTTP caching headers (RFC 7232):

| Header | Purpose |
|--------|---------|
| `ETag` | Content hash for cache validation |
| `Last-Modified` | Timestamp of last change |
| `Cache-Control` | `public, max-age=60` (configurable) |

Clients should send `If-None-Match` or `If-Modified-Since` headers. If the card hasn't changed, the server returns `304 Not Modified` with no body.

## Security

Agent cards can declare security requirements using OpenAPI-style schemes:

```json
{
  "securitySchemes": {
    "bearer": {
      "type": "http",
      "scheme": "bearer"
    }
  },
  "securityRequirements": [
    { "bearer": [] }
  ]
}
```

Individual skills can also declare their own security requirements, overriding the global ones.

## Extended Agent Cards

If the agent supports `extendedAgentCard` capability, clients can fetch an authenticated version with additional details via the `GetExtendedAgentCard` method.

## Next Steps

- **[Tasks & Messages](./tasks-and-messages.md)** — The data model for agent communication
- **[Streaming with SSE](./streaming.md)** — Real-time event delivery
