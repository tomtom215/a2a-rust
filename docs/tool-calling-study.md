<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Tool calling with small local models — does this SDK change the result?

Measured 2026-10-20 to 2026-10-22, by `scripts/tool_calling_study/`.

**Draft.** The method below is fixed and the runs are in
`scripts/tool_calling_study/runs/`, but the study is unfinished: two of the
16 cells are incomplete (see that directory's README), and no cell has been
scored or audited. There are no results here yet, and none should be read
into the raw files before the audit.

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

The rubric's verdicts are to be audited by reading every answer before any
result is published here.

## Reproducing it

```sh
cargo build --release -p tool-loop-agent
scripts/tool_calling_study/study.sh /path/to/llama-server /path/to/models OUT_DIR target/release/tool-loop-agent
python3 -I scripts/tool_calling_study/score.py OUT_DIR/direct_*.json OUT_DIR/sdk_*.json
```

`study.sh` starts the servers two at a time and runs each server's four
cells through `drive.sh` and `cell.py`, which skip finished cells and fill
partial ones, so after an interruption it is run again unchanged. Pointing
`OUT_DIR` at `scripts/tool_calling_study/runs` finishes this study.

Run each server's cells one after another, never two at once against one
server. Each output file records the harness's SHA-256, the server's model
path, build and chat-template hash, and (SDK arm) the binary's SHA-256.
