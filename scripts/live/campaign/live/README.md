<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Live-model campaign scripts (wave 2, 2026-11-08)

The SDK against a real small model (llama.cpp `llama-server`, one model at a time) and against a scripted
one. None of this runs in CI; every script starts with a comment saying what it needs and what it proves. The
findings are in `docs/audits/2026-11-07-campaign-wave2/live/README.md`, the recorded output in its `evidence/`.

## Build

Two binaries from one out-of-workspace crate (`agent/`): `a2a-campaign-agent`, a `ToolLoopExecutor` over an
OpenAI-compatible model configured from `C_*` environment variables (its `main.rs` has the table: tools, skills,
steering, authentication, telemetry, drafts, a coordinator mode), and `a2a-campaign-client`, a command-line
client for JSON-RPC, HTTP+JSON and gRPC. From the repository root:

    cp Cargo.lock scripts/live/campaign/live/agent/Cargo.lock
    CARGO_BUILD_JOBS=2 CARGO_PROFILE_DEV_DEBUG=0 CARGO_INCREMENTAL=0 \
      cargo build --manifest-path scripts/live/campaign/live/agent/Cargo.toml --target-dir <dir>
    export CAMPAIGN_TARGET=<dir>/debug      # the drivers find both binaries here
    export CAMPAIGN_WORK=<scratch>          # logs, tool logs, generated skills (default /tmp/a2a-campaign-live)

The three example binaries the brief names come from the workspace: `cargo build -p tool-loop-agent -p live-model
-p genai-a2a-agent`.

## Model server

    llama-server -m /opt/models/<model>.gguf --host 127.0.0.1 --port 11434 --jinja -c 8192 -np 1 -t 2

(`--reasoning off` for `genai-agent`, as its README says.) `00_environment.sh` records the build, the model files
and the start of their SHA-256, the load and the toolchain.

## Drivers (python3, standard library only; run each with `python3 -I`)

| Script | Needs a model | What it does |
|---|---|---|
| `01_m1_empty_and_cutoff.py [A B C]` | yes | audit M-1: a text answer cut at `max_tokens`, the billing question cut, thinking on with 40 tokens; task state, failure class, status text, artifact metadata on JSON-RPC, HTTP+JSON and gRPC, and `gen_ai.invoke_agent.duration`'s `error.type` |
| `02_skills_hostile.py scripted` / `live` / `scopes` | no / yes / no | `skills_fixture.py` builds a hostile skills directory (links, FIFO, 50 MiB, non-UTF-8, YAML bombs, 10 MiB description, 5,000-character names, look-alikes, a directory swapped after start-up); `scripted` has `model_stub.py` issue every hostile call, `live` lets the model read the injected skill, `scopes` tries a project skill with an operator's name |
| `03_steering_live.py [jsonrpc rest midstream grpc]` | yes | steering a running task through `model_proxy.py`: goal change, inbox bound, answering turn, another caller, oversize, completed task, cancel, mid-stream |
| `05_tool_results.py scripted` / `live` / `ctrl` | no / yes / no | repeated and parallel side-effecting calls, a 20 MiB tool result, 1 MiB / 100,000-deep / non-JSON arguments, a 1 MiB tool name, 90,000 calls in one reply, a hung tool under a deadline, provider `usage` absent / zero / negative / huge / float / string; `live`: an injected page; `ctrl`: control characters in a result |
| `06_provider.py` / `06_provider.py cardinality` | no | HTML with a 200, silent and trickling streams, a 100 MB line and body, odd `finish_reason`s, cut bodies and streams, 429 with odd `Retry-After`s, a stalled chunked body, a 502 with a forged log line; then 200 replies with 200 model names, counting metric series |
| `07_observability.py scripted` / `err` | no | twelve canary strings planted in the prompt, metadata, a header, the bearer token, a tool argument and result, a skill's body and file, the reply, a provider error body and an export header; where each appears (spans, metrics, logs, stderr, the task) via `otlp_sink.py` |
| `08_budget_delegation.py`, `08b_grant_time_only.py` | yes | token budgets spent mid-run, deadlines during a model turn and a tool, a coordinator delegating with what is left, a worker killed with SIGKILL, the coordinator cancelled, a deadline-only budget |
| `09_hostile_child.py` | no | a coordinator whose child (`fake_worker.py`) reports `spent.tokens` = `u64::MAX` |
| `10_listtasks_token.py` | no | `ListTasks` with malformed page tokens on the three bindings (gRPC status read with `curl`) |
| `12_empty_after_tool.py [N] [restart\|idem]` | yes | how often a model ends a tool-using run with an empty reply, and what the failed task then carries |
| `hand_mutants.py [types\|server]` | no | the hand mutants of the four fixes, baseline first, files restored by hash |

`common.py`, `otlp_sink.py`, `model_stub.py` (scripted provider misbehaviour; extends
`scripts/live/adopter/model_stub.py`), `model_proxy.py` (a logging pass-through to the real model),
`fake_worker.py` and `skills_fixture.py` are the helpers. Output goes to `docs/audits/2026-11-07-campaign-wave2/live/evidence/`
(`CAMPAIGN_EVIDENCE` overrides it).
