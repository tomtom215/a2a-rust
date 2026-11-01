# Triage brief (shared by every triage agent)

You are re-checking audit rows against the code that is on `main` TODAY. You are
one of ~10 agents, each with its own slice of rows. You do not fix anything.

## Baseline
- Repo: `/home/user/a2a-rust`. The working tree is `main` at
  `4a8243c66df5ae07a54a69168ab419ac609b06e9` (PR #185 merged). Confirm with
  `git rev-parse HEAD` first; if it differs, stop and say so.
- The task list is `docs/audits/2026-10-29/README.md` (the `Status` column).
  Each row's detail is in the report named in your slice
  (`docs/audits/2026-10-29/<X>-*.md`). **Those reports are a model's reading of
  the tree at `8dc4331`, not measurements. A claim in them is a claim to verify.**
- 34 commits landed AFTER the README's last edit (`git log --format='%h %s'
  369e580..HEAD`), several of them fixes for rows you will see as `open`
  (PR #185: proxy/CA variables for push, OIDC and model client; SSRF check;
  reaper; webhook bounds; delegation resume + journal (ADR 0030); token-endpoint
  and discovery binding; X-Forwarded-For trust; OTEL_SDK_DISABLED; abandoned
  streams; bearer-token tenant digest). `CHANGELOG.md` `[Unreleased]` lists
  them. Use `git log -S'<symbol>'`, `git log --oneline -- <path>`, `git blame`
  to date things.
- ADRs 0030-0034 in `docs/adr/` are proposed designs added for this session.
  Read the ones that touch your rows.

## Hard rules
1. **Read-only.** Do not edit, create or delete anything inside
   `/home/user/a2a-rust`. Do not run `git checkout/commit/stash/reset/add`.
   Do NOT run `cargo` (a shared build tree would block, and the disk is small).
   You may use `rg`/`grep`/`git log|show|blame|grep`, read files, and run small
   scripts (`python3 -I`) from the scratchpad that only read the tree.
2. Write your results ONLY under
   `<triage output directory>/`.
3. **Never assume, never skim, never trust the report, the CHANGELOG, a commit
   message or a code comment as proof.** Open the code. If the report says X is
   at `foo.rs:123`, the code has moved: find it by symbol.
4. "Already fixed" needs: the commit (hash) AND the code now doing the thing
   (`file:line`) AND the test that covers it, **with you having read the test's
   assertions and judged whether it would fail if the fix were reverted**
   (say so, and say that you did not run it). A test that only exercises the
   happy path, or asserts a constant, is not coverage: report the gap.
5. "Not a defect" needs: what the finding misread, with `file:line`.
6. If a row cites several sub-claims (a, b, c ...), judge each; a row is
   `partly_fixed` if some hold and some do not, and you say which.
7. Do not stop at the row's wording. Adversarially ask: *what else is wrong in
   this exact area, and what deployment (local, Docker, Kubernetes, behind a
   proxy or not, public CA, private CA, self-signed, mDNS, several replicas, a
   store outage, a disconnect mid-stream) breaks it?* Put what you find under
   `new_findings`, each with `file:line`, severity, the failing scenario, and
   whether you only READ it (say "read only"). Do not pad: only real ones.
8. Be candid about what you did not check. Do not write "verified" for
   something you only read.

## Verdicts (exactly one per row)
- `confirmed` - defect is still present on main (give `file:line` + mechanism).
- `already_fixed` - see rule 4.
- `partly_fixed` - say what is fixed (rule 4 evidence) and what is open.
- `not_a_defect` - see rule 5.
- `needs_decision` - real, but the fix is a design/policy choice. Give 2-3
  options with trade-offs and a recommendation. (Examples of this class: the
  H-report designs, wire-format changes, default changes that are breaking.)
- `cannot_check` - say what is missing (environment, hardware, credentials).
- `duplicate` - the canonical row ID.

For every row, also say whether a **live probe** is needed to settle or to
prove the fix (a behaviour only a running process shows: TLS handshake, proxy,
Postgres failover, real model, browser, Docker, Kubernetes ...). If yes: the
exact command or steps, the services needed, and what observation decides it.
Mark rows that can be settled by reading as `probe: none`.

## Output (two files, both required)
1. `<slice>.json` - an object:
   ```json
   {"slice": "...", "baseline": "4a8243c...", "rows": [
     {"id": "B-03", "verdict": "confirmed|already_fixed|partly_fixed|not_a_defect|needs_decision|cannot_check|duplicate",
      "severity_now": "S0|S1|high|medium|low|info",   // your judgement after checking; say if it changed from the report
      "evidence": ["path:line - what it shows", "commit abc1234 - ..."],
      "test": "path:line name, or 'none' - and whether it would fail without the fix (read, not run)",
      "summary": "<=2 sentences: what is true on main now",
      "remaining": "what is still open (empty if already_fixed / not_a_defect)",
      "theme": "short PR theme label, e.g. client-tls, store-retry, docs, otel-logs",
      "effort": "XS|S|M|L|XL",
      "breaking": "none|api|wire|behaviour - one line",
      "probe": "none | exact live probe description",
      "options": "only for needs_decision: option list + recommendation"}
   ],
   "new_findings": [
     {"id": "NEW-<slice>-1", "severity": "...", "where": "path:line", "finding": "...", "scenario": "...", "read_only": true, "theme": "..."}
   ]}
   ```
   Validate it with `python3 -I -c 'import json,sys; json.load(open(sys.argv[1]))' <file>`.
2. `<slice>.md` - a short human summary: counts by verdict, the 5 most
   important rows, anything you could not finish and why.

Your final chat message: counts by verdict, the paths of both files, and the
three things the maintainer most needs to know. No narration of your process.
