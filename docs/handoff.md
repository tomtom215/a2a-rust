<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Handoff — session state

Working state between sessions: which branches exist and why, what is in flight
outside this repository, and what the next session should pick up.

This is **not** `ROADMAP.md`. That file takes only items the repository has
committed to and refuses speculative milestones; this one records where things
stand, including decisions to *not* do something. When an item here becomes work
the repository commits to, move it there and delete it here.

Last updated 2026-10-14 — `claude/funny-heisenberg-e6tl83`: main's three red
workflows (docs site, OSV-Scanner, the rate-limit benchmark) and Dependabot
#169. Before that, 2026-10-12 — `claude/tender-cray-bsfw83`: the two spec MUSTs
left after #165, OpenTelemetry 0.33, a versioned documentation site, the
weekly mutation sweep, the ledger's last GAP (§3.3.4), push timeouts,
JSON-RPC `RetryInfo`, §13.1 caller isolation by default, Dependabot #166
and #167, §8.1's required agent card, the opt-in push and authentication
hardening (§13.2, §13.4), and §3.5.2's lag catch-up. `git log -1 --format='%ci %s' --
docs/handoff.md` remains the authority on this line.

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
| `claude/pensive-allen-socw7b` | merged, still present | **Merged as `fa2e901` via [#141](https://github.com/tomtom215/a2a-rust/pull/141) on 2026-09-23.** The adopter audit and phase 1 of its fixes; see its section below. Safe to delete. |
| `claude/determined-galileo-rywiyj` | merged, still present | **Merged as `8a54d7e9` via [#142](https://github.com/tomtom215/a2a-rust/pull/142) on 2026-09-23.** The adopter audit's open work after phase 1; see its section below. Safe to delete. |
| `claude/keen-noether-q73ekn` | merged, still present | **Merged as `0b7e87c2` via [#143](https://github.com/tomtom215/a2a-rust/pull/143).** The audit's gate gaps (escape classes 1, 6, 7, 8, 9), then phase 2 of observability. Safe to delete. |
| `claude/peaceful-ptolemy-noka5b` | merged | **Merged as `10f3435` via [#144](https://github.com/tomtom215/a2a-rust/pull/144).** N21 (the maintainer chose the admission side), the adopter's four reports, the claims ledger, and the drop-path hunt (N24–N32, N16, N18). This row said "open — no PR yet" until 2026-10-02; `git log --oneline --merges | grep 144` settles it. See its section below. |
| `claude/clever-curie-wbfxzc` | merged | **Merged via [#147](https://github.com/tomtom215/a2a-rust/pull/147) (`d4f32eb`) and [#148](https://github.com/tomtom215/a2a-rust/pull/148) (`775fe46`).** The 0.14.1 release for GHSA-hr9h-6jvf-wvg6 and its provenance re-measurement. It had no row here. |
| `claude/eager-galileo-h4x21s` | merged | **Merged as `7ce2d24` via [#149](https://github.com/tomtom215/a2a-rust/pull/149).** The 2026-10-02 SDK comparison, the in-memory store's TTL-sweep fix (`831b8ef`), `examples/swarm`, and `docs/swarm-orchestration.md`. This row said "open — no PR yet" until 2026-10-06; `git log --oneline --merges | grep 149` settles it. See its section below. |
| `claude/zealous-lovelace-n5ulfc` | merged | **Merged as `dbe63687` via [#162](https://github.com/tomtom215/a2a-rust/pull/162).** Telemetry, Langfuse, the span conventions and client spans. This row said "open — no PR yet" until 2026-10-14. See its section below. |
| `claude/bold-gauss-9qc9dc` | merged | **Merged as `f824b035` via [#163](https://github.com/tomtom215/a2a-rust/pull/163).** Compliance, audit and human oversight. It had no row here. |
| `claude/nifty-fermat-roj04f` | merged | **Merged as `6ce5dc9a` via [#165](https://github.com/tomtom215/a2a-rust/pull/165).** The spec ledger. It had no row here. |
| `claude/tender-cray-bsfw83` | merged | **Merged as `2b0a3abd` via [#168](https://github.com/tomtom215/a2a-rust/pull/168) on 2026-10-14.** Items 1-107 of its section. It had no row here. |
| `claude/funny-heisenberg-e6tl83` | open | **Destined for `main`.** Started from `2b0a3abd`: main's three red workflows first, then the spec, the EU AI Act and the rest. See its section below. |

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

*(Explained 2026-09-23: that FATAL line was the service's own health check.
`--health-cmd pg_isready` ran as the container's `root` with no `-U`, and
each probe logged one FATAL line every 5 s while still passing. Every
workflow now probes as `postgres` (audit N5), which removes the flood; whether
the API window then reaches the mutation output has not been re-measured.)*

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
    --test-tool=nextest --profile=mutants --all-features \
    -- --run-ignored all \
       -E 'not (binary(soak) or binary(soak_multi_replica) or binary(swarm_scale))'
  ```

  *(Corrected 2026-09-23: this recipe omitted `binary(swarm_scale)`, which
  `mutants.yml` excludes. Without it the server's unmutated baseline runs the
  ignored swarm-scale load experiments in parallel and failed here on
  `cost::a_channel_gets_slower_as_it_ages` — exit 4, "no mutants were
  tested" — which reads as a broken tree. See audit N14.)*

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

**What CI found on the PR that local runs did not** (fixed in the commit
after `c533e5e`):

- **Clippy 1.98 and nightly lints.** CI's stable is 1.98; this environment
  had 1.94, whose clippy lacks `while_let_loop`. Nightly adds
  `assert_is_empty` and deprecates `fetch_update`, whose replacement
  `try_update` is beyond the 1.88 MSRV, so the test store uses a
  compare-exchange loop instead. Run `rustup update stable` before trusting
  a local clippy, and `cargo +nightly clippy` too, since the nightly job
  stops at the first failing crate and hides the rest.
- **The official a2a-tck rejected the S2 fix.** STREAM-SUB-003 and
  STREAM-SUB-004 regressed on JSON-RPC, because the TCK reads any
  `text/event-stream` answer as success. The server's pre-stream error is
  plain JSON again, and a2a-go's loss of it is pinned. See the audit's OW13.
  The Go gate alone had graded the S2 fix as correct: a fix aimed at one
  peer has to pass every conformance suite, not just that peer's.

**Merged 2026-09-23 as `fa2e901` (#141).** `git log --oneline fa2e901^1..fa2e901^2`
lists its 49 commits. The watch-the-PR step that stood here is done; the work
that follows it is on `claude/determined-galileo-rywiyj`, whose section says
what the next session should do first.

## `claude/determined-galileo-rywiyj` — the audit's open work after phase 1

Started 2026-09-23 from `fa2e901`. The plan is
[`adopter-audit-2026-09-22.md`](adopter-audit-2026-09-22.md): its **Open
work** entries, then phase 2 (observability; design in
[ADR 0013](adr/0013-observability.md), status Proposed), then OW11 and
phase 3. Audit rows are marked **[Fixed: …]** as commits land.

**Baseline on `main` (`fa2e901`), before any change here:**
`scripts/preflight.sh --full` ran 71 of 71 CI gate commands, all PASS
(exit 0). The official a2a-tck (`263b9cf`, run as `official-tck.yml` runs
it): full profile 88 MUST graded with 4 failing, exactly the baseline;
minimal 66 graded, no regressions; extension profile CORE-CAP-004 passes.
`cargo +nightly clippy` (1.100, 2026-09-22) exits 0 with manifest warnings
(audit N6). `cargo deny check` passes. `cargo semver-checks` against the
published 0.13.0 with all features: 196 checks pass per crate, no update
required — which is what exposed N7.

**Landed (each fix reproduced first, then mutation-tested):** OW3
(`f7956d10`), OW5 with the slimrpc half (`2b1bb80a`), OW7 (`af60cece`), the
mutation gate's feature flag (N8, `587418fa`), the PostgreSQL health check
(N5, `d73cf871`), the 0.13.0 CHANGELOG correction (N7, `ff945d2f`), N1 — the
HTTP server's shutdown at the connection ceiling (`50753ef1`) — and OW6,
`serve_with_shutdown` for gRPC and WebSocket (`e5ee1518`). Audit updates:
`f81b2ed4` and the commit after `e5ee1518`.

**Verification of record, at `e5ee1518`:** `scripts/preflight.sh --full`
ran 71 of 71 CI gate commands, all PASS (exit 0). `cargo +nightly clippy
--workspace --all-targets --all-features` exits 0; `cargo deny check`
passes. The official a2a-tck at `263b9cf` (upstream `main`, unchanged since
2026-09-01; the newest tag, `1.0.0.alpha2`, is 14 commits behind it): full
88 graded, 4 failing = baseline; minimal 66 graded; extension CORE-CAP-004
passes — identical to `main`. cargo-mutants over the whole branch diff
(`git diff fa2e901...HEAD`, `--all-features` given to cargo-mutants, live
PostgreSQL): server 44 mutants, 16 caught, 28 unviable, 0 missed; client 22,
6 caught, 16 unviable, 0 missed; the slimrpc binding's 5, run separately, 4
caught, 1 unviable. Most of the unviable are N9's alias replacements and
`Default::default()` for types with no `Default` — read the OW6 entry in the
audit before taking "0 missed" as full coverage.

**Lessons from this session, each of which cost something:**

- **Read a mutation result's logs, not only its counts.** Every "caught"
  in feature-gated code was suspect until N8 was fixed: 8 of one diff's 14
  were mutants that did not compile. With `--all-features` given to
  `cargo mutants` itself, CI and a local run grade the same thing. And a run
  where every mutant is unviable is not a pass: cargo-mutants generates only
  uncompilable replacements for functions returning `ClientResult` or
  `A2aResult` (N9), so such code's coverage is its tests alone.
- **Check a published artifact from its own registry directory.** The first
  look at "the published 0.13.0" read `~/.cargo/registry/src/-69e9…`, the
  local registry `cargo package` builds during preflight — its
  `.cargo_vcs_info.json` named today's `main`. The crates.io copy is under
  `index.crates.io-…`.
- **`cmd | tail` reports tail's status.** It happened again here and was
  caught before it was reported; capture the exit code first.
- **Disk.** A full `--full` run peaks near the allowance with the slimrpc
  binding. Once the binding gates start, every remaining gate builds
  `--release`, so `target/debug` (12 GB at that point) can be deleted
  mid-run.

**What the next session should do first:** phase 2, gate first, as
[ADR 0013](adr/0013-observability.md) describes: an end-to-end test with an
in-memory span exporter and a manual metric reader that fails on this tree,
then the work that turns it green. N9 (1,000 `*Result`-alias mutants that
never compile) is the most consequential open gate gap; measure how many
survivors spelling those return types as `Result` would surface before
deciding.

**Merged 2026-09-23 as `8a54d7e9` (#142).** `git log --oneline 8a54d7e9^1..8a54d7e9^2`
lists its 13 commits. Both steps above were taken on
`claude/keen-noether-q73ekn`, whose section follows.

## `claude/keen-noether-q73ekn` — the gate gaps, then phase 2

Started 2026-09-23 from `8a54d7e9` (#142 merged), rebased 2026-09-24 onto
`638ff9a0`. The work order was the audit's escape classes first, each new
gate proven able to fail, then phase 2. No head SHA, for the reason the
sections above give.

**Baseline on `main` (`8a54d7e9`), before any change here:**
`scripts/preflight.sh --full` 71 of 71 PASS (exit 0); `cargo deny check`
exit 0 with one warning (N17); a2a-tck `263b9cf` full 88 graded, 4 failing =
baseline, minimal 66, CORE-CAP-004 passes; nightly clippy exit 0 (N6
warnings); `cargo semver-checks` "no semver update required" ×4, and an
injected rename exits 100.

**Landed, oldest first:** release gates (class 9, N10; `e2a2b7ed`); RFC 8785
vectors and the signing fixes (class 7, T2, T4 tie, N12; `1a8c4739`); fuzz
targets for every server parser of peer input (class 8; `fe0ba1d3`,
`0aacbe9a`); crate READMEs as doctests and feature tables checked (class 1;
`499cfb2b`); `ScriptedPeer` and N13 (class 6, E6; `a7a34ff4`); the book's
examples compiled and its defaults checked (class 1, N15, S12; `1e5ba59b`);
N17 (`b1f9041b`); N9's survivors (`02fc596d`); `tracing` on by default, and
the SDK's defaults its own (O13, K1; `3df532d9`, breaking); tests for every
body cargo-mutants can replace, stock and patched (`d7afef3a`, `d6259ff3`,
`ac13aca6`); the pinned, patched cargo-mutants in CI (N4, N9, N11;
`b0ebedc9`); N20 (`6e68d76a`); N19 (`d1faeed3`); phase 2 — spans on every
binding, `rpc.server.call.duration`, connection statistics (O1, O2, O4, O5,
O10, most of O11; `4426d3d2`); N21 recorded (`390f8f6b`); N22, a WebSocket
stream that went silent when its client was dropped (`cb5d81d3`). Then the
fixes the pull request's CI found: the mutants install self-test under
forced colour (`384b4142`), `book_defaults` on a CRLF checkout
(`1b49410d`), N23, a card poll watcher blind to a change made as it
started (`4c312e7a`, and its test given an mtime of its own for Windows,
`be061ae2`), and tests for nine mutation survivors in phase 2's server
code (`84ea735b`).

**Phase 2's gate** is `crates/a2a-protocol-sdk/tests/observability_e2e/`.
It failed on `main` and passes here 10 of 10. During development each of
its assertions was probed by injecting the defect it names (removing `error.type` from the
metric or the span, the connection tracker, the executor's task id, the
JSON-RPC code mapping, or building the SSE response outside the call's
span): each fails it with the matching gap. It found two defects in this
work before commit — a root span per SSE stream, and `Drop` still recording
child spans. The semantic conventions were re-read upstream; ADR 0013
records the three rules its list lacked.

**Phase 2's cost** is measured in ADR 0013's Consequences. With no
subscriber, JSON-RPC is within noise and HTTP+JSON +8 to +10%; with a
`tracing-opentelemetry` layer, about 100 µs a call, the bridge recording two
spans. The first cut was +36% and +46% with no subscriber, from two causes
the measurement found — a body wrapper that turned every fixed-size response
chunked, and a boxed future per call — both fixed before commit.

**Verification of record, at `cb5d81d3`:** `scripts/preflight.sh --full`
ran 73 of 73 CI gate commands, all PASS (exit 0), with `SPIRE_BIN_DIR` set
for the SPIFFE suites. `scripts/prove_gates_fail.sh` proved all 73 gates
able to fail: 70 in one run at `390f8f6b`, and at `cb5d81d3` the three it
graded PRE-BROKEN there — the SPIFFE suites (no `SPIRE_BIN_DIR` in that
shell), the binding's clippy (its target deleted mid-run; see the lessons),
and `cargo test --workspace --all-features` (N22, fixed in between).
`cargo +nightly clippy --workspace --all-targets --all-features -- -D
warnings` (1.100.0-nightly, 2026-09-22) exits 0 with N6's manifest warnings
and no others; `cargo +nightly test --workspace --all-features` exits 0,
4,063 passed across 148 test binaries. `cargo deny check` exits 0 for the
workspace and the binding, N17's warning gone; `cargo semver-checks` "no
semver update required" ×4. The official a2a-tck at `263b9cf`: full 88
graded, 4 failing = baseline; minimal 66; CORE-CAP-004 passes — identical to
`main`. cargo-mutants (the pinned, patched build, `--in-diff` over this
branch's source diff, `--all-features`, live PostgreSQL) for the client: 75
mutants, 60 caught, 15 unviable, 0 missed; the 15 are all
`Default::default()` for types with no `Default`. The server's 344 and the
types' 23 in-diff mutants were not run at this head locally; the pull
request's `mutants.yml` shards run them and are the record for those
crates. The fixes after `cb5d81d3` were checked by the targeted runs
their commit messages describe, not by a second full preflight; the pull
request's CI is the record for them.

**Open, from this branch:** N21, N16 and N18 were closed on
`claude/peaceful-ptolemy-noka5b`; the HTTP+JSON residual above,
unattributed below the bench's noise; `connection_timeout`, still measured
by no test; the cargo-mutants patch (kept here, not upstreamed — see `scripts/cargo-mutants/README.md`).

**Lessons, each of which cost something:**

- **An example that does not compile is usually also wrong about
  something else.** Converting the book's 127 blocks found five compile
  errors and, around them, seven prose claims the code contradicts,
  including a default (`cancel`) that changed in 0.7. The compiler is only
  the first reader.
- **`compile_fail` needs a control.** A block marked `compile_fail` passes
  for any error, including one in the hidden preamble. Pin the error code
  and run it under nightly rustdoc (stable does not enforce codes), with a
  deliberately wrong code as the control that must fail.
- **Count before writing a number.** A CHANGELOG draft said "36 of them
  run"; the diff says 79. Numbers in records come from a command.
- **A cost model that divides a run's total evenly assumes each unit's cost
  is independent of order.** cargo-mutants' build cost is not: dealing a
  PR's mutants round-robin across crates made its slowest shard take 110
  minutes where the even split said 77 (item 107). Quote a projection as one,
  and replace it with the first run's measurement.
- **A mutant a crate's own tests cannot reach is a coverage fact about that
  crate.** cargo-mutants runs only the mutated crate's tests: the client's
  gRPC success path was covered from the server and SDK crates and
  therefore, for mutation purposes, not at all. It happened again with
  phase 2: nine server survivors, every one covered by the SDK's
  `observability_e2e` and by nothing in the server crate. A doctest does
  not count either: CI runs the mutants under nextest, which runs none
  (item 106).
- **Measure the cost before calling observability done.** The spans were
  green and correct, and a default build had become 36–46% slower per call.
  Clippy's `large_futures` had pointed at one of the two causes; the bench
  found the other, which no test or lint would have.
- **A gate that reads the exporter can wait for the wrong thing.** `tracing`
  holds a parent span open until its children close, so a call's span is
  exported late. Waiting a fixed time for spans is a flake; waiting, with a
  deadline, for every span's parent to arrive is not.
- **`pgrep -f` / `pkill -f` match the shell running them.** A pattern
  quoted in the command line kills that command. Start helpers from a script
  and stop them by PID.
- **PRE-BROKEN on a timing-sensitive test is a finding until shown
  otherwise.** `prove_gates_fail.sh` graded the workspace tests PRE-BROKEN
  on one WebSocket assertion that failed about 1 run in 80 under load. It
  read like a flake; timestamps on the reader's polls showed a real client
  bug present on `main` (N22). The bound the test raced was incidental; the
  silence it caught was not.
- **Local is not CI.** Three failures reached the first CI run that no
  local gate could show: a self-test grepping output that CI colours
  (`CARGO_TERM_COLOR=always`), a test that finds headings by `\n` on a
  Windows checkout's CRLF, and a macOS runner slow enough to expose N23.
  Run text-matching checks with colour forced, and a file-reading test
  against a CRLF copy, before calling them portable.
- **`prove_gates_fail.sh` shares one target directory.** It exports
  `CARGO_TARGET_DIR=<repo>/target`, so the binding gates build there too:
  deleting `target/debug` mid-run, which is safe during preflight's
  release-only tail, broke a binding gate here, and the prover rightly
  graded it PRE-BROKEN. Free space before the run, not during it.
- **A mutation baseline is a load test.** It runs `swarm_scale`, which
  depends on scheduling (N21). A failed baseline on a busy host is a fact
  about the host and that race, not about the change; re-run idle or
  compare against `main` on the same host before believing either.

**What the next session should do first:** if a pull request is asked for,
open it and watch the mutation job for N21. Then the rest of phase 2 —
ADR 0013's option 5 (`init_telemetry`, OTLP traces; O3, O12), stream
duration and active streams (O6), the task-outcome metric (E5, with O11's
remainder), the client side (O8, O9), connection statistics for the gRPC
and WebSocket listeners (O10) — then OW11 and phase 3.

## `claude/peaceful-ptolemy-noka5b` — from "passes CI" to evidence

Started 2026-09-24 from `0b7e87c2` (#143 merged). Order of work: a baseline
on clean `main`; N21, on the side the maintainer chose (admission waits,
bounded); the v0.13.0 adopter's four reports; a claims audit of every
public statement from 0.6.0 on (`docs/claims-ledger-2026-09-24.md`); then a
hunt through drop and cancellation paths, which found eight more defects,
all present on `main`. Every finding has an entry in
`docs/adopter-audit-2026-09-22.md` and a regression test.

**Baseline on `main` (`0b7e87c2`), measured before any change here:**
`scripts/preflight.sh --full` 73 of 73 PASS, "All gates passed."; `cargo
deny check` exit 0 for the workspace and for the binding; a2a-tck `263b9cf`
— full profile 84 PASS, 4 FAIL; minimal 63 PASS, 3 FAIL; CORE-CAP-004
passes; every gate exits 0. **Not run on `main`:** nightly clippy and
tests, `cargo semver-checks`, `prove_gates_fail.sh`.

| N | What | Severity |
|---|---|---|
| N21 | a continuation refused while a parked turn's executor returns | Medium |
| N24 | the manifests admitted versions under RustSec advisories (adopter #4) | Medium |
| N25 | an idle SSE subscription never ended: each keep-alive reset its bound | Medium |
| N26 | a send dropped mid-commit wedged its task | High |
| N27 | a blocking send whose client left: the task stayed `working` | High |
| N28 | a failing `after` hook orphaned the running task | Medium |
| N29 | one unread WebSocket stream stalled every call on its socket | Medium |
| N30 | a WebSocket peer that stopped reading was never closed and kept its slot | Medium |
| N31 | a cancelled gRPC stream kept its subscription while the task was quiet | Low |
| N32 | the card poll watcher skipped a fixed card after a failed parse | Low |
| N33 | JSON that is not a Request object answered -32700, not -32600 | Low |
| N34 | a part in a media type the card does not declare reached the executor | Medium |
| N35 | task statuses served without a timestamp | Low |
| N36 | a refused credential answered `400`; the client never dropped a revoked token | Medium |
| N37 | the axum adapter's errors were not AIP-193 | Low |
| N38 | the axum adapter's successes carried no `A2A-Version`; `application/json` kept by choice | Low |
| N16 | missing constructors; `CancellationToken` re-exported | Low (API) |
| N18 | `WebSocketTransport` now reconnects, single-flight | Low (client) |

The adopter's reports: #1 (old cargo, misleading error) reproduced — cargo
1.80.1 and 1.84.1 fail, 1.85.0 resolves; the cause is edition 2024, not
resolver 3 — and the README says so. #2 (short git rev) did not reproduce on
cargo 1.88, 1.96 or 1.98. #3 is the CHANGELOG's new "Behaviour Changes"
heading. #4 is N24, and `scripts/check_advisory_floors.py` is a new CI gate
for it; on `main` it exits 1 with 13 requirement/advisory pairs.

The binding's RUSTSEC-2026-0285 waiver was re-checked and is still blocked:
`slim-auth` 0.16.0 reaches `aws-lc-rs =1.16.3` through
`mls-rs-crypto-awslc` 0.25, and `slim-rpc` 2.3.3 requires `slim-auth ^0.16`
while the binding pins 0.15.

**Lessons:**

- **The drop path is where the bugs were.** N25–N31 are each a future
  dropped at an await its author did not treat as a stopping point. N26–N28
  rest on the premise that hyper drops a request's future when the client
  disconnects; `tests/interceptor_on_complete.rs` now pins it for JSON-RPC
  and HTTP+JSON on a real socket. gRPC and WebSocket reach the same handler
  methods but have no disconnect test of their own.
- **A test that sleeps between turns hides the race it should catch.**
  `a_continuation_keeps_what_earlier_turns_wrote` slept 50 ms, which is why
  N21 showed only on a busy host. The new tests wait on conditions.
- **preflight's disk estimate was stale.** CI runs with
  `CARGO_INCREMENTAL=0`, which `dtolnay/rust-toolchain` sets. Locally,
  incremental state grew to 16 GB in one baseline. preflight now sets the
  same variable.
- **A private field can break the public API.** N18's reconnect lock, a
  `tokio::sync::Mutex<()>`, made `WebSocketTransport` stop being
  `RefUnwindSafe`; `cargo semver-checks` caught it (exit 100,
  `auto_trait_impl_removed`) where no test or lint did. The field is now
  `AssertUnwindSafe` — true, since it guards `()` — and a compile-time
  assertion pins the type's auto traits.
- **A manual build left behind fills the disk under the next gate.**
  `package_binding.py`, run by hand, left 6.8 GB in
  `bindings/a2a-protocol-slimrpc/target`, which nothing cleans. Full
  preflight #10 then failed two gates (exit 1 and 101) with ~24 GB of
  build output against a ~25 GB allowance; a full disk is the likely
  cause, not a confirmed one — the log was lost to a container restart
  before its errors were read. Delete a manual build's target directory
  before starting the next long run, and read `df` first.
- **The `pkill -f` lesson above did not stick.** It killed its own shell
  at least three more times on this branch. Stop helpers by PID, found with
  `ps -eo pid,args | awk '$3 ~ /name/'`; a pattern like
  `pgrep -f "name[.]sh"` does not match its own command line.
- **A release gate's healthy fixture can be the version, not the gate.**
  `prove_workflow_gates_fail.py` tagged its healthy release at the
  version the crates declare; bumping to 0.14.0 before release prep made
  that a release with no notes, and two probes went INCONCLUSIVE. Both
  release fixtures now key off the newest dated CHANGELOG heading
  (`37320002`).
- **A spec SHOULD is not the only reader of the wire.** N38's switch to
  the spec's media type passed every test here and ACTS, and broke the
  official Go client; only the cross-SDK interop gate caught it. Run
  `go_sdk_interop.sh` before pushing any change to HTTP+JSON headers.
- **Re-run the static gates after the last change, not before it.** The
  first branch preflight failed four gates, all introduced by this branch's
  own later edits: a file over the length limit, an API-reference row, a
  field dead without `tracing`, and an example that timed out because a
  local model server was contending for CPU.

**ACTS, the official conformance suite** (a2aproject/a2a-itk `429945f6`,
`run_acts.py --mount itk --transport all`). N33–N38 are what it found in
the SDK. The rest of the gap to the official Rust SDK's agent, which was
CONFORMANT on the same suite when ours scored 85/75/74 per binding, was the
ITK agent: the `tck-*` behaviours, the client-parse fixture, and the
reduced-capability and auth passes (`itk/src/acts_modes.rs`, whose guard
refuses through the SDK's own `BearerTokenAuthInterceptor` and
`A2aError::permission_denied`, so the auth tests grade what an adopter
gets). At `9496d1b1`: JSON-RPC 101/101, gRPC 88/88, HTTP+JSON 91/92, every
MUST passing, CONFORMANT overall; the one failure was REST-CT-001 (a
SHOULD: `application/a2a+json` on HTTP+JSON responses, §11.1). I first
recorded it as ACTS contradicting the spec; §11.1 says otherwise, and the
code had cited the 2026-03-31 snapshot. `dfc69ed2` switched to the A2A
media type and ACTS went to 92/92, but CI's `go_sdk_interop.sh` failed:
a2a-go's client reads HTTP+JSON errors only under `application/json`. The
switch was reverted and the deviation documented (N38 in the audit); the
official Rust SDK fails REST-CT-001 the same way. So REST-CT-001 fails by
choice until a2a-go accepts both. Build the ITK
and run ACTS with the workspace `target/` cleared: the two together do not
fit the disk (see Lessons).

`ServerInterceptor::on_complete` and `CallOutcome` were added at the
maintainer's request: one call per interceptor whose `before` ran, with
`Succeeded`, `Failed(&ServerError)` or `Cancelled`, and no effect on the
response. Every `RequestHandler` method runs its body between
`interceptors.begin(&call_ctx)`, `call.before()` and `call.finish(result)`
(`src/interceptor/completion.rs`), and all four bindings reach the ten
handler methods and nothing else (read, not tested, for gRPC and
WebSocket). Three mutations of the guard (no drop path, decrement after
the await, count `before` only on success) each fail at least one of the
new tests.

A first version took the body as an argument, `intercept(ctx, body)`, and
workspace clippy failed `large_futures` on every send path at about 30 KB:
an `async fn` stores its argument and again the future it awaits. Measured,
then designed around; the module comment records it. The unit and
integration tests had passed on that version; only clippy caught it, so
run clippy before calling a change on the send path done.

**Open, from this branch:**

- task #16, CI: pin the official suites, a lightweight daily canary, an
  ACTS gate against a baseline, and spec/proto drift detection — the last
  is what N38 (and the push sender's Content-Type before it) needed. The
  ACTS gate must key on `run_acts.py`'s exit status (MUST-only, §12.7).
  REST-CT-001 no longer needs a baseline entry: it passes since 2026-10-08;
- N38's media type: **resolved 2026-10-08** by negotiation (see the
  `claude/bold-gauss-9qc9dc` section). An unconditional
  `application/a2a+json` default waits for an a2a-go release whose
  `isJSONErrorContentType` accepts it (on a2a-go `main` at `3e1aa7fa`, not in
  v2.6.0); `go_sdk_interop.sh` is the check;
- close Dependabot's #128: genai 0.6.5 landed on this branch with
  `deny.toml`'s first advisory ignore (RUSTSEC-2024-0436, `paste`), the
  maintainer's choice on 2026-09-25; 0.7.0-beta.24 was checked and still
  depends on `paste`. Drop the ignore when genai drops `paste`;

- WS3 — genai, rig and mcp over every binding with the real model, and why
  a WebSocket `SendMessage` timed out while a model server was busy;
- extended fuzz runs, including the new `client_peer_input` target;
- a flake hunt under CPU load;
- `swarm_scale` under load, `main` against this branch, for N21;
- in-diff mutation runs;
- WS4 measurements against `docs/readiness-bar.md`, which defines the bar
  and records no measurements yet;
- WS5, a clean-`CARGO_HOME` walkthrough of the new Quick Start against
  crates.io 0.13;
- nightly clippy and tests, `cargo semver-checks`, and `prove_gates_fail.sh`
  on this branch;

Carried over, unchanged: `connection_timeout` is measured by no test; the
HTTP+JSON telemetry residual; phase 2's remainder (ADR 0013), OW11 and
phase 3.

**Release:** the next release is 0.14.0 (`[Unreleased]` carries
`### Breaking Changes` from #141–#143), and `release.yml`'s cadence check
refuses a second breaking minor in the calendar month of 0.13.0
(2026-09-20): the earliest tag date is 2026-10-01. The crates are already
at 0.14.0 on this branch (`fd028712`, the maintainer's choice): N36 needs a
types API that `cargo package` could not find in the published 0.13.0.
Until the tag, `cargo semver-checks` reads 0.13 → 0.14 as allowed to break.
Release prep (notes, CITATION, SECURITY, ROADMAP, the provenance manifest
last) is still a second pull request (`RELEASING.md`).

**Verification of record, at `9496d1b1`:** GitHub CI green, 21 of 21 jobs,
including `Test (1.88, macos-latest)`, which had failed at `37320002` on a
graceful-drain test that slept instead of waiting for its request
(`ff63be5a`). The last full local preflight, at `fd028712`, passed 74 of
75 gates; the 75th was `prove_workflow_gates_fail.py`, fixed and re-run
alone in `37320002`. After that, CI at `20a6d817` failed two jobs: the
Go interop (N38's switch, reverted) and `Test (stable, windows-latest)`,
where `the_task_store_stays_bounded_under_sustained_load` failed its own
precondition because a slow runner sent too few requests in its fixed
window; the load loop now has a request floor. Neither fix has had a full
preflight; see the commit for what ran.

**Documentation audit, 2026-09-25.** Six read-only agents audited every
book page, the root, crate and binding READMEs, and `book/static`, each
finding with source evidence; each then applied its own verified fixes
within its pages, and the root README was reviewed by hand. It found one
code defect, fixed with a test that fails without it: the SLIMRPC binding's
server still answered a refused credential `INVALID_ARGUMENT` (N36's
residue). The largest gaps were an `upgrading.md` with no 0.13 → 0.14
section, a push-delivery label table that described `timeout_truncated` as
`skipped`, pages saying the executor timeout and `tracing` were off by
default, lag described as a silent gap (the stream ends), and benchmark
prose contradicting its own tables (fixed in `generate_book_page.sh`).
Verified after: `a2a-book-tests` 216 passed (every book and README block),
agent-team 102/102, workspace clippy, rustdoc `-D warnings`, 22 script
gates, the book built with mdBook 0.4.40 and `seo_postprocess.py --check`
clean on 65 pages, and the changed pages rendered and read.

**0.14.0 is prepared on this branch (2026-09-26), in one pull request.**
The maintainer chose to fold release prep into the content PR rather than a
second one, and to ship inside September under a declared cadence exception
(its wording is the maintainer's). The prep commit dates the CHANGELOG and
moves CITATION.cff, SECURITY.md, ROADMAP.md and the book's changelog; the
provenance manifest is regenerated in the commit after it. Both must stay
last: a later change to a packaged file needs the 0.14.0 notes touched after
it, and any change at all needs the manifest regenerated last. Merge with a
merge commit (as #143), tag `v0.14.0` (annotated) on it the same day, since
the docs site deploys from `main` and tells readers to install 0.14. Then
publish the SLIMRPC binding 0.6.0 by hand (`RELEASING.md`, "SLIMRPC").

The first Official TCK run on the PR (#144) failed on `CORE-SEND-003`, on all
three bindings under the full and minimal profiles. The cause is the suite,
not N34: the requirement declares no `expected_error`, so it demands that an
unsupported media type be accepted. It is baselined, with the evidence in
`docs/official-tck-findings.md` §22. The tally to quote is now 87 of 114
passing and 5 failing (README, ROADMAP, conformance history). The in-diff
mutation run on the same head found surviving mutants in
`status_stamp.rs`, `dispatch/grpc/helpers.rs` (two arms),
`messaging/admission.rs`, `dispatch/rest/error_response.rs` (eight
arms of `canonical_status_name`) and `agent_card/hot_reload.rs`; each now
has a test that fails under its mutation. The default
`ServerInterceptor::on_complete` survivor is equivalent and is skipped
with an ADR 0006 note. Those are packaged
files, so the 0.14.0 notes were touched in the same commit and the manifest
regenerated after it.

**What the next session should do first:** check CI on the branch head,
then the `swarm_scale`
comparison for N21 and WS3.

## `claude/eager-galileo-h4x21s` — the comparison, one fix, and the swarm

The maintainer asked for a fresh, non-partisan comparison against a2a-rs at
both projects' crates.io releases, with a live model, plus a start on swarm
orchestration. What landed and why is in the three documents rather than
here:

* [`sdk-comparison-2026-10-02.md`](sdk-comparison-2026-10-02.md) — results,
  with every figure tied to a file under `benches/sdk-comparison/results/`.
  Headline: conformance and interop are parity on the A2A project's own
  suites; a2a-rs is 1.17–1.62× faster per request; a2a-rust's memory is
  bounded and a2a-rs's is not; a2a-rust's default store leaked across
  tenants exactly as a2a-rs does (fixed since, below).
* `831b8ef` — the TTL pass walks a write-ordered index (+15–18% on the echo
  benchmark). Its CHANGELOG entry carries the numbers.
* [`swarm-orchestration.md`](swarm-orchestration.md) — gaps ranked by
  measurement, with `examples/swarm` (gated in `ci.yml`) as the evidence.

* `a57de29` — R1 decided (G3 option 2): a store that cannot isolate refuses
  any request naming a tenant. Breaking for deployments that relied on the
  leak; the CHANGELOG says how to migrate.
* `1c2ca83` — queue events behind `Arc`: bytes allocated per unary send
  173,278 → 82,844. No throughput change; none is claimed.
* G4 in `swarm-orchestration.md` attributes the remaining ~20% CPU gap to
  a2a-rs: not instructions (equal within 1% once `memcpy` is excluded) but
  kernel and allocator time.

**Tried and not landed: running a blocking send's executor in its
collector's task.** One task per send instead of two. Measured on 5
interleaved runs: 8,422 vs 8,465 rps, CPU 180.0 vs 185.6 µs/request with
overlapping ranges — no gain. It also made the bug below certain rather than
racy, so it was not committed. The patch was kept only in the session's
scratchpad; the design (a deferred executor whose `Drop` spawns it, a
`catch_unwind` poll wrapper, the result returned over a oneshot before the
executor finishes) is recoverable from this paragraph.

**Fixed 2026-10-03 (`ce9c533`): a blocking send could drop events.** Its
collector read the 256-slot broadcast ring; a burst faster than the
collector stored it lost the overflow (20 sends of 600 artifacts kept 255
to 600). It now reads the persistence channel. After: 600 of 600 in 60 of
60 sends.

**Throughput, where it stands.** `996a736` (synchronous writes, shared
`Arc` on the persistence channel): +4.9% on one core, nothing measurable
on two. G4 in `swarm-orchestration.md` has the one-core attribution: of
a2a-rust's 74–80 µs user CPU per unary request against a2a-rs's 37–40,
about 16 µs is evicting from the bounded default store, which a2a-rs does
not do. Method notes that cost this session time: measure on one pinned
core (`taskset -c 0`; tokio sizes its pool from the affinity mask), read
user/system time from `/proc/<pid>/stat`, and do not trust cachegrind for
time — it serialises threads and prices every instruction alike.

**The store cap was kept and eviction was not redesigned (2026-10-03).**
Under jemalloc eviction costs nothing measurable; the ~16 µs it seemed to
cost is glibc in a bounded heap (G4). The production recommendation is
jemalloc or mimalloc, now in `book/src/deployment/production.md`: +65%
unary throughput on two cores. `examples/deploy-agent` now runs on it via
`tikv-jemallocator` (maintainer's choice, 2026-10-03; not on MSVC).

**Throughput after the third pass (2026-10-03).** On jemalloc for both:
one core 1.13× (was 1.65×), two cores unary 1.09×, two cores streaming
1.21×. G4 has the table, the landed changes (`701b02a`, `774593c`) and
four layouts measured and rejected.

**The terminal gate stays as it is (2026-10-03).** Its 6.6% of two-core
streaming was examined both ways: a processor-side broadcast was rejected
for ordering and liveness risks, and sharing one task between executor and
processor was measured slower (G4). The remaining streaming gap is not
attributed further.

**What the next session should do first.** ~~G1-A (the client-side
delegation handle)~~ landed on `claude/bold-gauss-9qc9dc` (below); next in
`swarm-orchestration.md`'s order is G3.

**Environment notes that will cost the next session time.**

* Docker is installed but the daemon is not running; `dockerd &` starts it.
  The a2a-itk image needs the CA and proxy overlay from
  `benches/sdk-comparison/scripts/itk_dockerfile_overlay.py` in this sandbox.
* A 4-core sandbox fills its disk with three or four Rust target
  directories. This repository's own `target/` reached 19 GB once in this
  session; budget for it.
* `pkill -f <pattern>` matches the shell that runs it if the pattern appears
  in the command line. Use `pkill -x <name>` or kill by PID. (This session
  still did it twice.)
* Measure throughput on one pinned core with `/proc/<pid>/stat` user and
  system time; two-core runs are latency-bound and the VM's noise is ±5–10%
  there. Use 6 interleaved runs and compare medians.

## `claude/funny-heisenberg-e6tl83` — main green again, then v1.0.1 and the rest

Started from `2b0a3abd` (#168's merge). Commits are authored and signed off
as the maintainer with no `Co-Authored-By` or session trailers, by the
maintainer's instruction (DCO), as on `claude/tender-cray-bsfw83`.

**Step 0: three workflows were red on `main` at `2b0a3abd`.** Each was
reproduced locally before its fix.

1. `fix(docs):` Deploy Documentation, step `Versioned site is complete`:
   `dev/api/index.html: a non-root page must be noindex with no canonical`.
   `docs.yml`'s API step writes that page with no `</head>`;
   `make_noindex` removed its canonical link and added `noindex` only by
   replacing `</head>`, so the page had neither. Reproduced with the real
   build (mdBook 0.4.40, the four crates' rustdoc, the step body extracted
   from `docs.yml` and run as written): 794 book pages, 1 fault. A page
   with no `</head>` now gets the tag after its doctype; the same build then
   checks 0 faults. The prover's fixture for that check now runs
   `docs.yml`'s own API step (`API_STEP` in
   `prove_workflow_gates_fail.py`); its healthy case failed before the fix
   with five faults, the dev API index and the four crate indexes, whose
   fixture pages had no head either. This workflow runs only on pushes to
   `main`, so no pull request could have caught it.
2. `ci(osv):` OSV-Scanner, job `Rust lockfiles`: RUSTSEC-2024-0449
   (unsafe-libyaml 0.2.11) unignored in the binding's lockfile, reproduced
   with osv-scanner 2.6.0 (exit 1, the same table). #168 waived it in the
   binding's `deny.toml` and not in its `osv-scanner.toml`; it is mirrored
   now, and all eight lockfiles scan clean with the workflow's arguments.
   The scheduled runs on `f3fdddc9` (before #168) were red on it too. The
   "unused ignore" warning for RUSTSEC-2026-0285 is osv-scanner matching
   the advisory by its GHSA id; both entries stay, and the file says why.
   On a pull request the blocking job was skipped and only the
   new-findings job ran, so a finding already on `main` failed nothing;
   `rust` now runs on pull requests too, as cargo-deny does. Found while
   checking the waiver: agntcy-slim-config 0.17.3 (under agntcy-slim-rpc
   3.3.1) replaces serde_yaml with yaml_serde, and agntcy-slim-auth 0.17.x
   takes regorus 0.12.0, so the SLIM 3.x upgrade (audit register R13)
   removes unsafe-libyaml. It does not clear RUSTSEC-2026-0285:
   mls-rs-crypto-awslc 0.25.0, the newest, still pins aws-lc-rs =1.16.3.
3. `fix(bench):` Benchmarks, `enterprise_scenarios`: `with_rate_limit`
   panicked at "rate limit exceeded: 100000 requests per 60 seconds".
   Criterion sizes its sample from the measured speed: 86k iterations at
   95.5 us a send on `2b0a3abd` against 40k at 207 us on `6ce5dc9a`, so the
   fixed budget was spent inside one window. The limit is `u64::MAX`; the
   limiter compares the count against it the same way whatever it is.

## `claude/tender-cray-bsfw83` — after #165: two MUSTs, OTel 0.33, versioned docs, the sweep

Picked up after #165 merged (`6ce5dc9`); pull request #168. Commits are
authored and signed off as the maintainer and carry **no** `Co-Authored-By`
or session trailers, by the maintainer's instruction (DCO). CONTRIBUTING's
note on AI-assisted changes suggests a `Co-Authored-By` trailer; the two
disagree, and that is the maintainer's to reconcile — it was not edited.

**What landed, in commit order.**
1. `test(client):` the keep-alive liveness test tolerates a runner stall and
   reports the stub's longest write gap.
2. `fix(adversarial):` `probe_push.py` receivers work on Python 3.13+, and
   hang mode holds its socket; two push-sender tests pin redirect and
   flood behaviour.
3. `ci:` Dependabot #164's action bumps.
4. `deps(otel)!:` OpenTelemetry 0.33 (Dependabot #158-#161 together).
   `opentelemetry-otlp` 0.33 retries a failed export (4 attempts), so export
   losses are now counted once per export by a `Counting` wrapper around
   each exporter rather than per HTTP attempt.
5. `fix(client)!:` §8.3.2 — `from_card` takes the card's first supported
   interface; `preferred_bindings` is deprecated.
6. `fix(server)!:` §3.6.2 — only `A2A-Version` 1.0 (any patch) is served;
   other minors get `VersionNotSupportedError`.
7. `docs(handoff):` the private upstream report (no identifier here, by
   design).
8. `test:` clippy 1.99's `assert_is_empty` and `unused_async`.
9. `test(server):` the unreachable-collector test uses an accept-and-drop
   listener; Windows retries a closed port for ~2 s per attempt.
10. `deps(slimrpc):` RUSTSEC-2024-0449 waived in the out-of-workspace binding,
    with its dependency chain.
11. `fix(book):` brand font URLs (the live site returned 404 for them).
12. `feat(docs):` versioned site — `/` the latest release, `/vX.Y/` each
    release line, `/dev/` main — built by `scripts/build_docs_site.py`,
    deployed from `docs.yml` on pushes to main and after `Release` completes
    (`workflow_run`, not a tag trigger).
13. `test:` the weekly sweep's six survivors (2026-09-28 onward) killed or
    retired; see `mutation-history.md`.
14. `ci(mutants):` `a2a-server` dealt round-robin across 24 shards; the four
    unrecorded sweeps since 2026-09-07 and the 2026-10-12 one get ledger rows.
15. `docs(handoff):` this section.
16. `feat(client)!:` §3.3.4, the ledger's last GAP — a client built from a
    card refuses, before sending, a call the card does not declare the
    capability for (`ClientError::CapabilityNotDeclared`), with
    `ClientBuilder::without_capability_checks` and the CLI's
    `--no-capability-check` to opt out.
17. `fix(server):` push webhook timeouts of 10 s per request and 30 s per
    delivery (§4.3.3, §13.2), so every attempt of the default three starts
    inside the bound.
18. `feat(client):` a rate limit refused over JSON-RPC or WebSocket
    (`InternalError` with a `RetryInfo` detail) is retried after the delay
    it names; `A2aError::with_retry_info_from_details`.
19. `test(server):` the stress test reports a failed connect's whole error
    chain, and its accept loop says when it stops (item 6 below).
20. `deps(itk):` the card-signature interop pin moves to a2a-sdk 1.2.2, after
    the interop matched all six cards against it.
21. `feat(server)!:` §13.1 — an authenticating interceptor isolates tasks by
    caller by default; `share_tasks_across_callers()` opts out. See "§13.1,
    as built" below for the rules and what `build()` now refuses.
22. `docs(handoff):` the §13.1 update.
23. `deps:` Dependabot #167 and #166 folded (hyper 1.12, tokio 1.53.2 and
    five more); the workspace lock's changed lines are #167's exactly, the
    binding's lock is #166's file. Both PRs can be closed once this merges.
24. `feat(server)!:` §8.1 — `build()` requires an agent card;
    `without_agent_card()` opts out. Tests opt out (425 builders, by
    script); docs set a card (hidden `card()` helper in 47 doc blocks, a
    visible card in both quick starts).
25. `docs(handoff):` this update.
26. `deps(itk):` the signing interop lock takes protobuf 7.36.2, the
    version the other Python locks had; the interop matched 6/6 on it.
27. `fix(server):` a card whose security scheme has an empty REQUIRED
    string (an API key's `name`, an HTTP `scheme`, `openIdConnectUrl`, the
    authorization-code, client-credentials and device-code flow URLs) is
    refused by `build()`, hot reload and the dynamic card handler, through
    the new `AgentCard::validate_security_schemes` (the const `validate`
    cannot walk the map, and dropping its `const` would break the public
    API at 0.14.1). Nine proto ledger entries move to TESTED. The ledger's
    191 citations into the edited files were carried through the diff by
    script (115 moved), each checked to name the same text as before, and
    every named test citation into them was then anchored to its `fn` line
    (14 moved: rustfmt ran after the remap, and two were already wrong).
    The ledger check verifies the file and the name, not the line, so a
    line drifts silently; anchor by name after any edit that moves code.
28. `ci:` pin freshness grades the Python SDK again (item 9 below).
29. `test(server):` §3.3.2's SHOULD log system errors is tested: a failed
    executor is logged at ERROR with its task and cause.
30. `deps(itk):` the conformance peers move to `@a2a-js/sdk` 1.3.0, a2a-go
    v2.6.0 and `a2a-java` 1.4.0.Final (maintainer's decision); the java-sdk
    REST skip goes, and the Go interop's step 1b runs the full battery.
31. `deps:` every lockfile takes the day's semver-compatible updates
    (maintainer's decision): root 59 crates, SLIMRPC binding 41, itk and fuzz
    48 each, loom-halt 2. `agntcy-slim-persistence` is held at 0.1.0 in the
    binding (item 11 below). The benchmark-harness locks are left alone.
32. `feat(server):` §13.2 — `HttpPushSender::https_only()`, opt-in
    (maintainer's choice of three hardening options).
33. `feat(server):` §13.4 — `AuthFailureThrottle`, opt-in, and
    `CallContext::peer_addr`, which every binding now records (item 12).
34. `fix(server):` §3.1.6 — the subscribe reattach hook re-enters the
    subscriber's tenant and caller (`TenantContext::capture`). It ran
    while the stream was read, outside the request, so on a partitioned
    store it found no task and ended the stream at the first turn boundary.
    Found while designing §3.5.2's catch-up; item 21's caller isolation
    made it reach every authenticated server. Any hook a reader runs later
    (the §3.5.2 catch-up included) needs the same capture.
35. `feat(server):` §13.2 — `PushConfigCipher`, opt-in: the SQL push-config
    stores' `with_cipher` seals each config (`a2a-sealed:v1:<base64>`),
    bound to its row, and `accept_unsealed_rows` reads a table written
    before. The SDK ships no cipher; the tests use AES-256-GCM from `ring`
    (`tests/support/aes_gcm.rs`). `tenant_sqlite_config_store.rs`'s tests
    moved to their own file, unchanged, to keep it under 500 lines.
36. `feat(server):` §3.5.2 — a stream that falls behind the broadcast
    buffer catches up from the task's event log, after the last position it
    delivered, and goes on live, instead of ending with `streamLagged` (the
    maintainer's choice). The request builds the hook (`lag_catch_up_hook`)
    inside its tenant scope (item 34) and hands it to the reader
    (`with_catch_up`). Each reader starts after the writer's position when
    it attached (`starting_after`), so a catch-up never replays what the
    stream was not owed. The hook takes only a complete replay whose head is
    still in the log, and after a catch-up a live event that does not follow
    it is reported as a lag, so a stream never skips an event silently.
    `streamLagged` remains when the log cannot supply every missed event:
    the in-memory stores keep 512 events per task by default, one replay
    reads at most `subscribe_replay_limit` (1,000), and a custom store
    without a log has nothing to read. `dispatch/websocket.rs`'s lag test
    now truncates the log to 4 events so that its stream still lags.
37. `test(server):` the sealing tests name a leaked secret instead of
    printing it. CodeQL's "cleartext logging of sensitive information"
    raised three high alerts on `748c95b4` against item 35's tests, whose
    failure messages printed the fixture secrets; nothing real was exposed.
38. `test(server):` cargo-mutants over items 36-37's source diff found four
    reader decisions no test reached (the live position expected after a
    catch-up, the gap's dropped count, a replayed terminal state ending the
    stream); three tests pin them.
39. `test(server):` `if_modified_since_matching_returns_304` retries a
    request pair that straddles a second. It failed once on macOS at
    `f95b5420` (200 for 304); reproduced by timing the pair across a second
    boundary. The handler and test date from `708ead97`.
40. `fix(server):` §8.6.1 — the dynamic card handler sends no
    `Last-Modified` and ignores `If-Modified-Since` (the maintainer's call,
    item 13 (c)): a produced card has no modification date (RFC 9110
    §8.8.2.1, §13.1.3). The `ETag` revalidates it; the static handler keeps
    its load-time `Last-Modified`. This replaced item 39's test, which
    pinned the old behaviour.
41. `fix(server):` §3.5.2 — writes to one queue broadcast in position
    order. A spec audit of this branch found that a terminal event held for
    the processor's verdict was overtaken by a later write through another
    handle on the queue's writer (`CancelTask` beside the executor), and
    that on a multi-threaded runtime any two writes could swap. Item 36's
    reader skips a position it has passed, so the terminal event was lost;
    v0.14.1 has the reordering without the loss. `InMemoryQueueWriter`'s
    `order` lock is held from the position's assignment to the broadcast;
    an uncontended write takes it with `try_lock` and still finishes before
    `write` returns. Five hand mutants of the lock are killed, one only by
    chance: `concurrent_clones_deliver_every_position_in_order` caught the
    sync path releasing before its broadcast in 17 of 20 runs, since that
    race is one call wide.
42. `fix(client):` a request with no `configuration` carries the client's
    `accepted_output_modes`. `SendMessageConfiguration::default()`'s
    `["text/plain"]` filled the field first, so the configured modes never
    applied; the unit test that covered the merge set an empty
    configuration by hand, with a comment explaining the default away.
43. `test(server):` item 41's stress test renamed two bindings clippy's
    `similar_names` refused. Item 41's clippy run predated the test's
    final form, so its message's clippy claim did not hold for the commit
    pushed, and CI's clippy jobs failed on it.
44. `fix(server):` a refused inline push config on a continuation keeps
    the task. `commit_task` saved the turn first and registered the config
    after; a refusal ran the rollback meant for a new task, which deleted
    the row, so the continued task and its history went with it. A
    continuation now registers first; if its turn then cannot be saved, a
    config the server created is withdrawn, one named by id is kept.
45. `fix(server):` §11.5 — `A2aRouter` reads GetTask's `historyLength`
    and every GET or DELETE route's `?tenant=`, as `RestDispatcher` does.
    Its module docs said a request without a resolver is always served
    from the default tenant; a POST body's `tenant` was honoured all along.
    The `:cancel` and `:subscribe` POST bodies are still not read (audit
    register R24).
46. `test(server):` `a_keepalive_that_finds_the_sink_busy_is_retried`
    failed once on macOS at `52e35e27` (no ping: the sink holder's wake-up
    overran the 100 ms left in a 400 ms budget). The budget is 2 s now, the
    hold 1.3 s, 700 ms to spare; the mutant it was written for (a busy sink
    recorded as pinged) is still killed. The same run's two-replica soak
    failed its p95 bound (3.94x, a dip at 40-50 s that recovered), not
    reproduced; it ran 59,090 pairs where the two runs before ran 32,773
    and 45,486, so a faster runner. Recorded, not fixed: read the next run.
47. `fix(client):` the WebSocket transport reaches `wss://`. `tls-rustls`
    now enables tokio-tungstenite's `__rustls-tls`, which compiles its
    rustls connector in without a root store of its own; the transport hands
    it `tls::default_tls_config()` or `with_tls_config`'s. The config sits
    in a private newtype that declares itself unwind-safe, since a bare
    `rustls::ClientConfig` would have cost `WebSocketTransport` the
    `RefUnwindSafe` it has had since 0.13.0. `__rustls-tls` is
    tokio-tungstenite's internal feature name: the `>=0.30, <0.31` range
    pins it; recheck it when that range moves.
48. `refactor(server):` item 44 took `handler/messaging/mod.rs` to 520
    lines, past the 500-line ratchet. A continuation's register, save and
    withdraw moved into `create.rs`'s `save_turn`; `mod.rs` is at exactly
    500. A fifth test, `an_accepted_continuation_registers_its_config_once`,
    covers the case the move made easy to break; 12 hand mutants over the
    new shape, all killed.
49. `fix(server):` §3.1.6 — a `SubscribeToTask` snapshot is consistent
    with the live stream. `streaming/event_queue/applied.rs` is the queue's
    mark of how far its events are stored: the background processor and the
    blocking collector mark each event applying, then stored (the processor
    at `on_persisted`, before a status update's push). A subscriber attaches
    under the order lock at an exact position P, waits (bounded by
    `subscribe_replay_catchup`) until every event up to P is stored and none
    is being stored, reads the row then, and skips live events up to the
    mark. Past the bound it replays the log from the mark. Two limits: an
    artifact's push still delays its mark (only status updates and snapshots
    report `on_persisted`), and the processor logs each event only after
    storing the one before, so the log replay rarely outruns a slow store;
    and a row written by `CancelTask` or another replica is outside the mark.
50. `fix(server):` §3.1.2 / §3.1.6 — a cancelled task's streams end on
    `Canceled` whatever its executor's `cancel` hook writes.
    `TerminalTracking::cancel_with` runs the hook and, when it returned `Ok`
    without a terminal state, writes the default hook's `Canceled`
    (`executor::write_canceled`); `CancelTask` and the shutdown/halt path
    both go through it. Before, a send stream ended after `Working` with no
    terminal frame, and a halt left the task `Working` (each checked by
    reverting its call site). A subscription already recovered `Canceled`
    through the reattach hook's store read. The conformance kit's
    `cancel_emits_terminal` check, which existed to catch the silent hook,
    is `cancel_succeeds` now and passes it; `a_cancel_that_fails_is_caught`
    keeps a failing hook failed, which no test covered before.
51. `fix(server):` §5.1 — proto3 defaults on the JSON bindings mean unset,
    as gRPC's conversion already read them: a part's `mediaType` and
    `filename`, a push config's `id` and `token` (standalone, and inline in
    `SendMessage`, where an `id` of `""` also made the continuation path
    treat the config as named), and ListTasks' `contextId`, `pageToken` and
    `status`. `helpers::unset_if_empty` does it on ingress; the four tests in
    `tests/proto3_defaults.rs` each failed before. Left as they were: an
    empty `tenant`, already read as none, and push `authentication.credentials`
    of `""`, which by the code (not run) is delivered as `Bearer ` with
    nothing after it; neither the audit nor a test covers that one.
52. `refactor(server):` the applied mark's version is bumped when an
    application starts, not when it ends. The Mutation Testing gate on
    `1672fbf9` found the two bumps covering for each other: each one's
    mutant survived because the other still changed the version (shard 6
    the end's, shard 5 the start's). Only the start's is needed, since a
    reader reads only when nothing is being applied; with the end's gone,
    `a_read_an_application_overlapped_is_taken_again` kills the start's.
    Shard 5's two other survivors, the `+` in each catch-up deadline
    (`snapshot_and_live`, `LogReplay::read`), get tests of their own.
53. `fix(client):` §3.1.11 / §13.3 — the client checks calls against the
    extended card's capabilities once `get_extended_agent_card` has fetched
    it (`A2aClient::adopt_capabilities`; the field is behind a `RwLock`,
    since the fetch takes `&self`). Replaced, not merged; `extendedAgentCard`
    stays declared. Ledger 425 moves from APPLICATION to TESTED and 3166's
    note no longer says the client keeps no card.
54. `fix(server):` §13.4 / §5.1 — a malformed GET query value
    (`historyLength`, `pageSize`, `status`, `includeArtifacts`) is
    `InvalidParams` on both HTTP+JSON dispatchers, from
    `dispatch::rest::query`'s parsers, which return `Result` now. The axum
    router's ListTasks reads the raw query with the same parser. The
    fuzzing API's `rest_route` returns `Option` params, so the fuzz target
    is unchanged.
55. `fix(server):` §3.3.2 / §13.4 — ListTasks refuses a `pageSize`
    outside a2a.proto's 1 to 100 (it clamped to 1000 and read 0 as 50), and
    a message's `referenceTaskIds` get `validate_id`. Two tests that pinned
    the clamp and the 0 default now require the refusal, and
    `probe_load.py` pages by 100. A behaviour change, in the CHANGELOG and
    the upgrading guide.
56. `fix(server):` §3.5.2 — a `SubscribeToTask` stream across turns gets
    every turn's events. The reattach hook is called with the stream's
    position, attaches with the manager's exact `attach`, and answers
    `Channel { rx, after, fresh }`, `Logged { through }` (no queue here, but
    the log moved: a turn ran between two polls, on this replica or another)
    or `Final { event, through }`. The reader replays the log for each before
    it goes on, and reports what the log cannot supply as `streamLagged`. The
    replay a stream owes lives in the reader (`Owed`), so a `read` dropped
    while the log is read resumes it; the lag catch-up had that hole too (a
    dropped `read` went back to the channel past the lag), and it is fixed
    with it. Without a log each turn numbers from 1, and the reader's dedup
    skipped as many of a turn's events as it had seen positions before;
    `fresh` resets it. A subscription with no live queue starts at the log's
    end, read before the task is re-read, so a race with another replica's
    writes duplicates an event rather than loses one. Across replicas a
    subscriber now sees the running replica's events a poll late:
    `multi_replica.rs`'s test asserts it and the book's horizontal-scaling
    page says so. A continuation admitted before the previous turn's
    processor had logged its last events seeded colliding positions, which
    the replay would skip as already delivered; item 57 fixes that.
57. `fix(server):` §3.5.2 / §3.3.3 — a continuation sent the moment a task
    parks keeps its events and the task's artifacts (audit register R55,
    reproduced 3 of 3 before the fix). A stream delivers `input-required`
    before the processor has logged and stored it, and the continuation was
    admitted when the executor returned: its queue numbered from the log's
    end while the turn was still being logged, its colliding positions were
    dropped as replays (the log held only the first turn's three events),
    and it was built from the task read before the turn stored it (the
    first turn's artifact was gone from the task). The executor's teardown
    now waits for `InMemoryQueueWriter::settled` (every assigned position
    stored, or the consumer gone, bounded by the write timeout) before the
    queue is released; `commit_task` takes the task from `settled_task`,
    which waits for a parked turn before the approval check and re-reads
    it, and `reject_in_flight_send` only checks. `ExecutorStart` moved to
    `messaging/start.rs` to keep `mod.rs` under 500 lines. Not covered: a
    continuation that reaches another replica while the first is still
    storing; nothing in-process can see that.
58. `fix(server):` §3.5.2 — a replay with a hole in the log is not used
    (audit register R53). A failed append leaves the log without that
    position and the processor goes on; `LogReplay::read` counted a read
    that reached its target as complete, so a lagged stream was replayed
    positions 1 to 9 and 11 to 32 with 10 skipped, and a resume after 1 got
    2, 4 and 5. `contiguous` now makes such a read incomplete (the lag is
    reported) and a resume falls back to the snapshot. Left open, low: the
    catch-up's target is the task's current queue, so a send stream that
    lags after its turn's queue is gone can be replayed into the next
    turn's events (the same task's, in order); §11.7 describes a stream as
    closing at an interrupted state, but §3.1.2's MUST is the terminal
    state, so send streams were left as they are.
59. `fix(server, client):` §5.1 — a gRPC error carries its `data` (audit
    register R60). `server_error_to_status` adds what the JSON bindings'
    `error_details` send: a BadRequest's field violations as a
    `google.rpc.BadRequest`, any other `data` as a `google.protobuf.Struct`
    `Any` (`dispatch/grpc/details.rs`, since `tonic_types::ErrorDetails`
    holds only the standard detail types). A stream's error goes through
    it too; it was `Status::internal("event queue error")`, which lost the
    `streamLagged` marker. The gRPC client's `status_to_error` decodes a
    `Struct` detail into `data` and a BadRequest into the JSON shape. The
    types crate's `proto` module re-exports `prost_types`, so neither the
    server nor the client takes a dependency of its own, and no lockfile
    changes.
60. `docs:` stale ledger notes and citations (audit register R62). The
    §3.6 patch-version entry said both sides compare only the major: the
    server compares `Major.Minor` (`is_served_version`) and the client only
    the major (`is_compatible`), so neither considers the patch, and the
    client takes a 1.1 interface listed first and addresses it as 1.0. The
    §14 and §10.2 entries still said the server accepts any 1.x, and
    §3.3.2's quoted the refusal as "supports 1.x". §3.3.2's `@type` entry
    said `A2aError.data` is dropped; it goes as a typed detail on every
    binding (gRPC since item 59). §3.3.2's retry-guidance entry and
    §13.4's said RetryInfo goes on every binding; SLIMRPC has no details
    field and sends none. The §9.5, §10.6 and §11.6 error entries said
    unary errors carry `ErrorInfo` alone, WebSocket sends no `data` and
    gRPC streams lose the marker, all fixed since (`ef31df91`, 2026-10-11,
    and item 59); §11.6's "further objects MAY" entry moves from
    NOT_SUPPORTED to TESTED on `unary_errors_keep_their_data`. Twelve
    entries cited `dispatch/mod.rs` lines whose code had moved, mostly to
    `dispatch/version.rs`. The 24 entries changed here had their citations
    re-pointed by pattern, each cited line read back. Ledger-wide, 296 of
    the 723 named test citations with a line number pointed at a line
    other than the test's `fn`; each now names the one `fn <name>` line in
    its file (only the numbers changed, checked by diff).
    `A2aError::is_stream_lagged`'s doc said the stream goes on after the
    lag; on every binding but SLIMRPC it is the last frame, and the doc now
    says so and how to resynchronize. SLIMRPC's `event_stream` sends the
    error as `"InternalError: …"` text and reads on after it; whether the
    stream ends there is unchecked (register: SLIMRPC stream marker).
61. `feat(client):` §3.2.6 — the client asks for extensions (audit register
    R63). The server read `A2A-Extensions` on every binding, but the
    client could send it only as a header the caller set; the ledger's
    `binds: both` entry was TESTED on the server half alone.
    `ClientBuilder::with_extensions` sends the URIs as one comma-separated
    header (gRPC metadata) on every request, through an interceptor the
    build adds after the caller's, which leaves alone a request an
    interceptor already gave the header (`src/extensions.rs`).
    `WebSocketTransportConfig::with_extensions` puts it on the upgrade,
    where that binding's server reads every service parameter; an
    `extra_headers` entry of the same name replaces it. A URI that is
    empty or holds a comma, whitespace or non-visible-ASCII is refused at
    build or connect. `extensions_e2e.rs` (SDK crate) checks what the
    executor sees over JSON-RPC, HTTP+JSON, gRPC and WebSocket, unary and
    streaming; the client crate's own tests (unit tests in
    `extensions.rs`, `ws_extensions_tests.rs`) kill the 14 hand-applied
    mutants, since cargo-mutants runs only the mutated crate's tests.
    SLIMRPC is not tested. Ledger §3.2.6 (entry 69), §9.2 and §10.2's
    comma-separated entries and the proto `AgentExtension.required` entry
    say so.
62. `fix(client):` §3.6.1 — the agent card fetch sends `A2A-Version`
    (audit register R61). "Clients MUST send the `A2A-Version` header with
    each request"; every transport did, the card fetch
    (`fetch_card_with_metadata`, behind `resolve_agent_card`,
    `fetch_card_from_url` and the caching resolver) did not. It sends
    `1.0` unless the caller's `CardFetchOptions` name the header, whose
    value is then sent alone. This SDK's server serves discovery with or
    without the header. Ledger §3.6.1 (entry 130),
    whose transport citations had drifted, now cites the lines that set
    the header.
63. `fix(server):` §8.6.1 — a card's `ETag` is the same for the same card
    (audit register R16). It hashed `serde_json::to_vec(card)`, and the
    card's `HashMap` fields (`securitySchemes`, a requirement's `schemes`,
    OAuth `scopes`) serialize in a per-instance random order: the new
    `card_etag_determinism.rs` gave 8 ETags in 8 parses of one card with
    16 schemes, statically and through a dynamic handler. All four call
    sites (static, dynamic conditional and unconditional, the REST
    extended card) take `caching::card_body_and_etag`, which hashes
    `serde_json::to_value(card)`; the body served is unchanged. That
    relies on `serde_json::Map` being a `BTreeMap`, which holds while no
    crate in the build enables serde_json's `preserve_order` (none does,
    `cargo tree -e features -i serde_json --all-features`);
    `serde_json_maps_are_sorted` fails if the workspace ever does. An
    application that enables it downstream gets the old variable `ETag`
    back, costing bandwidth, not correctness. An explicit sort would cover
    that, but in this build it is an equivalent mutant, which
    `mutants.toml` does not admit.
64. `fix(types)!:` proto REQUIRED — an agent card needs a description and
    a version (audit register R7). `AgentCard::validate` checked `name`
    but not `description` or `version`, and `AgentCard::new` leaves the
    description empty, so such a card was published with
    `"description": ""`. Both are refused now; `build()`, hot reload and
    the dynamic card handler run `validate`. 73 `AgentCard::new` call
    sites without `with_description` gained one, by script (book, READMEs,
    the server's doc comments and tests, the SDK's observability test, the
    swarm example); the 29 tests that failed first are among them.
    Compile-only snippets (`no_run`) in the server README, the book's
    dispatchers, security and hello-agent pages also lacked a skill, so
    `build()` refused them already; they have one now (hello-agent's as
    the example has it). Left without a description, because nothing
    validates them: the types crate's builder examples, the client's
    discovery and signature tests, the CLI's selection test, the
    a2a-rs benchmark harness (another SDK), and history (old CHANGELOG
    entries, the v0.9.0 review). Ledger: the proto `AgentCard` name,
    description, version and capabilities entries said `validate` is never
    called in production, which has been wrong since `build()` ran it;
    the three REQUIRED list entries said an empty list may be emitted.
65. `fix(server):` §4 / §5.1 — what a JSON binding accepts, gRPC can carry
    (audit register R6). `validate_send_params` now runs `make_portable`:
    each `raw` part is decoded as the gRPC conversion decodes it (standard
    or URL-safe, padded or not) and kept in standard padded base64, and an
    integer beyond ±2^53 in the request's, the message's or a part's
    metadata or in a `data` part is `InvalidParams` naming the field.
    `tests/portable_content.rs` failed 3 of 4 before (a non-base64 `raw`
    and the over-range integers were accepted, a URL-safe `raw` stored as
    sent); its gRPC case shows the conversion refusing both values.
    Hand mutants: 16 distinct over the final code, all killed; two
    survived an earlier version, where `serde_json` reports any positive
    integer up to `i64::MAX` through `as_i64`, so the `u64` branch, met
    only above `i64::MAX`, held an equivalent `<=`/`<` mutant and no test
    went that high: the branch is now `!n.is_u64()`, and `u64::MAX` is a
    case. `base64` is an unconditional dependency of the server now: the
    root lockfile is unchanged, and the ITK and SLIMRPC-binding lockfiles
    each gain one line, the server's edge to `base64 0.23.1`, which both
    already held. An executor's own events were left open (register
    R64): an artifact with such a value was served over JSON and failed a
    gRPC read. Item 105 closes it, and the §4 entry is TESTED.
66. `docs(ledger):` the ten entries items 61 to 65 changed with a test
    that failed first (69, 130, 143, 270, 279, 290, 413, 436, 438, 445)
    did not carry `changed_by`, which the ledger's method says marks a
    re-verified entry; they do now. The notes item 60 corrected stay
    unmarked, since reading the code is not that check, and the method
    says so.
67. `fix(server):` §9.3 — a JSON-RPC request refused for a header is
    answered with its id (audit register R18; adopter audit S14). The
    `A2A-Version` and `Content-Type` checks ran before the body was read,
    so their refusal answered `"id": null` and a batch got one object.
    Both now wait for the body: `refuse_with_ids` (`refusal.rs`) echoes
    the request's `id`, and a batch gets one error per request, each
    recorded as a failed call. A batch beyond `max_batch_size` still gets
    one null-id refusal, since an answer per item would let a body of
    `{}`s buy a response 94 times its size (1,000 of them, 3,001 bytes,
    were answered with 283,001 when the cap was lifted); so do a body that is
    not JSON, an empty batch, a request without an id, and a body over
    the size limit, where the header's refusal still wins over the read
    error. `tests/jsonrpc_header_refusal_ids.rs` failed 4 of 5 before (the
    fifth guards the null-id cases), and `rpc_calls.rs` gained a check of
    what is recorded. The `id` is echoed whatever its JSON type, as the
    dispatcher does for a request it serves; register R21 covers invalid
    id types for both. Hand mutants: 16 distinct,
    all killed; three were first written so that they did not build
    under `-D warnings`, and were rewritten until they did.
68. `fix(server):` §9.1 / §11.1 — a request's `Content-Type` is read as
    a media type (audit register R22). The JSON-RPC, HTTP+JSON and axum
    bindings matched the raw value by prefix, case-sensitively:
    `Application/JSON` was refused, `application/json-rpc` and
    `application/json-patch+json` accepted. One predicate,
    `dispatch::unsupported_content_type`, now serves all three: the
    type/subtype before any `;` (`rest::media_type::essence`), compared
    case-insensitively with the two A2A types. `content_type_essence.rs`
    failed on all three bindings before; hand mutants 9 of 9 killed.
    The stricter half is a behaviour change, in the CHANGELOG: a client
    sending `application/json-rpc` is refused where it was served
    (a2a-java 1.3.0.Final's server matches by prefix too, entry 295). The
    client was already case-insensitive (it lowercases before its
    `text/event-stream` check) and is unchanged. Ledger §3.6.2 (entry
    134) cited four lines that had drifted to a comment, an attribute,
    `};` and a doc line; it now cites the version checks entry 70 cites.
69. `fix(server):` RFC 9110 §15.5.9 / spec §5.1 — an HTTP+JSON body that
    never arrives is `408` (audit register R25). `read_body_limited`
    (`rest/response.rs`) returned a message and the caller answered every
    failure `413`, a stalled upload included, where the axum router
    answers `408`. It returns the status now: `413` over the size limit,
    `408` past `body_read_timeout`, `400` for a body stream hyper cannot
    read. `rest_body_read_errors.rs` (raw TCP) failed 2 of 3 before; the
    third pins `413`. Hand mutants 5 of 5 killed. The JSON-RPC binding
    still answers both with `-32700`, inside a `200`: its body was not
    JSON it could read. The axum router's own rejections stay plain text
    (register R26). Ledger §13.4 (entry 379) cited the JSON-parse
    `map_err` for the body cap, and now cites the read.
70. `fix(server, types):` §9.3 — three more JSON-RPC 2.0 rules (audit
    register R21, all but its notification part). An id that is not a
    string, a number or `null` is refused by `JsonRpcRequestId`'s
    `Deserialize`, so it is `-32600` with a null id over HTTP and
    WebSocket, where it was run and echoed; a header refusal (item 67)
    echoes only an id of those types. An omitted `params` is read as `{}`
    on both transports (HTTP's `parse_params`, WebSocket's two paths), so
    `ListTasks` lists. A stream in a batch is `UnsupportedOperation`
    (`-32004`). `jsonrpc_2_0_rules.rs` failed 6 of 6 before; the types
    crate has its own test, since cargo-mutants runs only the mutated
    crate's. Two tests pinned the old behaviour and now assert the new:
    `list_tasks_missing_params` (renamed `list_tasks_without_params_lists`)
    and the unit test now `parse_params_missing_names_the_required_field`,
    which entries 485, 495 and 503 cite. Hand mutants 12 of 12, one
    rewritten after it did not build. Still open, the maintainer's call
    (register R21): a notification is answered, which JSON-RPC 2.0 §4.1
    forbids; `jsonrpc_absent_id_request_answered_with_null_id` pins it.
71. `fix(types, server):` §5.6.1 / §5.1 — timestamps are read as RFC 3339,
    as the gRPC conversion reads them (audit register R20).
    `parse_iso8601_to_unix_millis` was lenient (whitespace, unpadded or
    signed fields, an offset without minutes, `02-30` rolled into March)
    and refused every pre-1970 instant; the gRPC conversion parses with
    the `time` crate's RFC 3339 parser. An executor's status timestamp in
    a lenient shape went out as written on the JSON bindings and failed a
    gRPC read; a 1969 `statusTimestampAfter` was `InvalidParams` on every
    binding. The parser now takes RFC 3339's fixed-width grammar with a
    real date, `t`/`z` in either case, second 60 only as a month's last
    second in UTC (read as `time` reads it, the millisecond before), and
    years 0000-9999 in UTC; `unix_millis_to_iso8601` writes pre-1970
    instants as themselves, through `utc_fields`, which moves an instant
    five 400-year cycles forward and its year back. `stamp_status` keeps a
    timestamp as written only in the canonical shape (`T`, `Z`, no
    offset). `tests/timestamp_parity.rs` (types, feature `proto`) compares
    the two parsers over 1,655,808 strings (141,444 accepted) and checks
    the `iso8601` fuzz target's round trip on each; it failed at once on
    the old parser (`0000-01-01T00:00:00Z`), as did the three
    `pre_epoch_timestamps.rs` tests (both stores, and the handler's
    filter). Hand mutants: 27 of 27 in the types crate, 2 of 2 in the
    server. Two tests that pinned the old behaviour changed with it:
    `parse_rejects_pre_epoch_dates` became
    `parse_accepts_pre_epoch_dates_back_to_year_0`, and the formatter test
    asserts `-5` writes `1969-12-31T23:59:59.995Z`. Two drifted citations
    were corrected in passing: entry 182's `list_tasks.rs:47` (the
    page-size message) and entry 424's `status_stamp.rs:21` (a doc line).
72. `fix(types):` §8.4 / RFC 7515 §4.1.11 — a card signature naming a
    critical JWS extension is refused (audit register R28).
    `signature_header` read only `alg`, `kid` and `jku`, so a protected
    header with `"crit": ["exp"]` verified and its expiry was never
    enforced. Any `crit`, protected or unprotected, is now an error from
    `signature_header`, so `verify_agent_card` and both JWK Set verifiers
    refuse it. `a_signature_with_crit_is_refused` signs with and without
    `crit` under one key; mutants 2 of 2 (the check removed, the
    unprotected header ignored). The a2a-sdk interop signs with `alg`,
    `kid` and `typ` only (`itk/interop/card_signature_interop.py`), so it
    is unaffected. Entry 262 cited a doc line for the verify step; it cites
    `verify_with` now.
73. `fix(server):` §13.2 — a push config a header cannot carry is refused
    when it is created (audit register R45). `refuse_an_unservable_config`
    checked the URL and the scheme, not the token or the credentials, so a
    value with CR, LF or another control character was stored and every
    delivery refused (the sender's `validate_header_value` for CR/LF, the
    HTTP client for the rest). Creation now applies `HeaderValue`'s rule to
    both, so the check matches what a delivery can send; the inline config
    of a send takes the same path. `push_config_header_values.rs` failed
    before; mutants 4 of 4, a CR/LF-only check among them (killed by the
    NUL and BEL cases). Entry 429's citation had drifted to a `));` and
    now names the scheme check.
74. `fix(server):` proto `REQUIRED` — `StaticAgentCardHandler::new`
    validates the card (audit register R39). It is public and re-exported,
    and was the one way to serve a card that ran neither
    `AgentCard::validate` nor `validate_security_schemes`. It already
    returned `ServerResult`, so it now returns `InvalidParams` as `build()`
    does, with no signature change. The three dispatchers build theirs
    from a card `build()` has already validated, so they are unaffected.
    `a_card_that_does_not_validate_is_refused` (no skills; an API key
    scheme without a name); mutants 2 of 2. The 25 ledger notes that
    listed where validation runs say this too.
75. `fix(types):` proto `oneof` — a `SecurityScheme` naming two schemes is
    refused (audit register R37). The deserializer returned the first arm
    of `SCHEME_ARMS` present and ignored the rest; it now counts the arms
    present, in either spelling, and refuses more than one, as `Part`
    does. `a_scheme_with_two_arms_is_refused`; mutants 4 of 4, one
    rewritten after it did not build. No ledger entry: the `oneof` has no
    RFC 2119 line of its own.
76. `fix(server):` RFC 9110 §5.3 / spec §9.2, §10.2, §11.2 — a header sent
    as several lines is one field (audit register R23). Each of the
    JSON-RPC, HTTP+JSON, axum, gRPC and WebSocket bindings copied headers
    into a map with `insert`, keeping the last line of a repeated field;
    the JSON-RPC echo of `A2A-Extensions` read the first. One helper,
    `dispatch::header_fields`, now builds every binding's map, joining a
    field's lines in order with `", "` (`"; "` for `cookie`, RFC 9113
    §8.2.3), as the SLIMRPC binding already did; gRPC drops `-bin` keys as
    before, and the echo reads the same joined value. Behaviour change: a
    single-valued field sent twice (`Authorization`, a tenant header)
    reaches interceptors as both values joined, so a bearer token sent on
    two lines is refused (`a_credential_sent_on_two_lines_is_refused`).
    `tests/repeated_header_lines.rs` (one test per binding, all five
    failing before) and four unit tests on the helper; mutants 14 of 14,
    two rewritten after they did not build (the "last line only" one, the
    old behaviour, fails the credential test too). Ledger: the eight
    entries that cited the deleted extractors (277, 278, 279, 288, 289,
    290, 296, 297) cite the helper, and 68, 275, 320 and 328 follow the
    code the diff moved; 279 and 290 lose the last-line-wins note and carry
    `changed_by`. Entry 296's note that axum never checks `A2A-Version`
    was stale (`validate_a2a_request` does, `axum_adapter.rs:212`) and its
    axum citation pointed at a doc comment; both are corrected.
77. `fix(client):` §8.6.2 / RFC 9111 §4.3.4 — `CachingCardResolver` takes
    the validators a `304` carries (audit register R32). It kept the
    first `200`'s `ETag` and `Last-Modified` and revalidated with them
    after a `304` that sent new ones; a `no-store` `304` left the card
    stored. `a_304_replaces_the_stored_validators` and
    `a_no_store_304_leaves_nothing_stored`, both failing before; mutants
    6 of 6. Ledger 272 (PARTIAL: max-age is still not used to skip the
    request, and the plain functions do not cache) and 273 cite the code
    by its current lines; six proto `REQUIRED` entries (431, 432, 434,
    468, 472, 481) cited a `.headers()` call and now cite the card parse,
    as 435 to 439 do. Not done: the resolver takes no
    `CardFetchOptions`. It was held back because `CardFetchOptions` then
    printed its headers, an `Authorization` among them, through `Debug`;
    item 99 removes that reason, and the resolver still takes none.
78. `fix(types):` §5.7 — the `oneof` wrappers skip unknown fields (audit
    register R46). `SendMessageResponse`, `StreamResponse` and
    `OAuthFlows` derived serde's externally tagged encoding, which fails on
    any key beside the member. `oneof_deserialize!` (`types/src/oneof.rs`)
    reads keys as identifiers, skips unknown ones, builds the member once,
    and refuses none or two; `Part` and `SecurityScheme` keep their own
    deserializers. `tests/oneof_unknown_fields.rs`: the unknown-field test
    failed before; the two-member refusal held before and only its message
    changed. Mutants: 10 of 10 on the final macro, one surviving until the
    test asserted "this one sets more". Cost, measured because
    `protocol/stream_events/*/deserialize` is in the Regression Gate:
    timed interleaved with a derived mirror enum in one release-build
    process, 21 pairs per run, 3 runs, Intel Xeon @ 2.10GHz, a stream
    event's parse is 1.08-1.10x the derived one's (a first version that
    moved the member through an `Option` was 1.09-1.17x; a per-enum tag
    enum and `#[inline]` did not help). The gate fails at 25 % with 95 %
    confidence. One client test asserted the old "unknown variant" text and
    now asserts the new one. Ledger 189 cites the macro and the tests.
79. `test(types):` CI on `152ea962`, mutants shard 7: `replace && with ||
    in offset_minutes` survived item 71's tests. Under it, a digit then `:`
    reads as two digits (`:` - `0` is 10), so `+05:0:` parses as `+05:10`;
    nothing put a non-digit in an offset's digit positions. Three such
    offsets join `parse_refuses_what_rfc_3339_does_not_allow`; the mutant
    is killed by hand. The same run's `Test (stable, windows-latest)` failed
    at `TcpListener::bind` with WSAENOBUFS (10055) in
    `dispatch_edge_tests`, unchanged by the push and green on `8059da21`;
    commented on the PR, and the next push is its one re-run.
80. `fix(server):` §8.6.1 / RFC 9110 §9.3.2, §13.1 — the card answers
    `HEAD`, and its conditionals follow RFC 9110 (audit register R29,
    R44). JSON-RPC and HTTP+JSON route `GET` and `HEAD` to the card (and
    HTTP+JSON's probes), axum exempts both from the version check, and
    `check_conditional` and the dynamic handler answer `304` to `GET` and
    `HEAD` only, read every `If-None-Match` line through
    `dispatch::field_value`, and (static) skip `If-Modified-Since` whenever
    `If-None-Match` is present. `tests/card_head.rs` (all three bindings
    failed before) and four unit tests, all failing before; mutants
    13 of 13. Ledger 270 and 271 carry the new rules and tests; 268's note
    that nothing honours `no-store` was stale (item 77's resolver does) and
    is corrected. Not changed: a non-`GET` request carrying a matching
    `If-None-Match` gets the card, not the `412` §13.1.2 asks for, because
    `ConditionalResult` has no such outcome and no SDK route sends one
    there; `If-Modified-Since` is still compared by exact string.
81. `test(server):` `eefd2399` — item 76's `repeated_header_lines.rs`
    imported `SocketAddr` for an annotation in its axum-gated test, so
    clippy on the server without `axum` failed (unused import). It was
    pushed in `d3f633d3`'s batch before a no-default-features clippy ran on
    it. Lesson: a test file with a feature-gated test needs clippy with
    that feature off before it is pushed, not only `--all-features`.
82. `fix(types):` §8.4.1 — the gRPC card conversion keeps an explicitly
    empty `icon_url` or `documentation_url` (audit register R47; both are
    `optional string` in a2a.proto). Both directions filtered `""` to
    `None`, a comment calling it idempotence.
    `an_explicitly_empty_url_keeps_its_presence` failed before; mutants 4
    of 4 (each filter put back). Ledger 248 cites it.
83. `fix(server):` §11.1 / RFC 9110 §12.5.5 — negotiated HTTP+JSON
    responses carry `Vary: Accept, Content-Type` (audit register R27), on
    `RestDispatcher` and axum, appended in `ResponseMediaType::apply` so
    the extended card keeps its `Vary: Authorization`; `from_headers`
    reads every `Accept` line through `dispatch::field_value`.
    `tests/rest_vary.rs` (both bindings failed before); mutants 4 of 4,
    one rewritten after it did not build; `append` turned `insert` is
    killed by the extended-card caching tests. Ledger 294 stays PARTIAL for
    its a2a-go trade-off.
84. `fix(server):` §11.3 / RFC 3986 §3.3 — `RestDispatcher`'s traversal
    guard refuses dot segments, not every `..` (audit register R24, first
    half). `contains_path_traversal` splits on `/` and `\` and refuses a
    `.` or `..` segment in each decoded form; the raw pre-check went, since
    the first pass holds every raw segment. `tests/rest_dot_segments.rs`
    (failed before: `/tasks/v1..prod` was 400) and two unit tests; mutants
    5 of 5, the fail-closed bound surviving until
    `a_path_still_decoding_at_the_bound_is_refused`. Ledger 299 records it;
    302's two query citations pointed into this function and now name the
    boolean and number parsers.
85. `fix(server, client):` §11.3 — on HTTP+JSON the path's id is the
    request's (audit register R38, and R24's second half; in Behaviour
    Changes). `set_path_field` (was `inject_field_if_missing`) inserts over
    the body, as axum's push-config create now does; axum's `:cancel` and
    `:subscribe` read their bodies through `body_with_path_id`; both
    routers keep empty interior segments (one trailing `/` tolerated); the
    REST client refuses an empty path id. `inject_field_preserves_existing`
    pinned the old behaviour and is replaced by
    `set_path_field_overrides_the_body`. `tests/http_json_path_ids.rs` (both
    bindings failed before) and the client's
    `an_empty_path_id_is_refused_before_the_request_is_built`; mutants 11 of
    11 (9 server, 2 client). Ledger: the eight proto id entries (485, 495,
    497, 498, 500, 501, 503, 505) say it. Not done from H-F10: the client
    still sends an empty `messageId` or empty parts unchecked; the server
    refuses both.
86. `fix(slimrpc):` — the SLIMRPC server drops a caller's `:`-prefixed
    metadata key (audit register R41; in Security). `a2a_headers` passed
    `:peer-addr` and `:path` through, so `CallContext::peer_addr` and
    `AuthFailureThrottle` read a caller's claim as the connection's
    address. `a_callers_pseudo_header_never_reaches_the_handler` failed
    before; mutants 2 of 2. The book's SLIMRPC page says which keys are
    dropped; ledger 321. **Open, for the maintainer:** the same caller can
    still send `x-forwarded-for`, and with `trusted_proxy_hops` above 0
    `forwarded_client` (read by `AuthFailureThrottle` and
    `RateLimitInterceptor`) takes the entry it names, because no proxy
    appends one over SLIMRPC. Read from the code, not tested. Dropping the
    key in the binding, or a per-binding hop count, are both API-visible
    choices.
87. `fix(slimrpc)!:` §3.6.2 — the SLIMRPC server refuses a call that
    declares no `A2A-Version`, as 0.3 (audit register R43; in Behaviour
    Changes). It served one as 1.0 because the official `a2a-slimrpc` sent
    none at 0.2.4; read on 2026-10-13, `a2a-client-lf` 0.2.8's `A2AClient`
    adds the version to every call and `a2a-slimrpc` 0.3.2 forwards it.
    `SlimRpcServer::with_require_version(false)` is the opt-out, held in an
    `AtomicBool` beside the handler (`Served`) so `from_app` needs no new
    argument. `an_absent_version_is_refused_as_0_3_unless_none_is_required`
    replaces `an_absent_version_is_accepted_for_interop` and failed before;
    e2e `an_empty_version_is_refused_unless_none_is_required`. The strict
    default failed 4 of 6 multicast tests: `SlimRpcMulticast` sent no
    version, so it now adds one through the unicast client's
    `metadata_from`. Mutants 9 of 9 (7 server, 2 multicast). Ledger 135
    PARTIAL to TESTED: its note had said every binding refused.
88. `fix(slimrpc):` — a multicast member's error is its outcome (found
    while testing item 87; in Fixed). `MulticastRpc`'s `origin` is the full
    four-part SLIM name, and `send_message` keyed it unchanged while
    outcomes are paired by the three-part address, so an erroring agent
    came back as a timeout; `fanout.rs` dropped every error frame. Both now
    go through `address_key`, which also replaced the unit tests' private
    copy of it. `an_agents_refusal_is_its_outcome` and
    `an_agents_refusal_reaches_its_stream` failed before; mutants 4 of 4.
    **Open, Low:** an `EventStream` ends at its first `Err` chunk
    (`event_stream.rs:485`), and the fan-out's lag report is one, so a
    member that falls behind is told so and its stream ends; `fanout.rs`'s
    docs describe it continuing. The test of the report,
    `a_consumer_that_falls_behind_is_told_what_it_missed`, stops reading at
    it, so it does not distinguish the two.
89. `fix(server):` a2a.proto `ListTasksRequest`/`ListTasksResponse` — every
    task store (audit register R40; in Behaviour Changes).
    `status_timestamp_after` includes its bound, `total_size` counts what the
    filters match before pagination, and the SQL stores' `page_size` is the
    size used, not the page's length; a2a-sdk 1.2.2's stores, read
    2026-10-13, do all three. `tests/support/list_proto.rs` runs against
    all six stores (Postgres two, `#[ignore]`d like the rest of that file)
    and failed on each before. Five tests pinned strictly-after and now
    expect the bound included; they are named in the commit. The in-memory
    count keeps an unfiltered or context-only list O(1) (`index.len()`); a
    timestamp or status filter walks what it matches. The SQL stores run a
    `COUNT(*)` per call. MEASURED 2026-10-13, `data_volume/list/
    filtered_page_50` (context filter, release, this container), criterion
    medians, two runs each: at 10K tasks old 34.07 and 34.04 µs, new 34.95
    and 35.81; at 100K old 33.20 and 33.86, new 33.86 and 35.28; at 1K no
    change. The 10K gap (0.9 to 1.8 µs) is wider than the old code's
    run-to-run spread and is not explained by the O(1) path it takes.
90. `docs:` spec section citations (audit register R48). Card caching is
    §8.6 and discovery §8.2 (four files cited §8.3), signing §8.4 (cited
    §10), and `method.rs`'s eleven methods §3.1.1 to §3.1.11 (cited §8.x).
    The axum quick-start's card declared JSON-RPC while `A2aRouter` serves
    HTTP+JSON (C-F10, ledger 162). E-F12 (`event: error` on SSE error
    frames) is kept: the Python SDK does the same.
91. `fix(server, slimrpc):` §3.3.2 — a refused credential's challenge
    reaches gRPC and SLIMRPC callers (audit register R59; in Fixed). gRPC:
    `www-authenticate` metadata on the `UNAUTHENTICATED` status. SLIMRPC,
    which has no error metadata: `(WWW-Authenticate: <challenge>)` ends the
    message. WebSocket refusals after the upgrade still carry none. The
    gRPC helper tests moved to `grpc/helpers_tests.rs` to keep
    `helpers.rs` under 500 lines; the ledger's 17 citations of them, and
    five line citations into `helpers.rs` the wrapper shifted, follow.
    Both tests failed before; mutants 4 of 4.
92. `fix(server)!:` §3.3.4 — push-config operations need a push sender, and
    so does a card that declares push (audit register R34; in Behaviour
    Changes). `ensure_push_supported` refuses when no sender is wired, so
    Get, List and Delete answer `PushNotificationNotSupported` as Create
    did; they ran against the store. `build()` refuses a card declaring
    `pushNotifications` with no sender. Three tests failed before:
    `every_push_op_without_a_sender_returns_push_not_supported`,
    `no_sender_rejects_push` and
    `build_refuses_a_card_declaring_push_without_a_sender`.
    The workspace run (`--all-features`) then failed 26 tests across 9
    targets, all from this change. Twenty now wire a sender: they test
    behaviour past the gate, or build a card that declares push for another
    reason. Three about the missing sender build without a card instead.
    Three named for the old answers (`*_no_push_sender_still_works`,
    `get_push_config_no_push_sender_returns_404`) now assert
    `PUSH_NOTIFICATION_NOT_SUPPORTED` under new names. Two metrics tests
    that had passed on the gate's error rather than the one they name, and
    three capability tests written for the old contract, wire a sender too.
    The SLIMRPC binding (its own workspace) was not run: by grep, no card
    in it declares push and no test calls a push-config operation. 39
    hidden book cards declared push with no sender wired and no longer
    declare it; the push-config store snippet wires one. Ledger 355, 376,
    397 and 573; 87 citations into the three edited files follow their
    lines (the 74 bare ones checked byte for byte against the old source).
    Mutants 8 of 8.
93. `fix(server):` a `Last-Event-ID` on a WebSocket upgrade no longer
    replays the event log (audit register R42; in Fixed). The binding page
    promised no resumption, but the upgrade's headers reach every call on
    the connection, so every `SubscribeToTask` replayed the log from it.
    `extract_upgrade_headers` drops the header.
    `ws_upgrade_last_event_id_does_not_replay_the_log` received the snapshot
    and the replayed events before the fix. Ledger 3030; 35 citations into
    `websocket.rs` follow the shifted lines (every bare one checked against
    the old source). Mutants 2 of 2.
94. `feat(client):` `AuthInterceptor::api_key` (audit register R35; in
    Added). It sends an `APIKeySecurityScheme`'s key in the header or cookie
    the scheme names; `with_scheme("api-key")` writes `Authorization`.
    `api_key_placement_e2e.rs` runs both against `ApiKeyAuthInterceptor`
    over JSON-RPC, HTTP+JSON and gRPC: the old placement gets `401`, the new
    one reaches the handler. A query-string key is refused with the new
    `ClientError::InvalidAuthScheme`: no transport can add a query
    parameter, which needs a `Transport` change and is put to the
    maintainer. `auth.rs` passed 500 lines, so its tests moved to
    `auth/tests.rs` (checked byte for byte). Ledger 423. Mutants 9 of 9.
95. `fix(server):` the axum router's own refusals are AIP-193 (audit
    register R26; in Fixed): `405` through `method_not_allowed_fallback`,
    which keeps `Allow`, and the body extractor's `413`, `408` and `400`.
    Not a router-wide `fallback` for unknown paths: it would answer every
    path of an app the router is merged into, and axum panics merging two
    routers that both have one. `axum_refusals_of_its_own_are_aip_193` and
    the extended `a_dribbling_body_is_cut_off_at_the_configured_timeout`
    failed before. Ledger 2908; ledger 2918's axum citation pointed at the
    body extractor, not the content-type check, before this change and now
    names the check. Mutants 4 of 4.
96. `fix(client, types):` a signed card is verified as received (audit
    register R31; in Fixed). `canonicalize_card_json` is now the one
    canonicalization path (`canonicalize_card` serializes and calls it);
    verification takes canonical bytes, so the typed and JSON entry points
    share it. `verify_card_json_with_jwks` is new; the client's fetch with
    signature keys parses the body as JSON and verifies that before the
    typed parse. Two tests sign over a field this SDK does not model: the
    types one shows the typed path refusing it, the client one failed
    before. Removing the presence prune survived every types-crate test;
    `a_received_cards_defaults_are_dropped_before_canonicalizing` now kills
    it (the Python interop test in `itk/` covers it across SDKs). The client
    gained `ring` and `base64` as dev-dependencies, already in the tree.
    Ledger 2131, 2133, 2140 (its note denied the opt-in that exists) and
    3352. Mutants 6 of 6.
97. `feat(server):` `DispatchConfig::with_agent_card_max_age` (audit
    register R30; in Added). The three HTTP dispatchers built their card
    handlers with the 3600-second default and no way to set it. The new
    test fails without it; the existing caching test now pins the default
    exactly rather than any `max-age=`, which let a changed default pass.
    Ledger 2229. Mutants 5 of 5.
98. `fix(server):` a failed push delivery logs the webhook's origin (audit
    register R14; in Security). `deliver_once`'s two WARNs logged
    `config.url` whole; they use `push::webhook::origin_for_log`, as
    `HttpPushSender`'s own lines have since audit O16.
    `a_failed_or_timed_out_delivery_logs_the_origin_not_the_secret` saw the
    secret in the failure line before the fix. Ledger 3138. Mutants 2 of 2.
99. `fix(client):` the client's `Debug` output names headers and JSON
    keys, never values (audit register R17, adopter audit O17; in
    Security). `ClientRequest`, `ClientResponse`, `CardFetchOptions` and
    `WebSocketTransportConfig` derived it. A scan of every crate for a
    derived `Debug` over a field named for a header, token, secret or
    credential found no other: the hits elsewhere hold header names,
    cancellation tokens, page tokens and token URLs. `redact::names`
    sorts, so a debug-print is the same line every time.
    `tests/debug_redaction.rs` pins each type's exact output under every
    feature set that changes it; all five tests failed before.
    `interceptor.rs` passed 500 lines, so its tests moved to
    `interceptor/tests.rs` (checked byte for byte). Ledger 3194, which binds
    both sides and cited only server evidence. Mutants 18 of 18; the one
    that drops the sort is caught through `HashMap` iteration order, so
    only by chance: if that order is uniform, six names come out sorted
    by accident once in 720 maps (not measured).
100. `fix(client):` a fetch with signature keys refuses an unsigned card
    (audit register R9; amends the unreleased `with_signature_keys` entry in
    Added, so no release behaved otherwise). Anyone able to alter a card on
    its way can remove its signatures too, and the fetch returned an
    unsigned card, so it returned the altered one unverified.
    `with_require_signature(false)` keeps verify-when-present for a fleet
    that is not all signed. `a_card_whose_signatures_were_removed_is_refused_when_keys_are_supplied`
    failed before. Mutants 6 of 6; treating an empty list as signed
    survived until the lenient test covered `[]`, since the types crate
    refuses an empty list with the same "not signed" message. The
    client's `signing` feature note in `Cargo.toml` said no code in the
    crate verifies a card, untrue since `7260a8f`; corrected. Ledger 3352
    (still PARTIAL, low: `CachingCardResolver` and `from_card` take no keys).
101. `feat(server):` `RequestHandlerBuilder::with_extended_agent_card`
    (audit register R56; in Added). No setting existed, so an agent that
    declared `extendedAgentCard` served its public card again. Not a MUST
    gap, as the register had it: ledger 3173 already read the public card
    as the configured extended card, which is why the change is additive
    rather than a `build()` refusal that would have broken about twenty
    builders declaring the capability. `build()` gives the extended card the
    public card's validation, push-sender check and extension
    advertisements (`advertise` is now one function for both), and refuses
    one a non-declaring public card would never serve. Four tests in
    `lifecycle/extended_card.rs`; the API is new, so none could fail before
    it. Mutants 8 of 8. The no-public-card test asserts the advertisement
    because, read before the run, nothing else would catch advertising
    only when a public card is present; that mutant was not run without it.
    Ledger: 424, 3157, 3158 and 3159 move from NOT_SUPPORTED to SUPPORTED
    (MAY clauses; 3159's user-specific half and 3152's per-caller variation
    remain unsupported), and eleven entries whose notes said the extended
    card was always the public one are corrected. Several of their
    citations already pointed at the wrong lines (575 at `result`); they
    cite the match arms now.
102. `fix(server):` a database that cannot answer for now is a temporary
    failure (audit register R57; in Fixed and Added). `A2aError::unavailable`
    marks it as `rate_limited` marks a refusal, with a field that is not on
    the wire (`A2aError` stays 96 bytes, measured). One classifier,
    `sql_error.rs`, serves the ten store conversions; the backend is passed
    in, since a Postgres SQLSTATE can be all digits and the first version,
    which told them apart by parsing, read `08000` as a SQLite code (the
    table test caught it). The exception is the decision to know about:
    `SendMessage`, `SendStreamingMessage` and creating a push config keep
    `INTERNAL` (`ServerError::not_retry_safe`), because the client re-sends
    those on `503` as refused before processing (`retry.rs`), and a store
    failure can come after the executor ran; two mutants that dropped the
    exception showed both calls answering `503`. The SLIMRPC binding now
    answers a halt `UNAVAILABLE`, as gRPC did. `tests/sql_transient_errors.rs`
    starves a one-connection pool: the read failed `500 INTERNAL` before.
    Mutants 23 of 23 (two in the SLIMRPC workspace). Ledger 532. Postgres
    paths are compiled and their codes table-tested, not run against a
    database here; CI's Postgres job runs those stores.
103. `fix(server):` the event queue refuses an executor's event missing a
    REQUIRED field (audit register R36, part (a); in Behaviour Changes).
    `event_queue/content.rs`, called where the artifact-parts check was:
    message `messageId`, role and parts; artifact `artifactId` and parts
    (an appended chunk may have none); status `state`, in every event kind
    and inside a snapshot's status, artifacts and history.
    `tests/executor_event_fields.rs` writes eleven broken events; all
    eleven were accepted before. The conformance harness drained only what
    the queue broadcast, so an executor writing a nameless artifact earned
    a clean report once the queue refused it; `HarnessWriter` keeps refused
    writes and `artifacts_have_ids` grades them. Mutants 18 of 18. Every
    workspace package's tests pass (server 2672, sdk and client 948, the
    other eighteen packages 463), run per package because the workspace
    build filled the disk. Not done, part (b): a task snapshot that moves
    back to `Submitted` is still accepted. The store guards terminal
    states; mirroring the status-update rule in both processors means
    sharing the background mark-failed path, which also drives the
    writer's terminal verdicts.
104. `docs(slimrpc):` a multicast member's lag report ends its stream, and
    the fan-out's docs say so (audit register R66; docs only). The report
    is an `Err`, and `EventStream::from_event_channel` bridges it into the
    one parsing path, which finishes the stream at its first error
    (`event_stream.rs`, `Phase::Finished`); the docs read as if the member
    went on, and the README cited the drop at `fanout.rs:116`, the
    malformed-event mapping. Making the report non-terminal is a client
    behaviour change and is left to the maintainer. A private intra-doc
    link in the same file did not resolve under `--document-private-items`;
    it does now.
105. `fix(server):` the event queue refuses an executor's event the gRPC
    binding cannot carry (audit register R64; in Behaviour Changes): a
    `raw` part that is not base64, an integer beyond ±2^53 in metadata or a
    `data` part, a `metadata` that is not a JSON object, in every event
    kind; `raw` that is base64 is kept in standard padded form. The register
    left refusing or converting lossily to the maintainer; refusing is what
    R6 does for a client's message and what R36 does at the same writer,
    and a lossy conversion would change an agent's data without a word.
    R6's `standard_base64` and `inexact_number` moved to `portable.rs`,
    which both paths use; their bodies are unchanged (diffed against HEAD),
    and they are `pub` in that private module. `tests/executor_event_portability.rs`
    writes twelve such events; all twelve were accepted before. The gRPC
    dispatcher's `reader_to_native_stream_stops_after_a_conversion_error`
    made its unconvertible event through the writer, which now refuses it;
    it sends the event onto the reader's channel instead (a store written
    before this check can still replay one), and still kills its `||`→`&&`
    mutant. Fifteen hand-applied mutants fail the new test: the writer
    skipping the check, each event kind's and each field's check removed,
    `raw` left as written, the object check off, and `exact` never
    refusing. The writer mutant, first written as a bare `Ok(())`, did not
    build (the check became dead code under `-D warnings`); it runs the
    check on a copy and ignores the result instead. Ledger 768 (§4) moves
    from PARTIAL to TESTED; 768 and 225 cite `portable.rs` for the two moved
    functions.
106. `test(types):` CI on `96decb0b`, mutants shard 7: `replace
    A2aError::is_unavailable -> bool with true` and `with false` survived
    (133 caught, 2 missed, 8 unviable). Item 102's hand mutants ran in the
    server crate, where the flag is read, and the types crate covered the
    accessor only with a doctest; CI grades a types-crate mutant with that
    crate's tests under nextest, which runs no doctests.
    `an_unavailable_error_is_marked_temporary` checks the flag both ways,
    the code and message it keeps, and that it is not on the wire; both
    mutants fail it, applied by hand.
107. `ci(mutants):` the incremental job deals its mutants round-robin across
    its eight shards, as the full sweep has since `6c2bff19`; that commit
    changed only the full sweep. The incremental job mutates the whole PR's
    diff against `main` on every push (1,160 mutants on this branch at
    `ee7df1e7`), and the default slicing gave whole shards to one crate: the
    slowest shard of the runs on `152ea962`, `2d551a2b` and `3f152a79` held
    only `a2a-server` mutants and took 114-120 minutes against the
    120-minute timeout (7,193 s of 7,200 on `152ea962`), while `3f152a79`'s
    two `a2a-types`-only shards took 20 and 27. The pinned binary's
    listings: sliced, four shards of 145 `a2a-server` mutants and two of
    `a2a-types` only; round-robin, every shard 91-92 `a2a-server`, 41-42
    `a2a-types`, 11-12 `a2a-client`, the eight together exactly the 1,160.
    Measured: `10827fae`'s run (same source as `ee7df1e7`, so the same
    1,160 mutants) finished all eight shards in 4,755-6,571 s and passed,
    where `ee7df1e7`'s sliced run lost four shards to the timeout at
    7,229-7,237 s, as `96decb0b`'s lost three. The ~77 minutes this item
    first projected was wrong: it divided `3f152a79`'s total evenly, as if a
    mutant's cost did not depend on the one before it. Round-robin used 787
    runner-minutes against the sliced run's 722 (cut short), likely because
    an `a2a-types` mutant makes the next `a2a-server` one rebuild its
    dependents (not measured: the per-mutant times are in the run's
    artifacts). The slowest shard has ten minutes of headroom; the next step,
    if a push brings it closer, is to split the job by crate as the full
    sweep is, so round-robin stays within one crate.

**Counts at this head, re-derived.** Spec ledger: 512 entries, TESTED 318,
PARTIAL 20, NOT_SUPPORTED 8, GAP 0, IMPLEMENTED_UNTESTED 0. Gate prover: 30
proven, 0 unproven, 23 exempt of 53. Static checks: 29 of 29 locally.

**Lessons, new this session.**
- A check describes the tree it ran on. After any edit that follows a
  clippy or test run, run it again before the commit claims it (item 43).
- CI's stable toolchain moves under you: 1.99 on 2026-10-12, two releases
  ahead of the container's. Its new clippy lints failed every clippy job.
  `rustup update stable` before the first local clippy run.
- `#[deprecated]` warns inside the defining crate too; in-crate uses need
  `#[allow(deprecated)]`.
- The weekly sweep's cost is the *caught* mutants (≈206 s to build each,
  against ≈26 s for an unviable one), and contiguous slices cluster them.
  The model in `mutants.yml`'s matrix comment (sum of build + test time ÷ 4,
  plus baseline) reproduced every finished shard within 1.1 minutes; reuse it
  rather than guessing a shard count.
- Four weekly sweeps went unrecorded because nothing prompts a row. Record
  each one as `mutation-history.md` "How to add an entry" says, from the
  run's `Mutants Summary`, the week it runs; a cancelled shard's report is
  still uploaded, so an incomplete sweep can be counted too.
- The versioned site is about 18 MB per release line (268 MB for 13 lines and
  dev, measured); GitHub Pages caps a site at 1 GB.
- A test can pass on a different branch than the one it is named for. Both
  §3.5.2 over-limit tests first ran the live-writer branch, because the
  agent finished before the test's first read and the stream lagged inside
  it; a hand mutant on the other branch survived. Print the decision's
  inputs once (here the replay's target) to see which branch a test
  reaches, and gate the agent when timing decides it.
- A hand-applied mutant checked with `cargo test` (no `--no-fail-fast`)
  shows only the first failing binary's tests; a test in a later target may
  kill it too. Run that target alone before citing it as the killer.
- No live queue on this replica does not mean no turn is running: another
  replica sharing the store may be writing (item 56). Reason about the
  multi-replica case before calling a read order safe.
- The ledger checker verifies that a cited test function exists, not that a
  `path:line` names the code meant. Item 60 anchored every named test
  citation to its `fn` line (296 had drifted); the 1,180 `impl_evidence`
  citations carry no name and were checked only in the entries items 56
  to 60 touched. Move citations through `git diff` when a file changes
  (item 56 did, 45 lines checked identical).
- Nightly cargo's `unused_dependencies` lint flags `book-tests`' dependencies,
  which only its doctests use. A warning on the informational nightly job
  today; it would need an allow if the lint stabilizes as-is.

**What the next session should do first.**
1. Read CI on the branch head.
2. The first 24-shard sweep (next scheduled Monday, or dispatch it): compare
   the slowest `a2a-server` shard with the ~110-minute projection, and give
   the sweep its ledger row.
3. After the next release, confirm `docs.yml` ran from `workflow_run` and
   that `/`, `/vX.Y/` and `/dev/` are what `build_docs_site.py check`
   accepted locally.
4. The incremental mutation job still slices: run 37906246207 put one shard
   at ~83 minutes while the other seven took 13-49. Round-robin is the likely
   fix there too; measure it from that run's artifacts before changing it.
5. `benches/sdk-comparison/harness/agent-a2a-rust` pins the published
   `a2a-protocol-sdk =0.14.1`, so the §13.1 default does not reach it yet.
   When the pin moves past it, its `AUTH_TOKEN` mode (one unlabelled token)
   with `SQLITE_URL` builds no more without `share_tasks_across_callers()`,
   and without `SQLITE_URL` it measures `TenantAwareInMemoryTaskStore`
   instead of the plain store: add the call to keep the measured store.
6. `stress_tests::concurrent_200_requests_all_succeed` failed once on
   `Test (1.88, windows-latest)` at `2d086b04` (run 38269273780): 1 of 200
   requests got `client error (Connect)`. The CI runs on `main` that
   preceded it succeeded, and both runs on the previous head (`40449307`)
   got past this binary to fail later, in `telemetry_export`. It failed at
   TCP connect, before a request reaches any code this branch changed; the
   cause is not established. Two candidates, neither confirmed: a
   full accept backlog (Windows refuses a connect then, where Linux retries
   the SYN), or the test's own accept loop, which `break`s on the first
   `accept()` error and so drops the listener. Item 19 makes a recurrence
   name its cause: read the failing log's error chain, and look for
   "accept failed". In that job it failed in the `sqlite` feature step,
   after passing in the five steps before it; the re-run of the job on the
   same commit passed (run attempt 2).
7. §8.1 stays PARTIAL (ledger line 1980, now low): the WebSocket and gRPC
   dispatchers serve no well-known card, so a deployment on only those
   bindings has no discovery endpoint from the SDK. Closing it means serving
   the card beside them (an HTTP route a gRPC-only server can mount), which
   is a design question, not a fix.
8. The ledger's three medium PARTIAL entries are the maintainer's to decide,
   not fixes: §1.4 (line 107; the serde types are hand-written, not generated
   from the proto), §8.4.3 (line 2132; `jku` is never followed,
   deliberately: `fetch_jwks` takes a URL the caller trusts, not one the card
   names), and §12 (line 2985; SLIMRPC's own spec maps two errors
   differently from A2A §5.4). §3.5.2 (line 692) moved to low with item 36.
   The 20 PARTIAL left are these three and 17 low.
9. Pin freshness (`pin-freshness.yml`, every third Monday; next 2026-10-26,
   or dispatch it with `force`). Its 2026-10-05 grading run had three red
   legs: python-sdk never graded (no PyYAML; fixed on this branch), go-sdk
   could not resolve a2a-go 2.6.0 on Go 1.25 (fixed on `main` 2026-10-10),
   and java-sdk graded the newest a2a-java (1.4.0.Final, on Maven Central by
   2026-09-28) and found `tck.yml`'s REST skip of `a2a_media_type_accepted`
   stale: upstream fixed it. Dispatched on this branch on 2026-10-12 (run
   38299384752), python-sdk, js-sdk and go-sdk passed at their latest
   releases and java-sdk failed only on that stale skip. The maintainer then
   moved all three pins (item 30), and no skip is left on any official SDK's
   leg. Next: extend `card_signature_interop.py` to a2a-go 2.6.0
   (`a2acrypto`) and to `@a2a-js/sdk`, which has exported card signing since
   at least 1.1.0 although the script said it had none.
10. The REST client still sends `Content-Type: application/json` (ledger
    line 2758, NOT_SUPPORTED), deliberately, because a2a-java 1.3.0.Final's
    server refused anything else. The ledger said to revisit when a2a-java
    accepts `application/a2a+json`; 1.4.0.Final does. Switching follows
    §11.1's SHOULD but breaks every call to a 1.3.0.Final or older a2a-java
    server: the maintainer's call.
11. The SLIMRPC binding's lock holds `agntcy-slim-persistence` at 0.1.0.
    Its 0.1.1, a patch release, moves to mls-rs 0.56 / mls-rs-core 0.27
    while `agntcy-slim-mls` 0.3.10 still builds against 0.54 / 0.26 and
    implements persistence's storage trait across the two: with both in the
    graph `agntcy-slim-mls` fails to compile (73 errors, E0277 "multiple
    different versions of crate mls_rs_core"). A plain `cargo update` in
    `bindings/a2a-protocol-slimrpc` brings it back; use `cargo update -p
    agntcy-slim-persistence --precise 0.1.0` after it until SLIM releases a
    matching `agntcy-slim-mls`. That is upstream's to fix.
12. `CallContext::peer_addr` travels in the headers map under `":peer-addr"`
    (`PEER_ADDR_KEY`), beside `":path"`, so no handler signature changed. The
    axum router has it only when served with
    `into_make_service_with_connect_info::<SocketAddr>()` or with a
    `PeerAddr` extension. `RateLimitInterceptor` does not use it: an
    unidentified caller is still keyed by trusted `x-forwarded-for` hops or
    the shared `anonymous` bucket. Keying that bucket by peer address
    instead would be a behaviour change to an existing feature, so it is the
    maintainer's call. `serve/mod.rs` is at exactly 500 lines.
13. Found on the way, not changed. (a) `validate_webhook_url` compares the
    scheme case-sensitively, so `HTTPS://…` is refused at creation although
    RFC 3986 schemes are case-insensitive: stricter than needed, not unsafe.
    (b) The gRPC client maps `RESOURCE_EXHAUSTED` to a 429 with
    `retry_after: None` (`transport/grpc.rs`), dropping the `RetryInfo`
    detail the server sends, so a retry policy backs off on its own schedule
    instead of the server's; the JSON-RPC and REST clients honour it.
    (c) Done (item 40): `DynamicAgentCardHandler` no longer sends
    `Last-Modified`, which had been the current second on every request.
14. §3.5.2 is done (item 36). Ledger line 692 stays PARTIAL, now low, for
    the `streamLagged` cases item 36 lists. Raising the in-memory retention
    (`TaskStoreConfig::with_max_events_per_task`) or the replay limit narrows
    them; the SQL stores already keep a task's whole log.
15. The Postgres-gated suites run locally against a throwaway PostgreSQL 16
    (`initdb` as the `postgres` user, outside the scratchpad, whose parent
    permissions get reset): `A2A_TEST_POSTGRES_URL=postgres://postgres:postgres@localhost:5432/postgres
    cargo test -p a2a-protocol-server --features postgres --test
    postgres_store_tests -- --ignored`.

## `claude/nifty-fermat-roj04f` — the spec ledger, and what it found

The maintainer asked to keep closing the gap to full A2A compliance. The first
`main` runs of the workflows #163 added were all green (2026-10-10, `f824b03`),
but green hid two things: Scorecard scored 5.8/10 with "58 existing
vulnerabilities", and nothing graded the specification below the level of an
area. This branch answers both.

**What landed, in commit order.**
1. `ci:` least-privilege workflow tokens; the conformance peers' advisories
   (122 unique by a local osv-scanner run) fixed without moving any graded SDK;
   setup-go 1.25 to 1.26; `osv.yml`'s PR job scans the loom lockfile.
2. `proto:` all six copies of `a2a.proto` were A2A v1.0.0's text. Now v1.0.1's,
   pinned in `proto/UPSTREAM`, checked offline (`check_proto_copies.sh`) and
   online (`check_a2a_proto_upstream.sh` in `official-tck.yml`).
3. `fix(signing):` §8.4.1 field presence in card canonicalization; §8.4.2
   `kid` required (`sign_card_es256`/`sign_card_ed25519`; the `Option` forms
   are deprecated) and `typ: "JOSE"`. `itk/interop/card_signature_interop.py`
   proves interop both ways against the official Python SDK, not yet in CI.
4. `fix(server):` a message-only streaming answer is one `Message` (§3.1.2).
5. `fix(server):` `?A2A-Version=` honoured (§3.6.1); the axum adapter checks
   the version and the request media type; REST's
   `ContentTypeNotSupportedError` carries `ErrorInfo` (§11.6).
6. `fix(types):` OAuth flows whose empty `scopes` map is omitted parse.
7. `docs(compliance):` `docs/compliance/spec-ledger.json` (512 entries) and
   `scripts/check_spec_ledger.py` in `ci.yml`, proven by
   `prove_gates_fail.sh`.
8. `feat(server):` per-caller task isolation (§13.1),
   `RequestHandlerBuilder::isolate_tasks_by_caller`; design below.
9. `fix(server):` push-config list/delete of an unknown task is
   `TaskNotFound`; `GetExtendedAgentCard` enforces required extensions; a
   repeated non-append `artifactId` replaces the artifact; `ROLE_UNSPECIFIED`
   is refused.
10. `fix(streaming):` mid-stream JSON-RPC error `data` is an array of typed
    details (§9.5); the client's lag predicates read both shapes.
11. `fix(rate-limit):` `A2aError::rate_limited`: 429 + `Retry-After`,
    `RESOURCE_EXHAUSTED`, `RetryInfo` (§13.4); an overload's REST body names
    `RESOURCE_EXHAUSTED`.
12. `fix(slimrpc):` the binding spec's §6.1 status table; case-insensitive
    metadata keys.
13. `fix(server):` empty or scheme-less push `url` refused; tests for
    §3.1.4, §3.3.4 (streaming capability on the wire) and §3.5.2.
14. `fix:` CI repairs (setup-go 1.26.9, rustdoc links); §3.4.1 context in a
    direct reply; §5.6.1 executor timestamps normalised to UTC or refused.
15. `fix(server):` a push config whose auth scheme the sender cannot write
    (anything but Bearer/Basic) is refused at creation (§4.3.3, §13.2).
16. `feat(server):` `RequestContext::accepted_output_modes()`; ledger records
    why the REST client keeps `application/json` (a2a-java refuses others).
17. GAP sweep: empty push-config ids; extended-card caching headers;
    `HttpPushSender::with_allowed_hosts`; opt-in `dispatch::Hsts`;
    `BadRequest` field violations on HTTP+JSON; the caching resolver can
    hold the extended card. Three GAPs remain, all maintainer decisions
    (below).
18. `ef31df9` one JSON-RPC error shape on HTTP, SSE and WebSocket (§9.5,
    §12.4); `51b93ad` the `Hsts` inert-bounds allowlist entries.
19. `2d20696` (breaking) `AgentCard::validate` refuses empty required
    arrays and `build`, the dynamic card handler and hot reload run it.
20. `f7cb779` every push webhook is attempted; the budget sets concurrency
    (§4.3.3). `7260a8f` `CardFetchOptions::with_signature_keys` (§14).
    `f400818` tests for four untested MUSTs; SLIMRPC credentials documented.
21. Proto `REQUIRED` sweep: `validate` refuses empty required strings; the
    queue writer binds events to the running task (empty ids filled,
    foreign ones refused); `tests/required_fields.rs` removes each
    REQUIRED card and event key and the gRPC status/artifact. Security-
    scheme strings are refused only when absent, not when empty (PARTIAL:
    `validate` is const and cannot walk the scheme map).
22. SHOULD-level tests: TLS 1.3 negotiated with forward-secret AEAD suites
    only; card revalidation sends both validators; refusals name the
    parameter and the accepted value; `#[deprecated]` on the implicit and
    password OAuth flows. §13 brute-force throttling (spec line 3198) is
    PARTIAL: a refused credential never reaches the rate limiter, and
    per-source counting needs the peer address, which `CallContext` lacks.
    Maintainer decision.
23. PARTIAL sweep: empty task ids on get/cancel/subscribe are
    `InvalidParams`; an artifact with no parts is refused at the queue
    write; an empty push auth scheme is covered by the Bearer/Basic check;
    731/733 were already fixed in `56f8d61`; the axum router's public card
    gets the same caching headers as JSON-RPC and REST. Left for the
    maintainer: the 5 s `push_delivery_timeout` default, below §4.3.3's
    recommended 10-30 s (raising it doubles per-event concurrency at the
    defaults and the blocking path's worst-case wait; 886, 3118);
    empty appending chunks (287), §13.3 extended card with no card
    configured (3172, tied to 1980).

24. WebSocket binding specification (`book/src/bindings/websocket.md`),
    pinned by `websocket_binding_spec.rs`; the WebSocket `message/stream`
    alias removed; HTTP+JSON path ids decoded (REST) and routed raw
    (axum); a task without `contextId` parses; `no-store` cards are not
    cached. Recorded for the maintainer: an opt-in `Origin` allowlist on
    the WebSocket upgrade (3222).

25. Streams end at the terminal state (§3.1.2, §3.1.6): the reader
    closes once the terminal event is delivered, not when the executor
    drops its writer. The conformance harness now reads the raw channel.
    CI note: `9aacd7a`'s Windows job failed once in
    `keep_alive_comments_reset_the_idle_bound` (client SSE timing, 100 ms
    heartbeats against a 1 s bound on a current-thread runtime; file
    unchanged since 2026-09-22); the job was re-run once. Left for the
    maintainer: legacy-name warnings (3429, needs custom deserializers),
    encrypted push-credential storage (3139, needs a key design).

26. Code-scanning alerts (132 open on `main`, 2026-10-12). This session's
    token cannot read the alerts API (403), so each tool was rerun locally on
    `main` and on this branch: osv-scanner 2.2.3, Scorecard v5.4.0 (built
    from the Go proxy; local mode reproduced main's seven Token-Permissions
    findings at the alerts' exact lines), CodeQL 2.27.2 with the
    workflow's suites. Fixed on this branch: peer dependency advisories
    (recursive osv-scanner: 0), Pinned-Dependencies (23 to 0: image digests,
    hash-locked pip under `.github/requirements/`, npm lockfile), osv.yml's
    three Token-Permissions findings, the four `jwt.rs` cleartext-logging
    test messages, 45 Python quality findings. Alerts close only when this
    reaches `main`. Left for the maintainer, in the alert UI: dismiss the
    four `contents: write` Token-Permissions alerts (release.yml x2,
    benchmarks.yml, itk-nightly.yml; each needs the grant), and Code-Review
    / Branch-Protection, which are repository settings. Disk note: the
    CodeQL Rust database (deps included) needs ~10 GB; `target/` was
    deleted to make room. Noticed, not fixed: `scripts/adversarial/
    probe_push.py` "hang" mode closes the socket it means to hold (its
    `finally: c.close()` runs after appending to `_held`).

**How the ledger was made, and how far to trust it.** Seven read-only audit
agents, one per specification region plus one over the proto's `REQUIRED`
fields, each opened the code and tests for every RFC 2119 line. Their entries
are model output. Every entry this branch changed was re-verified by
reproducing the defect and showing the new test fail without the fix; the
others are recorded as found. Treat an unchanged entry as a lead, and verify it
before acting on it, as this branch did.

**Measured, not assumed.**
- Official TCK, local run of this branch: identical to CI's on `main` (83
  MUST passing, 5 failing, all baselined, 5 skipped, 21 untested; gate exit 0).
  ROADMAP's "87 passing" did not add up and is corrected.
- In-repo TCK against a2a-go 2.5.0 (Go 1.26.9, patched deps) and a2a-java
  1.3.0 (Quarkus 3.37.0 with netty and jackson BOMs): all pass on JSON-RPC and
  REST with tck.yml's one REST skip.
- `cargo test --workspace --all-features --no-fail-fast` after item 13:
  172 binaries, 4,478 passed, 0 failed, 116 ignored. Run it with
  `CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0`:
  with debuginfo the all-features build passed 21 GiB and filled this
  container's disk allowance (ENOSPC mid-run). The SLIMRPC binding is
  outside the workspace; its `cargo test -- --test-threads=1` passed after
  item 12.
- Ledger after item 25: TESTED 295, PARTIAL 36, SUPPORTED 52,
  IMPLEMENTED_UNTESTED 2, GAP 3, NOT_SUPPORTED 13, NOT_APPLICABLE 22,
  APPLICATION 89 (`scripts/check_spec_ledger.py`). Items 21-22 moved ten
  entries from IMPLEMENTED_UNTESTED to PARTIAL (nine security-scheme
  REQUIRED strings and the brute-force SHOULD); item 23 moved eight PARTIAL
  entries to TESTED; item 24 moved eight WebSocket documentation entries to
  SUPPORTED and three to TESTED; item 25 moved one to TESTED. The two left untested: system errors are
  logged (534) and webhook URLs should be HTTPS (3137).
- Workspace tests after item 21: 182 binaries, 4,517 passed, 0 failed,
  116 ignored. Item 22's four new tests were run on their own and each shown
  to fail with its fix removed.
- Workspace tests after item 17: 177 binaries, 4,497 passed, 0 failed,
  116 ignored; clippy clean.
- CI lesson 2: run the whole static-checks job locally before pushing (its
  29 `run:` lines in `ci.yml`, all scripts, no build). `56f8d61` failed only
  `check_inert_bounds.py`, which no test or clippy run catches: a new
  `Dispatcher` (the `Hsts` wrapper) needed allowlist entries saying it
  delegates the request bounds to the dispatcher it wraps.
- CI lesson: every push to this branch cancels the previous run
  (concurrency), so commits pushed minutes apart never finished CI. Two real
  failures (Documentation, Go interop) sat hidden behind cancellations from
  `c482314` and `b2a7746` until a run was left to finish. Push, then let the
  run complete before pushing again.
- Workspace clippy `-D warnings`: clean. `prove_workflow_gates_fail.py`: 28
  proven, 0 unproven, 21 exempt.

**Decisions waiting on the maintainer.**
- *Interface order (ledger §8.3.2, lines 2007-2008, GAP).* The spec says a
  client MUST select the first supported interface in the card's order.
  `ClientBuilder::from_card` ranks by `ClientConfig::default()`'s
  `preferred_bindings = ["JSONRPC"]` first, so a card listing
  `[HTTP+JSON, JSONRPC]` gets JSONRPC; `from_card_applies_the_default_preference_list`
  pins that on purpose (a2a-go parity). Following the spec means `from_card`
  passes no preference (card order, with the existing fallback past an
  interface it cannot build) while `from_card_preferring` keeps the caller's
  explicit ranking. Not changed: it reverses a recorded decision and changes
  which binding default clients use. **Decided 2026-10-12: follow the spec;
  done on `claude/tender-cray-bsfw83`.**
- *Upstream candidate, not filed: a2a-java and `application/a2a+json`.*
  a2a-java 1.3.0.Final's REST server refuses any request Content-Type not
  starting with `application/json` (`A2AServerRoutes.validateContentType`,
  read from the jar's bytecode), so a client following §11.1's SHOULD gets
  `ContentTypeNotSupportedError` from it. This client keeps sending
  `application/json`; ledger line 2758 records why. A public issue on
  a2a-java is the maintainer's call. **Superseded 2026-10-12: nothing to
  file.** Pin freshness graded a2a-java 1.4.0.Final on 2026-10-05 (run
  37268203276) and its REST server passed `a2a_media_type_accepted`, which
  1.3.0.Final fails; see item 9 of this branch's section.
- *Minor-version matching (ledger §3.6.2, line 737).* The spec says requests
  are processed with the semantics of the requested `Major.Minor` and an
  unsupported one is `VersionNotSupportedError`. This server accepts any
  `1.x`, and so does the official Python SDK: `_is_version_compatible`
  compares only the major version, although its docstring says major and
  minor. Rejecting `1.1` follows the text and breaks the first 1.1 client.
  **Decided 2026-10-12: follow the spec; done on `claude/tender-cray-bsfw83`.**
- *Upstream report: the Python SDK's card canonicalization (reported).* Its
  `_clean_empty` drops a security requirement whose scope list is empty, so a
  card's `securityRequirements` (the ordinary bearer case) are not covered by
  its signature: one can be removed from, or added to, a signed card
  undetected. Reproduced on a2a-sdk 1.1.4 and 1.2.2 (2026-10-12), with
  controls; the SDK's `AuthInterceptor` then sends no credential, or a
  different declared scheme's. Reported privately to a2aproject/a2a-python
  through GitHub Security Advisories on 2026-10-12 (the advisory ID is in the
  maintainer's GitHub notifications; it stays out of this repository until
  published). This repository's `CHANGELOG.md` and
  `itk/interop/card_signature_interop.py` already described the behaviour
  publicly from 2026-10-10, which the report says. Related public threads:
  a2a-python#1278 and A2A#2122 (canonicalization of empty and REQUIRED
  fields; a maintainer there rules out changing signed bytes outside a major
  release). Do not file a public issue about the security consequence while
  the advisory is open.

**What the next session should do first.**
1. Read the first CI run of this branch. New or changed gates: the ledger
   check, the proto upstream check, Go 1.26 in tck/ci/pin-freshness, the Java
   peer's BOM overrides.
2. The ledger's remaining medium entries (`docs/compliance/spec-ledger.md`
   lists every open one, most severe first). The ones read and confirmed
   this session but not yet fixed: the client never checks card capabilities before
   streaming (a SHOULD, and checking would refuse servers that stream without
   declaring it). Done on `claude/tender-cray-bsfw83`: checked by default,
   with `ClientBuilder::without_capability_checks` for such servers.
3. Postgres was run this session too: `postgres_store_tests -- --ignored`,
   67 passed against postgres:16, including
   `caller_isolation_holds_on_the_tenant_postgres_stores`, which fails with
   the Postgres store keyed by tenant alone.
4. Done: `card_signature_interop.py` runs in CI (`tck.yml`, job
   `card-signature-interop`). Done: §9.5 mid-stream JSON-RPC error `data`
   is an array of typed details, and the client's lag predicates accept the
   old object shape too. Still open from that ledger area: the WebSocket
   binding (not a spec binding) sends errors with no `data` at all and
   rewrites every mid-stream error to -32000, dropping the code and the lag
   marker (ledger §12.4, spec line 3024, PARTIAL/medium); and unary
   responses drop `A2aError::data` (§9.5 MAY, PARTIAL/low).
   Done: §13.4 rate-limit refusals are `A2aError::rate_limited` (429 +
   `Retry-After` / `RESOURCE_EXHAUSTED` / `RetryInfo`). How this SDK's client
   takes it, read from the code and not tested end to end: over REST the body
   has no `ErrorInfo` reason, so `parse_aip193_error` returns `None` and the
   429 surfaces as `UnexpectedStatus` with `retry_after`, which
   `ClientError::is_retryable` (`retry.rs`) retries; over gRPC
   `RESOURCE_EXHAUSTED` maps to `UnexpectedStatus { 429 }` (`grpc.rs`), also
   retried. Over JSON-RPC it is `Protocol(InternalError)` with the `RetryInfo`
   in `data`, which the retry layer does not retry. Open: decide whether the
   JSON-RPC client should read `RetryInfo`. Decided 2026-10-12 and done on
   `claude/tender-cray-bsfw83`: it does, and retries.
   Done: SLIMRPC status table and key case; §3.1.4, §3.3.4 and §3.5.2
   tests; empty push `url`. The §12 PARTIAL that remains is a conflict
   between the SLIMRPC table and A2A §5.4 (push/unsupported-operation
   codes), which is upstream's to resolve.

**§13.1, as built.** Opt-in caller partitions: `RequestHandler::admit` runs
the interceptors and then records the caller in a per-request slot
(`TenantContext::scope_request` / `set_caller`), and the tenant-aware stores
key by `TenantContext::partition` (tenant, or tenant + U+001F + caller).
`TenantContext::current()` stays the tenant for the halt switch, quotas, audit
chains and metric labels. Spawned work re-enters both through
`TenantContext::capture()`. Rejected: a composite tenant string (each caller
would get its own halt scope, so halting a tenant would not stop its callers);
a nested future per method (clippy `large_futures`: 29-34 KB); a general
`TaskAuthorizer` hook (ownership would have to be stored in all four stores).
**Decided 2026-10-12 and done on `claude/tender-cray-bsfw83`: on by default
with an authenticating interceptor.** The rules, as built:
- `build()` resolves an unset isolation to `IdentifiedCallers` when an
  interceptor `authenticates()`, else `Shared`. Unset stores become the
  tenant-aware in-memory ones under isolation (1,000 partitions by default).
  `TenantStoreConfig` has no overall cap: each partition gets the whole
  `with_task_store_config` capacity (10,000 tasks by default), so that
  default store's worst case is now 1,000 times the old one. Documented, not
  changed; the maintainer was not asked about it.
- A request no interceptor identified (an unlabelled token or key, a JWT
  without `sub`) keeps the tenant's shared records under the default, as the
  maintainer was promised ("no change when there is no authenticated
  identity"); `isolate_tasks_by_caller()` (`EveryCaller`) still refuses it.
  The shared partition is the bare tenant, which `isolatable` keeps free of
  U+001F, so it never meets a caller partition.
- `build()` refuses a supplied task or push-config store that does not
  partition (agreed with the maintainer), and an approval gate that needs a
  second approver, including `Profile::Auditable`'s, since the approver
  could never reach the requester's task. The gate refusal was not in the
  question put to the maintainer: it follows their choice of a build error
  over silent sharing, and is theirs to reverse (the alternative is to
  resolve the default to `Shared` when such a gate is installed).
- Every handler built with an authenticating interceptor was read, in the
  workspace and out (TCK SUT, ITK agent, soak, examples, book): the ones that
  would no longer build call `share_tasks_across_callers()` or drop an
  explicit plain store. The ITK agent, outside the workspace, was
  compile-checked with `cargo check --locked` and `-D warnings`.

**Environment notes, new this session.**
- `pkill -f` / `pgrep -f` with a pattern from your own command killed the
  shell twice more. Find processes with `ps -eo pid,comm,args` filtered on
  `comm`, and kill by PID.
- Maven Central answers `curl` and osv-scanner's pom resolution with 429
  under load; `mvn` itself worked. A pom scan that cannot fetch the override
  BOMs falls back to Quarkus's own versions and reports advisories the build
  does not contain; check the packaged jars.
- `prove_workflow_gates_fail.py` needs `ssh-keygen` (`apt-get install
  openssh-client`) and free ports; a SUT left running fails it with
  "Address already in use".
- osv-scanner: `go install github.com/google/osv-scanner/v2/cmd/osv-scanner@v2.2.3`.

## `claude/bold-gauss-9qc9dc` — compliance, audit and human oversight

The maintainer asked to make a2a-rust the most compliant and verifiable A2A
SDK, measured against the EU AI Act
(`docs/compliance/control-map.md` is the map). The work came as a plan in six
phases, A–F; this branch carries all six.

* **A (pushed: `5683d09f`, `ae2efbce`, `d76ed2f7`, `df6daf1e`).**
  - HTTP+JSON negotiates `application/a2a+json` (REST-CT-001). ACTS against
    a2a-itk `82458cea` scored 101/101, 88/88 and 94/94.
  - HS256 secrets shorter than 32 bytes fail closed.
  - New workflows: OSV, CodeQL, Scorecard.
  - Release tags must be signed by a key in `.github/release-signers/`.
  - The compliance control map, with a CI gate.
* **B, the audit trail.** ADR 0015 and the book's "Audit Trail" chapter
  describe it.
* **C, human oversight.** The book's "Human Oversight" chapter describes it.
  It adds `delegation::Delegation` in the client (G1-A), with
  `examples/swarm` rewritten on it; `RequestHandler::halt`/`resume`; and the
  approval extension with `ApprovalGate`.
* **D, provenance (`6f2df2e3`).** The book's "Provenance and Signatures"
  chapter describes it.
  - The provenance extension: an AI-generated marker and signed content.
  - EdDSA and JWK Set verification for cards, and the client's `fetch_jwks`.
  - `verify_agent_card` accepts SPKI as its docs always said it did.
  - The HS256 and OAuth client secrets are zeroized.
  - An upstream draft of the approval and provenance extensions.
* **E, verification (`6bf6ca12`).**
  - Kani proofs in the types crate.
  - A loom model of the halt protocol (`verification/loom-halt`).
  - ACTS as a merge gate (`acts.yml`), and a signed conformance attestation
    on released crates.
  - A nightly ACTS run, and the ITK dashboard enrolment draft.
* **F, developer experience.**
  - `Profile::Auditable`.
  - Offline CLI commands: `a2a card sign|verify` and `a2a audit verify`.
  - The CLI carries the release version.
  - Release binaries behind `PUBLISH_CLI_BINARIES`.
  - Homebrew and winget drafts under `packaging/`.

**Decisions the maintainer made (2026-10-08).**
- Commits are authored as Tom F. with sign-off.
- Release tags are signed with the maintainer's own SSH or GPG key, and CI
  verifies them.
- New distribution channels (the CLI, Homebrew, winget) are drafted in-repo
  only; the maintainer publishes.

**Waiting on the maintainer, not on code.**
- Commit the release public key to `.github/release-signers/allowed_signers`
  (or `gpg-fingerprints.txt`) before the next tag. Until then
  `release.yml` refuses every tag, by design.
- Register the OpenSSF Best Practices badge. `docs/openssf-best-practices.md`
  has the answers.
- Add a `v*` tag ruleset.
- OSV, CodeQL and Scorecard trigger on `main` and pull requests only, so they
  had not run on GitHub when this was written.

**First thing after this branch merges.** Add
`--lockfile=verification/loom-halt/Cargo.lock` to the `rust-pr` job in
`osv.yml`. It was left out because the PR scan also scans the base, and the
base did not have the file yet (`osv.yml` explains). The push and scheduled
`rust` job already scans it.

**Two halt designs were rejected (2026-10-09).**
- *Writing `Canceled` from the halt, as `CancelTask` does.* It was measured
  to stall a send that the halt caught mid-admission for exactly the 5 s
  queue write timeout: the terminal write waits for a background processor
  that does not exist yet.
- *Re-checking the halt after the row is written and deleting the row.* That
  would delete an existing task when the send was a continuation.

The landed design marks the turn halted and fires its token; the executor's
own post-run hook writes `Canceled`, as on shutdown. The race test failed
with the admission re-check removed and passes with it.
`a_turn_admitted_after_the_walk_stops_itself` covers the same path
deterministically.

**The mutation gate is slow on this diff (counts as of 2026-10-09).**
`--in-diff` over B and C finds 586 mutants, and 642 once the diff was
regenerated after the first fixes. It runs in two-hour chunks:

| Chunk | Caught | Missed | Unviable | Output |
|---|---|---|---|---|
| 1 (stopped early to free disk) | 50 | 2 | 22 | `/tmp/claude-0/mutBC-partial-1` |
| 2 | 38 | 3 | 21 | overwritten by chunk 3 |
| 3 (stopped at its 2-hour cap) | 50 | 12 | 186 | `/tmp/claude-0/mutBC/mutants.out` |
| 4 (the rest of the B–F diff, `df6daf1e..HEAD`, 465 mutants) | 203 | 30 | 232 | same directory |
| Re-run of chunk 4's 15 real misses and their neighbours (24 mutants) | 23 | 0 | 1 | `/tmp/claude-0/mutRe` |

Chunks 1–3 tested 384 of the 642, and chunk 4 covered the rest of the B–F
diff, so the sweep is complete for this branch. Of chunk 4's 30 misses, 15
were the Kani harnesses, which only compile under `cfg(kani)`; they now
carry `cfg_attr(test, mutants::skip)` with the reason in
`kani_proofs.rs`. The other 15 now have tests, and a re-run of exactly those
positions misses none. The scratch paths do not survive the session.

Every miss so far is fixed in the commits on this branch, or the code was
simplified so the mutant no longer exists:

- the `Delegation::cancel` guard and the `export` short-page check;
- `failures()` and the anchor continuation;
- `end_run`'s eviction order;
- the `Debug` impls;
- `record_event`'s arms;
- the wrapper's three capability flags.
- chunk 4: `verify_chain` on a twice-purged chain among other chains'
  checkpoints, `MAX_SEQ`, `digest_of`, `CheckpointSigner::kid`, a mid-sized
  JWK Set, and `card verify` from URLs. The JWK coordinate length check was
  rewritten as a fixed-size conversion, so its equivalent mutant no longer
  exists.

Most "unviable" mutants replace a `BoxFuture` return with a value that does
not type-check. Resume with `cargo mutants --iterate --in-diff <diff>
--output <dir>`, without `RUSTFLAGS` and with `CARGO_INCREMENTAL=0`; each
chunk excludes what earlier ones caught.

**What the next session should do first.**
1. Run the mutation sweep again only over what changes next; this branch's
   diff is fully swept (above).
2. Read the first GitHub runs of `acts.yml`, `verification.yml`, `osv.yml`,
   `codeql.yml` and `scorecard.yml`, which only trigger on `main` or pull
   requests. Fix what they find.
3. ROADMAP's "Audit, oversight and provenance — what is left": a halt that
   holds across replicas, and G1-B.

**Environment notes.**
- Run every cargo command with `CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0`.
  Without them `target/` plus two mutants copies filled the disk three times.
- Postgres for the store tests: `sudo dockerd &`, then a `postgres:16`
  container on the host network, with
  `A2A_TEST_POSTGRES_URL=postgres://postgres:postgres@localhost:5432/postgres`.
- Run mutants without `RUSTFLAGS`, as `mutants.yml` does; `-D warnings`
  turns caught mutants into unviable ones.
- Kani 0.68.0 is installed with `cargo install --locked kani-verifier`, then
  `rustup set auto-self-update disable`, then `cargo kani setup`. Without the
  middle step, setup fails on a rustup self-update.
- Never `pkill -f` a pattern that appears in your own command: it kills the
  shell running it. That happened again on 2026-10-09; kill by PID.

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

*(2026-10-02: the maintainer asked for work toward orchestrating large agent
swarms. [`swarm-orchestration.md`](swarm-orchestration.md) reconciles that
request with the paragraph below. Orchestration lives in `examples/swarm`.
The protocol crates gain only a client-side helper and declared extensions;
no scheduler, registry or mesh.)*

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

## `claude/zealous-lovelace-n5ulfc` — telemetry out of the box, and Langfuse

Started 2026-10-06 from `20b162b`. The ask: traces, metrics and logs out of
the box, Langfuse as a first-class destination, and every gap found on the way
filled. Design and measurements are in [ADR 0013](adr/0013-observability.md)'s
2026-10-06 section; what a user does is in the book's Observability and
Langfuse pages. This section records what the code does not.

**What landed.** `otel::Telemetry` (ADR 0013 option 5, which had never
landed); `otel::Langfuse`; the draft A2A attributes on the `SERVER` span; the
executor span as GenAI `invoke_agent` (a renamed span — Behaviour Changes);
opt-in message capture; a `CLIENT` span per call and, with the new client
`otel` feature, automatic `traceparent` from it; export failures reported by
`shutdown`; `examples/langfuse-agent`.

**Verified against a real Langfuse.** Self-hosted Langfuse 4.53.0 from
`langfuse/langfuse@1a21a42`'s `docker-compose.yml`, test keys through
`LANGFUSE_INIT_*`, queried through `/api/public/v2/observations` (v4's
`events_only` mode refuses the v1 observations and traces endpoints). Three
probes, kept in the session scratchpad rather than the repository, then
the example:

1. The baseline, hand-wired the way a user would have had to: two untyped
   SPANs, no session, no input or output, no client span. The obvious wiring
   (default blocking HTTP client, simple processor, inside Tokio) panicked and
   exported nothing.
2. `Telemetry` + preset + content capture: the executor as an AGENT
   observation `invoke_agent hello-agent`, session = the A2A `contextId`,
   input and output in the GenAI message format — the same JSON-string shape
   as Langfuse's own fixture
   (`packages/shared/src/utils/normalized-io/conventions/providers/otel-genai/fixtures.ts`).
3. Caller → orchestrator → worker with no propagation code: seven
   observations, one trace, each the child of the one before.

4. `cargo run -p langfuse-agent` itself: seven observations, one trace,
   one parent chain, the session on every observation but the root, input
   and output on both AGENT observations — the tree its README draws. (The
   first attempt hit Docker Hub's `429 Too Many Requests` re-pulling images
   deleted to free disk; the retry succeeded. Run against an unreachable
   endpoint, it printed the export error on stderr and exited.)

**Lessons, each of which cost something:**

- **Read the convention's YAML, not a summary of it.** A research summary gave
  `a2a.method.name`'s values as `send_message`; those are the member ids, and
  the values are `SendMessage`.
- **A test that passes alone can still be a real bug.** The Langfuse-preset
  test failed every run in the suite and passed alone; the cause was a level
  hint of `OFF` from an absent layer, which hid every span whenever logs were
  off.
- **The gate caught what the unit tests did not.** The SDK's
  `observability_e2e` found the duplicated `a2a.task.state`; its duplicate-key
  check now fails any repeated attribute (proven by injection).
- **Docker images are not a cache to delete casually.** Re-pulling Langfuse's
  hit the anonymous pull rate limit.

**Decided 2026-10-08 by the maintainer:** the Langfuse preset and every page
default to a self-hosted Langfuse (`http://localhost:3000`, Langfuse's own
compose address) and lead with self-hosting, so the project does not appear
to endorse Langfuse's hosted service. Cloud is documented as an alternative
only. Keep it that way in anything new.

**Not done, with why:**

- Mutual TLS to the collector (`OTEL_EXPORTER_OTLP_CLIENT_*`): not
  implemented; documented as unsupported.
- `http/json` OTLP: not compiled in; refused with a message.
- A chain that crosses languages (a Python agent traced into the same Langfuse
  trace): not run. `traceparent` is W3C, so it should join; unverified.
- Langfuse Cloud: not run; no credentials in this environment.
- Upstream: five drafts, indexed in `docs/upstream/README.md`, none posted. Posting is
  the maintainer's decision.

**What the next session should do first:** decide which `docs/upstream/`
drafts to send, and run the example against Langfuse Cloud before the
Langfuse docs draft goes anywhere.
