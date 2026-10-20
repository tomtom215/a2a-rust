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

- **Released:** 0.14.1 (tag `v0.14.1`, 2026-09-30) is the newest release. The
  four crates are at 0.15.0 and the SLIMRPC binding at 0.7.0, all unreleased;
  `CHANGELOG.md` is `[Unreleased]`.
- **Working branch:** `claude/charming-lamport-yp20d6`, started 2026-10-18
  from `main` at `86c8fac` (pull request #172 merged). Pull request #173
  is open for it (2026-10-19). `git log 86c8fac..` lists its commits; each message says what
  it changed and how it was verified.
- **The spec ledger:** `python3 -I scripts/check_spec_ledger.py` is the
  authority on the counts. Since 2026-10-18 it also refuses `SUPPORTED` or
  `NOT_SUPPORTED` on anything but a MAY or OPTIONAL: 23 entries had used it
  for a MUST or SHOULD, 12 of them now `TESTED` and 11 `APPLICATION` (each
  note says which hook the SDK gives). Three entries stay PARTIAL, each
  short of a MUST for a reason outside this repository: §3.5.2 (a lag
  beyond the in-memory log's 512 events), §5.7 (the specification
  contradicts itself on an empty `ListTasksResponse.tasks`) and §12.4
  (agntcy-slim-rpc carries no error details). §5.4 and §11.1 closed on
  2026-10-18: the SLIMRPC README has the full error table, and §11.1's one
  exception is a stated, valid reason to set aside a SHOULD (a2a-go). Every
  NOT_APPLICABLE and APPLICATION entry records a 2026-10-18 review.
- **Ledger citations drift:** the checker verifies test names, not line
  text, so any edit above a cited line moves its citation silently. After
  editing a cited file, re-derive test lines by name and map
  `impl_evidence` lines through a line diff from the last commit.
- **Commits:** authored `Tom F. <tomf@tomtomtech.net>` with `Signed-off-by`,
  no other trailers (`CONTRIBUTING.md`, `PROVENANCE.md` §3.2).

## Next, in order

1. **Mutation testing of this branch's diff.** CI's incremental job
   (split by crate on this branch: types 2, client 2, sdk 1, server 10
   shards) grades it once a pull request is open. A local run with `lld`
   and `-j 2` takes roughly a minute a mutant; survivors found so far were
   fixed in the branch, and the last local results are in the session
   report, not here.
2. **APPLICATION entries (35).** 22 were re-examined and kept on
   2026-10-18 (each note says why); 11 are card-content, deployment or
   agent-reasoning requirements the SDK only warns about or hands the agent
   (moved from a mistaken `SUPPORTED`). What could still become SDK work:
   §3.1.11 423 (a client preflight that the credentials it attaches match
   a declared scheme), §8.4.3 x5c certificate expiry, a JWT revocation list
   (`jti`).
3. **Official TCK.** Re-run against the newest a2a-tck before a pull request
   that changes wire behaviour (`--min-graded 88` full, 66 minimal).
   Re-run 2026-10-19 on this branch at `8aef50d` (a2a-tck `263b9cf`, debug
   SUT): the full gate graded 88 with the 9 baselined failures, the minimal
   gate 66; both exit 0. Re-run it after any further wire change.
4. **Step 4 — OpenTelemetry on by default**, with the exporter staying in
   `a2a-protocol-sdk` only (decided 2026-10-18). Done 2026-10-18: UCUM
   units, `a2a.server.latency` removed for `a2a.server.request.duration`,
   `init_otlp_pipeline(Option<&str>)` with the environment first, ADR 0013
   Accepted, and the split: `otel` (instrumentation, the API only) is a
   default feature of the server, client and SDK, and `otlp` (`Telemetry`,
   the exporters) is default in the SDK only. Also done 2026-10-18: mTLS
   to the collector and `http/json` (the opt-in `otlp-json` feature; see the
   CHANGELOG for why it is not in `otlp`), and the server span and
   `rpc.server.call.duration` on SLIMRPC through the new public
   `BindingSpan` (ADR 0013, 2026-10-18 section), with
   `Transport::rpc_system` naming custom transports on client spans. The
   RPC semantic conventions were re-read on upstream `main` that day and
   match every name used. Step 4 is complete.
5. **Freshness:** the 2026-10-19 sweep is recorded in
   `book/src/reference/mutation-history.md`, from the shards' logs (the
   artifacts could not be downloaded here). It is incomplete: three of the
   24 `a2a-server` shards hit the 240-minute limit, so the shard count or
   the timeout needs raising again before a sweep is whole. Its three
   survivors have tests on this branch (the row names them). After the next release,
   check the versioned docs deploy.
