<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Handoff — session state

Working state between sessions: which branches exist and why, what is in flight
outside this repository, and what the next session should pick up.

This is **not** `ROADMAP.md`. That file takes only items the repository has
committed to and refuses speculative milestones; this one records where things
stand, including decisions to *not* do something. When an item here becomes work
the repository commits to, move it there and delete it here.

Last updated 2026-09-17.

## Branches

| Branch | Head | What it is |
|---|---|---|
| `claude/friendly-keller-edrezx` | *see `git log`* | The content branch: four documentation commits, the issue-130 fix `5ee3cfb`, the rustls security fix `eaf038c`, the merge from `main`, and revisions of this file. Its head moves every time this file is edited, so it is not pinned here. |
| `release/v0.12.1` | *see `git log`* | The 0.12.1 release prep, branched from the content branch rather than from `main` because `main` does not carry the content yet. Everything but the provenance manifest. Merge `main` into it once the content PR lands. |
| `claude/a2a-rig-held` | `caa8774` | Storage. The unpublished `a2a-rig` crate, one commit on top of `caac0ec`. |
| `claude/adk-rust-0.12-patch` | `6fbdd2f` | Storage. The outbound adk-rust patch as a file, one commit on top of `caac0ec`. |

The two **storage** branches — `claude/a2a-rig-held` and
`claude/adk-rust-0.12-patch` — are **not destined for `main`**. They exist so work
survives the session that produced it; delete either once its contents have
landed somewhere better.

### Position relative to `main`

`main` is merged in as of `3ee6f95`. The working branch is **11 ahead, 0
behind**, and there is nothing left to reconcile. The one conflict was
`CHANGELOG.md` — both sides had replaced `## [Unreleased]` / "Nothing yet." with
their own entries — resolved by keeping both and reordering the four sections to
Changed, Fixed, Security, Internal, which is Keep a Changelog's order and
0.11.0's.

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

## Issue #130 — fixed on this branch, not yet released

[#130](https://github.com/tomtom215/a2a-rust/issues/130) (`valliscooper`,
2026-09-15): a second message on an existing context, sent without a `taskId`,
returned a *new* task already carrying the previous task's artifacts, so each
round returned the whole context's accumulated set. Confirmed, reproduced, and
fixed in `5ee3cfb`; `history` and `metadata` leaked the same way and are fixed
with it. The reporter has not been answered yet — the issue is still open and
carries no reply.

The fix is on this branch only. It is in `CHANGELOG.md` under `[Unreleased]`,
classified there as the `STABILITY.md` §2 specification correction, which is
what makes it patch-eligible rather than a minor bump.

## RUSTSEC-2026-0285 — fixed in the workspace, waived in the binding

The advisory (rustls below 0.23.45 accepting TLS 1.3 handshake messages at the
wrong encryption level) was published after `main`'s last green CI run and
turned both `cargo-deny` jobs red on this branch without any change here
causing it. `main` fails the same way if re-run.

Fixed for the four published crates in `eaf038c`: the workspace lockfile moves
rustls 0.23.44 to 0.23.45, two lines, no manifest change.

**Not fixed for `bindings/a2a-protocol-slimrpc`, and it cannot be from here.**
rustls 0.23.45 needs `aws-lc-rs ^1.18`; `mls-rs-crypto-awslc 0.23.0` — the only
release in the range `agntcy-slim-auth 0.15.4` admits — pins `aws-lc-rs
=1.16.2`. The binding's `deny.toml` carries a dated ignore with the full chain.
**Re-check it at the next binding release**: run `cargo update -p rustls
--precise 0.23.45` from `bindings/a2a-protocol-slimrpc/`; when it succeeds,
delete the ignore and commit the lockfile instead. Do not let the waiver
outlive the constraint.

## What to pick up first

Release 0.12.1 as **two** pull requests, in this order. They cannot be one, and
the reason is mechanical rather than stylistic — see below.

