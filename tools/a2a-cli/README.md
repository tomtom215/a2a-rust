<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# a2a-cli

A command-line client for A2A agents, built on `a2a-protocol-client`.

It is **unpublished** (`publish = false`, version `0.0.0`, like the examples)
and is built from this repository:

```sh
cargo run -p a2a-cli -- --help
# or build once and put the binary on your PATH
cargo build -p a2a-cli --release   # target/release/a2a
```

Every command prints its result to stdout as JSON — the protocol type
serialized as it is on the wire (`{"task": …}` / `{"message": …}` for
`send`, one externally tagged event per line for `stream`). Nothing is
reinterpreted, so what you read is what the agent sent.

## Commands

| Command | A2A method | Output |
|---|---|---|
| `a2a card <URL>` | agent card discovery (`GET /.well-known/agent-card.json`; a `<URL>` ending in `agent-card.json` is fetched as-is) | the card, pretty JSON |
| `a2a send <URL> <TEXT> [--context-id ID] [--task-id ID] [--no-wait]` | `SendMessage` with one text part | `{"task": …}` or `{"message": …}`, pretty JSON |
| `a2a stream <URL> <TEXT> [--context-id ID] [--task-id ID]` | `SendStreamingMessage` | one JSON object per line, until the terminal event |
| `a2a task get <URL> <TASK-ID>` | `GetTask` | the task, pretty JSON |
| `a2a task cancel <URL> <TASK-ID>` | `CancelTask` | the task after cancellation, pretty JSON |
| `a2a task list <URL> [--context-id ID] [--page-size N] [--page-token T]` | `ListTasks` | one page: `tasks`, `nextPageToken`, `pageSize`, `totalSize` |

`--no-wait` sets `returnImmediately`, so `send` prints the task as soon as it
is accepted; useful for creating something to `task get` or `task cancel`.

`stream` stops after the first terminal event — a status update in a terminal
state (`completed`, `failed`, `canceled`, `rejected`), a task object already
in one, or a message reply — or when the agent closes the stream, whichever
comes first.

## Global flags

Accepted before or after the subcommand.

| Flag | Meaning |
|---|---|
| `--binding jsonrpc\|rest\|grpc\|websocket` | Which binding to speak to `<URL>` with. **Default: none given — the agent card is fetched from `<URL>/.well-known/agent-card.json` and the binding is chosen the way `ClientBuilder::from_card` does: `JSONRPC` if the card offers it, otherwise the card's first interface; the card's URL for that interface is used, not `<URL>`.** With the flag no card is fetched and `<URL>` is the endpoint of that binding: `http(s)://` for `jsonrpc` and `rest`, `ws(s)://` for `websocket`, `host:port` or `http(s)://` for `grpc`. |
| `--timeout <SECS>` | Per-request timeout; also bounds establishing a stream. Default 30. The TCP connect timeout stays at the library's 10 s unless this is shorter. |
| `--header K=V` | Extra header on every request; repeatable. Split at the first `=`, so `--header "Authorization=Bearer abc=="` keeps the padding. Over gRPC the headers become request metadata; over WebSocket they go on the upgrade request. |
| `--tenant <ID>` | Tenant for multi-tenant agents. Overrides the tenant the card advertises for the chosen interface. |
| `--grpc-plaintext` | Dial a bare `host:port` gRPC address without TLS. The default is the library's: TLS for every host except loopback. No effect on an address that carries a scheme. |

There is no `--insecure-http`. The client library accepts `http://` without
an opt-in (its HTTPS connector is `https_or_http`), and this tool does not add
or remove any policy of its own: TLS verification is the library's, against
its bundled roots, with no way to turn it off here.

## Exit codes

| Code | Meaning |
|---|---|
| `0` | Success. |
| `1` | Protocol or transport error. One `error: …` line on stderr; when the agent answered with a structured error, a second stderr line carries it as JSON, `{"error":{"code":…,"message":…}}`, so a script can read the code. Nothing on stdout. |
| `2` | Usage error: unknown flag or command, missing argument, a value that does not parse (`--timeout 0`, `--header` without `=`). Reported by `clap`; no network is touched. |

## Transcript

Captured 2026-09-10 from a real run of the debug binary against
`examples/echo-agent` in server-only mode (`A2A_BIND_ADDR=127.0.0.1:3111
cargo run -p echo-agent`, which serves JSON-RPC and HTTP+JSON on one port and
a card advertising both) and `cargo run -p hello-agent` on port 3000. Each
command is followed by its exit code. Ids and timestamps are whatever the
agents generated.

