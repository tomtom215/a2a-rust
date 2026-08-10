<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Provenance Manifest

**Measured 2026-08-11 at `c008ab0`. Regenerate with `scripts/provenance_manifest.sh`.**

This document exists to answer one question in one pass: *what is actually in
this repository's commit history, and what would it cost to change it?*

It is written for a downstream project's counsel — in particular the A2A
project and the Linux Foundation — evaluating whether this repository's
provenance is clear enough to accept a contribution from it.
[`PROVENANCE.md`](../PROVENANCE.md) is the *certification*; this is the
*evidence*. Read that first. Nothing here modifies it.

Every figure below is produced by `scripts/provenance_manifest.sh`, which
classifies commits using the same rules as
[`.github/workflows/dco.yml`](../.github/workflows/dco.yml) — the gate that
decides whether a pull request is accepted today.

---

## 0. The measurement caveat, first, because it invalidated two prior attempts

**Every count in this document requires a complete clone.** A shallow clone
silently truncates the *oldest* commits. In this repository those are precisely
the non-compliant ones — the sign-off policy begins near the tip — so a shallow
measurement leaves the "passes" count untouched while understating every
failure category. It does not look broken. It looks like good news.

This is not hypothetical. It has now produced wrong numbers twice:

| Where | Reported | Actual | Cause |
|---|---|---|---|
| `PROVENANCE.md` §2.1 (2026-08-10) | 282 non-merge commits, 43% passing | 641 non-merge, 19% passing | shallow clone hiding 430 commits |
| Session brief (2026-08-10) | 319 commits, 139 AI-authored, 126 passing | 749 commits, 478 AI-authored, 126 passing | shallow clone, different boundary |

The `126 passing` figure is identical in the true measurement and in one of the
wrong ones. That is the whole problem in a single number: the part that gets
truncated is the part that fails.

§2.1 attributed its discrepancy to "squash-merged PR branches still present
locally". That was the wrong diagnosis, and the arithmetic disproves it. §2.1
reported reaching 178 commits from `b416c1a` and 311 from `af7a1f8`. On a
complete clone those refs reach 608 and 741:

```
608 − 178 = 430
741 − 311 = 430
```

An identical shortfall measured from two different refs is one truncation
boundary, not two ref sets. Differing local branches would not produce equal
differences.

`scripts/provenance_manifest.sh` refuses to run on a shallow clone rather than
print a number from one. To reproduce anything here:

```sh
git fetch --unshallow --tags origin
scripts/provenance_manifest.sh
```

---

## 1. What the history contains

At `c008ab0`, **749 commits**, spanning **2026-03-15 to 2026-08-10**.

| | Commits |
|---|---:|
| Total reachable | 749 |
| Merge commits (`dco.yml` does not examine these) | 101 |
| **Non-merge commits — the population `dco.yml` grades** | **648** |

Git author field, all 749 commits:

| Author | Commits |
|---|---:|
| `Claude <noreply@anthropic.com>` | 478 |
| `Tom F. <tomf@tomtomtech.net>` | 127 |
| `Tom F <tomtom215@users.noreply.github.com>` | 115 |
| `github-actions[bot] <41898282+…>` | 29 |

The two `Tom F` identities are the same person: a GitHub no-reply address used
for web-UI edits and merges, and a real address used for local commits. 99 of
the 115 no-reply commits are merge commits created by GitHub's merge button.

## 2. Verdict under the project's own DCO gate

Applying `dco.yml`'s rules to all 648 non-merge commits:

| Outcome | Commits | Share |
|---|---:|---:|
| **Would pass** — human author, matching `Signed-off-by` | **126** | 19.4% |
| Fail — author `noreply@anthropic.com` | 477 | 73.6% |
| Fail — author `github-actions[bot]` | 29 | 4.5% |
| Fail — human author, no matching `Signed-off-by` | 16 | 2.5% |

Two facts that follow, and are not visible from the table alone:

**None of the 477 AI-authored commits carries any `Signed-off-by` trailer** —
not a mismatched one, not any. Verified directly rather than inferred from the
authorship rule, which short-circuits before the sign-off check.

**The pattern is closed, not ongoing.** The AI-authored commits run
2026-03-15 to **2026-07-24** and stop there. `b416c1a` (2026-07-24, tagged
`v0.7.0`) is where `PROVENANCE.md` §3.2 took effect. Of the **141 commits since**,
**zero** are AI-authored:

| Author, `b416c1a..c008ab0` | Commits |
|---|---:|
| `Tom F. <tomf@tomtomtech.net>` | 127 |
| `Tom F <tomtom215@users.noreply.github.com>` | 8 |
| `github-actions[bot]` | 6 |

The forward policy is doing what it claims. Whatever counsel decides about the
history, the practice that produced it has already stopped.

### 2.1 The 16 human-authored commits without a sign-off

Small enough to list in full, so no one has to go looking. All are GitHub
web-UI edits, which offer no sign-off affordance, plus the initial commit.

