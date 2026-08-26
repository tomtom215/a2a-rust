<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Readiness review — what a critical external reviewer would find

**Measured 2026-08-26 against `7093af3`.** Written for the review this project
keeps saying it wants: a Linux Foundation or A2A technical review, or an adopter
of `a2a-protocol-types` — which is past 30,000 downloads and therefore has users
who will never open an issue, only stop upgrading.

## Method, and what that means for these findings

Every claim below was produced by running something, and the command is named so
it can be re-run. Where a figure could not be established first-hand it is
marked **UNVERIFIED** and is not counted as either a strength or a defect.
Section 5 lists what was not examined at all, which is the part of a review most
likely to be quietly omitted.

One instrument was wrong before it was trusted, and it is worth recording
because it is the exact failure this repository has documented twice already:
`git rev-list --count origin/main` returned **431** against a document claiming
749. The clone was shallow. A shallow clone truncates the *oldest* history,
which is where the non-compliant commits live, so it does not produce a figure
that is merely wrong — it produces a flattering one. Everything in §3 was
re-measured after `git fetch --unshallow`.

## 1. What holds up

Verified, not taken from a document that claims it:

| Property | Evidence |
|---|---|
| No `unsafe` anywhere in the published crates | `grep -rn 'unsafe ' crates/*/src` → 0, and all four carry `#![forbid(unsafe_code)]` |
| Panic surface in runtime library code | **0 `.unwrap()`** — all three in the tree are in `build.rs`. 10 `.expect(`, each read: four are rustls static provider config, two are provably unreachable, three are a documented deliberate fail-fast on lock poisoning (a silent `None` there would be an auth downgrade), one more is rustls. **0** `panic!`, `todo!`, `unimplemented!` |
| Lints | `#![deny(missing_docs)]`, `#![warn(clippy::all, pedantic, nursery)]` on all four crates; CI runs clippy with `-D warnings` across a feature matrix on stable **and** MSRV 1.93, on three operating systems |
| Test population | 2,830 `#[test]`/`#[tokio::test]` functions in `crates/`, 79 in the SLIMRPC binding, 16 in examples |
| Licensing | Apache-2.0, `NOTICE`, and an SPDX header in **502 of 502** tracked source files |
| Governance completeness | `CODE_OF_CONDUCT`, `CONTRIBUTING`, `GOVERNANCE`, `MAINTAINERS`, `SECURITY`, `SUPPORT`, `DCO`, `CITATION.cff`, `.github/CODEOWNERS`, issue and PR templates — all present |
| crates.io / docs.rs metadata | All four crates carry every field a reviewer looks for, including `docs.rs` metadata and `rust-version` |
| Supply chain at release | SLSA build provenance attestations **and** a per-crate CycloneDX SBOM, both produced in `release.yml`; `cargo-deny` runs on the workspace and separately on the binding's 379-dependency tree |
| Action pinning | Every action SHA-pinned except one — see §4 |

That figure is now a CI ratchet — `scripts/check_panic_paths.py`, which closes
B5 — so it does not decay: adding a `.unwrap()` to library code fails the build
until the baseline is updated deliberately.

The panic surface is the one worth dwelling on, because it is the question an
adopter of a protocol library actually has: *can a malformed peer take my
process down?* A naive `grep` cannot answer it — it counts matches
inside doc comments, string literals, and `#[cfg(test)]` modules, which is why
no number had been quotable. The measurement above strips comments and literals
with a state machine and excises test modules by brace matching **and** by
resolving `#[cfg(test)] mod name;` declarations in parent modules. The first run
of that tool was wrong in exactly the predictable way — its top hits were all
`*tests.rs` files, whose gating attribute lives in the parent — and the number
was only quoted after a self-check for test-named files that survived the
filter reported one, which turned out to be an `include!`d vector file inside a
`#[cfg(test)] mod tests`. A second, subtler miss surfaced when the ratchet was
built: `#[cfg(test)]` gating is **inherited**, so a file declared by a
test-gated module is test code too even though it carries no attribute and no
"test" in its name. Both filters are pinned by `--self-test` now.

## 2. What was wrong, and is now fixed

Seven defects, each verified before and after.

1. **The binding's packaging gate could not pass during a release** (B23).
   No pin value was green between the version bump and publication, in either
   direction — both rows re-measured. `scripts/package_binding.py` now skips
   registry resolution alone for that one state and fails on everything else;
   six states proven end-to-end.
2. **The nightly canary tested but never linted**, and the breakage it exists to
   warn about was a lint breakage. Two of the three lints that broke `main` did
   not exist in the previous toolchain at all.
3. **The SLIMRPC spec check was blind to any spec file it had not been told
   about.** Upstream has carried `slimrpc-collaborative-channel.md` on a branch,
   and the official `a2a-slimrpc` crate has implemented it, with nothing here
   noticing for two review passes.
4. **The divergence from the official crate was undocumented.** Neither crate is
   a superset of the other; that is now a table in three places, plus B24.
