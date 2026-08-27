<!--
SPDX-License-Identifier: Apache-2.0
Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
-->
# Dogfooding Against a Real Model

"The tests pass" and "the example works" are different claims, and this project
makes the second one in nine READMEs. The three LLM-backed examples have
hermetic unit tests — a dead endpoint for `genai`, fake `CompletionModel`s for
`rig` — which is the right shape for CI and says nothing about whether the
integration actually works. The [agent-team dogfood](./dogfooding.md) exercises
the full protocol surface, but with scripted agents, not a language model.

So every runnable example was driven end to end against a **real** open-weights
model on one machine, with no mocking and no mechanical fallback.

## The setup

Reproducible by construction — the model is pinned by size and hash, the server
by commit:

| | |
|---|---|
| Model | `Qwen/Qwen3.5-0.8B`, Apache-2.0, as `ggml-org/Qwen3.5-0.8B-GGUF` Q4_0 |
| File | 563,036,064 bytes, `sha256:57d1997790d1744fba5b40a7317df71ea5e2acee28c47e78f0cce39c0703f8cf` |
| Server | `llama.cpp` built from source at `d7a2074`, `llama-server` on `127.0.0.1:11434` |

`llama-server` speaks the OpenAI-compatible API that both the `genai` and `rig`
examples target, so no example code changed to accommodate the test — the model
was simply present at the endpoint the examples already use.

## The results

Each example runs a coverage matrix — every A2A method over every binding it
supports (44 cells for the single-model examples). "Real" means the example
reported that the model answered a genuine request with **zero mechanical
fallbacks** — the labelled degraded path the unit tests exercise was not taken.

| example | result | LLM leg |
|---|---|---|
| `genai-agent` | 44/44 cells, exit 0 | **real** — "'qwen3.5:0.8b' answered a real request", zero fallbacks |
| `rig-agent` | 44/44 cells, exit 0 | **real** — same, zero fallbacks |
| `incident-response` | 44/44 cells, all five acts, 15/15 hardening checks, exit 0 | **real** — AI-summarised runbook guidance, zero degraded output |
| `agent-team` | 102/102 tests, every feature claim `[x]`, exit 0 | n/a (scripted agents) |
| `echo-agent` | 44/44 cells, exit 0 | n/a |
| `multi-lang-team` | 44/44 cells, exit 0 | n/a |

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
