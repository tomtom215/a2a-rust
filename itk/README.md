# A2A Integration Test Kit (ITK)

Cross-language interoperability testing for the A2A protocol.

## Overview

The ITK verifies that the Rust A2A SDK can communicate with agents written in
all official SDK languages: Python, JavaScript/TypeScript, Go, and Java.

Two agent tiers exist per language:

1. **Stub agents** (`python/`, `js-agent/`, `go-agent/`, `java-agent/`) —
   dependency-light servers that hand-write the wire format. They validate
   that a *minimal independent implementation* of the spec interoperates.
2. **Official-SDK agents** (`python-sdk/`, `js-sdk/`, `go-sdk/`,
   `java-sdk/`) — the same echo contract built on the official reference
   SDKs (`a2a-sdk` 1.x, `@a2a-js/sdk` 1.x, `a2a-go/v2`, `a2a-java`
   1.0.0.CR1). These are the runs that prove real cross-SDK interop.

The reverse direction is covered too: `interop/python_client_vs_rust.py`
drives our `echo-agent` with the official Python SDK **client** over both
HTTP bindings (send, streaming, get, list, cancel semantics, push-config
CRUD, terminal-subscribe rejection).

Each echo agent:
1. Accepts a `SendMessage` request
2. Returns a completed task with the echoed message as an artifact
3. Supports all A2A v1.0 methods via both JSON-RPC and REST bindings

### Known reference-SDK divergences (TCK `--skip` targets)

Documented upstream bugs surfaced by running our TCK against the official
SDKs — skipped with `--skip` (reported, never silent) in CI:

| SDK | Test | Divergence |
|---|---|---|
| `@a2a-js/sdk` 1.0.0 | `list_tasks_basic` | `ListTasksRequest.fromJSON({})` materializes proto3 default `status=TASK_STATE_UNSPECIFIED`; the store filters on it, so unfiltered lists are always empty. |
| `@a2a-js/sdk` 1.0.0 | `a2a_media_type_accepted` | Rejects `application/a2a+json` requests; spec §6.1 examples use that media type and the Python/Go SDKs accept it. |
| `a2a-java` 1.0.0.CR1 | `a2a_media_type_accepted` | Same `application/a2a+json` rejection. |

## Architecture

```
itk/
├── agents/
│   ├── python/          # Python agent (Starlette + uvicorn)
│   ├── js-agent/        # Node.js agent (Express)
│   ├── go-agent/        # Go agent (net/http stdlib)
│   └── java-agent/      # Java agent (com.sun.net.httpserver)
├── Dockerfile.rust-agent  # Builds the Rust echo agent image
├── Dockerfile.tck         # Builds the TCK runner image
├── docker-compose.yml     # Runs all agents + tests
└── README.md
```

## Running

### Docker Compose (recommended)

```bash
docker compose -f itk/docker-compose.yml up --build --abort-on-container-exit
```

### Manual

1. Start each language agent on its designated port:
   - Python: `cd itk/agents/python && pip install -r requirements.txt && python agent.py` (port 9100)
   - Node.js: `cd itk/agents/js-agent && npm install && node index.js` (port 9101)
   - Go: `cd itk/agents/go-agent && go run .` (port 9102)
   - Java: `cd itk/agents/java-agent && mvn compile exec:java` (port 9103)

2. Run the Rust TCK against each:
   ```bash
   # Test JSON-RPC binding
   cargo run -p a2a-tck -- --url http://localhost:9100 --binding jsonrpc
   cargo run -p a2a-tck -- --url http://localhost:9101 --binding jsonrpc
   cargo run -p a2a-tck -- --url http://localhost:9102 --binding jsonrpc
   cargo run -p a2a-tck -- --url http://localhost:9103 --binding jsonrpc

   # Test REST binding
   cargo run -p a2a-tck -- --url http://localhost:9100 --binding rest
   cargo run -p a2a-tck -- --url http://localhost:9101 --binding rest
   cargo run -p a2a-tck -- --url http://localhost:9102 --binding rest
   cargo run -p a2a-tck -- --url http://localhost:9103 --binding rest
   ```


## a2a-inspector validation

The official [a2a-inspector](https://github.com/a2aproject/a2a-inspector)
is a web-only debugging tool; its agent-card validation logic lives in
`backend/validators.py`. `interop/inspector_card_check.py` vendors that
exact ruleset and runs it headlessly against a live agent's card — the
scriptable equivalent of opening the agent in the inspector and confirming
it validates. Our `echo-agent` passes it; CI runs it in the TCK self-test
job.

Note: the inspector still lists the top-level `url` field as *required*,
which A2A v1.0 made optional (superseded by `supportedInterfaces`). Our
echo-agent sets `url` and passes; the official Python/Go/Java SDK echo
agents omit it and are flagged by the inspector — an inspector-lags-spec
issue, not a card defect.
