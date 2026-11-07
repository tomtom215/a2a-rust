<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# 4. The `a2a` command-line client

Scripts: `scripts/live/adopter/04-cli.sh` (the transcript),
`04b-cli-followup.sh` (signals, stalls, large bodies), `cli_stub.py` (a
misbehaving peer on nine loopback ports). Raw output:
`evidence/04-cli-transcript.log`, `evidence/04b-cli-followup.log`,
`evidence/04c-cli-followup-rerun.log`.

Built from this checkout (`cargo build -p a2a-cli`, `RUSTFLAGS=-D warnings`, debug).
`a2a --version` printed `a2a 0.16.0`. The agent was `examples/echo-agent` in
server-only mode (`A2A_BIND_ADDR=127.0.0.1:3111`, JSON-RPC and HTTP+JSON on one
port, as `tools/a2a-cli/README.md` says).

## Install

The book's command, as written, `cargo install --path tools/a2a-cli` (no `--locked`, so Cargo resolves
the tree afresh instead of reading `Cargo.lock`; `--debug` only to save disk, `04a-cli-install.sh`):
**built in 3 m 51 s, installed, and `a2a --version` printed `a2a 0.16.0`.** Cargo said `Locking 230
packages to highest Rust 1.94 compatible versions`: the workspace's `rust-version` steers the resolver.

## Subcommands run

`card`, `card sign`, `card verify`, `audit verify`, `send` (default, `--binding rest`,
`--binding jsonrpc`, `--no-wait`, `--context-id`, `--tenant`), `stream`, `task get`,
`task list` (page size, context filter, bad page token), `task cancel`, `task subscribe`,
and the global flags `--header`, `--header-env`, `--follow-card-origin`, `--timeout`,
`--send-timeout`, `--binding grpc --grpc-plaintext`, `--binding websocket`.
**Not run**: `--cacert`, `--client-cert`/`--client-key` (no TLS agent was stood up),
`--no-capability-check`, `--from` on `task subscribe`, `--task-id`, `audit verify` on a
real exported chain (only the missing-file and not-records errors), `--binding grpc` and
`--binding websocket` against an agent that serves them (echo-agent in server-only mode
serves neither; the two commands were run against the wrong port to read the errors).

