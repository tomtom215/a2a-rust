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
- **Working branch:** `claude/jolly-hypatia-lsr7uv`, started 2026-10-20
  from `main` at `6c90d0b` (pull request #173 merged that day), pushed, no
  pull request yet. `git log 6c90d0b..` lists its commits; each message says
  what it changed and how it was verified.
- **Built on this branch (2026-10-20):** ADR 0022's tool loop
  (`server::tool_loop`, feature `tool-loop`; `examples/tool-loop-agent`,
  whose README records the live run); the §3.5.2 holds, with the
  independent review's findings fixed (`store/holds`); a fix for the idle
  timeout cutting a blocking request still being answered
  (`serve::graceful::idle::Serving`); the dependency refresh (genai 0.7,
  tokio 1.54, no `deny.toml` waiver left at the root).
- **Encryption, verified three ways (2026-10-21):** the sealed formats
  read by Python's `cryptography` in both directions
  (`scripts/crypto_interop/check.py`, `tests/crypto_interop.rs`, a CI
  step); 37 NIST CAVP AES-256-GCM cases (the zip's SHA-256 and every case
  re-checked against the download, and every case re-decrypted
  independently); four fuzz targets on the unseal parsers, 600 s each, which
  found that a keyed audit digest accepted uppercase hex (fixed); and a
  mutation sweep of the crypto modules (210 mutants, the 2 missed rewritten
  so they are killed). `book/src/deployment/security.md`, "How this is
  verified".
- **Built on this branch (2026-10-21):** ADR 0024's pushed halts and
  cancels (`store/halt/postgres.rs`, `handler/halt/sync.rs`).
- **Built on this branch (2026-10-21):** ADR 0023's budget, G5 in
  `docs/swarm-orchestration.md`: `a2a_protocol_types::budget` (`Budget`,
  `BudgetStatus`, `BudgetHandle`), every server holding a turn to the
  caller's deadline (`handler/budget.rs`), `Delegation::start_budgeted`,
  and the tool loop charging the `tokens` meter. The 0.10/0.12
  `max_stored_tasks` deprecations are removed. `webpki-roots` requires
  1.0.10 (RUSTSEC-2026-0381).
- **Trailers on `main` (for the maintainer):** 36 of the 86 non-merge
  commits pull request #173 merged (`86c8fac..6c90d0b^2`) carry
  `Co-Authored-By` and `Claude-Session` trailers, against `CONTRIBUTING.md`'s
  "no other trailer" rule, and the `Co-Authored-By` line names a model.
  Counted 2026-10-20 with `git log -1 --format=%B` per commit. `main` is not
  rewritten; whether to leave it is the maintainer's call.
- **The spec ledger:** `python3 -I scripts/check_spec_ledger.py` is the
  authority on the counts (2026-10-21: TESTED 455, APPLICATION 32,
  NOT_APPLICABLE 20, PARTIAL 3; 510 entries). The three PARTIAL entries,
  each short of a MUST for a stated reason: §3.5.2, a stream lagging past
  the held-events ceiling (8 × `max_events_per_task`, 4,096 events by
  default) ends with `streamLagged`; §5.7, the specification contradicts
  itself on an empty `ListTasksResponse.tasks`; §12.4, agntcy-slim-rpc
  carries no error details (maintainer: stays PARTIAL until it does). The
  APPLICATION and NOT_APPLICABLE entries were reviewed against the code and
  the specification text on 2026-10-20 (*Next*, item 1). The checker now
  also reads `impl_evidence`: a cited file must exist, a cited line be
  inside it, a named symbol appear in it.
- **Ledger citations drift:** the checker verifies test names, not line
  text, so any edit above a cited line moves its citation silently. After
  editing a cited file, re-derive test lines by name and map
  `impl_evidence` lines by their text in the version they came from
  (2026-10-20: a three-way merge re-derived 2,341 that way; one, §5.1's
  `grpc/dispatcher.rs:888`, had pointed past the end of a 444-line file
  since `7deff577`).
- **CI on `main` at `6c90d0b`:** every workflow green except CI, cancelled:
  `Example surface coverage` ran out of its 20 minutes compiling `swarm`,
  every earlier step green. Split into two jobs on this branch
  (`example-surface-ops`).
- **Commits:** authored `Tom F. <tomf@tomtomtech.net>` with `Signed-off-by`,
  no other trailers (`CONTRIBUTING.md`, `PROVENANCE.md` §3.2).

## Next, in order

1. **After the APPLICATION and NOT_APPLICABLE review** (2026-10-20, all
   54 against the v1.0.1 text, tag `3303592`, and the code; about 130 note
   claims checked, 8 not verifiable from the repository): 35 kept as they
   were, 16 kept with wrong or stale note claims and citations corrected,
   §14 line 3244 split (what the SDK consumes is TESTED; content an agent
   forwards stays APPLICATION), and §4.6.3 1139 and §5.8 1297 moved to
   TESTED by URI-contract golden tests (`uri_contracts.rs`,
   `uri_contract_websocket.rs`). Its suggestions were built the same day:
   the `wss://` lint fix, the axum `A2aRouter` §5.2 warning, and `Server`'s
   plaintext warning (§7.1). Left: a public form of the §5.2 check for
   bindings outside the crate (SLIMRPC cannot call `warn_if_undeclared`).
   `spec_line` N is line N + 31 of the vendored file (the header ends at
   line 30, then one blank line).
2. **The tool-calling study** (`scripts/tool_calling_study/`,
   `docs/tool-calling-study.md` once written): direct and SDK arms over four
   GGUFs, n = 30 a cell. Started 2026-10-20; results to be scored, audited
   run by run against the rubric in `score.py`, and written up with
   intervals. What it answers: whether this SDK's loop changes what a model
   does with tools, against the same model asked directly.
3. **OpenTelemetry, batch B:** telemetry initialised in every example,
   `tck/sut`, the ITK agents and the CLI; streaming span lifetime; a span per
   WebSocket frame; a client call-duration metric; store spans; SLIMRPC's
   gRPC error classification. Started 2026-10-20.
4. **Mutation testing.** The full sweep deals `a2a-server` across 60 shards
   and the incremental gate across 20 (`mutants.yml`, measured from runs
   39193157308 and 39343090044). The three 2026-10-19 survivors were
   re-verified killed on this branch by hand mutants (2026-10-20). Read the
   next scheduled sweep's logs.
5. **Official TCK** after any wire change: `--min-graded 88` full, 66
   minimal. Last run 2026-10-20 at `1305674b`, after the auth-ordering,
   streaming-span and WebSocket trace-context changes, against a2a-tck
   `5daf014` (the workflow tracks `main`): full 88 graded, 81 passed, the
   9 baselined (requirement, transport) failures and no other; minimal 66
   graded, 8 of the 9 baselined failing (STREAM-SUB-003 not graded there);
   required-extension CORE-CAP-004 passed on JSON-RPC and HTTP+JSON. All
   three gates exit 0. Re-run after any further wire change.
6. **Release readiness for 0.15.0, then debt** (*Open engineering* below).

## Waiting on the maintainer

- **File GHSA-hr9h-6jvf-wvg6 with RustSec** (found 2026-10-21 by the CRA
  review). The project's own advisory (fixed in 0.14.1) is on GitHub only:
  the RustSec database and OSV have no entry (OSV queried 2026-10-21), so
  `cargo audit`, `cargo deny` and OSV-Scanner on an integrator's lockfile do
  not flag `a2a-protocol-server` or `a2a-protocol-sdk` below 0.14.1. CRA rows
  CRA-AI-I-2a, CRA-AI-II-2-4-8 and CRA-14 carry it as a gap. Filing is an
  upstream action, so it waits for the maintainer.
- **`STABILITY.md` §1** still gives the account written at 0.13; its
  version sentence now says so. Bring it up to 0.14 and 0.15 at release
  prep.
- **Compare-and-set for task writes (cross-replica).** The 2026-10-20 fix
  makes a stale continuation save record the position of its read, so a
  subscriber replays the event it overwrote, but only until the continuing
  turn's first fold stores a position past it again; the stored task keeps
  the lost update. Closing both needs a conditional write: provided methods
  `get_versioned` and `save_appending_history_if(.., expected)` (defaults
  unchecked), a `version BIGINT` column on Postgres and SQLite (two
  migrations; on SQLite compare `(row version, MAX(journal seq))` so the
  journal keeps its O(1) appends), a counter under the in-memory write
  lock, forwarding in the encrypted, audit and tenant wrappers, and a
  bounded re-read and retry on a refused continuation. It changes the store
  contract custom stores implement, so it is the maintainer's call. It
  does not make concurrent turns on two replicas safe; that needs
  cross-replica single-writer admission (a lease row per task).
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
  `tests/multi_replica.rs` that is ignored by default). A continuation's
  save on one replica still overwrites an artifact another replica's
  running turn folded after the save's read: a lost update only a
  compare-and-set write would refuse. Since 2026-10-20 the save records
  the position of its read, so a subscriber replays that event rather than
  losing it, until the continuing turn's first fold stores a position past
  it again (`store/applied.rs`, `handler/messaging/basis.rs`,
  `tests/stale_write_position`).
