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

- **0.15.1 is prepared on branch `claude/kind-curie-jskrn8` (2026-10-26); it is
  not tagged and no pull request is open.** Versions, the eight dependency
  pins, the four lockfiles, `CHANGELOG.md` (`[0.15.1] - 2026-10-26`, an empty
  `[Unreleased]`), `CITATION.cff`, `ROADMAP.md`, the book's changelog page and
  `docs/provenance-manifest.md` (last) are done. Content: OTLP export for
  proxies, gzip, a console exporter, `otel-prometheus`, a stderr layer with a
  startup line and lost-export warnings (with the transport's cause and a hint
  where the failure names the fix), resource detection, `OTEL_PROPAGATORS`,
  percent-decoded `OTEL_RESOURCE_ATTRIBUTES`, `shutdown_signal()`, a quick start
  per deployment in `observability.md`, and the corrections two read-only audits
  found (each verified before it was applied; the reports were leads).
  **For the maintainer, before opening the pull request:**
  1. *Sign-off.* The commits are authored as the container's identity with no
     `Signed-off-by`; `scripts/check_dco.sh` will refuse them. Re-author and
     sign off.
  2. *Patch or minor.* The release adds public API (`shutdown_signal`,
     `OtlpCompression`, five `TelemetryBuilder` and three `Telemetry` methods),
     an off-by-default feature and two behaviour changes (a later `.init()`
     after `install_subscriber` panics; `HTTP_PROXY` now applies to the export).
     `check_release_tree.py` accepts it as a patch (no `### Breaking Changes`),
     but the book's changelog page defines a patch as bug fixes, and 0.14.1 was
     one advisory with no API change. 0.16.0 is the other reading; the choice is
     yours, and `CHANGELOG.md`, the manifests and `CITATION.cff` follow it.
  3. *Not run, and so not claimed:* a real OpenTelemetry Collector (building one
     was refused in the session, so every collector was a stand-in), Kubernetes
     (the manifest was parsed as YAML; `$(POD_NAME)` expansion is Kubernetes's),
     IPv6, a public-CA endpoint, a production proxy, `cargo-semver-checks`
     (installing it was refused; the claim that nothing public was removed comes
     from a diff of `pub` lines against `v0.15.0`, and CI runs the real tool on
     the pull request), `cargo deny` and `promtool`.
  4. *Toolchain.* This session's clippy (1.97) reports, in the server crate at
     the last count, 49 `duration_suboptimal_units` and 7 `doc_markdown`
     findings, and more in the client crate, all in files this branch did not
     touch; none is in a file it changed. CI's stable may differ.
  Also **ADR 0028 (Proposed)** and `docs/adr/0028-wasm-spike/`: the real
  `RequestHandler` was run inside `workerd` (11 of 11 checks, native lib suite
  unchanged at 1,545). Nothing from it is in the crates. The next session
  should start by running `docs/adr/0028-wasm-spike/reproduce.sh`; if the base
  commit has moved, the patch needs a rebase and `results.txt` regenerating.
- **Released:** 0.15.0 (tag `v0.15.0` on merge commit `e4fc3bc1`, 2026-10-25) is
  the newest release; 0.14.1 (2026-09-30) is the one before. It is the first
  signed tag: SSH-signed by the key in `.github/release-signers/`, and
  `scripts/verify_tag_signature.sh` accepts it against the keys on `main`.
  Release run 40196985363: attempt 1 failed `test · Windows` on
  `lease_renewals_are_in_the_parents_trace` ("1 renewals"; it needs 2), a
  re-run of that job passed, and the run finished green at 14:25 UTC.
  crates.io lists all four crates at 0.15.0 (published 14:24 to 14:25 UTC;
  each checksum equals the release's `SHA256SUMS`), docs.rs built all four,
  and GitHub accepted the shortened release body (40,600 characters). The
  release has no CLI binaries: `PUBLISH_CLI_BINARIES` is unset
  (`packaging/README.md`). **The Windows failure's cause is not
  established.** The same tree passed Windows twice in the pull request and
  150 of 150 times in a probe on `windows-latest` (renewal 2 started at 679 to
  707 ms; the test's window closed at about 1,022 ms, so a stall of over
  ~315 ms would explain it, and nothing showed one). The test now waits for
  two recorded renewals, with a 10 s deadline and the span names in its
  failure message, instead of sleeping a fixed second.
- **Book audit (2026-10-25):** 13 readers checked every book page against
  the code, the specification and the changelog, and 13 editors applied what
  held up. Checked mechanically afterwards: the book's doctests (281 passed,
  0 failed, 5 ignored, the baseline), `rustdoc -D warnings` on the four
  crates, the CI gates that need no build, `promtool` on the alert rules, and
  the two generated benchmark pages (each regenerates byte-identical from its
  generator). The harness does not compile the bodies of `#[test]` and
  `#[tokio::test]` functions (rustdoc strips them), so `testing.md`'s seven
  were compiled and run by hand against a working executor: 7 of 7 pass; the
  page's hidden stub executor does nothing, so they fail against it, which is
  why the page now says `CalcExecutor` is not shown. The book's type face was
  11.9 px in a 560 px column (mdBook's 62.5 % root made `1rem` 10 px);
  `book/theme/custom.css` now sets 17 px in a 720 px column. The banner cards'
  subtitle and the code card's footer said v1.0.0; they say v1.0.1 now (only
  that text band was re-rendered; `book/static/brand/README.md`). Left open:
  `book/src/images/dashboards/a2a-server.png` predates the Agent executor row
  (the page says so; retaking it needs the live stack);
  `examples/deploy-agent/Dockerfile` was one Rust minor behind the MSRV and
  is now on 1.94, with no gate that keeps it there; `docs/official-tck-
  findings.md` §1 says 87 passing and 5 failing "cannot be reconciled" with
  114, and an editor's arithmetic suggests it can (87 + 5 + 1 + 21), which
  nobody has resolved.
- **Working branch:** `claude/sleepy-knuth-3lnx7k`, restarted 2026-10-25 from
  `main` after pull request #180 merged (merge commit `e4fc3bc1`, the 0.15.0
  tag). `git log origin/main..` lists what it holds that `main` lacks.
