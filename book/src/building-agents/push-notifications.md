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

- **HTTPS delivery** — With the `tls-rustls` feature (enabled by default via the `a2a-protocol-sdk` crate) it delivers to both `http://` and `https://` webhooks. In a build with the feature disabled it is plaintext-HTTP only and fails fast on an `https://` target with a clear error.
- **SSRF protection** — Resolves URLs and rejects private/loopback IP addresses. Uses `validate_webhook_url_with_dns()` which performs DNS resolution before IP validation, preventing DNS rebinding attacks where a hostname initially resolves to a public IP but later resolves to a private IP. For `http://` the validated IP is pinned at connect time; for `https://` the rebinding window is closed by TLS certificate verification instead (so the original hostname is preserved for SNI).
- **Header injection prevention** — Validates credentials contain no `\r` or `\n`

> **Capability + task-existence rules (spec §3.1.7, §3.3.4).** If you configure
> an agent card, it must advertise `capabilities.pushNotifications = true` or the
> push-config operations return `PushNotificationNotSupportedError`. Creating a
> config also requires the **target task to already exist** — a
> `CreateTaskPushNotificationConfig` for an unknown task returns
> `TaskNotFoundError` rather than storing an unroutable config. A
> `GetTaskPushNotificationConfig` for a config that does not exist likewise
> returns `TaskNotFoundError` (HTTP 404 over REST), not an invalid-params error.

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
    // `credentials` is `Option<String>`.
    credentials: Some("my-secret-token".into()),
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

struct DynamoDbPushConfigStore { /* ... */ }

impl PushConfigStore for DynamoDbPushConfigStore {
    // Implement set, get, list, delete...
}

RequestHandlerBuilder::new(executor)
    .with_push_config_store(DynamoDbPushConfigStore::new(client))
    // (SQLite and PostgreSQL push-config stores ship with the crate —
    //  SqlitePushConfigStore / PostgresPushConfigStore.)
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