- **Holds (§3.5.2):** the subscription's setup hold
  (`handler/lifecycle/subscribe/hold.rs`) has no test that fails without
  it; with the in-memory processor the window it guards cannot open past
  the bound (ledger §3.5.2 note). The gap path's early returns in
  `replay_after` predate the holds and skip the positions between the
  snapshot and the attach point if the log cannot cover them, with no
  `streamLagged` (independent review, 2026-10-20, finding 6): rarer now,
  not closed.
- **AES-GCM nonce budget is per process** (encryption verification,
  2026-10-21, medium, design). The 2^32-seal limit per key is counted per
  process and resets on restart, so R replicas can seal R × 2^32 under one
  key: a repeat probability R² times the NIST target (R = 10: 1.2e-8). The
  proposed fix seals each value under an HKDF-derived key with a random
  salt, marked by a leading `0x00` byte no current value can start with;
  it is a format change that needs a two-step rollout (readers first,
  then writers behind a flag), so it waits. The book tells deployers to
  give each replica its share of the limit meanwhile.
- **An intermittent test:** `push_delivery::tests::log_redaction::
  a_failed_or_timed_out_delivery_logs_the_origin_not_the_secret` misses
  its timeout warning about once in 20 parallel runs of `--lib -- audit
  store::encrypted push` (2 of 40, 2026-10-21; never alone, 0 of 6). The
  event is emitted (the timeout path's result is asserted) but not
  captured. `rebuild_interest_cache` after the install and again before
  the second delivery did not stop it (still 2 of 40), so the callsite
  cache is not the whole cause. Not skipped; cause unknown.
- **Tool loop:** spans record no content, because the handler's
  content-capture setting does not reach an executor; a `Failed` status
  carries up to 512 bytes of the provider's error body to the A2A caller;
  `CONTRIBUTING.md` still says the server has "eleven public traits"
  (21 `pub trait`s on 2026-10-20).
- **Privacy:** audit protection (`AuditLog::with_privacy`) leaves what its
  module docs list: an erased person's records stay linkable, keys survive
  in backups and logs, erasure is not serialised across replicas.
  `erase_caller` (2026-10-18) removes a caller's partitions but not what a
  caller left in a tenant's shared partition. `EncryptedTaskStore` leaves
  ids, states, timestamps, `metadata` and order unprotected, and its seal
  limit is per process while the true budget is per key across processes.
- **Measurement:** `docs/readiness-bar.md` records no measurements yet (WS4);
  WS3 and WS5; a send 6–21% slower at `2b0a3abd` than at `6ce5dc9a`, not yet
  measured with the pinned-core method; the 0.9–1.8 µs `ListTasks` gap at
  10K tasks.
- **Interop:** `itk/interop/card_signature_interop.py` checks Python only.
  `scripts/go_sdk_interop.sh` last passed both directions 2026-10-20 at
  `4101bb55`, after the auth-ordering change: leg 1 52 checks, leg 1b
  38, leg 2 33 cells exercised (11 WebSocket cells not applicable), 0
  failures.
- **Control map:** 13 supported, 2 documented, 11 partial, 22 integrator
  (`python3 scripts/check_compliance_map.py`); the partial rows' remaining
  gaps are in each row's `gap`.
- **Gates:** a mutation shard that starts after its PR merges grades
  nothing (low); the Windows stress-test connect failure, cause unknown; the
  benchmark comparison harness pins `=0.14.1`, the newest release on
  crates.io (checked 2026-10-20), and moves when 0.15.0 is published;
  `prove_workflow_gates_fail.py` needs `ssh-keygen`, absent in the cloud
  container (apt has no candidate), so it did not run here on 2026-10-20.

## Waivers and their removal tests

The SLIMRPC binding's `deny.toml` ignores one entry since 2026-10-19: the
yanked `mls-rs-crypto-awslc` 0.25.1, which its comment explains (0.25.0
would bring back four rustls advisories). **Never run a blanket `cargo
update` in the binding:** it cannot keep a yanked version, so it downgrades
agntcy-slim-* (3.4.3 to 3.4.2), aws-lc-rs and rustls (0.23.46 to 0.23.43)
back into those advisories. Update named packages with `cargo update -p`.
On 2026-10-20 agntcy-slim-rpc 3.4.5 still could not be taken: every
agntcy-slim-auth after 0.17.6 requires the yanked `^0.25.1`.
The root `deny.toml` ignores nothing since 2026-10-20, when the
examples moved to genai 0.7 and `paste` left the graph. The root
`osv-scanner.toml` keeps RUSTSEC-2023-0071 (`rsa`, in `Cargo.lock` through
`sqlx-mysql` but resolved by no feature; re-checked 2026-10-20).

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
  one respect: ADR 0023's deadline is relative (`remainingMs`, as gRPC's
  `grpc-timeout`), not absolute, so clock skew between agents does not
  move it; each hop's remainder is what its parent reserved.
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
  `A2A_TEST_POSTGRES_URL=postgres://postgres:postgres@localhost:5432/postgres`.
  The `postgres` role's password is not always `postgres` in a fresh
  container (scram auth refused it on 2026-10-20): set it with
  `su postgres -c "psql -c \"ALTER USER postgres PASSWORD 'postgres'\""`.
- **llama.cpp** b11751 (`8466395`) is built at `/opt/llama/src/build/bin`;
  the study's GGUFs are in `/opt/models` (hashes in the study). Use
  `-c 8192` (the default context sized the KV cache to ~4.6 GB and the OOM
  killer took the server), and send one request at a time to a server whose
  results must replay.
- **No Docker daemon** in the cloud container, so `itk/docker-compose.yml`
  cannot be run here.
- `pkill -f` and `pgrep -f` match their own shell's command line, which
  contains the pattern: kill by PID, or put the command in a script file
  (cost a killed shell twice on 2026-10-20).
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

## History

[`handoff-history.md`](handoff-history.md) holds the file as it stood on
2026-10-15, before this rewrite, verbatim: every branch section from
`claude/relaxed-planck-c4hsn0` to `claude/funny-heisenberg-e6tl83`, the
observability review, the future ideas (Part B, "What not to chase"), and
each branch's verification of record. Each branch's merged content is also in
`CHANGELOG.md` under its release, in its pull request, and in its commits.
