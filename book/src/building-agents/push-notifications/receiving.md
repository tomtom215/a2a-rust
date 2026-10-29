<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Receiving push notifications

What arrives at your webhook, how to tell a genuine notification from a forged one, and how `WebhookReceiver` does those checks for you.

## Authenticating to the webhook

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

## What the webhook receives

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

## With `WebhookReceiver`

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