| Commit | Date | Subject |
|---|---|---|
| `c6b33cb` | 2026-03-15 | Initial commit |
| `d19d2da` | 2026-03-16 | Fix formatting in introduction.md table |
| `064c777` | 2026-03-16 | Fix formatting issue in README.md |
| `b0c8976` | 2026-03-16 | Fix formatting issue in README.md |
| `d53dbfa` | 2026-03-16 | Fix formatting in dogfooding.md |
| `634ebfe` | 2026-03-18 | Fix alignment of output header in dogfooding.md |
| `682c442` | 2026-03-19 | Fix formatting issues in README.md |
| `787e1a4` | 2026-03-19 | Fix formatting in README.md for agent team section |
| `f867a80` | 2026-03-19 | Fix formatting in README for client demos section |
| `5fd1ff1` | 2026-03-19 | Fix formatting in multi-lang team diagram |
| `5e29df8` | 2026-03-19 | Fix formatting in README for protocol components |
| `10db73a` | 2026-03-31 | Add official a2a.proto specification file to docs |
| `e131bad` | 2026-03-31 | Create v1.0.0-specification-complete.md |
| `7ac16a2` | 2026-07-24 | Update README.md |
| `f9ab2c1` | 2026-07-24 | Update README.md |
| `6a936ce` | 2026-07-24 | Update README.md |

Every one is documentation. None is source.

### 2.2 The bot commits are ongoing and will not stop

The 29 `github-actions[bot]` commits run 2026-04-01 to 2026-08-09 and will keep
accruing: the benchmarks workflow commits generated results and pushes to
`main` directly. `dco.yml` triggers on `pull_request` only, so these never pass
through it.

This is not a gap in the gate — a workflow committing its own generated output
makes no assertion about the authorship of contributed work — but it does mean
"every commit on `main` carries a sign-off" is false today and will stay false.
The narrower true statement is: every *contributed* commit does.

## 3. What a history rewrite would cost

If the receiving project requires literal per-commit `Signed-off-by` trailers
rather than the blanket certification in `PROVENANCE.md` §2, the rewrite is
mechanical. Its cost is not.

**There is no partial rewrite.** The earliest commit failing `dco.yml` is
`c6b33cb`, **the initial commit** (2026-03-15, "Initial commit", no sign-off).
Amending it changes its SHA, and therefore the SHA of all 748 descendants.

| | |
|---|---|
| Commits whose SHA changes | **749 — all of them** |
| Release tags that must be re-cut | **10** (`v0.2.0` … `v0.7.0`) |
| Published crates.io releases whose source link breaks | 10 versions × 4 crates |
| SLSA provenance attestations that stop resolving | all, for every published tag |

All ten tags are lightweight (they point directly at a commit object, not an
annotated tag object), and every one is an ancestor of `main`, so every one
moves:

| Tag | Commit | Date |
|---|---|---|
| `v0.2.0` | `28bface` | 2026-03-16 |
| `v0.3.0` | `c690e44` | 2026-03-20 |
| `v0.3.1` | `ba921b9` | 2026-03-21 |
| `v0.3.2` | `faa2976` | 2026-03-30 |
| `v0.3.3` | `f16ac7f` | 2026-03-31 |
| `v0.4.0` | `0e2a216` | 2026-03-31 |
| `v0.4.1` | `82bc4ae` | 2026-03-31 |
| `v0.5.0` | `fae120d` | 2026-04-02 |
| `v0.6.0` | `319c79f` | 2026-06-10 |
| `v0.7.0` | `b416c1a` | 2026-07-24 |

The trade being offered is therefore explicit: **verifiable supply-chain
metadata is destroyed to gain a formality that `PROVENANCE.md` §2 already
supplies in substance.** That is the maintainer's stated reason for not having
done it unprompted, and he has pre-agreed to do it on request.

## 4. What this document does not settle

Deliberately out of scope, so that nobody reads a clean manifest as a clean
bill of health:

* **Whether the blanket certification is sufficient.** That is a legal
  judgement for the receiving project's counsel. This document supplies the
  facts it would need; it does not argue the conclusion.
* **Whether Anthropic's Commercial Terms assign output rights as
  `PROVENANCE.md` §1 relies on.** A recipient who needs that verified should
  read the version of the terms in force rather than this repository's summary
  of them.
* **Bus factor.** One maintainer, and no independent escalation path for a
  conduct report concerning him — see `GOVERNANCE.md` and
  `CODE_OF_CONDUCT.md`. Unrelated to provenance, and not fixed by any document.

## 5. Reproducing every figure here

```sh
git fetch --unshallow --tags origin      # required; the script refuses otherwise
scripts/provenance_manifest.sh           # every aggregate in sections 1-3
scripts/provenance_manifest.sh --csv     # one row per non-merge commit
scripts/provenance_manifest.sh --self-test
```

`--self-test` asserts that the script's classification rules still match
`dco.yml`'s, so this document cannot quietly start describing a different gate
than the one CI runs. Both guards were verified by deliberate injection on
2026-08-11: run inside a `--depth 20` clone the script exits 2 rather than
reporting the 83.3% pass rate it would otherwise have printed, and with
`dco.yml`'s `NON_HUMAN` regex altered by one character `--self-test` exits 1
naming the drift.
