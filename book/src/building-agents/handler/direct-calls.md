<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Calling the handler directly

The dispatchers are one way to feed a `RequestHandler`, not the only way. This
page shows how to call it from your own HTTP framework, queue consumer or
test, what you give up by skipping the dispatchers, and how one handler is
shared between several of them.

`RequestHandler` is the protocol layer, and it is transport-agnostic: no method
takes an HTTP type. Requests arrive as parsed params plus a plain
`HashMap<String, String>` of headers, which is all the interceptor chain needs
to make access-control decisions. The [dispatchers](../dispatchers.md) are one
way to feed it, not the only way.

That matters if you already own your HTTP surface. An agent framework, an
existing Axum or Actix application, a tower service, a queue consumer, or a
test harness can call these methods directly and keep its own routing,
middleware, and server lifecycle:

| Method | A2A operation |
|--------|---------------|
| `on_send_message(params, streaming, headers)` | `SendMessage`, `SendStreamingMessage` |
| `on_get_task(params, headers)` | `GetTask` |
| `on_list_tasks(params, headers)` | `ListTasks` |
| `on_cancel_task(params, headers)` | `CancelTask` |
| `on_resubscribe(params, headers)` | `SubscribeToTask` |
| `on_get_extended_agent_card(headers)` | `GetExtendedAgentCard` |
| `on_set_push_config(config, headers)` | `CreateTaskPushNotificationConfig` |
| `on_get_push_config(params, headers)` | `GetTaskPushNotificationConfig` |
| `on_list_push_configs_page(&params, headers)` | `ListTaskPushNotificationConfigs`, with `pageSize` and `pageToken`; what every dispatcher calls |
| `on_list_push_configs(task_id, tenant, headers)` | `ListTaskPushNotificationConfigs`, unpaged: every config of the task |
| `on_delete_push_config(params, headers)` | `DeleteTaskPushNotificationConfig` |

Those eleven methods are the whole protocol surface. Everything the
dispatchers do on top is decoding the wire format and encoding the reply.

```rust
# fn card() -> a2a_protocol_types::agent_card::AgentCard {
#     use a2a_protocol_types::agent_card::{AgentCapabilities, AgentCard, AgentInterface, AgentSkill};
#     AgentCard::new("my-agent", "1.0.0", AgentInterface::jsonrpc("http://localhost:3000"))
#         .with_description("An example agent")
#         .with_skill(AgentSkill::new("echo", "Echo", "Echoes text").with_tags(["echo"]))
#         .with_capabilities(AgentCapabilities::none().with_streaming(true))
# }
use std::collections::HashMap;

use a2a_protocol_server::agent_executor;
use a2a_protocol_server::builder::RequestHandlerBuilder;
use a2a_protocol_server::handler::SendMessageResult;
use a2a_protocol_types::params::MessageSendParams;
use a2a_protocol_types::{Message, MessageId, MessageRole, Part};

// Your agent. Nothing in it knows how the request arrived.
struct EchoAgent;
agent_executor!(EchoAgent, |_ctx, _queue| async { Ok(()) });

let handler = RequestHandlerBuilder::new(EchoAgent)
    .with_agent_card(card())
    .build()
    .expect("build handler");

// Whatever your framework hands you becomes params plus a header map.
let params = MessageSendParams::new(Message::user_text("msg-1", "hello"));

let mut headers = HashMap::new();
headers.insert("authorization".into(), "Bearer token".into());

let runtime = tokio::runtime::Runtime::new().expect("runtime");
let result = runtime
    .block_on(handler.on_send_message(params, false, Some(&headers)))
    .expect("send should succeed");

// `false` for `streaming` yields a synchronous response; `true` yields a
// reader you drain to produce SSE, a WebSocket feed, or whatever your
// transport emits.
assert!(matches!(result, SendMessageResult::Response(_)));
```

Pass `None` for `headers` for an in-process call on a handler with no
authenticating interceptor. Interceptors still run; they simply see an empty
header set, so an authenticating one refuses the call for want of a
credential. A transport that has already authenticated the caller passes the
credential headers on, or the handler is built without the authenticating
interceptor (with `authenticated_upstream()` if its card demands a
credential). Pass header names in lowercase, as the handler looks them up.

What you give up by skipping the dispatchers is exactly what they implement:
JSON-RPC envelope parsing and error mapping, REST path and query binding, SSE
framing, the agent-card endpoint, `A2A-Version` negotiation, the body-size
and read-time limits of `DispatchConfig` and the `Content-Type` check, CORS,
the `A2A-Extensions` response header, and the connection facts they add to
the header map: the `:path` entry a `PathSegmentTenantResolver` reads, and
the peer address a `RateLimitInterceptor` keys unidentified callers by. What
you keep is every protocol guarantee — task lifecycle, idempotency, streaming, push delivery,
interceptors, multi-tenancy, and limits — because all of it lives here, below
the transport.

## Sharing the handler

The handler is wrapped in `Arc` for sharing between dispatchers:

```rust
# fn card() -> a2a_protocol_types::agent_card::AgentCard {
#     use a2a_protocol_types::agent_card::{AgentCapabilities, AgentCard, AgentInterface, AgentSkill};
#     AgentCard::new("my-agent", "1.0.0", AgentInterface::jsonrpc("http://localhost:3000"))
#         .with_description("An example agent")
#         .with_skill(AgentSkill::new("echo", "Echo", "Echoes text").with_tags(["echo"]))
#         .with_capabilities(AgentCapabilities::none().with_streaming(true))
# }
# use a2a_protocol_sdk::prelude::*;
# struct MyExecutor;
# agent_executor!(MyExecutor, |_ctx, _queue| async { Ok(()) });
use std::sync::Arc;

let handler = Arc::new(
    RequestHandlerBuilder::new(MyExecutor)
        .with_agent_card(card())
        .build()
        .unwrap()
);

// Both dispatchers share the same handler
let jsonrpc = JsonRpcDispatcher::new(Arc::clone(&handler));
let rest = RestDispatcher::new(handler);
```

This means JSON-RPC and REST clients share the same task store, push configs, and executor.
