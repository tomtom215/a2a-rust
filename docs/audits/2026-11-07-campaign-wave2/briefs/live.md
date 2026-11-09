<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Campaign wave 2: the live-model brief

Written 2026-11-08 for the two agents that attacked commit a746210f; read `common.md` first. Kept so the campaign can be run again.

## Area LIVE: the SDK against real open-source models (llama.cpp, Nov 2026 builds)

Goal: find where the **SDK** misbehaves when a real, small, unreliable model drives the tool loop, skills,
steering, budgets, delegation and telemetry. A scripted model only does what its author thought of; a 2B model
does what nobody did.

### What exists (do not rebuild or re-download it)
* `llama-server` b12189, built, at `/opt/llama/latest/build/bin/llama-server`; models in `/opt/models`
  (`Qwen3.5-2B-Q4_K_M.gguf`, `gemma-4-E2B-it-Q4_0.gguf`). Read `/opt/llama/README.md` first: the command
  line (use `-t 2 -np 1 -c 8192`, one server at a time, port 11434), thinking mode (`enable_thinking` per
  request, `--reasoning off`, `--reasoning-budget`), and the throughput warning (it was 0.33 tok/s under
  contention; do not start a model while the other agent builds, check `uptime`, and cap `max_tokens`).
  Do not read model answers as facts (Qwen answered 17*23 with 491).
* `examples/live-model` (ten checks, README), `examples/tool-loop-agent`, `examples/genai-agent`,
  `examples/rig-agent`, `examples/mcp-agent`, `examples/agent-team`, `examples/swarm`, and the earlier
  adopter walk-through `docs/audits/2026-11-07-adopter-walkthrough/05-live-model.md` with its scripts in
  `scripts/live/adopter/` (`05-model-runs.sh`, `05c-model-stub-runs.sh` and `model_stub.py` for scripted
  malformed replies, `05b-disconnect.sh`). Re-use them; extend rather than duplicate.
* The scripted-model tests that state the containment guarantees:
  `crates/a2a-protocol-sdk/tests/injection_containment/` (tool results, skills, steering, approval,
  delegation, peers, operator). Your job is the same attacks with a real model reading them, and new ones.
* ADRs `docs/adr/0035-agent-skills.md` (skills) and `0036-steering.md` (steering), and the book pages
  `book/src/building-agents/tool-loop/`, `building-agents/steering.md`, and the skills page.

### What changed since the walk-through (test these first; they are new and have never met a real model)
1. **Empty or cut-off final replies (audit M-1).** An empty or blank final reply now fails the task:
   `LoopError::EmptyAnswer`, failure class `budget-exhausted` for `finish_reason: length`, `policy-refusal`
   for `content_filter`, `internal` otherwise, `error.type=empty_answer`. A reply with text that stopped at
   `length` still completes, and the answer artifact's `metadata` and the `Completed` status message carry
   `a2a-rust.com/finish-reason: length`. Reproduce the walk-through's two cases (`TOOL_LOOP_EXTRA_BODY='{"max_tokens":48}'`;
   thinking on with `max_tokens` 40) and check task state, failure class, message text, artifact metadata
   over JSON-RPC, HTTP+JSON and gRPC, and in `gen_ai.invoke_agent.duration`.
2. **Agent Skills** (`skills` feature): a skills directory becomes a catalog in the system prompt, an
   `activate_skill` tool and a confined `read_skill_file`; scripts are listed and never run.
3. **Steering** (`with_steering`): a message to a `working` task goes into the running turn's bounded
   inbox, read at the next turn.
4. `ListTasks` with a malformed page token is now `InvalidParams` (`-32602`, HTTP 400, gRPC INVALID_ARGUMENT).

