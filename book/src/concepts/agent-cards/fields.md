<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Card fields, skills and interfaces

This page covers what goes in a card: the required fields, how to name a
protocol binding, skills, the capability flags the server and client enforce,
and the interface list a client picks its transport from.

## Required fields

| Field | Description |
|-------|-------------|
| `name` | Human-readable agent name; not empty |
| `description` | What the agent does; not empty (`AgentCard::new` leaves it empty, so call `with_description`) |
| `version` | Version of this agent; not empty |
| `supportedInterfaces` | At least one interface; each needs a non-empty `url`, `protocolBinding` and `protocolVersion` |
| `defaultInputModes` | At least one MIME type accepted (`AgentCard::new` starts at `["text/plain"]`) |
| `defaultOutputModes` | At least one MIME type produced (starts at `["text/plain"]`) |
| `skills` | At least one skill; each needs a non-empty `id`, `name` and `description` and at least one tag |
| `capabilities` | Capability flags (streaming, push, etc.) |

A `provider`, when present, needs both `organization` and `url`; a card
signature needs `protected` and `signature`. `AgentCard::validate` checks
these, and `AgentCard::validate_security_schemes` checks the REQUIRED strings
of the security schemes (an API key's `name`, an HTTP scheme's `scheme`,
`openIdConnectUrl`, the flow URLs). `RequestHandlerBuilder::build` runs both on
the public and the extended card and fails with `ServerError::InvalidParams`
naming the rule; `StaticAgentCardHandler::new` refuses an invalid card,
`DynamicAgentCardHandler` answers `500` for one, and
`HotReloadAgentCardHandler` refuses to reload one and keeps serving the
previous card. `build()` also refuses a handler with no card unless you call
`without_agent_card()`.

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
> - A message part whose explicit `mediaType` is in neither
>   `defaultInputModes` nor a skill's `inputModes` is refused with
>   `ContentTypeNotSupportedError`. A part with no `mediaType` is not checked,
>   and `RequestHandlerBuilder::allow_undeclared_input_modes()` turns the
>   check off.
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
    protocol_version: "1.0".into(), // Major.Minor only (spec §3.6)
    tenant: None, // Optional: fixed tenant for this interface
};
```

For HTTP-based bindings `url` is an absolute URL (HTTPS in production). For
the gRPC binding it is a gRPC target, `"grpc.example.com:443"` — no scheme,
because gRPC names have none; whether the channel uses TLS is the client's
decision (see [gRPC](../transport-layers/grpc.md#client)).

An agent must have at least one interface. List them in the order you want clients to use them: the order is the agent's preference.

Spec §8.3.2 makes that order binding on clients: a client MUST select the first interface it supports and prefer earlier entries. `ClientBuilder::from_card()` does exactly that, skipping an interface for another protocol major and, at `build()`, one it cannot construct (gRPC under the synchronous `build()`, a binding it does not implement). Until 0.15 it preferred JSON-RPC wherever the card listed it. To rank bindings yourself, which the spec leaves to the client only when you ask for it, pass your own order with `ClientBuilder::from_card_preferring(&card, &["GRPC".into()])`; interfaces you did not rank follow in the card's order.

Because a card gives each binding its own URL, the endpoint follows the binding: selecting `GRPC` selects that interface's `url` and `tenant` as well. The `tenant` field from the selected interface is preserved in `ClientConfig::tenant` and sent on every request — all eleven methods, `GetExtendedAgentCard` included — as spec §8.3.2 rule 4 requires. Methods whose params carry a `tenant` (send, get, list, push-config) let a per-request value override it.