1. **Content PR — `claude/friendly-keller-edrezx` into `main`.** Ready: 11
   ahead, 0 behind, `cargo test --workspace` 3257 passed / 0 failed / 175
   ignored across 107 binaries, fmt clean, file-length ratchet clean, `cargo
   deny` clean on both trees, and the blocking incremental-mutation gate clean
   (2 mutants from the diff, 1 caught, 1 unviable, 0 missed). Everything in it
   sits under `[Unreleased]`; no version number moves.

2. **Release PR — `release/v0.12.1`.** Its content is **already written** and
   pushed. `RELEASING.md` §1 says to cut this branch from `main`; it is instead
   cut from the content branch's head, because `main` does not carry the
   content yet. **After (1) merges, bring `main` in** (`git merge origin/main`
   from this branch) so the pull request shows only the release diff. What it
   already does:
   - `0.12.1` in the four crate `Cargo.toml` files **and their eight
     inter-crate pins**, plus both lockfiles;
   - `CHANGELOG.md` dated `## [0.12.1] - 2026-09-17` with a fresh empty
     `[Unreleased]`; `CITATION.cff` at 0.12.1 / 2026-09-17;
   - `ROADMAP.md`'s current-release line; a v0.12.1 section in the book's
     changelog page; the two stale live version statements in `RELEASING.md`
     and the book's upgrading page;
   - `RELEASING.md` §1 expanded to name the eight pins, both lockfiles and the
     two unchecked files, which it never did.

   Every gate in `release.yml`'s validate job was simulated against this tree
   and passes — crate versions, dated CHANGELOG heading, notes extraction,
   `CITATION.cff`, `SECURITY.md` — with one exception, below.

   **The one thing left: the provenance manifest.** Run
   `scripts/provenance_manifest.sh HEAD` and commit
   `docs/provenance-manifest.md` as the **last** commit before the tag. It is
   deliberately not done yet: `check_provenance_manifest.py` passes only when
   the manifest's pinned commit differs from the tagged commit by nothing but
   the manifest file itself, so it has to be regenerated after everything else
   on this branch is final, including the merge from `main`.

3. Then `git tag -a v0.12.1` — annotated. `release.yml` rejects a lightweight
   tag, and the GitHub release UI creates lightweight ones. The tag runs six
   stages and the last, the crates.io publish, enters the `crates-io`
   environment, which **requires a manual approval** — the release will stop
   and wait there. Credentials come from Trusted Publishing over OIDC scoped to
   this repository, this workflow file and that environment, with
   `CARGO_REGISTRY_TOKEN` as fallback; none of that triple changed since
   0.12.0, but it is not verifiable from a session clone.

**Why the release prep cannot ride in the content PR.**
`check_provenance_manifest.py`, which `release.yml` runs against the tagged
commit, passes only when the manifest's pinned commit is the release commit or
differs from it by *nothing but the manifest file itself*. Regenerating it in
the content PR and then landing version bumps on top puts four `Cargo.toml`
files and `CHANGELOG.md` between the pin and the tag, and the check fails. The
check is in `release.yml` only — `ci.yml` does not run it — so a stale manifest
does not redden the content PR.

**The shallow-clone obstacle is gone.** An earlier note here said this could not
be done from a session clone. `git fetch --unshallow` succeeded; the clone now
carries full history and `check_provenance_manifest.py` runs properly.

Two things the release does **not** need: `SECURITY.md` already covers `0.12.x`
— verified against the workflow's own regex — and the SLIMRPC binding needs no
follow-up release, because `RELEASING.md` requires one only after an SDK
*minor* and the binding's requirement on the three SDK crates is
`version = "0.12"`, which `0.12.1` satisfies.

## Still open, unrelated to the release

1. Submit the adk-rust work if it is still wanted: issue first, then the patch.
2. Decide the TCK gating question above, either way.
3. The binding's `RUSTSEC-2026-0285` waiver — see its section above for the
   command that says when it can be deleted.