```console
$ a2a card http://127.0.0.1:3111
{
  "name": "Echo Agent",
  "description": "A simple echo agent that mirrors your input",
  "version": "1.0.0",
  "supportedInterfaces": [
    {
      "url": "http://127.0.0.1:3111",
      "protocolBinding": "JSONRPC",
      "protocolVersion": "1.0"
    },
    {
      "url": "http://127.0.0.1:3111",
      "protocolBinding": "HTTP+JSON",
      "protocolVersion": "1.0"
    }
  ],
  "defaultInputModes": [
    "text/plain"
  ],
  "defaultOutputModes": [
    "text/plain"
  ],
  "skills": [
    {
      "id": "echo",
      "name": "Echo",
      "description": "Echoes your message back as an artifact",
      "tags": [
        "echo",
        "demo"
      ]
    }
  ],
  "capabilities": {
    "streaming": true,
    "pushNotifications": true,
    "extendedAgentCard": true
  }
}
# exit 0

$ a2a send http://127.0.0.1:3111 "hello from the command line"
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
# exit 0

$ a2a send http://127.0.0.1:3111 "over rest" --binding rest
{
  "task": {
    "id": "2991a0d8-1b4b-4126-84e7-a708fe9bd45b",
    "contextId": "2990f2e5-258d-4061-8b05-6adb2983509e",
    "status": {
      "state": "TASK_STATE_COMPLETED"
    },
    "artifacts": [
      {
        "artifactId": "echo-artifact",
        "parts": [
          {
            "text": "Echo: over rest"
          }
        ]
      }
    ]
  }
}
# exit 0

$ a2a stream http://127.0.0.1:3111 "stream me"
{"task":{"id":"a84d8021-4b7c-40e7-a67c-e56301dd4dac","contextId":"28dcafba-39c9-460c-b06a-bf36066b40fa","status":{"state":"TASK_STATE_SUBMITTED","timestamp":"2026-09-10T10:32:47.429Z"}}}
{"statusUpdate":{"taskId":"a84d8021-4b7c-40e7-a67c-e56301dd4dac","contextId":"28dcafba-39c9-460c-b06a-bf36066b40fa","status":{"state":"TASK_STATE_WORKING"}}}
{"artifactUpdate":{"taskId":"a84d8021-4b7c-40e7-a67c-e56301dd4dac","contextId":"28dcafba-39c9-460c-b06a-bf36066b40fa","artifact":{"artifactId":"echo-artifact","parts":[{"text":"Echo: stream me"}]},"lastChunk":true}}
{"statusUpdate":{"taskId":"a84d8021-4b7c-40e7-a67c-e56301dd4dac","contextId":"28dcafba-39c9-460c-b06a-bf36066b40fa","status":{"state":"TASK_STATE_COMPLETED"}}}
# exit 0

$ a2a send http://127.0.0.1:3111 "slow:hold on" --no-wait
{
  "task": {
    "id": "6feac0e8-7bb5-425d-9db5-f6c529a88090",
    "contextId": "f09e695c-67c2-41c6-95c8-7f4ef3ee88af",
    "status": {
      "state": "TASK_STATE_SUBMITTED",
      "timestamp": "2026-09-10T10:32:47.441Z"
    }
  }
}
# exit 0

$ a2a task get http://127.0.0.1:3111 6feac0e8-7bb5-425d-9db5-f6c529a88090
{
  "id": "6feac0e8-7bb5-425d-9db5-f6c529a88090",
  "contextId": "f09e695c-67c2-41c6-95c8-7f4ef3ee88af",
  "status": {
    "state": "TASK_STATE_WORKING"
  },
  "history": [
    {
      "messageId": "4e2e85c1-5719-4b12-9ec4-f418a24de365",
      "role": "ROLE_USER",
      "parts": [
        {
          "text": "slow:hold on"
        }
      ]
    }
  ]
}
# exit 0

$ a2a task cancel http://127.0.0.1:3111 6feac0e8-7bb5-425d-9db5-f6c529a88090
{
  "id": "6feac0e8-7bb5-425d-9db5-f6c529a88090",
  "contextId": "f09e695c-67c2-41c6-95c8-7f4ef3ee88af",
  "status": {
    "state": "TASK_STATE_CANCELED",
    "timestamp": "2026-09-10T10:32:47.495Z"
  },
  "history": [
    {
      "messageId": "4e2e85c1-5719-4b12-9ec4-f418a24de365",
      "role": "ROLE_USER",
      "parts": [
        {
          "text": "slow:hold on"
        }
      ]
    }
  ]
}
# exit 0

$ a2a task list http://127.0.0.1:3111 --page-size 2
{
  "tasks": [
    {
      "id": "6feac0e8-7bb5-425d-9db5-f6c529a88090",
      "contextId": "f09e695c-67c2-41c6-95c8-7f4ef3ee88af",
      "status": {
        "state": "TASK_STATE_CANCELED",
        "timestamp": "2026-09-10T10:32:47.495Z"
      },
      "history": [
        {
          "messageId": "4e2e85c1-5719-4b12-9ec4-f418a24de365",
          "role": "ROLE_USER",
          "parts": [
            {
              "text": "slow:hold on"
            }
          ]
        }
      ]
    },
    {
      "id": "a84d8021-4b7c-40e7-a67c-e56301dd4dac",
      "contextId": "28dcafba-39c9-460c-b06a-bf36066b40fa",
      "status": {
        "state": "TASK_STATE_COMPLETED"
      },
      "history": [
        {
          "messageId": "fa8730ed-a2e7-4a0f-bd7d-cb00cb193dfe",
          "role": "ROLE_USER",
          "parts": [
            {
              "text": "stream me"
            }
          ]
        }
      ]
    }
  ],
  "nextPageToken": "1789036367429:11",
  "pageSize": 2,
  "totalSize": 4
}
# exit 0

$ a2a task get http://127.0.0.1:3111 no-such-task
error: protocol error: [-32001] Task not found: no-such-task
{"error":{"code":-32001,"data":[{"@type":"type.googleapis.com/google.rpc.ErrorInfo","domain":"a2a-protocol.org","reason":"TASK_NOT_FOUND"}],"message":"Task not found: no-such-task"}}
# exit 1

$ a2a send http://127.0.0.1:3000 Ada
error: could not fetch the agent card from http://127.0.0.1:3000: unexpected HTTP status 404: {"error":"agent card not configured"}
hint: pass --binding to skip discovery and use the URL directly
# exit 1

$ a2a send http://127.0.0.1:3000 Ada --binding jsonrpc
{
  "task": {
    "id": "a95b31a6-f448-4b6a-9b14-cf3dfe993ef7",
    "contextId": "0bded4a9-7e49-4750-90cb-6fbd1d4ac993",
    "status": {
      "state": "TASK_STATE_COMPLETED"
    },
    "artifacts": [
      {
        "artifactId": "greeting",
        "parts": [
          {
            "text": "Hello, Ada!"
          }
        ]
      }
    ]
  }
}
# exit 0

$ a2a card http://127.0.0.1:3111 --timeout 0
error: invalid value '0' for '--timeout <SECS>': 0 is not in 1..18446744073709551615

For more information, try '--help'.
# exit 2
```

