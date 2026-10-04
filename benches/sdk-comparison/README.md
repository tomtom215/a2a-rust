<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# SDK comparison harness

Reproduces [`docs/sdk-comparison-2026-10-02.md`](../../docs/sdk-comparison-2026-10-02.md).
`results/` holds the raw output of the run that report quotes. Re-running
writes new files rather than editing those, so the two runs can be diffed.

```text
harness/agent-a2a-rs     the benchmark agent on a2a-rs (=0.4.1 / =0.5.1 / client =0.2.7)
harness/agent-a2a-rust   the same agent on a2a-rust (a2a-protocol-sdk =0.14.1)
harness/loadgen          SDK-agnostic closed-loop JSON-RPC load generator
harness/shared/llm.rs    the model client both agents compile in (`--features llm`)
scripts/                 the procedures; each one says what it measures in its header
results/                 the 2026-10-02 run
```

Each harness crate is standalone, with an empty `[workspace]` and its own
`Cargo.lock`. That way neither SDK's dependency features unify with the
other's, and the lockfile records exactly what was measured. Build with
`--locked`.

## Setup

```bash
export WORK=$HOME/sdk-comparison-work     # scratch directory; nothing below writes into the repo
H=$PWD/harness
for a in agent-a2a-rs agent-a2a-rust loadgen; do (cd $H/$a && cargo metadata --locked >/dev/null); done

(cd $H/agent-a2a-rs   && CARGO_TARGET_DIR=$WORK/target-rs       cargo build --locked --release)
(cd $H/agent-a2a-rust && CARGO_TARGET_DIR=$WORK/target-rust     cargo build --locked --release)
(cd $H/loadgen        && CARGO_TARGET_DIR=$WORK/target-lg       cargo build --locked --release)
(cd $H/agent-a2a-rs   && CARGO_TARGET_DIR=$WORK/target-rs-llm   cargo build --locked --release --features llm)
(cd $H/agent-a2a-rust && CARGO_TARGET_DIR=$WORK/target-rust-llm cargo build --locked --release --features llm)
(cd $H/agent-a2a-rust && CARGO_TARGET_DIR=$WORK/target-rust-feat cargo build --locked --release --features sqlite)
```

Agent switches, all environment variables:

| Variable | Agent | Effect |
|---|---|---|
| `PORT` | both | JSON-RPC port. a2a-rust serves REST on `PORT+1000`; a2a-rs serves REST under `/rest`. |
| `NODELAY=1` | a2a-rs | Sets `TCP_NODELAY` on accepted sockets. Without it, the setup matches a2a-rs's own `helloworld` example. |
| `TENANT_STORE=1` | a2a-rust | Uses `TenantAwareInMemoryTaskStore` and `TenantAwareInMemoryPushConfigStore`. |
| `SQLITE_URL` | a2a-rust (`--features sqlite`) | Uses `SqliteTaskStore`. |
| `AUTH_TOKEN` | a2a-rust | Requires `Authorization: Bearer <token>`. |
| `NO_TTL_SWEEP=1` | a2a-rust | Sets `eviction_interval = 0`. Attribution probe only. |
| `LLM_URL` | both (`--features llm`) | OpenAI-compatible chat endpoint; defaults to `127.0.0.1:11434`. |

## The procedures

| What | Command | Report section |
|---|---|---|
| Echo throughput and latency matrix | `scripts/run_perf.sh`, then `python3 scripts/summarize.py $WORK/bench/perf/perf.jsonl` | §4.1 |
| RSS under 60 s of load | `scripts/run_mem.sh` | §4.3 |
| Tenant isolation | `python3 scripts/tenant_probe.py http://127.0.0.1:<port> <label>`, against each agent | §5.1 |
| Persistence and auth | `B=$WORK/target-rust-feat/release/agent-a2a-rust scripts/feature_probe.sh` | §5.1 |
| Live model | start `llama-server` (below) and both `--features llm` agents on ports 3101 and 3102, then `python3 scripts/llm_e2e.py 4` | §4.4 |
| Client × server interop | `target-*-llm/release/client http://127.0.0.1:<port> "llm:<prompt>"`, all four pairings | §3.3 |
| ACTS / ITK | `ITK_CHECKOUT=<a2a-itk @ b57c5332> scripts/run_acts.sh <sdk checkout> <a2a-rs\|a2a-rust>`; `run_itk.sh` takes the same arguments | §3.1, §3.2 |

`run_perf.sh` and `run_mem.sh` pin the server to CPUs 0–1 and the load
generator to CPUs 2–3 with `taskset`, so they need 4 cores. They expect the
binaries at the `$WORK/target-*` paths built above, and write
`$WORK/bench/perf/*.jsonl`; create that directory first. Stop anything else
CPU-heavy first, including `llama-server` and Docker builds. The 2026-10-02 run
did, and its numbers mean nothing otherwise.

**The model.** Qwen3.5-0.8B Q4_0
(`ggml-org/Qwen3.5-0.8B-GGUF`, 563,036,064 bytes, sha256
`57d1997790d1744fba5b40a7317df71ea5e2acee28c47e78f0cce39c0703f8cf`), served by
llama.cpp built from `bed0a856606ee4a24a164066f73d2379447033f5`:

```bash
taskset -c 2,3 llama-server -m Qwen3.5-0.8B-Q4_0.gguf --host 127.0.0.1 --port 11434 \
  --alias qwen --jinja -t 2 -c 4096 --parallel 1
```

`--parallel 1` matters for the text-equality check: with one slot, no batching
can perturb the logits between the direct call and the call through an SDK.

**ACTS and ITK** run the A2A project's own harness against each SDK's own
`itk/` agent, at each release commit (`32c31f69` for a2a-rs, `2be81e79` for
a2a-rust). The scripts mirror each SDK's `itk/run_itk.sh` shim, plus host
networking. They need Docker and the `itk_service` image built from a2a-itk.
Behind a TLS-re-terminating proxy that image needs the overlay from
`scripts/itk_dockerfile_overlay.py`; on a machine with direct internet access,
build the upstream `Dockerfile` instead.

## Profiling

Build with `CARGO_PROFILE_RELEASE_DEBUG=line-tables-only RUSTFLAGS="-C
force-frame-pointers=yes"` into a separate target directory. Run the agent
under 16 connections from `loadgen`, and attach
`perf record -F 999 -g -p <pid> -- sleep 8`. The report quotes
`perf report --no-children --sort dso,symbol` for self time, and
`--children` for inclusive time.
