<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Timeouts, retries and delivery

The builder options that decide how long a call may take, whether a failed call is tried again, and what the agent sends back. Every option here has a default, listed in the [builder reference](./reference.md); set only the ones you need to change.

## Timeouts

```rust
# use a2a_protocol_sdk::client::ClientBuilder;
# let url = "http://agent.example.com";
use std::time::Duration;

let client = ClientBuilder::new(url)
    .with_timeout(Duration::from_secs(60))              // Per-request timeout (default: 30s)
    .with_connection_timeout(Duration::from_secs(5))     // TCP connect timeout (default: 10s)
    .with_stream_connect_timeout(Duration::from_secs(15)) // stream headers (default: 30s)
    .with_stream_first_event_timeout(Duration::from_secs(60)) // first event after that (default: 5 min)
    .with_stream_idle_timeout(Some(Duration::from_secs(120))) // silence allowed mid-stream (default: 5 min)
    .build()
    .unwrap();
```

## Output modes

Specify which MIME types the client can handle:

```rust
# use a2a_protocol_sdk::client::ClientBuilder;
# let url = "http://agent.example.com";
let client = ClientBuilder::new(url)
    .with_accepted_output_modes(vec![
        "text/plain".into(),
        "application/json".into(),
        "image/png".into(),
    ])
    .build()
    .unwrap();
```

Default: `["text/plain", "application/json"]`

## History length

Control how many historical messages are included in responses:

```rust
# use a2a_protocol_sdk::client::ClientBuilder;
# let url = "http://agent.example.com";
let client = ClientBuilder::new(url)
    .with_history_length(10)  // Include last 10 messages
    .build()
    .unwrap();
```

## Retry policy

Enable automatic retries on transient failures (connection errors, timeouts, HTTP 429/502/503/504):

```rust
# use a2a_protocol_sdk::client::ClientBuilder;
# use std::time::Duration;
# let url = "http://agent.example.com";
use a2a_protocol_client::RetryPolicy;

let client = ClientBuilder::new(url)
    .with_retry_policy(RetryPolicy::default())  // 3 retries, 500ms initial backoff
    .build()
    .unwrap();

// Custom retry configuration
let client = ClientBuilder::new(url)
    .with_retry_policy(
        RetryPolicy::default()
            .with_max_retries(5)
            .with_initial_backoff(Duration::from_secs(1))
            .with_max_backoff(Duration::from_secs(60))
            .with_backoff_multiplier(3.0),
    )
    .build()
    .unwrap();
```

Sends (`send_message`, `stream_message`, creating a push config) are retried only
on a 429, a 503, or a protocol error that names a retry delay (`RetryInfo`), all
of which prove the server did not process them. After a timeout or a dropped
connection they are not re-sent, except a `send_message` or `stream_message` that
carries an idempotency key the peer honours; see
[Idempotent Sends](../idempotency.md).

## Return immediately

For push notification workflows, return the task immediately without waiting for completion:

```rust
# use a2a_protocol_sdk::client::ClientBuilder;
# let url = "http://agent.example.com";
let client = ClientBuilder::new(url)
    .with_return_immediately(true)
    .build()
    .unwrap();
```

Streams have their own timeouts for each phase of a stream; [Streaming Responses](../streaming.md#stream-timeouts) explains each one.