## Tests

`cargo test -p a2a-cli` runs unit tests for the grammar, the header parser,
the binding choice and the terminal-event rule, and `tests/e2e.rs`, which
starts the SDK's own server in-process — a `RequestHandler` behind the
JSON-RPC and HTTP+JSON dispatchers on one ephemeral loopback port, the
routing `examples/echo-agent` uses in server-only mode — and runs the built
binary against it with `std::process::Command`, asserting exit codes and
parsing the JSON it prints. Covered: `card`, `send` (discovery, `--binding
rest`, `--binding jsonrpc`, `--context-id`), `stream`, `task get` (found and
not found), `task cancel` of a running task, `task list`, every usage-error
path, and an unreachable agent.

gRPC and WebSocket are built into the binary and reachable with `--binding`,
but the integration test serves only the two HTTP bindings; the transcript
above exercises the same two.

## What building it found

This tool uses only the client crate's public surface, which makes it a check
of that surface from the outside. Two things it ran into, reported rather
than worked around:

* **`hello-agent` serves no agent card.** It never calls `with_agent_card`,
  so the JSON-RPC dispatcher answers `404 {"error":"agent card not
  configured"}` at the well-known path. The "smallest complete agent" is
  therefore undiscoverable, and the default (discovery) path of this tool
  fails against it — `--binding jsonrpc` is the way in, and the error message
  says so. The transcript shows both.
* **`resolve_agent_card` takes no headers.** `--header` reaches every method
  call through a `CallInterceptor`, but discovery has no such hook, so a card
  behind authentication cannot be fetched with this tool's flags. The
  agent-card fetch also has its own fixed 30-second budget, independent of
  `--timeout`.
* **The builder does not say which interface it chose.** `ClientBuilder::
  from_card` picks one, but the choice is not readable, and gRPC and WebSocket
  need different constructors (`build_grpc`, a custom transport), so the tool
  re-implements the same preference rule to know which to call. A `chosen_
  interface()` accessor on the builder would remove that duplication.
