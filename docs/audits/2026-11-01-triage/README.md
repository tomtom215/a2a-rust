<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Triage of the 149 open audit rows, 2026-11-01

Every row of `../2026-10-29/README.md` that was `open` was re-checked against `main` at
`4a8243c66df5ae07a54a69168ab419ac609b06e9` (the merge of pull request #185). The first job in
`handoff.md` was this check, starting with the S0 rows H2, H3 and H4.

| Result | Rows |
|---|---|
| confirmed (still real on `main`) | 71 |
| needs a decision from the maintainer | 46 |
| fixed in part (what is left is in the note) | 21 |
| already fixed | 7 |
| not a defect | 3 |
| cannot check by reading (needs a live Grafana) | 1 |
| **Total** | **149** |

The three S0 rows are all `fixed (part)`: H2 is now S1, H3 medium, H4 high (`h-s0.json`).
The triage also found 57 things no report had (52 distinct; one of them found by the session, not an agent): section N of
the audit README. `PLAN.md` groups the 191 items that need work into 19 commit groups for the one pull request;
`DECISIONS.md` has the 46 decisions plus twelve forks that gate work, each with a position.

## How it was done

Ten agents each took one report's rows (`a-l`, `b`, `c`, `d`, `e`, `f`, `g-api`, `g-docs`,
`h-s0`, `h-designs`), told by `BRIEF.md` to treat the reports as claims, to open the code, to
cite `file:line`, and not to run `cargo` or change anything. Their output is one `<slice>.json`
(a verdict, evidence, the test and whether it would fail on revert, remaining work, effort, any
breaking change, the live probe that would settle it) and one `<slice>.md`.

## What was checked mechanically, and what was not

- **Coverage.** The set of IDs that were `open` (read with the table counter from the
  findings-triage skill) and the set of IDs in the ten JSON files are identical: 149 and 149, none
  missing, none extra.
- **Citations.** 1,249 distinct `path:line` citations across the JSON were resolved against
  `git ls-files`: 1,209 name a tracked file with the line inside it. Of the 40 that did not, most
  point into dependency sources (tonic, hyper-util, tokio, rustls, opentelemetry_sdk, the `http`
  crate) that are not in this repository, or use a shorthand path (`types/src/lib.rs`). Four in-repo
  ranges run 1 to 7 lines past the end of their file; one, `e.json`'s `pg_schema/mod.rs:217-229`,
  is 23 lines past a 206-line file (the `to_regclass` guard it means is at lines 137 and 201); two
  bare `Cargo.toml:` citations are ambiguous between crates. **Treat the line numbers as
  approximate and re-derive them from the symbol when fixing.** That a line is in range says
  nothing about whether it is the right line.
- **Read by me, not only by the agents.** I opened the code or ran it for: `G/D-01` (the
  `without_tls` refusal and its test), `G/D-05` (the CLI's exit-code doc and `a2a card` ignoring
  headers), `G/C-01` (the CHANGELOG's 805,661 bytes and 12,496 lines), `G/C-02` (`check_release_tree.py:185`,
  `STABILITY.md`), `C-07` (the fix and its test), `C-20` (the 64 KiB budget at the audited
  commit), `A13` (README against the manifest), `NEW-g-api-1` (tonic's `GrpcTimeout` takes the
  shorter of the channel timeout and the request's), `H3`'s reaper loop, `sign_agent_card` (still
  public, promised removed in four places), and `STABILITY.md` sections 3.2 and 4.
  **Run, not read:** `NEW-f-1` (the `--header` bypass; `live-results.md` LR-1, LR-1b) and
  `NEW-b-2` (loopback through `HTTP_PROXY`; LR-2), each with a control that behaves correctly.
- **Facts about releases, checked against primary sources.** crates.io's newest version of all four
  crates is 0.15.0 (the sparse index), the GitHub latest release is `v0.15.0` (2026-10-25) and the remote
  tags end at `v0.15.0`. `NEW-t-1` is mine.
- **Not opened by me:** the other five `already fixed` rows (`SEC-11`, `SEC-12`, `D-04`, `B-04`,
  `B-22`) rest on the agents' reading and their cited test names, and so do all `confirmed` rows
  not listed above. Their status says `(read)`.
- **No test was run by anyone.** "Would fail if the fix were reverted" is a reading of the
  assertions. The agents compiled nothing. The base build in this sandbox (dev profile, CI's flags)
  succeeded; the suites have not been run on `4a8243c` here yet.
- **Probes are described, not run**, for everything except the two above: each row's `probe` field
  says what a live run would need.

## Corrections to the agents' output

The JSON and summaries are kept as the agents wrote them; where I found them wrong, the correction is here and in the
audit README's note for the row.

- **"0.15.1 shipped" is wrong.** `g-api.json` (NEW-g-api-4), `g-docs.json` (G/C-02, NEW-g-docs-4) and
  `g-docs.md` say 0.15.1 was published. It was never tagged or released (sources above), so its behaviour changes
  have not reached users, and `STABILITY.md` section 1 ("0.15.0 ... the newest release") is right. What stands: the
  gate would have let a patch carrying behaviour changes through (G/C-02), and `sign_agent_card` is still public
  although four documents promise its removal in 0.16.0 (NEW-g-api-4).
- **Line numbers are approximate** (see Citations above).

## Files

`BRIEF.md` the instructions the agents had · `<slice>.json`, `<slice>.md` their results ·
`PLAN.md` the commit groups · `DECISIONS.md` the forks · `live-results.md` and `probes/` the two runs.
