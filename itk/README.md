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
   1.3.0.Final). These are the runs that prove real cross-SDK interop.

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

| SDK | Test | Binding | Divergence |
|---|---|---|---|
| `a2a-java` 1.3.0.Final | `a2a_media_type_accepted` | REST only | Rejects `application/a2a+json`, which §11 says **SHOULD** be used for requests and responses. Re-verified 2026-08-30 at 1.3.0.Final. |

**A skip is a claim about someone else's software.** Every entry asserts that
a named third party is wrong, so each one needs re-checking against the
current release. This kit exits 1 on a `--skip`ped test that passes, which
catches a stale entry — but only once the pin moves.

Moving it is [`pin-freshness.yml`](../.github/workflows/pin-freshness.yml)'s
job: every three weeks it re-resolves each official SDK to its latest release
and re-grades against this table, so decay is reported rather than waiting to
be noticed. It never commits a pin bump — which version this repository
grades against stays a human decision.

Three of the four entries this table carried were cleared on 2026-08-30, and
the reasons are worth keeping:

**`@a2a-js/sdk` `list_tasks_basic` — was real, now fixed.** In 1.0.0,
`ListTasksRequest.fromJSON({})` materialised the proto3 default
`status=TASK_STATE_UNSPECIFIED` and the store filtered on it, so an unfiltered
list always came back empty. Fixed in 1.1.0, verified on both bindings. It
went on being reported for months because `package-lock.json` pinned 1.0.0
while 1.1.0 was current: **CI was grading a superseded release.**

**`@a2a-js/sdk` and `a2a-java` `a2a_media_type_accepted` on JSON-RPC — never
their bug.** Both reject `application/a2a+json` on JSON-RPC, and both are
right to: §9 specifies `Content-Type: application/json`, §14.1.1 registers the
A2A media type with the note *"This media type is intended for the
HTTP+JSON/REST binding"*, and §6's examples that use it are REST requests
(`POST /message:send`). Our own check was demanding it on the wrong binding —
on the strength of a comment claiming `a2a-protocol-client` sends it, which it
does not on either binding. The check is REST-only now. Two conformant SDKs
were listed here as divergent for as long as the check was wrong.

A third defect blamed on the JS SDK in the same session — `TaskNotFound`
answered as `-32603`/HTTP 500 — was also already fixed in 1.1.0, and was
nearly reported upstream against a release nobody runs.

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
