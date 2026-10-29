<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Setting up push notifications

Turn push on in the server, and register, list and delete webhook configurations from a client.

## Server side

Enable push by providing a `PushSender`:

```rust
# fn card() -> a2a_protocol_types::agent_card::AgentCard {
#     use a2a_protocol_types::agent_card::{AgentCapabilities, AgentCard, AgentInterface, AgentSkill};
#     AgentCard::new("my-agent", "1.0.0", AgentInterface::jsonrpc("http://localhost:3000"))
#         .with_description("An example agent")
#         .with_skill(AgentSkill::new("echo", "Echo", "Echoes text").with_tags(["echo"]))
#         .with_capabilities(AgentCapabilities::none().with_streaming(true).with_push_notifications(true))
# }
# use a2a_protocol_sdk::prelude::*;
# struct MyAgent;
# agent_executor!(MyAgent, |_ctx, _queue| async { Ok(()) });
# fn main() {
# let my_executor = MyAgent;
use a2a_protocol_sdk::server::{RequestHandlerBuilder, HttpPushSender};

let handler = RequestHandlerBuilder::new(my_executor)
    .with_agent_card(card())
    .with_push_sender(HttpPushSender::new())
    .build()
    .unwrap();
# }
```

`HttpPushSender` delivers each event to every webhook registered for the
task; what it checks and how it retries is in
[Delivery and retries](./delivery.md).

> **Capability + task-existence rules (spec §3.1.7, §3.3.4).** If you configure
> an agent card, it must advertise `capabilities.pushNotifications = true` or the
> push-config operations return `PushNotificationNotSupportedError`; they
> return it too when no push sender is wired, and `build()` refuses a card that
> declares `pushNotifications` without `with_push_sender`. Creating a
> config also requires the **target task to already exist** — a
> `CreateTaskPushNotificationConfig` for an unknown task returns
> `TaskNotFoundError` rather than storing an unroutable config. A
> `GetTaskPushNotificationConfig` for a config that does not exist likewise
> returns `TaskNotFoundError` (HTTP 404 over REST), not an invalid-params error.

## Client side

Register a push notification configuration:

```rust,no_run
# use a2a_protocol_sdk::prelude::*;
# async fn f(client: A2aClient) -> Result<(), ClientError> {
use a2a_protocol_sdk::types::push::TaskPushNotificationConfig;

let config = TaskPushNotificationConfig::new(
    "task-abc",                          // Task to watch
    "https://my-service.com/webhook",    // Webhook URL
);

let saved = client.set_push_config(config).await?;
println!("Config ID: {:?}", saved.id);
# Ok(())
# }
```

Spec §13.2 says webhook URLs SHOULD use HTTPS: the payload, the token and any
credentials travel with the request. `set_push_config` (and a send carrying a
push config inline) logs a warning for an `http://` URL to a host other than
loopback; build the client with `ClientBuilder::require_https_webhooks()` to
refuse such a config with `ClientError::InsecureWebhook` before anything is
sent. `http://localhost`, `127.0.0.0/8` and `[::1]` are allowed either way.

## Managing push configs

```rust,no_run
# use a2a_protocol_sdk::prelude::*;
# use a2a_protocol_sdk::types::params::ListPushConfigsParams;
# async fn f(client: A2aClient) -> Result<(), ClientError> {
// List all configs for a task
let configs = client.list_push_configs(ListPushConfigsParams {
    tenant: None,
    task_id: "task-abc".into(),
    page_size: None,
    page_token: None,
}).await?;

// Get a specific config
let config = client.get_push_config("task-abc", "config-123").await?;

// Delete a config
client.delete_push_config("task-abc", "config-123").await?;
# Ok(())
# }
```
