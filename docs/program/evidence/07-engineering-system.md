<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

> **Frozen audit input, 2026-11-25, of the tree at `2058058d`.** Not maintained. Written by a delegated
> read-only audit for [`docs/program/README.md`](../README.md); its VERIFIED and INFERRED labels are the auditor's own.
> Re-check an anchor before acting on it. `UPSTREAM/` is a local clone of the named upstream repository.

# 07 — Engineering system audit and architecture map (a2a-rust, `main` at `2058058d`)

Read-only audit. No repository file was modified; no `cargo build`/`cargo test` was run.
Every number below came from a command run in this session (named beside it) unless marked INFERRED.
`V` = VERIFIED (read in the file or printed by a command here). `I` = INFERRED (deduced; say so before relying on it).

Paths are relative to `/home/user/a2a-rust`. "server/src" abbreviates `crates/a2a-protocol-server/src`, likewise `client/src`, `types/src`.

---

## 0. Corrections to the handoff's own numbers (re-measured)

| Claim | Re-measured | Command |
|---|---|---|
| 1,036 files / 302,309 lines under `crates/*/src` | **1,036 / 302,309** (V, matches) | `find crates/*/src -name '*.rs' \| wc -l`; `… \| xargs cat \| wc -l` |
| 84 scripts | 84 **entries** in `scripts/` (72 files + 12 directories); **293 files** recursively (V) | `ls scripts \| wc -l`; `find scripts -type f \| wc -l` |
| 22 workflows | 22 `.yml` (+ a `README.md`) (V) | `ls .github/workflows` |
| 58 ADRs (`docs/handoff.md:47`) | **57** ADR files, `0001`–`0057`; the 58th entry of `ls docs/adr` is the directory `0028-wasm-spike` (V) | `ls docs/adr/*.md \| wc -l` |
| "19 workflows / 60 jobs" (`handoff.md:288`), "other 18 workflows" (`CONTRIBUTING.md:635`) | stale: 22 workflows today (V) | as above |
| "twelve server extension traits" (`STABILITY.md:122`), "twenty public traits" (`CONTRIBUTING.md:244`) | 24 `pub trait` in `server/src` non-test files by grep (V count of grep hits; reachability not checked) | `grep -rnE '^\s*pub trait' crates/*/src` |
| CHANGELOG "1 MB" | 14,411 lines, 984 K (V) | `wc -l`, `du -sh CHANGELOG.md` |
| `docs/README.md` ADR table | stops at ADR 0021 (V); the complete index (0001–0057) is `book/src/reference/adrs.md` (V) | `grep -o 'adr/00[0-9][0-9]' docs/README.md \| sort -u \| tail -1` |
| Spec-ledger CHANGELOG citation "13299" (`handoff.md:101`) | now `CHANGELOG.md:13311 "## [0.5.0] - 2026-04-02"` (`docs/compliance/spec-ledger.json:76`) (V) | `grep -n CHANGELOG.md docs/compliance/spec-ledger.json` |

---

# PART A — RULES AND GATES

## A1. The complete rule list

Format: **rule** — source — gate (or "unenforced").

### A1.1 Commits, authorship, DCO

1. **Every non-merge commit carries `Signed-off-by:` whose e-mail equals the git *author* e-mail.** — `CONTRIBUTING.md:12-35`; check at `scripts/check_dco.sh:104-110`. — Gate: `dco.yml` job `dco` ("Sign-off and authorship"), on PRs to `main` and pushes to `main` (`dco.yml:18-22,54`). V.
2. **The git author must not be an assistant identity.** Regex `NON_HUMAN='^(noreply@anthropic\.com|.*\[bot\]@users\.noreply\.github\.com)$'` (`scripts/check_dco.sh:51`); exempt by exact identity: Dependabot and the benchmarks bot (bot only when it touches the generated benchmark pages) (`check_dco.sh:55-56,69-90`). — Gate: same job. V.
3. **"The commit carries your sign-off and no other trailer"** — exact rule text `CONTRIBUTING.md:55-62`: *"You are the author. Commit as yourself, not as the assistant. … The commit carries your sign-off and no other trailer: `Signed-off-by: Your Name <your.email@example.com>`"*. The `Co-Authored-By` requirement was removed 2026-10-15 (`CONTRIBUTING.md:65-68`). — Gate: **unenforced for the "no other trailer" half** (V: `check_dco.sh` greps only for the sign-off and the author e-mail; nothing rejects a `Co-Authored-By` or `Claude-Session` trailer). The last 8 commits on `main` carry none (V: `git log -8 --format=…%(trailers:key=Co-Authored-By…)`).
4. **The exact command** (handoff, twice): `git -c user.name="Tom F." -c user.email=tomf@tomtomtech.net commit -s` (`docs/handoff.md:346`, `:1156`), then `bash scripts/check_dco.sh <base> HEAD` before pushing (`handoff.md:1156`). Trap: `git commit -s` signs as the **committer**; with `GIT_COMMITTER_NAME=Claude` it wrote `Signed-off-by: Claude`, which the gate rejects — use `--trailer "Signed-off-by: Tom F. <tomf@tomtomtech.net>"` if the committer identity cannot be set (`handoff.md:121-123`). The container's default identity is `Claude <noreply@anthropic.com>` (`handoff.md:1155`).
   **Conflict to resolve before any commit:** the session harness asks for `Co-Authored-By`/`Claude-Session` trailers; the handoff records "the maintainer's rule wins" (`handoff.md:347-348`). A plan must state this explicitly to each executing session. Whether the maintainer's identity may be used by a new session is the maintainer's call, not something this audit can grant.
5. **Say in the PR description that an assistant helped** — `CONTRIBUTING.md:64`. — unenforced.
6. **Commit subject convention** — no written rule found; observed prefixes in the last 40 subjects: `docs` 12, `test` 10, `deps` 3, `chore` 3, `ci` 2, `tck` 1, `mutants` 1 (V: `git log --format=%s -40`). — unenforced. I: follow `type: summary`.
7. **A fix's commit message carries its evidence**: the failing test that reproduced the defect, and for a mutant survivor the mutant and the assertion that fails (`handoff.md:114-115`, `:149`). — unenforced (practice).
8. **Never commit while `prove_gates_fail.sh`, a hand mutant or an `--in-place` mutation run is active in the same tree** (`CONTRIBUTING.md:892-895`; `handoff.md:1376-1378`). — partly enforced: the prover refuses a dirty tree at start and asserts clean at exit.

### A1.2 Branch / PR / labels / merge