6. **Release readiness for 0.15.0, then debt** (*Open engineering* below).

## Waiting on the maintainer

- **A release signing key.** `.github/release-signers/allowed_signers` and
  `gpg-fingerprints.txt` hold comments only, so `release.yml` refuses every
  tag, by design. Also a `v*` tag ruleset. `ROADMAP.md` tracks both.
- **Upstream drafts to send**, all in `docs/upstream/` (its README lists
  each with its status), held on 2026-10-15 for re-verification. The a2a-go
  draft for R19 is deliberately not committed: it was never checked against
  a2a-go's source. Nothing is filed upstream without asking.
- **A private advisory to a2a-python** (GHSA, filed by the maintainer; see
  `handoff-history.md`, `claude/nifty-fermat-roj04f`). Do not file anything
  public about it while it is open.
- **The OpenSSF Best Practices badge** is not registered;
  `docs/openssf-best-practices.md` has the answers.
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

None of this is in `ROADMAP.md`. Each line names where its evidence is.

- **Cross-replica:** single-writer admission (pinned by a test in
  `tests/multi_replica.rs` that is ignored by default); a stale unscoped write from another
  replica keeps a stored position whose effect it overwrote
  (`store/applied.rs`).
- **Server:** JSON-RPC parses a request's params
  before the interceptor chain authenticates it (a malformed body is
  refused `-32602` before `401`).
- **Privacy:** audit protection (`AuditLog::with_privacy`) leaves what its
  module docs list: an erased person's records stay linkable, keys survive
  in backups and logs, erasure is not serialised across replicas.
  `erase_caller` (2026-10-18) removes a caller's partitions but not what a
  caller left in a tenant's shared partition. `EncryptedTaskStore` leaves
  ids, states, timestamps, `metadata` and order unprotected, and its seal
  limit is per process while the true budget is per key across processes
  (F7 made deltas seal only what they store). Two concurrent writers no longer leave a record
  unreadable: the SQL appends are compare-and-append (F5, 2026-10-18).
- **Measurement:** `docs/readiness-bar.md` records no measurements yet (WS4);
  WS3 and WS5; a send 6–21% slower at `2b0a3abd` than at `6ce5dc9a`, not yet
  measured with the pinned-core method; the 0.9–1.8 µs `ListTasks` gap at
  10K tasks.
- **Interop:** `itk/interop/card_signature_interop.py` checks Python only.
  `scripts/go_sdk_interop.sh` re-run 2026-10-19 (debug build, a2a-go
  v2.6.0): both directions pass. Leg 1, 52 checks; leg 1b (secured card),
  38, now with the key enforced and a keyless call refused; leg 2, every
  method on all three bindings. The first run failed leg 1b: this branch's
  build check refused echo-agent's secured card, which nothing enforced.