- **Merged in #179** (branch `claude/zealous-fermat-f4kg9z`, 2026-10-23 to
  2026-10-24; merged 2026-10-24 13:23 UTC as `2781bb69`). The PR head
  `6707f4d` had 104 check runs, 98 success and 6 skipped by their own
  workflows' conditions (`repos/tomtom215/a2a-rust/commits/6707f4d/
  check-runs`, `total_count`, counted 2026-10-24), the labelled mutation
  gate among them. `git log 4cadc2af..2781bb69` lists the commits; each
  message says what it changed and how it was verified. In sum:
  - the tool-calling study scored, audited by reading every answer, and
    published (`docs/tool-calling-study.md`, `34f579d`);
  - every dependency refreshed in all nine lockfiles, the Python
    requirements, Actions, Go, npm and Maven pins; MSRV 1.94, which sqlx
    0.9 requires (`7a73bdc`; `STABILITY.md` §5 allows the raise);
  - Prometheus recording and alerting rules (with `promtool` tests) and
    three Grafana dashboards (server, client, GenAI), built from a live
    Prometheus 3.15.1 fed by the repository's own binaries, directly over
    OTLP and through an OpenTelemetry Collector (`deploy/observability/`,
    `c372919`; GenAI and task-store panels, with screenshots in the README
    and the book, 2026-10-24);
  - a database `CLIENT` span and `db.client.operation.duration` for each
    operation of a store that names its database (`TaskStore::database`;
    the SQLite and PostgreSQL stores do), and `server.address` on a
    WebSocket client's spans (`Transport::server_url`), 2026-10-24;
  - OpenTelemetry: a `service.instance.id` per process (`755b121`), export
    wherever an endpoint is configured in every binary (`988fb69`), the
    tool loop's `GenAI` metrics and the rest of its `chat` span
    (`d118aaf`);
  - the spec ledger cites code by symbol at its definition line, and the
    checker verifies it (`6202a56`);
  - a re-audit of all 510 ledger entries against the v1.0.1 text found
    273 confirmed as written; its defects are being fixed in work
    packages. WP-A (server security: §13.3 schemes on the public card,
    §13.2 push credentials, push config `Debug`, the kid-optional signers
    removed, card `max-age` on WebSocket and gRPC, eleven security tests
    that now test what they are cited for) and WP-B (the client validates
    every object and card it receives against a2a.proto's REQUIRED
    fields; the gRPC conversion refuses absent ones; wire forms) are
    merged. So are WP-C (handler-wide conformance: the executor sees only
    card-declared extensions, `BadRequest` names the missing field, an
    empty push url is `InvalidParams`) and WP-D (SLIMRPC: every operation
    tested over the fabric, an equivalence suite against JSON-RPC, the
    push-config list fixed, retryable errors prefixed, the server's
    default telemetry, an `otlp` feature, on by default since the TLS
    conflicts below were fixed);
  - `examples/live-model` (2026-10-23): the SDK end to end against a real
    model through llama-server, ten checks (the tool loop streamed and
    signature-verified on all four bindings, provenance refusal,
    idempotency, budget exhaustion, delegation under a budget, halt,
    quorum). Its README records the runs, the model, the quantization and
    the llama.cpp build. The other model-backed examples were run the same
    day against the same llama-server (b11836, Qwen3-1.7B Q4_K_M, alias
    `qwen3:1.7b` on port 11434, release builds): `tool-loop-agent` (billing
    question: right, two rounds), `incident-response` (exit 0, Act 4 44 of
    44 cells, Act 5 16 of 16 with `A2A_TEST_POSTGRES_URL`), `genai-agent`
    (44 of 44, LLM leg exercised), `mcp-agent` (exit 0; 1 of 2 answers
    used the MCP tools, and the billing answer, which called none, is
    invented), `swarm` (`llm` scenario 32 of 32 jobs completed) and
    `rig-agent` (44 of 44 on its second run; its first lost SendMessage over
    WebSocket to the clients' 30 s default timeout, which `84d37e1` replaced
    with one 300 s `MODEL_CALL_TIMEOUT` per example on every binding).
    Logs were not committed.
  - §3.1.2 (2026-10-23): a `Message` an executor writes once its task
    stream has begun is sent as a status update carrying it, folded once
    at the queue's writer (`streaming/event_queue/message_fold.rs`),
    reviewed independently; a follow-up to an existing task folds its
    first `Message` too (the maintainer's decision, reviewed again).
- **`sign_agent_card` is deprecated, not removed** (2026-10-23): it shipped
  in 0.14.1, so `STABILITY.md` §3 keeps it for one minor release. It now
  refuses `key_id: None`, which wrote a header without the `kid` §8.4.2
  requires (a specification correction, §2), and goes in 0.16.0.
  `sign_agent_card_ed25519` never shipped and stays removed. No §3 waiver
  remains.
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
- **Control map (2026-10-23):** 88 rows, every one citing a test or CI step
  that proves the row's `sdk` text; `check_compliance_map.py --check` fails
  on a row that cites nothing (`UNCITED` is empty: AIA-6 cites the book's
  "Is Your Agent High-Risk?" through `DOC_CLAIMS`, AIA-53 cites
  `check_no_model_artifacts.py`) and on a document that stops saying what
  `DOC_CLAIMS` quotes. All 187 cited tests were run 2026-10-23,
  PostgreSQL-ignored ones included, all passing. The checker verifies that
  a cited test exists, not that it passes: rerun them after editing a cited
  file. On 2026-10-23 the requirement text of every AI Act row claiming SDK
  support (12-1, 12-2, 12-3, 13-3f, 14-1-3, 14-4a, 14-4d, 14-4e, 14-5,
  15-4, 15-5, 19-26-6, and the unamended parts of 50-2 and 72-2) was
  compared with Regulation (EU) 2024/1689 as published (OJ PDF, SHA-256
  `bba63044…`, the same file as 2026-10-15, fetched with `curl -H 'Accept:
  application/pdf' -H 'Accept-Language: eng'
  http://publications.europa.eu/resource/celex/32024R1689`; EUR-Lex
  answers 202): all consistent, Art. 14(5)'s two-person verification and
  its law-enforcement exception included. What the rows say Regulation
  (EU) 2026/1744 changed was not re-checked: that text is not fetchable
  here.
