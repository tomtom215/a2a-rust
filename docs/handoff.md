<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Handoff — session state

Working state between sessions: which branches exist and why, what is in flight
outside this repository, and what the next session should pick up.

This is **not** `ROADMAP.md`. That file takes only items the repository has
committed to and refuses speculative milestones; this one records where things
stand, including decisions to *not* do something. When an item here becomes work
the repository commits to, move it there and delete it here.

Last updated 2026-09-19.

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
| `claude/wizardly-tesla-0f358t` | open — see note | **Destined for `main`.** Tool calling in `examples/rig-agent`, on top of `fa1a82b9`. No PR opened yet. |

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
clean runs; TCK against the live agent 21/21 graded, 0 failed, 1 N/A, which is
the figure `tck.yml` gates; no-model surface sweep 44/44, exit 0; fmt, workspace
clippy, file-lengths, doc-versions, book-code, doc-escapes, block-scalars,
api-reference, sitemap `--check` and DCO all clean.

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

## Still open

1. Delete `release/v0.12.1`, whose contents are merged and tagged.
2. Submit the adk-rust work if it is still wanted: issue first, then the patch.
4. The binding's `RUSTSEC-2026-0285` waiver — see its section above for the
   command that says when it can be deleted.
5. ~~Stale install snippets.~~ Done — see "Prose versions are checked now"
   below. The figure recorded here first, six, was wrong: it counted only
   `crates/`, and the real number was 28.
