<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Handoff — session state

Working state between sessions: which branches exist and why, what is in flight
outside this repository, and what the next session should pick up.

This is **not** `ROADMAP.md`. That file takes only items the repository has
committed to and refuses speculative milestones; this one records where things
stand, including decisions to *not* do something. When an item here becomes work
the repository commits to, move it there and delete it here.

Last updated 2026-09-23 — the adopter audit and phase 1 of its fixes on
`claude/pensive-allen-socw7b` (see its section below), and the branch table's
two merged rows.

This line said 2026-09-19 and named "the panic-hook fix and the type
constructors", which was two commits out of date. It is hand-maintained and
will rot again; `git log -1 --format='%ci %s' -- docs/handoff.md` is the
authority, and takes a second.

## 0.12.1 — released 2026-09-17

Two pull requests, content then release prep, in that order because
`check_provenance_manifest.py` passes only when the manifest's pinned commit
differs from the tagged commit by nothing but the manifest itself. That forces
the manifest to be the last substantive change before the tag, which forces a
separate prep commit. `ci.yml` does not run that check; only `release.yml` does
(`.github/workflows/release.yml:100`), which is why a stale manifest never
reddens a content pull request.

- **[#131](https://github.com/tomtom215/a2a-rust/pull/131)**, merged `1fa58c6`
  — eleven commits: the issue-130 fix `5ee3cfb`, the RUSTSEC-2026-0285
  remediation `eaf038c`, four documentation commits carrying six corrections
  between them, and this file with its revisions. List them with
  `git log --oneline --no-merges 1fa58c6^1..1fa58c6^2`.
- **[#132](https://github.com/tomtom215/a2a-rust/pull/132)**, merged `e057c8e`
  — the versions (`32ed409`, `ed4e1c8`) and the provenance manifest `2e9262e`,
  re-measured at `ee5b35d`.

`v0.12.1` is an annotated tag at `e057c8e`. Its release run,
[35248119785](https://github.com/tomtom215/a2a-rust/actions/runs/35248119785),
reports 13 jobs and every one green. The publish job took its crates.io
credential from the `Obtain a crates.io token via Trusted Publishing` step, not
from `CARGO_REGISTRY_TOKEN`, and all four crates were packaged, SBOM-attested
and published; the GitHub release carries the four `.crate` files and four
CycloneDX documents as assets.

Two things nearly went wrong. Both are invisible from the code, so both are
recorded here rather than left to be rediscovered.

### The eight inter-crate pins are not what `release.yml` checks

`release.yml` verifies crate versions with
`grep '^version' "$toml" | head -1` (`.github/workflows/release.yml:142`). The
inter-crate dependency pins are written as inline tables, as in
`a2a-protocol-types = { version = "0.12.1", path = "../a2a-protocol-types" }`,
so no line of any of them begins with `version` and the gate never sees one.

A stale pin therefore fails no gate and breaks no build, because a pin of
`version = "X.Y.Z"` means `^X.Y.Z`. What it does is publish, say, an sdk
`X.Y.Z+1` declaring a dependency on server `X.Y.Z`, so a consumer who bumps
only the sdk
against an existing lockfile keeps the old server and never receives the fix.
On a patch release whose entire content is a server fix, that defeats the
release.

`RELEASING.md` §1 now names all eight, gives the `git grep` that finds them,
requires both lockfiles to be refreshed, and names the two files nothing
checks at all (`ROADMAP.md`'s current-release line and the book's changelog
page). The grep returns fourteen lines; the eight pins are the ones in the four
crate manifests, and the other six are install snippets that named `"0.7"`.
Those six turned out to be a corner of a larger problem — see the section
below — because that grep is scoped to `crates`, and most of the prose a
reader actually copies from is not.

### Waiting for the benchmark bot is the whole trick

The benchmark workflow fired on the #131 merge and pushed `baedfd5`,
`chore: update benchmark results`, directly to `main`. Its release-window guard
did not apply, and correctly so: the guard's signal for "a release is in
flight" is an in-tree version with no matching tag
(`.github/workflows/benchmarks.yml`, the `Commit benchmark results to book`
step), and at that moment the tree said 0.12.0 with `v0.12.0` already tagged.

Regenerating the provenance manifest before that push landed would have pinned
it to a tree the release no longer had. That is exactly the failure that killed
the `v0.11.0` tag on 2026-08-30, which the guard was written for and which its
comment records. Waiting for the bot to settle before regenerating is the whole
trick; the guard covers the window after the *prep* merge, not the window after
the *content* merge.

## Branches

| Branch | Head | What it is |
|---|---|---|
| `claude/friendly-keller-edrezx` | deleted | The 0.12.1 content branch. Merged as `1fa58c6` via #131, then deleted. |
| `release/v0.12.1` | merged, still present | The 0.12.1 release prep. Merged as `e057c8e` via #132, and tagged. Safe to delete. |
| `claude/a2a-rig-held` | `caa8774` | Storage. The unpublished `a2a-rig` crate, one commit on top of `caac0ec`. |
| `claude/adk-rust-0.12-patch` | `6fbdd2f` | Storage. The outbound adk-rust patch as a file, one commit on top of `caac0ec`. |
| `claude/relaxed-planck-c4hsn0` | merged, still present | The 0.13.0 content branch *and* its release prep. **Merged as `707092f8` via [#137](https://github.com/tomtom215/a2a-rust/pull/137) on 2026-09-20.** Trace-context propagation, `CallContext` reachable from `RequestContext`, the executor conformance harness, the typed failure taxonomy, and the event log with SQL stores plus SSE `id:` / `Last-Event-ID` resumption. Safe to delete. |
| `claude/wizardly-tesla-0f358t` | merged, still present | Three examples: tool calling in `examples/rig-agent`, then `examples/mcp-agent` (tools over MCP) and `examples/mcp-bridge` (an A2A agent exposed *as* MCP). **Merged as `19766afb` via [#135](https://github.com/tomtom215/a2a-rust/pull/135) on 2026-09-19.** The row previously said "open … No PR opened yet"; both halves were false, which is what `git merge-base --is-ancestor origin/claude/wizardly-tesla-0f358t HEAD` answers in one command. Safe to delete. |
| `claude/prove-gates-needle` | merged, still present | The benchmark-prose prover fix — the gate matched its sentence by value rather than by shape, so it could not be made to fail — plus the panic-hook race it exposed. **Merged as `f732fe3b` via [#136](https://github.com/tomtom215/a2a-rust/pull/136) on 2026-09-19.** This branch had no row at all while its content was described further down the file. Safe to delete. |
| `claude/busy-cerf-ta682r` | merged, still present | **Merged as `7759fea` via [#140](https://github.com/tomtom215/a2a-rust/pull/140).** The swarm-scale experiment: `crates/a2a-protocol-server/tests/swarm_scale/` and `docs/swarm-scale-findings.md`. Test-only — it adds no crate code and changes none. See its section below. |
| `claude/optimistic-bell-680i9p` | merged, still present | **Merged as `391f0df` via [#138](https://github.com/tomtom215/a2a-rust/pull/138).** It began as documentation corrections on top of 0.13.0 and is now substantially code: six audit fixes and the regression tests three of them shipped without, W3C Trace Context conformance, event-log durability, `InboundTracePolicy`, and two new CI gates. See its section below. |
| `claude/pensive-allen-socw7b` | open — see note | **Destined for `main`.** The adopter audit and phase 1 of its fixes; see its section below. No head SHA, for the reason the sections below give — this file lives on the branch it would record. |

`release/v0.12.1`, `claude/wizardly-tesla-0f358t`, `claude/prove-gates-needle`
and `claude/relaxed-planck-c4hsn0` can all be deleted: their contents are on
`main`. The two **storage** branches — `claude/a2a-rig-held` and
`claude/adk-rust-0.12-patch` — are **not destined for `main`**. They exist so
work survives the session that produced it; delete either once its contents
have landed somewhere better.

**Check a row before trusting it.** Every "open" above rots the moment the pull
request merges, and two rows here said "open" for a branch already on `main`.
`git merge-base --is-ancestor origin/<branch> HEAD && echo merged` settles it
in one command, and `git log --oneline --merges | grep <pr-number>` names the
merge commit.

### `claude/relaxed-planck-c4hsn0` — 0.13.0, and two gate lessons

**Merged 2026-09-20 as `707092f8` (#137).** Written while the branch was open,
and kept because the two lessons below are about the gates, not about the
branch. The row above has its merge commit now; it had no head SHA while the
branch was open, for the reason the `wizardly-tesla` section gives — this file
lives on the branch it would record.

Two things cost a CI cycle each and will cost the next one the same unless they
are written down.

**`check_file_lengths.sh` runs inside the `Format` job.** A commit took
`crates/a2a-protocol-server/src/conformance.rs` to 501 lines — one over — and
the PR went red on a check named `Format` with `cargo fmt --check` passing
cleanly. The file was split into `conformance/{mod,report,run}.rs` rather than
added to the exemption list, which is what `CONTRIBUTING.md` asks for and what
the script's own message prefers. Before assuming a red `Format` is formatting,
read further down the job: the step order is `cargo fmt --check`, then
`check_proto_copies.sh`, then `check_file_lengths.sh`.

**The `Mutants incremental (shard N)` job logs cannot be read through the
GitHub API.** The Postgres service container's stdout is appended at the end of
the job log and fills the whole window the API returns; an 803 KB fetch of one
shard contained zero occurrences of "mutant", "MISSED" or any `##[group]`
marker. Three attempts on three different shards all came back as nothing but
`FATAL: role "root" does not exist` and checkpoint lines.

What does work:

* The `Mutation Testing (incremental)` aggregator job prints
  `Aggregated surviving mutants: N`. Trust it only once all eight shards have
  finished — while any shard is still running it sums over the artifacts
  uploaded so far and reads low. It said 7 twice on partial runs and 2 on the
  two complete ones.
* Reproducing locally is the reliable route to the *identities*. `cargo-mutants`
  27.1.0 and `cargo-nextest` (prebuilt, `https://get.nexte.st/latest/linux`, no
  build needed) with a local PostgreSQL:

  ```bash
  A2A_TEST_POSTGRES_URL=postgres://postgres:postgres@localhost:5432/postgres \
  cargo mutants --in-diff pr-src.diff --timeout 300 --jobs 2 \
    --test-tool=nextest --profile=mutants \
    -- --all-features --run-ignored all \
       -E 'not (binary(soak) or binary(soak_multi_replica))'
  ```

  where `pr-src.diff` is
  `git diff -M origin/main...HEAD -- ':(glob)crates/*/src/**/*.rs'`.
  `--shard K/8` reproduces one CI shard exactly; `--list` alone shows the
  selection without running anything, which is how to find out *which* files a
  shard holds before spending an hour on it.
* Watch the disk. `--jobs 2` builds two scratch trees under `/tmp` at roughly
  5 GB each, and a full disk kills the run rather than failing it. Deleting the
  repo's own `target/` frees more than both and costs only a rebuild, because
  `cargo-mutants` does not use it.

### `claude/wizardly-tesla-0f358t` — tool calling, and what the live run found

**Merged 2026-09-19 as `19766afb` (#135).** While it was open this section said
the row carried no head SHA deliberately, because this file lived on that
branch, so any commit recording a head invalidated the head it recorded — the
trap `dacfc88` fixed for the 0.12.1 branch, which re-forms on every branch this
file rides. That reasoning still applies to whichever branch currently carries
this file; it no longer applies here, so the row names the merge commit. Read
what landed with `git log --oneline 19766afb^1..19766afb^2`.

The repository had no tool calling anywhere: `grep -ril
'tool_call\|ToolCall\|tool_choice\|function_call'` over `examples/`, `crates/`
and `book/src/` returned zero files, and `rig-agent`'s own comment said "no
tools, no history". For an SDK whose adopters are building agents, that is a
hole in the examples rather than in the crates, and it is fixed there:
`examples/rig-agent/src/tools.rs` imports no `a2a-protocol-*` type at all.
A2A has no tool concept, so the loop sits between the executor and its model —
many model turns, one A2A task. A tool abstraction inside
`a2a-protocol-server` would make it a framework and board a model provider's
release treadmill, which is the trade
[`ROADMAP.md`](../ROADMAP.md)'s "what not to chase" already refuses.

Verified against fakes *and* against a live model, and the live run is what
makes this worth recording rather than just merging:

- **`Qwen3.5-0.8B-Q4_0` cannot call tools**, and it is the model the README
  documented for the fully-local path. It answers in prose; forced with
  `tool_choice: "required"` it still emits no call and runs to
  `finish_reason: "length"` after 7,400+ tokens. `Qwen3-1.7B-Q4_K_M` returns
  `finish_reason: "tool_calls"` on the first request. Measured 2026-09-19
  against llama.cpp `b23701f`. The example's default and the walkthrough moved
  to 1.7B; `genai-agent` and `incident-response` call no tools, so their 0.8B
  default stays correct for them, and that divergence is deliberate.
- **`--jinja` is load-bearing.** Without it `llama-server` applies no chat
  template and no model emits a tool call however capable it is. The earlier
  walkthrough did not pass it, because nothing needed it before.
- **The trace artifact caught the model inventing a number.** The README's
  first transcript is kept with the error intact: the model reports an uptime
  the tool never returned, with the real figure one line below in
  `tool-trace`. Better evidence for the artifact than any argument for it.

Gates run on the branch: workspace suite 3,347 tests over three consecutive
clean runs (3,376 after all three examples); TCK against the live agent 21/21 graded, 0 failed, 1 N/A, which is
the figure `tck.yml` gates; no-model surface sweep 44/44, exit 0; fmt, workspace
clippy, file-lengths, doc-versions, book-code, doc-escapes, block-scalars,
api-reference, sitemap `--check` and DCO all clean.

**`examples/mcp-agent`, added after the above.** The A2A project's guidance
is that the protocols compose — *"A2A handles inter-agent collaboration and
MCP handles tool integration"* — and nothing showed what that means in code.
The agent spawns an MCP server, discovers its tools with `tools/list`, and
derives its own agent-card skills from the answer; it is `rig-agent` with
only the tool source changed, which is the claim the two `agent.rs` files
exist to let a reader check by diffing. Built on `rmcp` 3.4.

Three things from building it that are cheaper to read than to rediscover:

- **`check_package_excludes.py` and `check_book_code.sh` both caught real
  registration gaps** — a new `publish = false` member missing from four
  `--exclude` lists, and an unregistered book page. Neither would have
  surfaced before the release job. Adding an example means touching
  `Cargo.toml`, `book/src/SUMMARY.md`, `book-tests/src/lib.rs`, the sitemap,
  and those four exclude sites; the gates name every one.
- **Two runs of the same demo disagreed on the tool count** — one call, then
  two, same binary and same model. The README keeps both, because it is the
  argument for `MAX_TURNS` being a bound rather than a default.
- **The session ran out of disk at 26 GB of `target/`**, 8 GB of it
  `debug/incremental`. `CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0` brings
  a full workspace test build down to roughly 3 GB and changes no outcome.
  Worth knowing before a long session in a container.

**`examples/mcp-bridge`, the reverse direction.** An A2A agent exposed as an
MCP server, so an MCP client can call it with no A2A library and no A2A
concept — a command in its server list. This is the direction with the
reach: `rmcp` has ~2,030 reverse dependencies on crates.io against
`a2a-protocol-types`' one external.

The finding: **more of A2A crosses than expected, and only because MCP's
`2026-07-28` spec added long-running tasks (SEP-2663).** An A2A task maps
onto an MCP task rather than a blocked request — the caller polls
`tasks/get`, agent status messages cross, and `tasks/cancel` reaches the
agent as `CancelTask`. Before that extension the bridge could not have been
built without misrepresenting one side. Three asymmetries stay: A2A skills
have no argument schema, A2A has no skill selector (the bridge sends a
metadata hint the agent may ignore), and `input-required` is reported rather
than bridged, because holding an A2A task id across MCP calls is real work
and a half-built version would strand tasks.

**Where it should live is still open, and deliberately so.** It was built in
`examples/` because the code is identical either way and that costs no
release commitment. The argument for publishing it — as a `bindings/` crate
or its own repo — is that a bridge speaks wire protocols, so it works against
`a2a-protocol-*`, against the official `a2a-lf`, or against a Python agent,
which makes it the one asset here that survives whatever happens to the SDK
contest. The argument against is the one this repository already applied to
`a2a-rig`: publishing later costs nothing, maintaining now costs
immediately. Decide it with a user in hand, not before.

### `claude/a2a-rig-held`

`integrations/a2a-rig` — a `rig_core::completion::CompletionModel` behind an A2A
`AgentExecutor`, structured like `bindings/a2a-protocol-slimrpc`: outside the root
workspace, own `Cargo.lock`, independently versioned.

It was built, verified, then **deliberately reverted and held unpublished**.
Verified before the revert: 7 tests and 3 doctests passing, `cargo fmt --check`
clean, `cargo clippy --all-targets -- -D warnings` clean under `clippy::pedantic`,
and `cargo package` building the packaged copy against the published 0.12.0 from
crates.io. Not re-verified since rebasing onto `caac0ec`, because that base
differs only by markdown and the crate is not a workspace member.

Held rather than shipped because publishing buys one thing — an entry in
`rig-core`'s reverse-dependency list — and costs a release per `rig-core` minor,
of which there were 12 in 195 days (one every 16.2 days). The name stays free;
publishing later costs nothing, maintaining now costs immediately.

## `claude/pensive-allen-socw7b` — the adopter audit, and phase 1

Using the server crate as the coordinator of a Go agentic application turned
up observability that was not there and defects that got past every gate. The
audit and its current status are in
[`adopter-audit-2026-09-22.md`](adopter-audit-2026-09-22.md). Phase 1 fixed
what that application could hit today: Go interop, client stream liveness,
graceful shutdown, cross-replica cancel, OAuth2 refresh. It is on this branch
with a CHANGELOG entry per change, and CI gained `go-sdk-interop`, which runs
a2a-go's client against this server and this client against an a2a-go server.

Three lessons, each of which cost something here:

* **Merging two correct fixes is a new change.** The server's new REST error
  frame and the client's new decoder each passed their own tests and together
  broke the stream-lag signal (`adce975`). The OAuth single-flight's exhaustive
  error match and the new `IncompleteStream` variant did not compile together
  (`a49a0ed`). After merging parallel work, run clippy and the tests across
  the whole workspace with all features before believing any per-branch
  result.
* **Disk is the binding constraint for parallel work here.** Four worktrees,
  each with its own target and a cargo-mutants scratch copy of about 4 GB,
  filled the allowance twice. One rustc run died with
  `IO failure on output stream: No space left on device`, which surfaces as
  exit 101 with no compile error. Build with `CARGO_INCREMENTAL=0
  CARGO_PROFILE_DEV_DEBUG=0` (CI's own setting). The target was 25 GB before;
  rebuilt that way it held 3.4 GB after a different set of builds (workspace
  clippy, the client and SDK suites, the interop binaries), so treat the ratio
  as indicative.
* **A mutant that hangs the suite is not caught.** Two tests hung rather than
  failed under mutation; both are now bounded (`9ee0ce6`, `f82983a`).
* **Per-change verification is not the gate set.** Every worker ran its
  crate's tests, clippy and mutants, and the merged branch still failed five
  of the repository's own gates under `scripts/preflight.sh --full`:
  timeout nesting, panic paths, inert bounds, book code and gate
  reachability. Fixing the first then pushed `write()` over clippy's line
  limit, because only the tests were re-run (`600e3f3`, fixed in `4a38f21`).
  Run `preflight.sh --full` on the merged tree, not a hand-picked subset.
  Its first 25 GB of target filled this environment's ~37 GB allowance
  twice. Clearing `target/debug/incremental` mid-run, which held 16 GB,
  is what let it finish.

**Verification of record, at `4a38f21`:** `scripts/preflight.sh --full` ran
71 of 71 CI gate commands; 69 passed. The two failures are this machine's,
not the branch's. The binding's SPIFFE suites stop at "spire-server and
spire-agent were not found" (CI installs SPIRE). `check_gate_reachability.py`
counts the two harness-locked agent worktrees under `.claude/worktrees/`, and
reports 0 findings in a clean clone of the same commit. `go_sdk_interop.sh`
passed inside that run.

After `4a38f21` the branch gained two code changes, each verified on its own
rather than by another full preflight: `09b2403` removed a redundant guard in
`finish_in_flight` that left two mutants alive (re-run: 0 missed; clippy and
both shutdown suites clean), and `3c1112f` fixed the `swarm_scale` fixture
card, which had not advertised streaming since `ce0d782` and so made the
replay test fail as if the event log were broken. Found by bisect; the
documented run is 13 of 13 again. Mutation testing over this branch's own
follow-up commits: 29 mutants, 11 caught, 16 unviable, 2 missed before
`09b2403`, 0 after.

**What the next session should do first:** watch this branch's pull request
until `go-sdk-interop`, the mutation gate and the rest of CI are green, and
fix what goes red. Then work from **Open work** in
[`adopter-audit-2026-09-22.md`](adopter-audit-2026-09-22.md): each item there
has evidence, file:line, a reproduction, a proposed fix and the test that
must fail first. OW3 (the PostgreSQL startup race) and OW5 (gRPC auth codes)
are the smallest self-contained ones; phase 2 (observability, OW12) is the
largest and the one the adopter who prompted the audit hit first — the
observability review above ("There is no tracing at all") is the same finding
from the other side.

## In flight outside this repository

- **adk-rust 0.12 upgrade — prepared, not submitted.** Patch and its base commit
  are on `claude/adk-rust-0.12-patch`; the issue and PR bodies are not stored here.
  Their `CONTRIBUTING.md` requires an issue before the PR, and the PR template
  requires `Fixes #N`. `adk-server` is the only external crate in the registry
  depending on any of ours.
- **rig #391 — comment drafted, not posted.** Rewritten to ask whether there is
  interest rather than to announce a crate, so it commits to nothing. It links
  `examples/rig-agent`, which is on `main` and runs in CI.
- **AutoAgents #52 — comment posted 2026-09-12.** Zero replies as of 2026-09-13.
  Nothing to do until the maintainer answers; the ask was for a layering decision
  before any code.

## The TCK figures are gated now

This was recorded here as deliberately not done, on the reasoning that gating
the example agents' TCK grades "needs a job per agent, and that is a CI-cost
decision nobody has made". **That reasoning was wrong, and the premise is worth
correcting rather than deleting**, because it is the kind of mistake that
re-forms: the agents are LLM-backed, so gating them looks like it needs an API
key or a model pull in CI, and nobody checked.

The TCK grades protocol conformance — card discovery, method routing, task
lifecycle, error envelopes, push-config CRUD — and none of that reaches the
executor's brain. All three agents are written to serve with the brain absent.
`rig-a2a-agent` defaults `OPENAI_API_KEY` to a placeholder, its own comment
saying that is "rather than making the surface impossible to start";
`genai-a2a-agent` treats an unreachable model as a failed task rather than a
failed launch; `incident-response`'s `logs` agent has no LLM by design.

Measured 2026-09-17 with no `OPENAI_API_KEY`, no `OPENAI_BASE_URL` and nothing
listening on any model port: **all three score 21/21 graded, 0 failed, 1 N/A**
(`a2a_media_type_accepted`, which §14.1.1 scopes to the REST binding) — the
same figure the pages claimed from the manual 2026-09-13 run. `rig-a2a-agent`
answered its card one second after launch. `genai-a2a-agent` passes with its
SSRF guard at the default, so the job does not set
`A2A_ALLOW_PRIVATE_WEBHOOKS`: weakening a security default to pass a
conformance run would be the wrong trade, and it is not needed.

`tck.yml`'s `tck-example-agents` job now gates all three on every push and pull
request, as a `fail-fast: false` matrix. The `incident-response` leg starts
`logs` and `runbook` before `triage`, because triage is the agent the book
quotes a grade for and it will not answer without the two it delegates to;
their ports are compiled in. The readiness-poll step is registered in
`scripts/prove_workflow_gates_fail.py` alongside its siblings.

Eight documentation sites said some version of "no CI job gates it". All eight
now name the job instead. The cost of a minor release is one extra build per
agent and no secret.

**That caveat is now closed, and it is worth recording how.** While the work
was open this paragraph read: `tck.yml` triggers only on push to `main` and
pull requests targeting `main` (`:6`-`:9`), so the job *"has not yet run on a
GitHub runner"* — everything measured had been measured on one developer
machine, and what that did not cover was the runner environment itself, a clean
build and free ports.

It has run since. The job landed on `main` in `623be9e2`, carried by
[#135](https://github.com/tomtom215/a2a-rust/pull/135) (`19766afb`,
2026-09-19), and `tck.yml` has run on every push and pull request since. The
most recent run on `main` at the time of writing is
[35504742095](https://github.com/tomtom215/a2a-rust/actions/runs/35504742095)
(push, `707092f8`, 2026-09-20, conclusion `success`), in which all three legs —
`TCK example agent (incident-response)`, `(rig-a2a-agent)` and
`(genai-a2a-agent)` — each completed `Start example agent` and
`TCK — JSON-RPC binding` green on an `ubuntu-latest` runner. Clean build and
free ports are therefore exercised, not assumed.

## Issue #130 — released in 0.12.1 and closed

[#130](https://github.com/tomtom215/a2a-rust/issues/130) (`valliscooper`,
2026-09-15): a second message on an existing context, sent without a `taskId`,
returned a *new* task already carrying the previous task's artifacts, so each
round returned the whole context's accumulated set. Confirmed, reproduced, and
fixed in `5ee3cfb`; `history` and `metadata` leaked the same way and are fixed
with it. The carry-forward itself dates from `f09c50f` (2026-06-10), which
fixed the *opposite* bug — continuations wiping accumulated state — and keyed
the new carry-forward on the context rather than the task id; it first shipped
in 0.6.0, whose notes describe it. Shipped in 0.12.1, classified in
`CHANGELOG.md` as the `STABILITY.md`
§2 specification correction, which is what made it patch-eligible rather than a
minor bump. The issue was answered and closed on 2026-09-17.

## RUSTSEC-2026-0285 — released for the workspace, still waived in the binding

The advisory (rustls below 0.23.45 accepting TLS 1.3 handshake messages at the
wrong encryption level) turned both `cargo-deny` jobs red without any change
here causing it. Fixed for the four published crates in `eaf038c` — the
workspace lockfile moves rustls 0.23.44 to 0.23.45, two lines, no manifest
change — and that fix is now released in 0.12.1.

**Before that: the binding is a blind spot for every workspace-wide check.**
It is outside the root workspace because it takes `RequestHandler` as a public
dependency, so `cargo check --workspace --all-targets --all-features`,
workspace clippy and `cargo test --workspace` all pass while it does not
compile — measured on 2026-09-20, when `EventQueueReader::read` changed shape
and only `scripts/preflight.sh --full` (which cds into the binding) caught it.
A breaking change to a public type in the server crate is not verified until
`cd bindings/a2a-protocol-slimrpc && cargo clippy --all-targets -- -D warnings`
has run.

**Still not fixed for `bindings/a2a-protocol-slimrpc`, and it cannot be from
here.** rustls 0.23.45 needs `aws-lc-rs ^1.18`; `mls-rs-crypto-awslc 0.23.0` —
the only release in the range `agntcy-slim-auth 0.15.4` admits — pins
`aws-lc-rs =1.16.2`. The binding's `deny.toml` carries a dated ignore with the
full chain. **Re-check it at the next binding release**: run
`cargo update -p rustls --precise 0.23.45` from
`bindings/a2a-protocol-slimrpc/`; when it succeeds, delete the ignore and
commit the lockfile instead. Do not let the waiver outlive the constraint.

**Probe run 2026-09-17: still blocked, and the blocker is now named
precisely.** The command fails to select `aws-lc-rs`, because rustls 0.23.45
requires `^1.18` (1.18.0 and 1.18.1 exist) while `mls-rs-crypto-awslc 0.23.0`
pins `=1.16.2`. `cargo update -p mls-rs-crypto-awslc` locks 0 packages and
`--precise 0.23.1` reports no such package, so 0.23.0 really is the only
release in the `^0.23` range that `agntcy-slim-auth 0.15.4` admits — the
constraint is exact, not merely current. `mls-rs-crypto-awslc` itself has
moved on to 0.25.0, so the half of the chain this project does not control is
already ready; what is missing is an `agntcy-slim-auth` release admitting
`^0.24` or later, and 0.15.4 is still its newest. Nothing to do here until
that ships. The binding's lockfile is on rustls 0.23.43, two patches behind the
workspace's 0.23.45, and the probe left it untouched.

## Future ideas — from a consuming agent's seat

Recorded here rather than in `ROADMAP.md` on that file's own terms: it takes
only what the repository has committed to and refuses speculative milestones,
while this file is explicitly where non-commitments live. Move an item there and
delete it here if it becomes work the project takes on.

Where these came from: a 2026-09-17 session, argued from the position of an
agent that would *consume* this SDK — delegating to other agents and being
delegated to — rather than from the maintainer's. Every claim about current
code was checked against the tree at `e057c8e`, and the check is named inline so
a later reader can re-run it rather than trust it. Line numbers are pinned to
that commit and will drift.

### The vehicle already exists

`AgentExtension` on the card
(`crates/a2a-protocol-types/src/agent_card/mod.rs:81`),
`extensions: Option<Vec<String>>` on both `Message`
(`crates/a2a-protocol-types/src/message.rs:140`) and `Artifact`
(`crates/a2a-protocol-types/src/artifact.rs:92`), and `metadata` on nearly
everything. Every item below can therefore ship as a declared, URI-identified
extension without touching spec conformance or the TCK. That is the difference
between "fork the protocol" and "publish an extension and implement it", and it
is what makes any of this tractable at all.

### Part A — reliability, ranked by what they change for a consumer

**A1. Idempotency keys on `message/send` (correctness).** The client's retry
logic is already the careful version: `is_idempotent_method`
(`crates/a2a-protocol-client/src/retry.rs:185`) treats `SendMessage` and
`SendStreamingMessage` as non-idempotent, and `safe_to_retry_non_idempotent`
(`:208`) retries only when the error proves the server rejected the request.
That is correct, and it is exactly why the gap bites: when a send fails
ambiguously — the connection drops after the bytes have gone out — the client
rightly refuses to retry, and the caller cannot tell whether a task exists.

A delegating agent retries constantly and networks fail mid-flight. Today the
only recovery is to list tasks on the context and pattern-match message content
to guess whether the send landed, which is heuristic, racy, and degrades as
concurrency rises. With a client-supplied key the server dedupes on, retry
becomes trivial and "did that actually start?" stops being a class of bug.
`Message.extensions` is the natural home if the spec has no slot of its own;
whether that is spec-clean is a question for the maintainer, not an assumption
made here.

This is the difference between a caller's retry logic being correct and being a
guess. Of everything here it is the smallest and the highest value.

**A2. A typed failure classification on `Failed` (correctness).** A failed task
today is `TaskStatus::with_timestamp(TaskState::Failed)`
(`crates/a2a-protocol-server/src/handler/messaging/execute.rs:180`) plus an
error message carried on the status message — prose. A caller deciding what to
do next is matching English.

The decisions are genuinely different. Bad input: never retry, fix the request.
Transient infrastructure: retry with backoff. Policy refusal: escalate to a
human and stop. Timeout or budget exhaustion: retry with more budget. Four
behaviours, distinguished only by a string whose wording is up to whoever wrote
the executor. An orchestrator with three delegated tasks, one of which fails,
wants to know in a single `match` whether to retry, re-plan or surface. Without
it, callers either retry what can never succeed or abandon what would have
worked on the second attempt — and both are visible as bad agent behaviour to
whoever is watching.

**A3. A queryable per-task event journal (debuggability).** Events are already
persisted — the 0.6.0 notes in `CHANGELOG.md` record a task being marked failed
"even though every event had already reached the persistence channel", which is
what makes SSE reattach work. What is missing is exposing that as an ordered,
queryable record rather than an internal mechanism.

This is more than convenience. #130 happened because a task's state is
derived — a stored snapshot folded together with deltas — and the fold was wrong
in a way nothing could observe. If a task's event sequence were the primary
record and state a fold over it, that bug is impossible by construction: in
the reporter's own terms, artifact `A1` cannot appear in task `T2`, because it
was never in `T2`'s log. When a delegated result
makes no sense, the caller asks what the agent actually emitted, in order,
instead of reasoning backwards from a final snapshot — debugging rather than
archaeology. Secondary benefit: exact resumption from an offset, rather than
reattaching and trusting the snapshot.

**A4. A conformance harness for executors (confidence).** The TCK tests
servers. Nothing tests the thing an adopter actually writes. The paths they will
get wrong are the awkward ones: cancellation arriving mid-artifact, an
`input-required` that is never answered, a client disconnecting mid-stream, an
executor that panics after emitting two of three artifacts. Point a harness at
an `AgentExecutor` and have it drive every transition the spec permits plus the
adversarial ones, the way `cargo mutants` drives tests. The alternative is each
adopter writing those cases by hand, badly, and finding the gaps in production.

### Part B — what would be an inherent edge rather than feature parity

**B1. Attenuable capability grants.** Delegation today is one-directional in
what it communicates: the card advertises what an agent can do, but nothing lets
a caller say "for this task you may read these three artifacts, spend at most N
tokens, and make no outbound network calls", and nothing lets them verify it was
honoured.

What makes it an edge rather than a feature is multi-hop delegation. A grant
passed onward should be strictly weaker than the one held — object-capability
attenuation. Get that right and agent chains are safe by construction instead of
by trust, which is currently the whole security model of every agent framework
worth naming. Rust is also the language in which enforcing a resource bound is
credible: no GC pause to blow a latency budget, predictable memory, and a
plausible path to seccomp or a sandbox underneath. The machinery is half-built:
`sign_agent_card` (`crates/a2a-protocol-types/src/signing.rs:255`) and
`verify_agent_card` (`:408`) already exist. They sign identity. This is the same
primitive pointed at authority.

**B2. Signed execution receipts.** The card is signed; the work is not. A
receipt would state: this task, these input hashes, this model, these tools
invoked, these output digests, signed by the agent that did it.

Why this project specifically: it already produces SLSA build provenance,
CycloneDX SBOMs, signed attestations and `docs/provenance-manifest.md`. No other
A2A SDK has that culture. Extending provenance from "how this crate was built"
to "how this task was executed" is a straight line from where the project
already stands, and it is a line nobody else is positioned to walk. The payoff
is handing a result to a human with the chain attached, rather than asking them
to trust a summary. In any reviewed or regulated setting that is the difference
between usable and not.

**B3. Content-addressed artifacts.** Artifacts travel by value. One flowing
through five agents is copied five times, and nothing checks it arrived intact.
Addressing them by hash gives dedup, integrity, and cheap references — and makes
an artifact something that can be pointed at across a whole context instead of
re-sent. There is some poetry in this given #130 was artifacts duplicating where
they should not: identity by content dissolves a whole class of "is this the
same artifact?" question.

**B4. Structured critique — the speculative one.** Every interaction today is
request to terminal state: `Completed` or `Failed`. The shape actually wanted
between collaborating agents is different: here is my draft, here is my
confidence, here is where I think I am wrong, push back.

`input-required` gestures at this but only one way — "I need something from you
before I continue". The missing half is the return path: a task coming back with
structured objections rather than a binary verdict. "I did this, but I disagree
with the premise, for these reasons." "Here is the result, and these two parts
are low-confidence." Today, a caller who believes a delegated result is wrong
can only send another message and hope the agent infers the objection. There is
no way to say "this specific artifact, this specific claim, here is my
counter-evidence."

Labelled honestly: this is the most speculative item here. It likely needs
design work upstream rather than a local extension, and the premise may be
wrong — critique might be a message convention that wants no protocol support at
all. It is recorded because it would most change how agents compose, where the
rest of this section only makes existing composition more reliable.

### What not to chase

Anything that turns the SDK into a runtime: agent registries, scheduling,
orchestration DSLs, a mesh. That is a different product, and pursuing it would
compromise the thing this project is actually good at — being a rigorous,
conformant, boringly correct protocol implementation. The TCK, the mutation gate
and the provenance manifest are the moat. Do not spend it on surface area.

### The through-line, and what would falsify all of it

B1 through B3 are all provenance and authority, which is the single axis on
which this project is already unusual. Any other SDK can add an idempotency key
as easily. None can credibly claim enforced resource bounds, and none has a
culture that already produces signed attestations for its own artifacts. An
inherent edge, if there is one, is there.

What would falsify this: if adopters are building single-hop integrations rather
than delegation chains, then B1 and B2 solve a problem they do not have, and A1 —
the idempotency key — is worth more than everything in Part B combined. The
argument above is reasoned from a delegating agent's seat, which may not be the
median user's. Worth checking against real adopters before building any of it.

## Prose versions are checked now

`a2a-protocol-sdk = "0.7"` means `^0.7`, which resolves to nothing in the 0.12
line, so a reader copying it gets a five-minor-old SDK and none of the fixes
since. Measured at `e057c8e`: **28 such snippets across 14 files** named 0.7,
0.8 or 0.11 — the root `README.md`, every page under `book/src/getting-started/`,
four in `crates/README.md`, the two `websocket.rs` module docs — against
exactly one that was current. `release.yml` never caught this and could not:
it checks the four crate manifests against the tag, and prose is not a
manifest.

All 28 now name `0.12`, and `scripts/check_doc_versions.py` keeps them there.
It reads the release line from the types crate's own manifest and requires
every dependency snippet in tracked prose to name `MAJOR.MINOR` — the line,
not the patch, so `^0.12` picks up the newest patch and a patch release has to
touch none of these files. Deliberately historical snippets, meaning a
migration guide's before/after pair, live in
`scripts/doc_versions_allowlist.txt` with a reason each; an entry matching no
snippet fails the gate, so the allowlist cannot rot either.

Excluded by design, and the reason matters if the scope is ever revisited:
`CHANGELOG.md` and the book's changelog page, whose content *is* what past
versions said, and `docs/`, which holds dated reviews and handoffs — records
of a moment rather than instructions to a reader. A snippet that moves into
one of those stops being checked.

Wired into `ci.yml`, `scripts/prove_gates_fail.sh` (injection: put the root
README back on the previous line), and the gate-reachability input table.
`RELEASING.md` §1 names it as a minor-release step. All three failure modes
were exercised: a stale snippet and an orphaned allowlist entry exit 1, an
entry with no reason exits 2.

## Observability review, 2026-09-19 — five maintainer findings, verified, plus a sixth

The maintainer reported five defects from recent use. All five were checked
against the tree rather than taken on report; four hold, one is right about
the symptom and wrong about the cause, and the check turned up a sixth that
matters more than the other five together. Every claim below names the file
and the mechanism so a later reader can re-run it rather than trust it.

### 1. No "just works from env" path — holds, and the failure mode is worse than ergonomics

`init_otlp_pipeline(service_name)` requires an argument, installs
process-global state last-write-wins, and **must be called from inside a
Tokio runtime or it panics** — the tonic OTLP channel is built there and
tonic spawns onto the ambient runtime. The release profile sets
`panic = "abort"`, so the penalty for getting the init order wrong is a
process abort rather than an `Err`.

That is documented, in `otel/pipeline.rs`'s `# Panics` section. It was not
documented anywhere a reader looks first — see finding 6's doc work.

### 2. `OTEL_SERVICE_NAME` ignored — holds, and the mechanism is structural

Not an oversight in reading the environment. `Resource::builder()`
(`opentelemetry_sdk-0.32.1`, `src/resource/mod.rs:62`) installs three
detectors, one of them `EnvResourceDetector`, which *does* read
`OTEL_SERVICE_NAME`. `build_pipeline` then calls
`.with_attributes([Kv::new("service.name", service_name.to_owned())])`,
which overwrites whatever the detector found.

So the required argument **structurally cannot lose to the environment**.
Reading the env harder does not fix it; the signature has to change —
`Option<&str>`, or drop the parameter and let the detector win, which is
what the OTel environment-variable specification expects. That is a
breaking change to a public function and is therefore *not* made here.

### 3. gRPC-only, not discoverable at the config layer — holds, plus a doc defect

`build_pipeline` hard-codes `MetricExporter::builder().with_tonic()`, and
the manifest compiles `opentelemetry-otlp` with
`default-features = false, features = ["grpc-tonic", "metrics"]` — the
http/protobuf exporter is not built at all.

The doc defect on top is the part worth fixing immediately, and it was:
`init_otlp_pipeline_with_endpoint`'s rustdoc claimed the other
`OTEL_EXPORTER_OTLP_*` variables "(headers, timeout, **protocol**) still
apply". Protocol cannot apply. A reader who set
`OTEL_EXPORTER_OTLP_PROTOCOL=http/protobuf` and pointed the endpoint at
`:4318` would get gRPC spoken at an HTTP port, silence, and a doc line
telling them that should have worked.

### 4. Dotted metric names — the symptom is real, the diagnosis is not, and my first counter-diagnosis was also wrong

**Dots are correct.** OTel semantic conventions name metrics
`http.server.request.duration`, and the Prometheus exporter specification
defines the `.`→`_` translation. Renaming away from dots would move *away*
from the convention.

What is actually off-convention, and is the likely source of the
mapping-guess friction:

* **The docs never said what the Prometheus names are.** This is the
  likeliest source of the reported friction, and the one thing here that was
  a pure documentation gap: nothing in the book stated that
  `a2a.server.requests` arrives as `a2a_server_requests_total`, so the only
  way to find out was to run it. `observability.md` now carries the measured
  name for all eleven instruments.
* **The units are not UCUM — but I over-charged this one, and the
  measurement is the reason.** `with_unit("request")`, `("response")`,
  `("error")`, `("queue")`, `("connection")`, `("delivery")`: six distinct
  strings across ten of the eleven instruments, every one except
  `a2a.server.latency`, whose `s` is already correct
  (`grep -rn with_unit crates/a2a-protocol-server/src/otel/`). UCUM wants
  `{request}`. I first predicted, from the Prometheus-compatibility
  specification's rule that a unit "suffix to the metric name SHOULD be
  added", that this injects a word into every name —
  `a2a_server_requests_request_total`. **That prediction was wrong.**
  Rendering the whole catalogue through `opentelemetry-prometheus` 0.32.0
  both ways produces byte-identical output, SHA-256
  `374ca05fdf3e2bc7bcca89e8c603dd442cc894b7ed1a27c39fcdb3b62b1fa59d` for
  both: the exporter only suffixes units it can translate, and `request` is
  not in its table, so it contributes nothing either way. The deviation is
  real against the specification and worth fixing for metadata correctness
  and for exporters that behave differently, but it costs nothing observable
  here. Recorded because reading the spec and predicting the behaviour gave
  the wrong answer and forty lines of throwaway code gave the right one.
* **`a2a.server.latency` should be `a2a.server.request.duration`.** This one
  *is* visible in the output. OTel names duration histograms `.duration`, so
  the conventional rendering is `a2a_server_request_duration_seconds`; this
  ships `a2a_server_latency_seconds`, which no OTel dashboard template will
  match.
* **Three parallel counters** — `.requests`, `.responses`, `.errors` — where
  the convention is one counter with an outcome attribute. As shipped, "error
  rate" is a division across two instruments.

None of these is changed here. `book/src/deployment/observability.md` states
"These names are stable; treat them as the contract", so the catalogue is a
published contract and renaming it is a breaking change that needs its own
decision, its own upgrade note and its own release. Recorded, not done.

### 5. Prior-task-state inheritance — fixed, confirmed

Issue [#130](https://github.com/tomtom215/a2a-rust/issues/130), fixed in
`5ee3cfb` and released in 0.12.1, classified in `CHANGELOG.md` as the
`STABILITY.md` §2 specification correction that made it patch-eligible.
Nothing outstanding.

### 6. There is no tracing at all — and for *this* protocol that is the biggest gap

The `otel` feature is metrics-only. `opentelemetry_sdk` is compiled with
`features = ["metrics", "experimental_metrics_custom_reader"]` and
`opentelemetry-otlp` with `["grpc-tonic", "metrics"]`. There is no
`TracerProvider`, no span export, and
`grep -rni 'traceparent\|tracestate' crates/ --include='*.rs'` matched
**nothing** before this change, and matches exactly one line after it:
`otel/pipeline.rs`'s new doc comment saying there is no `traceparent`. No
code reads or writes either header. (Widening the pattern to `propagat`
adds only unrelated uses of the English word "propagate".)

For an agent-to-*agent* protocol this is the wrong thing to be missing. The
defining property of A2A is that work crosses process and organisational
boundaries. Metrics say this server was slow. They cannot say that a triage
agent's 40-second task was 38 seconds waiting on a runbook agent two hops
away, and that is the only question anyone asks of a multi-agent system.
Worse, the task ids *differ* at every hop — each agent mints its own — so
even correlating by hand across logs does not join the chain.

**Three public documents claimed otherwise, and all three are corrected in
this change:**

* `book/src/deployment/observability.md` said task and context identifiers on
  spans mean "a single incident can be followed across the delegation chain
  when agents call agents". It cannot be: separate processes produce separate
  span trees with no shared trace id.
* `book/src/deployment/troubleshooting.md` listed "Metrics / **traces** over
  OTLP" against the `otel` feature.
* `docs/rust-sdk-assessment.md` claimed `✅ (otel feature: **traces** +
  metrics)` in the capability comparison against `a2a-rs` — an overclaim
  about this project inside a row asserting a shortfall in someone else's,
  which is the worst place for one.

**The hooks for fixing it already exist**, which is why this is the
recommendation rather than an aspiration. `CallContext::http_headers()`
already reads inbound headers. `CallInterceptor` already hands the client a
mutable `extra_headers` on the way out. Extract `traceparent` inbound,
inject outbound, attach `a2a.task.id` as a span attribute, and a delegation
chain becomes one trace. It is protocol-level, so it works cross-language —
and the ITK already runs against the official Python, JavaScript, Go and
Java SDKs, which means this project could publish a **cross-language trace
conformance result nobody else in the ecosystem is positioned to produce**.
Nor is anyone else placed to: `a2a-server-lf` 0.4.4 declares nineteen
dependencies, none of them an `opentelemetry` crate, and one feature,
`native-tls` — checked against the published manifest at
`https://index.crates.io/a2/a-/a2a-server-lf`, which is also true of
`a2a-lf` 0.3.1 and `a2a-client-lf` 0.2.5.

## Ergonomics, measured by building three examples in one session

Written from having actually used the SDK on 2026-09-19 to build
`rig-agent`'s tool loop, `mcp-agent` and `mcp-bridge`, rather than from
reading it.

### The `RequestContext` blind spot — closed

Recorded here as the single biggest constraint: an executor could not see
caller identity, tenant, HTTP headers or the activated extension set, because
`build_request_context` took no `CallContext` and `tokio::spawn` dropped
`TenantContext`. Both halves are fixed.

`RequestContext` now carries `call_context`, with `caller_identity()`,
`tenant()`, `http_header()`, `activated_extensions()` and `request_id()` on
top of it. The spawn re-enters `TenantContext::scope`, matching what the
background event processor and the sync collector already did — so the
task-local is correct inside an executor too, though `ctx.tenant()` is the
field to reach for, since it cannot silently read empty.

**Two things worth keeping from doing it.** The tenant drop was documented at
`handler/mod.rs:157-160` with its measurement ("the executor saw `\"\"`") and
was still not fixed, which is what a note without a test looks like a month
later; the regression test now fails without the scope and passes with it.
And the break is one line, measured: `cargo semver-checks` reports 196
checks, 195 passing, and only `struct_marked_non_exhaustive` failing. Adding
the field was free because the `#[non_exhaustive]` subsumes it — which is the
argument for marking it now rather than at the next field.

The `mcp-bridge` workaround this section named — passing the caller's chosen
skill through `Message.metadata` — is still what that example does. It is
now a choice rather than the only option: a bridge could send the skill as a
header and read it with `ctx.http_header`. Not changed, because the metadata
hint is what the MCP side can actually populate.

### Five things every example hand-writes — three of the five are gone

**Superseded in part.** The `AgentCard` literal, the `MessageSendParams`
construction and the artifact text extraction are all one call now; the list
below is kept because the other two are still true and because the measured
sizes are what justified fixing them. What changed, and what did not, is in
*What the constructors changed* below.

### The original list

Each of these was written three times in one session:

* a hyper accept loop, about 25 lines, because `serve()` does not cover
  "JSON-RPC and REST on one socket";
* an `AgentCard` struct literal of 38, 43 and 45 lines respectively — and
  this one is a **discoverability** failure, not a missing feature. The
  builder covers every field all three cards set: `AgentCard::new`, twelve
  `with_*` methods including `with_skill` and `with_interface`
  (`agent_card/builders.rs`), and `AgentSkill::new().with_tags()`. All three
  examples reached for the literal anyway. The likely reason is that they
  import `a2a_protocol_types::agent_card::{AgentCard, AgentSkill, …}` and
  land on the struct, whose rustdoc (`agent_card/mod.rs:182-190`) describes
  what the document is and never mentions that a builder exists; the fields
  are right there and `builders.rs` is not. `hello-agent`, which comes in
  through the SDK prelude, uses the builder. A `# Construction` line on the
  struct doc is a one-line fix for the 126 lines those three functions
  occupy;
* `MessageSendParams` construction, about 12 lines, to send one line of text;
* text extraction from a task's artifacts, about 8 lines;
* a message-id generator — `uuid_like()` was written twice rather than take
  a `uuid` dependency for two call sites.

`hello-agent` is 28 lines of code — `src/main.rs` lines 24-67, which is
everything above its `#[cfg(test)]`, excluding blanks and comments; the
file is 185 lines with its tests — because it uses `agent_executor!` and
`EventEmitter`. Almost nothing else does. The ergonomic layer exists, reaches
unevenly, and is under-advertised where it does reach. Two genuine holes:
`impl Message` has exactly two methods, `text` and `texts`, and no
constructor, so there is no `Message::user_text("hi")`; and `Task` has no
`impl` block at all in the types crate, so there is no `task.text()` to pull
an answer out of a finished task. Neither `MessageSendParams` nor `Task` has
a single inherent method between them. Adding those two, plus the
`# Construction` pointer above, would delete roughly a hundred lines from
every agent anyone writes and make the examples shorter rather than longer.

### What the constructors changed

Shipped 2026-09-19 in `feat(types): constructors for Message, Task and
MessageSendParams`. Counted on the tree with
`git ls-files '*.rs' | xargs grep`, filtering out signatures and definitions —
the earlier figures in this file were contaminated by `fn ... -> Message {`
lines and by protobuf `pb::Message {`, so they ran high:

| literal | sites |
|---|---|
| `Message {` | 105 |
| `AgentCard {` | 82 |
| `Task {` | 82 |
| `MessageSendParams {` | 75 |
| `AgentSkill {` | 54 |

`Message` has 8 fields, not nine as recorded above; a one-line-of-text message
cost a ten-line literal, five of whose fields said `None`.

Added: `Message::{new, user, agent, user_text, agent_text}` and `with_*` for
all five optional fields; `MessageSendParams::{new, with_tenant,
with_configuration, with_metadata}`; `Task::{text, texts}`; `# Construction`
sections on `AgentCard`, `AgentSkill` and `AgentInterface`.

Three things worth not rediscovering:

* **The id stays a parameter.** `a2a-protocol-types` depends on `serde` and
  `serde_json` and nothing else. Minting an id means either a new mandatory
  dependency for a pure-data crate or a clock-derived id that collides under
  concurrency. `Artifact::new` already made this call.
* **`AgentCard::new` leaves `default_input_modes` and `default_output_modes`
  empty**, where every hand-written literal set `["text/plain"]`. Converting a
  literal without adding `.with_input_modes(..)` silently changes the served
  card. This is the one trap in the conversion.
* **`Task::text` skips artifacts with no text** rather than stopping at the
  first, so it answers where `artifacts.first().and_then(Artifact::text)`
  returns `None`. Deliberate, documented and asserted — it is the rule
  `Message::text` already applies across parts.

Adoption: seven files under `examples/` (net −113 lines) and five book pages
(net −108). Three cards stopped setting `AgentCard.url`, which is
`#[serde(skip_serializing)]` and therefore unreadable by any client;
`rig-agent`'s test now asserts `supported_interfaces[0].url` instead. One book
snippet, `book/src/deployment/testing.md`, was setting a `context_id` field
`MessageSendParams` does not have — it sat in a `rust,ignore` fence, so
nothing had ever compiled it.

The two hand-rolled `uuid_like()` helpers went with them. Each was a
nanosecond timestamp — not unique under concurrency, and sequential enough to
guess — written to avoid a `uuid` dependency the workspace already carries and
seven other examples already use. Both examples now take `uuid` and call
`Uuid::new_v4()`.

**Still hand-written, and still worth doing:** the ~25-line hyper accept loop,
because `serve()` does not cover "JSON-RPC and REST on one socket". That is
the last of the five with no answer.

### The event-log absence has a measured cost now

State is a folded snapshot; `sqlite_store/journal.rs` is an artifact-parts
side table, not an event log; there is no SSE `id:` or `Last-Event-ID`.

*(Written before item 5 below. Both halves have since shipped: the log is
durable on every store this crate ships, and SSE frames carry an `id:` that
`Last-Event-ID` resumes from. The measured cost below is what motivated
them.)*

The measurement: in `mcp-bridge`'s demo the sample agent emits three progress
steps 120 ms apart and the MCP caller sees **one**, because a poller can only
ever observe the latest fold. That transcript is in the example's README with
the cause named. A reconnecting client gets a snapshot, not what it missed —
and #130 was itself a fold bug, which an event log makes impossible by
construction.

### Two smaller ones

* **`Failed` is prose** (already recorded above as A2). Building the bridge,
  mapping an A2A failure onto MCP's `isError` had only the state enum to work
  from. Bad input, transient infrastructure, policy refusal and budget
  exhaustion are four different caller behaviours behind one variant, so
  every bridge and orchestrator will re-invent an English matcher.
* **A documented wrong default.** `HandlerLimits::push_delivery_timeout` is
  5 s while `HttpPushSender::new()`'s retry schedule totals 98 s, so at
  defaults 1 of 3 attempts runs. Honestly recorded at
  `handler/limits/mod.rs` (`push_delivery_timeout`'s own rustdoc) — but a
  reader who sees `max_attempts: 3` and
  does not open the other file gets one attempt and no warning.

## What to build next, ranked

Ordered by value per unit of work, from the seat of someone who consumes
agents rather than maintains the protocol.

1. ~~**Trace context as a protocol concern.**~~ Done — see *Trace context
   is carried now* below. It did use the hooks that already existed:
   `CallContext::http_headers` inbound, `ClientRequest::extra_headers`
   outbound, and nothing else needed inventing.
2. ~~**Plumb `CallContext` into `RequestContext`.**~~ Done — see *The
   `RequestContext` blind spot — closed* above.
3. ~~**The executor conformance harness (A4 above).**~~ Done —
   `a2a_protocol_server::conformance` behind the `conformance` feature,
   thirteen checks, twenty-one tests of its own. (Recorded here first as
   "eight checks, fourteen tests"; both grew as the harness gained the
   cancel-path, clock and call-context grading. `grep -c results.push
   crates/a2a-protocol-server/src/conformance/mod.rs` gives the first, and
   `cargo test -p a2a-protocol-server --features conformance --lib
   conformance:: -- --list` the second.)

   **What it does not cover, stated so the next person does not assume it
   does.** It runs each check once, so it finds no races. It drives the
   executor directly, so "a client disconnecting mid-stream" — one of the
   three cases this item named — is still untested: that is a server-side
   event the executor never observes, and testing it needs a real stream
   rather than a queue. `input-required` never answered is likewise the
   *handler's* behaviour, not the executor's; what the harness checks is the
   half that is the executor's, that parking returns `Ok` rather than `Err`.
   Cancellation arriving *mid-artifact* is approximated by a pre-cancelled
   token, which catches an executor that never checks at all but not one
   that checks only before its first emit.
4. ~~**A typed failure taxonomy shipped as a declared extension.**~~ Done —
   `a2a_protocol_types::failure`, `Task::failure_class()`,
   `EventEmitter::fail`, advertised on the card, with a book page. **The
   client's retry policy does not consume it and should not**: `retry.rs`
   retries *transport* calls, and a task that ran and failed is not a failed
   call — the send succeeded. Consuming it belongs in whatever drives the
   task, not in the client's RPC retry loop. That half of the original item
   was wrong about where the seam is.
5. **Make the event log the record and state the fold.** Half done, and the
   half that is done is the substrate rather than the payoff.

   **Shipped:** `TaskStore::{supports_event_log, append_event,
   last_event_seq, read_events}`, with both fold paths — the background
   processor and `sync_collector` — recording every event before folding it.
   Positions are idempotent, and both paths seed `seq` from the store so a
   continuation does not collide with its own earlier run.

   Also shipped: every store this crate ships backs it —
   `InMemoryTaskStore`, `SqliteTaskStore`, `PostgresTaskStore`,
   `TenantAwareInMemoryTaskStore`, `TenantAwareSqliteTaskStore` and
   `TenantAwarePostgresTaskStore`. Tables `task_events` and
   `tenant_task_events`, created by both schema paths on each backend
   (SQLite migration 7 and Postgres migration 5, plus each `from_pool`'s
   inline DDL), with a test pinning each path — SQLite's two paths have
   drifted twice and `migration.rs` records both incidents. The tenant
   tables are keyed `(tenant_id, task_id, seq)`, because task ids are
   caller-supplied and an unscoped log is a cross-tenant read of message
   content.

   Also shipped: `id:` on every logged SSE frame and `Last-Event-ID` on
   resubscribe — the payoff, and the fix for the measured `mcp-bridge` case
   where three progress events arrive as one. The position is assigned in
   `InMemoryQueueWriter::write` and carried on both channels, so the `id:` a
   subscriber reads and the `seq` the store writes are one number rather than
   two counts that agree; `EventQueueReader::read` yields a `StreamEvent`
   accordingly. Server-synthesized frames (the snapshot, the rebuilt terminal
   frame) carry no position. Replay is bounded by
   `HandlerLimits::subscribe_replay_limit`.

   **Not shipped, in the order it is worth doing:**

   * **Making state a fold over the log on read.** The user's choice for this
     round was the log with the snapshot kept as the record, so this stays
     deliberately undone. It is what would make #130-class bugs impossible
     rather than merely detectable, and it is a migration for existing
     deployments.

   **One thing to check before building on it.** A *custom* store inherits
   the refusing defaults, so a deployment using one has no log at all.
   `supports_event_log()` is the gate; anything added downstream must ask
   rather than assume.
6. **Publish `tck/sut`.** The fastest route to people using the server crate
   is for it to become the thing they test *their* agent against. It is
   already built.

**What a heavy agent user wants from this SDK, stated plainly:** to hand a
coding agent an A2A endpoint and have it work (that is `mcp-bridge`, and it
should be on crates.io); to know why a delegated task failed without parsing
English; to see one trace across a delegation chain; and to get the caller's
identity inside an executor.

## What is good, since an all-negative list is neither complete nor credible

The gate culture is the best thing here and it is not close.
`check_package_excludes.py` and `check_book_code.sh` each caught a real
registration mistake in this session before CI would have, with error
messages that named the fix. The docs-as-argument style — every limit stated
where it happens, with the measurement that found it — is why verifying five
maintainer claims took an hour rather than a day. And three non-trivial
examples were built against the core in one day without fighting it once.
The foundation is sound; what is missing is mostly *above* it.

## `claude/optimistic-bell-680i9p` — the post-0.13.0 audit

Six defects were fixed in `1f5c5e2f`, three of them without a regression test.
That is worth stating first, because it is the pattern this whole branch is
about: the fixes were verified by reading the code, which is the standard of
evidence that let the original defects through.

### The verification gap that cost a red CI run

`1f5c5e2f` broke CI in fourteen jobs. One root cause, in code I had written
and checked: `trace_warn!` expands to nothing unless the `tracing` feature is
on, so an error bound only to be logged is an **unused variable** in every
default-feature build, and `RUSTFLAGS: -D warnings` makes that a hard error.

The gate that catches it — `cargo clippy --workspace --all-targets -- -D
warnings`, ci.yml line 386 — already existed and works. What failed was local
verification with `--all-features`, where `tracing` is on, the macro consumes
the binding, and the whole class of defect is invisible.

**The rule, for any future session:** `--all-features` is not a superset for
lint purposes. It compiles a different set of `#[cfg]` arms and it hides every
defect whose only consumer is a feature-gated macro. Verify with the
default-feature leg too. The convention for a value bound only to be logged is
a leading underscore plus a structured field —
`if let Err(_e) = … { trace_warn!(error = %_e, "…"); }` — and the macros carry
`#[allow(clippy::used_underscore_binding)]` for exactly that.

Related: `cmd | tail` reports `tail`'s exit status, not the command's. Capture
`${PIPESTATUS[0]}` or redirect to a file. A gate read through a pipe has been
misreported as passing more than once in these sessions.

### What landed

- **The three missing regression tests** (`61359911`), each proven to fail
  against the un-fixed code by reverting that fix alone: `message.id`
  validation, the idempotency replay wait, and the push-config rollback. The
  last of those includes a **deterministic** test of the per-context guard
  ordering — a gated push-config store parks the first send inside the
  push-config step while a task-store double reports when a concurrent send
  reaches `find_task_by_context`. No sleeps.
- **W3C Trace Context** (`569bbbfd`). A `traceparent` whose 55th byte fell
  inside a multi-byte character aborted the process (`panic = "abort"`, peer
  input, public path). Reserved `trace-flags` bits were propagated; an
  oversized `tracestate` was discarded whole; a version-`00` header with a
  trailing field was accepted; nineteen citations were wrong, two of them
  naming sections that said the opposite of the code beside them.
- **`InboundTracePolicy`**, reworked from the process-global `AtomicU8` it
  arrived as into a per-handler field set by
  `RequestHandlerBuilder::with_inbound_trace_policy`. The global could not
  express a process serving both a public front gate and an internal endpoint,
  and needed a test-only mutex to stop one test's policy leaking into another's.
- **Event-log durability** (`569bbbfd`): appends that wrote nothing were
  discarded silently; the in-memory log was unbounded; a reader asking for an
  evicted position got a gapped stream it could not detect; the SQLite orphan
  sweep ran only when a purge deleted something.
- **`prune_empty_tenants` destroyed live idempotency indexes.** It decided on
  `count()`, which counts tasks, and a key deliberately outlives its task. This
  one is worth remembering as a shape: a memory-reclamation path that looks
  unrelated to correctness, reopening the exact double execution the feature
  exists to prevent.
- **Two new gates.** `scripts/check_fuzz_matrix.py` fails when a fuzz target
  exists but no runner executes it — `trace_context` shipped registered in
  `fuzz/Cargo.toml` and absent from `fuzz.yml`'s matrix, which the gate caught
  on its first run. Registered in ci.yml and in `prove_gates_fail.sh`, and
  PROVEN (the harness is at 70 gates now, from 69). The per-crate rustdoc gate
  from `1f5c5e2f` is the other, and closes item 7 under **Still open**.

### What the next session should do first

1. **Watch CI on this branch.** The last push is `7182aac3`. Everything below
   was verified locally; CI is what says the fourteen-combination matrix and
   the live-PostgreSQL jobs agree.
2. ~~**The PostgreSQL half is unverified here.**~~ **Done, and the premise was
   wrong.** I recorded this as a container limitation; PostgreSQL installs from
   apt in under a minute. `apt-get install -y postgresql postgresql-contrib`,
   `service postgresql start`, `ALTER USER postgres PASSWORD 'postgres'`, then
   `A2A_TEST_POSTGRES_URL=postgres://postgres:postgres@localhost:5432/postgres`.
   Server 16.13 — the same major version as CI's `postgres:16`.

   **Run it before claiming a Postgres change is verified.** It caught a real
   failure on the first run: `migrations_apply_in_order_and_are_idempotent`
   hard-coded five migrations and a sixth had been added. Only a live server
   runs that test, so the drift was invisible until CI. It now derives the
   list from `BUILTIN_PG_MIGRATIONS` and asserts contiguity, so bumping the
   number is not a trap that rearms for migration 7.
3. **Regenerate `docs/provenance-manifest.md` as the last commit before the
   tag.** `RELEASING.md` step 4 has the ordering constraint; a manifest
   generated before any later commit is stale by definition and the gate fails
   the tag. `scripts/provenance_manifest.sh HEAD` regenerates it and
   `python3 scripts/check_provenance_manifest.py` is the gate — run it
   *without* a pipe, or you read `head`'s exit status instead of the script's
   and a failing gate looks green.
4. **Then the two 0.13.0 release steps that were already outstanding:**
   `git tag -a v0.13.0`, and the `bindings/a2a-protocol-slimrpc` 0.5.0 publish
   (`RELEASING.md` step 4), which has never been run.

### `claude/busy-cerf-ta682r` — what A2A does at a thousand agents

A new `#[ignore]`d load experiment,
`crates/a2a-protocol-server/tests/swarm_scale/`, and its write-up,
`docs/swarm-scale-findings.md`. Test-only: no crate source is added or changed.
Run it with

```text
A2A_SWARM_MAX=1000 cargo test -p a2a-protocol-server --release \
  --test swarm_scale -- --ignored --nocapture --test-threads=1
```

The question was whether many agents sharing **one** object works, which
nothing here had ever measured — `concurrent_agents.rs` stops at 64 and gives
every agent its own task, and `soak.rs` runs eight workers each on their own
task. Four findings, all reproducible from that command; the report has the
tables:

1. One task peaks at **four** concurrent writers and falls 18× by a thousand,
   to a 1.31s median post. It queues rather than refuses: `commit_task` holds
   the per-context lock across find-decide-save.
2. **A context holds exactly one addressable task, and which one it is
   changes.** `resolve_task_id` mints a fresh task whenever a message names
   none — unconditionally — and `find_task_by_context` resolves a context to
   its most-recently-updated non-terminal task. So one message without a
   `taskId` permanently locks every other caller out of the task they were
   using, with a 400 that says only `message task_id does not match task found
   for context`.
3. Sharding by **context** recovers it: 6.4× the throughput at a thousand
   agents, refusals gone from K=4, and the knee at roughly 4–16 writers per
   task.
4. A `SubscribeToTask` tail is live only when a turn outlasts
   `subscribe_reattach_interval` (250ms). Turns that park instantly deliver
   nothing to any tail at any subscriber count; turns that outlast the poll
   deliver everything to a thousand tails with zero gaps.

Two things for the maintainer that are not about swarms:

- The comment on the no-`taskId` branch of `resolve_task_id` says "If the found
  stored task is terminal, a new task will be created on this context". That
  path never reads `stored_task`. The comment names a condition the code does
  not check, and finding 2 is the measured behaviour. Whether the spec wants a
  context-only message to join the live task or fork a new one is an open
  question this experiment does not answer.
- `streaming::event_queue::in_memory`'s module docs say a lagging SSE consumer
  "receives `Lagged(n)` and skips missed events". It does not skip and resume:
  `read` returns `A2aError::stream_lagged`, `streaming::sse` writes it as an
  `event: error` frame and closes the stream. Measured across the 85 lagged
  tails of the burst arm (1 + 4 + 16 + 64), every one was cut off and every one
  was told, with zero undetectable gaps — the behaviour is the better of the
  two and the documentation describes the other one.

Neither was changed here. Both are one-line fixes in crate source, which this
branch deliberately does not touch.

#### Then the measurement that matters more than any of it

Two further modules, `independent.rs` and `cost.rs`, ask what the send path
costs when nothing contends at all. The answer reframes findings 1 to 4 as the
smaller problem.

`GET /health` — the same socket, listener and dispatcher, with no handler
behind it — answers 52,799 requests a second on this box. An uncontended
`POST /message:send` answers 4,671. At a concurrency of one the split is 21µs
of transport against a 206µs request, so **about 90% of a send is work behind
the dispatcher**, and posts stay between 3,669 and 5,250 per second from one
agent to a thousand. That flatness is a per-request cost, not a saturated box:
the same box does ten times the number through the same sockets.

Worse, the cost grows with the channel's age. One channel, 1,400 sequential
posts at concurrency one: service time rises 6.5x, from 385µs to about
2,500µs, and then flattens. The reply is a constant 152 bytes throughout, so
none of it is payload.

A controlled run names the cause. With `MAX_TASK_HISTORY_MESSAGES` lowered
from 1,024 to 64 and nothing else changed, the plateau falls from ~2,500µs to
~540µs and the growth from 6.5x to 1.3x. **History length drives it**, at
roughly 2µs per retained message per post. That edit was made to run the
experiment and reverted; the constant in the tree is 1,024.

The store is not implicated: one channel's cost is flat at 0, 100, 1,000 and
4,000 other tasks, so `context_index` is doing its job.

Reading the send path finds at least four O(history) touches per continuation
— `find_task_by_context`'s `list` clones each task it collects, `create`
clones the stored history to append one message, `save` stores a clone, and
the background state machine saves again per status event. That attribution is
read from source and consistent with the cap experiment; **no profiler ran**,
so which clone dominates is unmeasured. `perf` is not available in this
container.

#### Everything the findings report listed as open, closed

The report's "still unmeasured" list is empty and its two behavioural findings
are fixed. What that produced, in the order it happened:

* **`save_status_delta` reached the SQL stores.** It was overridden only
  in-memory, so SQLite and Postgres deployments got nothing from it and
  neither doc said so. Both override it now: 122,071µs → 37,216µs (SQLite)
  and 287,287µs → 120,104µs (Postgres) over 100 status events on a
  200-message channel. The SQLite override deliberately does **not** delete
  the artifact journal that its `save` deletes, and that divergence has its
  own test — copying `save` there would have silently dropped appended parts.
* **The context lockout (finding 2) is fixed, and the spec moved the defect.**
  §3.4.3 line 651 *permits* the fork; the bug was the 400 that followed it,
  because the server rejected any task that was not the one
  `find_task_by_context` returned while §3.4.3 mandates rejecting only a
  *contextId* mismatch. Two existing tests changed expectations and the commit
  says why at length: both named a task that did not exist and asserted
  `InvalidParams` where §3.4.2 requires `TaskNotFound`.
* **The send path stopped carrying the conversation (fix-program item 2).**
  `Task::history` cannot move — wire type, published crate, semver gate — so
  `TaskStore::save_appending_history` takes the snapshot plus only the turn's
  new messages. 3,962µs → 2,748µs at the history cap, growth 3.2x → 2.1x.
  **Not O(1)**, and the remaining stages are named below.
* **The dominant clone is measured, not inferred.** `find_task_by_context`
  511µs at the cap against `build_initial_task` 7µs and `persist_initial_task`
  42µs. So the next win is worth ~500µs of a ~2,750µs send and is blocked on
  one API decision, not on effort.
* **Findings 7–10 are new**: the SQL stores under the ageing probe, the
  single-writer refusal not crossing replicas, the three untouched surfaces,
  and the three bindings.

#### The one that is a correctness limit, not a number

`the_single_writer_refusal_does_not_cross_replicas`: two replicas sharing one
Postgres will **both** admit a continuation for the same task, both spawn an
executor, and both write the same row. `reject_in_flight_send` reads
`self.cancellation_tokens` and the send path serialises on `keyed_lock`, both
per-handler. Sharding by context does not help, because the shard key is
enforced by that same in-process lock. The test pins today's behaviour, so
anything that later makes admission shared has to fail it and say so.

#### Three near-misses worth carrying forward

Each of these passed something before it was caught, which is the reason to
write them down rather than the reason not to.

1. **A green suite is not coverage.** 2,037 tests passed with the
   history-append change in place and none of them covered a continuation
   keeping what an earlier turn wrote. The truncation did not happen —
   `background/mod.rs:84` re-reads the task — but nothing in the suite knew
   that, and I had predicted the opposite. `historyLength` *was* broken by the
   same change and also uncaught. Both have tests now.
2. **An arm that measures nothing can look like an arm that measures well.**
   The agent-card arm printed a full latency column beside `ok` of zero,
   because the deployment served no card and every fetch was a 404. The
   WebSocket arm posted with `contextId` alone, which forks, so it measured
   task creation and called it a channel post. Both now assert what they
   claim to measure.
3. **`cargo update -p rustls --precise 0.23.45` is the rustls waiver's own
   removal test.** Running it beats reasoning about it; it still fails, so the
   entry stays, now date-stamped.

#### Verification, once the tools existed

Everything the last session said it had not run, run. Recorded because three
of the five said something.

- **CI is green.** Run
  [35597781502](https://github.com/tomtom215/a2a-rust/actions/runs/35597781502)
  at `ab9c9a7`, all 20 jobs, including `cargo-semver-checks` — which is the
  independent check on "additive, non-breaking" — the PostgreSQL integration
  leg, and the test matrix on Linux, macOS and Windows.
- **CI caught two doc links nothing local had.** Run
  [35593616804](https://github.com/tomtom215/a2a-rust/actions/runs/35593616804)
  failed its Documentation job on
  `crate::handler::messaging::MAX_TASK_HISTORY_MESSAGES` (private module, so
  not linkable from a public doc comment) and on `[StoreData::update_status]`
  (private item linked from public docs). `cargo doc --workspace --no-deps`
  with `-D warnings` is in the PR checklist and had not been run. Fixed in
  `ab9c9a7`.
- **The mutation gate passes: 0 missed.** `cargo-mutants` was not installed in
  the container; it is now. 19 in-diff mutants, 3 caught, 13 unviable, **3
  timeouts** — all three in `StoreData::update_status`. Timeouts fail no gate
  but `mutants.yml` says in as many words not to read past them, so they were
  re-run at `--jobs 1 --timeout 900`: 4 caught, 4 unviable, **0 timeouts, 0
  missed**. The timeouts were four parallel test suites on this box's four
  cores, not an adequacy gap.
- **Soak is clean after the store change.** 120s, 688,438 requests, 16.7 bytes
  per request against the 1,024 ceiling, p95 latency 1.01x head to tail.
- **The two findings are fixed**, each with a test shown to fail under a
  mutation that breaks exactly what it checks. See the commits.

Two things worth knowing for next time. `cargo mutants` needs a live
PostgreSQL or its **baseline** fails — two `rate_limit::shared::postgres`
tests — and it reports that as "cargo test failed in an unmutated tree", which
reads like a broken tree rather than a missing service. The handoff's own
apt recipe above is what fixes it. And this container's disk allowance fills
quickly: a release build of the workspace plus two `cargo install`s exhausted
it twice, both times fixed by `rm -rf target/debug target/release`.

#### Both delta paths now pace eviction, and a doc comment had come adrift

`save_artifact_delta` carried the same eviction-pacing gap as the status one
and is now fixed too. Its `// No new entry, so the store cannot have grown
past its bound and there is nothing for eviction to reconsider` was right
about the capacity bound and wrong about the TTL one — expiry is driven by
elapsed time, not by growth — so a workload dominated by artifact streaming
swept expired tasks ever more rarely. Regression test
`an_artifact_delta_still_paces_the_eviction_sweep`, shown to fail under a
mutation that drops only the counter bump while the status test still passes.

Measured on this box, 500-chunk stream, in-memory store, snapshots built
outside the timer, medians of nine: `save` 18.6 ms against the delta's 0.89 ms
with the fix and 0.86 ms without it. Between-run variance is around 400 µs —
a third run of the fixed code came back at 1.26 ms — so the ~36 µs between the
arms is well inside the noise: **no measurable regression**, which is the only
claim the measurement supports. The first version of that harness rebuilt an
n-part task inside the timing loop and so measured its own O(n²) setup,
reporting the delta at 17.7 ms; the numbers above are from the corrected one.
The 43.4 ms → 2.5 ms in the trait docs was measured on different hardware and
is left alone.

Separately, and this is the one nothing mechanical would have caught: the
status delta had been inserted **between `save_artifact_delta`'s doc comment
and its function**, so the artifact doc was attached to the status method —
where "no artifacts, index out of range, a different artifact at that index"
is simply false — and the artifact method had no doc at all. rustdoc does not
check that a doc comment describes its item, so CI was green through it. Both
are back where they belong.

`TaskStore::save_artifact_delta`'s "Implementing this" section now states the
general rule both bugs broke: a delta is a cheaper way to do a write, not a
way to do fewer writes, so whatever per-write bookkeeping an implementation's
`save` does, an override must do too. That matters because the trait is
unsealed and third-party stores will override these.

#### What the next session should pick up

The fix program, in the order the evidence supports:

1. ~~**Stop the avoidable O(history) work on the send path.**~~ **Started, and
   the first piece landed.** `TaskStore::save_status_delta` is additive (its
   default delegates to `save`), overridden in the in-memory store to edit the
   status in place and re-key the indexes. One turn of 512 status events on a
   channel holding 600 messages: 54,301µs with `save`, 2,248µs with the delta,
   measured back to back on the final code, and the turn stops growing with
   the channel's age. It does **not**
   measurably move a turn that emits one event, and the report says so.

   What that bought, and what it did not, is now measured rather than guessed.
   Timing the stages of `commit_task` in a temporary build (reverted) gives,
   per send at the history cap: `find_task_by_context` ~280µs,
   `build_initial_task` ~300µs, `persist_initial_task` ~440µs — together
   roughly 40-50% of a 2,400µs request, and all three scale with history.

   The rest of item 1 as originally written turns out not to be separable.
   `build_initial_task` needs the stored history to carry it forward and
   `build_request_context` needs the stored task for the executor's view of
   the previous turn, so neither clone can simply become a move. That is item
   2, not a cleanup.
2. ~~**Decide whether `Task::history` belongs inside the `Task` snapshot.**~~
   **Decided and done, and it did not reach O(1).** `Task::history` cannot
   move: it is the A2A wire type in a published crate, and cargo-semver-checks
   holds that. So the change took the seam the trait already used twice.
   `TaskStore::save_appending_history` takes the snapshot plus only the
   messages the turn added; `build_initial_task` stopped cloning the stored
   conversation forward. Measured back to back, 1,400 sequential posts:
   3,962µs → 2,748µs at the history cap, growth 3.2x → 2.1x, and nothing at
   the start.

   **What is left, and why it stopped here.** Three stages were O(history);
   this removed one and a half. The other two:

   * `find_task_by_context` still lists and clones up to ten whole tasks to
     pick one. It could ask for no history now that `build_initial_task` does
     not need it — **except** that its result becomes `RequestContext`'s
     `stored_task`, a `pub` field on a `pub` struct, documented as the
     executor's view of the previous turn. Stripping history from it would
     silently change what every user-written executor sees. That is an API
     decision, not a refactor, and it is the next one to take.
   * The background processor re-reads the task at `background/mod.rs:84`.
     That read is why the refactor did not corrupt anything — it is the reason
     a continuation still accumulates history — and it is off the request's
     critical path, so it costs throughput rather than latency.
3. **The context lockout (finding 2)** and **the reattach poll (finding 4)**,
   each with a regression test that is shown to fail without its fix.
4. **Re-run every arm after each fix**, and keep the before-and-after in
   `docs/swarm-scale-findings.md` rather than overwriting it.

### Still not started

- ~~**Idempotency key expiry (H13).**~~ **Done.**
  `RetentionPolicy::idempotency_key_max_age` and
  `TaskStoreConfig::idempotency_key_ttl`, both 24 hours by default, `None` to
  keep the old behaviour. No migration was needed — all four SQL tables
  already carried a `created_at` column that nothing read.

  Two things worth carrying forward. The sweep refuses to delete a key younger
  than `terminal_max_age`, because a key expiring while its task is still
  retained produces a *second* task rather than a replay — the clamp is in
  `effective_idempotency_key_max_age`, not in a doc comment. And a replay does
  not refresh the claim time, or a client retrying on a loop would hold a key
  open for ever and the TTL would bound nothing for exactly the caller most
  likely to reach it.
- `supports_event_log()` is hard-coded `true` on `InMemoryTaskStore`. With the
  log now bounded, an opt-out would make the server advertise no resumption at
  all rather than a bounded one, so it was judged the wrong trade — recorded
  here because it was considered, not overlooked.

## Still open

Numbering was 1, 2, 4, 5 here — there was never a 3. Renumbered.

1. ~~Delete `release/v0.12.1`, whose contents are merged and tagged.~~
   **Done by the owner, 2026-09-22.** Verified after the fact:
   `git ls-remote --heads origin 'release/*'` returns nothing, and tag
   `v0.12.1` still resolves to `e057c8e`, so the release stays identifiable
   and the merged commits stay reachable from `main`. Nothing was lost.

   Kept as a record of how it was handled rather than deleted outright: this
   was the one irreversible outward-facing action on the list, it was verified
   ready here (branch at `2e9262e`, listed by
   `git branch -r --merged origin/main`) and then left for the owner, who
   asked to do it themselves.
2. Submit the adk-rust work if it is still wanted: issue first, then the patch.
   **Blocked on repository access, not on the work.** The patch is prepared and
   intact on `claude/adk-rust-0.12-patch` at `6fbdd2f` (verified against origin
   2026-09-21). This session's GitHub scope is `tomtom215/a2a-rust` alone, so
   the issue and PR cannot be opened from here. To unblock: add the adk-rust
   repository to a session, or open the issue by hand and apply the held patch.
3. ~~The binding's `RUSTSEC-2026-0285` waiver — see its section above for the
   command that says when it can be deleted.~~ **Re-checked 2026-09-21: it
   stays.** `cargo update -p rustls --precise 0.23.45` in
   `bindings/a2a-protocol-slimrpc` still fails with the documented conflict
   against a freshly fetched index — `^1.18` offers 1.18.1/1.18.0,
   mls-rs-crypto-awslc 0.23.0 still pins `=1.16.2`, and slim-auth 0.15.4 still
   admits only that 0.23.x. Neither release condition has been met. The date
   stamp in `deny.toml` records the re-check.
4. ~~Stale install snippets.~~ Done — see "Prose versions are checked now"
   below. The figure recorded here first, six, was wrong: it counted only
   `crates/`, and the real number was 28.
5. ~~Re-run `prove_gates_fail.sh` for the three `--features {sqlite,postgres,
   auth-jwt}` gates.~~ **All 70 run, 2026-09-22: 70 proven, 0 unproven.**
   Nothing PRE-BROKEN, nothing INCONCLUSIVE. The two that had never been
   provable here needed tools, not fixes — `cargo install cargo-hack --locked`
   (0.6.45) and SPIRE 1.11.2 unpacked to `/tmp/spire-1.11.2/bin` with
   `SPIRE_BIN_DIR` exported. Both now pass, the SPIFFE suites running 9 tests
   against a real server.

   **Run the whole set, not a subset, and run it last.** The first full sweep
   reported 13 PRE-BROKEN, and 11 of those were breaks this branch had
   introduced: `swarm_scale/bindings.rs` and `swarm_scale/cost.rs` used types
   that only exist under `websocket`, `grpc`, `sqlite` or `postgres`, so every
   single-feature build of the test targets failed to compile. A twelfth,
   `cargo package`, was the `a2a-protocol-client` dev-dependency making the
   server crate unverifiable. Every check while writing those arms had used
   `--all-features`, where all of it compiles. A green `--all-features` build
   says nothing about a single-feature one, which is what that gate is for.

   Two mechanics worth keeping. The script injects defects into **tracked
   source**, so nothing else may touch the repo while it runs and a clean
   `git status` is a precondition — if it dies mid-gate, discard with
   `git checkout -- <file>`, never commit. And its log prints each verdict
   twice, once per gate and again in the summary, so grepping verdict words
   doubles the count: read the summary block.
6. ~~The two hand-rolled `uuid_like()` helpers.~~ Done — both examples take
   `uuid` now.
7. ~~**`cargo doc -p a2a-protocol-client --no-deps` fails, and CI cannot see
   it.**~~ **Done, both halves.** The five links are fixed, and `ci.yml`'s
   `doc` job now documents each published crate on its own in that crate's own
   default feature set (`1f5c5e2f`). The gate caught two further breaks within
   minutes of being added, and a third on 2026-09-20 —
   `InboundTracePolicy`'s rustdoc linking to a private item — which is the
   behaviour it was added for. The history below is kept because the
   *counting* lesson in it is the durable part.

   The count and the locations recorded here were both wrong, and the
   correction is the point: this entry said **three** links, all in
   `builder/mod.rs`. Running the reproducer prints **five**, and two of them
   are in a file this entry never named:

   ```text
   $ RUSTDOCFLAGS="-D warnings" cargo doc -p a2a-protocol-client --no-deps
   error: unresolved link to `crate::WebSocketTransport`
     --> crates/a2a-protocol-client/src/builder/mod.rs:288:32
   error: unresolved link to `Self::build_grpc`
     --> crates/a2a-protocol-client/src/builder/mod.rs:290:24
   error: unresolved link to `Self::build_grpc`
     --> crates/a2a-protocol-client/src/builder/mod.rs:344:24
   error: unresolved link to `crate::WebSocketTransport`
     --> crates/a2a-protocol-client/src/config.rs:165:34
   error: unresolved link to `crate::WebSocketTransportConfig`
     --> crates/a2a-protocol-client/src/config.rs:166:58
   error: could not document `a2a-protocol-client`
   ```

   Counting from a reading of one file is how three became the recorded
   number; the reproducer was already written down two lines below it and
   would have said five.

   The cause is unchanged: each link points at an item behind the `websocket`
   or `grpc` feature, which are off in that crate's default build. `ci.yml`'s
   `doc` job runs `cargo doc --workspace --no-deps`, where feature unification
   turns both on (the client's own dev-dependencies pull them in), so the
   workspace build is green and the per-crate one is not. docs.rs builds with
   `all-features = true` and is unaffected too, which is why nobody has hit
   it. Pre-existing — the same text is at `f806792`, before any of this
   session's work.

   That half — a per-crate doc build in CI — is the one that shipped. Without
   it the next feature-gated link would rot exactly the same way with nothing
   going red, which is why it was the half that mattered.

### `prove_gates_fail.sh` was stuck at gate 5 of 65 — found and fixed 2026-09-19

Running the harness on a clean tree used to stop at step 5/65:

```text
[5/65] ./scripts/check_benchmark_prose.sh
       injection: benchmark_prose
AssertionError: benchmark_prose injection matched no text — the
connection-reuse figure in book/src/reference/benchmarks.md changed and
this string is stale
```

The injection rewrites the connection-reuse sentence to a wrong value and
asserts `check_benchmark_prose.sh` goes red. Its "from" side was the literal
figure, so every `cargo bench` regeneration invalidated it. The harness
aborts on that, so gates 6 through 65 went unproven with it.

**Pinning the number had already been tried and did not last a day.**
`80a9f401` fixed the first occurrence on 2026-08-27 by pinning the new value
and adding the assertion above, so the next drift would be loud rather than a
silent `UNPROVEN`. The page moved off it the same afternoon and seven times
after:

| commit | date | page reads | |
|---|---|---|---|
| `80a9f401` | 2026-08-27 | 122.5 µs (42.7%) | needle pinned here |
| `e388fb5f` | 2026-08-27 | 125.4 µs (43.3%) | stale the same day |
| `72465dc4` | 2026-08-30 | 123.3 µs (50.7%) | |
| `d52189ca` | 2026-09-09 | 113.4 µs (47.7%) | |
| `518bac67` | 2026-09-10 | 126.7 µs (43.7%) | |
| `baedfd50` | 2026-09-17 | 104.8 µs (44.5%) | |
| `9f0f3ea9` | 2026-09-17 | 143.8 µs (40.6%) | |
| `fa1a82b9` | 2026-09-18 | 116.6 µs (44.3%) | current |

**The fix taken** was the second option this section originally listed, not
the first: the needle carries no number at all and matches the sentence by
shape — `Connection reuse saves [0-9.]+ (?:µs|ns) \([0-9.]+%\) on loopback`.
The unit alternation comes from `derive_saving` in
`benches/scripts/generate_book_page.sh`, which writes the sentence and emits
microseconds at or above 1 µs and nanoseconds below, a branch the old literal
could never match. The assertion now demands exactly one match: zero means
the wording changed or the page has no measurement behind it, two or more
means a second sentence took the same shape.

Of the 34 injections, `benchmark_prose` was the only one targeting a
regenerated artifact, so this was the whole class rather than the first of
many.

**Where the harness stood on 2026-09-19**, run to completion in a detached
worktree: `55 proven, 10 unproven, 0 not selected (of 65 gates)`. The 10 were
not stale needles. Seven were **PRE-BROKEN** — already red on the clean tree,
so the harness correctly claimed nothing — and all seven were an
under-provisioned machine rather than a repository defect: five needed a
PostgreSQL server, one is the SLIMRPC SPIFFE suite, one is `cargo hack clippy`
failing in 0 s because `cargo-hack` is absent. The remaining three are the
next entry.

**"Under-provisioned" was doing too much work there, and it cost the session
a measurement.** A PostgreSQL server is one `apt-get install -y postgresql`
away in this container. Installed, started, and with the `postgres` role given
the password `ci.yml` already expects, three of those five gates go straight
to PROVEN — `postgres_store_tests --ignored`, `multi_replica --ignored` and
the `rate_limit::shared --ignored` counter suite, in 10 s, 7 s and 11 s. Do
this before recording a gate as unprovable for want of a machine:

```sh
apt-get install -y postgresql
PGV=$(ls /usr/lib/postgresql/ | head -1)
mkdir -p /var/run/postgresql && chown postgres:postgres /var/run/postgresql
su postgres -c "/usr/lib/postgresql/$PGV/bin/pg_ctl \
    -D /var/lib/postgresql/$PGV/main \
    -o '-c config_file=/etc/postgresql/$PGV/main/postgresql.conf \
        -c listen_addresses=localhost -p 5432' -l /tmp/pg.log start"
su postgres -c "psql -c \"ALTER USER postgres WITH PASSWORD 'postgres';\""
```

`prove_gates_fail.sh` reads `A2A_TEST_POSTGRES_URL` out of `ci.yml` itself, so
nothing else needs setting.

### The process-global panic hook is fixed — and the recommended fix was wrong

Recorded here on 2026-09-19 as found-but-not-fixed, with a proposal. Both the
count and the proposal turned out to be wrong, so this section is rewritten
rather than ticked off.

**There were three sites, not two, in two crates rather than one.** The two in
`crates/a2a-protocol-server/src/agent_card/hot_reload.rs` were recorded. The
third, `crates/a2a-protocol-client/src/auth.rs:277`, was not, so the client's
own test binary had the same hole and nobody knew. `grep -rn 'set_hook'
--include='*.rs'` over the whole tree is what found it; the original pass had
looked only where the symptom appeared.

**The proposal — move those tests into their own integration-test binary —
would not have worked, and rested on a false premise.** Both hot-reload tests
reach `handler.card` and the client test reaches `store.inner`, all private,
so an integration test in `tests/` cannot see them without making internals
public. That trades a real encapsulation boundary for a cosmetic one. And the
premise, that the hook swap "keeps the output clean", is false: libtest
captures panic output per test and discards it when the test passes, so an
expected panic in a passing test prints nothing whether the swap is there or
not.

**What shipped is the deletion**, at all three sites, plus
`scripts/check_panic_hooks.sh` to keep them deleted. Measured rather than
argued, on the pattern in isolation — an unrelated failing test in the same
binary:

```text
with the hook swap:     marker LOST in 3 of 3 parallel runs
without the hook swap:  marker present in 3 of 3
expected panic's text:  absent in 3 of 3 either way
```

So the swap suppressed nothing libtest was not already suppressing, and cost
every other test in the binary its failure message to do it.

The gate is a grep, deliberately: "some other test lost its message" is not
observable from inside the test that lost it, so there is no runtime assertion
to write. It strips line comments before matching, so the explanatory comments
now at the three sites are not findings. Registered in `ci.yml`'s Format job,
paired with an injection in `scripts/prove_gates_fail.sh`, and listed in the
gate-reachability input table. Proven three ways before shipping: exit 0 on
the fixed tree; exit 1 naming all six lines on the tree as it stood; and
`prove_gates_fail.sh --only check_panic_hooks` reports **PROVEN**, "gate
exited 1 citing the injected defect", with the tree clean afterwards. The
harness now counts 66 gates rather than 65.

**Re-measured: the three gates report PROVEN.** This was recorded here as the
one inference in the section and is now a measurement.
`prove_gates_fail.sh --only 'test -p a2a-protocol-server --features'` on a
clean tree selects nine gates and reports **9 proven, 0 unproven**, each
"gate exited 101 citing the injected defect", tree clean afterwards:

| gate | |
|---|---|
| `--features sqlite` | PROVEN, 26 s — was INCONCLUSIVE |
| `--features postgres` | PROVEN, 23 s — was INCONCLUSIVE |
| `--features auth-jwt` | PROVEN, 21 s — was INCONCLUSIVE |
| `--features tls-rustls` | PROVEN, 33 s |
| `--features axum` | PROVEN, 22 s |
| `--features auth-jwt,tls-rustls` | PROVEN, 24 s |
| `postgres_store_tests --ignored` | PROVEN, 10 s — was PRE-BROKEN |
| `multi_replica --ignored` | PROVEN, 7 s — was PRE-BROKEN |
| `rate_limit::shared --ignored` | PROVEN, 11 s — was PRE-BROKEN |

So the panic hook was the whole of the INCONCLUSIVE verdict, and a local
PostgreSQL was the whole of those three PRE-BROKEN ones.


### Trace context is carried now

Finding 6 above — "there is no tracing at all, and for *this* protocol that
is the biggest gap" — is half answered, and the half matters because the two
claims were being run together everywhere.

**What shipped: propagation.** `a2a_protocol_types::trace_context` holds the
W3C wire format; the server parses an inbound `traceparent`, advances the
span and exposes `RequestContext::trace_context()`; the client gains
`TracePropagationInterceptor` plus a `CurrentTrace` task-local. A delegation
chain now shares one trace id, and because `traceparent` is a wire format
rather than a Rust type, it shares it with the Python, JavaScript, Go and
Java agents the ITK already runs.

**What did not: span export.** The `otel` feature is still metrics-only, with
no `TracerProvider`. Nothing records a duration or a parent/child edge. The
cross-language *trace conformance result* this file named as the prize is
therefore still unbuilt — but its precondition now exists, which it did not
before.

**Three design calls, each of which could have gone the other way.** The
server propagates and never invents, so `None` is evidence about the caller
rather than a hole in the plumbing — the alternative, minting a root
server-side, makes every request traced and makes `Option` meaningless. A
malformed `traceparent` is dropped rather than repaired. An explicit header
on a request beats the ambient scope.

**The `CurrentTrace` task-local lives in the client crate, not the server**,
because `a2a-protocol-server` does not depend on `a2a-protocol-client` and
should not start to. The consequence is that an executor opts in explicitly
with one `CurrentTrace::scope` wrap rather than propagation being automatic.
That is the honest trade and it is documented on the type; anyone tempted to
make it implicit should check that dependency direction first.

**Three documents had to be corrected in the same change**, all of which had
been corrected *to* their previous wording on 2026-09-19:
`otel/pipeline.rs`, `book/src/deployment/observability.md` and the
`docs/rust-sdk-assessment.md` comparison row. Each said some version of "no
`traceparent` anywhere in the workspace". The lesson worth keeping is that
"no spans are exported" and "trace context is not carried" are different
claims, and writing them as one sentence is what made all three go stale at
once.

### Deferred by the 2026-09-19 observability review

The large items are argued in *What to build next, ranked* above and are not
repeated here. These are the small ones that would otherwise have no home.
Everything in this list was found and deliberately **not** changed, so that
the documentation fix and the behaviour change stay separable.

**Breaking — needs its own release and an upgrade note.** The catalogue in
`book/src/deployment/observability.md` is published as a contract, so each
of these renames something a user's dashboards already select on:

* Units to UCUM: `("request")` → `("{request}")` and the five others.
  Measured to change nothing in the Prometheus exposition (see finding 4),
  so this is for metadata correctness and for exporters that behave
  differently, not for the metric names.
* `a2a.server.latency` → `a2a.server.request.duration`. This one *does*
  change the Prometheus name, to `a2a_server_request_duration_seconds`,
  which is what an OTel dashboard template looks for.
* `.requests` / `.responses` / `.errors` → one counter with an `outcome`
  attribute, per convention. Note this loses the requests-minus-responses
  gap the current split is there to expose, so it is not a pure win; decide
  deliberately.
* `init_otlp_pipeline`'s `service_name` parameter, so `OTEL_SERVICE_NAME`
  can win as the specification requires — `Option<&str>`, or drop the
  parameter and let `EnvResourceDetector` supply it.

**Non-breaking, small, and independently useful — all three shipped.** The
`# Construction` sections are on `AgentCard`, `AgentSkill` and
`AgentInterface`; `Message` has `new`, `user`, `agent`, `user_text`,
`agent_text` and `with_*` for all five optional fields; `Task` has `text` and
`texts`. `MessageSendParams` turned out to have no `impl` block either and got
`new` plus three `with_*`. See the section below.
