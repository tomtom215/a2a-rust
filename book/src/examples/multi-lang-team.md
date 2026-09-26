# Multi-Language Team

Demonstrates a Rust coordinator agent that delegates work to worker agents implemented in Python, JavaScript, Go, Java — and Rust, so the package shows both halves of the SDK. It shows the shape of cross-language delegation; it does not prove it on its own — CI runs it with every worker unreachable. Cross-SDK interoperability is measured by `tck.yml`'s cross-language jobs and `scripts/go_sdk_interop.sh` instead.

**Source:** [`examples/multi-lang-team/`](https://github.com/tomtom215/a2a-rust/tree/main/examples/multi-lang-team)

## Architecture

```text
        ┌─────────────────────────────────┐
        │   Rust Coordinator              │  ← accepts user requests via A2A
        │   (a2a-protocol-sdk)            │
        └──┬──────┬──────┬──────┬──────┬──┘
           │      │      │      │      │
           ▼      ▼      ▼      ▼      ▼
        Python    JS     Go    Java   Rust    ← worker agents (each language)
        :9100   :9101  :9102  :9103  :9104
```

The coordinator:
1. Probes the workers once at startup and reports which are reachable
2. Receives a user message via A2A
3. Fans out the request to the reachable workers (with 10s timeout per worker)
4. Collects responses (or an error line for a worker that fails or times out)
5. Combines all results into a single artifact — or, with no worker reachable, answers on its own and says so in the artifact

## Running

### 1. Start the worker agents

```bash
cd itk/agents/python && python agent.py &
cd itk/agents/js-agent && node index.js &
cd itk/agents/go-agent && go run . &
cd itk/agents/java-agent && mvn compile exec:java &
cargo run -p multi-lang-team --bin rust-worker &
```

### 2. Start the Rust coordinator

```bash
cargo run -p multi-lang-team
```

Without `A2A_BIND_ADDR`, this is a self-driving run: it starts the
coordinator on all four bindings, drives every A2A method against it, sends
one request through the fan-out and prints the combined artifact, then exits.
Set `A2A_BIND_ADDR` to serve the coordinator over JSON-RPC instead.

## What it demonstrates

| Feature | How |
|---------|-----|
| **Cross-language A2A** | Rust coordinator talks to Python/JS/Go/Java agents via the A2A protocol |
| **Fan-out pattern** | Coordinator sends to all workers, collects results |
| **Graceful degradation** | Connection errors and timeouts are reported, not fatal |
| **`ClientBuilder`** | Used to create A2A clients for each worker dynamically |
| **Timeout handling** | 10s per-worker timeout via `tokio::time::timeout` |
