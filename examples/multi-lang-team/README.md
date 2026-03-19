<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Multi-Language Team — Cross-Language A2A Interoperability

Demonstrates a Rust coordinator agent that delegates work to worker agents implemented in Python, JavaScript, Go, and Java — proving end-to-end cross-language A2A interoperability.

## Architecture

```
        ┌─────────────────────────┐
        │   Rust Coordinator      │  ← accepts user requests via A2A
        │   (a2a-protocol-sdk)    │
        └──────┬──┬──┬──┬─────────┘
               │  │  │  │
          ┌────┘  │  │  └────┐
          ▼       ▼  ▼       ▼
        Python   JS  Go    Java     ← worker agents (each language)
        :9100  :9101 :9102 :9103
```

The coordinator:
1. Receives a user message via A2A
2. Fans out the request to all available workers (with 10s timeout)
3. Collects responses (or error messages for unavailable workers)
4. Combines all results into a single artifact

## Running

### 1. Start the worker agents

Each worker is a standalone A2A agent in its respective language:

```bash
# Python worker (port 9100):
cd itk/agents/python && python agent.py &

# JavaScript worker (port 9101):
cd itk/agents/js-agent && node index.js &

# Go worker (port 9102):
cd itk/agents/go-agent && go run . &

# Java worker (port 9103):
cd itk/agents/java-agent && mvn compile exec:java &
```

### 2. Start the Rust coordinator

```bash
cargo run -p multi-lang-team
```

The coordinator starts on an ephemeral port and sends a demo request to all workers.

## Expected output

When all workers are running:

```
Multi-Language Agent Team Example
=================================

Coordinator (with card) listening on 127.0.0.1:<port>

Sending request to coordinator...

Task ID: <uuid>
State:   Completed

Artifact: cross-lang-result
[Python] Echo: Hello from the multi-language team demo!
[JavaScript] Echo: Hello from the multi-language team demo!
[Go] Echo: Hello from the multi-language team demo!
[Java] Echo: Hello from the multi-language team demo!
```

When workers are not running, you'll see connection errors for each unavailable worker — the coordinator gracefully handles partial availability.

## What it demonstrates

| Feature | How |
|---------|-----|
| **Cross-language A2A** | Rust coordinator talks to Python/JS/Go/Java agents via A2A |
| **Fan-out pattern** | Coordinator sends to all workers, collects results |
| **Graceful degradation** | Connection errors and timeouts are reported, not fatal |
| **Agent card** | Coordinator publishes its own `AgentCard` with skills |
| **`ClientBuilder`** | Used to create A2A clients for each worker |
| **Timeout handling** | 10s per-worker timeout via `tokio::time::timeout` |

## Prerequisites

- Rust 1.93+ (MSRV)
- Worker agents from `itk/agents/` (Python, Node.js, Go, Java)

## License

Apache-2.0
