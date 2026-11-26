<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# DVX: developer experience

Read [`../README.md`](../README.md) §6 first, then E04: the measured facts (§1), the friction log for three
personas (§2: a first agent, a production fleet, a supervisor), the API-surface reading (§3) and the ranked
improvements (§6), which the packages below follow. Each E04 §6 item gives the problem, the change as a Rust
signature where one is needed, the files, the verification and the over-engineering risk; this file does
not restate them.

## Rules for this workstream

1. **Subtraction first.** Most of E04 §6 removes boilerplate using what already ships. A package that adds a
   public item says why nothing existing could do it.
2. **A public name follows `CONTRIBUTING.md`, *Naming a new public item***; a rename goes through
   deprecation.
3. **An example is copied by adopters.** Each example change keeps the example's own checks and its book
   page (`python3 scripts/check_book_examples.py`).
4. **Considered and declined** (E04 §6, last section): a proc-macro crate, typestate builders, a
   configuration-file layer, turning `otlp` off by default, a typed `ServerError::Config`. Do not start
   one.

---

## DVX-01: A refused start cannot be dropped (E04 §6 item 1)

- **Goal.** One method turns a `ServeReport` that carries a refusal into an `Err`, so the README's quick
  start and `hello-agent` stop losing it.
- **Gates.** none. **Depends.** none. **Release.** `[0.16.x]`, additive.
- **Read first.** `crates/a2a-protocol-server/src/serve/graceful/mod.rs` (`ServeReport`), ADR 0052 (the
  three serve shapes stay; do not add a fourth), `README.md` (the quick start).
- **Do.** `ServeReport::into_result`, as item 1 gives it. Use it in the README, the quick-start page and
  `hello-agent`.
- **Accept.** Item 1's checks: the unit test, and a gate that fails on a spawned `serve_with_shutdown`
  whose result is not bound, in the README, the crate READMEs and the getting-started pages.
  `cargo semver-checks` reports no break.
- **Out of scope.** `Server::run`.

## DVX-02: `hello-agent` in thirty lines of code (item 3)

- **Evidence.** E04 §1 (48 code lines today, 19 of them telemetry error handling and the refusal check).
- **Gates.** none. **Depends.** DVX-01. **Release.** an example.
- **Do.** As item 3, using only what ships plus DVX-01. The friendlier telemetry message moves to
  `deploy-agent`.
- **Accept.** Item 3's check as a script with a self-test, in `static-checks`: the non-blank, non-comment
  lines before the test module of `examples/hello-agent/src/main.rs` number at most 30. The example's
  header rule (one dependency, no feature flags) holds and its tests pass.
- **Out of scope.** New API.

## DVX-03: One production reference (item 4)

- **Goal.** `deploy-agent` applies a profile and prints its report, and one book page assembles a whole
  production handler.
- **Evidence.** E04 §2 persona P2; item 4; ADR 0025; ADR 0040.
- **Gates.** none. **Depends.** none. **Release.** an example, a book page, one additive method
  (`ServeConfig::describe`).
- **Do.** As item 4, both halves. If the JWT variables read as scope creep in review, ship the page and the
  report line alone (item 4 says the same).
- **Accept.** Item 4's checks: the two `deploy-agent` tests; the gate that the example applies a profile
  and that the page names each store and control; the page compiled by `book-tests`.
- **Out of scope.** A gateway binary (ADR 0040 §4 rejected one).

## DVX-04: The examples teach the short forms, held by a ratchet (item 5)

- **Evidence.** E04 §6 item 5 (the ergonomic API exists and the examples bypass it: 34 `Box::pin(async`, 19
  long-form artifact emits, 31 `Arc::new` around a build, 3 probe-drop-rebind binds, counted on
  2026-11-25; re-count before starting).
- **Gates.** none. **Depends.** none. **Release.** examples.
- **Do.** As item 5: mechanical edits, then `scripts/check_example_idioms.py` with a shrink-only baseline in
  the style of `.file-length-baseline`. Exempt `agent-team` in the first pass through the baseline.
- **Accept.** The script's self-test fails a planted regression of each pattern; each count is lower than
  before; every example's own checks pass (`.github/workflows/ci.yml` jobs `dogfood` and `example-surface`, run with
  `scripts/run_workflow_job.py`).
- **Out of scope.** Changing what an example demonstrates (SWM-01 does that for delegation).

## DVX-05: Serve an executor on loopback for a test (item 7)