9. **PRs target `main`**; all PR-triggered workflows filter `branches: [main]` (V: workflow dump). A plain push to a non-`main` branch starts no workflow (`handoff.md:1009`, `:1505`).
10. **`mutation-test` label.** `Mutation Testing (incremental)` runs on every PR event (`types: opened, synchronize, reopened, labeled, unlabeled`, `mutants.yml` `on.pull_request`); shards run only when the PR has the label (`mutants.yml:1212`); **without the label the aggregator exits 1** ("the PR has no 'mutation-test' label", `mutants.yml:1765-1771`). A PR opened before it is labelled gets a red run that a re-run cannot fix (the event is replayed without the label); the `labeled` event starts the grading run. **Add the label immediately after creating the PR and do not re-run the first run** (`handoff.md:275-280`). — Gate: `mutants.yml` job `mutants-incremental`. V.
11. **A push cancels the in-flight CI run on that ref** (`concurrency.cancel-in-progress` for every ref but `main`, V in every workflow header). Wait for all jobs, fix everything, push once (`handoff.md:1197-1198`, `:1504-1507`).
12. **Merge with a merge commit, never squash** when the PR contains release preparation (a squash leaves the manifest's pinned commit outside `main`) — `RELEASING.md:178-179`. — Gate: `release.yml` → `scripts/check_provenance_manifest.py` at tag time only.
13. **Branch protection is not in the tree**; `main` is unprotected (`handoff.md:185`). The *intended* required checks are listed in `.github/workflows/README.md:36-70`: DCO; Official TCK; CI `Static checks…`, `Clippy (stable, <3 OS>)`, `Feature matrix`, `Test (<stable|1.94>, <3 OS>)`, `Documentation`, `cargo-deny`, `Package validation`; `TCK self-test`, `TCK cross-language (8)`; `Mutation Testing (incremental)`; `Regression Gate`; OSV `Rust lockfiles` (+ new findings); CodeQL ×3; `Test coverage`. — unenforced by the repository itself (settings).
14. **PR template checklist** — `.github/PULL_REQUEST_TEMPLATE.md:12-26` (sign-off, SPDX, 500 lines, fmt, clippy, test, doc, docs on new public items, tests, zero mutants with `--test-tool=nextest --all-features`, CHANGELOG if user-visible, ADR if architectural). `CONTRIBUTING.md:901-926` adds: `prove_gates_fail.sh` if a `ci.yml` gate changed; `prove_workflow_gates_fail.py` probe/exemption if a verdict step in another workflow changed; `book/src/reference/pitfalls.md` if a non-obvious pitfall was met.
15. **Governance**: lazy consensus, 7 days; one maintainer (`GOVERNANCE.md:52-60,34-38`). — unenforced.

### A1.3 CHANGELOG

16. **Format**: Keep a Changelog; headings `## [Unreleased]` then `## [X.Y.Z] - YYYY-MM-DD` (`CHANGELOG.md:8-11,36`; `RELEASING.md:135-137`). Section names used in 0.16.0, in order: `### Upgrading`, `### Highlights`, `### Added`, `### Breaking Changes`, `### Behaviour Changes`, `### Fixed`, `### Deprecated`, `### Known Limitations`, `### Internal` (V: `CHANGELOG.md:52-3284`); `### Security` and `### Changed` are in `[Unreleased]` now (`CHANGELOG.md:13,25`). New entries go under `## [Unreleased]` (line 11), bold lead sentence then prose.
17. **Every breaking change is under `### Breaking Changes` with its migration in the same entry** — `STABILITY.md:109-111`. — Gate: release-time `scripts/check_release_tree.py cadence` (refuses a breaking section in a patch, or two breaking releases in one calendar month without `**Cadence exception:** <reason>`) (`RELEASING.md:308`); `book-tests/tests/upgrading_guide.rs` fails when CHANGELOG has a `### Breaking` heading for a release ≥0.7 (and `[Unreleased]` counts as the crates' version) and `book/src/reference/upgrading.md` lacks the `## X.Y -> X.Z` section (V: `spec-ledger.json:81` notes). So **adding `### Breaking Changes` under `[Unreleased]` obliges an upgrading-guide section in the same PR** (I from that note; verify by reading the test before relying on it).
18. **Release notes need `### Highlights`, non-empty, ≤15 non-blank lines** (from 0.16.0) — `RELEASING.md:309`. — Gate: `check_release_tree.py highlights` (release).
19. **CHANGELOG ↔ spec-ledger line coupling.** `docs/compliance/spec-ledger.json:76` cites `CHANGELOG.md:13311 "## [0.5.0] - 2026-04-02"`. **Any line added above the 0.5.0 heading — i.e. every `[Unreleased]` entry — moves that line and fails `check_spec_ledger.py`** until the citation moves (`handoff.md:100-101`, `:1135-1136`). Fix: `python3 scripts/check_spec_ledger.py --fix-lines`, then `--write` if the Markdown is stale (options at `scripts/check_spec_ledger.py:240-242`, description `:36-37,49-52`). — Gate: `ci.yml` static-checks step "Spec ledger covers every requirement…" (`ci.yml:236`). V.
20. **CHANGELOG is excluded from `check_doc_versions.py`** (historical versions allowed) — `scripts/check_doc_versions.py:36-39`. V.
21. **`book/src/reference/changelog.md` and `ROADMAP.md` "Current release:"** must be updated at release — `RELEASING.md:131-133` — **explicitly unenforced** ("neither is checked by anything, and both have rotted before").

### A1.4 Source files

22. **SPDX header on every file** (`// SPDX-License-Identifier: Apache-2.0` + copyright line) — `CONTRIBUTING.md:82-89`. — Gate: **none found** (I: no script whose purpose is SPDX; 0 of 1,036 `crates/*/src` files lack it — `grep -rL SPDX-License-Identifier --include=*.rs crates/*/src | wc -l` = 0). 531 of those files additionally carry an "AI Ethics Notice" comment line (V: grep) — a convention, unenforced; copy the header of a neighbouring file.
23. **500-line ratchet for tracked `.rs`, `.sh`, `.py`** — `CONTRIBUTING.md:91-118`. — Gate: `scripts/check_file_lengths.sh` (static-checks, `ci.yml:106`). Baseline `.file-length-baseline` (174 lines; output today: "82 of 2147 tracked source files exceed 500 lines, all recorded" — V, ran it). Rules: a non-baselined file >500 fails; a baselined file that drops to ≤500 fails until its entry is removed; `--update` rewrites the baseline and must be justified in the PR. **It counts tracked files only: run it after `git add`** (`handoff.md:1079-1081`). Test modules count (a 516-line test file failed, `handoff.md:1210`).
24. **Thin `mod.rs`** — `CONTRIBUTING.md:123-128`. — unenforced.
25. **Crate-root lints**: `#![deny(missing_docs)]`, `#![forbid(unsafe_code)]`, `#![warn(clippy::all, clippy::pedantic, clippy::nursery)]`, `#![allow(clippy::module_name_repetitions)]` — `CONTRIBUTING.md:130-143`; present at `server/src/lib.rs:96-103` (V). — Gate: compiler + `clippy … -D warnings` in 14 feature configurations × 3 OS (`ci.yml:605-637`) + `cargo hack clippy --each-feature` (`ci.yml:665`) + `RUSTFLAGS=-D warnings` globally (`ci.yml` `env`).
26. **`clippy.toml` thresholds**: `too-many-lines-threshold = 60`, `too-many-arguments-threshold = 7`, `cognitive-complexity-threshold = 25` (`clippy.toml:10-16`). They apply to **tests too** (`handoff.md:1137`). Cure is extraction, not `#[allow]` (`handoff.md:1138,1240-1241`).
27. **No `unwrap()`/unjustified `expect()`/`panic!`/`todo!` in library runtime code** — `CONTRIBUTING.md:145-188`. — Gate: `scripts/check_panic_paths.py` (static-checks, `ci.yml:492`), a ratchet failing on change **in either direction**; baseline `scripts/panic_paths_baseline.txt`; today "0 unwrap, 13 expect, 0 panic!, 0 todo!; build scripts 3 unwrap, 3 expect" (V, ran it). After a reviewed change: `scripts/check_panic_paths.py --update`.
28. **`unsafe`**: forbidden in published crates; allowed only in bench allocators, test env changes and the `build.rs` `set_var("PROTOC")`, each with a `// SAFETY:` comment; **do not add `set_var` anywhere else** — `CONTRIBUTING.md:190-204`. — Gate: `forbid(unsafe_code)` (compile error) for the crates; rest unenforced.
29. **No process-global panic hooks** — Gate `scripts/check_panic_hooks.sh` (`ci.yml:117`). V.
30. **Doc comments contain no literal `\n` escapes** — Gate `scripts/check_doc_escapes.py` (`ci.yml:450`). V.
31. **A claimed guard flag is released by `Drop`** — Gate `scripts/check_cancellation_release.py` (`ci.yml:441`). V.
32. **A timeout fits inside the bound enclosing it** — Gate `scripts/check_timeout_nesting.py` + `scripts/timeout_nesting_allowlist.txt` (`ci.yml:465`). V.
33. **A configurable bound is honoured by every sibling implementation, or allowlisted with a reason** — Gate `scripts/check_inert_bounds.py` + `scripts/inert_bounds_allowlist.txt` (129 lines) (`ci.yml:509`). Relevant to any new `with_*` limit on a trait with several impls. V.
34. **Every statement on a PostgreSQL task-store pool runs inside `pg_retry` or is checked** — Gate `scripts/check_pg_retry.py` (`ci.yml:475`). V.
35. **A `TaskStore` that writes a delta in place says so to wrappers** — Gate `scripts/check_store_delta_forwarding.py` (`ci.yml:276`). V.
36. **Client and server copies of the outbound-HTTP code agree** — Gate `scripts/check_net_copies.py` (`ci.yml:319`) — i.e. deliberate duplication between `client/src` and `server/src/net` held equal by a gate; edit both. V.
37. **Examples serve with `Server`, not a hand-written accept loop** — Gate `scripts/check_example_servers.py` + `scripts/example_accept_loops.txt` (`ci.yml:347`). V.
38. **Every binary starts `Telemetry` and no manifest drops `otel`** — Gate `scripts/check_otel_wiring.py` (`ci.yml:270`). A new binary/example needs this. V.
39. **No published crate carries or runs an AI model** — Gate `scripts/check_no_model_artifacts.py` (`ci.yml:228`). V.

### A1.5 Dependencies

40. `openssl-sys` and `reqwest` banned (`CONTRIBUTING.md:356`); in `deny.toml` today only `reqwest` is in `[bans].deny` with wrappers `rig-core`, `genai` (V: `deny.toml` dump) — `openssl-sys` is not listed there now (V; the CONTRIBUTING sentence and ADR 0002's snippet are stale on this point).
41. Licence allowlist: `Apache-2.0, MIT, BSD-2-Clause, BSD-3-Clause, ISC, Unicode-3.0, Zlib, CC0-1.0, CDLA-Permissive-2.0`; `unused-allowed-license = "deny"` (V: `deny.toml [licenses]`; `Unicode-DFS-2016` in `CONTRIBUTING.md:357` is not in the file).
42. `multiple-versions = "deny"` with an explicit `skip` list (18 entries, each with a reason); `yanked = "deny"`; `unknown-registry`/`unknown-git = "deny"` (V). A new dependency that duplicates a crate version fails unless a `skip` with a reason is added.
43. Advisory waivers live in `deny.toml [advisories].ignore` **and** must be mirrored in the `osv-scanner.toml` beside each lockfile (`handoff.md:125-126`; `SECURITY.md` Scope).
44. Bounded version ranges `>= x.y, < z` (`CONTRIBUTING.md:367`; ADR 0002 "Version Bounding"). — unenforced except by `scripts/check_advisory_floors.py` (a manifest must not admit a version with a RustSec advisory, `ci.yml:520`).
45. Philosophy (ADR 0002 + amendment 2026-11-01, `docs/adr/0002-dependency-philosophy.md` "Amendment"; `CONTRIBUTING.md:358-366`): small plain job → in-tree pure Rust; security-sensitive/intricate → the audited crate; prefer a crate already in `Cargo.lock`; say in the PR what the change adds to the graph. No `thiserror`, `anyhow`, `futures` (ADR 0002 "Explicitly Excluded"). — review only.
46. **Every tracked `Cargo.lock` must be current**: root, `bindings/a2a-protocol-slimrpc`, `itk`, `fuzz` (+ others the script finds) — Gate `scripts/check_lockfiles.sh` (`ci.yml:527`). **Changing a crate's default features or a dependency changes what `itk/` and `fuzz/` resolve** (`handoff.md:1379-1382`). Refresh: `for d in . bindings/a2a-protocol-slimrpc itk fuzz; do (cd "$d" && cargo metadata --format-version 1 >/dev/null); done` (`RELEASING.md:93-96`).
47. SQL driver isolation: `sqlite` must not link the postgres driver and vice versa — Gate `scripts/check_sql_driver_isolation.sh` (`ci.yml:352`). V.
48. Gates on PR: `cargo-deny` (`ci.yml` job `deny`), the binding's own `cargo-deny` step, `osv.yml` (`rust`, `rust-pr`). V.

### A1.6 Documentation

49. **Book builds and every link/fragment resolves** — Gate: static-checks "Book builds and its links resolve": `mdbook build book; cp -r book/static/. book/book/; python3 scripts/check_book_links.py book/book` (`ci.yml:186`). mdBook pinned 0.4.40 via `scripts/ci/install_mdbook.sh` (`handoff.md:329`).
50. **Every book Rust block compiles**: pages are `include_str!`ed into `book-tests/src/lib.rs` (169 `include_str` — V) and run by `cargo test --doc --workspace` (three feature sets, `ci.yml:742-746`). `ignore`d blocks are a **shrink-only ratchet**: `.book-ignore-baseline` (27 lines) + `scripts/check_book_code.sh` (`ci.yml:164`). Non-Rust blocks must carry a language tag (`book-tests/src/lib.rs` header). **A new book page must be added to `book/src/SUMMARY.md`, to `book-tests/src/lib.rs` (I — verify by reading the file's list), and the sitemap**: Gate `python3 scripts/gen_sitemap.py --check` (`ci.yml:172`).
51. **Every example crate has a book page or a recorded reason** — Gate `scripts/check_book_examples.py` + `scripts/book_examples_allowlist.txt` (`ci.yml:304`).
52. **`check_doc_versions.py`** (`ci.yml:296`): (a) every dependency snippet in tracked Markdown (outside `docs/`, CHANGELOG, the book's changelog page) and in `//!`/`///` under `crates/*/src` names the workspace **MAJOR.MINOR** (`"0.16"`), never another line and never an exact patch (`check_doc_versions.py:24-44`); allowlist `scripts/doc_versions_allowlist.txt`, an entry matching nothing fails; (b) `examples/deploy-agent/deployment.yaml` names the image at the exact workspace version, exactly once (`:46-52`); (c) **README wording**: exactly one TCK badge and exactly one sentence matching `\*\*(\d+) of (\d+) MUST requirements pass and (\d+) fail\*\*` (`:161`), and their three numbers must be equal (`:165-177`).
53. **Feature tables agree with manifests** — Gate `scripts/check_feature_tables.py` (`ci.yml:311`): the root README, `crates/README.md`, each crate's README, `book/src/getting-started/installation.md`, `book/src/reference/configuration.md` (`check_feature_tables.py:68-69,169`); it also flags a book Rust block using an item gated on a feature the page does not mention (`:263`).
54. **API Quick Reference names exist** — Gate `scripts/check_api_reference.py` over `book/src/reference/api-reference.md` (`ci.yml:200`).
55. **Crate READMEs are compiled as doctests** — Gate `scripts/check_readme_doctests.py` (`ci.yml:357`).
56. **Benchmark prose equals benchmark tables** — Gate `scripts/check_benchmark_prose.sh` (`ci.yml:150`).
57. **rustdoc: zero warnings, workspace and per crate in each crate's default features** — Gate job `doc` (`ci.yml:966,982`, `RUSTDOCFLAGS=-D warnings`). The per-crate default-feature build finds broken links to feature-gated items that the all-features build hides (`handoff.md:173-174`); the `otel-prometheus` docs are only built by the nightly job / `docs.yml` (`handoff.md:1478-1481`).
58. New public items have doc comments (`deny(missing_docs)`). New defaulted trait method needs a doc paragraph stating what not overriding costs (`CONTRIBUTING.md:268-271`).
59. "Count before writing a number into a document, and say what was counted" (`handoff.md:1329`); "Do not type the number here — run the script" (`CONTRIBUTING.md:163`). — culture; several gates exist because prose counts rotted.

### A1.7 Tests

60. **Every behaviour fix is first reproduced by a test that fails on the code as it stood** (`handoff.md:149`); "prove each new test fails without its fix, and drop one that cannot" (`handoff.md:1395-1397`). — unenforced directly; the mutation gate enforces the weaker form.
61. **Zero surviving mutants *and zero timeouts* in the lines a PR changes** — `CONTRIBUTING.md:491-500`; ADR 0006 "Decision" 2; timeouts fail since N15 (`mutants.yml` aggregator "Gate 3"). — Gate: `Mutation Testing (incremental)` (46 shards: server 40, types 2, client 2, sdk 1, discovery 1 — V from the matrix; `--in-diff pr-src.diff -p <crate> --timeout 300 --build-timeout 1800 --jobs 4 --shard k/n --sharding round-robin --test-tool=nextest --profile=mutants --all-features -- --run-ignored all -E '<filter>'`, `mutants.yml:1499-1511`; diff built by `git diff -M "${BASE}...HEAD" -- ':(glob)crates/<crate>/src/**/*.rs'`, `:1420`). Only `crates/<crate>/src/**/*.rs` is mutated; tests, examples, bindings, tools are not.
62. **No weakening**: there is deliberately no mutation baseline/allowlist (`CONTRIBUTING.md:511-512`; ADR 0006). Equivalent mutant → prove it, then `#[mutants::skip]` with the argument in a comment above it (ADR 0006 `docs/adr/0006-mutation-testing.md:82-158`); **never `mutants.toml`** (which is inert: `mutants.toml` banner; Gate `scripts/check_mutants_config.py`, `ci.yml:564`). The `mutants` crate is a dependency of `a2a-protocol-server` only (ADR 0006 `:128-130`) — I: using the attribute in another crate needs that dependency and a maintainer decision. First question for a survivor is "which behaviour has no test", and dead logic is deleted, not excused (`handoff.md:1139,1157-1158`).
63. **Mutation-kill evidence in the commit message**: name the mutant and the assertion that fails (`handoff.md:114-115`). A hand mutant counts only if the unmutated baseline passed in the same command (`handoff.md:1261-1263`).
64. **No fixed sleeps as synchronisation**; handshakes, then deadline polls (~10 s deadline, ~2 ms step) (`CONTRIBUTING.md:373-393`). No retries in nextest's `ci` profile by design (`.config/nextest.toml:101-104`). — unenforced.
65. **Per-test kill**: nextest default profile 20 s × 3 = 60 s (used by mutation runs); `ci` profile 30 s × 4 = 120 s, `fail-fast = false` (`.config/nextest.toml:90-109`). A test that legitimately runs >60 s breaks the mutation baseline.
66. **A slow, assertion-free or environment-dependent `#[ignore]`d test must be added to the `-E 'not (…)'` filter** — **four copies in `mutants.yml`** (`:685, :1177, :1511, :1674`) plus `CONTRIBUTING.md:461` — because the mutation jobs run `--run-ignored all` (`handoff.md:1147,1153-1154,1321-1323`). Current filter: `binary(soak) | soak_multi_replica | swarm_scale | readiness_bar | tool_loop_strict_live | terminal_truth_live | tool_loop_journal_live | tool_loop_review_live | tool_loop_audit_live | test(=audit::privacy::tests::cost::what_protection_costs_an_append)`.
67. **Every PostgreSQL-gated suite is run by some CI step** — Gate `python3 scripts/run_postgres_suites.py --check` (`ci.yml:482`); the script scans file text and wants a doc comment naming the URL variable in a PostgreSQL suite (`handoff.md:1195-1196`). PostgreSQL tests are `#[ignore]`d and read `A2A_TEST_POSTGRES_URL`.
68. **A test sharing a database must not assume it is alone** (own schema via `CREATE SCHEMA` + `options=-c search_path=…`) (`handoff.md:1152,1178-1180`). Platform traps: no `127.0.0.2` (macOS), `localhost` may be `::1`, no Unix-path assumptions (Windows), claim TTLs far above a round trip (`handoff.md:1266-1272,1489-1497,1195-1197`).
69. **Capture log lines with `crate::handler::warnings_during`/`warning_lines_during`, never `tracing::subscriber::with_default`, in a shared test binary** (`handoff.md:1305-1314`).
70. Test naming `{component}_{scenario}_{expected_outcome}` (`CONTRIBUTING.md:536-543`) — unenforced; recent tests use sentence-style names (`an_executor_holding_what_a_fold_needs_is_still_polled`).
71. **Every fuzz target is both built and run** — Gate `scripts/check_fuzz_matrix.py` (`ci.yml:209`): a new file in `fuzz/fuzz_targets/` must be added to `fuzz/Cargo.toml` and the `fuzz.yml` matrix (18 targets — V).
72. **Coverage**: Codecov `project` target auto ±1 %, `patch` 75 % ±5 % (`codecov.yml:11-26`) — dashboard, not a hard gate unless required in settings.
73. **Gates must be provable**: adding a `run:` gate to `ci.yml` without an injection in `scripts/prove_gates_fail.sh` is a hard error; a new verdict-bearing step in another workflow needs a Probe or Exempt in `scripts/prove_workflow_gates_fail.py` (`CONTRIBUTING.md:724-725,866-871`). — Gates: `scripts/check_gate_inventory.sh` (`ci.yml:567`), `prove_workflow_gates_fail.py` (`ci.yml:390`), `check_gate_reachability.py` (`ci.yml:411`), `check_block_scalars.py` (`ci.yml:427`), `check_workflow_downloads.py` (`ci.yml:330`: downloads must use `scripts/ci/fetch_verified.sh`/`retry.sh`).

### A1.8 Naming, deprecation, public API, SemVer

74. **Naming (ADR 0053; `CONTRIBUTING.md:284-312`)**: setters `with_<field>` consuming `self`; off-switch `without_<field>`; `RequestHandler` entry point = `on_` + wire method in snake_case (`SubscribeToTask` → `on_subscribe_to_task`; the four push-config methods drop `TaskPushNotification`: `on_create_push_config`); client method = same without `on_`; durations are `std::time::Duration`. — unenforced (ADR 0053 `:104-109` records no gate exists). Note: private REST handlers still use old names (`handle_resubscribe`, `handle_set_push_config`, `server/src/dispatch/rest/mod.rs:615,649` — V).
75. **Rename via deprecation only**: add new; `#[deprecated(since = "X.Y.0", note = "use `new_name`; removed no earlier than X.(Y+1).0")]` on old, old calls new; move every workspace caller (examples, book, READMEs, benches, tests); keep exactly one test of the old name under `#[allow(deprecated)]` with the removal version in its comment (`CONTRIBUTING.md:305-312`; model `server/src/handler/renamed.rs:28-36`). A doc-only rename may be a type alias (`RestRouter` = `A2aRouter`). — unenforced.
76. **Open traits (none sealed)**: never add a required method; a new defaulted method needs (1) a doc paragraph on the cost of the default, (2) every in-repo implementation updated in the same commit, (3) a gate where expressible (`CONTRIBUTING.md:254-282`). Existing mechanical checks: the three "overrides every method" tests for `AuditedTaskStore`, `TracedTaskStore`, `EncryptedTaskStore` (`server/src/audit/tests.rs:358`, `store/encrypted/tests.rs:449`, `store/traced/tests.rs:37`) parse the trait and have thresholds that must be raised by one per new method (ADR 0057 `:36-38`); `scripts/check_otel_metrics_coverage.py` for `Metrics` → `OtelMetrics` (`ci.yml:263`).
77. **Breaking = whatever `cargo-semver-checks` says, or an observable wire change** (`STABILITY.md:70-82`): removing/renaming a public item, changing a signature, a required trait method, a variant/field on a non-`#[non_exhaustive]` type, MSRV raise, changing default features or removing a feature, changing wire shape or which requests are rejected (except a demonstrated spec correction). — Gate: `ci.yml` job `semver` (`obi1kenobi/cargo-semver-checks-action`, packages types/client/server/sdk, `feature-group: all-features`, `ci.yml:1064-1068`). Local equivalent: `cargo semver-checks --package … ×4 --all-features` (from `preflight_all.py --list`). For a human-readable public diff see `handoff.md:233-237`.
78. **Protocol enums and the forty wire structs are `#[non_exhaustive]`, built with `new` + `with_*`; config structs likewise with `Default`** (`STABILITY.md:129-162`). A new public struct/enum should follow. — unenforced.
79. **At most one breaking minor per calendar month; never in a patch; deprecate one minor before removal** (`STABILITY.md:90-118`). — Gate: release-time `check_release_tree.py cadence`. 0.16.0 was 2026-11-19 and is breaking, so **another breaking release in November 2026 needs a `**Cadence exception:**` line** (I from the rule; today is 2026-11-25).
80. **Extension/binding URI contracts**: each `*_EXTENSION_URI`/`WEBSOCKET_BINDING_URI` (`types/src/steering.rs:64`, `lineage.rs:67`, `failure.rs:61`, `provenance.rs:63`, `budget.rs:61`, `idempotency.rs:62`, `lease.rs:57,61`, `approval.rs:60`, `lib.rs:91`) is pinned to a golden snapshot (`crates/a2a-protocol-types/tests/golden/uri_contracts/*.json` ×8, `crates/a2a-protocol-server/tests/golden/uri_contracts/websocket.json`). A wire change with the URI unchanged fails `cargo test -p a2a-protocol-types --all-features --test uri_contracts` / `cargo test -p a2a-protocol-server --features websocket --test uri_contract_websocket`; a person decides: bump the URI (breaking) or re-bless with `A2A_BLESS_URI_CONTRACTS=1` and say why in the commit (`RELEASING.md:112-129`).

### A1.9 Feature flags

81. **Features are additive** (`STABILITY.md:163-164`). — Gates: 14 clippy configs incl. `--no-default-features --features auth-jwt` (`ci.yml:635`); `cargo hack clippy --each-feature` on the four published crates (61 combinations, `handoff.md:1143`); `cargo nextest run --workspace --no-default-features` (`ci.yml:740`); feature tables (rule 53); SQL isolation (rule 47).
82. A new feature must be added in **server (or client/types) `Cargo.toml` and re-exported in `crates/a2a-protocol-sdk/Cargo.toml`** (every server feature has an sdk twin — V from the two `[features]` dumps; exceptions: `conformance`, client `testing`), with `#[cfg_attr(docsrs, doc(cfg(...)))]` (I: `lib.rs:101` enables `doc_cfg`; the nightly job greps for `Available on crate feature otel`, `ci.yml:942`).
83. **A function compiled out under `--all-features`** (`not(feature = …)`, `not(target_os = "linux")`) cannot be killed by the mutation gate: build it in test builds (`#[cfg(any(test, not(feature = "tls-rustls")))]`) and test it, or `#[mutants::skip]` with a compiler proof (`handoff.md:257-262`).
84. Changing **default features** is breaking (rule 77) and stales `itk/` and `fuzz/` lockfiles (rule 46).

### A1.10 MSRV vs stable

85. **MSRV 1.94** (`Cargo.toml:38`); test matrix `rust: [stable, 1.94] × 3 OS` (`ci.yml` job `test`); clippy runs on **stable only** ×3 OS. MSRV is raised only in a minor (`STABILITY.md:166-177`).
86. **Pitfalls**: CI's `stable` floats (1.100.0 on 2026-11-22; session image had 1.97.0 whose clippy "reports 255 errors CI's does not", `handoff.md:96-97,245`). **Do not `#[allow]` a lint the older toolchain does not know** (`unknown_lints` fails) — avoid the lint instead (`handoff.md:229-232`). Cargo 1.100 warns `unused dependency` for `book-tests` (false positive, left alone). Nightly clippy is informational but previews stable lints (`handoff.md:1485-1488`). MSRV leg locally: `rustup toolchain install 1.94 --profile minimal` and `cargo +1.94 check --all-targets` into its own target dir (`handoff.md:1144`). Edition 2024, resolver 3 (`Cargo.toml:7,37`).

### A1.11 ADRs

87. **Numbering**: next free four-digit number (next is **0058**), file `docs/adr/NNNN-kebab-title.md`. **Template** (from `0057`, V): SPDX + copyright HTML comments; `# ADR NNNN: <decision as a sentence>`; `**Date:**`, `**Status:**` (`Accepted`, or `Accepted for X.Y.Z (…)`), `**Author:** Tom F.`; `---`; `## Context`; `## Decision` (or `## Options` + `## Decision`); `## Consequences`; `## Alternatives considered`; optionally `## Revisit trigger`. Amendments are appended as dated `## Amendment, YYYY-MM-DD` sections or inline dated italics (ADR 0002, ADR 0006) — earlier text is kept, not rewritten.
88. **Index**: add a row to `book/src/reference/adrs.md` (complete to 0057 — V). `docs/README.md`'s table is stale at 0021 and no gate reads it (V: `grep -rn 'adrs.md\|docs/adr' scripts/*.py scripts/*.sh` finds only version/feature-table exclusions). — unenforced; `book/src/reference/adrs.md` is in the book so its links are checked by rule 49.
89. "ADR created or updated if an architectural decision was made or revised" (`CONTRIBUTING.md:926`). — unenforced.

### A1.12 Ledgers and registers

90. **Spec ledger** `docs/compliance/spec-ledger.json` (510 entries: TESTED 455, APPLICATION 32, NOT_APPLICABLE 19, PARTIAL 4 — V, ran the checker) + generated `spec-ledger.md`. Gate `scripts/check_spec_ledger.py` (`ci.yml:236`): every keyword line of the vendored spec has an entry; **every citation is `path:line Symbol` / `path:line "fragment"` / `path:line test_fn` and the line must be where that thing is** (`check_spec_ledger.py:27-37`). The JSON holds **3,156 line-number citations across 510 distinct files** (server 1,906, types 645, client 440, slimrpc 82 — V, regex count). **Any edit that moves a cited definition or test fails the gate**; repair: `python3 scripts/check_spec_ledger.py --fix-lines` (moves lines whose target is unique, lists the rest), check each unplaced one by hand, then `--write` to regenerate the `.md`. Apply a remap once, to a tree restored from `HEAD` (`handoff.md:1326-1328,1399-1402`).
91. **RFC ledger** `docs/compliance/rfc-ledger.json` (152 entries, 3 RFCs; 595 line citations in 38 files — V) — Gate `scripts/check_rfc_ledger.py` (`ci.yml:245`), same `--fix-lines`.
92. **Control map** — source `docs/compliance/controls.toml`, generated `control-map.md` (88 controls, 345 cited tests, 106 cited implementations — V). Edit the TOML, run `scripts/check_compliance_map.py --write`; Gate `--check` (`ci.yml:220`) fails when a cited test, file or CI step no longer exists or the Markdown is stale. **Renaming a test, a file or a `ci.yml` step name can break it.**
93. **TCK**: `tck/untested-musts.toml` (Gate `scripts/check_tck_untested_map.py`, `ci.yml:254`), `tck/conformance-baseline.json` (official-tck gate; now empty per `handoff.md:31-32`), `docs/official-tck-findings.md`.
94. **Audit registers**: `docs/compliance/audit-register.md`; `docs/audits/<date>-*/findings.json` (the newest: `2026-11-13-adversarial-review`, 31 rows). A fix for a registered finding updates its row with the evidence (`handoff.md:146-149`). — unenforced, except `release_evidence.py` reads the register's counts at release (`RELEASING.md:139-151`).
95. **Proto and generated types**: every vendored copy byte-identical — reference `crates/a2a-protocol-types/proto`, copies `crates/a2a-protocol-client/proto`, `crates/a2a-protocol-server/proto`, `tck/proto`, `proto`, plus `docs/implementation/a2a.proto`, pinned by `proto/UPSTREAM` (`scripts/check_proto_copies.sh:28-41,90-96`; `ci.yml:89`); generated types match (`cargo run --locked --manifest-path tools/a2a-codegen/Cargo.toml -- --check`, `ci.yml:99`). A hand-written type under a generated type's name anywhere fails (`CONTRIBUTING.md:216-218`). `check_a2a_proto_upstream.sh` (in `official-tck.yml`) excludes pre-releases by design (`handoff.md:43`).
96. **Packaging excludes**: a new `publish = false` workspace member must be added to the `--exclude` list in `ci.yml:1087`, `release.yml` and `RELEASING.md:193` — Gate `scripts/check_package_excludes.py` (`ci.yml:286`).
97. **Provenance manifest** `docs/provenance-manifest.md`: regenerated last, at the release commit — release-time gate only (`RELEASING.md:159-184`).
98. **`docs/handoff.md`** is the session state file: current state only; delete done items; never write the branch's own head SHA (`handoff.md:14-18,1015-1016`). — unenforced.

## A1.13 Workflows: every job and what it gates (V: parsed with `yaml.safe_load`)

| Workflow | Triggers | Jobs (steps/run-steps) — what each gates |
|---|---|---|
| `ci.yml` | push, PR→main, dispatch | `static-checks` (50/47: fmt + 44 script gates, list in A1.14) · `clippy` (stable × ubuntu/macos/windows; 14 feature configs) · `features` (`cargo hack clippy --each-feature`, 4 crates) · `test` (stable+1.94 × 3 OS; 16 nextest configs + 3 doctest configs; timeout 150 min) · `test-postgres` (PostgreSQL 16/18/19 service; 6 steps) · `nightly` (informational, not on PRs) · `doc` (2) · `deny` (action) · `semver` (action) · `package` (`cargo package --workspace --exclude …`) · `dogfood` (`cargo run -p agent-team --release --all-features`) · `slimrpc-binding` (fmt, clippy, build `slim-node`, deny, `package_binding.py`, tests `--test-threads=1`, example, clippy no-default, SPIRE install, SPIFFE tests) · `example-surface` (echo-agent, incident-response ×2, genai, rig — every method × every binding; postgres service) · `example-surface-ops` (multi-lang-team, resilient-agent, swarm) · `go-sdk-interop` (`scripts/go_sdk_interop.sh`) |
| `mutants.yml` | PR (incl. labeled/unlabeled), weekly Mon 03:00, dispatch | `mutants-crate` (70 shards: server 60, types 4, client 4, sdk 1, discovery 1; timeout 240 min; schedule/dispatch only) · `mutants-summary` (reports) · `mutants-incremental-shard` (46 shards; label-gated; timeout 180 min; postgres service) · `mutants-incremental` (the required aggregator; fails unlabelled, on any shard failure, any missed, any timeout) |
| `dco.yml` | PR→main, push main | `dco` |
| `tck.yml` | push, PR→main | `tck-self-test` · `tck-all-bindings` · `tck-cross-language` (8 SDK legs) · `official-client-vs-rust-server` · `card-signature-interop` · `grpc-wire-compat` · `tck-example-agents` (3) |
| `official-tck.yml` | push, PR, nightly 03:41, dispatch | `official-tck` (20/15: upstream a2a-tck at a pin; baseline equality; MUST floor; `check_method_denominator.py`; `check_a2a_proto_upstream.sh`; `check_slimrpc_spec.sh` — the last two read upstream and can go red with no local change, `handoff.md:281-283`) |
| `acts.yml` | push, PR, dispatch, workflow_call | `acts` (ACTS over JSON-RPC, gRPC, HTTP+JSON; Docker build) |
| `itk.yml` | push, PR, nightly, dispatch | `itk-traversal-selftest` · `itk-current-mount` (dispatch only, continue-on-error) |
| `itk-nightly.yml` | nightly 02:00, dispatch, PR touching `itk/**` | `nightly` (current vs every peer SDK) |
| `verification.yml` | push, PR (paths: types crate, `handler/halt.rs`, `handler/messaging/**`, `verification/**`) | `kani` (types-crate proofs, action) · `loom` (halt/admission model) |
| `fuzz.yml` | push, PR, nightly, dispatch | `fuzz` (18 targets; smoke on PR) |
| `benchmarks.yml` | push, PR (paths: `benches/**`, `crates/**`, manifests) | `bench` (not on PR; pushes results to `main` as the bot) · `bench-regression` ("Regression Gate", PR only, 60 min; has flagged noise once, `handoff.md:126-127`) |
| `coverage.yml` | push, PR, weekly, dispatch | `coverage` (llvm-cov + postgres; Codecov upload) · `ignores-applied` (weekly: `check_codecov_ignores.py`) |
| `osv.yml` | PR, push, daily | `rust-pr`, `rust` (blocking), `peers` (informational) |
| `codeql.yml` | PR, push, weekly | `analyze` (rust, actions, python) |
| `observability.yml` | PR, push | `promtool` (rules, alert tests, stack config) |
| `deploy.yml` | push, PR | `image` (build/run/stop deploy-agent) · `manifests` (kubeconform) |
| `soak.yml` | push, PR, nightly | `soak` (sustained load; postgres) |
| `docs.yml` | push, workflow_run (after Release), dispatch | `build`, `deploy` (Pages) |
| `pin-freshness.yml` | weekly, dispatch | `cadence`, `freshness` (4 peer SDKs at latest) |
| `scorecard.yml` | push, weekly | `analysis` |
| `release.yml` | tag push | `validate` (versions, CHANGELOG, tag signature, tree checks) · `ci` (7 reduced legs) · `security` · `ci-evidence` · `package` (+ evidence manifest, SLSA, SBOM) · `conformance` (ACTS) · `attest-conformance` · `publish-dry-run` · `github-release` · `cli-binaries` (var-gated) · `deploy-agent-image` · `publish` |

## A1.14 `scripts/`: every top-level entry, one line each

**G** = a gate run by `ci.yml` `static-checks` (step line in `ci.yml`). **W** = run by another job/workflow. **T** = tool/data. (V: step list parsed from `ci.yml`; purposes from each file's header.)

| Entry | Lines | Kind | Purpose |
|---|---|---|---|
| `check_proto_copies.sh` | 123 | G:89 | Vendored proto copies byte-identical and hash to `proto/UPSTREAM` |
| `check_file_lengths.sh` | 146 | G:106 | 500-line ratchet (`.file-length-baseline`; `--update`) |
| `check_panic_hooks.sh` | 84 | G:117 | No process-global panic hook |
| `check_mutation_scope.sh` | 149 | G:133 | The PR mutation pathspec reaches every library source |
| `check_benchmark_prose.sh` | 278 | G:150 | Benchmark prose numbers equal the tables |
| `check_book_code.sh` | 196 | G:164 | Book `ignore` blocks shrink-only (`.book-ignore-baseline`) |
| `gen_sitemap.py` | 204 | G:172 (`--check`) | Sitemap equals the book TOC |
| `check_book_links.py` | 102 | G:186 | Built-book links and fragments resolve |
| `check_api_reference.py` | 244 | G:200 | API Quick Reference names exist |
| `check_fuzz_matrix.py` | 159 | G:209 | Each fuzz target built and run |
| `check_compliance_map.py` | 449 | G:220 | Control map cites existing tests/files/CI steps; `.md` current |
| `check_no_model_artifacts.py` | 162 | G:228 | No model in a published crate |
| `check_spec_ledger.py` | 384 | G:236 | Spec ledger complete; citations exact (`--fix-lines`, `--write`, `--self-test`) |
| `check_rfc_ledger.py` | 304 | G:245 | RFC ledger likewise |
| `check_tck_untested_map.py` | 119 | G:254 | `tck/untested-musts.toml` true |
| `check_otel_metrics_coverage.py` | 187 | G:263 | `OtelMetrics` covers every `Metrics` callback |
| `check_otel_wiring.py` | 303 | G:270 | Every binary starts Telemetry; no manifest drops `otel` |
| `check_store_delta_forwarding.py` | 110 | G:276 | TaskStore delta writes visible to wrappers |
| `check_package_excludes.py` | 253 | G:286 | Every unpublishable member excluded from packaging |
| `check_doc_versions.py` | 453 | G:296 | Dependency snippets, deploy image tag, README TCK sentence = badge |
| `check_book_examples.py` | 125 | G:304 | Every example has a book page (`book_examples_allowlist.txt`) |
| `check_feature_tables.py` | 334 | G:311 | Feature tables equal manifests |
| `check_net_copies.py` | 156 | G:319 | Client/server outbound-HTTP copies agree |
| `check_workflow_downloads.py` | 242 | G:330 | Workflow downloads retried and verified (with `ci/test_fetch_verified.py`, `ci/test_retry.py`) |
| `hand_mutate.py` | 151 | G:340 (`--self-test`) / T | Apply mutants by hand to one file and run a test command per mutant |
| `check_example_servers.py` | 155 | G:347 | Examples use `Server` (`example_accept_loops.txt`) |
| `check_sql_driver_isolation.sh` | 46 | G:352 | Each SQL feature links only its driver |
| `check_readme_doctests.py` | 50 | G:357 | Crate READMEs compiled as doctests |
| `prove_workflow_gates_fail.py` | 2,425 | G:390 | Each verdict step of the non-`ci.yml` workflows moves both ways on synthetic input |
| `crypto_interop/check.py` | dir | G:397 | Sealed formats open under an independent implementation |
| `check_gate_reachability.py` | 116 | G:411 | Every gate runs on the events that can break it |
| `check_block_scalars.py` | 131 | G:427 | `lib/ci_gates.sh` reads YAML block scalars correctly |
| `check_cancellation_release.py` | 240 | G:441 | Guard flags released by `Drop` |
| `check_doc_escapes.py` | 86 | G:450 | No literal `\n` in doc comments |
| `check_timeout_nesting.py` | 37 | G:465 | Inner timeout < enclosing bound (`timeout_nesting_allowlist.txt`) |
| `check_pg_retry.py` | 259 | G:475 | PostgreSQL statements inside `pg_retry` |
| `run_postgres_suites.py` | 194 | G:482 (`--check`) / W (`test-postgres`) | Finds and runs every PostgreSQL suite |
| `check_panic_paths.py` | 363 | G:492 | Panic-surface ratchet (`panic_paths_baseline.txt`) |
| `check_inert_bounds.py` | 436 | G:509 | Bounds honoured by every sibling (`inert_bounds_allowlist.txt`) |
| `check_advisory_floors.py` | 294 | G:520 | No manifest admits an advised version |
| `check_lockfiles.sh` | 31 | G:527 | Every tracked `Cargo.lock` current |
| `check_mutants_config.py` | 215 | G:564 | `mutants.toml` inert status matches its banner |
| `check_gate_inventory.sh` | 86 | G:567 | Gate inventory complete (reads `preflight.sh`, `prove_gates_fail.sh`, `action_gate_exemptions.txt`) |
| `ci/` (`install_mdbook.sh`, `fetch_verified.sh`, `retry.sh`, 2 tests) | 5 files | G-support | Pinned installs and downloads |
| `check_dco.sh` | 143 | W (`dco.yml`, `benchmarks.yml`) | Sign-off and authorship over a range |
| `go_sdk_interop.sh` | 129 | W (`ci.yml` go-sdk-interop) | a2a-go client/server against this SDK |
| `package_binding.py` | 374 | W (`ci.yml` slimrpc-binding) | Package the binding against in-tree SDK via `[patch]` |
| `install_cargo_mutants.sh` | 154 | W (`mutants.yml`, `ci.yml` nextest version) | Patched cargo-mutants 27.1.0 + nextest 0.9.146 |
| `cargo-mutants/` | 2 files | W-support | The patch and its README |
| `check_a2a_proto_upstream.sh` | 149 | W (`official-tck.yml`) | Vendored proto vs upstream releases |
| `check_slimrpc_spec.sh` | 235 | W (`official-tck.yml`) | Vendored SLIMRPC spec vs upstream (`--update`) |
| `check_method_denominator.py` | 312 | W (`official-tck.yml`) | Method set agrees: proto, a2a-tck, `types/src/method.rs` |
| `acts_predicate.py` | 94 | W (`acts.yml`, `release.yml`) | ACTS reports → attestation predicate |
| `check_codecov_ignores.py` | 293 | W (`coverage.yml` weekly) | codecov ignores actually applied |
| `check_streaming_linearity.py` | 186 | W (`benchmarks.yml`) | Streaming cost linear in events |
| `check_provenance_manifest.py` | 174 | W (`release.yml`) | Provenance manifest describes the commit |
| `check_release_tree.py` | 479 | W (`release.yml`) | `unreleased`, `prep`, `cadence`, `highlights`, `history`, `self-test` |
| `ci_evidence.py` | 177 | W (`release.yml`) | `ci.yml` green for the tagged SHA |
| `release_evidence.py` | 340 | W (`release.yml`) | Release evidence manifest generate/verify |
| `verify_tag_signature.sh` | 197 | W (`release.yml`) | Tag signed by a listed key |
| `build_docs_site.py` | 343 | W (`docs.yml`) | Versioned site assembly |
| `seo_postprocess.py` | 183 | W (`docs.yml`) | Per-page SEO metadata |
| `preflight.sh` | 443 | T | Run `ci.yml` gates locally (`--fmt`, default, `--full`, `--list`) |
| `preflight_all.py` | 207 | T | `preflight.sh --full` + other workflows' runnable jobs + deny/semver equivalents (`--list`) |
| `run_workflow_job.py` | 490 | T | Run one workflow job's `run:` steps locally; `INCOMPLETE` never a pass |
| `prove_gates_fail.sh` | 2,055 | T (referenced by gate inventory) | Inject a defect per `ci.yml` gate and require red citing it |
| `check_extension_vectors.py` | 45 | T (I: not referenced by any workflow) | Extension conformance vectors vs JSON Schema (ADR 0049) |
| `convert_book_ignores.py` | 131 | T | `ignore` blocks → `no_run` doctests |
| `provenance_manifest.sh` | 202 | T | Regenerate `docs/provenance-manifest.md` |
| `render_book_diagrams.sh` | 90 | T | Mermaid → images |
| `lib/` | 27 files | support | `ci_gates.sh`, `ci_gate_audit.sh`, `preflight_prechecks.sh`, `workflow_inventory.py`, `run_workflow_job_{selftest,mutants}.py`, `spec_ledger/`, `gate_reachability/`, `timeout_nesting/`, `api_reference_roots.py` |
| `live/` | 126 files | T | Live-stack probes and campaigns (run without `python3 -I`) |
| `adversarial/` | 9 | T | Hostile-peer probes |
| `audit/` | 2 | T | `find_inert_bounds.py`, `plan_status.py` |
| `codemod/wire_structs` | 6 | T | The G-10 codemod |
| `mutants/push-outbox-h14` | 13 | T | Recorded hand-mutant set |
| `tool_calling_study/` | 28 | T | Model study drivers and data |
| `*.txt` allowlists ×7 | — | data | `action_gate_exemptions`, `book_examples_allowlist`, `doc_versions_allowlist`, `example_accept_loops`, `inert_bounds_allowlist`, `panic_paths_baseline`, `timeout_nesting_allowlist` |

---

## A2. The exact local verification recipe (ordered)

Cheap parts verified here: `preflight.sh --list` (107 commands in `--full`, 5 default, 1 `--fmt`), `preflight_all.py --list`, the checkers in §A1.12. Build steps were **not** run in this audit.

### Step 0 — environment (once per container; `handoff.md:94-105, 245-251, 321-348`)

```sh
git fetch --unshallow --tags origin            # clone is shallow; DCO range, semver baseline, blame need history
rustup toolchain install 1.100.0 --profile minimal -c clippy -c rustfmt   # CI's stable on 2026-11-22; re-read it from a CI log
export RUSTUP_TOOLCHAIN=1.100.0
apt-get update && apt-get install -y openssh-client cmake ninja-build     # ssh-keygen for prove_workflow_gates_fail.py
scripts/install_cargo_mutants.sh               # patched cargo-mutants 27.1.0 + cargo-nextest 0.9.146
cargo install --locked cargo-hack --version 0.6.46 cargo-deny cargo-semver-checks
scripts/ci/install_mdbook.sh "$HOME/.local/bin" && export PATH="$HOME/.local/bin:$PATH"
python3 -m pip install --quiet --require-hashes -r .github/requirements/pyyaml.txt
python3 -m pip install --quiet --require-hashes -r .github/requirements/cryptography.txt
export CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0   # disk: 14 GB -> <4 GB
# For clippy/tests/doc also match CI: RUSTFLAGS="-D warnings" (ci.yml env). NOT for cargo-mutants (step 9).
```
PostgreSQL:
```sh
pg_ctlcluster 16 main start; pg_isready -h localhost
su postgres -c "psql -c \"ALTER USER postgres PASSWORD 'postgres'\""
psql -h localhost -U postgres -c "ALTER SYSTEM SET ssl = off" -c "SELECT pg_reload_conf()"   # or export PGSSLMODE=disable
export A2A_TEST_POSTGRES_URL=postgres://postgres:postgres@localhost:5432/postgres
```
Pitfalls: a container restart stops PostgreSQL (`pg_isready` before reading any red PostgreSQL run); with SSL on, `statement_count` reads `(0, 0)` and its message wrongly says "update the constants" (`handoff.md:76-79,311-315`); stop any local OTLP collector on 4317/4318 (`otel_pipeline_default_endpoint` needs the port free, `handoff.md:1105-1108`); no IPv6 in the kernel.

### Step 1 — after every edit, before every commit (seconds; the three gates that read other files)

```sh
cargo fmt --all -- --check
python3 scripts/check_spec_ledger.py          # then --fix-lines / --write if it fails
python3 scripts/check_rfc_ledger.py
git add -A && ./scripts/check_file_lengths.sh # tracked files only -> after git add
```
Run them as the **last** step before `git commit`, after the final CHANGELOG edit (`handoff.md:1133-1137`).

### Step 2 — the whole `static-checks` job (47 run-steps)

```sh
scripts/run_workflow_job.py .github/workflows/ci.yml static-checks
```
Proves: every script gate in A1.14 + book build + links. Pitfalls: needs `ssh-keygen`, mdbook, promtool download; at 0 MB free four gates fail with no reason and promtool shows `curl: (23)`; leftover `.claude/worktrees/` change what `check_gate_reachability.py` sees (`handoff.md:1212-1214`); a first CI run stops at the first failing step, so run the whole list locally (`handoff.md:1141-1142`).

### Step 3 — clippy, all CI configurations

```sh
scripts/run_workflow_job.py --matrix rust=stable --matrix os=ubuntu-latest .github/workflows/ci.yml clippy     # 14 configs
scripts/run_workflow_job.py .github/workflows/ci.yml features          # cargo hack --each-feature, ~61 combinations
# Windows cfg class:
rustup target add x86_64-pc-windows-gnu && apt-get install -y gcc-mingw-w64-x86-64
cargo clippy --workspace --all-targets --target x86_64-pc-windows-gnu -- -D warnings
```
Proves: lints under every feature set CI names (a feature-gated import is clean under `--all-features` and red under one feature). Pitfalls: disk — `cargo clean -p a2a-protocol-server` between configurations; macOS cannot be cross-checked here.

### Step 4 — docs

```sh
scripts/run_workflow_job.py .github/workflows/ci.yml doc
# all-features docs (only nightly/docs.yml build these in CI):
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --all-features -p a2a-protocol-types -p a2a-protocol-client -p a2a-protocol-server -p a2a-protocol-sdk
```

### Step 5 — tests, one crate at a time

```sh
cargo nextest run --profile ci -p a2a-protocol-server --all-features          # single crate
cargo nextest run --profile ci -p a2a-protocol-server --features sqlite       # a CI feature leg
cargo nextest run --profile ci -p a2a-protocol-server --all-features -E 'test(~name_fragment)'
cargo nextest run --stress-count 50 --no-fail-fast -E 'test(~name)'           # flake measurement
cargo test --doc --workspace --all-features                                    # nextest runs no doctests; this also runs the book blocks
cargo test -p a2a-book-tests --doc                                             # I: book blocks alone (package name from RELEASING.md:193)
```
Whole-workspace legs (`cargo nextest run --profile ci --workspace --all-features` = 8,000 tests on 2026-11-22; `--no-default-features` = 7,968): run on an emptied `target/`, never after other big jobs. Sweep between jobs:
```sh
find target/debug/deps -maxdepth 1 -type f -perm -u+x ! -name '*.so' ! -name '*.d' -delete
rm -rf target/debug/incremental target/nextest target/semver-checks
```
Pitfalls: `cargo test --workspace` stops at the first failing binary (`--no-fail-fast`); a feature-gated test file built without its feature has 0 tests and exits 0 (`$BIN --list | grep -c ': test$'`); `cmd | tail` returns tail's status; `df` overstates the allowance — "linking with cc failed", "bus error", a command that prints nothing = disk full.

### Step 6 — PostgreSQL job

```sh
PGSSLMODE=disable scripts/run_workflow_job.py --services-ready --matrix postgres=16 .github/workflows/ci.yml test-postgres
python3 scripts/run_postgres_suites.py        # before any push touching a PostgreSQL suite
```

### Step 7 — out-of-workspace crates (reached by none of `cargo fmt --all`, `clippy --workspace`, nextest)

```sh
(cd bindings/a2a-protocol-slimrpc && CARGO_TARGET_DIR=<own dir> cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test -- --test-threads=1)
./scripts/check_lockfiles.sh
RUSTFLAGS="--cfg fuzzing" cargo check --manifest-path fuzz/Cargo.toml    # I: fuzz compiles only under --cfg fuzzing (handoff.md:1432)
```
Binding target reaches 12–16 GB; delete afterwards. Also outside the workspace: `itk`, `fuzz`, `verification/loom-halt`, `tools/a2a-codegen`, `benches/sdk-comparison/harness/*`, `scripts/live/replica`.

### Step 8 — generated types

```sh
cargo run --manifest-path tools/a2a-codegen/Cargo.toml -- write      # after editing the proto (all copies) or crates/a2a-protocol-types/codegen.toml
cargo run --locked --manifest-path tools/a2a-codegen/Cargo.toml -- --check
./scripts/check_proto_copies.sh
```
`tools/a2a-codegen` has its own lockfile and needs no `protoc`; it refuses a `codegen.toml` entry the proto lacks and a proto type `codegen.toml` does not mention.

### Step 9 — incremental mutation gate on the diff (reproduce before pushing)

```sh
# 9a. the baseline the shards run (about ten minutes; --no-fail-fast is the point)
cargo nextest run -p a2a-protocol-server --cargo-profile mutants --all-features --run-ignored all --no-fail-fast \
  -E '<filter copied from mutants.yml:1511, not retyped>'
# 9b. the mutants
git diff -M "origin/main...HEAD" -- ':(glob)crates/a2a-protocol-server/src/**/*.rs' > /tmp/pr-src.diff
env -u RUSTFLAGS CARGO_TARGET_DIR=<own dir> cargo mutants --in-diff /tmp/pr-src.diff -p a2a-protocol-server \
  --output <fresh dir> --timeout 300 --build-timeout 1800 --jobs 4 \
  --test-tool=nextest --profile=mutants --all-features \
  -- --run-ignored all -E '<same filter>'
cargo mutants --list --in-diff /tmp/pr-src.diff -p a2a-protocol-server --all-features   # list first
```
Read the **exit code** (0 caught, 2 missed, 3 timeout, 4 baseline failed — prints `caught=0 missed=0`). Pitfalls: **`RUSTFLAGS` must be unset** (CI's mutation job sets none; under `-D warnings` killable mutants read "unviable", `handoff.md:1223-1231`); `--all-features` before `--`; `--test-tool=nextest` mandatory; fresh `--output` (a reused dir mixes runs); `target/mutants` reaches 9 GB — delete it; `A2A_TEST_POSTGRES_URL` set or every PostgreSQL mutant survives; a TIMEOUT whose log shows tests passing is a survivor; doctests do not kill mutants (nextest); every new `impl Debug` is a mutant — list them with `git diff origin/main...HEAD | grep -E '^\+.*impl.*Debug for'` and assert the exact string; never restore with `git checkout --` unless `git diff --quiet` was clean; after restoring from a backup `touch` the file. Repeat per crate changed (types, client, sdk, discovery).

### Step 10 — SemVer, deny, package

```sh
cargo semver-checks --package a2a-protocol-types --package a2a-protocol-client --package a2a-protocol-server --package a2a-protocol-sdk --all-features
cargo deny check
scripts/run_workflow_job.py .github/workflows/ci.yml package
```
Package pitfall: stale `~/.cargo/registry/src/-*/a2a-protocol-*-<ver>` (`handoff.md:1082`).

### Step 11 — conformance, when wire behaviour or a binding changed

```sh
scripts/run_workflow_job.py .github/workflows/tck.yml tck-all-bindings
scripts/run_workflow_job.py .github/workflows/tck.yml tck-self-test
scripts/run_workflow_job.py .github/workflows/official-tck.yml official-tck    # needs network; uv fetches Python >= 3.15; clears /tmp/a2a-tck
scripts/run_workflow_job.py .github/workflows/tck.yml grpc-wire-compat
scripts/run_workflow_job.py .github/workflows/tck.yml official-client-vs-rust-server
scripts/run_workflow_job.py .github/workflows/verification.yml loom
scripts/go_sdk_interop.sh        # before changing HTTP+JSON headers; GOTOOLCHAIN=auto, GOCACHE/GOMODCACHE/GOPATH in scratch
```

### What `run_workflow_job.py` can and cannot run (V: its docstring `scripts/run_workflow_job.py:4-40`; `preflight_all.py --list`)

- **Can**: a job's own `run:` steps, in order, under the runner's shell, with the workflow's `env`. Options `--matrix k=v`, `--expr github.x.y=v`, `--event E`, `--services-ready`, `--log-dir`, `--json`, `--self-test`. No `--help` (V: "refused: unknown option --help").
- **Verdicts**: PASS 0, FAIL 1, refused 2, INCOMPLETE 3 (never a pass), SKIPPED 0.
- **Cannot**: `uses:` steps other than runner set-up (reported, verdict INCOMPLETE); unevaluable `if:`/`${{ }}`; jobs with `services:` unless `--services-ready`; `container:`; non-Linux; include-only matrices need `--matrix`; refuses outright any job whose steps call `sudo`, `git push`, `docker login`.
- **Passed locally per the handoff**: `ci.yml` `static-checks`, `doc`, `clippy` (ubuntu), `features`, `package`, `test-postgres`; `dco`, `itk-traversal-selftest`, `tck-self-test`, `tck-all-bindings`, `official-client-vs-rust-server`, `card-signature-interop`, `grpc-wire-compat`, `loom`, `ignores-applied`, `official-tck`, `promtool`.
- **CI only** (36 of the inventory's jobs): `deny`/`semver` (actions; local equivalents above), `nightly`, mutation jobs (the shard's disk step runs `sudo`), `acts`, `deploy.image`, `itk-nightly` (docker/build-push-action), `coverage`, `codeql`, `kani`, `fuzz`, `soak`, `tck-cross-language`, `tck-example-agents`, `benchmarks`, `osv`, `scorecard`, `docs`, `pin-freshness`, all of `release.yml`; and the macOS/Windows/1.94 legs.

---

## A3. Trap checklist for a fresh session (condensed from `docs/handoff.md:94-105, 217-348, 979-1130, 1131-1511`)

**Identity and git**
- [ ] Commit as the maintainer: `git -c user.name="Tom F." -c user.email=tomf@tomtomtech.net commit -s`; exactly one `Signed-off-by`, no `Co-Authored-By`/`Claude-Session`; verify with `bash scripts/check_dco.sh <base> HEAD`.
- [ ] Label the PR `mutation-test` immediately after creating it; do not re-run the first red run.
- [ ] Push once per batch: a push cancels the running CI. `workflow_dispatch` on the same ref also cancels.
- [ ] `git fetch --unshallow --tags origin` first; `git fetch origin tag v0.15.0` for the semver baseline.
- [ ] Never write the branch's own head SHA into `handoff.md`.
- [ ] `git commit --fixup=amend:` cannot take `-F`: use an `amend! <subject>` commit and `GIT_SEQUENCE_EDITOR=true git rebase -i --autosquash <base>`.
- [ ] Remove merged worktrees under `.claude/worktrees/` before running gates; never build two worktrees at once; shared `target/` races on rlibs.
- [ ] `go build ./...` in an ITK module leaves a binary in the module directory — delete before committing.

**Gates that fail on files you did not think you touched**
- [ ] After *any* edit incl. `CHANGELOG.md`: `cargo fmt --all -- --check`, `check_spec_ledger.py`, the `cargo doc` line. Run them last, after the final edit.
- [ ] CHANGELOG entry ⇒ ledger citation at `spec-ledger.json:76` moves ⇒ `--fix-lines`.
- [ ] Moving code ⇒ `check_spec_ledger.py --fix-lines`, `check_rfc_ledger.py --fix-lines`, `check_compliance_map.py --check`; apply a remap once from `HEAD`.
- [ ] `check_file_lengths.sh` after `git add`; a test module at 500+ lines fails; split by seam with `pub(super)` helpers.
- [ ] `clippy::too_many_lines` (60) applies to tests; extract, do not `#[allow]`.
- [ ] Do not `#[allow]` a lint that an older toolchain does not know.
- [ ] README TCK sentence must read `**N of M MUST requirements pass and F fail**` and equal the badge.
- [ ] Changing default features/deps ⇒ `scripts/check_lockfiles.sh` (itk, fuzz, binding).
- [ ] A new `run:` gate in `ci.yml` ⇒ injection in `prove_gates_fail.sh`; a new verdict step elsewhere ⇒ Probe/Exempt in `prove_workflow_gates_fail.py`.
- [ ] Check a rule in a planning document against the gate that enforces it (the "cadence exception" case).
- [ ] `check_slimrpc_spec.sh` / upstream TCK can turn `Official TCK` red with no local change: `--update`, new hash in `spec/slimrpc_v1/README.md`.

**CI scope wider than a Linux workspace build**
- [ ] Run every clippy config CI runs, plus `cargo hack clippy --each-feature`, plus per-crate `cargo doc`.
- [ ] Windows cross-clippy (`--target x86_64-pc-windows-gnu`); no Unix-path assumptions.
- [ ] No `127.0.0.2`; a "down" address is `::1` with nothing listening (`down_address` in `push/sender_pin_tests.rs`); two replicas = two ports on `127.0.0.1`; `localhost` may resolve to `::1`.
- [ ] Out-of-workspace crates: run each manifest's own fmt/clippy/tests after any codemod.
- [ ] A package right alone can fail the merge: run the whole workspace before merging; verify a "pre-existing" claim against the base commit.

**Mutation testing**
- [ ] Reproduce the shard baseline first (step 9a) with PostgreSQL up, SSL off, password `postgres`.
- [ ] `RUSTFLAGS` unset, own `CARGO_TARGET_DIR`, fresh `--output`, exit code not counts.
- [ ] A red mutation job is not a survivor list: look for `FAILED Unmutated baseline`; read `Found N mutants`, then `MISSED`/`TIMEOUT`/`caught`.
- [ ] Read **every** shard's log, green ones too (`grep '^MISSED \|^TIMEOUT '`); a survivor list written before the run says `completed` is incomplete.
- [ ] Reproduce a mutant from the shard artifact's `diff/` file, never hand-written.
- [ ] `apply_mutant.py`/`hand_mutate.py` `KILLED` = any non-zero exit incl. a compile error: read the failing test names (`grep '^test .* FAILED'`).
- [ ] A kill counts only if the unmutated baseline passed in the same command.
- [ ] Hand mutants chosen by the author miss operators (`||`→`&&`, `<`→`>`, `==`→`!=`, guard→`true`, `+=`→`-=`): run `cargo mutants` on the files a branch adds.
- [ ] Test the function the product calls, not a `#[cfg(test)]` copy; give doctest-only helpers unit tests; tight assertions (`>=`/`<` bounds let `<`→`<=` through; under `start_paused` assert exactly).
- [ ] A test of a guard must assert something the wrong branch changes; check the scenario reaches the guard.
- [ ] New slow/harness/live `#[ignore]` test ⇒ add to the `-E` filter (four copies in `mutants.yml` + CONTRIBUTING).
- [ ] Code compiled out under `--all-features` ⇒ build in test builds or `#[mutants::skip]` with a compiler proof.
- [ ] Interrupted `--in-place`/hand mutant leaves the mutant in the source: `git diff HEAD` first. Background jobs here ignore `SIGINT`: use `SIGTERM`, check `ps`. `pkill -f` kills its own shell: kill by PID.
- [ ] `--in-place` cannot combine with `--jobs`.
- [ ] Tests written for survivors are code: clippy them (`--all-targets`).

**Disk and environment**
- [ ] `CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0`; `df -h /home/user` before and after large runs; the allowance is smaller than `df`.
- [ ] Sweep test executables (the `find … -delete` line) between jobs; `cargo clean -p a2a-protocol-types -p a2a-protocol-client -p a2a-protocol-server -p a2a-protocol-sdk` removes stale per-feature builds; do not prune `incremental` while cargo runs.
- [ ] One big job at a time; the workspace suite and the all-features server suite do not fit together.
- [ ] PostgreSQL and llama-server die on container restart; results in scratch die too — commit raw results as they arrive.
- [ ] `ss` is absent: `fuser PORT/tcp` or `lsof -i`. `curl` applies `NO_PROXY` even to `-x`: `--noproxy ''`.
- [ ] `tail --pid=<pid> -f /dev/null` waits for a background command.
- [ ] `scripts/live/*.py` run with plain `python3` (not `-I`); `c1_chaos.py` and campaign drivers with `python3 -I`.

**Testing craft**
- [ ] No time margins a slow runner can lapse; `Date` has one-second resolution (margin > 1 s or a `Date` ahead of now); count events instead of comparing a duration with the period producing it.
- [ ] A test sharing a database gets its own schema; a claim that expired can always be retaken — read the row instead.
- [ ] A hang that only shows with earlier tests in the same process is still a product bug; bound each send (300 s) so it fails with a message.
- [ ] Measure a flake with `--stress-count N` under busy loops; prove the fix with more runs than found it; a timeline names a cause, counts do not.
- [ ] Capture warnings through `crate::handler::warnings_during`; warnings arrive in code order.
- [ ] `serde_json/preserve_order` may be on (genai example): sort explicitly where order is a contract.
- [ ] A serde change needs fixed inputs parsed with both builds; semver-checks does not show it.
- [ ] An end-to-end test through the client finds server bugs handler tests miss.
- [ ] Budget an independent review on any change to the stream or store contract.
- [ ] Model-backed code caps output and turns thinking off; a live test asserts what holds whatever the model does.

**Release (maintainer-driven; `RELEASING.md`, `handoff.md:979-993`)**
- [ ] Provenance manifest last; wait for the benchmark bot's commit; eight inter-crate pins + every lockfile; write notes last; merge commit not squash; signed tag on the prep commit; `for c in unreleased prep cadence highlights; do python3 scripts/check_release_tree.py "$c" vX.Y.Z HEAD; done`.

---

## A4. Maintainability risks of the engineering system, and simplifications

### A4.1 Evidence

1. **CI cost.** A labelled PR starts 46 mutation shards (timeout 180 min each; server shards measured at 116.6–177.9 min when there were 20, `handoff.md:265-266`; "about six hours of runner time" per cycle per shard-read mistake, `handoff.md:1151`); the weekly sweep is 70 shards × 240 min cap. `test` is 6 legs × 19 build configurations (timeout 150 min; Windows ≈100 min, macOS ≈90 min, `handoff.md:1197-1198`). Counting matrix expansion, a PR to `main` fans out to roughly: ci 3+1+6+3+1+1+1+1+1+1+1+1+1 = 22, mutants 47, tck 1+1+8+1+1+1+3 = 16, fuzz 18, codeql 3, plus ~14 single jobs ≈ **120 jobs** (V arithmetic over the parsed matrices; path filters can remove benchmarks/verification/itk-nightly).
2. **The mutation `-E` filter exists in five hand-synchronised copies** (`mutants.yml:685,1177,1511,1674`; `CONTRIBUTING.md:461` — the CONTRIBUTING copy is already stale: it lacks `readiness_bar` and the five `*_live` binaries). V.
3. **Line-number coupling.** 3,156 + 595 `path:line` citations in the two ledgers over ~548 files; one of them points into `CHANGELOG.md`, which every user-visible change edits above the cited line. The handoff records this failing a commit four times in one session (`handoff.md:1133-1137`). V.
4. **Baselines/allowlists: 11 files** — `.file-length-baseline` (82 files, i.e. 3.8 % of tracked sources exceed the limit and the list "may only shrink"), `.book-ignore-baseline`, `scripts/panic_paths_baseline.txt`, `inert_bounds_allowlist.txt` (129 lines), `doc_versions_allowlist.txt`, `timeout_nesting_allowlist.txt`, `action_gate_exemptions.txt`, `example_accept_loops.txt`, `book_examples_allowlist.txt`, `tck/conformance-baseline.json`, `tck/untested-musts.toml`. V.
5. **Gate machinery is itself large and over its own limit.** `prove_workflow_gates_fail.py` 2,425 lines, `prove_gates_fail.sh` 2,055, `mutants.yml` 92,988 bytes, `ci.yml` 84,795 bytes; 44 script gates in one job. Two parsers of the same YAML (`preflight.sh`/`lib/ci_gates.sh` and `run_workflow_job.py`) — the first pair once shared the same `env:` bug (`CONTRIBUTING.md:808-811`).
6. **Stale rule text in the binding documents** (each V above): `CONTRIBUTING.md:356-357` (openssl-sys ban, Unicode-DFS-2016) vs `deny.toml`; `CONTRIBUTING.md:635` "other 18 workflows"; `CONTRIBUTING.md:461` filter; `CONTRIBUTING.md:908-909` "81 of 333" vs 82 of 2,147; `STABILITY.md:122` "twelve traits"; `docs/README.md` ADR table at 0021; handoff "58 ADRs"; `mutants.toml` is a 256-line file whose banner says nothing in it is in effect; PR template says "No file exceeds 500 lines" while the rule is a ratchet. `RELEASING.md` contains ~200 lines of superseded history inline.
7. **Unenforced rules that a fresh session will get wrong**: "no other trailer"; SPDX; naming rule; deprecation note format; ADR index; `ROADMAP.md`/book changelog at release; regression-test-first.
8. **Duplicated configuration**: package `--exclude` list ×3 (gated); proto ×6 copies (gated); client/server net code ×2 (gated); nextest version read by `sed` from `install_cargo_mutants.sh`; version strings in 5 manifests + 8 pins + 4+ lockfiles.
9. **Local reproduction needs ~20 minutes of tool compilation and a specific stable**, and disk exhaustion is the most-recorded failure in the handoff (≥9 separate lessons).
10. **CHANGELOG as a 984 K single file**: GitHub refuses a release body over 125,000 characters, and 0.15.0's section is ≈289,000 (`RELEASING.md:292`); the 0.16.0 section spans lines 36–3,289 (V).

### A4.2 Co-update sets (what one change obliges, with the gate that notices)

**(i) Bug fix in a published crate**
1. A failing-first regression test (sibling `*_tests.rs` or `crates/*/tests/`).
2. The fix.
3. `CHANGELOG.md` `[Unreleased]` → `### Fixed` (or `### Security`).
4. `docs/compliance/spec-ledger.json` line citations (the CHANGELOG one always; code ones if cited lines moved) + regenerated `spec-ledger.md`; same for `rfc-ledger.json`/`.md` — gates 90–91.
5. `docs/compliance/controls.toml` → `control-map.md` if a cited test/file was renamed — gate 92.
6. `.file-length-baseline` if a file crossed 500 either way — gate 23.
7. `scripts/panic_paths_baseline.txt` if `expect`/`unwrap` count changed — gate 27.
8. Register row (`docs/audits/*/findings.json` or `docs/compliance/audit-register.md`) if the bug is a registered finding.
9. `book/src/reference/pitfalls.md` if a non-obvious pitfall; the book page describing the behaviour if it was wrong.
10. Mutation kills for every changed line; commit message naming them.
11. `docs/handoff.md` state.
12. If PostgreSQL-related: test discoverable by `run_postgres_suites.py`; if a slow/ignored test was added: the five filter copies.

**(ii) New public API (additive)** — all of (i).3–11, plus:
13. Doc comments (`deny(missing_docs)`), `#[non_exhaustive]`, `new`/`with_*` naming (rules 74, 78).
14. Re-export in `server/src/lib.rs` / `client/src/lib.rs` and, where appropriate, `crates/a2a-protocol-sdk/src/lib.rs` (prelude) + `crates/a2a-protocol-sdk/tests/reexport_tests.rs` (I: the file exists; read it).
15. `book/src/reference/api-reference.md` (gate 54), the relevant book page (compiled by book-tests), crate README if it lists the surface.
16. If a new defaulted **trait method**: every in-repo impl in the same commit — for `TaskStore` the 6 stores + 3 wrappers (anchors in B5) and their three "overrides every method" thresholds; for `Metrics`: `OtelMetrics`, `metrics/tee.rs` (gate 76).
17. If it adds a configurable bound: every sibling implementation or `inert_bounds_allowlist.txt` (gate 33); if a timeout: `check_timeout_nesting.py`.
18. `cargo semver-checks` clean (additive).
19. Every new `impl Debug` tested with its exact string.
20. `STABILITY.md` §4 lists if the item is a new extension trait or config struct (unenforced; the lists have been wrong before).

**(iii) Wire change** — all of (ii), plus:
21. Proto: all 6 copies + `proto/UPSTREAM` (if from upstream) → `codegen.toml` → `a2a-codegen write` (gates 95).
22. `types/src/method.rs` if a method (enum, `ALL`, `wire_name`, `is_streaming`, tests) — cross-checked by `check_method_denominator.py` against the proto and upstream a2a-tck (in `official-tck.yml`).
23. `types/src/proto/convert/*` (proto ↔ domain), serde shape, `crates/a2a-protocol-types/tests/tck_wire_format.rs`, `corpus_json.rs`, `proptest_types.rs`, a fuzz target if a parser.
24. Every binding (B6 checklist), client transports, the SLIMRPC binding, CLI, harness.
25. If an extension/binding this SDK defines: URI golden snapshot; new URI if breaking (rule 80).
26. `### Breaking Changes` or `### Behaviour Changes` entry with migration; `book/src/reference/upgrading.md` section; possibly `**Cadence exception:**`.
27. Spec ledger **entries** (not just line numbers): a new requirement needs rows with impl/test evidence; `docs/implementation/a2a-v1.0.1-specification.md` is the vendored text the ledger cites (a new spec version means a new vendored text and re-audit).
28. `tck/` (own tests, `tck/src/equivalence.rs`, `tck/fixtures/grpc/corpus/*.json`, `tck/untested-musts.toml`, `tck/conformance-baseline.json`), `acts/sut-behaviors.yaml`, `itk/src/*`, `scripts/go_sdk_interop.sh` peers; README TCK sentence + badge if the counts change; `book/src/reference/conformance-statement.md`, `docs/official-tck-findings.md`.
29. Observability: `rpc_span/names.rs`, client `call_span.rs`, Prometheus rules under `deploy/observability/prometheus/rules/` and `deploy/kubernetes/observability/prometheusrule.yaml` if they enumerate methods (both files mention `SubscribeToTask` — V), `examples/deploy-agent/network.yaml`.

**(iv) New ADR**
30. `docs/adr/0058-<title>.md` with the template (rule 87).
31. Row in `book/src/reference/adrs.md` (and, optionally, repair `docs/README.md`'s table).
32. Cross-references from the code's module docs / book page; CHANGELOG entry citing the ADR if user-visible; `ROADMAP.md` if it commits work; `docs/handoff.md` "Decisions the maintainer made" if it was the maintainer's decision.
33. If the ADR page embeds dependency snippets: `check_doc_versions.py` excludes `book/src/reference/adrs.md` (V: `check_doc_versions.py:186`) but not `docs/`? — `docs/` is excluded wholesale (`:36-39`).

**(v) New feature flag**
34. `[features]` in the owning crate's `Cargo.toml` (+ optional deps), twin in `crates/a2a-protocol-sdk/Cargo.toml`, `[package.metadata.docs.rs]` if needed.
35. `#[cfg(feature = …)]` on modules/re-exports with `doc(cfg)`; code that must exist without it tested in test builds (rule 83).
36. Feature tables: root `README.md`, `crates/README.md`, the crate's `README.md`, `book/src/getting-started/installation.md`, `book/src/reference/configuration.md` (gate 53).
37. `ci.yml`: a `Clippy (<feature>)` step (`ci.yml:605-637`) and a `Test (<feature>)` step (`ci.yml:707-740`) — each new `run:` step needs an injection in `prove_gates_fail.sh` and passes `check_gate_inventory.sh`; `release.yml`'s reduced CI copy if it enumerates features (I — read it).
38. All tracked lockfiles (gate 46); `deny.toml` skips/licences for new deps; `check_advisory_floors.py`; `check_sql_driver_isolation.sh` if SQL.
39. `tools/a2a-cli`, `tck/sut`, `itk`, examples that should enable it; `check_otel_wiring.py` if it affects telemetry.
40. CHANGELOG `### Added`; `STABILITY.md` if default features change (breaking).
41. Mutation: the gate builds `--all-features`, so the feature is on there; code under `not(feature)` needs rule 83.

### A4.3 Proposed simplifications (each keeps the evidence bar; each has a mechanical acceptance check)

| # | Change | Toil removed | Acceptance check |
|---|---|---|---|
| S1 | **Make ledger citations symbol-addressed.** Store `path Symbol` / `path "fragment"` / `path test_fn` in `spec-ledger.json`, `rfc-ledger.json`; let the checker *resolve* the line (it already locates unique targets for `--fix-lines`) and render line numbers only into the generated `.md`. Ambiguous symbols keep an explicit disambiguator (e.g. `impl` block name), not a line. | The single most-recorded recurring red (4× in one session); 3,751 line numbers stop being source of truth. | `grep -cE '"[^"]+:[0-9]+ ' docs/compliance/*.json` = 0; `check_spec_ledger.py --self-test` still refuses a missing symbol and an ambiguous one; inserting a blank line at the top of a cited file leaves the gate green. |
| S2 | **Drop the CHANGELOG line citation**: cite `CHANGELOG.md "## [0.5.0] - 2026-04-02"` by fragment only (subsumed by S1; can be done alone today). | Every `[Unreleased]` entry stops breaking the ledger. | Add a line under `[Unreleased]`; `python3 scripts/check_spec_ledger.py` exits 0. |
| S3 | **One source for the mutation test filter.** Put the nextest filter in `.config/nextest.toml` as a profile `default-filter` (or a file `scripts/mutants/test-filter.txt` read by all four `mutants.yml` sites and printed in CONTRIBUTING by reference). | Five hand-synced copies → one. | `grep -c 'binary(soak)' .github/workflows/mutants.yml CONTRIBUTING.md` totals ≤1; `prove_workflow_gates_fail.py` gets a probe that a harness binary named in the file is excluded. |
| S4 | **A single `scripts/precommit.sh`** (or `preflight.sh --quick`) that runs exactly: fmt check, both ledger checkers, compliance map, file lengths (on the index), panic paths, doc versions, feature tables, lockfiles — the gates that fail on files other than the one edited, all Rust-build-free. Wire it as the default `pre-push` tier. | Replaces the "run the three gates after every commit" lesson with one command (seconds). | The command list is derived from `ci.yml` by step name (like `preflight.sh`), so `check_gate_inventory.sh` covers drift; `time scripts/precommit.sh` < 60 s without a `target/`. |
| S5 | **Split `CHANGELOG.md` by release**: `CHANGELOG.md` keeps `[Unreleased]` + the current minor and an index; older sections move to `docs/changelog/0.15.0.md` … (git history preserved by `git mv`-style split). | 984 K file out of every edit/diff/merge conflict; release-body 125 k limit handled by construction. | `wc -c CHANGELOG.md` < 300,000; `check_release_tree.py self-test` + `history` unchanged verdicts; `book-tests/tests/upgrading_guide.rs` reads the index. |
| S6 | **Gate the two cheap unenforced commit rules in `check_dco.sh`**: reject any trailer key other than `Signed-off-by` whose value names an assistant (`Co-Authored-By: .*(Claude|Copilot|noreply@anthropic)`, `Claude-Session:`); and add an SPDX-first-lines check to `check_file_lengths.sh`'s file walk (same file set). | Two rules a fresh session is primed to break by its own harness stop failing silently/at review. | `prove_workflow_gates_fail.py` probe: a synthetic commit with a `Co-Authored-By: Claude` trailer turns `dco` red; a tracked `.rs` without SPDX turns static-checks red. |
| S7 | **Generate the stale lists instead of writing them**: ADR table in `docs/README.md` and `book/src/reference/adrs.md` from the ADR files' `# ADR NNNN:` + first `Decision` sentence; trait/config lists in `STABILITY.md` §4 from rustdoc JSON or a grep with a pinned count; the "N of M files exceed" sentence in CONTRIBUTING replaced by "run the script". | Removes four documents from the co-update set for (ii) and (iv). | A `--check` mode in static-checks; deleting an ADR row fails it. |
| S8 | **Risk-tier the incremental mutation fan-out**: compute the shard count from `cargo mutants --list --in-diff | wc -l` in a planning job (e.g. ⌈N/40⌉ capped at 40) instead of always 40 server shards; skip crates with an empty diff before allocating a runner (today each shard starts, builds the diff, then skips). | Runner-hours and wall-clock on small PRs; same zero-survivor/zero-timeout verdict. | For a one-line server diff the run has ≤2 server shard jobs; the aggregator's three gates and its "unlabelled fails" behaviour are unchanged and still proven by the existing probes. |
| S9 | **Delete `mutants.toml`** (or move the measured history to `book/src/reference/mutation-history.md`) and drop `check_mutants_config.py`. A file whose banner says nothing in it applies, guarded by a gate that checks the banner, is pure carrying cost. | One gate, 256 + 215 lines, one trap ("configuration is not where it looks"). | `test ! -e mutants.toml`; `cargo mutants --list -p a2a-protocol-types --all-features | wc -l` unchanged before/after. |
| S10 | **Machine-readable touch-point manifests for the two highest-fan-out changes**: `scripts/lib/method_sites.toml` listing every file that must name each `Method` (the B6 list), checked by a script that greps each site for every `Method::ALL` wire name; same for `TaskStore` methods × implementations (generalising the three "overrides every method" tests into one script over all 9 impls). | Turns B6/B7's checklists from prose into a red gate naming the missing site. | Add a fake method name to the proto fixture in the script's `--self-test`; it lists every site in B6. |
| S11 | **Prune `RELEASING.md`/`CONTRIBUTING.md` history into `docs/handoff-history.md`-style records**, leaving rules + commands. ~35 % of CONTRIBUTING (lines 694–900) is incident narrative. | Shorter binding documents; fewer stale counts. | `wc -l CONTRIBUTING.md` < 500 (the project's own limit for sources); every rule in A1 still has a `file:line`. |
| S12 | **One documented, pinned toolchain for local parity**: a `rust-toolchain.toml`-style *documented* pin file (e.g. `.github/ci-stable.txt`) written by CI's log step and read by `preflight.sh`, so "CI's stable is X" is a file, not a line read from a log. | The 1.97 vs 1.99.1 vs 1.100.0 class of trap. | `preflight.sh` refuses to run when `rustc -V` ≠ the file unless `PREFLIGHT_ANY_TOOLCHAIN=1`. |

Not proposed: weakening zero-survivor, removing the provers, or adding a mutation baseline — all three would lower the evidence bar the project explicitly chose (ADR 0006).

---

# PART B — ARCHITECTURE MAP

## B1. Size (V: `find … -name '*.rs' | xargs cat | wc -l`)

| Unit | Path | `src` files / lines | `tests/` files / lines | Published? |
|---|---|---|---|---|
| types | `crates/a2a-protocol-types` | 88 / 28,099 | 27 / 7,892 | yes 0.16.0 |
| client | `crates/a2a-protocol-client` | 171 / 51,180 | 122 / 23,456 | yes |
| server | `crates/a2a-protocol-server` | **772 / 222,057** | 351 / 87,643 | yes |
| sdk (umbrella) | `crates/a2a-protocol-sdk` | 1 / 154 | 51 / 9,894 | yes |
| discovery (mDNS) | `crates/a2a-protocol-discovery` | 4 / 819 | 1 / 228 | workspace member, excluded from packaging (`RELEASING.md:193`) |
| SLIMRPC binding | `bindings/a2a-protocol-slimrpc` (own workspace, v0.7.0) | 20 / 5,401 | 23 / 5,836 | never published |
| CLI | `tools/a2a-cli` (bin `a2a`) | 9 / 2,419 | 7 / 1,673 | no (release binaries) |
| codegen | `tools/a2a-codegen` (own lockfile) | 7 / 1,901 | — | no |
| TCK | `tck` (bin `a2a-tck`, deliberately no dependency on the SDK types) | 13 / 5,315 | — | no |
| TCK SUT | `tck/sut` | 3 / 709 | — | no |
| ITK agent | `itk` (own workspace) | 7 `.rs` / 1,455 total | — | no |
| fuzz | `fuzz` (own workspace) | 18 targets / 755 | — | no |
| loom | `verification/loom-halt` | 2 / 97 | — | no |
| benches | `benches` | 30 / 7,402 (15 criterion benches) | — | no |
| book-tests | `book-tests` | 3 / 661 (169 `include_str!`) | — | no |
| examples | `examples/*` (16 crates) | 125 / 30,498 | — | no |

Server is 73 % of all crate source. By the name heuristic (`*_tests.rs`, `tests.rs`, `tests/` dirs), server `src` splits into **455 production files / 139,397 lines** and **317 test files / 82,660 lines**; but 404 `src` files across the crates also contain inline `#[cfg(test)]` modules, so "production" lines include tests (V counts; the split is a heuristic).

## B2. Module tables (lines = prod / test-named, heuristic as above)

### types (`crates/a2a-protocol-types/src`) — lints at `lib.rs`; features `signing`, `audit` (⇒ signing), `proto`

| Module | Lines | Responsibility |
|---|---|---|
| `generated/` (10 files) | 1,331 | Output of `tools/a2a-codegen`; `include!`d by the documenting module (e.g. `security.rs:34`), never a module itself |
| `proto/` (`mod.rs`, `convert/{card,messaging,requests}.rs`) | 3,135 / 329 | prost types (feature `proto`, built by `build.rs:7-42`) and proto ↔ domain conversions |
| `error.rs` | 1,736 | `A2aError`, error codes, details |
| `signing.rs` + `signing/` | 2,033 / 1,119 | JWS agent-card signing/verification (feature `signing`) |
| `audit/` | 916 / 468 | Audit record types, hash chain (feature `audit`) |
| `message.rs`, `task.rs`, `artifact.rs`, `events.rs`, `push.rs`, `security.rs`, `responses.rs`, `params/`, `agent_card/`, `extensions.rs` | 1,171; 1,072; 257; 162; 398; 615; 316; 430; 669; 67 | Hand-written `impl`s and docs around the generated wire types |
| `jsonrpc.rs` | 792 | JSON-RPC 2.0 envelope types |
| `method.rs` | 459 | `Method` enum, `ALL`, `wire_name`, `is_streaming` — the method registry (`method.rs:64-146`) |
| `wire_builders*.rs` | 924 | `new`/`with_*` builders for the forty wire structs |
| Extension types: `steering.rs`, `lineage.rs`, `failure.rs`, `provenance.rs`, `budget.rs`, `idempotency.rs`, `lease.rs`, `approval.rs`, `delegation_journal.rs`, `trace_context.rs`, `auth_rejection.rs` | 84–500 each | Each defines an `*_EXTENSION_URI` and its metadata shape |
| `serde_helpers.rs`, `oneof.rs`, `required.rs`, `whole_number.rs`, `extension_value.rs` | small | Serde plumbing |
| `kani_proofs.rs` | 108 | `#[cfg(kani)]` proofs (`lib.rs:158-159`) |

Public traits: `AcceptedFields` (`params/mod.rs:204`), `Content` (sealed, `provenance.rs:96`), `DelegationJournal` (`delegation_journal.rs:153`).

### client (`crates/a2a-protocol-client/src`) — features: default `tls-rustls, tracing, otel, webhook`; `signing`, `websocket`, `grpc`, `grpc-tls`, `testing`

| Module | Lines | Responsibility |
|---|---|---|
| `transport/` (`mod.rs`, `jsonrpc.rs`, `rest/`, `grpc.rs` 1,925, `websocket.rs` 2,567, `connector.rs`, `dns.rs`, `resolver.rs`, `in_flight*`) | 9,434 / 1,066 | `Transport` trait (`transport/mod.rs:313`: `send_request`, `send_streaming_request`, `send_request_with_query`, all keyed by a method-name `&str`) and its four implementations |
| `methods/` (`send_message.rs`, `send_over_stream.rs`, `tasks.rs`, `push_config.rs`, `extended_card.rs`, `get_task.rs`, `follow.rs`) | 2,265 / 1,052 | `A2aClient` methods, one per wire method (`tasks.rs:131 subscribe_to_task`) |
| `streaming/` (`event_stream.rs` 1,684, `sse_parser/`) | 3,232 / 415 | `EventStream` (implements `futures_core::Stream`), SSE parsing |
| `builder/` | 2,688 / 1,179 | `ClientBuilder` |
| `delegation/` (incl. `steer.rs`) | 2,438 / 804 | Delegating to child agents: lease, budget, journal, steering |
| `discovery.rs` + `discovery/` | 3,142 / 1,267 | Agent-card fetch, cache (RFC 9111), verification |
| `retry.rs` + `retry/` | 2,177 / 574 | Retry policy; per-method idempotence (`retry.rs:256,304`) |
| `token_provider*`, `auth.rs`, `jwks.rs`, `credential_preflight.rs` | ~3,500 | Credentials: `TokenProvider` (`token_provider.rs:140`), `CredentialsStore` (`auth.rs:80`) |
| `balance*`, `pool/`, `proxy.rs`, `tls.rs`, `https_client.rs`, `origin*`, `plaintext.rs`, `loopback.rs` | ~3,000 | Endpoint balancing (`EndpointSource`, `balance.rs:107`), pooling, proxies, TLS |
| `interceptor.rs` | 395 | `CallInterceptor` (`interceptor.rs:154`) |
| `call_span*`, `call_metric*`, `trace*`, `otel_baggage*` | ~1,500 | Client telemetry; method → RPC name table at `call_span.rs:322-332` |
| `webhook/` | 1,045 | Push-notification receiver helpers (feature `webhook`) |
| `testing/` (`http1.rs`, `ws.rs`, `grpc.rs`) | 557 | Mock servers (feature `testing`); detect streaming by method name (`testing/http1.rs:99`, `ws.rs:34`, `grpc.rs:67`) |

### server (`crates/a2a-protocol-server/src`) — module list at `lib.rs:112-179`

| Module | prod f / L | test f / L | Responsibility |
|---|---|---|---|
| `handler/` | 81 / 26,622 | 93 / 26,933 | `RequestHandler` (`handler/mod.rs:119`): the binding-independent core; see B4 |
| `store/` | 73 / 25,967 | 44 / 12,025 | `TaskStore` + 6 stores + 3 wrappers; claims, halt, lease, retention, migrations; see B5 |
| `dispatch/` | 46 / 16,182 | 9 / 1,443 | One dispatcher per binding; see B3 |
| `tool_loop/` (feature `tool-loop`) | 34 / 7,810 | 26 / 6,163 | Model/tool loop executor: `ToolModel` (`model.rs:349`), `Tool` (`tool.rs:73`), `ToolPolicy` (`policy.rs:184`), `StepJournal` (`journal/mod.rs:375`) |
| `push/` | 26 / 8,281 | 17 / 3,906 | `PushSender` (`sender.rs:94`), `PushConfigStore` (`config_store.rs:19`) ×5 impls, `PushOutbox` (`outbox.rs:125`), `PushConfigCipher` (`cipher.rs:48`) |
| `otel/` (feature `otel`; `otlp`, `otel-prometheus`, `otlp-json`) | 32 / 7,464 | 15 / 4,254 | `OtelMetrics`, `Telemetry` |
| `audit/` (feature `audit`) | 21 / 5,408 | 16 / 4,156 | `AuditStore` (`store.rs:106`), `AuditedTaskStore` (`task_store.rs:106`), privacy/sealing |
| `streaming/` | 11 / 7,070 | 10 / 2,335 | `event_queue/` (`EventQueueWriter`/`Reader`, `mod.rs:193,218`; `in_memory.rs` 3,155; `manager.rs` 1,032) and `sse.rs` (`build_sse_response`, `sse.rs:389`) |
| `auth/` | 10 / 4,556 | 9 / 2,274 | API-key, bearer, JWT (`jwt.rs` 2,595, feature `auth-jwt`), mTLS interceptors |
| `rpc_span/` | 20 / 4,328 | 9 / 2,406 | One RPC span shape for every binding; method → name table `names.rs:122-135`; `BindingSpan`, `CustomBinding`, `WireStatus` (`names.rs:86`) |
| `skills/` (feature `skills`) | 11 / 2,846 | 19 / 3,855 | Agent skills on the tool loop |
| `rate_limit/` | 9 / 1,854 | 7 / 2,305 | `RateLimitCounter` (`shared.rs:84`), in-memory and PostgreSQL |
| `serve/` | 12 / 2,473 | 3 / 1,224 | `Dispatcher` trait (`serve/mod.rs:95`), `Server`, `ServeConfig`, graceful shutdown |
| `agent_card/` | 8 / 2,681 | 2 / 503 | Card serving: `AgentCardProducer` (`dynamic_handler/mod.rs:31`), `ExtendedCardProvider` (`per_caller.rs:40`) |
| `builder.rs` + `builder/` | 6 / 3,453 | 4 / 1,091 | `RequestHandlerBuilder` (`builder.rs` is 2,699 lines) |
| `conformance/` (feature `conformance`) | 6 / 1,634 | 4 / 992 | Self-check helpers |
| `net/` | 6 / 1,080 | 6 / 1,160 | Outbound HTTP (copy of the client's, rule 36) |
| `metrics/` | 5 / 887 | 1 / 282 | `Metrics` trait (`metrics/mod.rs:52`), `tee.rs`, labels |
| `interceptor/` | 2 / 498 | 1 / 354 | `ServerInterceptor` (`interceptor/mod.rs:56`) |
| `admission/`, `approval*`, `steering/`, `executor.rs`, `executor_helpers*`, `request_context.rs`, `call_context*.rs`, `tenant_resolver.rs`, `tenant_config/`, `profile*.rs`, `url_guard*`, `error/`, `provenance.rs`, `tls*`, `sqlite_pool*`, `sql_error.rs` | 40–600 each | `AgentExecutor` (`executor.rs:78`: `execute`, `cancel`, `resumes_after_hand_off`, `on_shutdown`), `TenantResolver` (`tenant_resolver.rs:94`), steering `Inbox`, deployment profiles |

Feature → modules gated (V: files containing `feature = "<f>"`): `sqlite` 56 files (store 25, handler 9, audit 7); `postgres` 47 (store 28, push 5, audit 4); `tracing` 51 (handler 19, rpc_span 15); `audit` 29 (handler 11, builder 5); `tls-rustls` 24 (dispatch 7, net 6); `otel` 20 (rpc_span 12); `encryption` 13; `websocket` 12 (dispatch 5, serve 3); `grpc` 8; `grpc-tls` 8; `axum` 6; `otel-prometheus` 6; `otlp` 4; `otlp-json` 4; `auth-jwt` 3; `tool-loop` 3 (+ whole `tool_loop/`); `skills` 2 (+ whole `skills/`); `conformance` 2; `signing` 7. Whole-module gates: `audit` (`lib.rs:119`), `skills` (`:162`), `tool_loop` (`:164`), `conformance` (`:170`), `otel` (`:173`), `tls` (`:155`), `sqlite_pool` (`:142`).

### sdk, discovery, slimrpc, cli, tck

- **sdk** `crates/a2a-protocol-sdk/src/lib.rs` (154 lines): re-exports + prelude; 20 features, each forwarding to client/server/types. Its `tests/` (51 files) hold the cross-binding suites: `binding_equivalence/`, `operation_matrix/`, `observability_e2e/`, `injection_containment/`, `grpc_*`, `websocket_on_http_port.rs`, `reexport_tests.rs`.
- **discovery**: `announce.rs` 338, `discovered.rs` 223, `browse.rs` 132, `lib.rs` 126 — mDNS announce/browse (ADR 0029).
- **slimrpc**: `binding.rs` 468; `server/mod.rs` 283, `server/methods/mod.rs` 496 (`register_a2a_methods` at `:88`, 11 methods; calls `RequestHandler` directly), `server/peer/`; `client/{mod,dispatch,bridge,push_config}.rs`; `multicast/`; `codec.rs`; `error.rs`; method-name constants `lib.rs:257-267`; bin `slim_node.rs`. Depends on server/types as **public** dependencies at a tight `0.16`.
- **a2a-cli**: `cli.rs` (clap; `Stream` `:240`, `Subscribe` `:364`), `commands.rs`, `connect*`, `offline.rs`, `terminal.rs`, `tls_opts.rs`, `error.rs`.
- **tck**: `main.rs`, `runner.rs`, `equivalence.rs` (per-method table `:141-166`, params `:324`), `tests/{agent_card,errors,grpc,messaging,push_config,streaming,task_ops,wire_format}.rs`, `fixtures/grpc/corpus/*.json`, `scripts/{check_conformance,grpc_wire_compat}.py`; **sut**: `main.rs`, `serve.rs`, `telemetry.rs` (features grpc, websocket, otlp).

## B3. Request path per binding (V anchors)

Common tail for all bindings: `RequestHandler::on_*` → (send) admission → create/continue → executor spawn → event processing → store → event queue → binding's stream writer.

| Stage | JSON-RPC | HTTP+JSON (hyper) | HTTP+JSON (axum, feature `axum`) | gRPC (feature `grpc`) | WebSocket (feature `websocket`) | SLIMRPC (binding) |
|---|---|---|---|---|---|---|
| Entry / `Dispatcher` impl | `dispatch/jsonrpc/mod.rs:101 dispatch`, `:482 impl Dispatcher` | `dispatch/rest/mod.rs:106 dispatch`, `:281 dispatch_rest` | `dispatch/axum_adapter.rs:224 into_router`, `:304 impl Dispatcher for A2aRouter`, `:550 handle_tasks_catchall` | `dispatch/grpc/native.rs:195 impl A2aService`; dispatcher in `dispatch/grpc/{dispatcher.rs,serving.rs,admit.rs}` | `dispatch/websocket.rs:488 serve`, `:609 handle_connection`, `:739 read_loop`, `:1184 process_ws_message` | `bindings/…/server/methods/mod.rs:88 register_a2a_methods` |
| Method match | string `match`: streaming `:322,343`; unary `:404-461`; batch refuses streaming `:417-421` | path/verb routing `:281-510`; handlers `:516-759` (`handle_resubscribe :615`) | path tail match `:609-615`; inner handlers `:872-1067` (`handle_subscribe_inner :962`) | one `async fn` per RPC `:198-396` (`subscribe_to_task :289`) | string `match` `:1223,1275` + `dispatch_send_message :1388`, `dispatch_simple :1428` | closures per constant in `lib.rs:257-267` |
| Params | `dispatch/jsonrpc/params.rs` | `dispatch/rest/{params,query}.rs`, `media_type.rs` | `axum_adapter/validate.rs`, `unreadable.rs` | `From` proto (`types/src/proto/convert`), `admit.rs` | `dispatch/websocket/params.rs` | `server/methods/headers.rs`, `to_proto :408` |
| Handler call (subscribe) | `:347 on_subscribe_to_task` | `:636` | `:977` | `:299` | `:1278` | `:235-` |
| Streaming out | `build_sse_response` (`streaming/sse.rs:389`) at `jsonrpc/mod.rs:332,352` | same, `rest/mod.rs:539,639` | same via `hyper_to_axum`, `axum_adapter.rs:890,978` | `NativeStream` (`native.rs:287`), helpers in `grpc/helpers.rs` | `dispatch/websocket/stream.rs:21 stream_events` called at `websocket.rs:1286,1420` | `event_stream` `server/methods/mod.rs:455` |
| Errors | `jsonrpc/{response,refusal}.rs` | `rest/error_response.rs` | shared with rest | `grpc/details.rs`, `native.rs:51-66 Failure` | `websocket.rs:1477-1533` | `error.rs` |
| Combined listener | `dispatch/combined.rs:59 CombinedDispatcher` (JSON-RPC + REST + optional WS on one port, `:110 with_websocket`); common config `dispatch/mod.rs:63 DispatchConfig`; `cors.rs`, `hsts.rs`, `version.rs` (A2A-Version), `base_path.rs`, `well_known.rs` | | | | | |

Handler core (send path), in order:
1. `handler/messaging/mod.rs:80 on_send_message` → `:143 send_message_inner` (metrics, tenant resolution `handler/tenant_resolution.rs`, `build_call_context` in `handler/helpers.rs`, interceptor chain `interceptor/mod.rs`).
2. Validation/admission: `handler/messaging/{validation,admission,idempotency,claimed,quorum}.rs`; capability checks `handler/capability.rs`; steering short-cut `handler/messaging/steer.rs:57 asks_to_steer`, `:76 steer_if_running`.
3. Create/continue: `handler/messaging/create.rs:47 build_initial_task`, `:89 build_request_context`, `:151 persist_initial_task`, `:267 save_turn`; commit `commit.rs`, `commit_guard.rs`.
4. Executor: `handler/messaging/execute.rs:221 spawn_executor`, `:236 defer_executor`, `:279 executor_future` → `AgentExecutor::execute` (`executor.rs:88`) writing to an `EventQueueWriter` (`streaming/event_queue/mod.rs:193`); queue owned by `streaming/event_queue/manager.rs:78 EventQueueManager`.
5. Event processing: streaming/background — `handler/event_processing/background/mod.rs:56 spawn_background_event_processor` → `processor.rs:199 Processor::handle` → `state_machine.rs:138 process_event_bg` / `:152 process_event_with`; blocking — `handler/event_processing/sync_collector.rs:237 collect_events`, `:276 collect_then_follow`; shared `event_processing/mod.rs:87 persisting` (retry of store writes), `:177 append_ending`, `:238 merge_append`.
6. Store: `TaskStore::{save_status_delta, save_artifact_delta, save_appending_history, append_event}` (`store/task_store/mod.rs:614, 539, 729, 789`).
7. Respond: `handler/messaging/respond.rs:55 respond_waiting`, `:74 respond_replay`, `:127 respond_in_background`, `:202 respond_blocking`.
8. Push: `handler/event_processing/background/push_delivery/`, `push/outbox/`, `push/sender.rs`.

Other entry points: `handler/lifecycle/get_task.rs:25`, `list_tasks.rs:27`, `cancel_task.rs:28`, `subscribe.rs:646`, `extended_card.rs:30,53`; `handler/push_config.rs:205,287,367,439`; deprecated aliases `handler/renamed.rs:31,50`.

## B4. `handler/` internal structure (81 prod + 93 test files; V)

| Sub-module | prod f / L | test f / L | Role |
|---|---|---|---|
| `event_processing/` (`mod.rs`, `sync_collector.rs` 2,988, `stale_reads.rs`, `background/{mod,processor,state_machine 2,279,reserve_testing}.rs`, `background/processor/`, `background/push_delivery/`) | 15 / 9,091 | 25 / 6,393 | Folding executor events into task state and the log; the two consumers (background vs blocking) |
| `messaging/` (22 files: `admission, basis, claimed, closed_early, commit, commit_guard, continuation, create, decisions, drop_spawn, eviction, execute, idempotency, mode, quorum, respond, start, steer, terminal, validation`, `execute/`) | 22 / 5,205 | 22 / 8,550 (`tests.rs` alone 3,446) | `SendMessage`/`SendStreamingMessage` |
| `lifecycle/` (`get_task, list_tasks, cancel_task, subscribe` 1,4xx, `extended_card`, `subscribe/`) | 7 / 2,557 | 8 / 1,947 | The other task RPCs |
| `push_config.rs` (1,612), `push_config_page.rs` | 2 / 1,779 | — | Push-config RPCs |
| `claims/` (`mod, renewal, steering, reaper/…`) | 4 / 1,297 | 7 / 2,480 | Turn claims across replicas |
| `helpers.rs` (961), `limits/` (914), `capability.rs` (473), `mod.rs` (469: the struct and 13 test-module declarations), `halt.rs`+`halt/`, `lease.rs`+`lease/`, `shutdown/` | | | Cross-cutting |
| small single-purpose files: `ids, input_modes, concurrency, tenant_resolution, load, locks, budget, introspection, subscribers, review, turn, turn_slots, lineage, undeclared_binding, upgrade, unreadable, context_lookup_cost, renamed` | 44–234 each | | |

Observation (I): `RequestHandler` is one struct with `impl` blocks spread over ~40 files (`pub(super) fn` on `Self`); a new RPC adds a file under `lifecycle/` or a sibling directory plus a `mod` line in `handler/mod.rs:23-53`, and usually a field on the struct wired through `builder.rs`.

## B5. Stores, schema and migrations (V)

`TaskStore` (`store/task_store/mod.rs:113`) has 31 methods (listed with `grep -nE 'fn '` from line 113): core `save :130, get :140, get_with_position :166, list :185, insert_if_absent :198, delete :208, count :222, health :240`; capability probes `isolates_tenants :263, supports_idempotency :277, database :294, turn_claims :317, is_process_local :332, seals_content :346`; idempotency `:373, :410, :455`; deltas `save_artifact_delta :539, save_status_delta :614, try_save_* :651,:671, save_appending_history :729`; **event log** `supports_event_log :772, append_event :789, last_event_seq :817, read_events :840, earliest_event_seq :890, hold_events :906, watch_event_log :943, event_log_covers :966, trim_event_log :1006`. There is **no separate `EventLog` trait**: the event log is a set of defaulted `TaskStore` methods (ADR 0012).

Implementations of `TaskStore` in production code (9):

| Impl | Anchor | Event-log impl |
|---|---|---|
| `InMemoryTaskStore` | `store/task_store/in_memory/mod.rs:831` (file 3,277 lines) | `:909 append_event`, `:989 trim`, `:1022 read`; `in_memory/log_bound.rs` |
| `TenantAwareInMemoryTaskStore` | `store/tenant/store.rs:416` | `:606, :660, :674` |
| `SqliteTaskStore` (feature `sqlite`) | `store/sqlite_store/mod.rs:609` | `:1271, :1347, :1363`; SQL in `sqlite_store/event_log.rs:33-109` |
| `TenantAwareSqliteTaskStore` | `store/tenant_sqlite_store.rs:270` | `:686, :765, :782`; `store/tenant_event_log.rs` |
| `PostgresTaskStore` (feature `postgres`) | `store/postgres_store/store_impl.rs:37` | `:615, :675 watch, :711, :730`; SQL in `postgres_store/event_log.rs:28-78`; `store/log_notify.rs` (LISTEN/NOTIFY), `log_wake.rs` |
| `TenantAwarePostgresTaskStore` | `store/tenant_postgres_store/store_impl.rs:36` | `:358, :408, :437, :445`; `log_bounds.rs` |
| `TracedTaskStore` (wrapper) | `store/traced.rs:194` | `:319, :332, :348, :363` |
| `EncryptedTaskStore` (wrapper, feature `encryption`) | `store/encrypted/mod.rs:182` | `:347, :363, :387, :399` |
| `AuditedTaskStore` (wrapper, feature `audit`) | `audit/task_store.rs:106` | `:227, :244, :257, :269` |

Plus ≈30 test doubles implementing `TaskStore` inside `src` (V: `grep 'impl .*TaskStore for'`), e.g. `handler/messaging/tests.rs:3008`, `handler/event_processing/background/reserve_testing.rs:202`.

Shared SQL helpers: `store/event_log_sql.rs`, `store/tenant_event_log.rs`, `store/tenant_idempotency.rs`, `store/caller_keys.rs`, `store/cursor.rs`, `store/pagination.rs`, `store/pg_retry.rs`, `store/database.rs` (`StoreDatabase`), `store/applied.rs` (`AppliedPosition`).

Other store-backed entities and their traits: turn claims `TurnClaimStore` (`store/claim/mod.rs:140`; `claim/{sqlite,postgres,upgrade}.rs`); halts `HaltStore` (`store/halt/mod.rs:111`; in-memory, SQLite, PostgreSQL); leases `LeaseStore` (`store/lease/mod.rs:76`; in-memory, PostgreSQL); delegation journal (`store/delegation_journal/`; trait in types); retention (`store/retention/`); holds (`store/holds/`); terminal-state rule (`store/terminal.rs`); content cipher `ContentCipher` (`store/encrypted/cipher.rs:35`); push configs/outbox (in `push/`); audit (`audit/store.rs`); rate limit (`rate_limit/`); tool-loop journal (`tool_loop/journal/`).

**Schema mechanism — two paths that must agree:**
1. **Inline DDL at construction**: each store's `from_pool` runs `CREATE TABLE IF NOT EXISTS …` from a `pub const CREATE_TABLE_SQL` (e.g. `sqlite_store/event_log.rs:33`, `postgres_store/event_log.rs:28`). On PostgreSQL concurrent DDL is serialised by an advisory lock `SCHEMA_LOCK_KEY` (`store/pg_schema/mod.rs:38`) and helpers at `:125-163`.
2. **Versioned forward-only migrations**: SQLite `store/migration/mod.rs` — `Migration {version, description, sql}` (`:72`), `BUILTIN_MIGRATIONS` (`:86-158`, versions 1–9; v7 = `task_events`, v8 = `applied_seq`, v9 = `a2a_task_claims`), `MigrationRunner::{new :188, with_migrations :199, current_version :223, pending_migrations :238, run_pending :258}`, table `schema_versions`, each in `BEGIN EXCLUSIVE`. PostgreSQL `store/pg_migration.rs` — `PgMigration` (`:53`), `BUILTIN_PG_MIGRATIONS` (`:67-145`, versions 1–9), `PgMigrationRunner` (`:178-251`). The migration SQL **shares the constant** with `from_pool` (comments at `pg_migration.rs:130-150`), and a test fails if the doc table and the array disagree in length (`migration/mod.rs:31-33`). SQLite cannot `ADD CONSTRAINT`, so constraints reach only new databases (`migration/mod.rs:35-43`).
   Inventory of what each release changed: `docs/audits/2026-11-13-adversarial-review/migration-inventory.md`; upgrade rehearsal scripts `scripts/live/upgrade_live.py`; tests `crates/a2a-protocol-server/tests/turn_claims/upgrade.rs`, `store/pg_schema/tests/`.

## B6. Types generated from the proto (V)

- Source: `crates/a2a-protocol-types/proto/a2a_v1/a2a.proto` (811 lines; v1.0.1 pinned in `proto/UPSTREAM`), five more byte-identical copies (rule 95).
- Generator: `tools/a2a-codegen` (`main.rs:154-161`: `write | --check | dump`; `proto.rs` parser, `config.rs`, `emit.rs`, `oneof.rs`, `shadow.rs`, `text.rs`), driven by `crates/a2a-protocol-types/codegen.toml` (904 lines; `[paths]` maps each proto message/enum to a Rust path, e.g. `GetTaskRequest = "params::TaskQueryParams"`).
- Output: `crates/a2a-protocol-types/src/generated/*.rs` (10 files, 1,331 lines), `include!`d by hand-written modules.
- prost/tonic: `types/build.rs:7-42` compiles messages under feature `proto` with the vendored `protoc`; `server/build.rs:41-45` builds the tonic **server** stubs and `client/build.rs:39-43` the **client** stubs, both with `.extern_path(".lf.a2a.v1", "::a2a_protocol_types::proto")` so there is one set of message types.
- Drift checks: `a2a-codegen --check` (`ci.yml:99`), `check_proto_copies.sh` (`ci.yml:89`), `types/src/method.rs` test `all_matches_the_ratified_proto`, `check_method_denominator.py` (official-tck), `check_a2a_proto_upstream.sh` (official-tck; pre-releases excluded).

## B7. Where tests live (V counts: `grep -rE '^\s*#\[(tokio::)?test'`)

| Location | Count |
|---|---|
| In-`src` tests (inline `#[cfg(test)]` + 293 sibling `*_tests.rs`/`tests.rs` + 44 files under `src/**/tests/`) | types 663, client 1,004, **server 3,687**, discovery 12 |
| `crates/*/tests` integration | types 230 (25 files + 3 dirs), client 548 (79 + 8), server 1,672 (208 + 27), sdk 120 (25 + 4), discovery 5 |
| `#[ignore]` attributes | server 283 (PostgreSQL, live-model, soak, readiness), sdk 1 |
| SLIMRPC | src 75, tests 64 (SPIFFE suites `#[ignore]`d) |
| CLI 81 · TCK 33 · examples 197 · book-tests 5 (+ doctests from 169 included pages; book has 224 `.md` files) | |
| Fuzz | 18 targets in `fuzz/fuzz_targets/` |
| Formal | Kani: `types/src/kani_proofs.rs` (`verification.yml:kani`); loom: `verification/loom-halt/tests/halt_model.rs` (`verification.yml:loom`) |
| Benches | 15 criterion benches in `benches/benches/`, 1 each in types/client/server |
| Conformance / interop | `tck/` (own kit), upstream a2a-tck (`official-tck.yml`), ACTS (`acts/sut-behaviors.yaml`, `itk/src/acts*.rs`), ITK (`itk/`, 8 peer agents under `itk/agents/`, `itk/interop/*.py`), `scripts/go_sdk_interop.sh` |
| Example harness | `examples/harness/src/lib.rs` (`record :168`, `excuse :182`, `report :228`, `run_surface :353`) iterates `Method::ALL` × bindings; used by echo-agent, incident-response, genai, rig, multi-lang-team |
| Method-set completeness tests iterating `Method::ALL` | `crates/a2a-protocol-server/tests/{method_routing_completeness, auth_every_method, authorization_per_task}.rs`, `tests/handler_conformance/{auth_before_store, required_extensions}.rs`, `crates/a2a-protocol-sdk/tests/{operation_matrix, binding_equivalence}/` |
| Golden | `tests/golden/uri_contracts/` (types ×8, server ×1) |

## B8. Structural hot spots

**The 20 largest `.rs` files** (V: `wc -l | sort -rn`):

| Lines | File |
|---|---|
| 3,740 | `crates/a2a-protocol-server/tests/postgres_store_tests.rs` |
| 3,446 | `server/src/handler/messaging/tests.rs` |
| 3,322 | `server/src/dispatch/websocket.rs` (production to ~`:1550`, inline tests after) |
| 3,277 | `server/src/store/task_store/in_memory/mod.rs` |
| 3,155 | `server/src/streaming/event_queue/in_memory.rs` |
| 2,988 | `server/src/handler/event_processing/sync_collector.rs` |
| 2,699 | `server/src/builder.rs` |
| 2,595 | `server/src/auth/jwt.rs` |
| 2,567 | `client/src/transport/websocket.rs` |
| 2,279 | `server/src/handler/event_processing/background/state_machine.rs` |
| 2,097 | `client/src/discovery.rs` |
| 2,024 | `client/src/retry.rs` |
| 1,925 | `client/src/transport/grpc.rs` |
| 1,802 | `server/src/store/tenant_sqlite_store.rs` |
| 1,764 | `client/src/token_provider.rs` |
| 1,736 | `types/src/error.rs` |
| 1,696 | `server/src/store/tenant/store.rs` |
| 1,684 | `client/src/streaming/event_stream.rs` |
| 1,654 | `crates/a2a-protocol-server/tests/jsonrpc_dispatch_coverage_tests.rs` |
| 1,646 | `server/src/dispatch/axum_adapter.rs` |

All are in `.file-length-baseline` (the gate passes). Growing them is legal; touching them invites merge conflicts and long mutation lists. The stream/store contract lives in four of the top ten.

**Highest file counts**: `handler/` 174, `store/` 117, `tool_loop/` 60, `dispatch/` 55, `otel/` 47, `push/` 43, `audit/` 37 (V, module table).

**Module dependency shape** (I: regex over `crate::<mod>` in non-test files, text before the first `#[cfg(test)] mod tests`; numbers = referencing files): intended layering `dispatch → handler → {store, streaming, push}` holds by weight (handler→store 48, dispatch→handler 13), but there are back-edges: **store→handler 5, push→handler 5, streaming→handler 3, streaming→dispatch 1, metrics→handler 3, metrics→store 2, rpc_span→handler 4, rpc_span→dispatch 2, error→streaming 1, call_context↔auth, call_context↔request_context, builder↔handler/store/agent_card, tool_loop↔audit/rpc_span/skills, otel↔metrics/store**. Examples: `streaming/event_queue/in_memory.rs:1003,1307` uses `crate::handler::subscribers::SubscriberSlot`. Being one crate, these compile; they mean `store`, `streaming` and `push` cannot be extracted or reasoned about without `handler`.

**Duplicated code / the same logic four or five times**

1. `examples/{echo-agent,genai-agent,mcp-agent,multi-lang-team,rig-agent}/src/card_url.rs` — five byte-identical files (md5 `5c8823e4…`, 69 lines each, 345 total) (V).
2. **Method dispatch by string literal in every binding** — no shared routing table: JSON-RPC `jsonrpc/mod.rs:322-461`; WebSocket `websocket.rs:1223-1420` (re-implements the JSON-RPC match); REST `rest/mod.rs:281-759`; axum `axum_adapter.rs:550-1067` (re-implements REST routing and all nine handlers); gRPC `native.rs:198-396`; SLIMRPC `server/methods/mod.rs:88-388`. The `Method` enum exists (`types/src/method.rs`) but the dispatchers match on `&str`. Non-test files carrying the literal `"CancelTask"`: 17 (V list in the audit transcript: 5 server dispatch files, `rpc_span/names.rs`, 7 client files, slimrpc `lib.rs`, `tck/src/equivalence.rs`, `method.rs`, `cancel_task.rs`).
3. **REST twice**: `dispatch/rest/` (3,105 lines) and `dispatch/axum_adapter.rs` (1,646 + 199) are parallel implementations of the same HTTP+JSON surface; an old mutant note at `axum_adapter.rs:1101` shows they are tested separately.
4. **Six `TaskStore` stores + three wrappers**, each re-implementing ~31 methods; tenant-aware variants are separate types rather than a parameter (`tenant_sqlite_store.rs` 1,802 vs `sqlite_store/mod.rs` 1,449); the push-config store has the same ×5 pattern (`push/{config_store,sqlite_config_store,postgres_config_store,tenant_sqlite_config_store,tenant_postgres_config_store}.rs`).
5. **Outbound HTTP ×2** (client and `server/src/net`), held equal by `check_net_copies.py`.
6. **Method → span-name table ×2**: `server/src/rpc_span/names.rs:122-135` and `client/src/call_span.rs:322-332`.
7. **Client streaming detection ×3 in test servers**: `client/src/testing/{http1.rs:99, ws.rs:34, grpc.rs:67}`.
8. **Proto ×6 copies**; **build.rs ×3** with the same `set_var("PROTOC")` block.
9. Private handler names lag the naming rule: `handle_resubscribe`, `handle_set_push_config` (`rest/mod.rs:615,649`).

## B9. Touch-point checklist A — a NEW WIRE RPC (traced from `SubscribeToTask`)

Every site below names `SubscribeToTask`/`subscribe_to_task` today (V by grep) unless marked I. For a **bidirectional** RPC (e.g. v1.1.0-rc `SendLiveMessage` with `StreamRequest`/`StreamError`), the rows marked ⚠ have no existing pattern: every current RPC is unary or server-streaming.

**Spec, proto, types**
1. `crates/a2a-protocol-types/proto/a2a_v1/a2a.proto` + the 5 copies (`crates/a2a-protocol-client/proto`, `crates/a2a-protocol-server/proto`, `tck/proto`, `proto`, `docs/implementation/a2a.proto`) + `proto/UPSTREAM` (tag, commit, sha256) — `scripts/check_proto_copies.sh:28-41,90-96`. `check_a2a_proto_upstream.sh` excludes pre-releases, so tracking an `-rc` needs a decision on that script.
2. `docs/implementation/a2a-v1.0.1-specification.md` (the vendored text the ledger cites): a new spec version needs a new vendored text, `spec` / `audited_commit` in `spec-ledger.json` and a re-audit of every keyword line (`check_spec_ledger.py` rules 1–2); additions can sit under `after_release` (rule 5, `check_spec_ledger.py:42-44`).
3. `crates/a2a-protocol-types/codegen.toml` — a `[paths]` entry for every new message/enum (the generator refuses an unmentioned proto type), then `cargo run --manifest-path tools/a2a-codegen/Cargo.toml -- write` → `src/generated/{params,responses,events}.rs`.
4. `tools/a2a-codegen/src/{proto,emit,oneof}.rs` — ⚠ I: check the parser handles `stream` on the request side and any new proto construct (`oneof` in a request stream).
5. `crates/a2a-protocol-types/src/method.rs`: variant `:64-87`, `ALL :99-111`, `wire_name :115-129`, `is_streaming :145-147`, the second name table `:226`, tests `:381,419`. ⚠ `is_streaming` is a bool; a bidi method needs a third kind and every caller of `is_streaming` reviewed.
6. `crates/a2a-protocol-types/src/params/mod.rs` (doc table `:18`, `AcceptedFields :204`), `responses.rs`, `events.rs`, `wire_builders_requests.rs` (+ tests), `lib.rs` re-exports.
7. `crates/a2a-protocol-types/src/proto/convert/{requests,messaging}.rs` — proto ↔ domain for the new request/response/stream item.
8. `crates/a2a-protocol-types/src/jsonrpc.rs` / `error.rs` if a new error (`StreamError`) or code.
9. Types tests: `tests/tck_wire_format.rs`, `corpus_json.rs`, `proptest_types.rs`, `optional_wire_forms.rs`; fuzz `fuzz/fuzz_targets/{json_deser,jsonrpc_envelope,proto_convert}.rs` (+ `fuzz/Cargo.toml`, `fuzz.yml` matrix if a new target — gate `check_fuzz_matrix.py`).

**Server**
10. Handler entry: new file under `server/src/handler/lifecycle/` (model `subscribe.rs:646 on_subscribe_to_task`: metrics `:655,723-728`, tenant `:658`, call context `:667`, capability check `:676`) named `on_<wire_method_snake>`; `mod` line in `handler/mod.rs:23-53`; struct fields in `handler/mod.rs:119` wired through `builder.rs`/`builder/`.
11. `server/src/handler/capability.rs` (capability gating per method, `:14`), `handler/undeclared_binding.rs`, `handler/subscribers.rs` (stream slots/limits `:63-71`), `handler/limits/` (new limits ⇒ `HandlerLimits` + `with_*` + `check_inert_bounds.py`), `handler/shutdown/` (in-flight accounting), `handler/load.rs`.
12. JSON-RPC: `dispatch/jsonrpc/mod.rs` streaming arm `:322-352`, unary arm `:404-461`, batch refusal `:417-421`; params `jsonrpc/params.rs`. ⚠ HTTP request/response cannot carry a client stream: bidi needs a decision per binding (not offered, or mapped onto WebSocket).
13. REST (hyper): route in `dispatch/rest/mod.rs:335-365`, handler fn `:615-647`, params `rest/params.rs`/`query.rs`, `media_type.rs:237`, span `rest/span.rs`.
14. REST (axum): `dispatch/axum_adapter.rs` route `:609-615`, inner handler `:962-988`, catch-all tests `:1183-1210`.
15. gRPC: `dispatch/grpc/native.rs` — associated stream type `:287`, `async fn :289-308` (`admit`, `rpc_span`, handler call), `Failure` mapping `:51-66`; `grpc/admit.rs`, `grpc/helpers.rs`, `grpc/details.rs`. ⚠ a bidi RPC takes `Request<Streaming<…>>`; `admit` (`:294`) assumes one request message.
16. WebSocket: `dispatch/websocket.rs` — streaming arm `:1223`, method arm `:1275-1286`, `dispatch_simple :1428`; `dispatch/websocket/{params,stream}.rs`; the binding's golden contract `crates/a2a-protocol-server/tests/golden/uri_contracts/websocket.json` and `tests/uri_contract_websocket.rs` — a new frame type means re-bless or a new `WEBSOCKET_BINDING_URI` (rule 80). ⚠ the natural home for client→server frames on an open stream; `read_loop :739` currently treats each text frame as a new request.
17. `dispatch/combined.rs`, `dispatch/version.rs`, `dispatch/cors.rs` if new headers/paths; `serve/` if connection lifetime rules change (idle timeouts count bytes — `handoff.md:1392-1394`).
18. Streaming plumbing: `streaming/sse.rs:389 build_sse_response` (frame synthesis `:84`), `streaming/event_queue/in_memory.rs` (`:1003,1307,1554`), `manager.rs`; ⚠ an inbound half needs a new reader abstraction — the steering `Inbox` (`server/src/steering/inbox.rs`, `handler/messaging/steer.rs:76-255`, `request_context.rs`) is the existing way a running executor receives further input, and is the closest precedent.
19. Telemetry: `rpc_span/names.rs:122-135`, `rpc_span/` stream end (`StreamEnd`, `StreamOutcome`), `metrics/labels.rs`, `Metrics` callbacks (`metrics/mod.rs:52`) + `otel/` (`check_otel_metrics_coverage.py`) + `metrics/tee.rs`.
20. Interceptors/auth/rate-limit/audit operate on the method name string: `interceptor/mod.rs:117` (docs enumerate streaming methods), `audit/` call records, `auth/`, `rate_limit/` — covered by the `Method::ALL` tests in row 30.
21. `server/src/lib.rs` re-exports; `handler/renamed.rs` only if replacing a name.

**Client**
22. `client/src/methods/<new>.rs` (model `tasks.rs:131-200`; `const METHOD :194`), `methods/mod.rs:142-157`; `client/src/lib.rs` exports.
23. `Transport` trait `client/src/transport/mod.rs:313-360` — ⚠ only `send_request`, `send_streaming_request`, `send_request_with_query`; a bidi call needs a new (defaulted — rule 76) method and an implementation in each of `transport/jsonrpc.rs`, `transport/rest/{routing.rs:36-70, request.rs:520}`, `transport/grpc.rs:764-930` (match arm `:925`), `transport/websocket.rs`, plus wrappers that implement `Transport` (retry `retry.rs`, `in_flight`, balance/pool — I: grep `impl Transport for`).
24. `client/src/retry.rs:256,304` (per-method retry/idempotence class), `client/src/call_span.rs:322-332`, `call_metric.rs`, `streaming/event_stream.rs` (`EventStream`), `delegation/` if delegation should use it, `client/src/testing/{http1.rs:99-101, ws.rs:34, grpc.rs:67}`.
25. `crates/a2a-protocol-sdk/src/lib.rs` prelude and `tests/reexport_tests.rs`.

**Out-of-workspace and tools**
26. SLIMRPC: `bindings/a2a-protocol-slimrpc/src/lib.rs:257-267` constant, `server/methods/mod.rs:88-388` registration, `client/dispatch.rs`, tests; its lockfile; `spec/slimrpc_v1/` if the SLIMRPC spec defines the mapping.
27. CLI: `tools/a2a-cli/src/cli.rs` (`Subscribe :362-364`), `commands.rs`, README, tests.
28. `tck/`: `src/equivalence.rs:141-166,324`, `src/tests/*.rs`, `fixtures/grpc/corpus/<new>_request.json`, `tck/sut/src/serve.rs`, `tck/untested-musts.toml`, `tck/conformance-baseline.json`; `itk/src/{main,serve,acts}.rs`, `itk/interop/*.py`, `itk/interop/go-sdk-client`; `acts/sut-behaviors.yaml`.
29. `examples/harness/src/lib.rs` (records `Method` × `Binding`; an un-exercised pair must be `excuse`d with a reason — `:182`), then each example that runs the surface (`echo-agent/src/main.rs`, `agent-team`, `incident-response`, `genai-agent`, `rig-agent`, `multi-lang-team`, `resilient-agent`), `examples/harness/src/bin/go_sdk_interop.rs`, `examples/deploy-agent/network.yaml`.

**Tests that enumerate the method set (will fail or need extending)**
30. `crates/a2a-protocol-server/tests/method_routing_completeness.rs:187-247`, `auth_every_method.rs`, `authorization_per_task.rs`, `tests/handler_conformance/{auth_before_store,required_extensions,stream_and_push}.rs`, `jsonrpc_dispatch_coverage_tests.rs`, `swarm_scale/main.rs`; `crates/a2a-protocol-sdk/tests/operation_matrix/main.rs`, `binding_equivalence/functionality.rs`; server `rpc_span/tests.rs:18`, client `call_span/tests.rs:23`; benches `benches/benches/{production_scenarios,advanced_scenarios}.rs`.

**Docs, ledgers, ops**
31. Book (44 pages mention the method today): `book/src/reference/{api-reference.md, configuration.md, conformance-statement.md, upgrading.md, pitfalls.md, adrs.md}`, `book/src/concepts/*`, `book/src/building-agents/*`, `book/src/client/*`, `book/src/bindings/*`, `book/src/deployment/*`, `book/src/SUMMARY.md` (+ sitemap, + `book-tests/src/lib.rs` include for a new page), `book/src/images/*` diagrams (`scripts/render_book_diagrams.sh`).
32. READMEs: root `README.md` (method/binding tables; TCK sentence + badge), `crates/a2a-protocol-client/README.md`, `tools/a2a-cli/README.md`, `examples/*/README.md`, `itk/README.md`.
33. `docs/compliance/spec-ledger.json` rows for each new requirement + `--write`; `controls.toml` if a control cites it; `docs/official-tck-findings.md`.
34. ADR `docs/adr/0058-…md` + `book/src/reference/adrs.md`; `CHANGELOG.md` (`### Added`; `### Breaking Changes` if `Method`'s public API changes shape — the enum is `#[non_exhaustive]`, so a new variant is additive); `STABILITY.md` if the trait surface grows; `ROADMAP.md`.
35. Ops: `deploy/observability/prometheus/rules/a2a-recording.yml`, `deploy/kubernetes/observability/prometheusrule.yaml` (both enumerate methods), dashboards under `deploy/observability/`.
36. CI: a new feature flag for an unratified RPC (recommended while it is an `-rc`; see co-update set v) ⇒ clippy/test steps + `prove_gates_fail.sh` injections; `check_method_denominator.py` will fail in `official-tck.yml` as soon as `method.rs` names a method that upstream a2a-tck `main` or the vendored proto does not — the three sources must agree, so **the proto bump and `method.rs` change cannot land separately, and cannot land ahead of upstream TCK without changing that gate**.

## B10. Touch-point checklist B — a NEW STORE-BACKED ENTITY (traced from the event log; e.g. a task timeline with `generation` counters)

**Decide first** (ADR): new defaulted methods on `TaskStore` (the event-log pattern, ADR 0012/0057) or a new trait with its own stores (the `HaltStore`/`LeaseStore`/`TurnClaimStore` pattern). The first multiplies by 9 implementations; the second needs builder wiring and a capability probe.

**Types**
1. Wire/domain types in `crates/a2a-protocol-types` (generated if they are proto messages: rows 1–3, 6–7 of B9; `TimelineEntry`, `timeline_length`, `generation` would be proto fields on existing messages ⇒ `codegen.toml` field entries, and `history_length`/`history` deprecation ⇒ rule 75 and spec-ledger entry at spec line 115 about deprecated names).

**Trait**
2. `server/src/store/task_store/mod.rs` — defaulted methods beside `supports_event_log :772 … trim_event_log :1006`, each with the "what the default costs" paragraph (rule 76); a `supports_<entity>()` probe; records in `store/task_store/records.rs`; config in `task_store/config.rs` (`TaskStoreConfig` + `with_*`).
3. Re-exports `server/src/store/mod.rs:71-94,154-175`, `server/src/lib.rs:248-268`.

**Every implementation, same commit**
4. `store/task_store/in_memory/mod.rs:831` (+ a sibling file like `log_bound.rs`; eviction in `in_memory/eviction/`, erase in `erase.rs`, listing in `listing.rs`).
5. `store/tenant/store.rs:416`.
6. `store/sqlite_store/mod.rs:609` + new `sqlite_store/<entity>.rs` with `CREATE_TABLE_SQL` and statement constants (model `event_log.rs:33-109`); `from_pool` DDL.
7. `store/tenant_sqlite_store.rs:270` (+ `tenant_sqlite_store/`), shared SQL in a `store/tenant_<entity>.rs` (model `tenant_event_log.rs`).
8. `store/postgres_store/store_impl.rs:37` + `postgres_store/<entity>.rs` (model `event_log.rs:28-78`); every statement inside `pg_retry` (`store/pg_retry.rs`; gate `check_pg_retry.py`); `from_pool` DDL under the schema lock (`store/pg_schema/mod.rs:38,125-163`).
9. `store/tenant_postgres_store/store_impl.rs:36` (+ a file like `log_bounds.rs`).
10. Wrappers: `store/traced.rs:194` (span per call), `store/encrypted/mod.rs:182` (seal/unseal content — `seals_content`), `audit/task_store.rs:106` (audit record) — and raise the thresholds in `store/traced/tests.rs:37`, `store/encrypted/tests.rs:449`, `audit/tests.rs:358`.
11. Test doubles that implement `TaskStore` by hand (≈30 in `src`, more in `tests/`) keep compiling only because methods are defaulted — a required method would break all of them.

**Schema**
12. SQLite migration: a new entry in `BUILTIN_MIGRATIONS` (`store/migration/mod.rs:86`, next version **10**) sharing the DDL constant, plus the doc table row `:21-29` (a test compares the counts).
13. PostgreSQL migration: `BUILTIN_PG_MIGRATIONS` (`store/pg_migration.rs:67`, next version **10**), sharing the constant with `from_pool`.
14. Upgrade safety across replicas of mixed versions: `store/claim/upgrade.rs` (triggers keeping earlier releases off claimed tasks) is the precedent; tests `crates/a2a-protocol-server/tests/turn_claims/upgrade.rs`, `store/pg_schema/tests/search_path.rs`.
15. `docs/audits/2026-11-13-adversarial-review/migration-inventory.md` (the per-release schema inventory, register row N14) and `scripts/live/upgrade_live.py`.

**Lifecycle of the rows**
16. Retention/purge: `store/retention/mod.rs` (+ `retention/tests/parked/{sqlite,postgres}.rs`), `PurgeReport` field (it is `#[non_exhaustive]`), orphan sweep (model `DELETE_ORPHANS_SQL`, `sqlite_store/event_log.rs:108`), task delete (`DELETE_FOR_TASK_SQL :66`), tenant erase (`in_memory/erase.rs`, `caller_keys.rs`), bounds (`in_memory/log_bound.rs`, `tenant_postgres_store/log_bounds.rs`) ⇒ `check_inert_bounds.py`.
17. Fencing/consistency: writes made by a turn are fenced by the claim (`store/claim/`), stamped with `applied_seq` (`store/applied.rs`; migration 8/7), and refused after a terminal state (`store/terminal.rs`); delta forwarding to wrappers (`check_store_delta_forwarding.py`).
18. Multi-replica wake-up if readers wait on it: `store/log_notify.rs`, `store/log_wake.rs`, `watch_event_log` (ADR 0057).

**Writers and readers**
19. Writer: `handler/event_processing/mod.rs:87 persisting`, `:177 append_ending`; `background/state_machine.rs:138-152`; `sync_collector.rs:237-364`; the fold `streaming/event_queue/message_fold.rs`, `applied.rs`, `status_stamp.rs`, `terminal_gate.rs`.
20. Readers: `handler/lifecycle/get_task.rs:25` (history hydration: `handler/messaging/respond.rs:37 hydrate_response_history`, `store/task_store/history.rs`), `list_tasks.rs:27`, `subscribe.rs` (replay from the log, reattach hook `:111`), `streaming/event_queue/in_memory.rs:1554` (snapshot then replay), `handler/messaging/respond.rs:74 respond_replay`.
21. Builder/profile: `builder.rs`, `builder/`, deployment profiles `profile.rs`, `profile_report.rs` (durable-production checks: `is_process_local`, `supports_event_log`), `conformance/`.
22. Metrics/telemetry: `Metrics` callbacks (store-write, persistence error), `otel/`, `store/traced.rs`; `StoreDatabase` (`store/database.rs`).

**Tests**
23. Per-store unit tests (model: `sqlite_store/event_log_tests.rs`, `task_store/in_memory/event_log_tests.rs`, `store/tenant/event_log_tests.rs`), `crates/a2a-protocol-server/tests/postgres_store_tests.rs` (`#[ignore]`, `A2A_TEST_POSTGRES_URL`; discoverable by `run_postgres_suites.py --check`), `multi_replica`, `stale_write_position`, `encrypted_store`, `soak_multi_replica.rs`, `tests/handler_conformance/stream_and_push.rs`; statement-count budget `statement_count` constants if the hot path gains a statement (`handoff.md:76-79`).
24. Benches `benches/benches/{data_volume,task_lifecycle}.rs`; `check_streaming_linearity.py` (benchmarks workflow) if per-event cost changes; `Regression Gate`.

**Docs, ledgers, examples**
25. `book/src/deployment/horizontal-scaling.md`, `horizontal-scaling/shared-stores.md`, `horizontal-scaling/streaming.md` (all name `task_events`), store pages under `book/src/building-agents/`, `book/src/reference/{configuration,api-reference,upgrading,pitfalls}.md`; `examples/deploy-agent/src/retention.rs` + README; `examples/resilient-agent`.
26. ADR (0058+), `book/src/reference/adrs.md`, `CHANGELOG.md` (`### Added` + `### Behaviour Changes` for schema; `### Upgrading` for the migration), spec-ledger rows, `controls.toml` (retention/erasure controls cite store tests), `STABILITY.md` §4 if a new trait.
27. Live scripts that read the tables directly: `scripts/live/{common.py, finish_live.py, upgrade_live.py}`, `scripts/live/campaign/hostile/chaoslib.py`.

---

## Appendix — commands run for this audit

`git log --oneline | head`; `wc -l` on the rule documents; `python3` + `yaml.safe_load` over `.github/workflows/*.yml` (jobs, steps, matrices, triggers); `bash scripts/preflight.sh --list`; `python3 scripts/preflight_all.py --list`; `python3 scripts/run_workflow_job.py --help` (refused); `python3 scripts/check_spec_ledger.py`; `check_rfc_ledger.py`; `check_compliance_map.py --check`; `./scripts/check_file_lengths.sh`; `python3 scripts/check_panic_paths.py`; `find`/`wc`/`grep`/`md5sum` as cited. No cargo command was run.
