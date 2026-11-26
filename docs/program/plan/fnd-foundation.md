<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# FND: the engineering and document system

Read [`../README.md`](../README.md) §6 first. These packages change no library behaviour. They remove the
couplings and the overlapping sources of truth that make every other package slower. Evidence: E07 §A4,
E06 §5 and §9.

Rule for the whole workstream: **a simplification removes a copy or a coupling, never a check.** Each package
that touches a gate re-proves it with `scripts/prove_gates_fail.sh` or `scripts/prove_workflow_gates_fail.py`.

---

## FND-01: A `CLAUDE.md` that states the session rules

- **Goal.** A fresh session learns the binding rules from one short file at the repository root instead of
  from `docs/handoff.md`.
- **Evidence.** E07 §A1.1 item 4 (the harness asks for trailers `CONTRIBUTING.md` forbids; the rule is
  recorded only inside the handoff); E07 §A3.
- **Gates.** DEC-04. **Depends.** none. **Release.** not user-visible.
- **Do.**
  1. Write `CLAUDE.md` at the root, at most 80 lines: the commit identity and trailer rule exactly as the
     maintainer decides it; the three commands to run before every commit (E07 §A2 "Step 1"); "add the
     `mutation-test` label when opening a pull request"; "do not create a pull request unless asked";
     pointers to `docs/program/README.md` §6, `CONTRIBUTING.md` and `docs/handoff.md`.
  2. State no count and no version in it (ROADMAP *Antipatterns* 2 and 3).
- **Touch.** `CLAUDE.md` (new); `docs/README.md` ("Start here").
- **Accept.** `wc -l CLAUDE.md` is at most 80; `python3 scripts/check_doc_versions.py` exits 0; the file
  names no number that a script could derive.
- **Out of scope.** Restating `CONTRIBUTING.md`.

## FND-02: Cite the changelog by heading, not by line

- **Goal.** Adding an entry under `[Unreleased]` no longer fails `check_spec_ledger.py`.
- **Evidence.** ROADMAP *Release engineering* item 2; E07 §A1.3 item 19 (`docs/compliance/spec-ledger.json`
  cites a `CHANGELOG.md` line that every new entry moves); E07 §A4.3 S2.
- **Gates.** none. **Depends.** none. **Release.** `[0.16.x]`, internal.
- **Read first.** `scripts/check_spec_ledger.py` (how a citation is parsed and how `--fix-lines` locates a
  target).
- **Do.**
  1. Reproduce: add a blank line under `## [Unreleased]`, run `python3 scripts/check_spec_ledger.py`, keep
     the failing output, revert.
  2. Let a citation of a Markdown file name a heading or a quoted fragment with no line; resolve it at
     check time; fail when the fragment is absent or occurs more than once.
  3. Convert the changelog citation. Regenerate `docs/compliance/spec-ledger.md` with `--write`.
- **Touch.** `scripts/check_spec_ledger.py`, `docs/compliance/spec-ledger.json`,
  `docs/compliance/spec-ledger.md`.
- **Accept.** With a line added under `[Unreleased]`, `python3 scripts/check_spec_ledger.py` exits 0. The
  script's self-test refuses a fragment that is missing and one that is ambiguous. The entry count the
  script prints is unchanged from before the change.
- **Out of scope.** The other 3,750 citations (FND-07).

## FND-03: Correct the claims that are stale at HEAD

- **Goal.** No session plans from a row that the code has already closed, or from two documents that
  disagree.
