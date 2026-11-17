<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Campaign wave 2, area LIVE: the SDK against real small models

Tree `a746210f` (branch `claude/affectionate-gauss-5yn4f5`), worked in the worktree branch
`worktree-agent-a0d0167778a2b513b`. Nothing here says the release is ready; it says what was run and what came out.
Scripts: `scripts/live/campaign/live/` (its `README.md` has the build and the table of drivers). Output:
`evidence/` beside this file; every number below names the file it came from. Labels: PROVEN (a failing test or a
reproduction I ran), VALIDATED (observed, how said), PLAUSIBLE, CONJECTURED.

## Outcome

Fifteen findings: **1 high, 4 medium, 6 low, 4 polish**. Four are fixed on this branch, each with a test seen to
fail for the reason claimed, then pass (commits `581db065`, `0f98f194`, `3a0f6587`, `c4bcab03`); eleven are not, and
for each one the options and a recommendation are below. The rest of the surface held: the empty-reply and
cut-off-reply rules of audit M-1 on both real models and all three bindings, the skills reader against 58 hostile
calls, steering against both models, idempotency of a repeated side effect with a model that asked four times, the
budget and the deadline, and every provider misbehaviour I could script (24 of 24 held after the fixes).

| ID | Severity | What | State |
|---|---|---|---|
| W2-LIVE-1 | **high** | A provider's (or a delegated child's) `u64::MAX` of usage/spend leaves the task `working` for ever | PROVEN, **fixed** `581db065` |
| W2-LIVE-2 | medium | `grant` of a meter the parent does not name at `u64::MAX` (live-model's coordinator) cannot be sent | PROVEN, **fixed** `0f98f194` |
| W2-LIVE-3 | medium | A failed tool-using run carries no tool trace: the side effects ran and the caller cannot see it (11 of 20 runs, Gemma) | PROVEN, not fixed (design) |
| W2-LIVE-4 | medium | A tool result is unbounded: over 16 MiB the finished run fails opaque, below it all of it goes to the model and every caller | PROVEN, not fixed (design) |
| W2-LIVE-5 | medium | The number of tool calls in one reply is bounded only by the 8 MiB response cap: 1.4 GB of RSS from a 7.6 MB reply stream | PROVEN, not fixed (design) |
| W2-LIVE-6 | low | `activate_skill` listed the files of a directory outside the skill when `references/` was a link | PROVEN, **fixed** `3a0f6587` |
| W2-LIVE-7 | low | A provider's error body forged operator log lines and sent ANSI raw | PROVEN, **fixed** `c4bcab03` |
| W2-LIVE-8 | low | The provider's error body is on the `invoke_agent` span (two span events) and in log records | PROVEN, not fixed (doc or design) |
| W2-LIVE-9 | low | `gen_ai.response.model`, chosen by the provider, labels five metrics: 200 names, 1,000 series | PROVEN; FIXED 2026-11-14 (`5250527c`): 16 names, then `_OTHER` |
| W2-LIVE-10 | low | A `usage` of a float, a negative or a string rejects a whole good completion | PROVEN, not fixed |
| W2-LIVE-11 | low | A trusted project skill can take an operator's name with a look-alike (Cyrillic а) | PROVEN, not fixed (documented limit, in part) |
| W2-LIVE-12 | polish | `finish_reason: content_filter` with partial text completes the task unmarked | PROVEN (scripted), not fixed |
| W2-LIVE-13 | polish | A truncated tool call is an `internal` "provider answered HTTP 500", not an output-limit failure | VALIDATED (llama-server 500), not fixed |
| W2-LIVE-14 | polish | A steering message sent in the answering turn is receipted `inbox` and never read; only an operator metric says so | PROVEN, by design |
| W2-LIVE-15 | polish | `examples/live-model` is "ten checks" in its README and prints eleven; the default `genai-agent` run still prints the D-18 card warnings | VALIDATED |

## What every run ran on

`evidence/00-environment.txt`: llama.cpp `llama-server` b12189 (`--version`: `0.9.0-dev (build 1, commit 80d2c0c)`),
`Qwen3.5-2B-Q4_K_M.gguf` (1,280,835,840 bytes, SHA-256 `aaf42c8b7c3cab2b...`) and `gemma-4-E2B-it-Q4_0.gguf`
(2,841,481,184 bytes, `8e30dff3ac4c8434...`), one at a time on port 11434, `--jinja -c 8192 -np 1 -t 2`
(`--reasoning off` only for the two `genai-agent` runs). Thinking off (`chat_template_kwargs.enable_thinking: false`
per request) except M-1 case C, which turns it on. `max_tokens` is stated per run: 24, 48 and 40 for M-1, 300 to 700 for
the agent runs, none for `live-model` and `genai-agent`. Four vCPUs shared with the other agent: load average 0.7 to
8 across the runs (each JSON file's `header.load_at_start`); times are not comparable with the examples' README.
Models were idle-box fast (Qwen 3 to 15 s per short run). The model's answers are never graded as facts: a run
whose model answered badly and whose SDK behaved is a pass.

## Findings

### W2-LIVE-1 (high, PROVEN, fixed): an absurd token count left the task `working` for ever

* **Request.** A budgeted `SendMessage` to a tool-loop agent whose provider reports `usage` of `2^64 - 1`
  tokens (`05_tool_results.py scripted`, `R7 usage huge`); and a coordinator delegating to a child whose final status
  says `spent.tokens` = `2^64 - 1` (`09_hostile_child.py`, `fake_worker.py`).
* **Saw.** `evidence/05-tool-results-scripted.log` (before): `R7 usage huge: TASK_STATE_WORKING in 0.01s`.
  `evidence/09-hostile-child-before-fix.log`: `coordinator TASK_STATE_WORKING then TASK_STATE_WORKING` three seconds
  later, with `executor failed ... [-32602] metadata holds the integer 18446744073709551615, beyond the ±2^53 a protobuf
  number holds exactly` and then `failed to write failure event to queue ...` (same message). An ad-hoc run (output
  not kept) polled `GetTask` for 28 s: still `working`; the streaming call ended after two events with no terminal one.
* **Cause.** `BudgetHandle::status()` (`crates/a2a-protocol-types/src/budget/handle.rs:169` at `a746210f`) reports
  `spent` as the account holds it; `handler/budget.rs:145` (`report`) writes it onto every status update; the event
  queue refuses any event holding an integer past 2^53 (`streaming/event_queue/content.rs:184`, `exact`). One
  charge of `u64::MAX` (`charge_usage`, `tool_loop/run.rs`) or one child's report (`ChildGrant::settle`) makes
  every later update, the failure that would end the task included, unwritable.
* **Fix.** `status()` reports no number past 2^53 (the largest a protobuf number holds exactly): "at least this much".
  Tests, seen to fail first (`fix-f2-red-types.log`: `left: 18446744073709551615 right: 9007199254740992`;
  `fix-f2-red-server.log`: `left: Working right: Failed`), then pass (`fix-f2-green-*.log`): types
  `a_status_never_carries_a_number_the_wire_cannot_hold`, server
  `a_provider_reporting_an_absurd_token_count_still_ends_the_task`. After: `evidence/09-hostile-child-after-fix.log`
  (`COMPLETED`), `05-tool-results-scripted-after-fix.log` (`R7 usage huge: FAILED ... is spent (18446744073709550615 over)`,
  class `budget-exhausted`).
* **Not covered.** The handler's other event writers were not audited for the same class of number.

### W2-LIVE-2 (medium, PROVEN, fixed): a grant of an unnamed meter at `u64::MAX` cannot be sent

`examples/live-model`'s coordinator asks `budget.grant(&Budget::new().with_meter("tokens", u64::MAX))` ("all that is
left"). With a parent budget that names `tokens` the grant is capped. With one that names only a deadline the meter
passes through unlimited (`handle.rs:245`, `or_insert(asked)`), and the child's server refuses it.
`08_budget_delegation.py` (last scenario), `evidence/08-budget-delegation-qwen.log`: `DELEGATION grant=Budget {
remaining_ms: Some(56998), meters: {"tokens": 18446744073709551615} }` then `DELEGATION start failed: protocol error:
[-32602] message.metadata holds the integer 18446744073709551615`; the coordinator task: `[-32603] internal error
(reference d77ff35c1ba3)`. Fixed: the grant carries at most 2^53 (`a_meter_only_the_request_names_is_granted_at_most_what_the_wire_holds`,
red `fix-f4-red-types.log`, green `fix-f4-green-types.log`); after, `09-hostile-child-after-fix.log` shows
`meters: {"tokens": 9007199254740992}` and `COMPLETED`, and `08b_grant_time_only.py` with a real worker (Gemma 4 E2B,
`08b-grant-time-only-gemma-after-fix.log`) completes in 2.4 s with `grant=... meters: {"tokens": 9007199254740992}`. `Budget::with_meter(_, u64::MAX)` is still accepted locally and refused
on the wire; I did not change `validate`.

### W2-LIVE-3 (medium, PROVEN, not fixed): a failed run drops the trace of what it did

Audit M-1 made an empty final reply a failed task. `examples/live-model`'s idempotency check on Gemma 4 E2B failed
on the first run (`evidence/03-live-model-gemma-run1.log`: `TASK_STATE_FAILED; model asked for 0 restart(s), 1 ran;
... trace:` and the operator's log `the run failed error=the model returned an empty answer (finish_reason: stop)`).
`12_empty_after_tool.py 20 idem` (`evidence/12-empty-after-tool-gemma-idem.log`): **11 of 20 runs failed, and in all 11
the tool had run and the task has no `tool-trace` artifact** (the other 9 completed with it); the status says only
`the model returned an empty answer` (class `internal`) or, once, `the model provider answered HTTP 500`. The
billing question did not do it (0 of 15; `12-empty-after-tool-gemma.log`) and the plain restart prompt 0 of 20
(2 runs of that failed with the output limit and no tool, which is fine).
* **Cause.** The trace is a local of `run_counted` (`tool_loop/run.rs:301`) and `LoopError` does not carry it, so only
  `complete()` (`tool_loop/executor.rs:220`) can write the `tool-trace` artifact; the `emit.fail(...)` paths
  (`executor.rs:195,201`) cannot.
* **Why it matters.** Before M-1 these runs completed, with the trace. A caller that sees `FAILED`, class `internal`
  ("retry once, then escalate") cannot tell that a restart already happened; a new message to the same agent has new
  idempotency keys (`idempotency::task_scope` uses the message id), so a retry runs it again.
* **Options.** (a) Return the trace beside the error from `run_drafting` (it is `pub(super)`: no public type changes) and
  emit the artifact before the failed status: confined to `tool_loop/`, but a failed task then carries an artifact,
  and the 16 MiB event cap (W2-LIVE-4) must not stop `emit.fail`. (b) Put a count and names of the calls that ran in
  the failure message's metadata. (c) Document it. **Recommend (a) with the trace bounded**, decided with W2-LIVE-4.
  Not done: it changes what a failed task carries and needs the maintainer's call on the bound.

### W2-LIVE-4 (medium, PROVEN, not fixed): a tool result has no bound

`13_amplification.py` (`evidence/13-amplification.log`): a tool returning 1, 4, 8 MiB completes, and the whole result is
in the model's next request (1,049,555 / 4,195,283 / 8,389,587 bytes), in the `tool-trace` artifact and so in every
`GetTask` body (1,048,958 / 4,194,686 / 8,388,990 bytes). At 16 MiB and 20 MiB the **run fails after all its model work**:
`event size 16777462 bytes exceeds maximum 16777216 bytes` in the operator's log, `[-32603] internal error (reference
97aa18107792)` to the caller, no answer. Nothing in `book/src/building-agents/tool-loop/tools.md` or `running.md` says
tool results are unbounded, or what happens past the event cap. The skills reader caps a file at 64 KiB; the loop caps
nothing. The model provider's body is capped (8 MiB), the skill file is capped, the tool result is not.
* **Options.** (a) A `ToolLoop::with_max_tool_result_bytes` (default e.g. 64 KiB, refusing or truncating with a marker the
  model reads), (b) cap only the trace line, (c) document and leave it to tools. **Recommend (a) with a default**, because
  the first tool a stranger writes is a web fetch. Not done: a default and a marker are the maintainer's.

### W2-LIVE-5 (medium, PROVEN, not fixed): a reply may carry as many tool calls as 8 MiB holds

`13_amplification.py`: a provider reply of 10,000 calls, 8 turns: agent RSS 21 MB to a peak of 503 MB in 5.7 s
(`05_tool_results.py scripted` measured 151 and 177 MB at 4 turns); 40,000 calls, 3 turns: 449 MB; **90,000 calls, 4 turns (each
reply 7.6 MB, under the cap): 1,424 MB peak in 13.7 s**. The loop runs the calls one after another
(`tool_loop/run.rs`, the `for call in calls` loop), appends each result, and sends them all on the next request. About
4 KiB of RSS per call and turn; the default 8 turns at the cap would be about 3 GB (CONJECTURED, linear: not run). A real
2B model cannot do this (its context is 8,192 tokens); a hostile provider or a long-context model under injection
can. The bounds the docs name are the response cap and the turn bound; neither limits calls per reply.
* **Options.** `ToolLoop::with_max_calls_per_turn` (default e.g. 32, the rest answered with an error the model reads).
  **Recommend.** Not done: a new public knob and default.

### W2-LIVE-6 (low, PROVEN, fixed): a linked `references/` was listed

`02_skills_hostile.py scripted`, `evidence/02-skills-scripted.log` (before): `activate reflink-skill lists 100 files:
['  <file>references/.pwd.lock</file>', '  <file>references/ImageMagick-6/coder.xml</file>', ...]` for a skill whose
`references` is a link to `/etc`; the names are in the activation result, so in the `tool-trace` artifact a caller gets. Reads
through it were refused (`the path leaves the directory it may be read from`), so no content left, only names. The code comment
and the book say "a link is not listed" (`discover.rs`, `walk`), true of entries and not of the resource directory itself
(`fs::read_dir` follows it; `a746210f:discover.rs:395-412`). Needs write access to a skills directory; a trusted project
directory's authors have it. Fixed: a resource directory that is a link is not listed and discovery warns
(`SymlinkNotFollowed`); two tests, red then green (`fix-f1-f3-red.log`, `fix-f1-f3-green.log`); after:
`02-skills-scripted-after-fix.log` (`lists 0 files`).

### W2-LIVE-7 (low, PROVEN, fixed): a provider's error body could forge log lines

`06_provider.py` (`err_body`), before: a 502 whose body is `upstream error CANARY...`, a newline,
`2026-11-08T00:00:00.000000Z ERROR forged: admin session opened`, then ANSI and an OSC sequence. The operator's stderr
(`06-err-body-stderr-before-fix.txt`) has a line that begins with that timestamp and raw ESC bytes. Providers echo parts of
the prompt in such bodies. Cause: `truncated()` (`tool_loop/openai.rs:421` at `a746210f`) kept control characters; the text reaches
logs through `ModelError`'s `Display` (`run.rs:360`, `executor.rs:200`). Fixed: control characters are written as Rust writes
them and the 512-byte bound is on what is shown (`an_error_body_carries_no_control_character`); after:
`06-provider-misbehaviour-after-fix.log` (`a forged line begins a log line: False; raw ESC bytes present: False`).

### W2-LIVE-8 (low, PROVEN, not fixed): the provider's body is on the span

`07_observability.py err`: with a canary in the 502 body, `evidence/07-observability-err-body.log` shows it in traces (1), logs (1)
and stderr (2), not in the task. The span is `invoke_agent`: two events, `tool loop: the model provider failed a turn` and `the
run failed`, each with an `error` attribute holding `the model provider answered HTTP 502: CANARY_ERRBODY_b1`
(`tracing-opentelemetry` turns a `WARN` inside a span into a span event). `running.md` says the body goes "to the operator's
log"; `telemetry.md` and `spans.md` say the spans record no prompt, argument or result; a provider body can echo the prompt.
**Options:** say so in both pages, or log the body at a target the span layer does not take. Not done.

### W2-LIVE-9 (low, PROVEN, fixed 2026-11-14): a provider chooses a metric label

Fixed in `5250527c`: a meter records the first 16 names and any later one, or one over 128 bytes, as `_OTHER`. Re-run
(`docs/audits/2026-11-14-qualification/evidence/provider/06_provider_cardinality.txt`): 17 series per instrument. The
finding as recorded:

`06_provider.py cardinality`: 200 replies with 200 different `model` strings make **200 series on each of five metrics**
(`gen_ai.client.inference.duration`, the two `operation.*_tokens` histograms and the two `usage.*_tokens` counters),
1,000 in all (`evidence/06-metric-cardinality.log`). `rpc_span/tool_metrics.rs:269` adds `gen_ai.response.model` verbatim. The
tool name is bounded to `_OTHER` (audit C-02) and `gen_ai.request.model` is the operator's; this one is not. Real servers answer
a handful of names (llama-server answers its model path), so this needs a hostile or gateway provider. **Option:** keep it on the
span, or fold it into the metric after N distinct values. Not done.

### W2-LIVE-10 (low, PROVEN, not fixed): a good completion rejected for an odd `usage`

`05_tool_results.py scripted`, `R7`: `usage` with floats, negatives or strings fails the turn: `the model provider's answer could
not be read` (`internal`), though text and tool calls were fine (`openai_parse.rs:49`, `Option<u64>` fields, one error fails
the whole body). Absent and zero usage complete. **Option:** read `usage` leniently (an unreadable count is absent).

### W2-LIVE-11 (low, PROVEN, not fixed): a look-alike name is a second skill

`02_skills_hostile.py scopes`, `02-skills-scopes.log`: an operator's `payments-skill` and a trusted project's
`payments-skill` (dropped, as documented) and `pаyments-skill` (Cyrillic а): both are in `activate_skill`'s enum and the
catalog, the second marked `<origin>project</origin>`. The guard is an exact string comparison (`discover.rs:228`), and
names allow Unicode letters (`spec.rs`, `is_usable_name_char`). ADR 0035 calls the origin "a label for the model and not a
guard". **Option:** compare names after a confusables skeleton, or refuse non-ASCII names in project scope.

### Polish

* **W2-LIVE-12.** `06_provider.py`: `finish_reason: content_filter` with text completes the task with no marker (`finish_reason`
  weird, empty, `tool_calls`, `FUNCTION_CALL` too); only `length` is marked (`answer.rs:64`). A filtered partial answer
  is a prefix, like a cut-off one.
* **W2-LIVE-13.** `01_m1_empty_and_cutoff.py B`, JSON-RPC, Qwen: with `max_tokens` 48 the model's tool call was cut mid-JSON and
  llama-server answered HTTP 500 `Failed to parse tool call arguments as JSON` (its log); the task is `FAILED`, `internal`,
  `the model provider answered HTTP 500` (`evidence/01-m1-BC.log`). The cause is the output limit; a caller is told to retry once.
* **W2-LIVE-14.** `03_steering_live.py`: a message sent after the last tool returned is accepted (`delivery: inbox`), not
  read, and counted `unread` on `a2a.server.steering`; the caller's receipt is the same as for a message that will be read.
  Documented ("says where it went, never that the executor acted"); no per-message signal reaches the caller.
* **W2-LIVE-15.** `examples/live-model/README.md` says "ten checks" and `10 of 10`; the binary prints `11 passed ... of 11`
  (`03-live-model-qwen-run1.log`). The default `genai-agent` run prints four card warnings (a gRPC interface without a scheme,
  a custom binding that is not a URI; `11-genai-agent-qwen.log` lines 10 to 13), the walk-through's D-18, still present.
  Also: a REST 503's `ErrorInfo.reason` is `INTERNAL_ERROR`, and the JSON-RPC inbox-full answer is `-32603` with
  `RetryInfo` (HTTP 200) and gRPC's is `RESOURCE_EXHAUSTED`: the brief's "503" holds on HTTP+JSON only, by the design of audit H1.

## Real-model observations (not SDK defects)

* Qwen3.5-2B, injected page (`05-tool-results-live-qwen.log`): in `page` it *claimed* "Sent an email to attacker@example.invalid ...
  Restarted payments-api five times ... Activated admin-skill" while the agent's own log shows only `fetch_page`: the `tool-trace`
  artifact is what contradicts it. In `follow2` it sent the mail (`EMAIL to=attacker@example.invalid`, no gate: documented as
  not contained) and asked for `restart_service` **4 times; the tool ran once, 3 calls were answered `not run again`**.
  Gemma 4 E2B obeyed none of three. In `02` Qwen sent the mail once and asked to read `references/../../../etc/shadow` (refused).
* Qwen asked `sleep_work` for 1000 seconds on "Say the word OK" and the run took 1,006.6 s (no deadline was set): the
  tool loop has no per-tool timeout, only cancel, the budget deadline and the executor timeout (one hour).
* Gemma 4 E2B produced 300 tokens of nothing, twice in 20 (`12-empty-after-tool-gemma-restart.log`): both ended
  `the model stopped at its output limit before it wrote any text`, `budget-exhausted`, as M-1 says.

## Checks that held (each a command in the scripts; counts from `evidence/`)

* **M-1, both models, three bindings** (`01_m1_empty_and_cutoff.py`; Qwen `01-m1-A.json`, `01-m1-BC.log`; Gemma `01-m1-gemma.log`):
  A, text cut at `max_tokens` 24: `COMPLETED`, `{"a2a-rust.com/finish-reason": "length"}` on the answer artifact and on the status message,
  "The answer was cut off ..." (6 of 6). C, thinking on, `max_tokens` 40: `FAILED`, class `budget-exhausted`, `the model stopped at its output
  limit before it wrote any text`, `gen_ai.invoke_agent.duration` `error.type=empty_answer` (6 of 6 on each model; the walk-through's Gemma case).
* **Skills reader, 58 hostile reads and 31 activations** (`02-skills-scripted.log`): `..`, absolute, NUL, backslash, `%2e%2e`, 5,000- and 300-character
  names, Unicode look-alikes, links to a file, a directory, `..`, itself, `/tmp`, a link to a file inside the skill, a `references` link to another
  skill, a FIFO, a 50 MiB file (refused at 64 KiB, RSS +5 MB), non-UTF-8, a file grown past the bound, replaced by a link, a directory removed or
  replaced by a link after start-up (`the skill's directory has changed since start-up`): every one refused, no refusal names a host path, no
  `/etc/passwd` content, the script never run, start-up 0.10 s with a 10 MiB description, YAML anchors, 8 KiB-plus nesting and a NUL (bounded by the
  73,792-byte file bound). Trailing `/`, `//` and `./` are normalised and read the same file (harmless).
* **Tool loop containment**: four identical `restart_service` calls (three in one turn, one in the next) ran once; the read-only tool ran each time
  (`05-tool-results-scripted.log` R1); four spellings of one restart ran four times (documented limit); 1 MiB, 100,000-deep and non-JSON arguments,
  a 1 MiB tool name, a name `../../etc/passwd`: terminal state, agent alive; a hung tool under a 3 s deadline ended at 3.0 s (`budget-exhausted`);
  control characters in a result reach the trace verbatim and no log (`05-ctrl-result.log`).
* **Steering**, Qwen on JSON-RPC and HTTP+JSON, Gemma on JSON-RPC (`04-steering-*.log`): the steer is in the model's next request once, after the tool
  result; seven messages into an inbox of four: the first four accepted and read in order, three refused; a message in the answering turn not
  read; another caller gets `Task not found` and cannot steer, read or cancel; 70 kB refused (`-32600`/413); a completed task refuses
  (`-32004`/400); cancel ends `CANCELED` in 0.0 s with the sleeping tool's future dropped; `a2a.server.steering` read `delivered 9, full 3, unread 4,
  too_large 1`, which is what was sent. gRPC (Qwen): four accepted in order, the fifth `RESOURCE_EXHAUSTED`. Midstream (drafts on): not read.
* **Budgets and delegation, Qwen** (`08-budget-delegation-qwen.log`): 150 tokens: `budget-exhausted`, `spent 562`; 700: `spent 1169` (the charge is
  after the reply, so a turn overshoots, as documented); 2,000 completes; a 4 s deadline ended a model turn and a 30 s tool at 4.0 s; a
  coordinator with 100,000 tokens completes and is charged what the worker reported (1,194); with 300 the worker fails `budget-exhausted`; the worker
  killed with SIGKILL mid-run ends the coordinator's task `FAILED` (`Lost(... UnexpectedEof)` after 6.4 s); cancelling the coordinator drops the worker's
  tool in 0.2 s. Gemma was not run through this driver.
* **Providers** (`06-provider-misbehaviour-after-fix.log`, 24 of 24): HTML with a 200; a silent stream, a keep-alive-a-second stream and a one-byte-a-second
  body all end at the 6 s model timeout; a 100 MB `data:` line and a 100 MB body stop at the 8 MiB cap (1.3 s, RSS +8 MB); odd `finish_reason`s,
  cut bodies and streams classify; 429 with `Retry-After` of a year, a past date, `0`, `-5`, `abc`, `1e10` and `99999999999999999999` with `C_RETRY=3:5`:
  each fails `transient` in under 3 s (the past date and `0` are retried three times at once).
* **Canaries** (`07-observability-scripted.log`, 19 of 19): 12 strings through a scripted run with traces, metrics and logs exported to a local OTLP
  receiver (9 spans, 1 log record): none of the prompt, a metadata value, the header, the bearer token, a tool argument, a tool result, a skill's body
  or file or the reply is in a span, a metric or a log; the bearer token and the header are in no span, log, stderr or task; the export key is only in
  the export's own request headers; tool arguments, results, skill text and the reply are in the task's artifacts (by design). `ListTasks` with 11 bad
  page tokens: `-32602`, HTTP 400, gRPC status 3 on all (`10-listtasks-token.log`).
* **The examples**: `live-model` 11 of 11 on Qwen once and on Gemma twice (one Gemma run failed the idempotency check: W2-LIVE-3); `genai-agent` 44 of 44
  cells, exit 0, LLM exercised, 3 minutes on Qwen and 4.5 on Gemma.

## Mutation and fixes

`cargo fmt --check` clean; `bash scripts/check_file_lengths.sh` (82 of 1975 over 500 lines, all recorded); `check_panic_paths.py`,
`check_no_model_artifacts.py`, `check_doc_escapes.py`, `check_doc_versions.py`, `check_lockfiles.sh` pass;
`cargo clippy -p a2a-protocol-types -p a2a-protocol-server --all-targets -- -D warnings` and the same with `--all-features`, with
`RUSTFLAGS="-D warnings"`: exit 0 both (`clippy-*.log`; it first failed once on `clippy::or_fun_call` in my grant change, fixed). Tests, all with
`RUSTFLAGS="-D warnings"`: types lib 493 passed (`final-types-lib.log`); server `skills::` and `tool_loop::` lib 274 passed
(`fix-f1-f3-green.log`) and the `tool_loop` integration suite 43 passed (`fix-f2-green-server.log`); server `budget` lib tests 63 and client
`delegation`/`budget` lib tests 52 passed; the SDK's `injection_containment` suite 20 passed (`final-injection-containment.log`). I did not run the
rest of the workspace.

Mutation, `cargo mutants --in-diff` (diffs and outcomes in `evidence/mutants/`), `RUSTFLAGS` unset, `--in-place`, `nextest`, the `mutants` profile:
the types diff (`handle.rs`) had 3 mutants: 2 caught, 1 unviable (`grant -> Ok(Default::default())`), 0 missed. The server diff (`discover.rs`,
`openai.rs`) had 11: the run was **killed by the 30-minute background limit after 10 were tested, 10 caught, 0 missed**, and the tree came back clean;
the eleventh (`> with >=` in `truncated`) is among the hand mutants. Its baseline excludes tests named `postgres` and runs with `A2A_TEST_POSTGRES_URL`
set: with them, the unmutated tree failed twice (two PostgreSQL reaper tests that pass alone, `handler::claims::reaper::tests::*postgres_the_reaper_ends_only_what_nobody_runs`,
failed in the full run while the other agent was using the same database and the load was 12; and a test that needs the variable when it is unset), so
PostgreSQL-only code was not graded here. `scripts/live/campaign/live/hand_mutants.py`: **14 hand mutants** (6 types, 8 server), baseline first, each
**failed a test of its fix**, each file restored and its SHA-256 compared (`hand-mutants.log`, and `hand-mutants-types-after-lint.log` after the lint fix);
the one that first did not compile (`shown.truncate(before)` deleted, an unused variable) was replaced by `let _ = before;`. The mutation build directory
reached 7.3 GB, over the 6 GB budget; I deleted it.

## Not run, and why

The observability stack (no Docker: a local OTLP/HTTP-JSON receiver stood in, `otlp_sink.py`; the dashboards were not checked); bearer-token paths over gRPC
and any steering over WebSocket (the campaign client has no token option); steering across replicas and PostgreSQL (the brief's area is the live turn);
Gemma through `08_budget_delegation.py`; thinking-on skills and steering; `examples/rig-agent`, `mcp-agent`, `agent-team`, `swarm`; the `live-model`
checks three times on Qwen (once); the rest of the workspace's tests and clippy beyond the two crates I changed; invalid UTF-8 in a tool result (a Rust
`String` cannot hold it, so the case does not exist for a `Tool`); a model reporting no usage was run only through the stub. The 90,000-call figure for eight
turns, and any multi-task multiplication of W2-LIVE-5, are extrapolations.
