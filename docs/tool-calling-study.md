<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Tool calling with small local models — does this SDK change the result?

Measured 2026-10-20 to 2026-10-22, by `scripts/tool_calling_study/`.

Scored and audited 2026-10-23. The runs and their provenance are in
`scripts/tool_calling_study/runs/`; every number below comes from
`score.py`, `audit.json` and `compare.py` in that directory, run as
*Reproducing it* shows.

**The answer, in one paragraph.** At 30 runs a prompt per cell, no
difference between asking a model directly and asking it through this SDK
was detected in how often it answered correctly: across the 16 comparisons
the smallest two-sided Fisher p is 0.095 and every Holm-adjusted p is 1.
That is not a proof that the arms are equivalent. With 30 runs a side the
95% intervals on the difference are wide — up to ±0.2 to ±0.4 — so an
effect of that size in either direction is not excluded. The one large
difference on a secondary measure, a wrong uptime in 30 of 30 direct
answers against 7 of 30 through the SDK on Qwen3-1.7B, did not survive a
follow-up: on another host the same llama.cpp build, model file, harness
and seeds gave 3 of 30, so it belongs to the serving host, not to the SDK
(*Follow-up*, below).

## The question

The rig and SDK tool-loop examples ask a small local model two questions
about an imaginary service inventory, and small models get some answers
wrong: they report a different service as the one asked about, invent a
service name, or state a number the tool never returned. The question this
study answers is whether any of that comes from this SDK — its tool loop,
its request shaping, the A2A path in front of it — or from the model and its
serving stack. It does so by asking the same model the same questions twice:
once directly, with no code of this SDK in the path (the **direct** arm), and
once through this SDK end to end (the **SDK** arm), with the same sampler and
the same seeds.

It does not measure which model is best, and its numbers do not carry to
other prompts, tools or hardware.

## Method

### What is asked

Two prompts, the ones `examples/rig-agent/README.md` measures:

| Prompt | Kind | What a correct answer does |
|---|---|---|
| "How is the billing service doing?" | `billing` | The inventory has no billing service. Calls `list_services`, says billing is not in the inventory, and gives no service's figures as billing's. |
| "How is the checkout service doing? Give me its version." | `checkout` | Calls `service_status` with exactly `checkout` and states version `2.8.0`. |

Two tools, `list_services` (no arguments) and `service_status` (`service`,
a string), over a three-service inventory (`payments-api` degraded 4.2.1,
`checkout` healthy 2.8.0, `search` healthy 1.19.3), and one system preamble.
Tool descriptions, schemas, inventory values, error texts, the preamble and
the turn bound (6) are `examples/rig-agent`'s, copied into `run.py`
(`src/tools.rs`, `src/agent.rs`; each string compared with that source on
2026-10-20).

### The two arms

- **Direct** (`run.py`): Python's `urllib` against llama-server's
  `/v1/chat/completions`, the tool calls executed in the script.
- **SDK** (`sdk_arm.py`): `examples/tool-loop-agent`'s release binary
  (SHA-256 `01dce94e…5816f2fc7`; built from the tree of `18f9e8b4`, inferred
  from its date and not verified), per run: an A2A client sends the
  prompt as a streaming `SendMessage` to the example's own A2A server, whose
  `ToolLoopExecutor` runs `ToolLoop` over `OpenAiChatModel` against the same
  llama-server. The answer and the tool trace come back as the task's
  `answer` and `tool-trace` artifacts.

Differences between the arms that are part of the comparison, by
construction, and recorded in each output file:

