# Push Notifications

Push notifications let agents deliver results asynchronously via webhooks. Instead of the client holding an SSE connection open, the server POSTs events to a URL the client provides.

## How Push Notifications Work

```text
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

The built-in `HttpPushSender` includes:

- **HTTPS delivery** — With the `tls-rustls` feature (enabled by default via the `a2a-protocol-sdk` crate) it delivers to both `http://` and `https://` webhooks. In a build with the feature disabled it is plaintext-HTTP only and fails fast on an `https://` target with a clear error.
- **SSRF protection** — Resolves URLs and rejects private/loopback IP addresses. Uses `validate_webhook_url_with_dns()` which performs DNS resolution before IP validation, preventing DNS rebinding attacks where a hostname initially resolves to a public IP but later resolves to a private IP. For `http://` the validated IP is pinned at connect time; for `https://` the rebinding window is closed by TLS certificate verification instead (so the original hostname is preserved for SNI).
- **Header injection prevention** — Validates credentials contain no `\r` or `\n`
- **HTTPS only, if you ask** — `HttpPushSender::new().https_only()` refuses an `http://` webhook (spec §13.2: webhook URLs SHOULD use HTTPS). A push config with one is refused at creation with `InvalidParams`, and a delivery to one is refused before connecting. Off by default.

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

### Managing Push Configs

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

## Authentication

Push configs support authentication for the webhook endpoint:

```rust,no_run
use a2a_protocol_sdk::types::push::{TaskPushNotificationConfig, AuthenticationInfo};

let mut config = TaskPushNotificationConfig::new("task-abc", "https://webhook.example.com");
config.authentication = Some(AuthenticationInfo {
    scheme: "bearer".into(),
    // `credentials` is `Option<String>`.
    credentials: Some("my-secret-token".into()),
});
```

For scheme `Bearer` or `Basic` (any case) the server sends `Authorization: <Scheme> <credentials>` when POSTing to the webhook; other schemes, or a scheme with no credentials, produce no header.

## Receiving Push Notifications

`HttpPushSender` POSTs the event as a `StreamResponse` JSON body with
`Content-Type: application/a2a+json`, the media type §4.3.3 specifies. If the
config has a `token`, it is sent twice, under both header names in use, with
the same value:

| Header | Who uses it |
|---|---|
| `X-A2A-Notification-Token` | a2a-sdk (Python) sends and reads it |
| `A2A-Notification-Token` | a2a-go sends and reads it |

The specification names no token header, and the two reference SDKs picked
different ones, so a webhook written against either one works with this
sender. Read the token in your own webhook the same lenient way:

```rust
use a2a_protocol_server::push::webhook::notification_token;

fn token_of(req: &hyper::Request<()>) -> Option<&str> {
    // Either spelling; `None` if two *different* tokens arrive.
    notification_token(req.headers())
}
```

Compare it with the token you registered using a constant-time comparison.
Accept `application/json` as well as `application/a2a+json`, because a2a-go
and a2a-sdk agents send the former. Log lines from the sender name the webhook
by `scheme://host[:port]` only, because webhook URLs often carry a secret in
their path or query.

## Custom PushSender

Implement the `PushSender` trait for custom delivery:

```rust
# use std::future::Future;
# use std::pin::Pin;
# use a2a_protocol_sdk::prelude::*;
# use a2a_protocol_sdk::types::push::TaskPushNotificationConfig;
# mod aws_sdk_sqs { pub struct Client; } // stands in for the real crate
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
# fn card() -> a2a_protocol_types::agent_card::AgentCard {
#     use a2a_protocol_types::agent_card::{AgentCapabilities, AgentCard, AgentInterface, AgentSkill};
#     AgentCard::new("my-agent", "1.0.0", AgentInterface::jsonrpc("http://localhost:3000"))
#         .with_description("An example agent")
#         .with_skill(AgentSkill::new("echo", "Echo", "Echoes text").with_tags(["echo"]))
#         .with_capabilities(AgentCapabilities::none().with_streaming(true).with_push_notifications(true))
# }
# use std::future::Future;
# use std::pin::Pin;
# use a2a_protocol_sdk::prelude::*;
# struct MyAgent;
# agent_executor!(MyAgent, |_ctx, _queue| async { Ok(()) });
# use a2a_protocol_sdk::types::push::TaskPushNotificationConfig;
use a2a_protocol_sdk::server::PushConfigStore;

struct MyPushConfigStore { /* ... */ }

impl PushConfigStore for MyPushConfigStore {
    // Implement set, get, list, delete...
#     fn set<'a>(&'a self, _: TaskPushNotificationConfig) -> Pin<Box<dyn Future<Output = A2aResult<TaskPushNotificationConfig>> + Send + 'a>> { unimplemented!() }
#     fn get<'a>(&'a self, _: &'a str, _: &'a str) -> Pin<Box<dyn Future<Output = A2aResult<Option<TaskPushNotificationConfig>>> + Send + 'a>> { unimplemented!() }
#     fn list<'a>(&'a self, _: &'a str) -> Pin<Box<dyn Future<Output = A2aResult<Vec<TaskPushNotificationConfig>>> + Send + 'a>> { unimplemented!() }
#     fn delete<'a>(&'a self, _: &'a str, _: &'a str) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> { unimplemented!() }
}

# fn f(executor: MyAgent) -> ServerResult<RequestHandler> {
RequestHandlerBuilder::new(executor)
    .with_agent_card(card())
    .with_push_config_store(MyPushConfigStore { /* ... */ })
    .with_push_sender(a2a_protocol_sdk::server::HttpPushSender::new())
    // (SQLite and PostgreSQL push-config stores ship with the crate —
    //  SqlitePushConfigStore / PostgresPushConfigStore.)
    .build()
# }
```