5. **The provenance manifest had gone stale in the project's favour** — it
   claimed 19.4% of history passes the project's own DCO gate when the figure
   had reached **39.2%**. Now re-measured and gated at release time.
6. **`SECURITY.md` understated release verifiability**, telling a reviewer there
   was "nothing to verify" about any tag when `v0.8.0` and `v0.9.0` are
   annotated objects carrying a tagger and a date.
7. **The SQLite connection pragmas were written out four times**, byte-identical
   and unguarded. The previous review found two of the four.

Two of these — 3 and the discovery gap found while fixing 5 — are the same
defect class this repository names everywhere else: *a check that verifies what
it knows and is silent about what it does not.* That class survived four prior
review passes. It is worth assuming there is more of it.

## 3. Provenance, as counsel would read it

Re-measured at `7093af3` with the repository's own generator:

| | |
|---|---:|
| Commits reachable | 977 |
| Non-merge commits (the population `dco.yml` grades) | 870 |
| **Would pass the project's own DCO gate** | **341 — 39.2%** |
| Fail — AI-authored (`noreply@anthropic.com`) | 477 |
| Fail — bot-authored | 36 |
| Fail — human author, no matching sign-off | 16 |
| Commits since the policy changed (`b416c1a`) that are AI-authored | **0 of 369** |

The passing count more than doubled since the previous measurement while every
failing count stayed put except the bot's. That is the shape a closed pattern
makes, and it is a materially stronger position than the document was stating.

## 4. Open, ranked by what a reviewer would raise first

1. **No dependency-update automation.** No Dependabot, no Renovate. This is a
   standard OpenSSF Scorecard finding. It was **not** added here because of a
   real conflict that needs a decision: `dco.yml` rejects any author matching
   `*[bot]@users.noreply.github.com`, so every Dependabot pull request would
   fail the project's own DCO gate on arrival. Adding an automation whose output
   is permanently red is worse than not adding it. The options are an explicit
   bot exemption in `dco.yml`, or a documented human re-authoring step.
2. **`dtolnay/rust-toolchain` is the only action not SHA-pinned** — three refs
   (`@stable`, `@nightly`, `@master`). Not pinned here on purpose: the pin must
   be applied together with an explicit `toolchain:` on every job, and getting
   that wrong silently redirects the MSRV leg of the matrix at the wrong
   compiler — an MSRV job that no longer checks the MSRV. That is a worse defect
   than the one being fixed, and a silent one. It should be done deliberately,
   with CI observed.
3. **Release tags are not signed.** Annotation is now enforced and working;
   signing needs a maintainer key and a documented way for adopters to obtain
   it. Already stated honestly in `SECURITY.md` and `ROADMAP.md`.
4. **No PGP key for security reports.** Already stated honestly in
   `SECURITY.md`, which directs reporters to GitHub Security Advisories instead.
5. **Example test coverage is the hole, not example count.** Measured:

   | example | LOC | files with tests |
   |---|---:|---:|
   | `incident-response` | 3,498 | **0** |
   | `harness` | 920 | 0 |
   | `echo-agent` | 692 | 0 |
   | `multi-lang-team` | 679 | 0 |
   | `rig-agent` | 647 | 0 |
   | `genai-agent` | 632 | 0 |
   | `deploy-agent` | 416 | 0 |
   | `hello-agent` | 142 | 0 |
   | `agent-team` | 8,911 | 16 |

   `incident-response` at 3,498 lines with nothing exercising it is the one a
   reviewer will name.
6. **`docs/rust-sdk-assessment.md` is a dated deliverable addressed to "Linux
   Foundation / A2A project technical leadership"** whose figures (608 commits,
   ten tags) are superseded. It carries its date, which is defensible, but a
   reviewer handed it today will read stale numbers. It needs a supersession
   note pointing at the manifest.
7. **Four book chapters named in B10 are absent** from `SUMMARY.md`:
   troubleshooting, observability, multi-tenancy, security.

## 5. What this review did not examine

Stated so that a clean report is not mistaken for a complete one.

* **The rendered documentation site.** `a2a-rust.com` was never loaded. Link
  integrity, search, and whether `api-reference.md`'s name list is still
  accurate are all unassessed.
* **A full CI run.** Individual gates were run locally on **rustc 1.94.1**; CI
  uses `stable`, which has since moved to 1.98 and is what broke three lints on
  unchanged code earlier. A green local run here does not imply a green CI run,
  and this session deliberately does not claim one.
* **The full test suite.** Subsets were run — the SQLite store, push config
  store, and the new pragma test. `cargo test --workspace --all-features` was
  not run to completion.
* **`cargo deny` was not executed.** Its configuration is present in both
  places; that it currently passes is **UNVERIFIED** here.
* **Coverage, mutation, fuzz, soak, benchmarks, and the cross-language TCK.**
  All have workflows; none were run.
* **The published crates on crates.io** were not compared against this tree.
* **Cross-implementation SLIM interop** with the official `a2a-slimrpc` crate.
  Identical dependency pins make it plausible. Nothing tests it, and plausible
  is not tested.