| | Direct | SDK |
|---|---|---|
| Seed | per run and per turn (`seed × 8 + turn`) | per run, the same on every turn (`OpenAiChatModel` sends one fixed body) |
| Tool-call ids sent back | fixed, `call-{turn}-{i}` | the server's own |
| Turn bound | 6 (rig-agent's) | 8 (the SDK's default) |
| `service_status` result key order | `service, state, version, uptimeSeconds` (rig-agent's) | `service, state, uptimeSeconds, version` (serde_json without `preserve_order`) |
| Unknown tool name | `error: no tool named 'x'` | `error: no tool named 'x'; the tools are: …` |

### Models and server

| Label | File (`/opt/models`) | Source | SHA-256 |
|---|---|---|---|
| `qwen3.5-0.8b-q4_0` | `qwen3.5-0.8b-q4_0.gguf` | `ggml-org/Qwen3.5-0.8B-GGUF`, `Qwen3.5-0.8B-Q4_0.gguf` | `57d1997790d1744fba5b40a7317df71ea5e2acee28c47e78f0cce39c0703f8cf` |
| `qwen3.5-0.8b-q4_k_m` | `qwen3.5-0.8b-q4_k_m.gguf` | `bartowski/Qwen_Qwen3.5-0.8B-GGUF`, `Qwen_Qwen3.5-0.8B-Q4_K_M.gguf` | `fb044e93939a70469c905781334f5de1e6c8b608ced6cbc8c9249bd4127d9526` |
| `qwen3-1.7b-q4_k_m` | `qwen3-1.7b-q4_k_m.gguf` | `ggml-org/Qwen3-1.7B-GGUF`, `Qwen3-1.7B-Q4_K_M.gguf` | `d2387ca2dbfee2ffabce7120d3770dadca0b293052bc2f0e138fdc940d9bc7b5` |
| `qwen3.5-2b-q4_k_m` | `qwen3.5-2b-q4_k_m.gguf` | `bartowski/Qwen_Qwen3.5-2B-GGUF`, `Qwen_Qwen3.5-2B-Q4_K_M.gguf` | `57a1085840f497d764a7fc5d346922dbde961efb54cc792ea81d694fd846a1d8` |

The two `Q4_K_M` 0.8B and 2B files and the `Q4_0` 0.8B file embed one chat
template (the same SHA-256 from `GET /props`); Qwen3-1.7B's is its own.
`Q4_0` against `Q4_K_M` on the one 0.8B model is the study's quantization
check.

llama.cpp `8466395` (`b11751`, 2026-10-19), CPU build, one `llama-server`
per model, every one started the same way: `llama-server -m <file> --port
<p> -c 8192 -t 2 --jinja`. 4 vCPU Intel Xeon at 2.80 GHz, 15 GB, Linux
6.18; two servers ran at a time, so each had two cores.

### Sampling, thinking, and what makes a run replayable

Every request names llama.cpp's own default sampler explicitly
(`temperature 0.8, top_k 40, top_p 0.95, min_p 0.05`) and a seed, and sets
`max_tokens 4096`, `cache_prompt false`, and
`chat_template_kwargs.enable_thinking` to `false` or `true` per cell. Both
directions of the thinking switch were checked on every server: `true`
returns `reasoning_content`, `false` returns none.

A run is replayable byte for byte, measured: two passes over the same seeds,
one request at a time per server, gave **8 of 8** identical runs. What it
took, each found by a failed replay first:

- **The tool-call ids.** llama-server draws each id at random and the id is
  sent back in the next request's messages, so the same seed gave a
  different conversation from the second turn on: 1 of 8 replays identical,
  every turn equal but the ids. `run.py` replaces them by position.
- **One request at a time per server.** Two runs sent at once to one server
  are batched together, which changes the arithmetic: 0 of 8 identical.
- **The prompt cache off**, so that a turn never depends on what an earlier
  run left in a slot.

The direct arm's first six runs were also replayed under a tenfold
difference in machine load (288 s and 27 s a run): 6 of 6 identical. The SDK
arm's runs are not byte-replayable, because the server's own call ids reach
the model; its numbers are compared as rates.

### Scoring

`score.py`, whose rubric was written before any of these runs were scored,
grades each run and gives it one class: `correct`, `server_error`,
`turn_limit`, `no_tool_call`, `leaked_call` (a tool call written as the
answer's text), `unlisted_name` (a `service_status` call naming a service
not in the inventory, never recovered), `substituted` (billing: says billing
is missing, then offers another service's figures as the nearest match),
`wrong_service` (billing: another service's figures given as billing's),
`other`. For checkout it also counts answers that state an uptime that is
not checkout's 259,200 s in any unit (`uptime wrong`), beside `correct`
rather than in it, because the prompt does not ask for the uptime.
Intervals are Wilson score intervals at 95%.

The rubric's verdicts were audited by reading every answer before any
result was published here (*Audit*, below).

## Results

Audited verdicts (`score.py`'s, with `audit.json`'s 98 corrections). "SDK −
direct" is the SDK arm's correct rate minus the direct arm's, with a
Newcombe hybrid-score 95% interval; p-values are two-sided Fisher exact,
Holm-adjusted within each table's family. The first table answers the
study's question. The other two were chosen after the runs were read, so
they are exploratory and their p-values say less than they appear to.

### Correct answers (the study's question; Holm over 16)
| Model | Thinking | Prompt | Direct | SDK | SDK − direct [95% CI] | Fisher p | Holm p |
|---|---|---|---:|---:|---|---:|---:|
| qwen3.5-0.8b-q4_0 | off | billing | 5/30 | 10/30 | +0.17 [-0.05, +0.37] | 0.233 | 1 |
| qwen3.5-0.8b-q4_0 | off | checkout | 24/30 | 17/30 | -0.23 [-0.44, +0.00] | 0.0946 | 1 |
| qwen3.5-0.8b-q4_0 | on | billing | 8/30 | 6/30 | -0.07 [-0.27, +0.15] | 0.761 | 1 |
| qwen3.5-0.8b-q4_0 | on | checkout | 25/30 | 26/30 | +0.03 [-0.15, +0.22] | 1 | 1 |
| qwen3.5-0.8b-q4_k_m | off | billing | 5/30 | 8/30 | +0.10 [-0.11, +0.30] | 0.532 | 1 |
| qwen3.5-0.8b-q4_k_m | off | checkout | 22/30 | 24/30 | +0.07 [-0.15, +0.27] | 0.761 | 1 |
| qwen3.5-0.8b-q4_k_m | on | billing | 1/30 | 0/30 | -0.03 [-0.17, +0.08] | 1 | 1 |
| qwen3.5-0.8b-q4_k_m | on | checkout | 30/30 | 28/30 | -0.07 [-0.21, +0.06] | 0.492 | 1 |
| qwen3-1.7b-q4_k_m | off | billing | 30/30 | 30/30 | +0.00 [-0.11, +0.11] | 1 | 1 |
| qwen3-1.7b-q4_k_m | off | checkout | 30/30 | 30/30 | +0.00 [-0.11, +0.11] | 1 | 1 |
| qwen3-1.7b-q4_k_m | on | billing | 20/30 | 25/30 | +0.17 [-0.05, +0.37] | 0.233 | 1 |
| qwen3-1.7b-q4_k_m | on | checkout | 30/30 | 30/30 | +0.00 [-0.11, +0.11] | 1 | 1 |
| qwen3.5-2b-q4_k_m | off | billing | 0/30 | 1/30 | +0.03 [-0.08, +0.17] | 1 | 1 |
| qwen3.5-2b-q4_k_m | off | checkout | 30/30 | 30/30 | +0.00 [-0.11, +0.11] | 1 | 1 |
| qwen3.5-2b-q4_k_m | on | billing | 1/30 | 1/30 | +0.00 [-0.14, +0.14] | 1 | 1 |
| qwen3.5-2b-q4_k_m | on | checkout | 22/30 | 19/30 | -0.10 [-0.32, +0.13] | 0.58 | 1 |

### Checkout answers stating a wrong uptime (exploratory; Holm over 8)
| Model | Thinking | Prompt | Direct | SDK | SDK − direct [95% CI] | Fisher p | Holm p |
|---|---|---|---:|---:|---|---:|---:|
| qwen3.5-0.8b-q4_0 | off | checkout | 0/30 | 1/30 | +0.03 [-0.08, +0.17] | 1 | 1 |
| qwen3.5-0.8b-q4_0 | on | checkout | 3/30 | 4/30 | +0.03 [-0.14, +0.21] | 1 | 1 |
| qwen3.5-0.8b-q4_k_m | off | checkout | 1/30 | 0/30 | -0.03 [-0.17, +0.08] | 1 | 1 |
| qwen3.5-0.8b-q4_k_m | on | checkout | 0/30 | 4/30 | +0.13 [-0.01, +0.30] | 0.112 | 0.787 |
| qwen3-1.7b-q4_k_m | off | checkout | 30/30 | 7/30 | -0.77 [-0.88, -0.56] | 1.74e-10 | 1.39e-09 |
| qwen3-1.7b-q4_k_m | on | checkout | 13/30 | 8/30 | -0.17 [-0.38, +0.07] | 0.279 | 1 |
| qwen3.5-2b-q4_k_m | off | checkout | 0/30 | 2/30 | +0.07 [-0.06, +0.21] | 0.492 | 1 |
| qwen3.5-2b-q4_k_m | on | checkout | 0/30 | 1/30 | +0.03 [-0.08, +0.17] | 1 | 1 |

### Empty answers, thinking on, both prompts (exploratory; Holm over 4)
| Model | Thinking | Prompt | Direct | SDK | SDK − direct [95% CI] | Fisher p | Holm p |
|---|---|---|---:|---:|---|---:|---:|
| qwen3.5-0.8b-q4_0 | on | both | 4/60 | 6/60 | +0.03 [-0.07, +0.14] | 0.743 | 1 |
| qwen3.5-0.8b-q4_k_m | on | both | 8/60 | 1/60 | -0.12 [-0.23, -0.02] | 0.0322 | 0.0966 |
| qwen3-1.7b-q4_k_m | on | both | 0/60 | 0/60 | +0.00 [-0.06, +0.06] | 1 | 1 |
| qwen3.5-2b-q4_k_m | on | both | 17/60 | 31/60 | +0.23 [+0.06, +0.39] | 0.015 | 0.0601 |

What the tables say, and do not:

- **Correct answers.** No cell differs detectably. Where both arms are near
  a ceiling or a floor (Qwen3-1.7B thinking off, Qwen3.5-2B billing) they
  agree to within one run; elsewhere they move within each other's
  intervals.
- **Billing is the hard prompt for every model but Qwen3-1.7B.** Most
  failures, in both arms, are a model presenting `payments-api` as the
  billing service (`wrong_service`), which is what the rig-agent README
  reports.
- **Empty answers.** With thinking on, a model sometimes ends its turn with
  reasoning only: no text and no tool call. Qwen3.5-2B did this in 17 of
  60 direct runs and 31 of 60 SDK runs; Qwen3.5-0.8B (Q4_K_M) in 8 and 1,
  the other way. Neither survives correction across the four models, and
  the two go in opposite directions. The SDK passes the model's empty
  content through as an empty `answer` artifact; it neither causes nor
  hides the behaviour, which a caller can detect.
- **Uptime.** See the follow-up.

## Audit

`score.py` was written before any run was scored, and is kept as written:
its verdicts are this study's raw scoring. The audit read every answer
against the rubric's text. Four graders, each given the rubric and the
inventory but not the scorer (language-model agents run for this audit),
each read every run of four cells — all 960 — and recorded a verdict per
run. Their verdicts were diffed mechanically against the scorer's: 22
disagreed on correctness and 76 on the failure class only, none on uptime.
Every one of those 98 answers was then read again before deciding it. The
decision for each, with its reason, is in `scripts/tool_calling_study/audit.json`;
`score.py --audit audit.json` applies them and still prints its own count
beside each row.

What the audit changed:

- **21 runs the scorer failed are correct**, by the rubric's wording.
  Eleven say billing is not in the inventory and then list all three
  services' figures, each under its own name: the scorer fails any answer
  that contains `payments-api`'s figures, but the rubric fails only figures
  given *as billing's*. Eight refuse in words the scorer's `MISSING`
  pattern does not match ("I don't see a 'billing' service listed", "there
  is no specific billing service") and give no figures. Two are borderline
  and marked so in their reason: one says billing is "not explicitly
  listed" before an overview of all three, one that it cannot verify a
  name it does not recognise.
- **2 runs the scorer passed are not correct.** One offers `checkout`'s
  version and uptime as billing's entry point (the scorer's figure check
  knows only `payments-api`'s figures); one never mentions billing and says
  that `payment` is not recognised, which `MISSING` matched.
- **75 runs keep their verdict but change class.** 67 are a new class,
  `empty_answer`, for the empty final answers above: the rubric had no
  class for them, and the scorer filed them under `other` (or a class
  reached through an earlier tool call). The other 8 are billing failures
  whose class the scorer computed in a different order from the one its
  own docstring states.

The audit cannot catch an answer that the scorer and a grader both got
wrong in the same way; only a second, independent read of all 960 would.
Changing correctness on 23 of 960 runs moves no comparison across a
significance threshold (the raw scores are `score.py` without `--audit`).

## Follow-up: the uptime difference belongs to the host

On Qwen3-1.7B with thinking off, every direct-arm checkout answer stated an
uptime of 7,200 seconds ("2 hours") although the tool returned 259,200;
through the SDK 7 of 30 did. The arms differ by construction in the seed
scheme and in the key order of the tool's result (*The two arms*), so the
gap could have come from either. Measured 2026-10-23, on a different host
(4 vCPU Intel Xeon at 2.10 GHz, where the study ran on one at 2.80 GHz;
llama.cpp built with `GGML_NATIVE=ON`, `-c 8192 -t 2 --jinja`), the
direct arm's checkout prompt, seeds 2000-2029, thinking off, one request
at a time (`runs/followup/`):

| llama.cpp | Result key order | Wrong uptime | Byte-identical to the study's run |
|---|---|---:|---:|
| `8466395` (b11751, the study's) | rig-agent's | 3/30 | 0/30 |
| `f5027fb` (b11836) | rig-agent's | 3/30 | 0/30 |
| `f5027fb` (b11836) | the SDK's (serde) | 1/30 | — |

The study's own build, model file (SHA-256 checked), harness and seeds gave
3 of 30 where the study's host gave 30 of 30 (Fisher p = 9.2e-14), and
none of the 30 runs replayed byte for byte. Key order made no detectable
difference (3 against 1, p = 0.61). So the 30 of 30 was a property of that
host's arithmetic, not of anything the SDK does, and the replay this
document's method claims (8 of 8 identical) holds on one host only: CPU
features change llama.cpp's kernels and so its floating-point results. Not
run: the SDK arm on the second host.

## Reproducing it

```sh
cargo build --release -p tool-loop-agent
scripts/tool_calling_study/study.sh /path/to/llama-server /path/to/models OUT_DIR target/release/tool-loop-agent
python3 -I scripts/tool_calling_study/score.py OUT_DIR/direct_*.json OUT_DIR/sdk_*.json
# this study's audited tables:
python3 -I scripts/tool_calling_study/score.py --audit scripts/tool_calling_study/audit.json \
  scripts/tool_calling_study/runs/direct_*.json scripts/tool_calling_study/runs/sdk_*.json
python3 -I scripts/tool_calling_study/compare.py scripts/tool_calling_study/runs \
  scripts/tool_calling_study/audit.json
```

The follow-up: `run.py --prompts checkout --seed-base 2000 --n 30
--thinking off --key-order rig|serde` against one `llama-server` per build.

`study.sh` starts the servers two at a time and runs each server's four
cells through `drive.sh` and `cell.py`, which skip finished cells and fill
partial ones, so after an interruption it is run again unchanged. Pointing
`OUT_DIR` at `scripts/tool_calling_study/runs` finishes this study.

Run each server's cells one after another, never two at once against one
server. Each output file records the harness's SHA-256, the server's model
path, build and chat-template hash, and (SDK arm) the binary's SHA-256.
