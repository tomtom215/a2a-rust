<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Custom push sender

Deliver notifications some other way than an HTTP POST, such as a queue, by implementing `PushSender`.

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
