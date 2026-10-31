<!--
SPDX-License-Identifier: Apache-2.0
Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
-->
# Dogfooding Against a Real Model (2026-08-27)

This page is a dated record of a run made on 2026-08-27 (it is the record in
`docs/lf-readiness-review.md`, section 4a); its figures are that day's. What
the project has recorded against a real model since is under
[Since then: live-model](#since-then-live-model).

"The tests pass" and "the example works" are different claims, and the project
made the second one in nine READMEs at the time. The three LLM-backed examples
have hermetic unit tests — a dead endpoint for `genai`, fake `CompletionModel`s
for `rig` — which is the right shape for CI and says nothing about whether the
integration actually works. The [agent-team dogfood](./dogfooding.md) exercises
the full protocol surface, but with scripted agents, not a language model.

So the six examples in the table below were run end to end on one machine, the
three LLM-backed ones against a **real** open-weights model, with no mocking and
no mechanical fallback.

## The setup

The model is pinned by size and hash, the server by commit:

| | |
|---|---|
| Model | `Qwen/Qwen3.5-0.8B`, Apache-2.0, as `ggml-org/Qwen3.5-0.8B-GGUF` Q4_0 |
| File | 563,036,064 bytes, `sha256:57d1997790d1744fba5b40a7317df71ea5e2acee28c47e78f0cce39c0703f8cf` |
| Server | `llama.cpp` built from source at `d7a2074`, `llama-server` on `127.0.0.1:11434` |

`llama-server` speaks the OpenAI-compatible API that both the `genai` and `rig`
examples target, so no example code changed to accommodate the test — the model
was simply present at the endpoint the examples already use.

The record does not give the `llama-server` flags of that day. To run the
examples against a local model now, the `genai-agent` and `incident-response`
READMEs give this (the flags are theirs, not a record of the run above):

```bash
curl -L -o model.gguf \
  'https://huggingface.co/ggml-org/Qwen3.5-0.8B-GGUF/resolve/main/Qwen3.5-0.8B-Q4_0.gguf'
llama-server -m model.gguf --port 11434 --alias qwen3.5:0.8b \
  -c 8192 --reasoning off &   # direct answers, no thinking preamble; 8K context bounds memory
cargo run -p genai-a2a-agent     # GENAI_MODEL defaults to qwen3.5:0.8b
cargo run -p incident-response   # INCIDENT_MODEL defaults to qwen3.5:0.8b
```

Both send a model name that matches no hosted provider to
`http://localhost:11434/v1/` (`src/local_model.rs`). `rig-agent` needs more:
see `examples/rig-agent/README.md` for `OPENAI_API_KEY` (any non-empty value),
`OPENAI_BASE_URL=http://127.0.0.1:11434/v1`, `RIG_MODEL`, and a server started
with `--jinja`, without which no model emits a tool call.

## The results

Five of the six run a coverage matrix — every A2A method over every binding it
supports (44 cells in each); `agent-team` reports its tests instead. "Real"
means the example reported that the model answered a genuine request with
**zero mechanical fallbacks** — the labelled degraded path the unit tests
exercise was not taken.

| example | result | LLM leg |
|---|---|---|
| `genai-agent` | 44/44 cells, exit 0 | **real** — "'qwen3.5:0.8b' answered a real request", zero fallbacks |
| `rig-agent` | 44/44 cells, exit 0 | **real** — same model (`qwen3.5:0.8b`), zero fallbacks |
| `incident-response` | 44/44 cells, all five acts, 15/15 hardening checks (16 now), exit 0 | **real** — AI-summarised runbook guidance, zero degraded output |
| `agent-team` | 102/102 results (100 tests and the 2 surface-sweep results), every feature claim `[x]`, exit 0 | n/a (scripted agents) |
| `echo-agent` | 44/44 cells, exit 0 | n/a |
| `multi-lang-team` | 44/44 cells, exit 0 | n/a |

The setup table above gives one model for all six runs, and so does the commit
that recorded them (`a76d8b0`). The current `rig-agent` README documents a
different model for its tool-calling loop, Qwen3-1.7B served with `--jinja`,
and `RIG_MODEL` now defaults to `qwen3:1.7b`.

## What this establishes that unit tests cannot

* **The fallback labels are not load-bearing in practice, and that is the
  point.** Every LLM run reported zero mechanical fallbacks, so the label paths
  the unit tests exercise are genuinely the *degraded* path — not what a reader
  with a model actually gets.
* **`multi-lang-team` told the truth about itself.** With no workers running it
  completed its own A2A surface and printed "cross-language delegation was NOT
  exercised — no worker agents were reachable" — exactly the disclosure its unit
  test asserts. The claim and the behaviour were verified independently and
  agree.
* **The examples are honest about a model that is present but small.** A
  0.8B model is not GPT-scale; the point of the run is that the *protocol
  integration* works end to end with a real completion in the loop, not that the
  model's answers are impressive.

## The gap this did not close

`incident-response`'s PostgreSQL persistence check reports `[NOT RUN]` without
`A2A_TEST_POSTGRES_URL` and correctly refuses to score itself as passing. The
run above used the in-memory and SQLite stores; the Postgres path is covered by
the crate's own integration tests, not by this end-to-end sweep.

Having a real model in the loop also made it worth attacking the running server
directly — see [Adversarial Testing](./security-testing.md), which stressed the
live `genai-agent` and found (and fixed) an SSRF-filter gap that no unit test
had caught.

## Since then: live-model

`examples/live-model` runs the SDK end to end against a real model behind an
OpenAI-compatible API (llama.cpp's `llama-server`, vLLM, Ollama). Three agents
run on loopback: a worker (`ToolLoopExecutor` over the model with a small
service-inventory toolset, signing what it serves and served on JSON-RPC,
HTTP+JSON, gRPC and WebSocket at once), a coordinator that delegates to the
worker under a token budget, and an approver that holds a task for two
approvers.

```bash
llama-server -m qwen3-1.7b-q4_k_m.gguf -c 8192 --port 8080 --jinja
LIVE_BASE_URL=http://127.0.0.1:8080/v1 cargo run --release -p live-model
```

It runs 11 checks and reports each as pass, fail or not exercised: the tool
loop streamed and verified on each of the four bindings (4), a client holding
another key failing the same stream, a repeated restart that runs once, an
exhausted one-token budget, delegation under a budget, a halt that cancels a
working run, an approval quorum of two, and each turn's text streamed as a
draft artifact before the answer. It exits non-zero if any check
failed. `LIVE_BASE_URL` defaults to `http://127.0.0.1:8080/v1` and `LIVE_MODEL`
to `qwen3:1.7b`; `LIVE_THINKING`, `LIVE_EXTRA_BODY` and
`LIVE_MODEL_TIMEOUT_SECS` (600) are the other settings.

Its README records runs on 2026-10-23 (three with each of two models,
Qwen3-1.7B Q4_K_M and Qwen3.5-2B Q4_K_M) and on 2026-10-24 (three more with
Qwen3-1.7B). Every run exited 0, with 10 of 10 checks passing and none not
exercised. On 2026-10-30, with the drafts check added, two Qwen3.5-2B runs
passed 11 of 11. These are SDK checks, not a model evaluation: they pass on any
non-empty answer, and the README reads the answers by hand. On its billing
question the Qwen3-1.7B file of 2026-10-23 was right 12 of 12 times and
Qwen3.5-2B 2 of 12; the Qwen3-1.7B file of 2026-10-24, different weights, 1 of
12.
