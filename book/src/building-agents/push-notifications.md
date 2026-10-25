# Push Notifications

Push notifications let agents deliver results asynchronously via webhooks. Instead of the client holding an SSE connection open, the server POSTs events to a URL the client provides.

## How Push Notifications Work

```text
  Client              Agent Server          Client Webhook
     │                      │                      │
     │  SendMessage         │                      │
     │  + push config       │                      │
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

1. Client sends a message with a push config inline
   (`configuration.taskPushNotificationConfig`, with no task ID;
   `returnImmediately: true` for async). The server creates the task and
   registers the config for it.
2. Alternatively, once it holds a task ID, the client registers a webhook with
   `CreateTaskPushNotificationConfig`; a config for a task that does not exist
   is refused with `TaskNotFoundError`.
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
- **SSRF protection** — Resolves the host at delivery and rejects a URL that is, or resolves to, a private, loopback, link-local, CGNAT or IPv4-mapped address, a numeric IPv4 spelling such as `0x7f.1`, or an internal name (`url_guard::resolve_public_url`, public for your own senders). Resolving before validating prevents DNS rebinding attacks where a hostname initially resolves to a public IP but later resolves to a private IP. A config is also screened when it is created, without a DNS lookup, so a hostname that resolves privately is stored but never delivered to. For `http://` the validated IP is pinned at connect time; for `https://` the rebinding window is closed by TLS certificate verification instead (so the original hostname is preserved for SNI). `with_allowed_hosts([..])` restricts webhooks to listed hosts (checked at creation and at delivery); `allow_private_urls()` turns the screen off for tests.
- **Header injection prevention** — Validates credentials contain no `\r` or `\n`
- **HTTPS only, if you ask** — `HttpPushSender::new().https_only()` refuses an `http://` webhook (spec §13.2: webhook URLs SHOULD use HTTPS). A push config with one is refused at creation with `InvalidParams`, and a delivery to one is refused before connecting. Off by default.

**Delivery and retries.** Every config of a task is attempted for every event,
concurrently (`HandlerLimits::push_delivery_budget`, 30 s, sets how many at
once; at the defaults, all). One attempt is bounded by the sender's request
timeout, 10 s (`HttpPushSender::with_timeout`), after a DNS lookup bounded by
5 s (`with_dns_timeout`). `PushRetryPolicy` (`with_retry_policy`) allows 3
attempts with 1 s then 2 s between them (a shorter `backoff` list repeats its
last entry; 0 attempts counts as 1). A transport error, a timeout, `408`,
`429` and any `5xx` are retried with the same body; any other status, a
redirect included (it is not followed), fails the delivery at once, and the
response body is not read. A whole delivery, retries included, is bounded by
`HandlerLimits::push_delivery_timeout` (30 s); a webhook that never answers
needs up to 38 s at the defaults, so the third attempt can be cut short (the
`timeout_truncated` outcome of `a2a.server.push_deliveries`). A failed delivery
is counted (and logged, with the `tracing` feature) and does not fail the task.

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

Spec §13.2 says webhook URLs SHOULD use HTTPS: the payload, the token and any
credentials travel with the request. `set_push_config` (and a send carrying a
push config inline) logs a warning for an `http://` URL to a host other than
loopback; build the client with `ClientBuilder::require_https_webhooks()` to
refuse such a config with `ClientError::InsecureWebhook` before anything is
sent. `http://localhost`, `127.0.0.0/8` and `[::1]` are allowed either way.

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

For scheme `Bearer` or `Basic` (any case) the server sends `Authorization: <Scheme> <credentials>` when POSTing to the webhook. Any other scheme, and a scheme with absent or blank `credentials`, is refused when the config is created (`InvalidParams` naming `authentication.scheme` or `authentication.credentials`), and a stored config that slipped past that is not delivered. Leave `authentication` out for an unauthenticated webhook; the token header below is separate.

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

### With `WebhookReceiver`

The client crate's `webhook::WebhookReceiver` (feature `webhook`, on by
default) does the receiving side's checks from spec §4.3.3 and §13.2. Give
every config its own token from `new_notification_token()`, register the
config the agent stored, and pass each request's headers and body to
`receive`; answer with the outcome's `status()`.

```rust
use a2a_protocol_sdk::client::webhook::{new_notification_token, WebhookOutcome, WebhookReceiver};
use a2a_protocol_sdk::types::push::TaskPushNotificationConfig;

# fn main() -> Result<(), Box<dyn std::error::Error>> {
let receiver = WebhookReceiver::new() // duplicate detection on: 10 000 bodies, 1 hour
    .with_rate_limit(10, 20);          // per config: 10/s, bursts of 20 (off by default)

let mut config = TaskPushNotificationConfig::new("task-abc", "https://my-service.com/webhook");
config.token = Some(new_notification_token());
// ... `let config = client.set_push_config(config).await?;` ...
receiver.register(&config)?;

# fn handle(_: a2a_protocol_sdk::types::StreamResponse) {}
# let (headers, body) = (hyper::HeaderMap::new(), Vec::<u8>::new());
// In the webhook's HTTP handler:
let outcome = receiver.receive(&headers, &body);
let status = outcome.status(); // answer the agent with this
if let WebhookOutcome::Accepted { event, .. } = outcome {
    handle(*event); // a `Duplicate` is a retry already handled: just answer 200
}
# assert_eq!(status, 401);
# Ok(())
# }
```

| Check, in order | Refused with |
|---|---|
| The token (either header) belongs to a registered config — looked up by SHA-256, compared in constant time | 401 |
| If the config has `authentication`, `Authorization` is its scheme (any case) and credentials (constant time) | 401 |
| The config's rate limit, if one is set, is not spent | 429 |
| The body parses as a `StreamResponse` | 400 |
| The event's task is the config's task (a message with no task fails) | 403 |
| The same body for the same config, already accepted, is `Duplicate` | — (200) |

`register` refuses a config with no token, a token that is not visible ASCII
(it could not travel in a header unchanged), a token another config already
uses (§13.2: unique, single-purpose tokens), no task ID, or `authentication`
without `credentials`. A duplicate is the same *bytes* again: `HttpPushSender`
re-sends the body it serialized once, so its retries are caught. An event is
recorded as seen when `receive` returns `Accepted`; if your handler can fail
after that and you want the agent to retry, build the receiver
`without_dedup()` and deduplicate in your own store.

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

A custom sender gets none of `HttpPushSender`'s delivery-time checks or
retries. A sender that POSTs to the config's URL should resolve and screen it
with `url_guard::resolve_public_url`, and send the notification token and
`Authorization` as the config specifies. The handler still screens a URL when
the config is created, and bounds each `send` with `push_delivery_timeout`
(30 s). Override `requires_https`, `allows_webhook_url` and
`max_delivery_duration` when they apply.

## Push Config Storage

The default `InMemoryPushConfigStore` (the tenant-aware in-memory store when the task store isolates tenants) keeps configs in memory with per-task limits. For production use `SqlitePushConfigStore` or `PostgresPushConfigStore` (features `sqlite` / `postgres`; tenant-aware forms for multi-tenant servers), or implement `PushConfigStore` for your own database:

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