**There is no `completions` subcommand and no `--json` flag**, and neither the README, the
book page nor `--help` claims one (`a2a completions bash` and `a2a send ... --json` both
exit 2 with clap's "unrecognized subcommand" and "unexpected argument"). Every command
prints JSON already. There is no `discovery` subcommand: `a2a card` is discovery.

## What matched the documentation

| Claim | Observed |
|---|---|
| Exit 0 on success, 2 on a clap usage error | `--timeout 0`, `--header novalue`, `--header-env` of an unset variable, `--binding carrier-pigeon`, a missing `<TEXT>`, an unknown subcommand: all exit 2 with a message naming the argument |
| Exit 1 on a protocol or transport error, `error: ...` on stderr and the agent's JSON error as another line | `task get no-such-task`: `error: protocol error: [-32001] Task not found: no-such-task` then `{"error":{"code":-32001,...}}`, exit 1 (but see D-8: a log line comes first) |
| `--timeout` bounds a request, `--send-timeout` a blocking send | A peer that reads the request and never answers: `send --timeout 3 --send-timeout 3` exits 1 after 3.03 s with a message that says what to do ("raise `ClientBuilder::with_send_timeout`, or stream..."); `card --timeout 3` exits 1 after 3.02 s |
| `--header` values go only to the host in the URL, unless `--follow-card-origin` | A card naming `http://evil.example.invalid:9`: refused with a message naming both hosts; with the flag the call is attempted and the client logs a plaintext-credential warning naming the header, not its value |
| A secret on the command line is not printed | `s3cr3t-token-do-not-print`, `user:pw-in-url-123@` in the URL and `?api_key=qs-secret-456` in the query appear in no stdout or stderr of any run; the stub's log shows the header really was sent |
| Signing and verification work offline | `card sign` (Ed25519 PKCS#8 DER from `openssl genpkey`) then `card verify` against a JWK Set built from the public key: `{"signatures": 1, "verified": true}`, exit 0. A tampered card, an unsigned card, a wrong key: exit 1 with distinct messages |
| An unusable `OTEL_*` setting is exit 2 | `OTEL_EXPORTER_OTLP_ENDPOINT=not-a-url`: exit 2, one `error:` line. Other bad values (`OTEL_EXPORTER_OTLP_PROTOCOL=carrier-pigeon`, `OTEL_SDK_DISABLED=maybe`) are a `WARN` and a default, exit 0; they are not what the README's "cannot use" means, but the README does not say which kind is which |
| Unreachable collector does not break the command | `send` with `OTEL_EXPORTER_OTLP_ENDPOINT=http://127.0.0.1:4318` and nothing there: the task is printed, exit 0, three `WARN` lines (traces, metrics, logs) at exit, 1.95 s in total |

## Defects and rough edges

See `README.md` in this directory for the ranked list. In order of what a stranger meets:

* **D-8 (low).** A protocol error is preceded by the library's own `WARN` log line on
  stderr (`2026-11-07T... WARN a2a_protocol_client::transport::jsonrpc: JSON-RPC error
  response method="GetTask" code=-32001`), so stderr is three lines and not the "one
  `error:` line, then a second line carrying the JSON" that `tools/a2a-cli/README.md`'s
  exit-code table and transcript describe. The transcript was captured on 2026-09-10 and
  the CLI has installed the SDK's subscriber since. A 401, 429 and 500 on a
  `--binding` call print the same kind of line; on card discovery they do not.
* **D-9 (low).** A malformed URL (`a2a card 127.0.0.1:3111`, `a2a card "not a url"`) is exit 1
  (`error: invalid endpoint: base URL must start with http:// or https://: ...`), not exit 2,
  though the table puts "a value that does not parse" under 2. The message is clear.
* **D-10 (low).** A 401 prints `error: unexpected HTTP status 401: {"error":"unauthorized"}`.
  The response's `WWW-Authenticate: Bearer realm="a2a"` and a 429's `Retry-After: 2` are not
  shown, and nothing says "pass `--header-env Authorization=VAR`". The agent's body is shown
  verbatim, which is what the README promises.
* **D-11 (low).** A message can only be given as one argument, so it is bounded by the
  kernel's 128 KiB single-argument limit: 100,000 bytes worked, 200,000 bytes fails with the
  shell's `Argument list too long` before `a2a` starts. `a2a send URL -` sends the text
  `-`, not stdin. Nothing says so.
* **D-12 (low).** `--timeout` does not bound a stream that has started. A server that sent one event
  and went silent held `a2a stream ... --timeout 3` for exactly 300 s (`evidence/04c-cli-followup-rerun.log`),
  the library's idle timeout (`with_stream_idle_timeout`), and then it exited 1 with a good message
  (`stream idle timeout: no data, not even a keep-alive comment, for 300s since the last data received; the
  task on the server is not cancelled, so resubscribe to continue`). The CLI exposes no flag for it and neither
  README mentions it. SIGINT during that stream, and during a blocking send to a half-open peer, ends the
  process at once (killed by the signal, no output after it).
* **D-13 (polish).** That idle-timeout error was followed by the "agent card named another URL, pass
  `--binding`" hint (`connect/route.rs`: any `Timeout` on a card-routed call to an endpoint other than the typed URL),
  which is wrong for a stream that was connected and had delivered an event.
* `task list --page-token '!!!'` answers `{"tasks":[],"nextPageToken":"","pageSize":0,"totalSize":0}`
  and exit 0, although the task list holds six: a bad or expired cursor reads as "no more
  tasks". Deliberate in the in-memory store (a comment says so); listed under "needs a decision".
* `a2a send ... --tenant acme` against echo-agent is refused with an accurate, actionable
  message naming `accept_unisolated_tenants()`; it is a good error.

## Verdict

The CLI does what its README and book page say on every path that was run, with the
exceptions above (all low). No panic, no hang other than the stream idle case, no secret
echoed, no wrong exit code on a usage error.
