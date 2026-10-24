<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# live-model

The SDK end to end against a real model behind an OpenAI-compatible API
(llama.cpp's `llama-server`, vLLM, Ollama). Three agents run on loopback:

* a **worker**: `ToolLoopExecutor` over the model, with `tool-loop-agent`'s
  read-only inventory (`list_services`, `service_status`) and one tool with
  a side effect (`restart_service`, which counts what it really did). It
  signs what it serves (`with_content_signer`), and is served on JSON-RPC,
  HTTP+JSON, gRPC and WebSocket at once;
* a **coordinator**: delegates each message to the worker with
  `Delegation::start_budgeted`, granting it what is left of the `tokens`
  meter it was given, and relays the worker's answer;
* an **approver**: asks the model for a one-line plan, then holds the task
  for two approvers (`ApprovalGate::with_quorum(2)`, bearer-token callers).

```bash
llama-server -m qwen3-1.7b-q4_k_m.gguf -c 8192 --port 8080 --jinja
LIVE_BASE_URL=http://127.0.0.1:8080/v1 cargo run --release -p live-model
```

It prints one line per check, then one JSON line per check, and exits
non-zero if any check failed.

| Variable | Default |
|---|---|
| `LIVE_BASE_URL` | `http://127.0.0.1:8080/v1` |
| `LIVE_MODEL` | `qwen3:1.7b` (llama-server serves its one model whatever the name) |
| `LIVE_THINKING` | unset: every request carries `"chat_template_kwargs": {"enable_thinking": false}`; `1` sends nothing extra |
| `LIVE_EXTRA_BODY` | none: a JSON object merged into every request after the line above |
| `LIVE_MODEL_TIMEOUT_SECS` | 600: how long one model turn may take |
| `OTEL_EXPORTER_OTLP_ENDPOINT` (and the other standard `OTEL_*` variables) | unset: nothing is built. Set, the run exports every binding's server and client spans and `rpc.*.call.duration`, and the tool loop's `invoke_agent`, `chat` and `execute_tool` spans and `gen_ai.*` metrics |

## The checks

| Check | Passes when | Not exercised when |
|---|---|---|
| tool loop streamed and verified, per binding (4) | the billing question ("How is the billing service doing?") streams to `COMPLETED` with a non-empty answer and a tool trace, every signed part verified against the worker's key by the client (`with_content_keys`) | the run completed but the model called no tool |
| provenance: another key's client refuses | a client holding a different key fails the same stream with `ClientError::ContentNotVerified` | — |
| idempotency: a repeated restart runs once | the model asked for `restart_service("checkout")` at least twice, the tool ran once, and each run carried a 64-hex-digit idempotency key | the model asked once |
| budget: one token is exhausted | a one-token `tokens` meter fails the task with failure class `budget-exhausted` after the first turn's reported usage | — |
| delegation under a budget | the coordinator, given a budget, completes with a relay naming the worker's task | — |
| halt cancels a working run | `halt` at the first `WORKING` ends the run `CANCELED`, a send while halted is refused, and `resume` succeeds | the run completed before the halt |
| quorum of two approvers | the task waits `INPUT_REQUIRED`; the requester's own approval is refused; after bob it still waits; after carol it completes naming both | — |

What the checks do not check:

* **The answer's correctness.** A check passes on any non-empty answer;
  whether the model got the facts right is read by hand below. These are
  SDK checks, not a model evaluation (`docs/tool-calling-study.md` is
  that).
* **Halt mid-turn.** The halt lands at the first `WORKING` event, before
  the model has answered its first turn; it shows a working run cancelled,
  not a model request aborted.
* **Why the requester's approval is refused.** Any error counts. The
  reason should be `ApprovalGate`'s refusal of a requester approving their
  own action, which `crates/a2a-protocol-server/src/approval/tests.rs`
  `who_may_approve` pins; this check does not read the error.

## Recorded runs, 2026-10-23

| | |
|---|---|
| Server | llama.cpp `llama-server` 0.7.0-dev, commit `f5027fb` (build b11836), `-c 8192 --port 8080 --jinja`, 4 slots |
| Models | Qwen3-1.7B Q4_K_M (`qwen3-1.7b-q4_k_m.gguf`, SHA-256 `d2387ca2…9bc7b5`) and Qwen3.5-2B Q4_K_M (`qwen3.5-2b-q4_k_m.gguf`, SHA-256 `57a10858…a1d8`) |
| Settings | thinking off (the default above); no sampler settings, so the server's defaults |
| Binary | `cargo build --release -p live-model`, rustc 1.99.1, this example as committed with this file |
| Host | four shared vCPUs (Intel Xeon @ 2.10GHz), no GPU |

Three runs with each model, one after the other. Every run exited 0, with
10 of 10 checks passing and none not exercised (60 of 60).

| Check | Qwen3-1.7B (s) | Qwen3.5-2B (s) |
|---|---|---|
| tool loop, JSON-RPC | 5.5–6.5 | 7.9–8.6 |
| tool loop, HTTP+JSON | 4.6–8.7 | 8.0–16.5 |
| tool loop, gRPC | 5.1–5.4 | 7.4–8.3 |
| tool loop, WebSocket | 4.9–6.3 | 8.1–9.6 |
| provenance refused | 2.6–3.9 | 2.9–3.4 |
| idempotency | 8.9–12.5 | 10.0–11.5 |
| budget exhausted | 1.1–1.2 | 1.4 |
| delegation | 3.5–5.2 | 5.3–5.7 |
| halt | 0.0 | 0.0 |
| quorum | 2.0–2.3 | 2.5–2.9 |

The answers, read one by one. There is no `billing` service: the right
answer says so and names the three services.

* **Qwen3-1.7B, billing:** 12 of 12 right. Each called
  `service_status("billing")`, read the error pointing at `list_services`,
  called it, and named `payments-api`, `checkout` and `search`.
* **Qwen3.5-2B, billing:** 2 of 12 right. In the other 10 it called
  `list_services` first, decided that billing meant `payments-api` (no
  tool said so), and reported that service's status. Where it gave the state
  (degraded) and version (4.2.1), it copied them correctly. Every conversion of the
  uptime it attempted was wrong: the tool returned 1,814,400 s, which is
  exactly 21 days. It gave 30, 50, 50.4, 505, 20.2 and 20.19 days, "240
  minutes and 40 seconds", and one "1,209,600 seconds (15 days)", and it
  once wrote the number as "181,4400".
* **Delegation:** 6 of 6 relays gave the search service's version, 1.19.3,
  correctly. Two Qwen3-1.7B relays added "running for 90 minutes (7776000
  seconds)"; 7,776,000 s is 90 days.
* **Idempotency:** in all 6 runs the model asked for the restart twice and
  the tool ran once.

An earlier set of the same runs, before the "not exercised" column existed
for the tool-loop check, had one Qwen3.5-2B run on HTTP+JSON that answered
"I don't have access to a tool…" with an empty trace and still passed. That
is why a completed run with no tool call now reports not exercised. All the
figures above come from the runs after that change.

## Reproducing

```bash
llama-server -m /opt/models/qwen3-1.7b-q4_k_m.gguf -c 8192 --port 8080 --jinja
cargo build --release -p live-model
LIVE_BASE_URL=http://127.0.0.1:8080/v1 ./target/release/live-model

llama-server -m /opt/models/qwen3.5-2b-q4_k_m.gguf -c 8192 --port 8080 --jinja
LIVE_BASE_URL=http://127.0.0.1:8080/v1 LIVE_MODEL=qwen3.5:2b ./target/release/live-model
```

Small models are not deterministic under the server's default sampler, so
the timings and wording will differ; the verdicts should not.
