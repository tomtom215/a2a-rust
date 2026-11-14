<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Handoff — session state

Working state between sessions: which branches exist and why, what is in flight
outside this repository, and what the next session should pick up.

This is **not** `ROADMAP.md`. That file takes only items the repository has
committed to and refuses speculative milestones; this one records where things
stand, including decisions to *not* do something. When an item here becomes work
the repository commits to, move it there and delete it here.

It is hand-maintained; `git log -1 --format='%ci %s' -- docs/handoff.md` says
when it last changed. Everything before 2026-10-15 — each branch's narrative,
its item list, its verification of record — is kept verbatim in
[`handoff-history.md`](handoff-history.md). This file is the current state
only: when something here is done, delete it rather than annotating it.

## Now

State on 2026-11-13 (UTC). Each figure was read from a command in the session that wrote it, and the command is named so the next session re-runs
it instead of copying the number. The previous version of this block (pull request #209 and the 2026-11-12 review) is kept verbatim in
[`handoff-history.md`](handoff-history.md), last section.

- **Where the code is.** `main` is `70dc3374` (`git rev-parse origin/main`), the merge of pull request #209; the workspace reads 0.16.0 and **nothing is
  tagged** (`git tag --sort=-creatordate | head -1` prints `v0.15.0`). **This session's branch** is `claude/dazzling-wozniak-xp79ii`, ahead of `main`
  (`git log --oneline 70dc3374..HEAD`), pushed, **with no pull request** (none was asked for). Commits are authored as Tom F. with one `Signed-off-by` and no
  assistant trailer (`CONTRIBUTING.md`, `dco.yml`).
- **Why this session.** An independent adversarial review of `70dc337` (a 27-page PDF) added sixteen findings, `N01` to `N16`, reported `main`'s CI red on macOS,
  and corrected stale claims in this file. Its register is `docs/audits/2026-11-13-adversarial-review/` (`findings.json`, 26 rows: 23 FIXED, 2 PARTIAL (N14, E1),
  1 OPEN (CI1); the README has the command that counts them). The 2026-11-12 register was updated: S8 fixed, S1 and S6 notes. Read both before acting.
- **Built, each behaviour defect first reproduced by a test that failed on the code as it stood** (the evidence is in each commit message and register row):
  `861187ae` N01 (approval identities sealed; one field classification for sealing, reveal and erasure scans), `dd9502ad` N02 (`ToolPolicy::approved`; an approved
  call is recorded durably before it runs), `aa26b2b0` N03 and S8 (key cleanup and erasure fenced against other replicas in every shipped store),
  `21d03e00` N04 and N07 (a run's end is a fenced, retained, completed checkpoint; journal entries owned, sealable, sweepable and erasable), `8369649f` N05 and
  N06 (a claim is kept until a task's ending is durable; `record_now` says how it recorded), `36d894cd` N10 to N12 (client: extended card verified, only `1.0`
  interfaces selected, opt-in strict tenant), `ac63a18f` N08 and N09 (call ids unique per run; byte limits on results, requests and checkpoints), `a0d6300e`
  N16 (per-model `Connection: close`), `01d28c48` N15 (the incremental mutation gate fails on a timeout), `3f6be5f7` N13 (a release requires `ci.yml`'s green
  run of the tagged commit and writes and verifies the evidence manifest), `b04d6992` N14 (`migration-inventory.md`).
- **Found by the real-model tests and fixed, `1591c8b6` (D1):** on a single-worker runtime a blocking send could hang for the executor timeout (an hour),
  because the collecting task stopped polling the executor it runs inline while it folded an event; with `AuditedPolicy` the executor held the audit chain lock
  the fold needed. Present since 2026-10-03 and in 0.15.0. Reproduced by `inline_executor_tests::an_executor_holding_what_a_fold_needs_is_still_polled` (no
  answer in 10 s before the fix); live: 5 hangs in 11 runs of the file before, 0 in 8 after.
- **Real models.** llama.cpp `9daa0aa` (`/opt/llama/src/build/bin/llama-server`), Qwen3-1.7B Q4_K_M (`/opt/models/Qwen3-1.7B-Q4_K_M.gguf`, SHA-256 equal to the
  Hugging Face `lfs.oid`), served with `--host 127.0.0.1 --port 11434 --alias qwen3:1.7b --jinja -c 4096 -np 1 -t 4`. Five live files, all `#[ignore]`d without
  a server and excluded from the mutation runs: `tool_loop_strict_live`, `terminal_truth_live`, `tool_loop_journal_live` (3 tests each, 9 of 9 passed),
  `tool_loop_review_live` (2) and `tool_loop_audit_live` (1), run with `--features "tool-loop sqlite audit encryption" -- --ignored --test-threads=1`.
  Measured: llama-server closes the connection after a streamed answer (3 of 3, N16).
- **Verified** (re-run each; the tree is this branch's head unless a commit is named):
  `A2A_TEST_POSTGRES_URL=postgres://tester:tester@localhost:5432/postgres cargo test --workspace --all-features --no-fail-fast` with a live PostgreSQL 16:
  **8,491 passed, 0 failed, 275 ignored** over 420 result lines (summed from the `test result:` lines), at `b04d6992`; the ignored are the live-model, soak,
  readiness and PostgreSQL-only tests. The server library with `--include-ignored` (PostgreSQL on): **3,659 passed, 0 failed**, on the tree that became
  `1591c8b6` before its `closing` refactor; after it, the collector's and inline-executor tests (53) passed. `cargo clippy --workspace --all-targets
  --all-features -- -D warnings`: exit 0 (Cargo 1.100's two `unused dependency` warnings for `book-tests`, under *Facts*). `scripts/run_workflow_job.py
  .github/workflows/ci.yml static-checks`: **PASS, 47 steps run, 0 failed, 0 not run** (the book built and its links resolved); the same for `doc`: PASS after
  this session's fixes of five broken doc links (three from `ac63a18f`, two feature-gated ones from `36d894cd`), which the per-crate default-feature build
  found and the all-features build did not. **`ci.yml`'s jobs run locally with `scripts/run_workflow_job.py`** at `c47b818b` or its content: `static-checks` PASS (47 steps), `doc` PASS (2), `clippy` on `os=ubuntu-latest` PASS (14 feature sets), `features` PASS (cargo-hack, each feature of the four published crates), `package` PASS, `test-postgres` with `--services-ready` against the local PostgreSQL PASS (6 steps) after `c47b818b` fixed a race it found (D3; `statement_count` also needs `ssl = off` locally, under *Facts*). **Not run locally:** the `test` job's nextest legs per feature set (each a build of its own; this disk allowance holds about one; their compilation is covered by `clippy` and `features`, their runtime only by the all-features run above), and the Windows, macOS and MSRV legs. Live: the five live files as listed above. `scripts/ci_evidence.py --self-test` 13 of 13;
  `scripts/release_evidence.py --self-test` 15; `scripts/prove_workflow_gates_fail.py` 34 proven, 0 unproven at `3f6be5f7`.
- **Corrections to this file the review made, each checked here.** *Advisory waivers:* `deny.toml` has one, RUSTSEC-2026-0459 (`libsqlite3-sys`, since
  2026-11-11), so "no waivers anywhere" below was stale and is corrected there. *`unsafe`:* the five published crates' library sources carry
  `#![forbid(unsafe_code)]`; outside them 18 files hold `unsafe` (`git grep -lE '\bunsafe\b *(\{|fn|impl)' -- '*.rs'`, less `fuzz/`): `set_var("PROTOC")` in five
  build scripts, three counting `GlobalAlloc` impls (two benchmarks, one test) and environment changes in ten test files. Each has a SAFETY note;
  the two benchmark allocators' were added this session. *Spec ledger:* 510 rows for v1.0.1 = 455 TESTED + 32 APPLICATION + 19 NOT_APPLICABLE + 4 PARTIAL
  (`python3 scripts/check_spec_ledger.py`). *TCK and ACTS:* the official TCK at its pin `70f6f73` was **run here** (`scripts/run_workflow_job.py .github/workflows/official-tck.yml official-tck`,
  every step passed): full profile 240 passed, 4 failed (all baselined: CORE-SEND-003 on three bindings, HTTP_JSON-ERR-001), 19 skipped, 87 MUSTs graded
  (floor 87), MUST classification 85 pass, 2 fail, 5 skipped, 7 manual, 16 not tested; minimal profile 175 passed, 4 failed, 84 skipped, 65 graded (floor 65);
  required extension 2 passed. ACTS passed on `main` at `70dc337` (run 43394485553); its per-binding counts are the review's, not re-run here. Both are now
  in `book/src/reference/conformance-statement.md`. *Branch protection:* `main` is unprotected (G1, the maintainer's), as *Waiting on the maintainer* already said.
- **Not verified / not done:** the workflows on GitHub for any commit of this branch (no pull request, so no CI run); `CI1`, the macOS mDNS failures
  (diagnosed, not root-caused: each process saw the other's announcement and not its own; Linux 0 of 160 concurrent runs failed); the N13 and N15 workflow
  changes beyond `prove_workflow_gates_fail.py` and synthetic inputs; N14 on PostgreSQL; any two-replica run outside the tests that pause one log at the race.
  Of the review's enterprise asks: the conformance statement is written (`book/src/reference/conformance-statement.md`); named deployment
  profiles are done but for the step journal (E1, PARTIAL: `a76a5957` `LocalDevelopment`, `DurableProduction` and `profile_report()`; `21ab2e93`
  `RegulatedData`, which checks audit privacy, egress and sealing at rest and requires declarations of proxies, key custody, backups and
  retention; a journal is set on the `ToolLoop`, which the builder never sees); a support matrix (E2), operations runbooks (E3) and curated
  release notes (E4: `### Upgrading` and `### Highlights` lead `[Unreleased]`; the release page keeps both, D5) are written, in `21ab2e93`.
  Writing them found D4 (a rotated checkpoint key made `AuditLog::verify` report an intact chain broken) and D5, both fixed there.
  The **qualification matrix** (replica kill, store faults, provider faults) is **not started**; see *First, next*.
  At `21ab2e93`: `ci.yml` `clippy`, `features`, `doc` and `static-checks` PASS locally (`scripts/run_workflow_job.py`); the server library with
  PostgreSQL and `--include-ignored`: **3,686 passed, 0 failed**.

### First, next

1. **Open a pull request** for this branch when the maintainer asks, label it `mutation-test`, and read every shard's log (the aggregate now fails on a
   timeout, N15). The Windows and macOS legs decide CI1.
2. **CI1, the macOS mDNS failures:** reproduce on a macOS runner with `workflow_dispatch`; the conjecture to test is that a `ServiceDaemon` does not deliver
   its own process's announcement to a browser `ServiceDaemon` in the same process on that runner.
3. **The review's last release criterion** (PDF section 7): the qualification matrix (replica kill, store faults, provider faults), each row a
   run with its evidence. E1's rest is a decision first: whether the step journal's durability should be checked, which needs the builder to see
   the `ToolLoop`'s journal.
4. **N14's rest:** a PostgreSQL 0.15 to 0.16 rehearsal of the audit and push-config stores, and a PostgreSQL rollback. Tasks across a rolling
   two-version fleet were already measured against a real 0.15.0 replica on 2026-10-28 (`book/src/deployment/horizontal-scaling.md`,
   *Upgrading a fleet*; `tests/turn_claims/upgrade.rs`), which this file's earlier wording missed.
5. **Older register rows still open:** S2 skill pinning, S3 fleet quota (a decision first), S4 telemetry-failure visibility; partial R3, A3, S1, S5.
6. The maintainer's items stay under *Waiting on the maintainer*, G1 among them.

### Facts this session established

- **A container restart stops PostgreSQL and llama-server** (seen 2026-11-14: 80 library tests failed with `PoolTimedOut` and connection
  refused, and nothing else). Before reading a red PostgreSQL run, `pg_isready -h localhost`; restart with `pg_ctlcluster 16 main start`
  (`ssl = off` and both roles survive, being on disk), and llama-server with the command under *Now*.
- **Added 2026-11-13.** `rustup update stable` gave 1.100.0; CI is 1.99.1, so an `#[allow]` of a lint 1.99 does not know fails CI
  (`unknown_lints`): avoid the lint (as `unused_async_trait_impl` was avoided by returning boxed futures) rather than allowing it. Cargo 1.100 also
  warns `unused dependency` for `book-tests`' `a2a-protocol-sdk` and `a2a-protocol-discovery`, which only the book's `include_str!` doc-tests use
  (93 pages and `local-discovery.md`): a false positive, left alone because a `[lints.cargo]` entry naming the lint would itself warn on 1.99.
- The rustdoc JSON that `cargo-semver-checks` leaves under `target/semver-checks` documents **private items too**: `cargo public-api diff` over those
  files reported 713 changed lines, most crate-private. For a public diff build the JSON yourself:
  `RUSTC_BOOTSTRAP=1 cargo rustdoc -p <crate> --all-features --lib -- -Z unstable-options --output-format json` (in a `v0.15.0` worktree for the
  baseline), then `cargo public-api -sss diff old.json new.json`. `-sss` hides derived impls, so a hand-written `impl Default` that became
  `#[derive(Default)]` shows as removed: check the source before calling it a break.
- **A hang that only shows with earlier tests in the same process** (D1) is still a product bug. What found it: a 300 s bound on each send (so a hang
  fails with a message instead of waiting out the executor's hour), `eprintln!` markers whose heavier version (a backtrace on every lock) hid it, and a
  backtrace captured **only when a lock is contended** (`try_lock_owned` first), which named both sides. `gdb -p PID -batch -ex "thread apply all bt"`
  showed the runtime parked in `epoll_wait` with one distant timer: a future nobody polls, not a busy loop.
- The `v0.15.0` tag is not in a fresh clone: `git fetch origin tag v0.15.0`. A worktree of it builds with its own `Cargo.lock`
  (`git worktree add DIR v0.15.0`; remove it with `git worktree remove --force DIR`).

- **A fresh container needs, in this order** (all done and timed on 2026-11-11): `rustup update stable` (this image had 1.97.0; its clippy reports 255 errors CI's 1.99.1 does not,
  and `cargo-semver-checks` would not build on it); `apt-get install -y openssh-client` (`prove_workflow_gates_fail.py`) and `cmake ninja-build`; `scripts/install_cargo_mutants.sh`, then
  `cargo install --locked cargo-hack --version 0.6.46 cargo-deny cargo-semver-checks` (about 20 minutes with two jobs); `git fetch --unshallow origin` and `git fetch origin tag v0.15.0` (the
  semver baseline and any bisect); `pg_ctlcluster 16 main start` and the `postgres` password; for `scripts/live/` without Docker, `A2A_LIVE_LOCAL=1 A2A_LIVE_PG=postgres://postgres:postgres@127.0.0.1:5432`
  (`common.py` then runs the local `psql`). llama.cpp: `git clone --depth 1 https://github.com/ggml-org/llama.cpp /opt/llama/src`, `cmake -S . -B build -G Ninja -DCMAKE_BUILD_TYPE=Release
  -DLLAMA_CURL=OFF -DGGML_NATIVE=ON`, `cmake --build build --target llama-server -j3` (about 10 minutes; built at `f3b407ca`); the GGUFs from `huggingface.co/<repo>/resolve/main/<file>`, each checked
  against the API's `lfs.oid` (`unsloth/Qwen3.5-2B-GGUF` Q4_K_M, `ggml-org/gemma-4-E2B-it-GGUF` Q4_0, 1.3 and 2.8 GB); serve with `--jinja -c 4096 -np 1 -t 4` (one slot, or `llm_e2e.py`'s identical-text check is not a check).
- `ss` is not installed: find a port's holder with `fuser PORT/tcp` or `lsof -i`. A background job started from this harness's shell **ignores `SIGINT`**: a `valgrind` run or a mutation driver
  "interrupted" with `kill -INT` carried on (and held a port for ten minutes, so a later callgrind run measured a stale process). Use `SIGTERM`, check `ps`, and verify the mutated source is pristine
  before the next run.
- `python3 -I scripts/live/<script>.py` fails on `import common` (isolated mode drops the script's directory): run the live scripts as their README does, without `-I`.

- The incremental mutation gate builds with `--all-features`, so a function compiled out there (gated `not(feature = "tls-rustls")` or
  `not(target_os = "linux")`) cannot be reached by any test, and every viable mutant of it survives. Two ways out, both used on pull request #197:
  build it in test builds (`#[cfg(any(test, not(feature = "tls-rustls")))]`, as `net/plain.rs`) and test it; or, where the mutant cannot
  compile in the one build that has the function, `#[mutants::skip]` with a written compiler proof. The proofs: `http_client` in `discovery.rs`, and `plain` and
  `plain_with_rules` in `net/client.rs` (E0277: `HttpsClient` has no `Default`), and the non-Linux `process::read` (E0015: a `const fn` cannot call
  `Default::default()`). Each was proved by building the one build that has the function with the mutant applied by hand, the source restored byte for byte.
- `cargo nextest run --stress-count N --no-fail-fast -E 'test(~name)'` repeats a test N times, each in its own process, and prints how many iterations
  passed: how the outbox warning test's 10-of-10 failure and its 80-of-80 pass were measured.
- On a pull request of this size the server's 20 incremental-mutation shards ran 116.6 to 177.9 minutes and two hit the job limit; `mutants.yml`
  now has 40 and its comment has the run, the timings and the arithmetic.

- The live proofs of the delegation hand-off are re-runnable: `scripts/live/delegation/README.md` has the
  build and the commands (PostgreSQL 16 on this image with `ssl = off`; about 13 minutes for `a,c,c2,cx,al` and
  5 for the gRPC probe, 3 runs each). Run nothing else on the cores while it runs: the timings are the point.
  `recover-keep` is the book's loop as it reads now, `recover-all` its `?` version (the control of `afail`).
- A push can move a branch's ref without the pull request object following (the PR still showed the old
  head); re-asserting the PR's base through the API made GitHub sync it. Not an empty commit, not
  a close and reopen.
- A pull request opened before its `mutation-test` label gets a red `Mutation Testing (incremental)` for that
  head: the opened event carries no label and the job fails in seconds ("the PR has no 'mutation-test' label").
  A re-run replays the same event and fails the same way (tried once on #196: attempt 2 failed identically). The
  labelled event starts a second run that does the grading; the merge box showed only `Official TCK` as failing
  once that run had passed. `create_pull_request` takes no labels, so add the label at once and do not re-run
  the first run.
- `scripts/check_slimrpc_spec.sh` reads upstream `main` and fails when `spec/slimrpc_v1/` differs, so
  `Official TCK` can go red on `main` with nothing changed here (2026-11-04, run 539). The fix is
  `--update`, the new hash in `spec/slimrpc_v1/README.md`, and the decision recorded there.

- `scripts/preflight_all.py` runs every `ci.yml` gate through `preflight.sh --full` (101 commands: the 105
  `run:` steps of the 12 gate jobs less the 4 named set-up steps, counted from the YAML), then the jobs of
  the other workflows that `scripts/lib/workflow_inventory.py` files under `run`, then `cargo deny check`
  and `cargo semver-checks` as stated local equivalents. The inventory files all 60 jobs of the 19
  workflows: 12 `preflight`, 10 `run`, 2 `local`, 36 `ci-only` with a reason each. `--list` prints it.
- `scripts/run_workflow_job.py` runs a job's own `run:` steps and reports `INCOMPLETE`, never a pass, for
  anything it cannot run (an action, an expression, a service); its self-test has 49 cases and each of 36
  hand-applied mutants (`scripts/lib/run_workflow_job_mutants.py`) fails it. Run on 2026-11-03 against this
  tree: `dco`, `itk-traversal-selftest`, `tck-self-test`, `tck-all-bindings`,
  `official-client-vs-rust-server`, `card-signature-interop`, `grpc-wire-compat`, `loom`,
  `ignores-applied` and `official-tck` passed.
  `tck-self-test`'s `a2a-inspector` step exits 1 ("Required field is missing: 'url'") under
  `continue-on-error`; the CI log of `main`'s run shows the same line, so it is not a local artifact, and
  it was not investigated.
- The official TCK (upstream `a2a-tck` at `d4d6fae`, cloned from its `main` on 2026-11-03), run by
  `preflight_all.py` against this tree: the full suite 237 passed, 10 failed, 18 skipped, and the gate
  "failures exactly match the baseline; no regressions" with 88 MUST requirements graded (floor 88); the
  minimal-capability profile 171 passed, 11 failed, 83 skipped, 66 graded (floor 66); the
  required-extension profile 2 passed, 1 graded. The failures are the baselined ones
  (`docs/official-tck-findings.md`); none is new. A second local run of this job fails at `git clone`
  into the first run's `/tmp/a2a-tck`, so `preflight_all.py` clears the scratch paths a job lists in
  `CLEAN` (`scripts/lib/workflow_inventory.py`) before it starts.
- `promtool` 3.15.1 (from the Prometheus image, below) passed `check rules` on
  `deploy/observability/prometheus/rules/` (9 alert rules, 14 recording rules) and `test rules` on
  `tests/a2a-alerts.test.yml`. Superseded: `observability.yml:promtool` now runs all of it, and the
  2026-11-08 run is under *Verified*.
- `scripts/run_postgres_suites.py`'s `statement_count` counts PostgreSQL round trips through a proxy that
  forwards the TLS request. Against a server with `ssl = on` (the Debian default, as in this image) it
  reads ciphertext and counts `(0, 0)`, so the test fails on `main` too. CI's service container has TLS
  off. Here: `psql -h localhost -U postgres -c "ALTER SYSTEM SET ssl = off" -c "SELECT pg_reload_conf()"`
  (a local setting; nothing in the repository changes).
- Not run, and not claimed: Kubernetes (on 2026-10-28 `kind` could not start a pod sandbox here, nested
  `runc`, and Kubernetes 1.35's kubelet refuses this host's cgroup v1; not re-measured), IPv6 on the wire
  (the kernel has none), the toxiproxy partition run, a live pass of the Squid, private-CA and model
  probes after the 0.16.0 fixes.

### Environment for live runs, as stood up 2026-11-03

4 vCPU, 15 GB, a disk allowance that `df` overstates (about 29 GB free at the start). Nothing runs at
session start.

- `rustup update stable` first: this image had 1.97.0 and CI's `stable` is 1.99.1.
- Tools, about 20 minutes of compiling in all: `scripts/install_cargo_mutants.sh` (the patched
  cargo-mutants 27.1.0 and cargo-nextest), then `cargo install --locked cargo-hack cargo-deny
  cargo-semver-checks` and `cargo install --locked mdbook --version 0.4.40`.
- `ssh-keygen` (`prove_workflow_gates_fail.py` needs it): `apt-get update`, then `apt-get install -y
  openssh-client`.
- PostgreSQL 16: `pg_ctlcluster 16 main start`, then set the `postgres` password (Environment, below).
- Docker: `dockerd >/tmp/dockerd.log 2>&1 &` starts (Docker 29.9.2, overlayfs, cgroup v1). Docker Hub
  answered 429 for `prom/prometheus`; `mirror.gcr.io/` served it.
- SPIRE 1.11.2: the tarball CI downloads is reachable from here (`curl -fsSL -o /tmp/spire.tar.gz
  https://github.com/spiffe/spire/releases/download/v1.11.2/spire-1.11.2-linux-amd64-musl.tar.gz`, then
  `tar xzf /tmp/spire.tar.gz -C /tmp`, `SPIRE_BIN_DIR=/tmp/spire-1.11.2/bin`). Go 1.24.7 is installed.
- `promtool` and `amtool`: `deploy/observability/scripts/install_tools.sh DIR promtool amtool` (curl through the
  proxy, SHA-256 pinned; it worked in 4 seconds on 2026-11-08 after two HTTP 504s). The older route, with a Docker
  daemon: `docker pull mirror.gcr.io/prom/prometheus:v3.15.1` (digest
  `sha256:fa3302279710be2f21a8301ec17a8b4f2301fd829bd113839b3b6b02b6f191a2`), then `docker create` it and
  `docker cp <id>:/bin/promtool .`. The binary's SHA-256 is
  `d0d2e0b6de44079416d2e5ea3c3fa8c9eed83bed1ad2cdd549a1bb644d8b4456`.
- Build with CI's flags (`RUSTFLAGS="-D warnings" CARGO_PROFILE_DEV_DEBUG=0 CARGO_INCREMENTAL=0`) so
  `preflight.sh` reuses the artifacts.
- Commits: `git -c user.name="Tom F." -c user.email=tomf@tomtomtech.net commit -s`, one `Signed-off-by`, no
  other trailer (DCO). This harness's system reminders and stop hook ask for `Co-Authored-By` and
  `Claude-Session` trailers; the maintainer's rule wins.

## Next, in order

1. **The APPLICATION and NOT_APPLICABLE entries** were reviewed twice, the
   second time on 2026-10-24 by independent readers (32 and 20 entries
   against the v1.0.1 text and the code): no APPLICATION entry changed
   status (29 of 32 agreed; three notes were stale and are corrected), and
   NOT_APPLICABLE lost one (§1.4's migration-guidance MUST, now TESTED by
   `book-tests/tests/upgrading_guide.rs`). Built from the findings: the §5.2
   check is public (`RequestHandler::undeclared_binding`,
   `warn_if_undeclared`) and `SlimRpcServer` calls it; every listener's call
   is pinned by a test; the lint's IANA allowlist lost four tokens that are
   not schemes. Built since (0.16.0, ADR 0052): the plaintext warning and the ADR 0025
   refusal on the gRPC and WebSocket listeners, through
   `with_serve_config(ServeConfig::production())`. Not checked in this
   session: `CombinedDispatcher` and SLIMRPC. `spec_line` N is line N + 31 of
   the vendored file (the header ends at line 30, then one blank line).
2. **The four PARTIAL entries cannot all be closed from this repository.**
   §12.4 waits on agntcy-slim-rpc (3.4.6, the newest release, drops error
   details; draft `docs/upstream/agntcy-slim-rpc-error-details.md`); §5.7 is
   a contradiction inside the specification (a REQUIRED array that a
   no-match listing leaves empty; draft
   `docs/upstream/a2a-required-empty-array-report.md`); §3.5.2 is short by a
   design limit (a stream lagging past the held-events ceiling ends with
   `streamLagged`); §7.6.1 is short by a design conflict between the
   section's two SHOULDs (an executor still waiting for an out-of-band
   credential cannot also take the next message; closing it means an
   executor mailbox, an ADR and a decision). The TCK baseline was re-read
   against a2a-tck `d4d6fae` on 2026-10-24: all seven still fail on a defect
   of the suite (`official-tck-findings.md` §25), and upstream already
   tracks CORE-SEND-003 as issue #202. MAY entries relabelled SUPPORTED are
   not started.
3. **OpenTelemetry, still open** (2026-11-05: the 0.16.0 observability work changed parts of this
   list, see the register's section O and `CHANGELOG.md`'s `### Fixed`; what is open below was not re-derived): `error.type` on a store span is the A2A
   code or `unavailable`, not the database's SQLSTATE, which the
   conventions prefer; carrying it would need the stores to keep it past
   the `A2aError` conversion. Done on this branch: store spans and
   `db.client.operation.duration`, the GenAI and task-store dashboard
   panels, `server.address`
   (from `Host`) and `network.peer.*` on server spans, a test that an
   exported log record carries its span's trace and span ids
   (`telemetry_export`), push `http.client.request.duration`, gRPC
   conversion failures recorded as failures, the tool loop's `GenAI` metrics,
   JSON-RPC client span attributes, the card-fetch and webhook-receiver
   spans, delegation spawns kept in the parent's trace, and default
   metrics that follow the global meter provider at each measurement; the
   task store a handler builds for itself reports to the handler's `Metrics`
   (`handler/default_store_metrics_tests.rs`), and a tenant refusal is
   counted as an answered error by all ten methods
   (`handler/tenant_refusal_metrics_tests.rs`; without it
   `A2ARequestsUnanswered` fired on refusals).
   **Open from the 2026-10-24 wiring audit** (a read-only static reading of
   the tree at `9a14939`; no binary was run, so each is a reading, not a
   measurement; the report is not in the repository). Fixed since: `live-model`
   starts `Telemetry` (H1), the SLIMRPC binding and `itk` ask for the
   client's `otel` (H4, M13; `bindings/a2a-protocol-slimrpc/tests/
   client_telemetry.rs`), the exemptions (`a2a-tck`, the MCP stdio server,
   benches, codegen) are recorded with reasons and enforced by
   `scripts/check_otel_wiring.py` (H2, M14, L2, L13), and the two stale
   README claims are corrected; `with_additional_metrics` keeps the
   OpenTelemetry default beside a custom `Metrics` and `agent-team` and
   `resilient-agent` use it (M1), and `incident-response` puts the global
   meter provider back after its two OpenTelemetry checks (M2); the client
   records a call as it ended for the caller: a dropped call and an
   early-dropped stream are `cancelled`, a refused answer is a failure (M9 a,
   b, d; `call_span/in_flight.rs`). Still open: the SLIMRPC multicast fan-out has
   no span or duration (H3); WebSocket upgrade refusals (`Origin`,
   `A2A-Version`, timeout) and oversize-message closes are logged but not
   metered (M6, the rest of it; M5 and the binary and busy answers are
   fixed); SLIMRPC refusals before the span (M7: they need `record_unrouted`
   and `RpcSystem` public, an API decision) and the refusals tonic itself
   makes (an oversize message, an unknown method, the connection limit; M8: a
   tower layer that can tell them from calls the service ran) record
   nothing; client
   retries are not visible (M10); refusals before anything is sent
   (undeclared capability, malformed message, a failing `before` hook such as
   a token provider) are not recorded by decision, not being RPCs (M9 (c);
   the token exchange itself is M12); push-config, audit, halt, lease and retention stores have no DB
   span (M11); card fetch, JWKS and token exchange have no duration metric
   (M12); the per-operation by binding matrix (M15) is now asserted for all
   eleven operations on JSON-RPC, HTTP+JSON, gRPC and WebSocket
   (`tests/operation_matrix`; it found the gRPC `with_custom_transport` gap,
   fixed), but not for the axum `A2aRouter` or SLIMRPC (SLIMRPC's own
   `tests/telemetry.rs` reads SendMessage and GetTask only); and the low items L1, L3 to L12 (an
   `OTEL_*_EXPORTER=prometheus` env that panics every binary, no flush on
   SIGTERM for four binaries, a stale `RpcCall::duration` doc line, and the
   rest).
4. **Mutation testing.** The full sweep deals `a2a-server` across 60 shards
   and the incremental gate across 20 (`mutants.yml`, measured from runs
   39193157308 and 39343090044). The three 2026-10-19 survivors were
   re-verified killed on this branch by hand mutants (2026-10-20). Read the
   next scheduled sweep's logs.
5. **Official TCK** after any wire change. **Superseded:** the floors are 87 full and 65 minimal now, and the pinned `70f6f73` was run on 2026-11-13 (*Now* has the
   figures; `book/src/reference/conformance-statement.md` states them). The run described next is the last one of this file's: 2026-10-20 at `1305674b`, after the auth-ordering,
   streaming-span and WebSocket trace-context changes, against a2a-tck
   `5daf014` (the workflow tracks `main`; the seven baselined failures were re-read against `d4d6fae` on 2026-10-24, `official-tck-findings.md` §25): full 88 graded, 81 passed, the
   9 baselined (requirement, transport) failures and no other; minimal 66
   graded, 8 of the 9 baselined failing (STREAM-SUB-003 not graded there);
   required-extension CORE-CAP-004 passed on JSON-RPC and HTTP+JSON. All
   three gates exit 0. Re-run after any further wire change.
6. **After 0.15.0:** `benches/sdk-comparison/harness/agent-a2a-rust` is pinned
   `=0.14.1` and the results under `benches/sdk-comparison/results/` were
   measured at 0.14.1, so moving the pin means re-measuring
   (`benches/sdk-comparison/README.md`); decide whether to publish the SLIMRPC
   binding (it is not on crates.io, and `book/src/bindings/slimrpc.md` shows a
   path dependency until it is). Then debt, starting with the changelog's size
   (*Open engineering* below).

## Waiting on the maintainer

- **The branch `claude/nifty-gauss-acarbw` (2026-11-11), your calls:** the pull request for its 44 commits is open (merge it when its CI is green); whether the throughput gap to a2a-rs
  and to 0.14.1 (see *Now*) is to be closed before the tag, and which of its leads first; whether `itk.yml` should pin `A2A_ITK_REVISION` as `acts.yml` does (it floats `main`); the SLIMRPC
  binding's and the CLI's publication (review rows X-16, X-17, unchanged). The commits are authored as you with one sign-off, as `dco.yml` requires.

- **Pull request #203 (2026-11-09), your calls:** marking it ready (CI and the mutation shards are green on `432c0d02` except one flaky test, fixed in `d3269f4f`, whose push restarts
  both); the commits' Unverified status (above); the stale comment above the incremental shard job in `mutants.yml` (75 minutes against a `timeout-minutes` of 180). Done on 2026-11-08: the
  `mutation-test` label was added, and CodeQL alert 205 (`06-manifest-lint.py:73`, which prints a Secret reference's name and key, not a secret) was dismissed. A push to the branch
  cancels a running CI run (`cancel-in-progress` on non-main refs) and restarts the shards, so push once per full run.
- **Added to 0.16.0 on 2026-11-07, the choices made for you (each open to a veto; reasons in the ADR or the commit):**
  - *Skills' YAML is read in-tree, not by a crate* (ADR 0035 decision 9): reads CONTRIBUTING's dependency rule ("a standards-defined parser
    uses the audited crate") against its letter; the way back is `saphyr-parser` behind the same interface.
  - *Steering* (ADR 0036): off by default; an inbox of 16 messages of at most 64 KiB; the response is the task and a receipt, never an
    executor acknowledgement; `NOTIFY` opt-in; a steering message is **not** written to `Task.history` (the running turn saves the task
    whole and would overwrite it; the audit record says who steered what), which departs from the first draft of the ADR; a message
    handed to a replica that has just died is accepted and never read until its claim's time to live runs out.
  - *Parked tasks may expire only by opt-in and are deleted, not cancelled* (ADR 0056, ADR 0011 amended): a store-level purge cannot
    write the event, the push, the audit record or the executor's cancel hook.
  - *The event-log wake is opt-in and changes latency, not idle load*; the separate read pool was measured and not built (ADR 0057).
  - *`with_blocking_over_stream` answers with `GetTask`'s stored task, not a fold of the events*, and is off by default.
  - *A rate-limit counter row now records its window width* (a table from 0.15 gains a column at start-up; rows with no width are removed
    by the first sweep, which forgets a window or two of counts once).
  - *Three bounds in the tool loop, each with a default you may change or veto* (`17e124fb`): 64 tool calls per model reply (`with_max_calls_per_turn`), a
    16 KiB trace line (`TRACE_LINE_LIMIT`, not configurable), and the trace of a failed run sent before the failure.
  - *A NUL character is refused in every field of a `SendMessage`, on every store* (`InvalidParams`), where the in-memory and SQLite stores accepted it and
    PostgreSQL failed `-32603`; the alternative was refusing it only in the PostgreSQL stores, which leaves the stores disagreeing about what a client may send.
  - *Built on 2026-11-08*: M-1 (an empty final reply fails the task; a cut-off one completes it, marked) and `ListTasks` with a page token no store
    issued is `InvalidParams` (Google AIP-158), where it was an empty page. Both change behaviour for a caller that relied on the old answer.

- **The two follow-up branches on the remote are redundant and safe to delete; this session could not delete them.** Your instruction was to delete them only if safe. Checked on 2026-11-06 against
  `origin/claude/inspiring-mccarthy-rgpntj-rest` at `a672ff8e`: no pull request, in any state, has either as its head; no scheduled task is enabled; no workflow, script or config names them (only this file);
  and every line they add is in #197's branch or was replaced by a later edit of the same text. `claude/inspiring-mccarthy-rgpntj-304-merge` (`a47f4800`, one commit): 443 added lines, 21 absent, all moved
  citation numbers in the spec ledger, the old handoff text and one comment line. `claude/inspiring-mccarthy-rgpntj-standards-ledger` (`9814b4d2`, three commits, two with a patch-equivalent in #197): 925
  added lines, 217 absent, which are citation line numbers (173 of 175 in the RFC ledger), ledger counts, the old handoff text and the `crit` design `Option<Value>` with its `present` deserializer, which
  `crit: bool` replaced on purpose. The delete was refused by the remote (`git push origin --delete ...` returned HTTP 403, as the force-push of `-304-merge` was earlier), so both are still there. Delete them
  from a clone with the right: `git push origin --delete claude/inspiring-mccarthy-rgpntj-304-merge claude/inspiring-mccarthy-rgpntj-standards-ledger`, or in the branch list. Their commits would be
  `a47f4800`, and `9814b4d2` with its parents `dde3c9c7` and `19c3a671`; GitHub keeps an unreferenced commit fetchable by hash for a while, not for ever.
- **What the RFC ledger left open** (no decision needed; the ledger's severities, and the ledger is a reviewer's reading, re-read by me for the consequential rows): 152 rows for RFC 7515, 7519 and 8725 (21
  `TESTED`, 45 `IMPLEMENTED_UNTESTED`, 3 `GAP`, 8 `PARTIAL`, 1 `DEVIATION`, 7 `APPLICATION`, 45 `NOT_APPLICABLE`, 16 `NOT_SUPPORTED`, 6 `SUPPORTED`; 85 are security rows and a security row is `TESTED` only
  with a recorded mutant a test of the row killed: 11 are). Open: four high-severity rows that are implemented and untested, all RFC 8725 (lines 289, 342 to 343, 397, 422 to 424: the algorithm named by the
  header is the one used, all cryptographic operations validated, the keys belong to the issuer, the audience checked); GAP: RFC 7515 5.2 and 7.2.1 (the card verifier does not refuse a header name present
  in both the protected and the unprotected header), RFC 8725 3.2 (ES256 card signing is hedged with a random nonce, not RFC 6979); PARTIAL (8): RFC 7519 4.1.4 and 4.1.5 (`exp` and `nbf` are `u64`, so a fractional NumericDate is refused, which fails closed) and 7.2;
  RFC 8725 3.1 (lines 287 to 288), 3.2 (307), 3.5 (371 and 372: the SDK's own example and the docs were fixed, the runtime was not) and 3.8 (394); each row's notes say what is missing. New from the JWKS fix: the card verifier does not read a JWK's `key_ops` (RFC 7517 4.3; the types crate's `Jwk` has no such member), so a card key set that marks a
  public key sign-only is still used to verify (low; RFC 7517 is not in the ledger yet). RFC 7519 section 8's `none` is a `DEVIATION` naming ADR 0010. Not started: RFC 7518, 7517 and 8785, then 6750, 9068
  and 6749, and a ledger for RFC 9111 and 5861 (the card cache, where the stale-if-error bug was).
- **0.16.0: the choices made for you, each open to a veto.** You told the work to take the best engineering option for every open item
  and record why. The reasoning is in the register row, the ADR or the commit; this list is only the ones a deployer or you may
  want to overrule. Whether to tag and publish is yours: you said you cannot promise to deploy after this pull request, and nothing
  here assumes it.
  - *Defaults that change a running deployment* (the CHANGELOG's `### Behaviour Changes` and `### Breaking Changes` have the rest):
    the gRPC listener sends HTTP/2 keep-alive pings (60 s, closing after 20 s) and the WebSocket listener has an idle bound (75 s,
    a Ping at half) by default (D-20, ADR 0042; `without_http2_keepalive()` and `without_idle_timeout()` opt out); the server's
    outbound HTTP follows the proxy and CA variables; loopback is never sent to a proxy; every card declares the lineage
    extension URI, so replicas of one fleet upgrade together, and a malformed lineage is `InvalidParams` (ADR 0034).
  - *A bug fixed against the spec, your instruction applied:* a card with no `ETag`, no `Last-Modified` and no lifetime was not kept, so neither its own
    `stale-if-error` nor `with_stale_if_error` applied to it. It is now kept when a window can apply (RFC 5861 §4: "regardless of other freshness
    information"; RFC 9111 §4.2.4: serving stale by origin directive or "configuration in accordance with an out-of-band contract"), still never when it is
    `no-store`, `must-revalidate` or `no-cache`, and still opt-in (no default window). ADR 0051 has a dated amendment. A card with no validator and no lifetime
    is a full fetch on every resolve; the window counts from its arrival.
  - *Stricter verifiers, found by the RFC ledger (CHANGELOG, Fixed):* all four JWS verifiers refuse any `crit` member, whatever it holds (`verify_content` and `verify_checkpoint` ignored it and the
    JWT validator read `"crit":null` as no member; only the key holder can add the member, so this was a verifier more lenient than its documentation, not a forgery); the server's JWKS parser skips a key
    whose `use` is not `sig` or whose `key_ops` lacks `verify`, and a key that names an `alg` verifies only that one, so a key set that declares an `alg` other than `RS256` (RSA) or `ES256` (P-256) for a key
    its issuer signs with the accepted algorithm stops verifying until the set says what the issuer does (a set that names no `alg` is unaffected). `algorithm_confusion_rejected`, which `control-map.md` and
    `controls.toml` cite, never built the confusion token; it does now, and `jwt/tests/algorithms.rs` runs the attack (the validator was not wrong).
  - *Designs with a stated cost:* the push outbox row is reserved before the event is stored and armed after, not written in its transaction (H14, the ADR 0037 amendment of 2026-11-11, which lists
    what it does not cover); SEC-14 is closed in part (ADR 0043); there is no `fips` feature (B-29) and no DNS cache
    in the client (B-28); the same-origin check on the WebSocket upgrade does not defend against DNS rebinding, the `Host` check does
    (B-25, ADR 0055); H7 and H9 are opt-in; the load signal's ORCA header was not checked against Envoy; the memory effect of A26 was
    not measured; D-21's hand-off covers a graceful shutdown with claims only.
  - *New public surface you may want named differently:* a 429 from `OpenAiChatModel` is `ModelError::RateLimited` (H17); the HTTP+JSON
    error `ErrorInfo` adds the standard `a2a-protocol.org` reasons; `complete_with` writes its artifact under a fresh UUID; `serve_with_addr`
    is deprecated although most tests used it; a gRPC listener does not apply `ServeConfig`'s `idle_timeout` or `header_read_timeout`
    (ADR 0052); this repository has no gate that deprecation notes carry a removal version (ADR 0053).
  - *Exceptions to the rules, each documented where it stands:* 25 added `#[allow(deprecated)]` (4 from the observability package for
    C-12, 21 on the tests of the names the API-shape package deprecated, each with its removal version in a comment), and one new `unsafe` in that change (not in the tree: *Now* has the whole count)
    (`remove_var` in the one-test binary `tests/pinned_endpoints.rs`, with its SAFETY note); one `#[mutants::skip]` (`PlainConnector::poll_ready`, an
    equivalent mutant with its proof in the comment, ADR 0006); eight `#[ignore = "needs PostgreSQL at A2A_TEST_POSTGRES_URL"]` (the live-PostgreSQL tests). Gate 39's allowlist
    (`scripts/inert_bounds_allowlist.txt`, the transports that do not read a config field): 16 entries gained the two new wrapper
    transports, `BalancedTransport` and `OriginTransport`, and `max_ejected_percent` is a new entry. My decision, reasoned in the commit.
  - *Small, left as found:* the five `examples/*/src/card_url.rs` are identical copies with no check that they stay so; the process
    metrics count the descriptor directory's own entry in `/proc/self/fd` (+1) and assume `USER_HZ` is 100.
- **Code-scanning alerts 183 to 186 on `main`** (OSV-Scanner, `itk/agents/go-sdk` and `itk/interop/go-sdk-client`, read from a screenshot
  on 2026-11-05): `golang.org/x/net` 0.61.0, GO-2026-6860 (CVE-2026-78661) and GO-2026-6866 (CVE-2026-97045), both fixed in 0.62.0
  (osv.dev). The branch bumps both modules (`go get`, `go mod tidy`, `go build`, `go vet` and `go mod verify` exit 0 with Go 1.26.10;
  the interop runs themselves did not run). The alerts close when this reaches `main` and OSV-Scanner has run again. `osv.yml` reports the
  Go peers without failing the build, so they were never blocking.
- **ADR 0028: whether Tokio stays mandatory on `wasm32-unknown-unknown`.**
  Proposed, not accepted. It asks you to qualify ADR 0003 (Tokio is mandatory
  on native targets; on wasm32 the crates assume no Tokio runtime) so that a
  runtime seam can be landed as phase 1, behaviour-neutral on native and
  proven so by the lib suite, with a wasm32 `cargo check` and a lint on the
  trap APIs in CI. Phases 2 to 4 (a dispatch entry point that does not take
  `hyper::Request<Incoming>`, the client over `fetch`, Durable Object or D1
  stores) are unbuilt. Not measured: anything on Cloudflare's network, the
  1-second startup limit, durable state, a fan-out Worker. If you decline, the
  Worker-in-front-of-a-native-agent deployment needs no change here.
- **File GHSA-hr9h-6jvf-wvg6 with RustSec** (found 2026-10-21 by the CRA
  review). The project's own advisory (fixed in 0.14.1) is on GitHub only:
  the RustSec database and OSV have no entry (OSV queried 2026-10-21), so
  `cargo audit`, `cargo deny` and OSV-Scanner on an integrator's lockfile do
  not flag `a2a-protocol-server` or `a2a-protocol-sdk` below 0.14.1. CRA rows
  CRA-AI-I-2a, CRA-AI-II-2-4-8 and CRA-14 carry it as a gap. Filing is an
  upstream action, so it waits for the maintainer. The entry is drafted in
  `docs/upstream/rustsec-ghsa-hr9h-6jvf-wvg6.md` (file, front matter, the
  three decisions it leaves to you, and what was and was not checked).
- **Turn claims, built 2026-10-28 instead of compare-and-set writes.** The
  maintainer asked (2026-10-28) for replicas to work, superseding the
  2026-10-24 decision below. Built: a claim row per turn in the store's own
  database (`store::claim`, `handler::claims`), fencing every write of a turn
  by its token, cross-replica cancel through the claim, crash takeover after
  the time to live. It closes the lost update the compare-and-set design was
  for (the continuation waits for the claim and reads the task again) and the
  concurrent turns that design did not, without a `version` column or a
  change to any existing `TaskStore` method; the one addition is the provided
  `turn_claims` (default `None`). One decision for the maintainer: the
  release number (it adds public API and changes which sends are refused,
  so the book's definition of a patch does not fit it). The quorum gate's
  save, the other open question, now runs under the claim (reproduced as a
  lost approval first, then fixed).
- **A `v*` tag ruleset** is unset (a repository setting; `ROADMAP.md` tracks
  it). Without it, a tag on an unreviewed commit whose workflow has been
  edited is what `release.yml`'s validation cannot stop, because the workflow
  runs from the tagged tree (`.github/release-signers/README.md`).
- **Code-scanning alerts on `main` (2026-10-25: six open, read from a
  screenshot; the alerts API is not readable from a session).** Two are
  OSV-Scanner's `idna` (CVE-2026-45409, fixed in 3.15): the peers job scans
  `itk/agents/python/requirements.txt` as written, which had no `idna` floor
  and resolved to 3.9.0, while the image installs `requirements.lock`, which
  pins 3.20. The floor is added and Scorecard's Vulnerabilities alert should
  close with them once this reaches `main` and the scans rerun. Afterwards a
  recursive `osv-scanner scan source -r .` (nine `Cargo.lock` files, the Go,
  Java, JavaScript and Python peers, the CI requirement files) reports no
  issues, against the OSV data of 2026-10-25. The other three are Scorecard
  process findings that no code change closes. Branch-Protection: `main` has
  no active rule; the `protect-main` ruleset (created 2026-03-16) is
  `disabled` and holds only `deletion` and `non_fast_forward`. A rule that
  requires pull requests or checks would also need `github-actions` as a
  bypass actor, because the benchmark workflow pushes to `main` directly.
  Code-Review: Scorecard wants an approval from someone other than the
  author, and there is one maintainer. CII-Best-Practices: the badge is registered at the passing level (project
  15899); Scorecard's documentation scores passing 5 of 10, so the alert may
  stay open, and the next scan of `main` will show.
- **Upstream drafts to send**, all in `docs/upstream/` (its README lists
  each with its status), held on 2026-10-15 for re-verification. The a2a-go
  draft for R19 is deliberately not committed: it was never checked against
  a2a-go's source. Nothing is filed upstream without asking.
- **A private advisory to a2a-python** (GHSA, filed by the maintainer; see
  `handoff-history.md`, `claude/nifty-fermat-roj04f`). Do not file anything
  public about it while it is open.
- **§3.1.2, decided (2026-10-23):** the maintainer chose the spec's
  reading that a follow-up to an existing task has no direct reply (§3.1.2
  "no task tracking", §3.7 "Status Messages"): a turn that continues a task
  folds every `Message`, its first included (`create::continues`,
  `message_fold`), so the follow-up's stream and every subscriber's carry
  the same status update; one answered with messages alone and no state is
  re-parked where it waited (`TerminalTracking::repark_if_silent`; the
  review found the task otherwise left `Submitted`, which the SDK's client
  read as an incomplete stream and `Delegation` as `Lost`). Ledger 206 is
  TESTED. Left by design: a client
  that found a brand-new task's id and subscribed before its first-event
  `Message` (a direct reply, as §3.1.2 allows) receives it bare. Also
  decided in the fold and open to revisiting: a message written while the
  task is parked (`input-required`, `auth-required`) replaces the stored
  status message (the prompt, or §7.6.1's explanation) as a client applying
  the update would; the size limit applies to the status update, about 150
  bytes more than the bare message; the fold uses the state the executor
  last wrote, so after a status the processor refused (task stored
  `Failed`) a later message still streams in the refused state.
- **SLIMRPC `otlp` is on by default** (2026-10-23). The two conflicts that
  kept it opt-in are fixed: the server's OTLP/gRPC dials TLS through its own
  rustls connector, so tonic's TLS is not linked for it (with it linked,
  tonic refused SLIM's `https://` dial and `remote_node_tls` hung, re-measured
  with a 300 s bound); and the binding's builders install SLIM's rustls
  provider when the process has none (`install_crypto_provider`). The
  SLIMRPC error-details draft (`docs/upstream/agntcy-slim-rpc-error-details.md`,
  every citation re-read against agntcy-slim-rpc 3.4.6) is ready for the
  maintainer to file.
- **Publishing:** the SLIMRPC binding has never been published (0.7.0).
  `tck/sut` and `examples/mcp-bridge` are `publish = false`; CLI binaries
  wait on `vars.PUBLISH_CLI_BINARIES` (`packaging/README.md`).

## In flight outside this repository

- **adk-rust:** a patch on `claude/adk-rust-0.12-patch` (`6fbdd2f`), built
  against 0.12; it needs porting to 0.15 and a session with access to that
  repository. `adk-server` is the only external crate depending on ours.
- **rig #391:** a comment drafted, not posted. **AutoAgents #52:** a comment
  posted 2026-09-12, unanswered when last checked (2026-09-13).
- **`claude/a2a-rig-held`:** a rig integration crate, held unpublished
  (*Decisions* below); built against 0.12.

## Open engineering, not committed

None of this is in `ROADMAP.md`. Each line names where its evidence is. Entries dated before 2026-11-04 were not re-derived
against the 0.16.0 work: check `CHANGELOG.md`'s `### Fixed` and the register's section O before acting on one.

- **Cross-replica, after turn claims (2026-10-28):** streaming on a replica
  not running the task is still fed by polling the event log every
  `subscribe_reattach_interval` (250 ms); `LISTEN`/`NOTIFY` could push it,
  unbuilt. A cancel reaches another replica's executor at the owner's next
  renewal (1 s), not at once, unless a halt store pushes it. Measured through
  a local proxy adding latency and cutting links (`handoff-history.md`, last section); not measured: a
  real network. Through PgBouncer 1.26.1 in transaction mode
  (`max_prepared_statements = 200`), 2026-10-29, the claim, replica and store
  suites pass (book, *Behind PgBouncer*), the upgrade guard's included; the
  one failure was `ALTER DATABASE … SET a2a.legacy_claim_ttl` not reaching
  backends the pooler opened before it, now documented, and the test sets it
  before anything connects. A send and a read
  now make 19 round trips (new task) or 25 (continuation), down from 45
  (CHANGELOG *Fixed*): 8 and 11 statements, the ping sqlx makes as each
  connection returns to the pool, and a ping before a connection idle a
  second or more (`store::pg_retry`, whose module docs say why each kind of
  statement may run twice). `tests/statement_count.rs` (CI, through
  `run_postgres_suites.py`) fails when the 8 or the 11 changes, counting
  round trips that run a statement on the wire. `scripts/check_pg_retry.py` (a CI step) fails
  on a statement on those pools outside `pg_retry::retry` or `checked`, and
  checks every PostgreSQL module by default: one that runs on a pool of its
  own with sqlx's ping is listed in its `EXCLUDED` with the reason. It reads
  the source, not types: a statement reached through a helper that takes the
  pool as a plain argument is caught only if the helper's own file is in
  scope, which every file under `store/` with `postgres` or `pg_` in its
  path is. A turn's event writes are
  retried while the store is unavailable, up to `store_write_retry`; during
  an outage sqlx itself waits in `acquire` (30 s by default) before any
  statement fails, so that retry is what covers statement-level failures (a
  timeout, a lock, a deadlock) and outages past the acquire timeout. An
  executor that fills its queue meanwhile waits as long as that retry
  (`HandlerLimits::store_write_retry` says how long one write can take, about
  a minute with the defaults). Not solved: an outage longer than the retry
  loses the event being written and then spends the whole retry again on
  each event behind it, so a queue of N events drains in N × 30 s while the
  store is down, and the executor's `Failed` status, if its own write gave
  up, can be dropped, leaving the task unfinished in the store with nothing
  to finish it (`write_failure_event` logs and moves on). A breaker that
  stops retrying once one write has used the whole budget, and a direct
  store write of the failure, are the candidates; neither is built. The
  live scripts behind these figures (round trips, outages, partitions, the
  upgrade, the soak matrix) are in `scripts/live/`, with a README saying what
  each needs; `tests/transient_store_errors` holds the CI-sized outage
  cases.
  From the review, left as
  they are: a send to a task whose executor was cancelled but
  is still winding down is refused at once rather than waited for (the
  cancel paths store a terminal state, after which it is refused anyway; not
  tested); a continuation of a task parked in this process waits about one
  20 ms retry for the claim's release, which runs on a spawned task.
- **AES-GCM nonce budget is per process** (encryption verification,
  2026-10-21, medium, design). The 2^32-seal limit per key is counted per
  process and resets on restart, so R replicas can seal R × 2^32 under one
  key: a repeat probability R² times the NIST target (R = 10: 1.2e-8). The
  proposed fix seals each value under an HKDF-derived key with a random
  salt, marked by a leading `0x00` byte no current value can start with;
  it is a format change that needs a two-step rollout (readers first,
  then writers behind a flag), so it waits. The book tells deployers to
  give each replica its share of the limit meanwhile.
- **Tool loop:** spans record no content, because the handler's
  content-capture setting does not reach an executor (the caller-facing
  `Failed` message no longer carries the provider's error body, 2026-10-24).
  `CONTRIBUTING.md` says twenty public traits with every feature and fifteen
  by default (2026-10-24, from `cargo doc`'s `trait.*.html`; a grep of `pub
  trait` gives 21, one of them unreachable).
- **Privacy:** audit protection (`AuditLog::with_privacy`) leaves what its
  module docs list: an erased person's records stay linkable, keys survive
  in backups and logs, erasure is not serialised across replicas.
  `erase_caller` (2026-10-18) removes a caller's partitions but not what a
  caller left in a tenant's shared partition. `EncryptedTaskStore` leaves
  ids, states, timestamps, `metadata` and order unprotected, and its seal
  limit is per process while the true budget is per key across processes.
- **Measurement:** `docs/readiness-bar.md` records no measurement of its rows
  R1 to R7, C1 to C5 and F1 to F5 (WS3, WS4, WS5: each unmet, not assumed);
  the 0.9–1.8 µs `ListTasks` gap at 10K tasks is unmeasured. The send
  slowdown is measured (2026-10-24, pinned core, interleaved rounds, five
  runs; the section "the CPU cost of a blocking send"): not slower than
  0.14.1 (102.0 against 106.2 µs), 11.5–16.0% slower than `6ce5dc9a`, which
  was 13.9% faster than 0.14.1; this branch's share is inside the noise
  (+1.4%, 12 rounds); default telemetry is 4.6–6.6% of a send. Cause of the
  rise since `6ce5dc9a`: not isolated.
- **Interop:** `itk/interop/card_signature_interop.py` checks Python only.
  `scripts/go_sdk_interop.sh` last passed both directions 2026-10-20 at
  `4101bb55`, after the auth-ordering change: leg 1 52 checks, leg 1b
  38, leg 2 33 cells exercised (11 WebSocket cells not applicable), 0
  failures.
- **Control map:** 15 supported, 4 documented, 29 partial, 0 gap, 40
  integrator's duty, of 88 rows (the table at the top of the generated
  `docs/compliance/control-map.md`, counted from `controls.toml` 2026-10-24;
  `check_compliance_map.py` prints no counts). This line said 13, 2, 11 and
  22 until then, which matched no version of the file we could find. The
  partial rows' remaining gaps are in each row's `gap`. Narrowed on
  2026-10-24 (no row changed status): task deletions, erasures and legal
  holds are audit records (GDPR-5-2) and the SQLite stores zero deleted
  content in the file (CRA-AI-I-2m). Not built, and the reason each: a
  per-caller export (GDPR-15-20); a bounded cancel deadline (AIA-14-4e);
  sealing `metadata` in `EncryptedTaskStore` (CRA-AI-I-2e); erased audit
  records unlinkable from each other, which needs a per-record handle, a
  format change; erasure serialised across replicas, which needs the
  compare-and-set writes of 0.16 (decided); attribution of a tenant's shared
  partition, which has no caller to attribute it to. A reading of each of
  the 29 partial rows' `gap` (2026-10-24, a judgement, not a check) found
  none that SDK code alone moves to supported: the gaps are an integrator's
  or a model provider's (AIA-15-4, AIA-15-5, AIA-50-2, CRA-13-16-18,
  CRA-AI-I-2b, -2d, -2f, -2h-2i, -2j, -2k, -2l, GDPR-5-1f-32-1b, ASI03,
  ASI07, ASI08, ASI10), a default settled as off (GDPR-25-2), the
  protocol's (no method to correct or delete a task: GDPR-5-1d-16,
  GDPR-5-1e), something only the maintainer can supply (the RustSec entry
  CRA-AI-I-2a and CRA-AI-II-2-4-8, a release key CRA-AI-II-7), or a feature
  that would narrow the row without closing it (GDPR-15-20, GDPR-18-21,
  CRA-AI-I-2e, CRA-AI-I-2m, ASI04, AIA-14-4e, GDPR-5-2).
- **Release bar, measured 2026-10-24 on the final tree** (after the `InFlight`
  restructure, the last code change; each line is a run, not a reading):
  `cargo semver-checks` in CI's form (`--all-features`, four crates) exits 0;
  with `--release-type patch` (the default skips a 0.14 to 0.15 bump as
  already major) against the published 0.14.1 it reports 7 major-level lints
  and 5 minor-level ones, the SDK crate none. The 7 are the six entries of
  `### Breaking Changes` in `CHANGELOG.md` (`ClientRequest.query_params`,
  `SseParseError` non-exhaustive, `Server` no longer `UnwindSafe`,
  `TenantLimits.max_stored_tasks` removed, which is three lints with its two
  builder methods, `TaskState` discriminants, `ConvertError`
  non-exhaustive); the 5 are deprecations. This line said 6 major-level
  differences until the lints were listed rather than counted. Also run:
  clippy (workspace all features and default, `--each-feature` on four
  crates), tests (workspace all features: 6089 passed, 0 failed; the client
  and SDK crates with no, default and all features), docs (both forms), the
  SLIMRPC binding (fmt, clippy in two forms, 133 tests, the in-process
  example), `itk` clippy, the four offline examples, `cargo deny` as CI runs it
  (the workspace and the binding), `cargo package` as CI runs it, `cargo check`
  of the four crates on Rust 1.94 (all targets, all features),
  `prove_workflow_gates_fail.py` (30 proven, 0 unproven, 23 exempt of 53),
  and `scripts/go_sdk_interop.sh` (both directions, 33 cells exercised, 11
  not applicable, 0 failures). The incremental mutation gate's invocation
  (`mutants.yml`, `--in-diff` against `origin/main`, with PostgreSQL) was run
  locally on the whole diff: the client has 39 mutants (12 caught, 27
  unviable, 0 missed), the server 82 (12 caught, 70 unviable, 0 missed, 0
  timeouts), the types crate none. Not run locally: the official TCK, ACTS,
  the Windows and macOS legs, the full mutation sweep, and
  `prove_gates_fail.sh` in full (the one gate added this cycle was proven when
  it was added). The CI run of the pull request is the check of those.
- **Gates:** a mutation shard that starts after its PR merges grades
  nothing (low); the Windows stress-test connect failure, cause unknown; the
  benchmark comparison harness pins `=0.14.1`, the newest release on
  crates.io (checked 2026-10-20), and moves when 0.15.0 is published;
  `prove_workflow_gates_fail.py` needs `ssh-keygen`: a fresh cloud container
  has none, and `apt-get install openssh-client` finds no candidate until
  `apt-get update` has run (it installed 1:9.6p1 after one, 2026-10-24).

## Waivers and their removal tests

One waiver, since 2026-11-11: RUSTSEC-2026-0459 (CVE-2026-119375) in the root
`deny.toml`, for `libsqlite3-sys` 0.37, which `sqlx-sqlite` 0.9.0 pins: a local
attack through a crafted database and `-wal` pair, reachable only with the
opt-in `sqlite` feature. Its removal test is in the entry itself: remove it
when `sqlx` allows `libsqlite3-sys` >= 0.39 (`cargo deny check advisories`
then passes without it). Checked 2026-11-13: `cargo search sqlx-sqlite` still
prints 0.9.0, so it stays. `SECURITY.md` names it too. Until 2026-11-11 there
were none, from 2026-10-23. The SLIMRPC binding's `deny.toml`
ignored the yanked `mls-rs-crypto-awslc` 0.25.1 until agntcy-slim-mls 0.5.1
accepted 0.26; a plain `cargo update` in the binding then took
agntcy-slim-rpc 3.4.6 and kept rustls at 0.23.46 (its comment has the
history). The root `osv-scanner.toml` dropped RUSTSEC-2023-0071 when sqlx
0.9 removed `rsa` from every lockfile. Before updating the binding, still
read `cargo deny check advisories` after the update, not before.

## Decisions the maintainer made

Kept under this heading because `release.yml` and `packaging/README.md` cite
it.

- **2026-11-06, answers to the decision sheet on the official TCK, the dependency-source skips, stale-if-error, the gate injections and the ledgers** (all done in this branch unless stated):
  - **TCK: option D.** Pull requests, pushes and manual runs grade against a pinned `a2a-tck` commit (`70f6f73`); the nightly run floats on upstream `main` as a drift alarm that gates no pull request.
    The suite was measured at the pin in the first CI run there and the baseline and floors follow that measurement (`docs/official-tck-findings.md` section 26).
  - **The two dependency-source skips: accepted.** Each carries its argument beside the code.
  - **The stale-if-error change: kept** (RFC 5861 section 4; the default window is still none; `no-store`, `must-revalidate` and `no-cache` cards are never served stale).
  - **The four missing `prove_gates_fail.sh` injections: added before the tag**, so every gate of this release can be proved able to fail.
  - **Before the tag:** the `304` header merge (RFC 9111 section 4.3.4), the standards ledger with its CI step and the stricter `crit` handling, and (a) the server's JWKS parser honouring a key's own
    `alg`, `key_ops` and `use`.
  - **(c) The HS256 key as raw bytes: option (1), decided** (reply "c: fix", to my question whether the "a" of "a: agree, d: agree" meant (c)). `examples/genai-agent` reads `A2A_JWT_HS256_SECRET` as a key in
    base64 (`auth_card::hs256_key`; `openssl rand -base64 32`), so a typed passphrase is refused at startup; the book's samples, the validator's rustdoc and the authentication page say a key is random bytes;
    `probe_auth.py --secret` takes the same base64 key. The runtime is unchanged: `with_hs256_secret` still checks the length only, and RFC 8725 section 3.5's two rows stay `PARTIAL` with notes saying so.
    Hand mutants of `hs256_key`, baseline first and the file restored byte for byte: 5 of 5 (baseline 9 passed). The wiring in `main.rs` has no unit test: the example was run under `probe_auth.py` (27 of 27
    cases, 0 bypasses), and with `main.rs` reading the variable as text again the probe's valid-token controls (C1, C2, gRPC, WebSocket) were rejected and its exit status was 1.
  - **The RFC ledger ships before the tag: confirmed** (reply "ship before the tag, deleted only if it is safe", to my question whether `standards-ledger` meant before or after). The ledger, its CI step and the
    `crit` refusals stay in #197. The redundant branches: see *Waiting on the maintainer*.
  - **(d) `fetch_jwks` sends none of the card fetch's headers: option (1), decided** (reply "a: agree, d: agree"; I read "d" as this). The key-set fetch sends `Accept` and only the headers set
    with the new `CardFetchOptions::with_jwks_header` (read back with `jwks_headers()`; `Debug` prints names only). `a2a card verify` sets `--header` for the key fetch exactly when it already
    decided to send it there. CHANGELOG `### Behaviour Changes`, the 0.15 to 0.16 upgrade guide and `book/src/deployment/provenance.md` say so; RFC 8725 section 3.10's row keeps `IMPLEMENTED_UNTESTED`
    (the URL allow-list half is the caller's, and no test sets `jku`, `x5u` or a header `jwk`). Hand mutants, baseline first and each file restored byte for byte: the fetch and the option, 7 of 7
    (baseline 8 passed in `jwks_fetch_tests`); `connect.rs`, 2 of 2 (baseline 8 passed in `card_origin`).
  - **CodeQL alerts 191 to 204 ("Log entry depends on a user-provided value", `otel/telemetry/prometheus/tests.rs`): dismissed as `Used in tests`** (option A). The maintainer ran the dismissal; the only check
    this session could make is that the 14 review threads read resolved. Comment used: "Test-only: this assertion message prints a local test exporter scrape. No production path and no external input."
- **2026-11-01, answers to `docs/audits/2026-11-01-triage/DECISIONS.md`** (all built or recorded as
  below; the forks are described there):
  - **F1, 0.16.0 breaks: all five.** `#[non_exhaustive]` on the generated wire structs (G-10),
    `tls-rustls` as a default feature of the server crate (G-06), `RequestContext.context_id` becomes
    `ContextId` (G-18), `AgentCard::new` defaults `streaming` to true (G-07), and `sign_agent_card` is
    removed (NEW-g-api-4). Each under Breaking Changes with a migration. No `**Cadence exception:**` line
    is needed for a 0.16.0 dated in November or later (0.15.0 is dated in October; the check compares
    calendar months, and this file said otherwise until 2026-11-02).
  - **F2, loopback and proxies: exempt loopback by default**, `A2A_PROXY_LOOPBACK=1` to opt back in, and
    Go's `NO_PROXY` dialect (B-23).
  - **F3, bad `OTEL_*` values: warn and fall back**; only an unusable endpoint URL still fails.
  - **F4, Production profile: refuse** a process-local halt, lease, push-config or audit store when the
    task store hands out claims, with an explicit opt-out (amends ADR 0025).
  - **F5, delegation on shutdown: detach and resume.** With a journal attached, shutdown detaches; the
    resume streak resets on any event; stream keep-alive for gRPC and WebSocket; a journaled
    delegation can be leased and budgeted.
  - **F7, one HTTP pool: build ADR 0032's opt-in `HttpPool` now** (not the "document the `Arc` pattern"
    position the triage recommended).
  - **F8, container image: publish a signed image to GHCR from `release.yml`** with provenance (written
    here, cannot be exercised here), beside fixing the README and manifest.
  - **F9, Agent Skills: an ADR now, a feature-gated module in 0.17.0**, no script execution.
  - **F10, steering a running agent: an ADR and the extension URI now, the build after the audit PR.**
  - **F12, swarm designs: only the four fixes** (H16, NEW-h-designs-1, -4, -7) in this PR; each design is
    its own ADR and later PR.
  - **F11, changelog: a highlights block (at most 15 lines) checked by `check_release_tree.py`, and
    `### Internal` moved out to `docs/changelog-internal/`**, as its own commit at the end.
  - **F6, patch behaviour policy: no work.** The maintainer's answer: "we are planning to release
    v0.16.0, v0.15.1 was never released." Read as: the `[0.15.1]` changelog section (dated 2026-10-26,
    never tagged) is not a release and its entries belong to 0.16.0's notes; no patch gate is built.
    That folding is not done yet (group P); say so to the maintainer if it was not what they meant.
  - **F13 to F15 were not asked;** the positions in `DECISIONS.md` stand until told otherwise (F13
    accept and document the window, F14 a bounded wait in the stream reader, F15 certificate reload
    after 0.16.0 and deprecate `tls_config_with_extra_roots` now).

- **2026-10-28:** replicas must work without session affinity, and the
  release is not ready until they do and the observability stack works out of
  the box; every check is to be run live (llama.cpp with current small Qwen
  and Gemma models, a real Collector, Prometheus, Grafana, trace and log
  backends). Supersedes the next entry.
- **2026-10-24 (superseded 2026-10-28):** multi-replica high availability is
  not a requirement for 0.15.0. It shipped as a documented limitation
  (CHANGELOG "Known Limitations", the book's horizontal-scaling page).
- **2026-10-08:** commits are authored as Tom F. with sign-off; release tags
  are signed with the maintainer's own SSH or GPG key and CI verifies them;
  new distribution channels (the CLI, Homebrew, winget) are drafted in-repo
  only, and the maintainer publishes. The Langfuse preset and every page
  default to a self-hosted Langfuse and lead with self-hosting; Cloud is an
  alternative only.
- **2026-10-14:** every audit-register recommendation approved as written;
  R49, the types generated from the proto (ADR 0016); R12 as a one-shot
  migration per store (`migrate_caller_keys`), not reading old keys for a
  release; R67 option (b) with 0.15.0 as the next release; OpenTelemetry as
  in Step 4; R68, follow the text.
- **2026-10-15:** R69 option (a), the store reports the log position a saved
  task reflects. Later the same day, for the next session's plan:
  `subscribe_max_idle` off by default, kept as an opt-in (§3.1.6); lag
  catch-up paging, so `streamLagged` remains only where the log no longer
  holds the events (§3.5.2); `application/a2a+json` for a request that states
  no preference, gated on `scripts/go_sdk_interop.sh` (§11.1); the card
  resolver honours `max-age` with an always-revalidate option and an opt-in
  default lifetime (§8.6, §8.6.2); the §5.7 and SLIMRPC error-mapping drafts
  re-verified and held; for 0.15.0, UCUM units, `a2a.server.latency` renamed
  `a2a.server.request.duration` with the old name removed, and
  `init_otlp_pipeline` taking an optional service name — not the single
  outcome counter; commits carry `Signed-off-by` only, and
  `CONTRIBUTING.md` follows. After the a2a-go check failed (its streaming
  calls name no JSON type and it reads only `application/json` errors), the
  narrowed §11.1 rule: a request asking only for an event stream keeps
  `application/json` errors. A part-level at-rest cipher hook (AES-256-GCM
  on message and artifact part payloads, structure left plaintext) for
  GDPR-25-1 and GDPR-32-1a.
- **2026-10-16:** the audit trail protects who and what its records name,
  with per-person keys that can be destroyed (crypto-shredding), amending
  ADR 0015 — chosen over keyed digests alone and over deferring it.
- **2026-10-18:** the OTLP exporter stays in `a2a-protocol-sdk` only; the
  WebSocket listener gets an opt-in `Origin` allowlist (built,
  `with_allowed_origins`); an unidentified caller is rate-limited by its
  peer address (built); a total-task cap across tenant partitions is an
  option (built, `TenantStoreConfig::with_max_total_tasks`), and the
  approval-gate refusal under caller isolation stays.
- **2026-10-20:** §3.5.2: build a per-task watermark so the in-memory event
  log never trims what an attached stream still owes (bounded by a hard
  ceiling), with an independent review. §12.4: SLIMRPC stays PARTIAL until
  agntcy-slim-rpc carries error details; no details in the message text.
  Tool calling moves into the SDK as a feature-gated module (ADR 0022),
  superseding the earlier examples-only decision below.
- **2026-10-21:** production-grade safety work for delegation at scale,
  each built with an ADR, tests that fail without it, and an independent
  review where it touches the store/stream contract. Provenance: receivers
  verify present signatures by default and refuse altered content, signing
  turns on when a key is configured, unsigned peers still interoperate
  (marked unverified), and the production profile requires a key.
  Deadlines and budgets: an extension carrying an absolute deadline and
  named meters (tokens, tool calls, cost in a named currency), decremented
  per hop, with the remainder visible before expensive work. Production
  profile: builder presets (`RequestHandlerBuilder::production()`,
  `ServeConfig::production()`) that refuse to start on plaintext off
  loopback, an undeclared binding, no authenticating interceptor, or
  delegation without leases and a halt store; defaults unchanged. Instant
  halt: the Postgres halt store pushes halts and cross-replica cancels
  with LISTEN/NOTIFY; SQLite and in-memory keep polling. Also: narrower
  credentials per hop through RFC 8693/8707, idempotency keys for tool
  side effects by default, a multi-agent prompt-injection harness, the
  tool-calling study published, kill-switch conformance tests, upstream
  drafts for deadline propagation and provenance (not filed without the
  maintainer), the three encryption verifications, and the removal of the
  0.10/0.12 deprecations in 0.15.0. Built differently from this record in
  two respects: ADR 0023's deadline is relative (`remainingMs`, as gRPC's
  `grpc-timeout`), not absolute, so clock skew between agents does not
  move it; each hop's remainder is what its parent reserved. And the
  handler preset is `with_profile(Profile::Production)`, not
  `RequestHandlerBuilder::production()`, because profiles are how the
  builder already applies presets (ADR 0025).
- **Earlier, still standing:** ~~tool calling stays in the examples, not the
  crates (`examples/rig-agent`)~~ (superseded 2026-10-20, above); the rig integration crate is held
  unpublished; the cargo-mutants patch is kept in-repo, not proposed
  upstream (`scripts/cargo-mutants/README.md`); a store that cannot isolate
  refuses a request naming a tenant (G3 option 2); caller isolation is on by
  default with an authenticating interceptor (2026-10-12); the store cap and
  the terminal gate were kept as they are (2026-10-03, G4 in
  `docs/swarm-orchestration.md`); `deploy-agent` runs on jemalloc.
- **Rejected designs worth not re-proposing:** the halt writing `Canceled`
  itself, and re-checking the halt after the row is written (both
  2026-10-09); for §13.1, a composite tenant string, a nested future per
  method and a general `TaskAuthorizer` hook. The reasons are in
  `handoff-history.md` under each date.

## Release gotchas

`RELEASING.md` is the procedure; these are what it cost to learn.

- Regenerate the provenance manifest last, at the release commit:
  `release.yml` checks it against the tag.
- After the content merge, wait for the benchmark bot's commit before
  preparing the release; `benchmarks.yml`'s guard covers only the window
  after preparation.
- The eight inter-crate version pins are not what `release.yml` checks; it
  reads the first `version` line. `RELEASING.md` §1 has the loop over every
  lockfile and `check_lockfiles.sh`.
- The maintainer has allowed release preparation inside the content pull
  request, most recently for 0.15.0 (2026-10-25); the manifest-last rule
  holds either way.

## Branches

Three kinds matter. The working branch, `claude/affectionate-gauss-5yn4f5` (*Now*, above; the 0.16.0 integration branch `claude/inspiring-mccarthy-rgpntj-rest` merged as #197). The storage branches
(`claude/adk-rust-0.12-patch`, `claude/a2a-rig-held`), both built against 0.12. And the branches of this work's six earlier pull
requests (#191 to #196, all merged: `claude/inspiring-mccarthy-rgpntj`, `-client`, `-client2`, `-server`, `-server2`,
`-wire-structs`). The ten `claude/inspiring-mccarthy-rgpntj-rest-<package>` branches and the two `-live` ones were an agent's or a
probe's worktree in one container and were never pushed; their content is in `main`, so there is nothing of them to
delete on the remote. Every other `claude/*` branch is merged or a pre-0.12 session branch; deleting them is the maintainer's
call. To check one:

```sh
git fetch origin && git merge-base --is-ancestor origin/<branch> origin/main && echo merged
```

Two follow-up branches were pushed on 2026-11-06 for work that was then held out of pull request #197 (the maintainer's instruction: push the branches, open no pull request, since a plain push to a
branch other than `main` starts no workflow): `claude/inspiring-mccarthy-rgpntj-304-merge` (the `304` header merge in the card cache, RFC 9111 section 4.3.4 and 3.2) and
`claude/inspiring-mccarthy-rgpntj-standards-ledger` (the RFC requirements ledger and what it found). The maintainer then asked for both before the tag, so #197's branch carries them (the `304` commit
rebased onto the flaky-test fix, the ledger commits); the two remote branches are redundant and were verified safe to delete, but the delete was refused by the remote (*Waiting on the maintainer*).

This clone is shallow, so a "no" for an old branch is not proof, and `git
blame` stops at the shallow boundary. Never write this branch's own head SHA
into this file: the commit that records it changes it.

## Environment

- **The live stack of 2026-10-28, rebuilt in minutes.** `dockerd &` starts a
  Docker daemon (the binary is installed; nothing starts it). Then:
  `docker compose -f deploy/observability/docker-compose.yml up -d`;
  PostgreSQL 17 with `docker run -d --name pg --network host -e
  POSTGRES_PASSWORD=a2a -e POSTGRES_USER=a2a -e POSTGRES_DB=a2a postgres:17`;
  llama.cpp with `docker run -d --name llama --network host -v
  /opt/models:/models:ro ghcr.io/ggml-org/llama.cpp:server -m
  /models/<gguf> --port 11434 --host 127.0.0.1 --jinja -c 16384 -np 2 -t 4`
  (its image healthcheck probes 8080 and reports unhealthy; `curl
  localhost:11434/health` is the real one). GGUFs from Hugging Face by
  `curl -L https://huggingface.co/<repo>/resolve/main/<file>` and checked
  against the API's `lfs.oid` (`unsloth/Qwen3.5-2B-GGUF`,
  `ggml-org/gemma-4-E2B-it-GGUF`). Docker Hub answers 429 when busy:
  pull the same digest from `mirror.gcr.io/library/...`. A `docker build`
  that fetches crates needs this sandbox's proxy: `--network host`,
  `--build-arg HTTPS_PROXY=$HTTPS_PROXY`, and the CA from `/root/.ccr`
  through `--build-context` (a scratch Dockerfile; never the repository's).
  GitHub release downloads were refused on 2026-10-28 and worked on 2026-11-03 (the SPIRE
  tarball, see `handoff-history.md`); `go install` through the module proxy works (kind was installed that way). Kubernetes does not run here
  (kind: nested `runc` cannot start a pod sandbox).
- **Live-testing pitfalls met in this sandbox** (2026-10-28): the kernel has
  no IPv6 (no `/proc/net/if_inet6`; an AF_INET6 socket fails with errno 97),
  so nothing IPv6 can be dialled. The sandbox's `NO_PROXY` covers 127.0.0.1
  and curl applies it even to `-x`, so check a local proxy with
  `--noproxy ''`, and start test processes under `env -i` with only the
  variables meant. Squid (`mirror.gcr.io/ubuntu/squid`) cannot write
  `/dev/stdout` as its `proxy` user: log to `/var/log/squid/`. A second
  Collector on host networking collides on its own telemetry port 8888;
  publish ports instead. `docker compose up` does not recreate a container
  whose bind-mounted config changed: `--force-recreate`. A Collector on
  `localhost:4318` makes `tests/otel_pipeline_default_endpoint.rs` fail
  (it needs that port free). A debug build of the whole server suite needs
  about 14 GB of disk; `CARGO_PROFILE_DEV_DEBUG=0` brings it under 4 GB.
- **A model's thinking costs minutes on four CPUs**: Qwen3.5 reasons by
  default, and an uncapped call generated over 5,000 tokens at about 6.5
  tokens/s. Every model-backed example now caps output and turns thinking
  off unless told otherwise; anything new must too.

- **Disk** is a fixed allowance: `CARGO_INCREMENTAL=0`, and delete
  `target/semver-checks` and the incremental directories when space runs
  low. A linker "bus error" (signal 7), or `linking with cc failed`,
  means the disk is full. `bindings/a2a-protocol-slimrpc/target` reaches
  12 GB for its full test suite: run it with `CARGO_PROFILE_DEV_DEBUG=0`
  and delete it afterwards. `cargo test --workspace` fills the allowance
  while linking (2026-10-24: three gates failed with `No space left on
  device`, which a failed test run reports as a compile error): run one
  package at a time and delete the executables in `target/debug/deps` between
  (`find target/debug/deps -maxdepth 1 -type f -perm -u+x ! -name '*.so' !
  -name '*.d' -delete`; 14.7 GB of 20 GB were test executables). When the
  harness prints "Disk full ... 0 MB free", its output is lost: free space
  first, then re-run (a command that prints nothing, not even a trailing
  `echo`, is that symptom). `target/debug/deps` also keeps every build of the
  workspace crates, one per feature set and profile: `cargo clean -p
  a2a-protocol-types -p a2a-protocol-client -p a2a-protocol-server -p
  a2a-protocol-sdk` removed 10.6 GiB of 172 stale builds of the server crate
  alone (2026-10-24) and costs only those crates' recompilation. Do not run
  a script that prunes `target/debug/incremental` while another `cargo` is
  building in the same tree: the build fails with `failed to move dependency
  graph`.
- **`check_file_lengths.sh` counts tracked files only:** run it after
  `git add`, or a new file over 500 lines passes unseen (it did:
  `tests/provenance_signing.rs`, 533 lines, at `69163dec`).
- **`cargo package --workspace` can verify against a stale copy of an unpublished crate** (2026-11-04). Cargo unpacks the crates it has just packaged into `~/.cargo/registry/src/-<hash>/<crate>-<version>` and reuses a directory of that name left by an earlier run, and it treats a registry crate's compiled build as immutable. After the tree changed, gate 84 failed with `no associated function 'new'` for seven constructors (the extracted types crate predated #192) until both were deleted: `rm -rf ~/.cargo/registry/src/-*/a2a-protocol-*-0.16.0`, then the build made from it (`grep -l '/registry/src/-' target/debug/deps/*.d` names it; remove its `deps/` files and its `.fingerprint/` directory). Deleting the source alone left the failure unchanged. CI's runners have neither.
- **PostgreSQL 16** is installed: `pg_ctlcluster 16 main start`, then
  `A2A_TEST_POSTGRES_URL=postgres://postgres:postgres@localhost:5432/postgres`.
  The `postgres` role's password is not always `postgres` in a fresh
  container (scram auth refused it on 2026-10-20): set it with
  `su postgres -c "psql -c \"ALTER USER postgres PASSWORD 'postgres'\""`.
- **llama.cpp, in a fresh container (2026-10-24, none was installed):**
  `git clone --depth 1 https://github.com/ggml-org/llama.cpp`, `cmake -S . -B
  build -G Ninja -DCMAKE_BUILD_TYPE=Release -DLLAMA_CURL=OFF -DGGML_NATIVE=ON
  -DLLAMA_BUILD_EXAMPLES=OFF -DLLAMA_BUILD_TESTS=OFF`, `cmake --build build
  --target llama-server -j2` (about 6 minutes on 4 vCPUs; built at commit
  `3f03b17`). github.com release pages answer 403 through the proxy;
  huggingface.co is reachable, e.g. `unsloth/Qwen3-1.7B-GGUF`
  `Qwen3-1.7B-Q4_K_M.gguf` (1.1 GB, 7 s). The examples default to
  `localhost:11434` and the name `qwen3:1.7b`: start the server with `--port
  11434 --alias qwen3:1.7b --jinja -c 8192`. Earlier: b11836 (`f5027fb`) at `/opt/llama/src/build/bin`, and
  the study's b11751 (`8466395`) at `/opt/llama/old`; GGUFs in
  `/opt/models` (hashes in the study). Use `-c 8192` (the default context
  sized the KV cache to ~4.6 GB and the OOM killer took the server), and
  send one request at a time to a server whose results must replay. A
  replay is host-dependent: the same binary and seed gave different
  answers on a 2.10 GHz and a 2.80 GHz host (`GGML_NATIVE`), which is
  what the study's uptime gap turned out to be.
- **A local OTLP collector breaks one test:**
  `otel_pipeline_default_endpoint` expects its flush to the default
  endpoint to fail, and fails whenever something listens on 4317. Stop the
  `deploy/observability/` stack before running the server suite.
- **The Docker daemon is not running at session start, but `dockerd` starts** (2026-11-03: Docker
  29.9.2); `itk/docker-compose.yml` and `acts` are not run by `preflight_all.py` for the reasons its
  inventory gives.
- `pkill -f` and `pgrep -f` match their own shell's command line, which
  contains the pattern: kill by PID, or put the command in a script file
  (cost a killed shell twice on 2026-10-20 and twice more on 2026-10-24).
- Agents in worktrees that share `target/` race on a crate's `.rlib`
  (`linking with cc failed` while the other build replaced it): re-run the
  step before reading anything into it.
- `cargo test --workspace` stops at the first failing test binary; use
  `--no-fail-fast`, or one package at a time when the disk allowance cannot
  hold every test binary at once.
- `cmd | tail` reports `tail`'s exit status; use `PIPESTATUS` or a file.
- Run `rustup update stable` and nightly clippy before trusting local clippy;
  CI's stable moves.
- Measure performance on one pinned core, medians of six interleaved runs.
- Run `scripts/preflight.sh --full` on the merged tree before pushing a
  release-relevant change, and `scripts/go_sdk_interop.sh` before changing
  HTTP+JSON headers.
- Mutation testing: read the logs, not the counts; reproduce a CI shard with
  the "How to Reproduce Locally" block `mutants.yml` prints.

## Lessons

- **Run the three gates that read other files after every commit, not once at the end (2026-11-12).** Three of this session's commits landed red on a gate their own tests did not exercise: the audit commit failed
  `cargo fmt --check` and `RUSTDOCFLAGS=-D warnings cargo doc` (a doc link in a file I had moved text out of), and a later one drifted 56 spec-ledger citations by moving a line in `builder.rs`.
  After any edit, **including to `CHANGELOG.md`, whose line numbers the ledger cites** (the dropped-send entry moved a citation and `ba335f90` landed with a failing ledger), run `cargo fmt --all -- --check`, `check_spec_ledger.py` and the `cargo doc` line in *Now*; a green test suite says none of them.
- **That lesson recurred, and three more came with it (2026-11-12).** (1) I extended the S1/S7 CHANGELOG entry after the ledger run, and the ledger failed at the next check (`6437c144` repairs it): run the gates
  as the *last* step before `git commit`, not before the final edit. (2) A test module that reaches 500 lines fails `check_file_lengths.sh`, and clippy's `too_many_lines` (60) applies to tests: split by seam
  (`run_tests/journal*.rs`, with `pub(super)` helpers) and extract a helper instead of allowing the lint. (3) The three S1/S7 mutant survivors were each a missing test, not dead logic (a resumed run's turn count; a
  receipt's `Started` write on a re-run; an approval for call one releasing call two of the same turn), so the first question for a survivor is which behaviour has no test, and only then whether the code is equivalent.
  To wait for a background command without polling, `tail --pid=<pid> -f /dev/null` blocks until it exits; `$?` after `cmd | tail` is `tail`'s status, not `cmd`'s.
- **A first CI run skips every step after its first failure, so run the whole job's step list before the first push (2026-11-12).** `ci.yml`'s Static checks has 31 gates after the RFC ledger; this session had run seven of the job's gates by name and CI found the rest. List them from the file
  (`yaml.safe_load` over `.github/workflows/ci.yml`, the job's `steps[*].run`) and run each, taking the exit status of the gate and not of a pipe (a scratch script that loops over the commands ran 28 gates in about a minute). Also run `cargo hack clippy -p a2a-protocol-types -p a2a-protocol-client -p a2a-protocol-server -p a2a-protocol-sdk --all-targets --each-feature -- -D warnings`
  (`cargo install cargo-hack --locked --version 0.6.46`; 61 combinations, about 25 minutes; add `--keep-going` to see every failure at once): a feature-gated `self` or import is clean under `--all-features`, which is all the earlier clippy runs used. `prove_workflow_gates_fail.py` needs `apt-get update && apt-get install -y openssh-client`
  (without the `update`, apt has no candidate); the crypto interop gate needs `pip install --require-hashes -r .github/requirements/cryptography.txt`. For a toolchain leg, `rustup toolchain install 1.94 --profile minimal` and `cargo +1.94 check --all-targets` into its own target directory.
- **Reproduce a mutation job's baseline before pushing (2026-11-12).** It costs about ten minutes and spares a CI cycle per hidden failure: `cargo install cargo-nextest --locked`, then
  `cargo nextest run -p a2a-protocol-server --cargo-profile mutants --all-features --run-ignored all --no-fail-fast -E '<the filter, read from mutants.yml and not retyped>'` with `A2A_TEST_POSTGRES_URL` set. `--no-fail-fast` is the point: CI's run stops at the first failure and hides the rest. The local cluster must match CI's:
  `ALTER USER postgres PASSWORD 'postgres'` and `ALTER SYSTEM SET ssl = off` (then `select pg_reload_conf()`), because `statement_count` counts statements on the wire and reads `(0, 0)` through TLS. A harness or live-model test that cannot run in that job belongs in the `-E` filter (four copies in `mutants.yml`).
- **A survivor list written before the run finishes is incomplete, and a script's `KILLED` is not yet a kill (2026-11-12).** I wrote the list from the 15 shards that had failed while 5 were still running; one of them (shard 36) held a sixteenth failure, found only because the run was re-read once it said `completed`. Re-read the run's `conclusion` and every shard's status before calling a list final. `apply_mutant.py` prints `KILLED` for *any* non-zero exit, a compile error included, and shows only the last lines of the output: for each mutant, read the names of the tests that failed (apply the line by hand, send the output to a file, `grep '^test .* FAILED'`).
  The tests written for survivors are test code too: `cargo clippy --all-targets -D warnings` caught two lints in them (`assert_is_empty`; a `MutexGuard` held across a loop body) that the test run never would. And the build directory is a budget: five `clippy` configurations and a `cargo hack` sweep filled the volume (0 MB free, every later step failed with `No space left on device`); use `CARGO_INCREMENTAL=0` and `cargo clean -p a2a-protocol-server` between configurations.
- **A TIMEOUT in a mutation run is a possible survivor, and a test that passes under the mutant may never reach the guard (2026-11-12).** The same mutant was TIMEOUT in one run (300 s of tests) and MISSED in the next (278 s): a timeout is a full suite that did not finish, not a hang that a test caught. Apply each timed-out mutant to the whole lib suite before calling it handled; five of six survived it. And a test written for a guard can pass under the mutant because the scenario never gets there: mine for the sync collector's fate guard passed with the guard forced `true` until a temporary `eprintln!` of the fold's result showed the store refused the write as `InvalidStateTransition`, not as the `Protocol` conflict the guard tests; the store's read counter (0 either way) was the cheaper proof, and a test of a guard should assert something the wrong branch changes.
- **Before a push that answers a mutation run, read every shard's log, the green ones too, and never restore a file with `git checkout` unless `git diff --quiet` says it is clean (2026-11-13).** A mutation job passes a shard that only has timeouts, so the survivors of one run are the timeouts of the one before; reading only the red shards cost three CI cycles (about six hours of runner time each). One `get_job_logs` call per shard returns a signed URL, and a loop of `curl`, `sed` and `grep '^MISSED \|^TIMEOUT '` over the 40 logs takes minutes. Restore a mutated file from a copy or with the script that checks the hash; `git checkout -- file` also discards any edit of yours in it.
- **A test that shares a database with the rest of the suite must not assume it is alone in it (2026-11-12).** A process that is killed half way (every `cargo test` I stopped for a mutant) leaves rows behind; the reaper takes the oldest 32 per look and the test's own task was the 33rd. Read the database's history before suspecting the code (`select ... group by date_trunc('minute', updated_at)` over what the reaper wrote showed 32 where a clean run writes 3), and reproduce the symptom on purpose before fixing it.
- **A red mutation job is not a list of survivors (2026-11-12).** All 41 server and client shards were red and the aggregator said "2 surviving mutants". The server shards had failed their *unmutated baseline* (`FAILED Unmutated baseline` in the log, a test that fails with no mutant at all); the 2 came from the client shard, which ran. The job runs `--run-ignored all`, so every `#[ignore]`d test runs under
  a debug profile with four jobs: a measurement harness belongs in the `-E` filter beside `soak` and `swarm_scale`. Read one shard's log for `Found N mutants`, then `FAILED`, `MISSED` or `caught`, before reading anything else. Download a job log by asking the tool for `return_content: false` and `curl` the signed URL into a file; the logs are 30,000 lines.
- **`git commit -s` signs as whoever `git config` says (2026-11-12).** The container's identity is `Claude <noreply@anthropic.com>`, and `scripts/check_dco.sh` refuses an assistant author. Commit with
  `git -c user.name="Tom F." -c user.email="tomf@tomtomtech.net" commit -s`, then `bash scripts/check_dco.sh <base> HEAD` before pushing.
- **A surviving hand mutant is often dead logic, and the fix is to delete it (2026-11-12).** Three survivors this session were equivalent: a guard that could never be false (`reviewed_requester` re-checking what admission had
  already refused), a default branch reached only after the case it handled was refused earlier, and a map-or default that an `&&` made irrelevant. Each was removed, not excused, and the code got shorter.
- **The disk allowance is not `df`'s free space (2026-11-12).** The container's writable allowance ran out three times at 14 to 3 GB "free". Delete `/tmp/claude-0/target/debug/incremental` and the test executables
  (`find debug/deps -maxdepth 1 -type f -perm -u+x ! -name '*.so' ! -name '*.d' -delete`) before a long run, build `book-tests` into a separate target directory and delete it (2.9 GB), and set
  `CARGO_PROFILE_DEV_DEBUG=0 CARGO_INCREMENTAL=0` for mutant runs, which build the crate once per mutant.
- **A real model found where a schema check's value is, and where the first live prompt proved nothing (2026-11-12).** Asked to transfer from "account 4417" against a schema wanting eight characters, Qwen3-1.7B padded the
  number to eight characters and passed the check, so the run exercised only the policy. A prompt that *conflicts* with the schema (an amount of 500 against a cap of 100) is what makes the model's own call the thing under
  test. A live test must assert what holds whatever the model does and print whether the case it was written for happened.

- **Hand mutants chosen by their author miss what `cargo mutants`' operators find (2026-11-11).** All 64 hand mutants of the H14 code were caught, and the first real incremental run on that code still
  left seven survivors in six of 40 shards: `||` to `&&` in two guards, `<` to `>` in a loop bound, `==` to `!=` in a log filter, a match guard replaced by `true`, a trait default's `false` to `true`, and
  `+=` to `-=`. Each was an operator I had not tried on a line I had not looked at. Hand mutants prove the tests written for the rules in mind; run `cargo mutants` on the files a branch adds, or the
  CI shard that holds them, before the pull request. Two of the seven needed more than a test: `settled += 1` on a counter only compared with zero is an equivalent mutant (refactored to a flag), and a
  match arm nothing reaches today (release an ending when the terminal was already pushed) was tested by setting the processor's state by hand.
- **A time margin in a test fails on a slower runner than yours (2026-11-11).** `reaper::…::finish` claimed one task with a 1 ms expiry against a one-second staleness margin and asserted the exact
  count of tasks finished by a second look; it passed everywhere until a mutation shard's baseline took 2.9 s and `recent` had really gone stale (`left: 2, right: 1`). A stall of the margin's length
  before the assertion reproduces it, on SQLite and PostgreSQL alike. Assert what holds at any speed (the count rises by one exactly if the log says that task was ended), not the number a fast run gives.

- **A claim that expired can always be taken again, so "the claim is released" cannot be tested by taking it (2026-11-11).** My first test of the reaper's finish phase asserted release by
  claiming the task as another owner, which succeeds on a stale claim whether or not the reaper released it; five of 17 hand mutants survived, each a state dropped from the SQL or a join column.
  Read the row (`SELECT count(*) FROM a2a_task_claims WHERE task_id = ...`), and give a tenant-aware case the same task id in two tenants.
- **A test that runs a reaper shares the claims table with every other test's reaper (2026-11-11).** It passed alone and three times in a row, and failed in the full parallel run: another test's
  reaper took its stale claims. Count-tolerance did not help (the other reaper also decides who pushes). A schema of its own (`CREATE SCHEMA`, `options=-c search_path=...` in the URL, dropped after)
  did: 12 of 12 stress iterations beside the other reaper tests.
- **callgrind counts a `rep movsb` copy per byte, so it over-states a large `memcpy` (2026-11-11).** Boxing two 36,000-byte futures cut instructions per request from 604,000 to 277,000 and the
  server's CPU per request by 5% to 7%. Use the instruction count to find what is copied, and the interleaved timing to say what it was worth. `strace -c` per request and `strace -k` with
  `-C force-frame-pointers=yes` found what neither did: 2.2 wake-up writes per request against 0.1.
- **A bisect needs the noise measured first (2026-11-11).** The control build swung 3.0k to 6.2k requests/s between steps on this host, a 29% total fall was spread over many commits, and the
  bisect landed on a commit that only moved tests. Three interleaved repetitions per step cannot resolve a 10% step on a shared 4-vCPU machine.
- **Reversing a hand mutant by replacing its `new` text can hit legitimate code (2026-11-11).** Cleaning up after two drivers ran at once, it rewrote the original `REAPED` line to `FINISHED`. Restore
  from a byte-for-byte backup, then check `git diff` shows no removed lines.

- **A cache test against a server whose `Date` has one-second resolution needs a margin over one second, or a `Date` ahead of now (2026-11-09).** The resolver ages a response by
  `received_at - Date` (RFC 9111 §4.2.3), so a response arrives up to a second old, and a case that expects a card to be fresh with a second to spare fails on about one run in three
  hundred under load and more often on a slow runner (`a_zero_draw_leaves_the_lifetime_whole`: 2 of 600 under 12 busy loops, and a Windows job in CI). A `Date` ahead of now is an age of
  zero; one 1.5 s old makes the failure certain, which is how to prove the cause. Measure a suspected flake with `cargo nextest run --stress-count N` under busy loops
  (`(while :; do :; done) &`, 12 of them on 4 vCPUs) and prove the fix with more runs than found it.
- **The first CI run finds what local parity cannot, and a push cancels the run (2026-11-08).** Per-package tests, clippy and the Windows cross-clippy were green, and CI's
  first pass still found three defects: a gate that scans file text (`run_postgres_suites.py` takes a doc comment naming the URL variable for a PostgreSQL suite), a 30 ms claim
  time to live that a stall can lapse, and two tests that assume Unix paths (`PathBuf::push` onto a `\\?\` path removes `..`). Run `scripts/run_postgres_suites.py` against a
  local PostgreSQL before the push, and give a test claim a time to live far above a round trip. CI cancels the running run on every push (`cancel-in-progress`), a Windows job
  stops at its first failing step and then takes about 100 minutes once it passes, and macOS about 90: wait for all jobs, fix everything found, push once.
- **A scanner's sanitizer is a specific syntax, so read its query source instead of guessing (2026-11-08).** `re.sub` on a header value and a `re.fullmatch` guard on `self.path` did
  not clear CodeQL's alerts; its library models only `x.replace("\n", ...)` for header injection and a guard on a local variable for partial SSRF. A downloaded CodeQL bundle can be read
  as text (`qlpacks/codeql/python-all/*/semmle/python/security/dataflow/*Customizations.qll`); running it was refused by this session's permission classifier.
- **A full disk makes gates fail and cargo-mutants die without saying so (2026-11-08).** At 0 MB free, four static gates exited 1 for no reason
  and a mutation run died at its baseline with "failed printing to stdout". Neither named the disk. `df -h /home/user` before and after any large
  run, a watchdog that stops cargo-mutants below 2 GB (it restores the mutated file on SIGTERM; a kill -9 would not), and delete test
  executables, not target directories, to get space back (free space went from 0 to 14 GB after deleting the executables under two build
  directories: `find target/debug/deps -maxdepth 1 -type f -perm -u+x ! -name '*.so' ! -name '*.d' -delete`). The mutation build directory
  alone was 9.2 GB (`du -sh`).
- **A merged tree needs the CI jobs nobody ran, not only the crate's tests (2026-11-08).** Per-crate nextest and clippy were green on every
  agent's branch and on the merge. The doc job, the Windows cross-clippy and the file-length gate then found six defects in an hour
  (three rustdoc errors, a `const fn` and an import that only Windows' clippy saw, a test file at 516 lines). Run `ci.yml`'s `doc`, `static-checks`
  and a Windows `cargo clippy --target` before the push, not after the first red run.
- **Leftover agent worktrees under `.claude/worktrees/` change what the gates see.** `check_gate_reachability.py` read their `benches/` as
  the repository's and reported 14 findings that a clean clone does not have. Remove merged worktrees first (check `git merge-base
  --is-ancestor <branch> HEAD` and `git -C <worktree> status --short` before deleting any).
- **A flaky test's counts do not name its cause; a timeline does (2026-11-08).** `log_wake_e2e` failed with `notifications=0`, which fits a
  late listener, a lost notification and an early release equally. Stamping each wake and the release showed `released@7585ms` equal to
  `timeout@7585ms` and a resync at 0 ms: the listener was up, the release was early. The first resync wake takes up one of the stream's naps,
  so "four timeouts" meant the fourth had just ended. A rate of about 1.5% needed 137 instrumented runs to see twice.
- **`cargo mutants --examine-re` with `--iterate` on a reused output directory mixes runs.** The directory held mutants from two earlier
  invocations; the "21 of 21" it printed was the directory, not the list. Read `outcomes.json` for the names asked about, or use a fresh
  `--output`.

- **CI's mutation job builds without `-D warnings`, and a local run with it miscounts "unviable" (2026-11-07).** The handoff's advice to
  build with CI's flags is right for clippy and tests and wrong for `cargo mutants`: `mutants.yml` sets no `RUSTFLAGS`. Under
  `-D warnings` a mutant that leaves a parameter or variable unused (`fn -> ()`, `-> None`, a replaced body) fails to build and is
  reported unviable; in CI it builds and has to be killed. A first local run on this session's diff read "8 caught, 31 unviable, 0 missed"
  for the server and "12 caught, 25 unviable, 0 missed" for the client; only a re-run of the unviable ones without `RUSTFLAGS` says
  whether those 56 were killable. Run cargo-mutants with `RUSTFLAGS` unset and its own `CARGO_TARGET_DIR` (the dependencies rebuild,
  about 4 GB), restrict the tests after `--` only to the relevant library tests and binaries, and list the unviable set with
  `cargo mutants --list --in-diff ...` first: a `const fn` or a type with no `Default` is unviable either way, anything else is not.
  `--profile=mutants` builds into `target/mutants`, which held 9.3 GB here after three runs: delete it when the runs are done (a disk
  at 3 GB free with every agent building was that directory). Also: `--in-place` cannot be combined with `--jobs`; a script that prints `$?` after a `$(date)` in the same `echo` prints 0 (it
  reported a failed cargo-mutants run as `EXIT 0`; read the log, not the status line).

- **A deserializer that returns data nobody reads has mutants no test can see, and a refusal by parse error looks the same as a refusal by rule.** `JwtHeader.crit` was an `Option<serde_json::Value>` whose only use was `.is_some()`;
  mutation run 470 replaced the deserializing function by `Ok(None)` and by `Ok(Some(Default::default()))` and both survived, because a `deserialize_with` function that does not read its value makes serde_json fail the header,
  and a header that does not parse is refused, which is what every validator-level test asserted. The cure was to keep only what is used (a `bool`) and to test the header's own deserialization, not only the verdict on a token.
  When a mutant of a helper survives every end-to-end test, check whether the end-to-end verdict has a second path to the same answer.
- **A commit that says "Not run: clippy" has not been checked, and clippy found three things in it.** The JWKS change passed its tests and 17 hand mutants and still failed `clippy::similar_names`,
  `clippy::unnecessary_wraps` and `clippy::too_many_lines` (`JwtValidator::validate` at 65 of 60) in the first CI-scope clippy gate run on it. The cure for the last was a method (`VerifyKey::verifies`),
  not an `#[allow]`. Run gate 46 before the mutants, not after; the mutants then have to be re-run on the bytes that changed (they were: 14 of 14 on the final file).
- **A gate that fails with `No space left on device` is the quota, and a stage of three test builds can spend it.** Nextest gates 65, 66 and 74 and a three-gate stage (68 to 70) failed while
  linking, with the volume at 0 MB (the harness then could not store command output). Sweep (idle test executables older than a few minutes, the workspace crates' rlibs), then run one gate at a time with a
  sweep between them; re-running the same tree is justified for that failure, and only for it. The log's first `error` line says which kind of failure it is.
- **`git commit --fixup=amend:<sha>` cannot take `-F`.** To rewrite an earlier local commit's message with a long body without an editor, write the subject `amend! <the target's subject>`, a blank line and
  the new message into a file, commit it with `-F`, and run `GIT_SEQUENCE_EDITOR=true git rebase -i --autosquash <base>`; `git diff ORIG_HEAD HEAD` (and the two tree hashes) then show that the rebase changed no content.
- **A test that dates a response with `Date` has a second less margin than it says.** `Date` has one-second resolution, so a response generated 0.99 s after a
  whole second reads as dated that second and its apparent age is up to a second. Three stale-if-error tests stepped a 60 s, 30 s and 20 s window to one second short
  of its end (59, 29 and 19 s) and failed about once in a few hundred runs: 399 of 400 stress iterations passed before the fix (`the_configured_window_serves_the_stored_card_on_a_503`
  the failure), 1200 of 1200 after it moved to a two-second margin. `a_response_that_arrives_stale_has_its_window_counted_from_arrival` pins the arithmetic with a
  `Date` header two seconds old, so no test depends on the clock's phase. Measure a suspected flake with `cargo nextest run --stress-count N`, not by re-running until green.
- **A test that releases the thing it is checking before the thing can happen cannot fail.** `the_bundled_sender_holds_the_address_it_connects_to` started the second
  delivery and released the first without yielding, so the second always arrived after the release, held or not, and `literal_ip -> None` survived (run 468). It now
  waits 500 ms and requires that the second request has not arrived while the first holds the address; that wait costs 0.5 s in the correct code, and a mutant that
  drops the hold is killed by it (100 of 100 stress iterations pass). When a mutant survives a test that "obviously covers" it, look for the step that lets the mutant's
  behaviour happen before the assertion.
- **The disk quota, not `df`, ends a session, and one big stage can spend it.** A 14-gate clippy stage run after the mutant drivers had left their test executables and
  per-worktree builds in the shared target directory ran the allowance to 0 MB (the harness then could not store command output at all). Delete what rebuilds
  (idle test executables, `target/debug/incremental`, workspace `liba2a_*` files) rather than sources, and run gates in small stages with a sweep between them
  (a small script that deletes idle test executables and the workspace crates' rlibs, all of which rebuild).
- **A hand mutant is a kill only if the test passes without it.** A test that fails on every run "kills" every mutant. Run the unmutated tree first, in the same
  command as the mutant, and count the kill only when that baseline passed; then run the whole suite before the commit is pushed, which would also have caught
  it. The case is in `handoff-history.md`, last section, under *Withdrawn or corrected* (the outbox warning test, 2026-11-06).
- **Warnings arrive in code order, not in the order a test's story tells them.** A failed push delivery warns "push notification delivery failed" before the
  outbox warns that it could not be updated; a test that reads "the first new warning" sees the wrong one. Wait for the message you mean, among the new ones.
- **`127.0.0.2` is loopback on Linux only.** A push test used it as "an address that is down" and passed
  on Linux; on macOS nothing configures it, so the connect waited out the request timeout and the macOS
  job of #186 failed with "timed out after 2s" (not "refused"). A test that needs a fast refusal on every
  platform uses `::1` with nothing listening on it (`down_address` in `push/sender_pin_tests.rs`). The
  sandbox cannot reproduce a black-holed address (it rejects at once), so the fix was verified on Linux
  and confirmed by the CI re-run, not reproduced. A second test, `tests/pinned_endpoints.rs`, bound a replica on it and failed both
  macOS legs of the 0.16.0 CI (2026-11-05; the lesson of that day below).
- **Check a rule in a planning document against the gate that enforces it.** `DECISIONS.md` said a
  November 0.16.0 needed a `**Cadence exception:**` line because 0.15.0 was breaking in October; the gate
  (`check_release_tree.py`, `check_cadence`) compares the calendar months of the two release dates, so a
  November release needs none. The wrong line had been copied into three documents and a to-do list; it
  was found by reading the gate before drafting the line, 2026-11-02.
- **A sandbox run that fails oddly may be a full disk.** `cancel_keeps_concurrent_writes sqlite`
  failed once in a full run while `df` read 100%, and passed 3 of 3 alone and in the next full run. The
  target directory is about 8 GB and each side manifest (`fuzz/`, `itk/`, `bindings/`) adds 300-700 MB;
  `cargo clean -p a2a-protocol-server -p a2a-protocol-sdk`, `rm -rf target/nextest` and removing the side
  `target/` directories gets it back. Check `df` before blaming a test.
- **A codemod over hundreds of sites needs a fixpoint, a path allowlist and a comment rule.**
  Nested literals are not reached by one pass; `pb::Message` (prost) and a different SDK's harness matched
  the type names and had to be kept out; comments inside a literal vanish unless carried to the new call.
  Run it to zero conversions, then let the compiler list what is left. See the G-10 commit message.
- **A liveness check that compares a clock with the interval is decided by
  timer jitter** (found 2026-11-01 by a test that failed 22 of 40 runs on an
  idle machine). The WebSocket client's first keepalive asked "has the idle
  time reached the interval?" at each tick; right after a pong the idle time
  was 99 ms against a 100 ms interval, so that tick sent no ping and the next
  saw 200 ms and closed a live connection. At the 15 s default the same thing
  would have dropped healthy idle connections about every other timer run. It
  now counts quiet ticks (`Keepalive`, `transport/websocket.rs`): a ping on the
  first, close on the second, any frame resets. Count events, do not compare a
  duration with the period that produces the sample.
- **Restoring a mutated file by `shutil.move` of a backup leaves cargo with a
  stale binary.** The backup was copied before the mutant was written, so the
  restored file's modification time is older than the mutant's build output, and
  cargo does not rebuild: the next test run still contains the last mutant
  (`a2a_extensions_is_not_reported_as_a_dropped_header` failed with left 1,
  right 0 on correct source). Restore by writing the content back, or `touch`
  the file, and treat any failure that follows a mutant run as stale until a
  rebuild shows it.
- **A scoped tracing subscriber is not enough to capture a log line in a
  parallel test binary** (root cause of the 1-in-20 `log_redaction` flake,
  found 2026-10-24). `tracing-core` 0.1.36 keeps `has_just_one` true while
  one dispatcher is registered and then computes a callsite's interest from
  the first-reaching thread's own default, so another test's thread, with no
  subscriber, reaching the same `warn!` first makes it "never interested" for
  every thread. Two threads and two barriers reproduce it every time; 160
  runs of the real test did not, so repetition cannot find it. Capture
  through `crate::handler::warnings_during` or `warning_lines_during` (one
  global subscriber), never `with_default`, in a test that shares a binary.


- A mutation run's "caught" is only as good as the test that failed. Read
  the per-mutant logs in the shard artifacts (`mutation-report-*`,
  `outcomes.json` and `log/`): on 2026-10-22 an assertion-free `#[ignore]`
  measurement timing out at nextest's 45s kill had scored 245 of 610 server
  catches, so each labelled run surfaced survivors the last had scored
  caught (`640773fc`). A slow test that asserts nothing belongs in the
  mutation filter beside the soak binaries.
- A test passing and `cargo semver-checks` passing do not show a serde
  change preserves behaviour; parse fixed inputs with both builds.
- After code moves, re-point ledger citations with `--fix-lines` and
  check by hand every one it cannot place: the checker knows where a
  symbol is defined, not which of two same-named impls a citation meant.
- Count before writing a number into a document, and say what was counted.
- A test that reads spans through a thread-local subscriber needs a test
  binary of its own when other tests in its binary reach the same callsite
  with none: `tracing` caches interest globally. Rebuilding the interest
  cache was not enough (`tests/card_span_over_tls.rs`, 2026-10-23: 9 of 20
  runs failed beside the other TLS tests, 20 of 20 passed alone).
- Look at a dashboard before showing it to anyone. A short-lived process
  exports each series once or twice, so `rate()` over it tapers to nothing,
  and a process started without its `OTEL_*` environment exports nothing at
  all; both read as an empty or dying panel. Feed a screenshot from a
  long-lived process, confirm its series in Prometheus first
  (`count by (job, instance)`), and wait a full window (2026-10-24: the
  first capture had both faults, and a client legend that exposed the
  WebSocket transport's missing `server.address`).
- A `compile_fail` doctest needs a control that compiles.
- A re-run after a fix must cover the binary that failed, not only the files
  edited.
- An independent review before committing found two defects in R69 that the
  change's own tests and 23 hand mutants did not; budget for one on any
  change to the stream or store contract. It did again on 2026-10-23: a
  per-reader §3.1.2 fold passed its tests and broke §3.5.2 on resume; the
  writer fold that replaced it was reviewed the same way, and the review's
  findings (a failed write still setting the fold's state, the agent span
  ending at a mid-task message, the audit record losing the message id, a
  revert leaving `history: []`) were fixed with a test each.
- The workspace suite and the all-features server suite in one target
  directory exceed this container's disk allowance; delete `target/debug`
  between them (2026-10-23). Building with `CARGO_INCREMENTAL=0
  CARGO_PROFILE_DEV_DEBUG=line-tables-only
  CARGO_PROFILE_TEST_DEBUG=line-tables-only` keeps the server's ~180 test
  binaries well under it; full debuginfo filled it at 14 GB of `deps`.
- Run every clippy invocation CI runs, not only `--all-features`: a type
  used by an always-compiled store from a module gated on `sqlite` built
  with all features and nowhere else (2026-10-18).
  The 13 named invocations are not all of it: `cargo hack clippy
  --each-feature` (57 runs) and `cargo doc -p <crate>` per crate, outside
  the workspace's feature unification, caught a dead function and a link to
  a `signing`-only method on pull request #178 that every named run passed
  (2026-10-22).
- An end-to-end test of a new client helper found a server bug the
  server's own tests missed: after a blocking send answered at
  `AUTH_REQUIRED`, nothing stored the rest of the run. Test the path a
  client takes, not only the one a handler test takes.
- A cargo-mutants run spends almost all its time building: link with `lld`
  (`RUSTFLAGS="-C link-arg=-fuse-ld=lld"`), pass `--test` filters through
  `--`, run it detached from a pinned worktree, and resume with
  `--iterate`.
- An `--in-place` mutation run, or a hand mutant, interrupted by a
  container restart leaves the mutant in the source (2026-10-19): diff
  against `HEAD` before anything else, and never commit while one runs.
- A change to a crate's default features changes what `itk/` and `fuzz/`
  resolve, though neither manifest changes: run `scripts/check_lockfiles.sh`
  before a pull request. On 2026-10-19 `otel` by default left both stale,
  and the ITK harness's `cargo build --locked` failed every scenario.

- tokio 1.54.0 starts an accepted socket readable (#8435), so a mock that
  read only the request head and answered left the body unread; five push
  sender mocks failed until `tests/support/raw_http.rs` read the whole
  request. Its signal driver's panic text also changed ("IO is disabled").
- One example dependency turning on `serde_json/preserve_order`
  (genai 0.7) changes every crate built with it: map iteration order (the
  card ETag, a lint's output) and type sizes (`A2aError` past clippy's
  `result_large_err`). Sort explicitly where order is a contract.
- An idle timeout that counts bytes cuts a request the server is still
  answering: a blocking `SendMessage` is silent while its executor works.
  Found only by driving a slow live model through the SDK (2026-10-20).
- A test that passes with and without the code it is meant to guard is
  not coverage: prove each new test fails without its fix, and drop one
  that cannot.

- A line remap of ledger or control-map citations is applied once, to
  files restored from `HEAD`: run on an already-remapped tree it shifts
  every moved citation again, and the checkers, which check names not line
  text, pass it (2026-10-22).
- Long work in this container dies with it: results kept in scratch space
  are lost on a restart. Commit raw results as they arrive, and make any
  long driver resumable and check that its server is ready before each
  chunk (a restart script that did not filled two cells with failed runs
  in seconds, 2026-10-22).

- A subagent's "fails at HEAD too" is a claim about the environment as
  much as the code: two agents reported `otel_pipeline_default_endpoint`
  as pre-existing; it failed because a collector of mine held port 4317
  (2026-10-23).
- An agent's report written as an untracked file in its worktree is
  deleted with the worktree: copy what is not yet applied before
  `git worktree remove --force`.
- A defect-injection script that restores a file from a backup restores
  its old modification time too, older than the build, so Cargo reuses the
  binary built with the defect: the next "green" or "red" run is about the
  defect, not the code. Touch every restored file (`os.utime`), or edit the
  crate, before trusting a run after one (2026-10-23).
- A stricter client breaks the repository's own fixtures outside the
  crate it changed: after WP-B, the CLI's and the MCP bridge's tests
  served cards without skills. Run the whole workspace, not the crates a
  package touched, before merging it.
- Crates outside the workspace (`bindings/a2a-protocol-slimrpc`, `itk`, `fuzz`,
  `benches/sdk-comparison/harness/*`, `scripts/live/replica`, the wasm spike) are
  reached by none of `cargo fmt --all`, `cargo clippy --workspace` or nextest. A
  codemod over the workspace leaves them unformatted and their old assertions
  unrun, and the first CI run is where it shows. After one, run each manifest's
  own commands: the SLIMRPC job is `cargo fmt --check`, `cargo clippy
  --all-targets -- -D warnings`, `cargo test --tests`, run from the binding;
  `fuzz` compiles only under `RUSTFLAGS="--cfg fuzzing"` (2026-11-02).
- My local gate list (`gates.sh`, 36 scripts) was written from memory and is not
  CI's: "Static checks" has 46 steps, and `prove_workflow_gates_fail.py` was not
  among the ones I ran. Build the list from the job, with `yaml.safe_load` over
  `.github/workflows/ci.yml` and every `run:` of the job, and treat a step that
  needs a binary the container lacks (`ssh-keygen`) as not run, not as passed.
  A new `if`/`run` step in a workflow that can decide to fail needs a Probe or
  an Exempt in `scripts/prove_workflow_gates_fail.py` (2026-11-02).
- The incremental mutation run uses nextest, which does not run doctests: a public
  helper tested only by its doctest (`webhook::delivery_id`) survives every mutant.
  Give such a helper unit tests, and write the assertion as tight as the behaviour:
  a bound checked with `>=` and `<` around it let `<` to `<=` through. Under
  `start_paused` the elapsed time is exact, so assert it exactly. To list a mutation
  run's failed shards cheaply, ask `get_job_logs` for `failed_only` with
  `tail_lines: 1`; the verdict is the `MISSED` and `N mutants tested` lines of each
  shard's `cargo-mutants execution log` group (2026-11-02).
- Every `Debug` impl a branch adds is a mutant (`replace fmt with
  Ok(Default::default())`) that survives unless a test prints it, and a run reports
  them one shard at a time. List them from the diff before the run
  (`git diff origin/main...HEAD | grep -E '^\+.*impl.*Debug for'`) and test each,
  asserting the exact string, including what it leaves out (2026-11-02).
- A `#[cfg(test)]` function that repeats a production function's steps lets the
  tests pass while the production conditional goes unexercised:
  `SyncCollector::collect_events` (test-only) and `collect_then_follow` (what a
  blocking send runs) both had `if !interrupted { close_out }`, only the copy was
  tested, and deleting the `!` from the real one survived. Test the function the
  product calls, or make the test wrapper call it (2026-11-02).
- A feature-gated test file built without its feature holds no tests, and running
  its binary by name exits 0: a repeat-run measurement of it "passes" every time.
  Before counting failures, list the binary (`$BIN --list | grep -c ': test$'`) and
  build with the feature the file is gated on (`content_verification` needs
  `--features signing`); I reported 1,940 passing runs of a binary with 0 tests
  before I checked (2026-11-02).
- A test that forces an interleaving by firing on "the next read" breaks when
  production code gains a read. The in-memory store hid it (its reads never
  yield); SQLite showed it in 17 of 200 runs. Fire on the write that has one
  author, and measure a flake as N separate-process runs on the branch against
  N on main before deciding whose fault it is (2026-11-02).

- **CI's scope is wider than a Linux workspace build, and each gap cost a CI cycle (2026-11-05).** Run 1811 failed on five things the
  local clippy, gates and nextest had passed. What covers each, now:
  - *Windows and macOS.* A helper only a Linux function called failed `-D warnings` on both. `rustup target add x86_64-pc-windows-gnu`
    and `apt-get install -y gcc-mingw-w64-x86-64`, then `cargo clippy --workspace --all-targets --target x86_64-pc-windows-gnu --
    -D warnings` (about 4 minutes incremental) reproduces that class; it also type-checks `cfg(windows)` code and the tests. macOS cannot
    be cross-checked here: `ring`'s C build needs Apple's SDK headers. Windows and macOS *tests* ran for the first time in run 1812,
    because 1811 never got past compile; do not read an earlier green as having covered them.
  - *An all-features doc build.* The `Documentation` job runs `cargo doc --workspace --no-deps`, which no member turns `otel-prometheus`
    on for, so a broken link in that feature's docs was invisible. `cargo doc --no-deps --all-features -p a2a-protocol-types -p
    a2a-protocol-client -p a2a-protocol-server -p a2a-protocol-sdk`, with `RUSTDOCFLAGS="--cfg docsrs -D warnings"` on nightly (the
    `Nightly` job and `docs.yml`) and plain `-D warnings` on stable.
  - *The out-of-workspace crates.* `bindings/a2a-protocol-slimrpc` has its own lockfile and CI job (fmt, clippy `--all-targets`, build
    `slim-node`, `cargo test -- --test-threads=1`); a workspace run never builds it, and a behaviour the observability package changed
    on purpose was still asserted there. Give it its own `CARGO_TARGET_DIR`; it is about 4.4 GB.
  - *Nightly clippy.* `cargo +nightly clippy --workspace --all-targets --all-features -- -D warnings` (`rustup toolchain install nightly
    --profile minimal -c clippy -c rust-docs`; its own target dir). The job is informational, but a lint it reports reaches stable:
    four `future_not_send` errors on functions returning an `impl Future` that wraps another one were the first sign. Nightly
    `cargo test` stops at the first failing test binary, so its later binaries are unseen until that one passes.
  - *A name can be more than one address.* `DnsEndpoints` pins every address of a name; a test that serves on `127.0.0.1` and names
    `localhost` fails wherever `localhost` also resolves to `::1`. Resolve the name the way the code does and serve on every address.
    This kernel has no IPv6, so a changed `/etc/hosts` (`127.0.0.2 localhost` before the original line, restored and checked by md5)
    stands in for the second address.
- **`127.0.0.2` again, and a limit someone wrote down is a CI failure someone has not met yet (2026-11-05).** The lesson at the top of
  this section existed; the client-balancing package's test for pinned endpoints bound a replica on `127.0.0.2` anyway, and its own
  notes said "not checked on Windows or macOS". Both macOS legs failed at that `bind` (`AddrNotAvailable`, os error 49). A test that
  needs two addresses behind one name uses two ports of `127.0.0.1`: a pin is a `SocketAddr`. A "not checked on platform X" note on
  a test is a reason to fix or gate it before the push, not to record it.
- **A package that passes its own gates can still fail the merge, and a report is data (2026-11-05).** Seven failures in the first
  runs of the merged tree were each a pair of packages that were right alone: a test that pinned a `Debug` suffix another package
  extended, an end-to-end check of a status one package changed on purpose, a file two packages pushed over 500 lines together. An
  agent's report listed two of these as "pre-existing" (they were fixed on the integration branch already) and called a behaviour
  change new that the base already had. Check a "pre-existing" against the base commit, and a "new behaviour" against the code it
  replaced, before it reaches the CHANGELOG.
- **`ci.yml` cancels the run in flight when a new one is dispatched on the same ref (2026-11-05).** `concurrency` has
  `cancel-in-progress` for every ref but `main`. A push to the branch starts nothing (the workflow runs on `main` pushes and pull
  requests); `workflow_dispatch` does, and takes the running run's unfinished jobs with it. Wait for the run to end before
  dispatching, and push fixes together.
- **Go: `go build ./...` in a module with one `main` package writes the binary into the module directory.** Delete it before committing
  (`itk/interop/go-sdk-client/a2a-itk-go-sdk-client`). The image's Go is 1.24.7 and the ITK modules need 1.26.10;
  `GOTOOLCHAIN=auto` fetches it (about 70 MB), with `GOCACHE`, `GOMODCACHE` and `GOPATH` pointed at the scratchpad so nothing is left.

## History

[`handoff-history.md`](handoff-history.md) holds the file as it stood on
2026-10-15, before this rewrite, verbatim: every branch section from
`claude/relaxed-planck-c4hsn0` to `claude/funny-heisenberg-e6tl83`, the
observability review, the future ideas (Part B, "What not to chase"), and
each branch's verification of record. Each branch's merged content is also in
`CHANGELOG.md` under its release, in its pull request, and in its commits.
