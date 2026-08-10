<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Provenance and Developer Certificate of Origin

This document records how the code in this repository was produced, who is
responsible for it, and under what certification it is offered. It exists so
that a downstream project — in particular the A2A project and the Linux
Foundation — can evaluate this repository's provenance from the repository
itself, without having to reconstruct it from `git log`.

It has three parts:

1. **AI-assisted development disclosure** — what was generated, by what, and
   what the human maintainer's role was.
2. **Retroactive DCO certification** — a one-time certification covering every
   commit made before this policy was adopted.
3. **Forward policy** — how commits are authored and signed from now on, and
   how CI enforces it.

---

## 1. AI-assisted development disclosure

This project was developed with heavy use of AI coding assistants — primarily
Anthropic's Claude, operating through Claude Code. This is stated up front
because it is visible in the commit metadata and because a reader who
discovers it unannounced is entitled to wonder what else was not disclosed.

**The measurable facts, as of commit `b416c1a`:**

| | |
|---|---|
| Total commits | 608 |
| Commits with `Claude <noreply@anthropic.com>` in the git *author* field | 478 (79%) |
| Commits authored by the human maintainer | 107 |
| Automated release/bot commits | 23 |
| Commit bodies carrying a `Co-Authored-By: Claude` trailer | 61 |

These numbers are reproducible with:

```sh
git log --format='%an' | sort | uniq -c | sort -rn
```

**Re-verified 2026-08-11 against a complete clone.** All five figures reproduce
exactly at `b416c1a`: 608 total, 478 / 107 / 23 by author, and 61 commit bodies
carrying a `Co-Authored-By: Claude` trailer. The §2 statement that none of those
608 commits carries a `Signed-off-by:` trailer also holds — checked
individually, the count is 0. A note in §2.1 previously said the 608 figure
could not be reproduced; that note was itself measured on a shallow clone and
has been corrected there. This section was right as written.

At `c008ab0` the totals have moved — 749 commits, of which 478 are still the
same assistant-authored set, because none has been added since. The current
figures, and what they would cost to change, are in
[`docs/provenance-manifest.md`](docs/provenance-manifest.md).

**What that does and does not mean.**

The `Claude <noreply@anthropic.com>` author line reflects the mechanics of the
tool that wrote the commit, not an assertion that a non-human entity is the
legal author of the work. In every case:

- The work was initiated, directed, scoped, and prompted by the human
  maintainer named below.
- The output was reviewed and accepted by that maintainer before it entered
  the repository. Nothing merged unreviewed.
- The maintainer retained and exercised the authority to reject, revise, or
  revert any of it, and did so.
- No contribution in this repository was received from any third party under
  terms other than the Apache-2.0 licence and, from the date in Section 3
  below, the DCO.

The maintainer's position — offered as a factual account of how the work was
made, not as a legal opinion — is that this is authorship assisted by a tool,
in the same category as a compiler, a refactoring engine, or a code generator,
and that the resulting work is his to license under Apache-2.0. Section 2 is
the certification that follows from that position.

**The relevant terms.** The AI assistance was obtained through Anthropic's
commercial Claude offerings. Anthropic's Commercial Terms of Service assign
Anthropic's rights in the outputs to the customer. The maintainer relied on
those terms; a downstream recipient who needs that verified should verify the
version of the terms in force rather than relying on this paragraph.

**Where this disclosure does not reach.** This section describes authorship,
not correctness. It is not a claim that AI-assisted code is equivalent in
quality to human-written code, and it should not be read as one. The
repository's quality argument rests on its tests, its mutation-testing gate,
its fuzzing, and its conformance suites — all of which are independently
runnable — not on how the source was typed.

### Third-party material incorporated

The following files originate outside this project and are not covered by the
maintainer's own authorship claim. Each is Apache-2.0 and each is identified
in place:

| Path | Origin | Licence |
|---|---|---|
| `proto/a2a_v1/a2a.proto`, `docs/implementation/a2a.proto`, and the per-crate copies under `crates/*/proto/` | The A2A v1.0 specification's protobuf schema (`a2aproject`). Kept byte-identical; a test enforces this. | Apache-2.0 |
| `proto/a2a_v1/google/api/*.proto` | Minimal vendored stubs from `github.com/googleapis/googleapis` | Apache-2.0 |
| `itk/protos/instruction.proto` | Vendored verbatim from `a2aproject/a2a-itk` | Apache-2.0 |
| The `validate_agent_card` ruleset reimplemented in `itk/interop/inspector_card_check.py` | Derived from `a2aproject/a2a-inspector`'s `backend/validators.py` | Apache-2.0 |
| `tck/fixtures/grpc/` golden bytes | Generated output of the official Python `a2a-sdk`, produced by running it — not copied source | n/a (generated data) |