- **Built on this branch (2026-10-22):** kill-switch conformance,
  `conformance::halt::{check_store, check_handlers}` (feature
  `conformance`): 8 store checks and 9 end-to-end replica checks, tested
  against deliberately broken stores; the in-memory, SQLite and PostgreSQL
  stores pass (`tests/kill_switch_conformance.rs`). This completes the
  2026-10-21 safety list except the upstream drafts, which wait on the
  maintainer.
- **Built on this branch (2026-10-22):** the prompt-injection containment
  harness (`crates/a2a-protocol-sdk/tests/injection_containment/`,
  `docs/injection-containment.md`): a model that obeys every instruction
  it reads, 8 scenarios a control contains and 3 limits asserted as limits;
  cited by the control map's ASI-MODEL row. Chosen by the maintainer over a
  live-model susceptibility study (2026-10-22).
- **Built on this branch (2026-10-22):** ADR 0027's provenance wiring:
  `with_content_signer` signs what the server serves (`src/provenance.rs`,
  the stream reader, push, the unary handlers), `with_content_keys` on the
  server (`RequestContext::content_check`) and the client
  (`src/content.rs`, `ClientError::ContentNotVerified`), and
  `Profile::Production` requiring a signer. An independent review found 12
  problems; the 9 fixed and the limits left are in ADR 0027 (including an
  unreleased flaw in `verify_content`). Verified at `69163dec`: all 169
  server test targets with all features (1,221 passed, 114 ignored), run 5
  at a time because 15 at a time filled the disk at link time. That
  commit's message says 22 hand mutants and 170 citations; recounted, they
  are 21 and 169. Found on the way, and fixed
  2026-10-22: metadata on an appended artifact chunk was dropped by the
  SQLite and PostgreSQL stores (`tests/artifact_append_metadata.rs`).
