# Push Notifications

Push notifications let agents deliver results asynchronously via webhooks. Instead of the client holding an SSE connection open, the server POSTs events to a URL the client provides.

## How Push Notifications Work

```
  Client              Agent Server          Client Webhook
     │                      │                      │
     │  CreatePushConfig    │                      │
     │ ────────────────────►│                      │
     │  Config with ID      │                      │
     │ ◄────────────────────│                      │
     │                      │                      │
     │  SendMessage         │                      │
     │ ────────────────────►│                      │
     │  Task (submitted)    │                      │
     │ ◄────────────────────│                      │
     │                      │                      │
     │                      │  Executor runs       │
     │                      │                      │
     │                      │  POST event          │
     │                      │ ────────────────────►│
     │                      │  POST event          │
     │                      │ ────────────────────►│
     │                      │                      │
```

1. Client registers a webhook URL via `CreateTaskPushNotificationConfig`
2. Client sends a message (with `return_immediately: true` for async)
3. Agent processes the message and pushes events to the webhook

## Setting Up Push Notifications

### Server Side

Enable push by providing a `PushSender`:

```rust
use a2a_protocol_sdk::server::{RequestHandlerBuilder, HttpPushSender};

let handler = RequestHandlerBuilder::new(my_executor)
    .with_push_sender(HttpPushSender::new())
    .build()
    .unwrap();
```

The built-in `HttpPushSender` includes:

- **SSRF protection** — Resolves URLs and rejects private/loopback IP addresses
- **Header injection prevention** — Validates credentials contain no `\r` or `\n`
- **HTTPS validation** — Optionally enforces HTTPS-only webhook URLs

### Client Side

Register a push notification configuration:

```rust
use a2a_protocol_sdk::types::push::TaskPushNotificationConfig;

let config = TaskPushNotificationConfig::new(
    "task-abc",                          // Task to watch
    "https://my-service.com/webhook",    // Webhook URL
);

let saved = client.set_push_config(config).await?;
println!("Config ID: {:?}", saved.id);
```

### Managing Push Configs

```rust
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
```

## Authentication

Push configs support authentication for the webhook endpoint:

```rust
use a2a_protocol_sdk::types::push::{TaskPushNotificationConfig, AuthenticationInfo};

let mut config = TaskPushNotificationConfig::new("task-abc", "https://webhook.example.com");
config.authentication = Some(AuthenticationInfo {
    scheme: "bearer".into(),
    credentials: "my-secret-token".into(),
});
```

The server includes these credentials in the `Authorization` header when POSTing to the webhook.

## Custom PushSender

Implement the `PushSender` trait for custom delivery:

```rust
use a2a_protocol_sdk::server::PushSender;

struct SqsPushSender {
    client: aws_sdk_sqs::Client,
}

impl PushSender for SqsPushSender {
    fn send<'a>(
        &'a self,
        url: &'a str,
        event: &'a StreamResponse,
        config: &'a TaskPushNotificationConfig,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async move {
            // Send event to SQS instead of HTTP webhook
            Ok(())
        })
    }
}
```

## Push Config Storage

The default `InMemoryPushConfigStore` stores configs in memory with per-task limits. For production, implement `PushConfigStore`:

```rust
use a2a_protocol_sdk::server::PushConfigStore;

struct PostgresPushConfigStore { /* ... */ }

impl PushConfigStore for PostgresPushConfigStore {
    // Implement set, get, list, delete...
}

RequestHandlerBuilder::new(executor)
    .with_push_config_store(PostgresPushConfigStore::new(pool))
    .build()
```

## Security Considerations

- **Always use HTTPS** for webhook URLs in production
- The built-in `HttpPushSender` rejects private IP addresses to prevent SSRF attacks
- Webhook credentials are validated for header injection characters
- Consider rate limiting webhook delivery to prevent abuse

## Next Steps

- **[Interceptors & Middleware](./interceptors.md)** — Server-side request hooks
- **[Task & Config Stores](./stores.md)** — Persistent storage backends
