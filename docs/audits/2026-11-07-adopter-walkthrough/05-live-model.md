<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# 5. A real small model

Scripts: `05-build-llama.sh`, `model_probe.py`, `05-model-runs.sh`, `05-tool-loop-page.sh`,
`05b-disconnect.sh`, `05c-model-stub-runs.sh` with `model_stub.py`. Raw output: `evidence/05-*`.
The server build, the two models and the exact command line are recorded in `/opt/llama/README.md`.

## The setup

* llama.cpp **b12189** (commit `80d2c0c2c56e`, the newest `bNNNNN` tag on 2026-11-07), built with the
  brief's cmake line, `-j2`, about 14 minutes on a shared box.
* `Qwen3.5-2B-Q4_K_M.gguf` (`unsloth/Qwen3.5-2B-GGUF`, 1,280,835,840 bytes, SHA-256 `aaf42c8b...9223`) and
  `gemma-4-E2B-it-Q4_0.gguf` (`ggml-org/gemma-4-E2B-it-GGUF`, 2,841,481,184 bytes, SHA-256 `8e30dff3...6a52`),
  each equal to the `lfs.oid` Hugging Face's tree API reports.
* `llama-server -m <gguf> --host 127.0.0.1 --port 11434 --jinja -c 8192 -np 1 -t 2`, one model at a time.
  `-t 2` because the box was at load average 15 to 23 from other agents' builds (a 4-thread first request had
  not answered after 25 minutes, with thinking on and 120 tokens; a 2-thread request with thinking off and 24 tokens
  took 20 s: not a controlled comparison).
* **Throughput is the constraint, and it was not the model's.** Decode rates the server logged ranged from
  0.3 to 7 tokens/s depending on what else the machine was doing. The examples' recorded runs (README of
  `live-model`, 2026-10-23/24) are on an idle four-core box; the times below are not comparable with them.

## Results

