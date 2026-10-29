<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# REST dispatcher

`RestDispatcher` serves the HTTP+JSON binding: one route per method, tenants in
the path, and protections against path traversal and oversized requests built
in. This page has its route table and what it refuses.

Routes RESTful HTTP requests to the handler:

```rust,no_run
# fn card() -> a2a_protocol_types::agent_card::AgentCard {
#     use a2a_protocol_types::agent_card::{AgentCapabilities, AgentCard, AgentInterface, AgentSkill};
#     AgentCard::new("my-agent", "1.0.0", AgentInterface::jsonrpc("http://localhost:3000"))
#         .with_description("An example agent")
#         .with_skill(AgentSkill::new("echo", "Echo", "Echoes text").with_tags(["echo"]))
#         .with_capabilities(AgentCapabilities::none().with_streaming(true))
# }
# use a2a_protocol_sdk::prelude::*;
# struct MyAgent;
# agent_executor!(MyAgent, |_ctx, _queue| async { Ok(()) });
# #[tokio::main]
# async fn main() -> Result<(), Box<dyn std::error::Error>> {
# let handler = std::sync::Arc::new(RequestHandlerBuilder::new(MyAgent).with_agent_card(card()).build()?);
use a2a_protocol_sdk::server::RestDispatcher;
use std::sync::Arc;

let dispatcher = Arc::new(RestDispatcher::new(handler));
# Ok(())
# }
```

## Route table

| Method | Path | Handler |
|--------|------|---------|
| `POST` | `/message:send` | SendMessage |
| `POST` | `/message:stream` | SendStreamingMessage |
| `GET` | `/tasks` | ListTasks |
| `GET` | `/tasks/{id}` | GetTask |
| `POST` | `/tasks/{id}:cancel` | CancelTask |
| `GET\|POST` | `/tasks/{id}:subscribe` | SubscribeToTask |
| `GET` | `/extendedAgentCard` | GetExtendedAgentCard |
| `POST` | `/tasks/{id}/pushNotificationConfigs` | CreateTaskPushNotificationConfig |
| `GET` | `/tasks/{id}/pushNotificationConfigs` | ListTaskPushNotificationConfigs |
| `GET` | `/tasks/{id}/pushNotificationConfigs/{cfgId}` | GetTaskPushNotificationConfig |
| `DELETE` | `/tasks/{id}/pushNotificationConfigs/{cfgId}` | DeleteTaskPushNotificationConfig |
| `GET` | `/.well-known/agent-card.json` | AgentCard |
| `GET\|HEAD` | `/health`, `/ready` | Health probes, answered `200` |

## Multi-tenancy

Tenant-scoped routes accept two forms — the canonical bare-segment form from
the spec proto's `google.api.http` additional bindings (what official-SDK
REST clients send), and this SDK's original explicit prefix:

```text
# Canonical form
GET  /acme-corp/tasks
POST /acme-corp/message:send

# Explicit form
GET  /tenants/acme-corp/tasks
POST /tenants/acme-corp/message:send
```

In the canonical form, literal route segments always win over the tenant
variable (a tenant named like a route head must use the explicit form).

## Built-in security

The REST dispatcher includes automatic protections:

| Protection | Behavior |
|-----------|----------|
| **Path traversal** | a `.` or `..` path segment, raw or percent-encoded at any depth, or separated with `\` → 400; an id that merely contains `..` is served |
| **Query string size** | Over 4 KiB → 414 |
| **Body size** | Over 4 MiB → 413 |
| **Content type** | Accepts `application/json` and `application/a2a+json` (any other `Content-Type` on a `POST` is `ContentTypeNotSupportedError`, 400). Answers `application/a2a+json` unless the request's `Accept` prefers `application/json` or asks only for `text/event-stream`; the agent card and `/health`, `/ready` stay `application/json` ([negotiation](../../concepts/transport-layers/rest.md#content-types)) |


To serve it, and to change the limits above (`DispatchConfig`), see
[Serving and combining](./serving.md).
