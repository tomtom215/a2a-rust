<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# SPC: specification conformance, and readiness for A2A 1.1

Read [`../README.md`](../README.md) §6 first, then E01. Its §2 is the numbered delta (rows R, G, T, A, S, L,
D, C) that the packages below cite; §3.1 maps each row to files here; §5 says what upstream has not settled.

Abbreviations: `S/` is `crates/a2a-protocol-server/src/`, `C/` is `crates/a2a-protocol-client/src/`,
`T/` is `crates/a2a-protocol-types/src/`.

## The position

- **1.0.1 stays the conformance target of every default build** until upstream tags a 1.1 that is not a
  release candidate. `proto/UPSTREAM` stays `v1.0.1`.
- **1.1 is a moving target.** `v1.1.0-rc` is `78e1d6a1` (2026-10-29). The `dev-1.1` tip was `3d92e711`
  (2026-11-24) when this was written, and its two commits since the rc renamed a `oneof` member (breaking
  the rc's JSON), turned a MUST NOT into a hint, added six timestamp fields and relaxed the `ListTasks`
  ordering MUST (E01 §1.4). **Re-run E01 §1.1's commands before any SPC package from SPC-05 on** and say in
  the pull request what moved.
- **Stable enough to build behind a feature** (E01 §5.4): `generation` on the task and its events,
  `currentGeneration` long-polling, `ifGenerationMatch` with `-32010`, `TimelineEntry`, the gating of
  `messageUpdate` by requested version, the unknown-payload skip rule, `userArtifacts`.
- **Held until a release** (E01 §5.2, §5.3): `SendLiveMessage` on every binding (no binding section defines
  it), `timeline_length` (an unmerged branch deletes it), timestamps, list re-ordering, `StreamError`
  delivery, declaring a 1.1 interface, re-pinning the proto, re-anchoring the main ledger.

---

## SPC-01: Make a pre-release visible

- **Goal.** A new upstream pre-release tag, or a moved `dev-1.1`, is reported by the tool that already
  watches upstream. The rc went unseen for weeks because that tool excludes pre-releases by design.
- **Evidence.** E01 §6 item 1; `docs/handoff.md` *Now* ("nothing here reported it").
- **Gates.** none. **Depends.** none. **Release.** internal.
- **Read first.** `scripts/check_a2a_proto_upstream.sh`, `proto/UPSTREAM`, `proto/README.md`.
- **Do.** Report, without failing, each pre-release tag newer than the pinned tag (name, commit, proto
  SHA-256) and the `dev-1.1` tip. Exit codes as today.
- **Touch.** `scripts/check_a2a_proto_upstream.sh`; the workflow that runs it if its summary is parsed.
- **Accept.** With network, the output names `v1.1.0-rc`, `78e1d6a1` and the SHA-256 beginning `8d9c363f`;
  the exit code is 0 while `proto/UPSTREAM` says `v1.0.1`; without network the exit code is what it is
  today (state it). `scripts/prove_workflow_gates_fail.py` exits 0.
- **Out of scope.** Anything that reads the rc into a build.

## SPC-02: Vendor the 1.1 text as reference, outside every build path

- **Goal.** The delta ledger has a fixed text to cite, and no crate can compile it by accident.
- **Evidence.** E01 §6 item 2; E01 §3.0 (six byte-identical proto copies pinned by hash).
- **Gates.** DEC-02. **Depends.** SPC-01. **Release.** documentation.
- **Do.** Add the rc's and the `dev-1.1` tip's `specification/a2a.proto` and `docs/specification.md` under
  `docs/implementation/`, each with a provenance entry (ref, commit, SHA-256, date fetched). A check that a
  reference file's hash equals its entry.
- **Touch.** `docs/implementation/`, `scripts/check_proto_copies.sh` or a sibling, `docs/README.md`.
- **Accept.** `./scripts/check_proto_copies.sh` exits 0 with the six pinned copies untouched;
  `grep -rn 'v1.1' crates/*/build.rs tools/a2a-codegen` prints nothing; altering one byte of a reference
  file fails the new check.
- **Out of scope.** Generating anything from them.

## SPC-03: A delta ledger

- **Goal.** Every normative line 1.1 adds, changes or removes has a row saying what this SDK does about it
  and how settled it is upstream.
- **Evidence.** E01 §6 item 3; §1.1 (296 keyword-bearing lines at 1.0.1, 342 at the rc, 347 at the
  `dev-1.1` tip); §2 is the seed.
- **Gates.** DEC-02. **Depends.** SPC-02. **Release.** documentation and a gate.
- **Read first.** `scripts/check_spec_ledger.py` (it keys on keyword lines of the vendored text),
  `docs/compliance/spec-ledger.json` (its `after_release` block already holds post-1.0.1 requirements,
  §7.6.4 among them).
- **Do.** `docs/compliance/spec-ledger-1.1-delta.json`: one entry per keyword line added, changed or removed
  between the two vendored texts, with the existing status vocabulary, an `upstream_stability` of
  `released`, `rc`, `post-rc` or `tentative`, and the ACTS test id where upstream's
  `conformance-spec-1.1` branch has one (E01 §2.9 lists the 60). A checker mode with the two failure cases
  E01 names. Do not build a second 510-row ledger.
- **Touch.** The new ledger and its generated Markdown, `scripts/check_spec_ledger.py`,
  `.github/workflows/ci.yml`, `scripts/prove_gates_fail.sh`, `SPEC_COMPLIANCE.md` (a pointer).
- **Accept.** E01 §6 item 3's check as written. `python3 scripts/check_spec_ledger.py` still prints 510
  entries for 1.0.1 with the same status counts. The new gate is proven able to fail.
- **Out of scope.** Changing any 1.0.1 row.

## SPC-04: The client skips what it does not recognise

- **Goal.** A stream that carries a payload kind, or a timeline entry kind, this client does not know
  continues; today it fails.
- **Evidence.** E01 §3.1 rows S1 to S7 and T1 to T7: `C/streaming/event_stream.rs` returns
  `ClientError::Serialization` for a frame that is not a known `StreamResponse`;
  `C/transport/grpc.rs` has a test, `grpc_stream_reader_task_rejects_empty_payload`, asserting an empty
  payload ends the stream; `T/oneof.rs` errors when no known member is set. 1.1 says a client MUST skip
  (E01 §2.5 S5, §2.3 T7). It is not a 1.0.1 violation; it is the cheapest forward-compatible change there
  is.
- **Gates.** none. **Depends.** none. **Release.** `[0.16.x]` as a behaviour change, labelled; it needs no
  1.1 type.
- **Read first.** The three files above; `C/transport/rest/` (the error-frame path, which must still end a
  stream); ADR 0005.
- **Do.**
  1. Reproduce on each framing: SSE under JSON-RPC, SSE under HTTP+JSON, gRPC (a `StreamResponse` with an
     unknown field number, which prost decodes to no payload), WebSocket. Feed an unknown kind, then a
     terminal status update; assert the status update is yielded and the stream ends cleanly.
  2. Skip, and count the skip (a debug log and a metric through the existing client call metric; no new
     public type). A frame that sets two known members is still an error. A frame that is not JSON is still
     an error.
  3. Replace the gRPC test with one that asserts the skip.
- **Touch.** `C/streaming/event_stream.rs`, `C/transport/grpc.rs`, `C/transport/websocket.rs`, `T/oneof.rs`
  only if the skip belongs there, tests, `CHANGELOG.md` (*Behaviour Changes*), the upgrade guide if the
  section requires one.
- **Accept.** The four tests fail before, pass after. The two "still an error" tests pass. The fuzz targets
  that feed the stream parser still build (`python3 scripts/check_fuzz_matrix.py`).
- **Out of scope.** Parsing `messageUpdate` (SPC-08).

## SPC-05: Carry the negotiated version through the request path

- **Goal.** The version a request asked for reaches the handler, the subscriber registry and push-config
  creation, with one served version as today.
- **Evidence.** E01 §4.2 (`S/dispatch/version.rs`: `validate_version_header` returns `Result<(), A2aError>`,
  so the version "is checked and discarded"; `is_served_version` compares to one constant), §4.3 items 1
  and 2; the call sites E01 lists in the JSON-RPC, REST, WebSocket, axum and gRPC dispatchers and the
  SLIMRPC binding.
- **Gates.** DEC-02. **Depends.** none. **Release.** `[0.17]` if a public signature changes, else
  `[0.16.x]`.
- **Read first.** `S/dispatch/version.rs`, `S/call_context.rs`, each call site,
  `crates/a2a-protocol-sdk/tests/binding_equivalence/version.rs`.
- **Do.** The validator returns the parsed `(major, minor)`; it is stored on the call context; served
  versions become a set that holds only `1.0`. `validate_version_metadata` is public: keep its signature or
  deprecate it by the rule.
- **Touch.** `S/dispatch/version.rs`, `S/call_context.rs`, the dispatchers,
  `bindings/a2a-protocol-slimrpc/src/binding.rs`, tests.
- **Accept.** E01 §6 item 5's checks as written: the "1.1 is refused" assertions still pass; a new test
  reads the negotiated version from the context on every binding; `cargo semver-checks` reports no
  unannounced break.
- **Out of scope.** Serving a second version.

## SPC-06: The client selects an interface by a version set

- **Goal.** The client chooses among the versions it speaks and sends the version of the interface it
  chose.
- **Evidence.** E01 §4.2 (client: `C/builder/selection.rs` `is_compatible` accepts only `1.0`; seven send
  sites send the constant), §4.3 item 5.
- **Gates.** DEC-02. **Depends.** none. **Release.** `[0.16.x]`, no behaviour change with the default set.
- **Do.** `is_compatible` takes a set; the newest version both sides speak wins; each transport sends the
  selected interface's version. The default set is `{1.0}`.
- **Touch.** `C/builder/selection.rs`, `C/builder/mod.rs`, the transports and `C/discovery.rs`,
  `C/version_query.rs`, `bindings/a2a-protocol-slimrpc/src/client/`, tests.
- **Accept.** E01 §6 item 6's checks as written. `C/builder/selection_tests.rs` passes unmodified.
- **Out of scope.** A public way to widen the set (it arrives with SPC-08's feature).

## SPC-07: The 1.1 design record

- **Goal.** The questions 1.1 raises against this SDK's existing designs are answered once, before any type
  work.
- **Evidence.** E01 §6 item 7 and the "Design questions" column of §3.1.
- **Gates.** DEC-02. **Depends.** SPC-03. **Release.** an ADR.
- **Do.** One ADR, each question with a chosen answer and a rejected alternative:
  1. One superset type surface with behaviour chosen at run time, a feature gate, or a parallel module.
  2. `MessageSendParams.message` becoming optional (E01 §2.4 A3): the one change the generated types cannot
     absorb additively. A break in a minor, or a parallel type.
  3. `generation` against ADR 0012's `seq`: `seq` is sparse and gap-tolerant and gives client messages no
     position; 1.1 needs a dense counter from 1. A second counter persisted in the same write as the
     snapshot, or a redesign of `seq`.
  4. Steering (ADR 0036) once the core protocol lets a server accept messages in any non-terminal state:
     does the extension stay, and does a live uplink message go to the steering inbox.
  5. `Last-Event-ID` against `lastSeenGeneration`: precedence when both are sent.
  6. Whether 1.1 fields are sent on a response to a 1.0 request (upstream's own example does; E01 §2.8 C9).
  7. Stream lifetime: 1.1 says an interrupted state SHOULD NOT close a stream, which reverses 1.0.1 §11.7
     and changes what a turn owns here (E01 §2.6).
  8. The spec-added-field rule (ROADMAP conformance item 6): minor or patch.
- **Touch.** `docs/adr/` (the next number; check `ls docs/adr`), the ADR indexes, `ROADMAP.md` item 8 (a
  link).
- **Accept.** The ADR exists with eight answers and eight rejected alternatives; ADR 0012 and ADR 0036 each
  gain a dated pointer to it.
- **Out of scope.** Code.

## SPC-08: 1.1 types behind an unstable, off-by-default feature

- **Goal.** The 1.1 wire types can be parsed and produced, and no default build compiles them.
- **Evidence.** E01 §6 item 8; `CONTRIBUTING.md` *Changing a protocol type*; ADR 0016; E07 §B6 and §B9.
- **Gates.** DEC-02. **Depends.** SPC-07, FND-12. **Release.** `[0.17]`; the feature is named unstable and
  is outside `STABILITY.md`'s guarantee (add it to §6 there).
- **Read first.** `crates/a2a-protocol-types/codegen.toml`, `tools/a2a-codegen/`, `T/generated/`, `T/oneof.rs`,
  `T/whole_number.rs` (`int64` is a JSON string in ProtoJSON; E01 §3.1 row G1).
- **Do.** Per the ADR's answer to question 1. The codegen entry for every new message and field; golden
  round-trip fixtures; both spellings of the `StreamRequest` member recorded, since the rc and `dev-1.1`
  differ.
- **Touch.** The types crate, `tools/a2a-codegen/`, fixtures, `STABILITY.md` §6, the feature tables.
- **Accept.** E01 §6 item 8's checks as written. With default features, `cargo semver-checks` and a diff of
  the public API show no change. `python3 scripts/check_feature_tables.py` exits 0. The CLIENT-PARSE-007,
  -009 and -010 payloads from upstream's `conformance-spec-1.1` branch parse (vendor the three payloads
  with their source commit).
- **Out of scope.** Any behaviour. `SendLiveMessage`.

## SPC-09: Server-side `generation`, behind the same feature

- **Goal.** The most settled part of 1.1 works on every store: a dense counter, long-polling and
  compare-and-append.
- **Evidence.** E01 §2.2 (G1 to G10), §3.1 rows G1 to G8, §6 item 9; E07 §B10 (the 27 touch points of a
  store-backed entity; the next migration version on both databases).
- **Gates.** DEC-02. **Depends.** SPC-05, SPC-08. **Release.** `[0.17]`, behind the feature.
- **Read first.** ADR 0012, ADR 0057 (the wake a long poll should share), `S/store/task_store/mod.rs` (the
  event log is defaulted methods on `TaskStore`), `S/handler/lifecycle/get_task.rs`,
  `S/handler/messaging/continuation.rs`, `T/error.rs` (a test there treats `-32010` as unassigned).
- **Do.**
  1. The counter is persisted in the same write as the snapshot, in every store and wrapper. State the
     migration number.
  2. Compare-and-append for `ifGenerationMatch` is atomic with the bump in every store.
  3. A long poll holds a connection and an admission slot: say which limit bounds it and give it a timeout.
  4. With the feature off, serialized output is byte-identical to 0.16.0 fixtures.
- **Touch.** Per E07 §B10's list, plus the error mappings in `S/dispatch/`.
- **Accept.** E01 §6 item 9's checks as written, on the in-memory, SQLite and PostgreSQL stores; a restart
  test shows the counter continues; `tests/statement_count.rs` is unchanged with the feature off.
- **Out of scope.** The timeline, `messageUpdate`, user artifacts (each its own package after this one,
  written when SPC-07 has answered its questions).

## SPC-10: What waits for a 1.1 release

- **Goal.** A list a session can act on the day upstream tags 1.1, and a test that nothing leaked before.
- **Evidence.** E01 §6 item 10, §5.2, §5.3.
- **Gates.** DEC-02, and an upstream tag that is not a release candidate. **Depends.** SPC-09.
- **Do.** When the tag exists: re-run E01 against it and replace `evidence/01`'s §2 with the released delta;
  then, each as its own package written at that time, `SendLiveMessage` per binding, `timeline_length` or
  whatever replaced it, timestamps, list ordering, the 1.1 interface on the card, the proto re-pin, the
  main ledger re-anchored (§8.2 is removed in 1.1, so every row citing §8.3 to §8.6 moves), and the 1.1
  `tck-*` prefixes in `acts/sut-behaviors.yaml`. Until then, add one test: no card built by this SDK with
  default features declares `protocolVersion` 1.1.
- **Touch.** A test now; the rest later.
- **Accept.** The test passes; `proto/UPSTREAM` says `v1.0.1`.
- **Out of scope.** Everything on the list, until the tag.

## SPC-11: Re-diff on every upstream movement

- **Goal.** The delta ledger cannot go stale silently.
- **Evidence.** E01 §6 item 11.
- **Gates.** DEC-02. **Depends.** SPC-03. **Release.** internal.
- **Do.** A scheduled job that compares the vendored reference with `dev-1.1` and fails, listing the changed
  keyword lines, when the proto hash or the keyword-line set moved. Informational: it gates no pull
  request (the same shape as the nightly official TCK; `docs/handoff.md`, decision of 2026-11-06, option D).
- **Touch.** `.github/workflows/`, `scripts/lib/workflow_inventory.py`, `scripts/prove_workflow_gates_fail.py`
  (a probe or an exemption with its reason).
- **Accept.** The job fails against a reference deliberately one commit old and passes against the current
  one; `scripts/prove_workflow_gates_fail.py` exits 0.
- **Out of scope.** Updating the ledger automatically.

## SPC-12: The 1.0.1 remainder

- **Goal.** Every open 1.0.1 conformance row is closed or carries its reason in one place.
- **Evidence.** ROADMAP *A2A v1.0.1 conformance gaps* items 2 to 4; `docs/handoff.md` *Next, in order* item
  2 (why the four PARTIAL rows cannot all close from here); E06 §6 rows 11 and 19.
- **Gates.** none, except the executor mailbox for §7.6.1 (E06 §6 row 19), which this package does not
  build. **Depends.** none. **Release.** `[0.16.x]`.
- **Do.**
  1. The four PARTIAL rows (`python3 scripts/check_spec_ledger.py` lists them): a test for each, or the
     reason it stays partial, in the ledger row.
  2. The MUSTs the official suite marks manual and not tested: for each, the ledger cites a test here or
     says there is none (`tck/untested-musts.toml`; its header counts are two pins old, E06 §5 C-17).
  3. Run ACTS per binding and record this repository's own counts in
     `book/src/reference/conformance-statement.md`, which quotes a reviewer's.
- **Touch.** `docs/compliance/spec-ledger.json` and `.md`, `tck/untested-musts.toml`, the conformance
  statement, tests.
- **Accept.** `python3 scripts/check_spec_ledger.py` and `python3 scripts/check_tck_untested_map.py` exit 0;
  the statement's counts each name the run that produced them.
- **Out of scope.** The mailbox; the two upstream reports (ECO-02).

## SPC-13: A conformance run for the WebSocket binding

- **Goal.** The one binding outside both external suites has a suite.
- **Evidence.** ROADMAP *conformance gaps* item 5; ADR 0055.
- **Gates.** none. **Depends.** none. **Release.** `[0.17]`.
- **Read first.** `tck/` (the in-repo TCK and how it drives a binding), ADR 0055, the binding-equivalence
  tests under `crates/a2a-protocol-sdk/tests/binding_equivalence/`.
- **Do.** Drive the in-repo TCK's requirement set over the WebSocket binding where a requirement is
  binding-independent, and write the binding's own requirements (framing, upgrade refusals, idle bound)
  as a ledger for its URI.
- **Touch.** `tck/`, `.github/workflows/`, the binding's documentation.
- **Accept.** The run grades a stated number of requirements with none failing, in CI; an injected defect
  in the WebSocket dispatcher turns it red (`scripts/prove_workflow_gates_fail.py`).
- **Out of scope.** Proposing the binding upstream.
