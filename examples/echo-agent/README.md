<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Echo Agent — A2A Rust SDK Example

A minimal end-to-end example of the [A2A protocol](https://github.com/a2aproject/a2a-spec) using the [a2a-rust](https://github.com/tomtom215/a2a-rust) SDK. The agent echoes incoming messages back as artifacts, demonstrating the complete request lifecycle.

## What it demonstrates

| Feature | How |
|---------|-----|
| **AgentExecutor** | Implements the `AgentExecutor` trait with Working → Artifact → Completed lifecycle |
| **JSON-RPC transport** | Server via `JsonRpcDispatcher`, client via `ClientBuilder` (default binding) |
| **REST transport** | Server via `RestDispatcher`, client via `ClientBuilder::with_protocol_binding("REST")` |
| **Streaming (SSE)** | `stream_message()` consumes `StatusUpdate` and `ArtifactUpdate` events |
| **Agent card discovery** | `resolve_agent_card()` fetches `/.well-known/agent.json` at runtime |
| **Task retrieval** | `get_task()` retrieves a completed task by ID |
| **Pre-bind pattern** | Listeners are bound before the handler is built so agent card URLs are correct |

## Running

```bash
# From the workspace root:
cargo run -p echo-agent

# With structured logging:
RUST_LOG=debug cargo run -p echo-agent --features tracing
```

## Expected output

```
=== A2A Echo Agent Example ===

JSON-RPC server listening on http://127.0.0.1:<port>
REST server listening on http://127.0.0.1:<port>

--- Demo 1: Synchronous SendMessage (JSON-RPC) ---
  Task ID:    <uuid>
  Status:     Completed
  Artifact:   echo-artifact
  Content:    Echo: Hello from JSON-RPC client!

--- Demo 2: Streaming SendMessage (JSON-RPC) ---
  Status update: Working
  Artifact update: echo-artifact
  Content:    Echo: Hello from streaming client!
  Status update: Completed

--- Demo 3: Synchronous SendMessage (REST) ---
  Task ID:    <uuid>
  Status:     Completed
  Content:    Echo: Hello from REST client!

--- Demo 4: Streaming SendMessage (REST) ---
  Status update: Working
  Content:    Echo: Hello from REST streaming!
  Status update: Completed

--- Demo 5: Agent Card Discovery ---
  Agent:      Echo Agent
  Version:    1.0.0
  Skills:     ["Echo"]
  Streaming:  true
  Interfaces: 2

--- Demo 6: GetTask ---
  Fetched task: <uuid> (Completed)

=== All demos completed successfully! ===
```

## Architecture

```
┌─────────────────────────────────────────┐
│                  main()                 │
│                                         │
│  1. Pre-bind TCP listeners              │
│  2. Build AgentCard with real URLs      │
│  3. Build RequestHandler                │
│  4. Start JSON-RPC + REST servers       │
│  5. Run client demos 1-6               │
└─────────┬──────────────┬────────────────┘
          │              │
    ┌─────▼─────┐  ┌─────▼─────┐
    │ JSON-RPC  │  │   REST    │
    │ Dispatcher│  │ Dispatcher│
    └─────┬─────┘  └─────┬─────┘
          │              │
          └──────┬───────┘
                 │
         ┌───────▼───────┐
         │RequestHandler │
         │               │
         │ EchoExecutor  │
         │ TaskStore     │
         │ EventQueue    │
         └───────────────┘
```

## Server-only mode (TCK / CI)

Set `A2A_BIND_ADDR` to run as a standalone server without the client demos:

```bash
A2A_BIND_ADDR=127.0.0.1:8080 cargo run -p echo-agent
```

This mode serves both JSON-RPC and REST on a single address and is used by the A2A [Test Compatibility Kit](https://github.com/a2aproject/a2a-tck).

## Prerequisites

- Rust 1.93+ (MSRV)
- No external services required — everything runs in-process

## License

Apache-2.0