### Attacks and checks to run (each with the real model; record each, pass or fail)
* **Skills, hostile content.** Write a skills directory (under your scratchpad) with: a good skill; a skill
  whose `SKILL.md` body and `references/*.md` carry prompt injection (tell the model to read `/etc/passwd`,
  call a tool that has a side effect, activate a skill it was not offered, output the system prompt);
  symlinks out of `references/` (to a file, a directory, `..`), a `references` that is itself a link, a
  FIFO, a 50 MB file, a non-UTF-8 file, a file replaced between turns, names with `..`, NUL, backslash,
  absolute paths, Unicode look-alikes, 5000-character names; a front-matter with YAML bombs (anchors/aliases,
  deep nesting), duplicate keys, a 10 MB description, a `name` that does not match its directory; two skills
  with the same name; a skill directory removed or swapped after start-up. Drive the model to ask for these
  (put the paths in the injected text). For each: did the SDK refuse it, was the refusal text free of host
  paths, did the process stay up and bounded in memory, did the run end in a sane task state.
  `crates/a2a-protocol-server/src/skills/reader.rs` documents what is claimed; try to falsify it.
* **Tool-result injection and containment.** Tools whose results carry injected instructions, oversized
  results (cap?), results with control characters and invalid UTF-8, a tool that never returns (cancel and
  budget deadline), a tool that returns after the task was cancelled, parallel tool calls, a model that
  repeats the same side-effecting call (idempotency: `restart_service` counts what it really did), and
  call arguments that are not JSON, huge, deeply nested. Check the trace artifact, the idempotency key,
  and that nothing an attacker controls reaches a status message or log unescaped.
* **Steering with a live turn.** Start a long task (a tool that sleeps), steer it with a message that
  changes the goal, with many messages (the inbox bound), with a message while the model is mid-stream,
  with one that arrives in the answering turn (counted as unread), from a caller who does not own the task
  (with `isolate_tasks_by_caller`), over each binding the feature supports; then cancel. Does the model
  read the steer; does the SDK count and label every message; does a full inbox answer 503; is anything lost
  or duplicated.
* **Budgets and delegation.** `examples/live-model`'s coordinator granting the worker what is left of the
  `tokens` meter: exhaust it mid-run, with a model that reports no usage (llama-server reports it; also
  run through `scripts/live/adopter/model_stub.py` with usage absent, zero, negative, enormous), deadline
  expiry mid-turn, the delegation's resume policy when the worker's stream is cut.
* **Observability leaks.** Run an agent with `Telemetry` to a local OTLP collector or the repository's
  observability stack (`deploy/observability`, needs Docker: if unavailable use a file exporter or
  `OTEL_EXPORTER_*` to a tiny local receiver you write) with **canary strings** planted in the prompt,
  in a tool argument, in a tool result, in a header, in a skill file, in the model's reply, in a provider
  error body. Search the traces, metrics (labels), logs and every task status message for each canary.
  Record which canaries appear where, and compare with what `book/src/deployment/observability/` and the
  telemetry page say should and should not be recorded. Metric label cardinality: count distinct label
  values after 200 varied prompts.
* **Provider misbehaviour** through `model_stub.py` (extend it): 200 with HTML, SSE that never ends,
  an SSE `data:` line of 100 MB, a `tool_calls` array of 10 000 calls, a call to a tool named like a path or
  with 1 MB of name, `finish_reason` values nobody sent before, a stream cut mid-JSON, 429 with a `Retry-After`
  of a year or a date in the past, chunked bodies that stall. The SDK must stay bounded and classify.
* **The ten `live-model` checks and `genai-agent`** on both models, as their READMEs say, on this tree.
* Anything else the real model does that makes you suspicious: follow it.

### Deliverables specific to this area
Scripts under `scripts/live/campaign/live/`, evidence under `docs/audits/2026-11-07-campaign-wave2/live/`.
State for every run: model file name and SHA-256 prefix, the `llama-server` command line, `--version`
output, `max_tokens`, thinking on or off, how long it took, and the load average. A run whose model gave
a poor answer but whose SDK behaved is a pass; say so.