- **Built on this branch (2026-10-22):** ADR 0026's tool-call idempotency:
  `ToolInvocation::idempotency_key`, `Tool::read_only`, and a repeated call
  to a side-effecting tool answered from the first
  (`tool_loop/idempotency.rs`); `Delegation::start` keys the child's
  message when the agent's card honours keys.
- **Built on this branch (2026-10-21):** ADR 0025's production profile:
  `Profile::Production` (`profile.rs`) and `ServeConfig::production()`
  (`serve/graceful/production.rs`, `ServeReport::refused`). Not covered:
  the gRPC and WebSocket listeners. Since ADR 0027 the profile also
  requires a content signer.
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
  authority on the counts (2026-10-24: TESTED 455, APPLICATION 32,
  NOT_APPLICABLE 19, PARTIAL 4; 510 entries). Re-audited 2026-10-24: the 20
  NOT_APPLICABLE entries were read again against the v1.0.1 text by an
  independent reader (18 agreed; §1.4's migration-guidance MUST at line 112
  moved to TESTED, with a drift test; Appendix A's note rewritten). The four PARTIAL entries, each short for
  a stated reason: §3.5.2, a stream lagging
  past the held-events ceiling (8 × `max_events_per_task`, 4,096 events by
  default) ends with `streamLagged`; §5.7, the specification contradicts
  itself on an empty `ListTasksResponse.tasks`; §12.4, agntcy-slim-rpc
  carries no error details (maintainer: stays PARTIAL until it does);
  §7.6.1,
  a message to a task in AUTH_REQUIRED is refused while its executor still
  runs (recommended: document that the executor returns, PARTIAL naming
  the conflict between the section's two SHOULDs). The APPLICATION and
  NOT_APPLICABLE entries were reviewed on 2026-10-20 (*Next*, item 1).
- **Ledger citations:** since `6202a56` every Rust citation is
  `path:line Symbol` at the symbol's definition line (non-Rust:
  `path:line "fragment"`), and the checker verifies that the symbol is
  defined at that line. After a code move, `check_spec_ledger.py
  --fix-lines` re-points the citations it can; where a symbol is defined
  twice in a file (`TryFrom` both ways) it cannot, and each must be
  re-pointed by hand to the same impl it named before (2026-10-23: 23
  after WP-B, each checked against the old file).
- **CI on `main` at `2781bb69`** (the fifteen push workflows, read with
  `gh api repos/tomtom215/a2a-rust/actions/runs?head_sha=…`, 2026-10-24
  13:40 UTC): DCO, Scorecard, ITK, OSV-Scanner, Verification, Deploy
  Documentation, Official TCK, Soak, ACTS, Fuzz, TCK and CodeQL passed; CI,
  Coverage and Benchmarks were still running. The scheduled OSV-Scanner
  runs failed on 10-21, 10-22 and 10-23 at head `6c90d0b`
  (RUSTSEC-2026-0381/0382/0383: rustls-webpki 0.103.15, webpki-roots and
  webpki-root-certs 1.0.9, in four lockfiles) and on 10-24 at head
  `4cadc2a` (RUSTSEC-2026-0387: crypto-bigint 0.7.5 in the SLIMRPC
  lockfile). Each cause was read in the job log; #179 fixed all of them
  (`8d7e878` the last) and OSV-Scanner passed on the merge.
- **Commits:** authored `Tom F. <tomf@tomtomtech.net>` with `Signed-off-by`,
  no other trailers (`CONTRIBUTING.md`, `PROVENANCE.md` §3.2).

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
   not schemes. Still not built: a plaintext warning on the WebSocket and
   gRPC listeners (only `Server` has one) and the ADR 0025 refusal for
   them, `CombinedDispatcher` and SLIMRPC. `spec_line` N is line N + 31 of
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
3. **OpenTelemetry, still open:** `error.type` on a store span is the A2A
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
5. **Official TCK** after any wire change: `--min-graded 88` full, 66
   minimal. Last run 2026-10-20 at `1305674b`, after the auth-ordering,
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
- **Compare-and-set for task writes (cross-replica).** *Decided 2026-10-24:
  not for 0.15.0; 0.16 (see Decisions).* The 2026-10-20 fix
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

No waivers anywhere since 2026-10-23. The SLIMRPC binding's `deny.toml`
ignored the yanked `mls-rs-crypto-awslc` 0.25.1 until agntcy-slim-mls 0.5.1
accepted 0.26; a plain `cargo update` in the binding then took
agntcy-slim-rpc 3.4.6 and kept rustls at 0.23.46 (its comment has the
history). The root `osv-scanner.toml` dropped RUSTSEC-2023-0071 when sqlx
0.9 removed `rsa` from every lockfile. Before updating the binding, still
read `cargo deny check advisories` after the update, not before.

## Decisions the maintainer made

Kept under this heading because `release.yml` and `packaging/README.md` cite
it.

- **2026-10-24:** multi-replica high availability is not a requirement for
  0.15.0. It ships as a documented limitation (CHANGELOG "Known
  Limitations", the book's horizontal-scaling page): the supported topology
  is one replica, or several with session affinity per task. Compare-and-set
  writes and cross-replica single-writer admission move to 0.16.
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
- **No Docker daemon** in the cloud container, so `itk/docker-compose.yml`
  cannot be run here.
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

## History

[`handoff-history.md`](handoff-history.md) holds the file as it stood on
2026-10-15, before this rewrite, verbatim: every branch section from
`claude/relaxed-planck-c4hsn0` to `claude/funny-heisenberg-e6tl83`, the
observability review, the future ideas (Part B, "What not to chase"), and
each branch's verification of record. Each branch's merged content is also in
`CHANGELOG.md` under its release, in its pull request, and in its commits.
