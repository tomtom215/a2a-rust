<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Multi-Language Team — Cross-Language A2A Interoperability

Demonstrates a Rust coordinator agent that delegates work to worker agents implemented in Python, JavaScript, Go, Java — and Rust — proving end-to-end cross-language A2A interoperability.

The package has two binaries. The coordinator (`src/main.rs`) is the **client**
side of this SDK: it builds a client per worker, fans out, and parses replies.
The Rust worker (`src/bin/rust-worker.rs`, agent in `src/worker.rs`) is the
**server** side: the same echo agent the four `itk/agents/` workers are, in
about forty lines of this SDK, so a reader who wants to see what a worker looks
like in Rust has one to run.

## Architecture

```
        ┌─────────────────────────┐
        │   Rust Coordinator      │  ← accepts user requests via A2A
        │   (a2a-protocol-sdk)    │
        └──┬────┬────┬────┬────┬──┘
           │    │    │    │    │
           ▼    ▼    ▼    ▼    ▼
        Python  JS   Go  Java Rust   ← worker agents (each language)
        :9100 :9101 :9102 :9103 :9104
```

The coordinator:
1. Receives a user message via A2A
2. Fans out the request to all available workers (with 10s timeout)
3. Collects responses **concurrently** (a down worker costs one timeout
   window in total, and is reported inline in the artifact)
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

# Rust worker (port 9104; RUST_WORKER_ADDR=host:port to move it):
cargo run -p multi-lang-team --bin rust-worker &
```

Any subset is fine — the coordinator probes each one at startup and says
which it found. The Rust worker needs nothing but this repository, so it is
the one to start first if the other toolchains are not installed.

### 2. Start the Rust coordinator

```bash
cargo run -p multi-lang-team
```

The coordinator starts on ephemeral ports (one per binding), sends a demo
request that fans out to every reachable worker, then drives every A2A method
over every binding against itself and prints the coverage matrix.

## Expected output

With only the Rust worker running (the state a fresh clone can reach with
`cargo` alone):

```
Multi-Language Agent Team Example
=================================

Worker agents:
  Python      http://127.0.0.1:9100    not reachable
  JavaScript  http://127.0.0.1:9101    not reachable
  Go          http://127.0.0.1:9102    not reachable
  Java        http://127.0.0.1:9103    not reachable
  Rust        http://127.0.0.1:9104    REACHABLE

Coordinator:
  JSON-RPC  http://127.0.0.1:<port>
  HTTP+JSON http://127.0.0.1:<port>
  gRPC      127.0.0.1:<port>
  WebSocket ws://127.0.0.1:<port>

=== Demo round-trip: one SendMessage, fanned out to every reachable worker ===

  Artifact: cross-lang-result
    [Rust Echo] Hello from the multi-language team demo!

=== Coverage: every A2A method over every binding ===

  --- JSONRPC ---
    [ok]   SendMessage                        task <uuid>
    ...
44 exercised, 0 not applicable, 0 missing, of 44 cells
...
Cross-language delegation exercised against 1 worker(s): Rust.
```

Each reachable worker contributes one line to the `cross-lang-result`
artifact, `[Python Echo] …`, `[JS Echo] …`, `[Go Echo] …`, `[Java Echo] …`,
`[Rust Echo] …`. A worker that is down is not in the artifact at all — it was
never delegated to, and the startup table says so — while one that goes down
*after* the probe is reported inline as `[<Language>] Error: …`. With no
workers at all the artifact says nothing was delegated, and the closing line
says the delegation was **not** exercised.

## What it demonstrates

| Feature | How |
|---------|-----|
| **Cross-language A2A** | Rust coordinator talks to Python/JS/Go/Java agents via A2A |
| **Fan-out pattern** | Coordinator sends to all workers, collects results |
| **A worker in this SDK** | `src/worker.rs`: `agent_executor!` + `EventEmitter`, one card, one artifact — the server half, `--bin rust-worker` |
| **Graceful degradation** | Connection errors and timeouts are reported, not fatal |
| **Agent card** | Coordinator publishes its own `AgentCard` with skills |
| **`ClientBuilder`** | Used to create A2A clients for each worker |
| **Timeout handling** | 10s per-worker timeout via `tokio::time::timeout` |

## Prerequisites

- Rust 1.88+ (MSRV)
- Worker agents from `itk/agents/` (Python, Node.js, Go, Java) — optional;
  the Rust worker needs only `cargo`

## License

Apache-2.0

After the demo round-trip the coordinator **keeps serving** (probe it with
the TCK or any A2A client) until you press Ctrl+C.
