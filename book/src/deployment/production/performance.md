<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Performance Tuning

What affects throughput and latency once the agent is correct: the
allocator, the event queue's size, and reusing clients. Most of it needs no
change; the allocator is the exception worth acting on. Measured figures for
the SDK itself are on [Benchmark Results](../../reference/benchmarks.md).

## Connection handling

Both dispatchers use hyper's HTTP/1.1 and HTTP/2 support via `hyper_util::server::conn::auto::Builder`. This automatically negotiates the best protocol version.

`TCP_NODELAY` is enabled on the sockets of `serve::Server` (so `serve` and `serve_with_addr`), of the WebSocket listener, of the push sender and of the client's HTTP transports. The gRPC listener does not set it, and a listener you own (Axum) is yours to configure. This disables Nagle's algorithm, eliminating the ~40ms delayed-ACK latency that would otherwise penalize small SSE frames and JSON-RPC responses on loopback and low-latency networks.

## Task store performance

The `InMemoryTaskStore` uses a pre-allocated `HashMap` with secondary indexes for efficient queries:

- **O(1) amortized get** — a `HashMap` lookup regardless of store size; save and delete also maintain the ordered indexes, O(log n)
- **No resize-induced latency spikes** — pre-allocation to the configured `max_capacity` eliminates the periodic full-rehash events that cause unpredictable 5-7× latency cliffs when the table outgrows its capacity
- **O(log n + page\_size) list queries** — a `BTreeMap<(i64, u64), TaskId>` index keyed by (status timestamp, write sequence) gives the spec's status-timestamp-descending order and O(log n) cursor positioning, and a `HashMap<String, BTreeMap<(i64, u64), TaskId>>` context index enables O(log m + page\_size) filtered queries where m = matching tasks. This replaces the previous O(n log n) per-call sort that caused 20-70× regressions at 10K+ tasks.

## No web framework overhead

a2a-rust works directly with hyper — no middleware framework overhead. Cross-cutting concerns (rate limiting, request logging, auth) are handled via the interceptor chain rather than a framework.

## Event queue sizing

Tune the event queue for your workload:

```rust
# fn card() -> a2a_protocol_types::agent_card::AgentCard {
#     use a2a_protocol_types::agent_card::{AgentCapabilities, AgentCard, AgentInterface, AgentSkill};
#     AgentCard::new("my-agent", "1.0.0", AgentInterface::jsonrpc("http://localhost:3000"))
#         .with_description("An example agent")
#         .with_skill(AgentSkill::new("echo", "Echo", "Echoes text").with_tags(["echo"]))
#         .with_capabilities(AgentCapabilities::none().with_streaming(true))
# }
# use a2a_protocol_sdk::prelude::*;
# struct MyAgent;
# agent_executor!(MyAgent, |_ctx, _queue| async { Ok(()) });
# fn f(executor: MyAgent) -> ServerResult<RequestHandler> {
# let builder = RequestHandlerBuilder::new(executor).with_agent_card(card());
// High-throughput: larger queues for tasks producing >250 events/task
let builder = builder.with_event_queue_capacity(512);

// Memory-constrained: smaller queues
let builder = builder.with_event_queue_capacity(64);
# builder.build()
# }
```

> **Benchmark data:** Per-event cost rises once a task writes more events than
> the queue holds and a reader falls behind: the reader then catches up from the
> task's event log (see [Streaming](../../concepts/streaming/server.md#queue-limits)),
> which is more work than reading the live channel. Set capacity to at least
> your expected peak event count per task. The `serde_helpers::SerBuffer`
> module can further reduce per-event serialization overhead via thread-local
> buffer reuse.

## Allocator

Run the server on jemalloc (or mimalloc), not glibc's default allocator.
A server with a bounded task store sits at a steady heap size full of freed
holes, and glibc's allocator is slow in exactly that state; jemalloc is not.
Measured on the echo benchmark (`benches/sdk-comparison`, two server cores,
16 connections, unary `SendMessage`, 4 interleaved runs, 2026-10-03):

| Allocator | Requests/s | CPU µs per request | Resident memory |
|---|---|---|---|
| glibc 2.39 (default) | 7,189 | 217.7 | 79 MB |
| jemalloc 5 | 11,862 | 137.5 | 74 MB |
| mimalloc 2.1 | 10,457 | 155.9 | 74 MB |

Streaming sends gain in the same proportion (6,721 → 10,969 requests/s on
jemalloc). glibc's tunables do not close the gap: a larger thread cache
(`glibc.malloc.tcache_count=64`) measured the same as the default.

Two ways to switch, with the same effect:

- **In the binary**, with the `tikv-jemallocator` or `mimalloc` crate and a
  `#[global_allocator]` static in your `main.rs`. Both build their C sources
  with your Rust build; `tikv-jemallocator` also needs `make`.
- **At run time**, with no code change: install the system library and
  preload it.

```sh
# Debian/Ubuntu images
apt-get install -y libjemalloc2
LD_PRELOAD=/usr/lib/x86_64-linux-gnu/libjemalloc.so.2 ./my-agent
```

The SDK does not choose an allocator for you; a library that set
`#[global_allocator]` would override the one your application chose.
`examples/deploy-agent` sets jemalloc in its `main.rs` and is the pattern
to copy.

## Client retry & reuse

When calling remote agents, build clients once and reuse them. Connection reuse is critical for performance — creating a new client per request bypasses HTTP keep-alive and connection pooling, adding about 130-180 µs of overhead per call on loopback (more over TLS, where the handshake dominates):

```rust,no_run
# use a2a_protocol_sdk::prelude::*;
# async fn f(params: MessageSendParams) {
// Build once at startup
let client = ClientBuilder::new("http://agent.example.com")
    .with_retry_policy(RetryPolicy::default())
    .build()
    .unwrap();

// Reuse across all requests — client holds a connection pool
let result = client.send_message(params).await;
# }
```