### Sealing stored configs

A push config carries secrets: the client's `token`, and the `credentials` the
agent presents to the webhook. The SQL push-config stores keep each config as
JSON, so without more they hold both in the clear (spec §13.2: agents SHOULD
store them securely). Give a store a `PushConfigCipher` and it writes each
config sealed, as `a2a-sealed:v1:<base64>`, and opens it on read:

```rust,no_run
# use std::future::Future;
# use std::pin::Pin;
# use std::sync::Arc;
use a2a_protocol_server::push::{PushConfigCipher, SqlitePushConfigStore};
use a2a_protocol_types::error::A2aResult;

/// An AEAD (AES-256-GCM, ChaCha20-Poly1305) whose key your KMS holds,
/// authenticating `context` as associated data.
struct KmsCipher;

impl PushConfigCipher for KmsCipher {
    fn seal<'a>(
        &'a self,
        plaintext: Vec<u8>,
        context: &'a [u8],
    ) -> Pin<Box<dyn Future<Output = A2aResult<Vec<u8>>> + Send + 'a>> {
        Box::pin(async move { aead_seal(plaintext, context) })
    }

    fn open<'a>(
        &'a self,
        sealed: Vec<u8>,
        context: &'a [u8],
    ) -> Pin<Box<dyn Future<Output = A2aResult<Vec<u8>>> + Send + 'a>> {
        Box::pin(async move { aead_open(sealed, context) })
    }
}
# fn aead_seal(p: Vec<u8>, _: &[u8]) -> A2aResult<Vec<u8>> { Ok(p) }
# fn aead_open(s: Vec<u8>, _: &[u8]) -> A2aResult<Vec<u8>> { Ok(s) }

# async fn f() -> Result<(), Box<dyn std::error::Error>> {
let store = SqlitePushConfigStore::new("sqlite:push.db?mode=rwc")
    .await?
    .with_cipher(Arc::new(KmsCipher));
# Ok(())
# }
```

The SDK ships no cipher, by design; bring a vetted AEAD. The server crate's
tests use AES-256-GCM from `ring` with a random 96-bit nonce per seal
(`tests/support/aes_gcm.rs`), which is safe for up to 2^32 seals under one key.

- **The context binds a config to its row**: the JSON array
  `["a2a-push-config/v1", partition, task_id, id]`, with `partition` `""` in a
  store without tenants. The cipher must authenticate it, or anyone who can
  write the table could move a sealed config to another task and receive that
  task's updates.
- **Fails closed**: with a cipher, a row that is not sealed, or one the cipher
  cannot open, is an error rather than a config. Without one, a sealed row is
  an error.
- **Migrating a table written before**: `accept_unsealed_rows()` also reads
  plain rows; each is sealed when it is next written. A config that is never
  rewritten stays plain, so find them (`data NOT LIKE 'a2a-sealed:%'` in
  SQLite, `jsonb_typeof(data) <> 'string'` in Postgres) and re-create them
  before dropping the flag.
- The task and config ids stay in their own columns, unsealed, so lookups and
  deletes work as before.

## Security Considerations

- **Always use HTTPS** for webhook URLs in production
- The built-in `HttpPushSender` rejects private IP addresses to prevent SSRF attacks
- Webhook credentials are validated for header injection characters
- Consider rate limiting webhook delivery to prevent abuse

## Next Steps

- **[Interceptors & Middleware](./interceptors.md)** — Server-side request hooks
- **[Task & Config Stores](./stores.md)** — Persistent storage backends