- **Goal.** The twelve-line bind, card and spawn pattern, re-written in every example and racy in one,
  becomes one function.
- **Evidence.** E04 §6 item 7; E03 §3 G23 (every multi-agent test binds real loopback ports by hand).
- **Gates.** none. **Depends.** none. **Release.** `[0.17]`, additive, behind `conformance`.
- **Read first.** `crates/a2a-protocol-server/src/conformance/mod.rs`, `examples/swarm/src/topology.rs`, the
  rule that the server crate does not depend on the client.
- **Do.** `serve_on_loopback` and `LoopbackAgent`, as item 7 gives them. One function; the client stays the
  caller's.
- **Accept.** Item 7's checks: two agents started at once get different ports and both answer; dropping the
  guard closes the listener; `grep -rc 'drop(probe)' examples` totals 0.
- **Out of scope.** A scripted model, a fake clock, record and replay, a deterministic simulator (each a
  separate decision; item 7 says which is most defensible).

## DVX-06: `/health` and `/ready` on the JSON-RPC dispatcher (item 8)

- **Gates.** DEC-11. **Depends.** none. **Release.** `[0.17]`, a behaviour change on the wire.
- **Do.** As item 8.
- **Accept.** Item 8's checks; the in-repo TCK passes on JSON-RPC; the health-checks page no longer tells
  the reader to write the route.
- **Out of scope.** Putting `CombinedDispatcher` in `hello-agent`.

## DVX-07: Retire what the deprecations allow (item 9)

- **Evidence.** E04 §6 item 9 and §1 (the deprecated items and their removal versions; re-count); ROADMAP
  *Developer experience* item 4 and *OpenTelemetry* item 3.
- **Gates.** none. **Depends.** FND-11 (the deprecation gate). **Release.** `[0.17]`, **breaking**: one
  breaking minor per calendar month (`STABILITY.md` §3), each removal under `### Breaking Changes` with its
  migration, and an upgrade-guide section (`book-tests/tests/upgrading_guide.rs`).
- **Do.** As item 9: (a) the removals; (c) `Duration` setters for the two `_secs` forms, deprecating those;
  (d) the two documentation corrections. Item 9 (b) is done in SWM-04.
- **Accept.** `cargo semver-checks` reports exactly the removals the changelog lists. The deprecation gate
  passes. No doc comment outside upgrade notes names a pre-1.0 method name (item 9's grep).
- **Out of scope.** Normalising every builder verb (ADR 0053 rejected it).

## DVX-08: Group the handler builder's documentation (item 10)

- **Evidence.** E04 §3 (59 public methods on one `impl`, alphabetical in rustdoc; `S/builder.rs` is 2,699
  lines and in `.file-length-baseline`).
- **Gates.** none. **Depends.** none. **Release.** `[0.16.x]`; no signature changes.
- **Do.** As item 10: several `impl` blocks, each with a heading comment, in files under
  `crates/a2a-protocol-server/src/builder/`.
- **Accept.** `cargo semver-checks` reports nothing; `./scripts/check_file_lengths.sh` passes with
  `S/builder.rs` out of the baseline; the count of public methods across the builder files is unchanged
  (state it before and after).
- **Out of scope.** Sub-builders.

## DVX-09: `a2a doctor` (item 11)

- **Evidence.** E04 §6 item 11 (proposed 2026-09-22 with a failing-first test, never built).
- **Gates.** none. **Depends.** none. **Release.** the CLI (unpublished; ECO-01).
- **Do.** As item 11, with `--no-send` for production agents.
- **Accept.** Item 11's test: an agent whose card advertises a binding on a closed port makes the command
  exit non-zero naming that interface; against `hello-agent` it exits 0.
- **Out of scope.** Anything that writes.

## DVX-10: The smaller rows

- **Evidence.** E04 §6 item 12; ROADMAP *Developer experience* items 1, 3 and 5, *The book's structure*
  items 2 and 3 (re-check each against HEAD first: E06 found `NEW-g-docs-2` already fixed).
- **Gates.** none. **Depends.** FND-03. **Release.** `[0.16.x]` for documentation, `[0.17]` for
  `ServeConfig` on the gRPC and WebSocket listeners (G-09).
- **Do.** One commit per row: the card helper for bearer authentication; docs.rs metadata for the discovery
  crate; a supplied store inheriting the handler's metrics; the facade's doc sentence; the triage rows.
- **Accept.** Item 12's per-row checks; each register row carries its commit.
- **Out of scope.** Anything item 12 does not list.
