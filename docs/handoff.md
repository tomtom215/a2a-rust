<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Handoff — session state

Working state between sessions: which branches exist and why, what is in flight
outside this repository, and what the next session should pick up.

This is **not** `ROADMAP.md`. That file takes only items the repository has
committed to and refuses speculative milestones; this one records where things
stand, including decisions to *not* do something. When an item here becomes work
the repository commits to, move it there and delete it here.

Last updated 2026-09-19 (second session: the panic-hook fix and the type constructors).

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
| `claude/wizardly-tesla-0f358t` | open — see note | **Destined for `main`.** Three examples: tool calling in `examples/rig-agent`, then `examples/mcp-agent` (tools over MCP) and `examples/mcp-bridge` (an A2A agent exposed *as* MCP). On top of `fa1a82b9`. No PR opened yet. |

`release/v0.12.1` can be deleted. The two **storage** branches —
`claude/a2a-rig-held` and `claude/adk-rust-0.12-patch` — are **not destined for
`main`**. They exist so work survives the session that produced it; delete
either once its contents have landed somewhere better.

### `claude/wizardly-tesla-0f358t` — tool calling, and what the live run found

No head SHA in the row above, deliberately: this file lives on that branch, so
any commit recording a head invalidates the head it recorded. That is the trap
`dacfc88` fixed for the 0.12.1 branch and it re-forms every time. Read the
branch with `git log --oneline origin/main..claude/wizardly-tesla-0f358t`.

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

**One caveat, because it is the honest limit of what was checked.** `tck.yml`
triggers only on push to `main` and pull requests targeting `main` (`:6`-`:9`),
so the job has not yet run on a GitHub runner — it first executes when this
work reaches a pull request. What *was* exercised, on this machine, is every
leg's full sequence using the workflow's own startup blocks and the same
`a2a-tck --binding jsonrpc` invocation: all three reach 21/21 with exit 0, and
the `incident-response` leg brings its two dependencies up and answers on
`:9200` within a second of each poll starting. What that does not cover is the
runner environment itself — a clean build and free ports. Watch the first
`TCK` run on the pull request.

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

### The `RequestContext` blind spot is the single biggest constraint

An executor cannot see caller identity, tenant, HTTP headers, or the
activated extension set. `build_request_context`
(`handler/messaging/create.rs`) takes no `CallContext`, and `tokio::spawn`
drops `TenantContext`, so the executor observes tenant `""` — stated at
`handler/mod.rs:157-160`.

This was hit directly building `mcp-bridge`: the only channel for getting
the caller's chosen skill to the agent was `Message.metadata`, because there
is no supported alternative. The consequence in general is that **an
executor cannot enforce "only this tenant may invoke this skill"**, which
rules out a large class of real deployments. Every workaround fails —
`ServerInterceptor::before` runs before the task id exists, so there is not
even a key to stash something under.

Of everything in this file, plumbing `CallContext` into `RequestContext`
would most expand what people can build on top.

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
  `handler/limits.rs:44-84` — but a reader who sees `max_attempts: 3` and
  does not open the other file gets one attempt and no warning.

## What to build next, ranked

Ordered by value per unit of work, from the seat of someone who consumes
agents rather than maintains the protocol.

1. **Trace context as a protocol concern.** Finding 6. Biggest gap, clearest
   differentiator, and it uses hooks that already exist.
2. **Plumb `CallContext` into `RequestContext`.** Unblocks auth-aware
   executors, per-tenant policy, and every higher layer anyone would build.
3. **The executor conformance harness (A4 above) — move it up.** Three
   executors were written this session; all three got the happy path right
   and none is tested against cancellation arriving mid-artifact, an
   `input-required` never answered, or a client disconnecting mid-stream,
   because writing those by hand is exactly the work people skip. This
   project already believes in the tooling: `cargo mutants` is the same
   instinct pointed at tests.
4. **A typed failure taxonomy shipped as a declared extension**, with the
   client's retry policy consuming it. Idempotency proved the extension
   pattern works end to end.
5. **Make the event log the record and state the fold.** The one
   architectural change worth making if only one can be made. It kills
   #130-class bugs by construction, gives exact resumption from an offset
   instead of snapshot-and-hope, makes the hand-rolled `tool-trace` artifact
   unnecessary, and is the substrate signed execution receipts need.
   Everything in Part B above gets easier downstream of it.
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

## Still open

Numbering was 1, 2, 4, 5 here — there was never a 3. Renumbered.

1. Delete `release/v0.12.1`, whose contents are merged and tagged.
2. Submit the adk-rust work if it is still wanted: issue first, then the patch.
3. The binding's `RUSTSEC-2026-0285` waiver — see its section above for the
   command that says when it can be deleted.
4. ~~Stale install snippets.~~ Done — see "Prose versions are checked now"
   below. The figure recorded here first, six, was wrong: it counted only
   `crates/`, and the real number was 28.
5. ~~Re-run `prove_gates_fail.sh` for the three `--features {sqlite,postgres,
   auth-jwt}` gates.~~ Done — 9 proven, 0 unproven, including three that were
   PRE-BROKEN only for want of a local PostgreSQL. Still unrun on this branch:
   the other 57 gates, and the two remaining PRE-BROKEN ones (SLIMRPC SPIFFE,
   and `cargo hack clippy` with `cargo-hack` absent).
6. ~~The two hand-rolled `uuid_like()` helpers.~~ Done — both examples take
   `uuid` now.

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