| Step | Qwen3.5-2B | Gemma 4 E2B |
|---|---|---|
| `tool-loop-agent`, "How is the billing service doing?" (the README's own question) | **completed**, 300 s: called `list_services`, then `service_status("payments-api")`, answered for `payments-api` | **completed**, 119 s: same two calls, same answer (`payments-api`, `degraded`, 4.2.1, 1,814,400 s) |
| a request a model should refuse (a bomb) | **completed** with a refusal as the answer (42 s) | **completed** with a refusal as the answer (86 s) |
| answer capped at 48 tokens (`TOOL_LOOP_EXTRA_BODY='{"max_tokens":48}'`) | **completed, answer cut mid-sentence** ("...millions of transactions per day"), no marker (41 s) | not run |
| thinking on (`TOOL_LOOP_THINKING=1`), output capped at 40 tokens | not run | **completed, empty answer** (42 s) |
| `genai-a2a-agent`, a plain `cargo run` (its README's default model name `qwen3.5:0.8b`, which llama-server ignores) | not run | **exit 0 in 2,275 s**: `LLM leg: EXERCISED`, then the four binding sweeps; exit 0 is the README's verdict "all 44 cells ran". The script kept only the first 3.5 KB of stdout, so the final matrix was not saved (38 `[ok]` lines are in it); the answers were not read |
| `rig-a2a-agent` default demo | not run | not run |
| `live-model` (ten checks) | not run | not run |

* **The documented behaviour holds for the README's own claim about this prompt.** `examples/live-model/README.md`
  records that Qwen3.5-2B answered the billing question correctly in 2 of 12 runs and otherwise "decided that billing
  meant `payments-api` (no tool said so)"; one run here did the same, and so did Gemma 4. The README's
  statement is about the model, not the SDK, and it is true of both models.
* **M-1 (medium, needs a decision): a truncated or empty answer is a `COMPLETED` task.** The loop keeps the reply's
  `finish_reason` for telemetry only (`tool_loop/run.rs:360`). When the model stops at the token limit
  (`finish_reason: "length"`, which a small `max_tokens`, a full context window or a reasoning model that spends
  its tokens thinking all produce) the run ends `COMPLETED` with whatever text there is, which may be half a sentence
  (Qwen above) or nothing (Gemma above, and scripted scenario 10). The task status says nothing, the answer artifact says
  nothing, and a caller cannot tell it from success. The page says "A run that ends without an answer fails the task"
  (`running.md`, "How a run fails"); an empty answer is an answer that is not there.

## Scripted malformed replies (`05c-model-stub-runs.sh`, `model_stub.py`: scripted, not a real model)

Twelve failures a small model produces only sometimes, served on loopback to `tool-loop-agent`:

| Scenario | Task state | What the SDK did |
|---|---|---|
| tool call with truncated JSON arguments | `COMPLETED` | returned `error: the arguments are not valid JSON (EOF while parsing ...)` to the model as that call's result; the model answered; the trace shows the failed call |
| call to a tool that does not exist | `COMPLETED` | returned `error: no tool named 'launch_missiles'; the tools are: list_services, service_status` |
| arguments are a JSON array | `COMPLETED` | returned `error: the arguments must be a JSON object` |
| answer cut by the token limit (`finish_reason: length`) | `COMPLETED` | **delivered the cut text with no marker (M-1)** |
| HTTP 500 whose body carries `Authorization: Bearer sk-live-...` | `FAILED` | the caller is told only `the model provider answered HTTP 500`; the body goes to the operator's log verbatim (as `running.md` says: "Those go to the operator's log") |
| HTTP 429 | `FAILED` | `the model provider answered HTTP 429`, not retried (documented) |
| HTTP 200 with a non-JSON body | `FAILED` | `the model provider's answer could not be read` |
| a 6 MiB answer | `COMPLETED` | delivered in full: the cap is 8 MiB (`DEFAULT_MAX_RESPONSE_BYTES`, documented) |
| the same tool call forever | `FAILED` | stopped after 8 turns: `the model asked for tools on all 8 turns without answering` |
| an empty reply | `COMPLETED` | **an empty answer (M-1)** |
| a tool call written as JSON text in `content` | `COMPLETED` | ran it as a call, as the page says |
| a model that never answers (`TOOL_LOOP_MODEL_TIMEOUT_SECS=8`) | `FAILED` | `the model provider did not answer within 8s` after 8 s |

Every row except the two M-1 rows is what `book/src/building-agents/tool-loop/running.md` documents.

## A client that disconnects mid-stream (`05b-disconnect.sh`, echo-agent, no model)

`a2a stream` killed with SIGKILL after its first event, three times: each time the task kept running
and was `COMPLETED` with its artifact 0.4 s later, as the book says ("the task on the server is not cancelled").
Fifty streams killed at once: the agent answered the next call and logged no warning or error. With a model behind the
agent the same holds by design (the task continues and the model keeps generating); I did not run that case.

## The tool-loop page, as a stranger builds it (`05-tool-loop-page.sh`)

The page's "A minimal agent" with its hidden `fn main` restored and only the page's dependency line:
**does not build** (four errors: `tokio` and `serde_json` unresolved, and `main` cannot be `async`; the page named
neither crate: D-15, fixed). With `tokio` and `serde_json` added it builds in 24 s and serves. Asked a question with
the page's own model URL (`http://127.0.0.1:8080/v1`, where nothing listened on this box) it answered at once with
a `FAILED` task whose message is `could not reach the model provider` and whose failure class is `transient`
(`evidence/05-tool-loop-page.log`): the right error, and, because the page installs no `tracing` subscriber, nothing
on the agent's own output says why. I did not rerun it with the port edited to this box's llama-server (the script now
does the edit; the rerun was not done).

## What did not finish

* `genai-a2a-agent` with Qwen3.5-2B, and the matrix of the Gemma run (see the row above): not saved or not run.
  Its own stderr at startup carries three `WARN`s about its own card (D-18). At 0.3 to 1 token/s it took 38 minutes.
* `rig-a2a-agent` (the binary had been deleted to make room for the observability images when the script reached it:
  exit 127), `live-model` (ten checks), and every step for Qwen3.5-2B other than the three above: not run.
  `examples/live-model/README.md` records those runs on an idle box (10 of 10 checks passing for both models on
  2026-10-23); nothing here confirms or contradicts them.
