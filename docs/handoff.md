<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Handoff — session state

Working state between sessions: which branches exist and why, what is in flight
outside this repository, and what the next session should pick up.

This is **not** `ROADMAP.md`. That file takes only items the repository has
committed to and refuses speculative milestones; this one records where things
stand, including decisions to *not* do something. When an item here becomes work
the repository commits to, move it there and delete it here.

Last updated 2026-09-13.

## Branches

| Branch | Head | What it is |
|---|---|---|
| `claude/friendly-keller-edrezx` | `caac0ec` | The working branch. Four documentation commits, no code. |
| `claude/a2a-rig-held` | `caa8774` | Storage. The unpublished `a2a-rig` crate, one commit on top of `caac0ec`. |
| `claude/adk-rust-0.12-patch` | `6fbdd2f` | Storage. The outbound adk-rust patch as a file, one commit on top of `caac0ec`. |

Both storage branches are **not destined for `main`**. They exist so work
survives the session that produced it; delete either once its contents have
landed somewhere better.

### Position relative to `main`

`origin/main` is at `c5d1379`. The working branch is **4 ahead, 6 behind**. None
of the six commits on `main` touches a file this session edited, so a merge is
clean on our side — verified with `git diff --name-only 518bac6..origin/main --`
over the edited paths, which returns nothing.

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

## Deliberately not done

- **The TCK figures are dated measurements, not gates.** Seven places claimed
  "20/20"; measured 2026-09-13, `rig-a2a-agent`, `genai-a2a-agent` and
  `incident-response` each report 21/21 graded with 1 N/A on the JSON-RPC binding.
  Each site now carries the date and says no CI job gates it. Making it a gate
  needs a job per agent — `tck.yml` currently points only at `echo-agent` (`:24`,
  `:35`) and `a2a-tck-sut` (`:118`) — and that is a CI-cost decision nobody has
  made.

## What to pick up first

1. Merge `main` into the working branch, or rebase onto it. Six commits behind,
   no overlap, should be uneventful.
2. Submit the adk-rust work if it is still wanted: issue first, then the patch.
3. Decide the TCK gating question above, either way.