No third-party source is incorporated under any licence outside the allowlist
enforced by `cargo deny` (`deny.toml`).

---

## 2. Retroactive Developer Certificate of Origin certification

The Developer Certificate of Origin 1.1 (reproduced verbatim in the [`DCO`](DCO)
file at the root of this repository) was adopted by this project **after** the
commits described above were made. None of those 608 commits carries a
per-commit `Signed-off-by:` trailer, because no such requirement was in force
when they were written.

Rather than rewrite the repository's history — which would invalidate all ten
release tags, break the SLSA provenance attestations bound to the published
`v0.2.0`–`v0.7.0` crates, and sever the link between the crates.io releases and
their source commits — the project adopts the DCO by way of the following
one-time blanket certification.

> ### Certification
>
> I, **Tom F. \<tomf@tomtomtech.net\>** (GitHub [@tomtom215](https://github.com/tomtom215)),
> the sole maintainer and sole copyright holder of the a2a-rust project, hereby
> certify that:
>
> 1. I have read the Developer Certificate of Origin, Version 1.1, as
>    reproduced in the [`DCO`](DCO) file in this repository.
>
> 2. I make the certification set out in that document — clauses (a) through
>    (d) — **with respect to every commit in this repository reachable from
>    commit `b416c1a43212775afa68fb5d4824043311ca7de5`**, inclusive, whether or
>    not that commit carries a `Signed-off-by:` trailer, and whatever identity
>    appears in its git author field.
>
> 3. This certification is made in the same terms, and with the same effect, as
>    if a `Signed-off-by: Tom F. <tomf@tomtomtech.net>` trailer appeared on each
>    of those commits individually.
>
> 4. Specifically, and for the avoidance of doubt: the commits whose git author
>    field reads `Claude <noreply@anthropic.com>` were made at my direction,
>    under my review, and on my behalf. I claim them as my contributions under
>    clause (a) of the DCO, and I certify that I have the right to submit them
>    under the Apache-2.0 licence indicated in the files.
>
> 5. I understand and agree that this project and these contributions are
>    public, and that a record of them — including the personal information in
>    this certification — is maintained indefinitely and may be redistributed
>    consistent with this project and the Apache-2.0 licence.
>
> Signed-off-by: Tom F. \<tomf@tomtomtech.net\>

The commit that introduces this document into the repository carries that
sign-off in its own commit message, so the certification is itself attested in
the git history at the point it takes effect.

**A note for counsel.** This is a blanket certification, which is the ordinary
mechanism for adopting the DCO mid-project and is accepted by a number of
Linux Foundation projects in that situation. If the receiving project requires
a literal per-commit `Signed-off-by:` trailer on historical commits instead,
that can be produced by rewriting the history with `git filter-repo`; the
consequences (new SHAs for all 608 commits, ten tags re-cut, published SLSA
attestations no longer resolving) are understood and the maintainer is willing
to do it on request. It was not done unprompted because it destroys verifiable
supply-chain metadata to gain a formality this document already supplies.

### 2.1 What the history actually contains — measured 2026-08-11

The blanket certification above says historical commits carry no sign-off. This
subsection says *how many*, because "some of the history predates the policy"
and "81% of it does" are different statements and only one of them is a fact.

Replicating `.github/workflows/dco.yml`'s own logic — its `NON_HUMAN` regex and
its author-matching `Signed-off-by` pattern — over all **648 non-merge commits**
reachable from `c008ab0`:

| Outcome under `dco.yml` | Commits |
|---|---:|
| Would **pass** | 126 (19%) |
| Fail — non-human author `noreply@anthropic.com` | 477 |
| Fail — non-human author `github-actions[bot]` | 29 |
| Fail — human author, no matching `Signed-off-by` | 16 |

Three things follow, and the third is the one that is not already covered above.

1. **The assistant-authored commits are closed out.** All 477 fall between
   2026-03-15 and 2026-07-24 — none postdates §3.2, which discontinued the
   pattern. Of the 141 commits made since `b416c1a`, **zero** are
   assistant-authored. The policy took effect and held.

2. **The sixteen unsigned human commits** are GitHub web-UI edits, which do not
   offer a sign-off, plus the initial commit. All sixteen are documentation;
   none is source. Listed individually in
   [`docs/provenance-manifest.md`](docs/provenance-manifest.md) §2.1. Covered by
   the blanket certification; noted so the count above reconciles.

3. **The bot commits are ongoing, not historical.** The 29
   `github-actions[bot]` commits run 2026-04-01 to **2026-08-09** and will keep
   accruing: the benchmarks workflow commits generated results to the book and
   pushes to `main` directly. `dco.yml` triggers on `pull_request` only, so
   these never pass through it. That is not a gap in the gate — a workflow
   committing its own generated output is not making an assertion about
   authorship of contributed work — but it does mean "every commit on `main`
   carries a sign-off" is false today and will stay false. Say the narrower
   true thing instead: every *contributed* commit does.

#### Correcting the 2026-08-10 version of this subsection

Every figure in the table above except one changed on 2026-08-11. The previous
version reported 282 non-merge commits, 120 passing (43%), 138 assistant, 21
bot, 3 unsigned human. **It was measured on a shallow clone**, and each of
those failure counts was understated by roughly a factor of three.

That subsection also carried a note saying §1's figure of 608 total commits at
`b416c1a` "could not be reproduced", attributing the gap to squash-merged PR
branches still present in the original clone. Both halves of that note were
wrong. On a complete clone `git rev-list --count b416c1a` returns **608**,
exactly as §1 states — §1 was correct throughout and needs no change.

The arithmetic identifies the real cause. The note reported reaching 178
commits from `b416c1a` and 311 from `af7a1f8`; a complete clone reaches 608 and
741:

```
608 − 178 = 430
741 − 311 = 430
```

An identical shortfall from two different refs is a single truncation boundary,
not two ref sets — differing local branches could not produce equal
differences.

The failure mode is worth stating plainly, because it is not obvious and it is
easy to repeat: **a shallow clone drops the oldest commits, which in this
repository are exactly the non-compliant ones.** The pass count is unaffected —
it was 120 in the shallow measurement and 120 over the same ref on a complete
one — while every failure category is understated. A shallow run does not look
broken. It looks like better news than the truth.

`scripts/provenance_manifest.sh` now refuses to run on a shallow clone rather
than print a number from one, and `--self-test` asserts its rules still match
`dco.yml`'s. Both guards were verified by deliberate injection rather than
assumed to work.

---

## 3. Forward policy — effective immediately

From the commit that adds this document onward:

### 3.1 Every commit carries a sign-off

Every commit must carry a `Signed-off-by:` trailer whose email matches the
commit's git author:

```sh
git commit -s -m "your message"
```

This is enforced in CI by [`.github/workflows/dco.yml`](.github/workflows/dco.yml),
which fails any pull request containing a non-merge commit without a matching
sign-off.

### 3.2 A human is always the git author

The pattern that produced the 478 commits described in Section 1 —
`Claude <noreply@anthropic.com>` in the git *author* field — is discontinued.
It obscured the responsible human behind a tool identity and made per-commit
sign-off impossible, since a sign-off is an assertion by a person.

AI-assisted commits are now authored by the human who directed and reviewed
the work, with the assistant credited in a trailer:

```
Some change that an assistant helped write

Signed-off-by: Tom F. <tomf@tomtomtech.net>
Co-Authored-By: Claude <noreply@anthropic.com>
```

Configure the local repository once:

```sh
git config user.name  "Tom F."
git config user.email "tomf@tomtomtech.net"
```

CI enforces this too: the DCO workflow rejects any commit whose author email is
a known non-human assistant identity, so the old pattern cannot silently
return.

### 3.3 Disclosure is maintained

Section 1 of this document is kept current. Material changes in how the project
uses AI assistance are recorded here, not left to be inferred from commit
metadata.

---

## 4. Contact

Questions about anything in this document — including requests from a
downstream project or its counsel for a different form of certification —
should go to the maintainer: Tom F. \<tomf@tomtomtech.net\>,
[@tomtom215](https://github.com/tomtom215).
