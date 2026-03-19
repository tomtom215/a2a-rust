# Multi-Language Team

Demonstrates a Rust coordinator agent that delegates work to worker agents implemented in Python, JavaScript, Go, and Java — proving end-to-end cross-language A2A interoperability.

**Source:** [`examples/multi-lang-team/`](https://github.com/tomtom215/a2a-rust/tree/main/examples/multi-lang-team)

## Architecture

```text
        ┌─────────────────────────┐
        │   Rust Coordinator      │  ← accepts user requests via A2A
        │   (a2a-protocol-sdk)    │
        └──────┬──┬──┬──┬────────┘
               │  │  │  │
          ┌────┘  │  │  └────┐
          ▼       ▼  ▼       ▼
        Python   JS  Go    Java     ← worker agents (each language)
        :9100  :9101 :9102 :9103
```

The coordinator:
1. Receives a user message via A2A
2. Fans out the request to all available workers (with 10s timeout per worker)
3. Collects responses (or error messages for unavailable workers)
4. Combines all results into a single artifact

## Running

### 1. Start the worker agents

```bash
cd itk/agents/python && python agent.py &
cd itk/agents/js-agent && node index.js &
cd itk/agents/go-agent && go run . &
cd itk/agents/java-agent && mvn compile exec:java &
```

### 2. Start the Rust coordinator

```bash
cargo run -p multi-lang-team
```

## What it demonstrates

| Feature | How |
|---------|-----|
| **Cross-language A2A** | Rust coordinator talks to Python/JS/Go/Java agents via the A2A protocol |
| **Fan-out pattern** | Coordinator sends to all workers, collects results |
| **Graceful degradation** | Connection errors and timeouts are reported, not fatal |
| **`ClientBuilder`** | Used to create A2A clients for each worker dynamically |
| **Timeout handling** | 10s per-worker timeout via `tokio::time::timeout` |

This example validates that any A2A-compliant agent — regardless of implementation language — can interoperate with the Rust SDK.
