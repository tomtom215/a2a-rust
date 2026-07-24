<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# ITK current-mount traversal agent

This directory doubles as the **a2a-itk "current" agent** for this
repository. The upstream Integration Testing Kit
([a2aproject/a2a-itk](https://github.com/a2aproject/a2a-itk)) mounts an SDK
repo at `agents/repo/` and runs whatever agent it finds under
`<repo>/itk/`. Its `test_suite/current.py` detects a `Cargo.toml` here,
runs `cargo build --release`, and launches the resulting `itk-*` binary
with `--httpPort N --grpcPort N`.

`src/main.rs` implements the ITK's multi-hop **traversal instruction**
protocol on top of `a2a-protocol-server` and `a2a-protocol-client`:

- Incoming messages carry a serialized `itk.Instruction`
  (`protos/instruction.proto`, vendored verbatim from upstream) — as a raw
  `application/x-protobuf` part, a part named `instruction.bin`, or base64
  text.
- **`ReturnResponse`** returns its text, optionally *holding* the task in
  `WORKING` with a `task-finished` marker instead of completing.
- **`CallAgent`** resolves a peer agent card and calls it over the
  configured transport (`JSONRPC`, `GRPC`, `HTTP+JSON`) — plain, streaming,
  with a push-notification config, or via the disconnect-then-resubscribe
  flow — and propagates the peer's responses.
- **`SeriesOfSteps`** runs nested instructions in order and concatenates.

The agent serves all three bindings: JSON-RPC (`POST /` and `/jsonrpc`),
REST/HTTP+JSON (other paths), and gRPC on the `--grpcPort`.

## Running it locally

```bash
cargo build --release            # from this itk/ directory
./target/release/itk-current-agent --httpPort 10110 --grpcPort 11010
```

## Validation

Two layers, both in CI (`.github/workflows/itk.yml`):

1. **In-repo self-test** (`interop/itk_traversal_selftest.py`) — drives the
   agent with the exact ITK protobuf instruction contract over every
   transport (send, streaming, multi-hop, series, hold, resubscribe)
   without needing the full ITK cluster. Deterministic and fast.

   ```bash
   pip install httpx grpcio-tools
   ./target/release/itk-current-agent --httpPort 10110 --grpcPort 11010 &
   python interop/itk_traversal_selftest.py
   ```

2. **Upstream current-mount** — clones the real a2a-itk, mounts this repo,
   and runs every `current` scenario against the official Python v1.0
   baseline agent (multi-hop send / streaming / push / resubscribe across
   JSONRPC, gRPC, HTTP+JSON). This is the LF's own harness exercising our
   agent against agents built on the reference SDK, and our agent starts
   and serves correctly under it.

   ```bash
   git clone https://github.com/a2aproject/a2a-itk
   ln -s "$(git -C .. rev-parse --show-toplevel)" a2a-itk/agents/repo
   cd a2a-itk && uv run run_tests.py --sdks current,python_v10
   ```

   In CI this runs as the **`workflow_dispatch`-only** (manual)
   `itk-current-mount` job, not as a PR gate: the upstream a2a-itk's
   `uv.lock` pins several baseline dependencies (e.g. `aiosqlite` via
   `a2a-sdk[sqlite]`) to a **private** Google Artifact Registry that
   returns `401` to public runners, so the baseline cluster cannot be
   provisioned on GitHub-hosted CI. A maintainer with registry access (or
   a future public ITK lockfile) can trigger it from the Actions tab. The
   in-repo self-test (1) is the authoritative automated gate.
