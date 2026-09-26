<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Comparison harness

This harness drives the measurements in [`../README.md`](../README.md). Each
SDK gets one agent and one client driver, and every one of them is built only
against that SDK's **crates.io** release. None uses a path or git dependency.

| Package | SDK and pins |
|---|---|
| `agent-rust`, `driver-rust` | `a2a-protocol-sdk =0.14.0` (feature `grpc`) |
| `agent-rs`, `driver-rs` | `a2a-lf =0.3.1`, `a2a-server-lf =0.4.4`, `a2a-client-lf =0.2.5`, `a2a-grpc =0.3.7`, `a2a-pb =0.2.1` |
| `common/` | Code included verbatim by both sides (`#[path]`): the llama.cpp client (`llm.rs`), the push-webhook receiver (`webhook.rs`) and result reporting (`report.rs`) |

Every package has its own `[workspace]` table and its own lockfile. Each
resolves its dependencies the way a fresh consumer of that SDK would.

## What each piece measures

| Script | What it does |
|---|---|
| `run_matrix.sh <echo\|llm> <out.jsonl>` | Runs both clients against both servers over JSON-RPC, HTTP+JSON and gRPC. That is 12 cells, and each cell runs 17 named checks, C01–C17 (see `driver-*/src/main.rs`). |
| `probe.py` | 15 raw-wire spec probes (P01–P15), sent identically to both neutral agents. |
| `robust.py` | The robustness battery (R00–R08): oversize, nesting, invalid UTF-8, slowloris, idle sockets, 1,000 held streams, 20,000 tasks. Resident memory (RSS) is read from `/proc`. |
| `ttft.py N` | Time-to-first-token with a real model, measured three ways: direct from llama-server, through each SDK's server, with the order rotated and the prompt cache off. |
| `perf.sh` | The load test (oha 1.10.0 for HTTP, ghz 0.121.0 for gRPC). Servers are pinned to CPUs 0–1 and load generators to CPUs 2–3. It also reports server CPU µs per request, read from `/proc/<pid>/stat`. |
| `run_all.sh` | Runs everything above in order, plus ACTS. |

The executor behaviour contract is written at the top of both `agent-*`
mains and is the same for both:
- `wait:` text stays `WORKING` until the task is cancelled.
- echo mode sends one artifact.
- llm mode streams the model's deltas as appended chunks, with `lastChunk` set on the final one.

Both agents set the same card capabilities and input/output modes. Both turn
off only the loopback push-URL guard that each SDK ships, because the test
webhook listens on 127.0.0.1.

`NODELAY=1` is a harness-only switch that exists on both agents. It turns on
`TCP_NODELAY` on the listeners an SDK leaves at the OS default. It is off in
every headline run; §3.6 of the report explains why it exists.

## Prerequisites (versions used on 2026-09-26)

- **Rust:** 1.94.1 stable. **Python:** 3.11. **uv:** 0.8.17.
- **protoc:** the `protoc-bin-vendored-linux-x86_64` 3.2.0 binary (libprotoc 31.1), set via `PROTOC`.
- **llama.cpp:** built from `81bc6b83f` with `-DGGML_NATIVE=ON` and run as
  `llama-server -m Qwen3-0.6B-Q8_0.gguf --port 8080 -c 4096 -np 4 -t 4 --jinja`.
  The model file is `ggml-org/Qwen3-0.6B-GGUF` at revision `b5f37287`:
  804,753,632 bytes, sha256 `361cc68159042c36ebff7715dc5a2e4612153e88f3e9c9c234820849d6dc9e1d`.
- **Load generators:**
  - `oha` 1.10.0: `cargo install --locked oha --version 1.10.0`.
  - `ghz` 0.121.0, the release tarball, sha256 `9ae3b93f2c46dac9136e29e81b4a1de8d4e56f092a6fe46884a25c9c83cb2324`.
- **Checkouts under `$BENCH` (default `/opt/bench`):**
  - `a2a-itk` at `429945f`
  - `a2a-rs-acts` (a2a-rs) at `365d056`
  - `a2a-rust-acts` (this repository) at `10f3435`

None of these are vendored in this repository; install them first.

## Deep-dive additions

These scripts produce [`../deep-dive.md`](../deep-dive.md):

| Piece | What it does |
|---|---|
| `agent-*/src/deep.rs` | The extended behaviour contract, identical for both agents: `ask:`, `fail:`, `msg:`, `parts:`, `slow:`. |
| `driver-*/src/bin/deep_*.rs` | The tier-2 checks D01–D15, check for check on both sides. |
| `run_deep.sh <out.jsonl>` | Runs the tier-2 matrix: both clients × both servers × 3 bindings. |
| `capture_proxy.py` | Logging reverse proxy for JSON-RPC, REST and SSE traffic. It rewrites agent-card URLs so that clients discover the proxy. |
| `run_capture.sh <dir>` | Runs every check, for each client/server pair, through the proxy. |
| `analyze_wire.py <logs>` | Audits the captured traffic against the spec. |
| `profiling/` | Callgrind (`profile_one.sh`, `callers.py`), perf (`perf_one.sh`) and the two A/B load tests (`ab.sh`, `ab_cap.sh`). These scripts expect the `$BENCH=/opt/bench` layout and the line-table builds described in the report. |
| `futsize/{ours,theirs}` | Print handler future and `StreamResponse` sizes for each published SDK. |
| `sdk-params-matrix/` | The servers and clients for the seven-SDK `GetExtendedAgentCard` params probe. The .NET SDK was installed with Microsoft's `dotnet-install.sh`, which is not vendored here. |

Two harness-only switches exist on `agent-rust`, and both are unset in every
headline run:
- `QUEUE_CAP` sets the event-queue capacity, used for the memory
  investigation;
- `NODELAY` enables `TCP_NODELAY`, as described earlier.