- **Control map (2026-10-18 review):** of the 13 `partial` rows, four
  had gaps engineering could close. ASI07's, the HTTP dispatchers naming a
  caller from a mutual-TLS client certificate, is closed (ADR 0018, with no
  new dependency); AIA-14-4e's and ASI10's, a halt that persists and crosses replicas, is
  closed too (ADR 0019), ASI08's, G1-B's lease (ADR 0020), and ASI03's,
  the originating principal on delegated calls (ADR 0021, RFC 8693; it
  needs an identity provider that supports token exchange). The rest, and all 19 `integrator` rows, are split
  by nature (reviewed, unchanged).
- **Gates:** a mutation shard that starts after its PR merges grades
  nothing (low); the Windows stress-test connect failure, cause unknown; the
  benchmark comparison harness still pins `=0.14.1`.

## Waivers and their removal tests

The SLIMRPC binding carries none since 2026-10-15: its move to
agntcy-slim-rpc 3.4.1 (R13) took rustls to 0.23.46 and removed
`unsafe-libyaml`, and its `deny.toml` and `osv-scanner.toml` notes say how
each of the five was cleared. The root `deny.toml` ignores
RUSTSEC-2024-0436 (`paste`, unmaintained) until genai drops it; its own
comment is the authority.

## Decisions the maintainer made

Kept under this heading because `release.yml` and `packaging/README.md` cite
it.

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
- **Earlier, still standing:** tool calling stays in the examples, not the
  crates (`examples/rig-agent`); the rig integration crate is held
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
  request twice; the manifest-last rule holds either way.

## Branches

Only two kinds matter: the working branch (*Now*, above) and the storage branches (`claude/adk-rust-0.12-patch`,
`claude/a2a-rig-held`), both built against 0.12. Every other `claude/*` branch is merged or a pre-0.12 session branch;
deleting them is the maintainer's call. To check one:

```sh
git fetch origin && git merge-base --is-ancestor origin/<branch> origin/main && echo merged
```

This clone is shallow, so a "no" for an old branch is not proof, and `git
blame` stops at the shallow boundary. Never write this branch's own head SHA
into this file: the commit that records it changes it.

## Environment

- **Disk** is a fixed allowance: `CARGO_INCREMENTAL=0`, and delete
  `target/semver-checks` and the incremental directories when space runs
  low. A linker "bus error" (signal 7), or `linking with cc failed`,
  means the disk is full. `bindings/a2a-protocol-slimrpc/target` reaches
  12 GB for its full test suite: run it with `CARGO_PROFILE_DEV_DEBUG=0`
  and delete it afterwards.
- **PostgreSQL 16** is installed: `pg_ctlcluster 16 main start`, then
  `A2A_TEST_POSTGRES_URL=postgres://postgres:postgres@localhost:5432/postgres`
  (password `postgres`).
- **No Docker daemon** in the cloud container, so `itk/docker-compose.yml`
  cannot be run here.
- `pkill -f` matches its own shell: kill by PID.
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

- A test passing and `cargo semver-checks` passing do not show a serde
  change preserves behaviour; parse fixed inputs with both builds.
- The ledger checker verifies test names, not line text: after code moves,
  re-point `impl_evidence` citations from a diff and check each one's text.
- Count before writing a number into a document, and say what was counted.
- A `compile_fail` doctest needs a control that compiles.
- A re-run after a fix must cover the binary that failed, not only the files
  edited.
- An independent review before committing found two defects in R69 that the
  change's own tests and 23 hand mutants did not; budget for one on any
  change to the stream or store contract.
- Run every clippy invocation CI runs, not only `--all-features`: a type
  used by an always-compiled store from a module gated on `sqlite` built
  with all features and nowhere else (2026-10-18).
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

## History

[`handoff-history.md`](handoff-history.md) holds the file as it stood on
2026-10-15, before this rewrite, verbatim: every branch section from
`claude/relaxed-planck-c4hsn0` to `claude/funny-heisenberg-e6tl83`, the
observability review, the future ideas (Part B, "What not to chase"), and
each branch's verification of record. Each branch's merged content is also in
`CHANGELOG.md` under its release, in its pull request, and in its commits.