- **Evidence.** E06 §5 (C-01 to C-22), E03 §7 (D1 to D10), E06 §1 ("stale claims that would mislead a
  plan").
- **Gates.** none. **Depends.** none. **Release.** `[0.16.x]`, documentation.
- **Do.** For each row of the two tables, open both sides, confirm which is true at HEAD, and correct the
  loser. The rows that change a status rather than a sentence:
  1. C-01: `docs/readiness-bar.md` says F1 is measured in one section and not in another and in its
     `measured-limits` block. State what was measured (ROADMAP *Release engineering* item 12).
  2. C-04, C-05, C-06: mark `NEW-g-docs-2`, `NEW-h-s0-7` and `NEW-g-docs-4` fixed in
     `docs/audits/0.16.0-release/findings.json` with the code anchor, and drop them from `ROADMAP.md`.
  3. C-11, C-12, C-13 and E03 D2, D3: `docs/swarm-orchestration.md` (the "What is measured today" table and
     the G1 heading predate G1-A, G2 and G5; the SQLite journal exists) and
     `docs/swarm-scale-findings.md` Finding 8 (the test is now `the_single_writer_refusal_crosses_replicas`
     in `crates/a2a-protocol-server/tests/multi_replica.rs`).
  4. E03 D4, D5, D8, D9: ADR 0050 (a server does send load now), ADR 0029 (the `mdns-sd` pin), ADR 0020
     (G5 is built), ADR 0030 (its status line; both SQL journals exist). Amend with a dated line; do not
     rewrite a decision.
  5. C-15 and C-14: `docs/handoff.md` *Now* (the pull requests it waits on are merged) and its two lines on
     the benchmark pin.
  6. C-08, C-09, C-10, C-17, C-19, C-20, C-22: each a stale sentence; correct or date it.
  7. `CONTRIBUTING.md`: `deny.toml` no longer bans `openssl-sys` nor allows `Unicode-DFS-2016` (E07, summary
     "Stale rule text"); the mutation filter it prints is stale (FND-08 removes the copy).
- **Touch.** The files named above.
- **Accept.** `python3 scripts/check_spec_ledger.py`, `python3 scripts/check_rfc_ledger.py`,
  `python3 scripts/release_evidence.py --self-test` and `mdbook build book` exit 0. The pull-request
  description lists every row of E06 §5 and E03 §7 with one of: corrected (which side), already true, or
  left with the reason.
- **Out of scope.** The 41 unmarked rows of `docs/adopter-audit-2026-09-22.md` (FND-05). Restructuring any
  document (FND-05, FND-10).
- **Done (2026-11-26).** Every row of E06 §5 and E03 §7 has a disposition, listed in the pull request that
  landed it, and the four commands of *Accept* exit 0. Left by design, each with its owning package: C-03 (a
  decision, SPC-07), E03 D6 (COR-05), D7 (SWM-01 step 4), D10 (SWM-01 step 3). Found while doing this and
  corrected in the same pull request: `docs/readiness-bar.md` said the one-core gap "was 4 to 12%", which
  mixed a throughput gap (6% and 4%) with a CPU gap (12% and 4%) from table 3 of the comparison.

## FND-04: One build-free pre-commit command

- **Goal.** The gates that fail on a file other than the one edited run in one command, in seconds, with no
  `target/`.
- **Evidence.** E07 §A4.3 S4; E07 §A2 "Step 1"; `docs/handoff.md` *Lessons* (the same three gates forgotten
  repeatedly).
- **Gates.** none. **Depends.** FND-02. **Release.** internal.
- **Read first.** `scripts/preflight.sh` (it derives its gate list from `.github/workflows/ci.yml` by step
  name and refuses to run when a named gate changed there), `scripts/check_gate_inventory.sh`.
- **Do.**
  1. Add a tier to `scripts/preflight.sh` (`--quick`) that runs, by `ci.yml` step name: the formatting
     check, both ledger checkers, the compliance map, file lengths, panic paths, doc versions, feature
     tables, lockfiles. No step that compiles Rust.
  2. Make it the default tier of `.githooks/pre-push` (today the formatting gate alone).
  3. Add its injection to `scripts/prove_gates_fail.sh` if the script requires one for a new tier.
- **Touch.** `scripts/preflight.sh`, `.githooks/pre-push`, `CONTRIBUTING.md` (*Quality Gates*).
- **Accept.** `scripts/preflight.sh --quick` on a clean tree exits 0 without creating `target/`;
  `scripts/preflight.sh --list` shows the tier; `./scripts/check_gate_inventory.sh` exits 0; with a blank
  line inserted at the top of a file the RFC ledger cites, the quick tier exits non-zero and names the
  ledger.
- **Out of scope.** Anything that needs a build.

## FND-05: One register for what is open

- **Goal.** Status lives in one machine-readable file with a checker; every other list is frozen or
  generated from it.
- **Evidence.** E06 §9 (the debt and the ten-step proposal), §4 (40 items under two to six names; ten id
  series collide), §1 (206 rows; 67 checked against HEAD).
- **Gates.** none. **Depends.** FND-03. **Release.** `[0.16.x]`, documentation and a gate.
- **Read first.** E06 §9 "Proposal" in full; `scripts/check_spec_ledger.py` as the model for a ledger
  checker with a self-test; `docs/audits/0.16.0-release/README.md`.
- **Do.**
  1. Create `docs/backlog/register.json` from `docs/program/evidence/06-open-items.json`, with the fields
     E06 §9 step 1 lists. Evidence anchors are a heading, a test name or a symbol, never a line number.
  2. Before importing a row whose status is `partial`, re-check it at HEAD (E06 §2 found five of nine such
     rows already fixed). Give each of the 41 unmarked rows of `docs/adopter-audit-2026-09-22.md` a status.
  3. Write `scripts/check_backlog.py` with `--self-test` (E06 §9 step 2) and add it to `ci.yml`
     `static-checks`, with its injection in `scripts/prove_gates_fail.sh`.
  4. Add one line at the top of each superseded register and dated audit: frozen on which date, and that
     status now lives in the register under the same ids (E06 §9 steps 1 and 7).
  5. Generate ROADMAP's "After 0.16.0" block from the register between markers, as `docs/readiness-bar.md`
     does for `measured-limits` (E06 §9 step 3).
  6. Point `docs/program/README.md` §6 step 9 and step 13 at the register.
- **Touch.** `docs/backlog/register.json` (new), `scripts/check_backlog.py` (new),
  `.github/workflows/ci.yml`, `scripts/prove_gates_fail.sh`, `ROADMAP.md`, the frozen files,
  `docs/README.md`.
- **Accept.** `python3 scripts/check_backlog.py --self-test && python3 scripts/check_backlog.py` exits 0.
  The self-test fails a duplicate id, an unresolved alias, a missing evidence path, a `fixed` row with no
  test, and a ROADMAP block that differs from the register. `./scripts/check_gate_inventory.sh` exits 0.
  `scripts/prove_gates_fail.sh --only backlog` reports PROVEN. `jq length docs/backlog/register.json` and
  the count by status are printed in the pull request.
- **Out of scope.** Moving history out of `ROADMAP.md`, `docs/handoff.md` or `CHANGELOG.md` (FND-10).
  Assigning a normalized severity (the maintainer's; E06 §9 step 9).

## FND-06: Pin the toolchain a session and CI lint with

- **Goal.** "CI's stable is X" is a file, not a line read from a CI log.
- **Evidence.** ROADMAP *Release engineering* item 8; E07 §A4.3 S12; `docs/handoff.md` *Environment facts*
  (1.97.0 in the image against 1.100.0 in CI; their clippy disagreed).
- **Gates.** DEC-05. **Depends.** none. **Release.** internal.
- **Do.** Per E07 §A4.3 S12: a pin file under `.github/`, written when the pin is moved, read by `ci.yml`
  and by `scripts/preflight.sh`; `preflight.sh` refuses a different `rustc -V` unless an override variable
  is set. The MSRV leg (1.94) is untouched.
- **Touch.** `.github/` (the pin file), `.github/workflows/ci.yml`, `scripts/preflight.sh`,
  `CONTRIBUTING.md`, `docs/handoff.md` (*Environment facts*).
- **Accept.** `scripts/preflight.sh --quick` under a different toolchain exits non-zero naming both
  versions; with the override it runs. `scripts/prove_workflow_gates_fail.py` exits 0.
- **Out of scope.** A `rust-toolchain.toml` at the root (it would bind adopters building from a checkout).

## FND-07: Ledger citations by symbol

- **Goal.** An edit to a cited file does not fail a ledger gate unless it removes what was cited.
- **Evidence.** E07 §A4.3 S1 (3,156 citations in `docs/compliance/spec-ledger.json`, 595 in
  `docs/compliance/rfc-ledger.json`; "the single most-recorded recurring red").
- **Gates.** none. **Depends.** FND-02. **Release.** internal.
- **Do.** Per S1: store `path` plus a symbol, a test function or a quoted fragment; the checker resolves the
  line and renders it only into the generated Markdown; an ambiguous symbol needs an explicit
  disambiguator. Convert with a script that fails on any citation it cannot convert, and list those.
- **Touch.** `scripts/check_spec_ledger.py`, `scripts/check_rfc_ledger.py`, both ledgers' JSON and Markdown.
- **Accept.** The S1 acceptance check as written there. Both checkers print the same entry and status
  counts as before the change. A blank line inserted at the top of a cited source file leaves both green.
- **Out of scope.** Changing any row's status.

## FND-08: One source for the mutation test filter; retire `mutants.toml`

- **Goal.** The filter that says which tests a mutant is run against exists once.
- **Evidence.** E07 §A4.3 S3 and S9; E07 summary (five hand-synced copies, the one in `CONTRIBUTING.md`
  stale); `CONTRIBUTING.md` *Mutation Testing* ("configuration is not where it looks").
- **Gates.** none. **Depends.** none. **Release.** internal.
- **Read first.** `.github/workflows/mutants.yml`, `.config/nextest.toml`, `mutants.toml`,
  `scripts/check_mutants_config.py`, ADR 0006.
- **Do.** Per S3, then S9. Keep the measured history from `mutants.toml` in
  `book/src/reference/mutation-history.md`.
- **Touch.** `.github/workflows/mutants.yml`, `.config/nextest.toml`, `CONTRIBUTING.md`, `mutants.toml`,
  `scripts/check_mutants_config.py`, `.github/workflows/ci.yml`.
- **Accept.** The S3 and S9 checks as written. `scripts/prove_workflow_gates_fail.py` exits 0 with a probe
  that a harness binary named in the single filter is excluded. `./scripts/check_gate_inventory.sh` exits 0.
- **Out of scope.** Changing which tests the filter excludes; the zero-survivor rule; shard counts (S8 is
  worth doing and is left as a follow-up row in the register, since it changes CI cost and not correctness).

## FND-09: Release-engineering rows that can refuse or corrupt a tag

- **Goal.** The next tag is not refused or mis-measured by a known defect of the release tooling.
- **Evidence.** ROADMAP *Release engineering* items 1, 3, 4, 10 and 11; E06 §8.
- **Gates.** E06 §6 row 12 for step 1 only. **Depends.** none. **Release.** `[0.16.x]`, internal.
- **Do.** Each is its own commit with its own reproduction.
  1. Item 1: per the maintainer's choice, `scripts/ci_evidence.py` skips jobs `ci.yml` marks
     `continue-on-error`, or the nightly job stops being able to fail.
  2. Item 10: one check that `release.yml`'s `ci` job and `ci.yml` agree on the build environment.
  3. Item 11: `crates/a2a-protocol-server/tests/statement_count.rs` refuses a server that accepts the
     SSLRequest, with a message that says to turn TLS off, not to change the constants.
  4. Item 4 (`CI2`): make `a_send_carried_over_a_stream_crosses_the_same_proxy` independent of a pause of
     the machine, without widening its window or retrying it.
  5. Item 3: find why `scripts/provenance_manifest.sh` miscounted once; the suspect is `git log` piped into
     `grep -q` under `pipefail`.
- **Touch.** `scripts/ci_evidence.py`, `.github/workflows/release.yml`, `.github/workflows/ci.yml`, the two
  tests, `scripts/provenance_manifest.sh`.
- **Accept.** `scripts/ci_evidence.py --self-test` and `scripts/release_evidence.py --self-test` exit 0 with
  a new case each for what changed. `scripts/prove_workflow_gates_fail.py` exits 0. For step 3: against a
  PostgreSQL with `ssl = on` and no `PGSSLMODE`, the test fails with the new message. For step 4:
  `cargo nextest run --stress-count 200 -E 'test(~crosses_the_same_proxy)'` passes 200 of 200 while another
  process stops the test binary for one second at random (say how the pause was injected).
- **Out of scope.** The first publish of the SLIMRPC binding (ECO-01).

## FND-10: Split the documents by rate of change

- **Goal.** A session reads the current state on one screen; history stays, linked and with its blame.
- **Evidence.** E06 §9 steps 3 to 6 and 10; E07 §A4.3 S5 and S11.
- **Gates.** DEC-07 for the changelog; none for the rest. **Depends.** FND-05. **Release.** documentation.
- **Do.** In commits that only move text, so `git log --follow` survives:
  1. `docs/handoff.md` keeps *Now* and *First, next*. Decisions move to `docs/decisions.md` (keep the
     heading text *Decisions the maintainer made*: `.github/workflows/release.yml` and
     `packaging/README.md` cite it; leave a stub). Environment recipes move to `docs/environment.md`,
     lessons to `docs/lessons.md`, "Previous state" blocks to `docs/handoff-history.md`.
  2. `ROADMAP.md`'s closed history moves to `docs/history/`.
  3. One Known Limitations page generated from the register, linked from the changelog (ROADMAP *The
     book's structure* item 1).
  4. `docs/official-tck-findings.md` keeps its newest section and a table of contents.
  5. With DEC-07: `CHANGELOG.md` keeps `[Unreleased]`, the newest release and an index; older sections move
     verbatim to `docs/changelog/`. `scripts/check_release_tree.py` and
     `book-tests/tests/upgrading_guide.rs` read the per-version files.
- **Touch.** The files named; every script that reads them (`grep -rn 'handoff.md\|CHANGELOG.md' scripts
  .github book-tests` first, and list the hits in the pull request).
- **Accept.** `scripts/run_workflow_job.py .github/workflows/ci.yml static-checks` passes.
  `python3 scripts/check_release_tree.py self-test` exits 0. `wc -l docs/handoff.md` is at most 150. For each moved block, `git log --follow` on the new file
  reaches the commit that wrote it.
- **Out of scope.** Rewriting any moved text.

## FND-11: Gate the rules that are written and unenforced

- **Goal.** A rule a session is primed to break fails in CI, not at review.
- **Evidence.** E07 §A4.3 S6 and S7; E04 §6 item 9 (ADR 0053 names the missing deprecation gate); E07
  summary ("Unenforced: SPDX headers, the naming rule, deprecation note format, the ADR index").
- **Gates.** DEC-04 (the trailer rule must be decided before it is gated). **Depends.** none.
  **Release.** internal.
- **Do.**
  1. S6: `scripts/check_dco.sh` rejects an assistant-naming trailer; an SPDX first-lines check joins the
     file walk of `scripts/check_file_lengths.sh`.
  2. `scripts/check_deprecations.py`: every `#[deprecated]` outside `generated/` has `since` and "removed no
     earlier than X.Y.0".
  3. S7: generate the ADR table of `docs/README.md` (it stops at 0021) and `book/src/reference/adrs.md`
     from the ADR files, with a `--check` mode.
- **Touch.** `scripts/check_dco.sh`, `scripts/check_file_lengths.sh`, `scripts/check_deprecations.py` (new),
  `.github/workflows/ci.yml`, `scripts/prove_gates_fail.sh`, `scripts/prove_workflow_gates_fail.py`,
  `docs/README.md`.
- **Accept.** The S6 probes as written; `scripts/prove_gates_fail.sh` proves each new `ci.yml` gate;
  deleting one row of the generated ADR table fails the check.
- **Out of scope.** Gating the naming rule (no mechanical form of it is proposed).

## FND-12: Machine-readable touch-point manifests

- **Goal.** Adding a wire method or a `TaskStore` method names every site that must change, as a red gate.
- **Evidence.** E07 §B9 (36 sites for a new RPC; every binding dispatches on method-name string literals,
  not the `Method` enum), §B10 (27 sites for a store-backed entity; the three "overrides every method"
  tests), §A4.3 S10.
- **Gates.** none. **Depends.** none. **Release.** internal. **Do this before SPC-08.**
- **Do.** Per S10: a manifest of the files that must name each `Method`, checked by a script that greps
  each site for every wire name in `Method::ALL`; the same for `TaskStore` methods against all nine
  implementations and wrappers.
- **Touch.** `scripts/lib/` (the manifests), a new `scripts/check_touch_points.py`,
  `.github/workflows/ci.yml`, `scripts/prove_gates_fail.sh`.
- **Accept.** The S10 self-test as written: a fake method name in the script's fixture makes it list every
  site of E07 §B9. On the tree as it stands the check passes.
- **Out of scope.** Replacing the string literals with the enum (a refactor across six dispatchers; propose
  it separately with a measurement of what it removes).
