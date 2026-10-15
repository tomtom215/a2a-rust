<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# ITK current-mount traversal agent

This directory doubles as the **a2a-itk "current" agent** for this
repository. The upstream Integration Testing Kit
([a2aproject/a2a-itk](https://github.com/a2aproject/a2a-itk)) mounts an SDK
repo at `agents/repo/` and runs whatever agent it finds under
`<repo>/itk/`. Its `test_suite/current.py` detects a `Cargo.toml` here,
runs `cargo build --release`, and launches the resulting `itk-*` binary
with `--httpPort N --grpcPort N`. `run_itk.sh` in this directory drives that
mount through the ITK's shared driver; see `README.md` ("Upstream ITK
nightly").

`src/main.rs` implements the ITK's multi-hop **traversal instruction**
protocol on top of `a2a-protocol-server` and `a2a-protocol-client`:

- Incoming messages carry a serialized `itk.Instruction`
  (`protos/instruction.proto`, vendored from upstream: identical from the
  `syntax` line on, below a two-line provenance comment) — as a raw
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
   cd a2a-itk && uv sync && uv run python run_tests.py --sdks current,python_v10
   ```

   In CI this runs as the **`workflow_dispatch`-only** (manual)
   `itk-current-mount` job, not as a PR gate: the upstream a2a-itk's
   `uv.lock` pins several baseline dependencies (e.g. `aiosqlite` via
   `a2a-sdk[sqlite]`) to a **private** Google Artifact Registry that
   returns `401` to public runners, so the baseline cluster cannot be
   provisioned on GitHub-hosted CI. A maintainer with registry access (or
   a future public ITK lockfile) can trigger it from the Actions tab. The
   in-repo self-test (1) is the authoritative automated gate.

## ACTS system under test

The same binary is also the system under test for the A2A project's
conformance suite (ACTS, in a2a-itk). `run_itk.sh` runs ACTS instead of
the traversal set when `ITK_ACTS_RUN=1`.

- `src/acts.rs` implements the ACTS `tck-*` behaviours: a plain-text
  message beginning with one of those prefixes produces the scripted
  outcome the suite expects. `../acts/sut-behaviors.yaml` (at the
  repository root) declares the same prefixes to the runner, and a unit
  test in `src/acts.rs` checks that the two lists match.
- `src/acts_client_parse.rs` implements `tck-client-parse`, which runs a
  wire payload through this SDK's real client against a loopback fixture.
- `src/acts_modes.rs` covers the extra passes the runner starts:
  `ITK_ACTS_REDUCED_CAPABILITIES` (advertise no optional capability) and
  `ITK_ACTS_AUTH` (declare and enforce a bearer scheme), plus the guard on
  the extended agent card in every pass.

[`acts.yml`](../.github/workflows/acts.yml) runs ACTS on JSON-RPC, gRPC
and HTTP+JSON at a pinned a2a-itk revision as a merge gate, and
`release.yml` calls it for the tagged commit. `itk-nightly.yml` runs it
nightly without the gate. Reports from the pinned revision are kept under
`../acts/reports/`.
