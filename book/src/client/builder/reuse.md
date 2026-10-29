<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Reusing and sharing a client

Create clients once and reuse them across requests. Each `A2aClient` holds
a connection pool internally (via hyper), so reuse avoids repeated DNS
resolution, TCP handshakes, and TLS negotiation on every call.

```rust,no_run
# use a2a_protocol_sdk::prelude::*;
// Good: build once, reuse across requests
struct MyOrchestrator {
    analyzer: A2aClient,
    builder: A2aClient,
}

impl MyOrchestrator {
    fn new(analyzer_url: &str, builder_url: &str) -> Self {
        Self {
            analyzer: ClientBuilder::new(analyzer_url).build().unwrap(),
            builder: ClientBuilder::new(builder_url)
                .with_protocol_binding("HTTP+JSON")
                .build()
                .unwrap(),
        }
    }

    async fn run(&self, params: MessageSendParams) {
        // Reuse the same client for every request
        let _ = self.analyzer.send_message(params.clone()).await;
        let _ = self.builder.send_message(params).await;
    }
}
```

```rust,no_run
# use a2a_protocol_sdk::prelude::*;
// Avoid: rebuilding the client on every call
async fn bad_pattern(url: &str, params: MessageSendParams) {
    // This works but wastes resources — connection pool is discarded each time
    let client = ClientBuilder::new(url).build().unwrap();
    let _ = client.send_message(params).await;
}
```

## Sharing across tasks

`A2aClient` is `Send + Sync` and can be shared across tasks via `Arc`:

```rust,no_run
# use a2a_protocol_sdk::prelude::*;
# #[tokio::main]
# async fn main() {
# let url = "http://agent.example.com";
# let params = MessageSendParams::new(Message::new("m", MessageRole::User, vec![Part::text("hi")]));
# let (params1, params2) = (params.clone(), params);
use std::sync::Arc;

let client = Arc::new(
    ClientBuilder::new(url).build().unwrap()
);

// Share across async tasks
let c1 = Arc::clone(&client);
tokio::spawn(async move { c1.send_message(params1).await });

let c2 = Arc::clone(&client);
tokio::spawn(async move { c2.send_message(params2).await });
# }
```
