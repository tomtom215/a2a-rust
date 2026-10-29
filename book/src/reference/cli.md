<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# The `a2a` command-line client

`a2a` talks to an A2A agent from a shell: fetch its agent card, send a
message, follow a stream, get, cancel or list tasks. It also signs and
verifies agent cards and verifies an exported audit chain, with no agent
needed. Use it to try an agent, or this SDK, before writing any Rust, and to
script checks in CI.

It is built on `a2a-protocol-client` and uses only that crate's public API, so
it chooses a binding, sends headers and reports errors the way an application
built on the client does. Every command prints JSON on standard output: the
protocol type serialized as it is on the wire, not reinterpreted.

The source is `tools/a2a-cli` (its
[README](https://github.com/tomtom215/a2a-rust/blob/main/tools/a2a-cli/README.md)
has a full recorded session).

## Install

`a2a` is not on crates.io (`publish = false`); build it from a checkout of
this repository. Its version is the SDK release's, so `a2a --version` says
which release it was built from.

```bash
# Install the `a2a` binary into ~/.cargo/bin
cargo install --path tools/a2a-cli

# Or build it in the workspace: target/release/a2a
cargo build -p a2a-cli --release

# Or run it without installing
cargo run -p a2a-cli -- --help
```

No package manager carries it yet. `release.yml` has a `cli-binaries` job that
builds `a2a` for `x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu`,
`aarch64-apple-darwin` and `x86_64-pc-windows-msvc`, each archive with a
`.sha256` file and SLSA build provenance, and attaches them to the GitHub
release; it runs only when the maintainer sets the repository variable
`PUBLISH_CLI_BINARIES=true`. The `packaging/` directory holds a Homebrew
formula and winget manifests that install those archives. They are drafts:
not submitted to any tap or to `winget-pkgs`, and not validated by
`brew audit` or `winget validate`
([packaging/README.md](https://github.com/tomtom215/a2a-rust/blob/main/packaging/README.md)).

## First calls

Against `examples/echo-agent` on port 3111, from the recorded session in the
README (ids are whatever the agent generated):

```bash
a2a card http://127.0.0.1:3111
a2a send http://127.0.0.1:3111 "hello from the command line"
```

```text
{
  "task": {
    "id": "d7d25ae9-8a61-47f6-9160-f7304829b92f",
    "contextId": "05ed0339-b203-48a4-a756-ff6a89494263",
    "status": {
      "state": "TASK_STATE_COMPLETED"
    },
    "artifacts": [
      {
        "artifactId": "echo-artifact",
        "parts": [
          {
            "text": "Echo: hello from the command line"
          }
        ]
      }
    ]
  }
}
```

`stream` prints one event per line, each externally tagged, and stops after
the terminal event:

```bash
a2a stream http://127.0.0.1:3111 "stream me"
```

```text
{"task":{"id":"a84d8021-4b7c-40e7-a67c-e56301dd4dac","contextId":"28dcafba-39c9-460c-b06a-bf36066b40fa","status":{"state":"TASK_STATE_SUBMITTED","timestamp":"2026-09-10T10:32:47.429Z"}}}
{"statusUpdate":{"taskId":"a84d8021-4b7c-40e7-a67c-e56301dd4dac","contextId":"28dcafba-39c9-460c-b06a-bf36066b40fa","status":{"state":"TASK_STATE_WORKING"}}}
{"artifactUpdate":{"taskId":"a84d8021-4b7c-40e7-a67c-e56301dd4dac","contextId":"28dcafba-39c9-460c-b06a-bf36066b40fa","artifact":{"artifactId":"echo-artifact","parts":[{"text":"Echo: stream me"}]},"lastChunk":true}}
{"statusUpdate":{"taskId":"a84d8021-4b7c-40e7-a67c-e56301dd4dac","contextId":"28dcafba-39c9-460c-b06a-bf36066b40fa","status":{"state":"TASK_STATE_COMPLETED"}}}
```

## Commands that call an agent

| Command | A2A method | Prints |
|---|---|---|
| `a2a card <URL>` | Agent card discovery: `GET <URL>/.well-known/agent-card.json`; a `<URL>` that ends in `agent-card.json` is fetched as it is | The card |
| `a2a send <URL> <TEXT> [--context-id ID] [--task-id ID] [--no-wait]` | `SendMessage`, with one text part and a fresh message id | `{"task": …}` or `{"message": …}` |
| `a2a stream <URL> <TEXT> [--context-id ID] [--task-id ID]` | `SendStreamingMessage` | One JSON object per line, until the terminal event |
| `a2a task get <URL> <TASK-ID>` | `GetTask` | The task |
| `a2a task cancel <URL> <TASK-ID>` | `CancelTask` | The task after cancellation |
| `a2a task list <URL> [--context-id ID] [--page-size N] [--page-token T]` | `ListTasks` | One page: `tasks`, `nextPageToken`, `pageSize`, `totalSize` |

- `--context-id` continues a conversation; `--task-id` addresses an existing
  task, for example one waiting in `input-required`.
- `--no-wait` sets `returnImmediately`, so `send` prints the task as soon as
  the agent accepts it. Use it to create a task to `get` or `cancel`. `stream`
  ignores it.
- `stream` stops after the first terminal event: a status update in
  `completed`, `failed`, `canceled` or `rejected`, a task already in one of
  those states, or a direct message reply. It also stops when the agent closes
  the stream.
- `task list` without `--page-size` takes the agent's default. Pass
  `--page-token` with the previous page's `nextPageToken` to read the next.

`a2a card <URL>` fetches the card with the client library's defaults:
`--header` and `--timeout` do not apply to it. They do apply to the card
fetch that the other commands make to choose a binding, and to `card verify`.

## Commands that need no agent

Each reads files, prints a JSON report, and exits 1 when what it checks does
not hold, so a script can gate on it.

| Command | Does |
|---|---|
| `a2a card sign <CARD> --key <DER> --alg es256\|eddsa --kid <KID>` | Signs the card file and prints it with the signature added. The key is PKCS#8 DER (`openssl pkcs8 -topk8 -nocrypt -outform DER`); `--kid` is the key id the JWK Set names it by. |
| `a2a card verify <CARD> --jwks <JWKS>` | Checks the card's signatures against a JWK Set and prints `{"verified": true, "signatures": N}`, or exits 1. `CARD` is an agent URL (fetched as `a2a card` does) or a file. `JWKS` is an `https://` URL you trust, or a file; never the card's own `jku`, which is unverified until a signature is. |
| `a2a audit verify <RECORDS> [--checkpoints FILE] [--keys JWKS]` | Verifies an exported audit chain: hashes, order, anchors and signed checkpoints. `RECORDS` is a JSON array in `seq` order (`AuditLog::export`), `--checkpoints` a JSON array (`AuditStore::checkpoints`), `--keys` the public keys you trust to have signed checkpoints. Exits 1 unless the chain is intact. |

The `audit verify` report carries `chain`, `records`, `range`, `anchoredAt`,
`checkpointsVerified`, `signedThrough`, `unsignedTail` (records no checkpoint
covers), `intact` and `failure` (`seq` and `reason`, or `null`).
[Audit Trail](../deployment/audit.md) says how to export a chain and what each
check proves; [Provenance and Signatures](../deployment/provenance.md) covers
card signing.

## How the binding is chosen

Without `--binding`, `a2a` fetches the agent card from
`<URL>/.well-known/agent-card.json` and chooses the binding the way
`ClientBuilder::from_card` does: the first interface the card lists that the
client supports (spec §8.3.2). It then calls the URL the card gives for that
interface, not `<URL>`. If the card cannot be fetched, the error says so and
suggests `--binding`.

With `--binding`, no card is fetched and `<URL>` is the endpoint of that
binding: `http(s)://` for `jsonrpc` and `rest`, `ws(s)://` for `websocket`,
and `host:port` or `http(s)://` for `grpc`. Use it for an agent that publishes
no card, or to test one binding of an agent that serves several:

```bash
a2a send http://127.0.0.1:3111 "over rest" --binding rest
```

All four bindings are built into the binary.

## Global flags

Accepted before or after the command.

| Flag | Meaning |
|---|---|
| `--binding jsonrpc\|rest\|grpc\|websocket` | Skip discovery and speak this binding to `<URL>` (above). |
| `--timeout <SECS>` | Per-request timeout, which also bounds establishing a stream. Default 30; 0 is refused. The TCP connect timeout stays at the library's 10 s unless this is shorter. |
| `--header K=V` | Extra header on every request; repeatable. Split at the first `=`, so `--header "Authorization=Bearer abc=="` keeps the padding. Over gRPC the headers are request metadata; over WebSocket they go on the upgrade request. |
| `--tenant <ID>` | Tenant for a multi-tenant agent. Overrides the tenant the card gives for the chosen interface. |
| `--grpc-plaintext` | Dial a bare `host:port` gRPC address without TLS. By default, as in the client library, every host except loopback is dialled over TLS. No effect on an address with a scheme. |
| `--no-capability-check` | Send even when the agent card does not declare the capability the call needs. By default a client built from a card refuses, before sending anything, a stream to an agent whose card does not set `capabilities.streaming` (spec §3.3.4). No effect with `--binding`, which fetches no card. |

There is no flag to turn TLS verification off. TLS is the client library's,
against its bundled roots, and `http://` URLs are accepted as they are by the
library.

## Exit codes

| Code | Meaning |
|---|---|
| `0` | Success. |
| `1` | A protocol or transport error, or a failed offline check. One `error: …` line on standard error. When the agent answered with a structured error, a second line carries it as JSON, `{"error":{"code":…,"message":…}}`, so a script can read the code. A network command prints nothing on standard output; an offline command still prints its report. |
| `2` | A usage error: unknown flag or command, missing argument, or a value that does not parse (`--timeout 0`, `--header` without `=`). Reported before any network call. Also returned when the `OTEL_*` environment names a telemetry configuration the SDK cannot use. |

From the recorded session, a task that does not exist:

```text
$ a2a task get http://127.0.0.1:3111 no-such-task
error: protocol error: [-32001] Task not found: no-such-task
{"error":{"code":-32001,"data":[{"@type":"type.googleapis.com/google.rpc.ErrorInfo","domain":"a2a-protocol.org","reason":"TASK_NOT_FOUND"}],"message":"Task not found: no-such-task"}}
```

## Scripting

Output is JSON on standard output and errors go to standard error, so `jq`
reads the result directly. Create a task without waiting for it, then cancel
it (`slow:` makes `echo-agent` keep the task working):

```bash
id=$(a2a send http://127.0.0.1:3111 "slow:hold on" --no-wait | jq -r '.task.id')
a2a task get http://127.0.0.1:3111 "$id" | jq -r '.status.state'
a2a task cancel http://127.0.0.1:3111 "$id"
```

Gate a deployment on a signed card:

```bash
a2a card verify https://agent.example.com --jwks https://keys.example.com/jwks.json \
  || { echo "agent card signature does not verify"; exit 1; }
```

## Telemetry

With `OTEL_EXPORTER_OTLP_ENDPOINT` set (or a signal's own
`OTEL_EXPORTER_OTLP_<SIGNAL>_ENDPOINT`), `a2a` exports each call's `CLIENT`
span and `rpc.client.call.duration` there through the SDK's `Telemetry`,
configured by the standard `OTEL_*` environment, with service name `a2a`
unless `OTEL_SERVICE_NAME` says otherwise. It flushes before it exits.
Without an endpoint no exporter is built and nothing is sent. Warnings and
errors print on standard error either way; the startup line a server logs is
left out. [Observability](../deployment/observability.md) describes the
variables.

## Tests

`cargo test -p a2a-cli` runs unit tests for the grammar, the header parser,
the binding choice and the terminal-event rule, and two integration tests.
`tests/e2e.rs` starts the SDK's own server in-process, JSON-RPC and HTTP+JSON
on one loopback port, and runs the built binary against it, asserting exit
codes and the JSON it prints. `tests/offline.rs` signs a card and verifies it
(and a changed copy, which fails), and verifies an exported audit chain (and
an edited one, which fails). gRPC and WebSocket are reachable with
`--binding`, but neither test serves them.
