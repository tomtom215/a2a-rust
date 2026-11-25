<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

> **Frozen audit input, 2026-11-25, of the tree at `2058058d`.** Not maintained. Written by a delegated
> read-only audit for [`docs/program/README.md`](../README.md); its VERIFIED and INFERRED labels are the auditor's own.
> Re-check an anchor before acting on it. `UPSTREAM/` is a local clone of the named upstream repository.

# A2A v1.0.1 -> v1.1 delta, mapped onto a2a-rust

Research date 2026-11-25. Read-only: nothing under `/home/user/a2a-rust` was modified, no cargo command was run.

Conventions

- `UP` = `UPSTREAM/A2A` (clone of github.com/a2aproject/A2A, `git fetch --all --tags` run 2026-11-25).
- `proto@rc:N` = line N of `specification/a2a.proto` at tag `v1.1.0-rc`; `proto@dev:N` = same file at `origin/dev-1.1`; `spec@rc:N` / `spec@dev:N` = `docs/specification.md` at those refs; `spec@1.0.1:N` at tag `v1.0.1`.
- Repo paths are relative to `/home/user/a2a-rust`.
- **VERIFIED** = I opened the file or ran the command. **INFERRED** = a conclusion drawn from verified facts, not itself observed.
- Class: **WB** wire-breaking, **ADD** additive, **DEP** deprecation, **NORM** normative behaviour change without a wire change, **ED** editorial.

---

## 1. Provenance

### 1.1 Refs (VERIFIED: `git log -1 --format='%H %ci %s' <ref>`)

| Ref | Commit | Committer date | Subject |
|---|---|---|---|
| `v1.0.1` | `3303592588e388e62e0f69f701af531d2f4e3991` | 2026-05-28 13:34:16 +0200 | chore(main): release 1.0.1 (#1749) |
| `v1.1.0-rc` | `78e1d6a1f7f118c8e9bc830b04a8579c1119cb78` | 2026-10-29 12:05:26 +0000 | chore: merge main into dev 1.1 |
| `origin/dev-1.1` | `3d92e7113959d11d4ce3b9c3f1a1fa59b03b0015` | 2026-11-24 12:32:57 -0500 | feat: add timestamps across the task model (#2426) |
| `origin/main` | `80e4d59e98797fa101cb39506f967cfe4136d528` | 2026-11-24 12:48:48 -0500 | docs: add a Rust SDK page (#2442) |
| `origin/dev` | `78e1d6a1f7f118c8e9bc830b04a8579c1119cb78` | 2026-10-29 | same commit as `v1.1.0-rc` |
| `origin/conformance-spec-1.1` | `3252da724681a50daa15b0f7c414564974b1aa06` | 2026-11-18 11:36:56 +0000 | test: close two gaps in the generation tests and accept artifact update deliveries |

Other VERIFIED facts:

- `v1.1.0-rc` is a **lightweight** tag (`git cat-file -t v1.1.0-rc` -> `commit`); it carries no tagger, date or message of its own.
- `git merge-base v1.0.1 v1.1.0-rc` = `33035925` (v1.0.1 itself): the rc is a strict descendant of v1.0.1. 100 commits in `v1.0.1..v1.1.0-rc`.
- `git merge-base origin/main origin/dev-1.1` = `89699392`. `main` does **not** contain the rc. `git diff v1.0.1 origin/main -- specification/a2a.proto` is 5 lines: only the `AgentInterface.url` comment (gRPC address `hostname:port`). This agrees with `docs/handoff.md:39-45`.
- Proto SHA-256: v1.0.1 `e195bf96ab630c69797851970203e1b2b6b19528f2e9803b7d904b91a5104016` (equals `proto/UPSTREAM` and `sha256sum proto/a2a_v1/a2a.proto`); v1.1.0-rc `8d9c363f8654205780654306af307355a7a34dcf96a8e221caf251268deab127`; dev-1.1 tip `626a6776c4e71ef01c2e62fbca07963a931fcc11c6f7dbd1987a7547ead50c33`.
- Proto size: v1.0.1 44 messages / 11 RPCs; rc and dev-1.1 48 messages / 12 RPCs (`grep -c '^message '`, `grep -c '^  rpc '`). Package is still `lf.a2a.v1` (proto@rc:3).
- Lines carrying an RFC 2119 keyword in `docs/specification.md` (`grep -c 'MUST\|SHOULD\|REQUIRED\|SHALL\|RECOMMENDED\|MAY'`): 296 (v1.0.1) -> 342 (rc) -> 347 (dev-1.1). The ledger checker keys on exactly these lines (`scripts/check_spec_ledger.py:21-24`).

### 1.2 `git diff --stat v1.0.1 v1.1.0-rc` (VERIFIED; normative files only, full output was 70 files, 3092 insertions, 631 deletions)

```
 adrs/adr-002-coherent-task-history.md              | 237 +++++++++++
 docs/specification.md                              | 456 +++++++++++++++++----
 docs/topics/agent-discovery.md                     | 129 +++++-
 docs/topics/custom-protocol-bindings.md            |   9 +
 docs/topics/extension-and-binding-governance.md    |  15 +-
 docs/topics/extensions.md                          |  22 +-
 docs/topics/life-of-a-task.md                      | 110 +++--
 docs/topics/multi-tenancy.md                       |  35 +-
 docs/topics/streaming-and-async.md                 |  28 +-
 docs/roadmap.md                                    |  12 +-
 docs/whats-new-v1.md                               |  57 +--
 specification/a2a.proto                            | 226 +++++++++-
 70 files changed, 3092 insertions(+), 631 deletions(-)
```

`handoff.md:41` says "208 added and 18 removed lines" for `specification/`; the stat above (226 changed lines in a2a.proto) is consistent.

### 1.3 `git diff --stat v1.1.0-rc origin/dev-1.1` (VERIFIED)

```
 docs/specification.md   | 53 ++++++++++++++++++++---------
 specification/a2a.proto | 89 +++++++++++++++++++++++++++++++++++++++----------
 2 files changed, 110 insertions(+), 32 deletions(-)
```

### 1.4 Is v1.1 final? No. (VERIFIED)

Commits after the rc tag on `dev-1.1` (`git log v1.1.0-rc..origin/dev-1.1`):

| Commit | Date | Subject | Effect |
|---|---|---|---|
| `24bbb098` | 2026-11-17 | fix: refine live streaming and user artifact semantics (#2411) | Renames a `oneof` member (JSON-breaking vs rc), reverses a MUST NOT, adds StreamError MUST NOTs |
| `3d92e711` | 2026-11-24 | feat: add timestamps across the task model (#2426) | 6 new fields, 2 new deprecations, **relaxes the ListTasks ordering MUST to SHOULD and changes the sort key** |

The newest of these is one day older than this report. No `CHANGELOG.md` entry for 1.1 exists on `main` (its top entry is 1.0.1) and `dev-1.1` has no release-please manifest (`git show origin/dev-1.1:.release-please-manifest.json` prints nothing). See section 5.

### 1.5 Other upstream repositories (VERIFIED)

- `a2aproject/a2a-tck` (clone at `UPSTREAM/a2a-tck`, only branch `origin/main`, tip `5e6f37c2` 2026-11-18, no tags): **no v1.1 content**. `specification/version.json:5` pins `"branch": "v1.0.1"`; `tck/transport/_helpers.py:14` hardcodes `A2A_VERSION = "1.0"`. `git grep -il 'SendLiveMessage\|timeline'` finds only unrelated backlog/doc files. No new requirement IDs for 1.1 exist there.
- `a2aproject/a2a-itk` (clone at `UPSTREAM/a2a-itk`, `main` tip `604239c6` 2026-11-19): `main` runs ACTS at `SPEC_VERSION = '1.0'` (`acts_runner.py:63`), corpus mirrored from A2A branch `conformance-spec-adjustments` commit `a8c99376` (`docs/conformance/06_status.md:19`). Thirteen unmerged branches `origin/jakubworek/v11-*` (tips 2026-11-20 to 2026-11-24) add 1.1: proto stubs generated from `v1.1.0-rc` (`v11-proto-stubs`, `6aa2ac9`), per-version runs, live exchange driver, and a wire map binding `SendLiveMessage` to JSON-RPC method `SendLiveMessage`, gRPC `SendLiveMessage`, REST `/message:live` (`test_suite/acts/wire_map.py:202-205` on `v11-acts-versioned-wire-map-and-live-steps`).
- The 1.1 conformance corpus lives in the **A2A repo itself**, branch `origin/conformance-spec-1.1` (`tests/acts/*.acts.yaml`, `spec_version: "1.1"`; 22 files, 12,753 insertions vs `main`). It adds **60 tests with `min_spec_version: "1.1"`** (counted by script over the 10 files that carry the key). Listed in section 2.9.

---

## 2. Every change

Numbering is stable; section 3 maps by number. "rc" column = present at `v1.1.0-rc`; "dev" = state at `origin/dev-1.1` tip where it differs.

### 2.1 RPCs and bindings

| # | Change | Upstream evidence | Class |
|---|---|---|---|
| R1 | New RPC `SendLiveMessage(stream StreamRequest) returns (stream StreamResponse)`, HTTP `POST /message:live` and `POST /{tenant}/message:live`, `body: "*"` | proto@rc:43-53 "Establishes a bidirectional stream for live interaction." | ADD |
| R2 | Binding table row: JSON-RPC `SendLiveMessage`, gRPC `SendLiveMessage`, REST `POST /message:live` | spec@rc:1355 | ADD |
| R3 | New §3.1.12 Send Live Message. Uplink/downlink model; on HTTP transports "the operation is invoked as a server stream. The invoking call's response is the **downlink** ... To send further input, the client makes **additional, short-lived calls to the same operation**" | spec@rc:432-482 | ADD |
| R4 | §3.1.12 errors: `UnsupportedOperationError` when `capabilities.streaming` is false/unset; `TaskNotFoundError` | spec@rc:448-451; capability rule spec@rc:721 "attempts to use `SendStreamingMessage`, `SubscribeToTask`, or `SendLiveMessage` operations **MUST** return `UnsupportedOperationError`" | NORM |
| R5 | §3.1.12 opening rules: first request binds the stream to exactly one task; `subscribe` only valid first, later "**MUST** reject it with a `StreamError` and keep the stream open"; request naming another task "**MUST** be rejected with a `StreamError`" | spec@rc:460-474 | ADD |
| R6 | rc: "An uplink call that is not the downlink call MUST return only an acknowledgement — the current `Task` state — and MUST NOT replay task events." **dev-1.1 demotes both MUSTs to descriptive text**: "Where a binding carries input on a separate call from the downlink, that call returns only an acknowledgement" | spec@rc:475 vs spec@dev:478 (diff hunk in #2411) | NORM, changed after rc |
| R7 | `returnImmediately` ignored on a live stream; a `taskPushNotificationConfig` supplied there "is equivalent to calling Create Push Notification Config" | spec@rc:477 | NORM |
| R8 | **No binding section defines SendLiveMessage.** §9.4 (JSON-RPC) lists 9.4.1-9.4.8, §10.4 (gRPC) 10.4.1-10.4.11, §11.3 (REST) lists no `/message:live`; none mentions it (`grep -n 'SendLiveMessage' spec@dev` hits only lines 444-486, 740, 1374) | spec@dev:2520-2670, 2790-2906, 3020-3045 | unfinished upstream |
| R9 | SubscribeToTask REST verb in prose corrected `POST` -> `GET` (#2068). The **proto already said GET at v1.0.1** (`get: "/tasks/{id=*}:subscribe"`, proto@1.0.1:78) | spec@rc:1359, spec@rc:3009; spec@1.0.1 had `POST /tasks/{id}:subscribe` in both places | ED (doc catches up to proto) |
| R10 | §3.2.5/§11.5: query parameter `currentGeneration` added to the mapping table and a long-poll example | spec@rc:3081, 3094-3100 | ADD |

### 2.2 Generation, long-polling, optimistic concurrency

| # | Change | Upstream evidence | Class |
|---|---|---|---|
| G1 | `Task.generation` (`int64`, field 7). "incremented by exactly 1 by the server on every state-changing mutation ... Starts at 1 when the task is created." | proto@rc:214-222; §3.2.7 spec@rc:572-578 "**MUST** be incremented by exactly `1`" | ADD + NORM |
| G2 | `TaskStatusUpdateEvent.generation` (5), `TaskArtifactUpdateEvent.generation` (7) | proto@rc:407, 428 | ADD |
| G3 | Streaming mutations "**MUST** map 1:1 to exactly one emitted event whose `generation` value reflects the task's generation **after** the mutation" | spec@rc:580 | NORM |
| G4 | "A correctly-implemented server **MUST NOT** emit two events for the same task with the same `generation` value." Client rule: on `generation = 0` or two consecutive equal values it "**MUST** conclude that the server does not implement the `generation` field and **MUST NOT** use `generation` for ordering, long-polling, or precondition checks"; "**SHOULD** fall back to stream ordering or timestamps" | spec@rc:587 | NORM (client) |
| G5 | Client on a gap "**SHOULD** re-fetch the full task state via Get Task" | spec@rc:585; repeated in §3.5.2 "Clients **SHOULD** use this value to detect when their view is behind" (added paragraph after "Events MUST NOT be reordered") | NORM (client) |
| G6 | `GetTaskRequest.current_generation` (`optional int64`, 4): unset -> "**MUST** return ... immediately"; already greater -> "**MUST** return the current state immediately"; equal or less -> "**SHOULD** hold the request open ... **MAY** return earlier" | proto@rc:820-825; spec@rc:589-595 | ADD + NORM |
| G7 | `SendMessageConfiguration.if_generation_match` (`optional int64`, 5). Mismatch -> "**MUST** reject the request with `TaskGenerationMismatchError` (HTTP `409 Conflict` / gRPC `ABORTED`)"; "If the task ... does not yet exist ... this field **MUST** be ignored" | proto@rc:178-183; spec@rc:521 | ADD + NORM |
| G8 | New error `TaskGenerationMismatchError`: JSON-RPC `-32010`, gRPC `ABORTED`, HTTP `409 Conflict`; ErrorInfo reason `TASK_GENERATION_MISMATCH`, domain `a2a-protocol.org`, metadata `taskId`, `expectedGeneration`, `currentGeneration` | spec@rc:710, 1381, 3167-3194 | ADD |
| G9 | New "Long-Polling" bullet list in §3.5 update-delivery overview | spec@rc (hunk at 804-812 of diff) | ED |
| G10 | Back-compat statement: "`generation` field was introduced in version **1.1** ... Servers that have not yet implemented `generation` will return `0`" | spec@rc:614 (dev:621) | NORM (compat) |

### 2.3 Timeline and history deprecation

| # | Change | Upstream evidence | Class |
|---|---|---|---|
| T1 | New message `TimelineEntry { oneof entry { Message message = 1; TaskStatus status = 2; } int64 generation = 3; }` | proto@rc:359-376 | ADD |
| T2 | `Task.timeline` (`repeated TimelineEntry`, 8) | proto@rc:207-211 | ADD |
| T3 | `Task.history` marked `[deprecated = true]`: "Servers continue to populate `history` throughout the v1.x line ... it is removed in 2.0." | proto@rc:202-206 | DEP |
| T4 | `history_length` deprecated on `SendMessageConfiguration` (3), `GetTaskRequest` (3), `ListTasksRequest` (6) | proto@rc:166, 814, 848; spec@rc:547 "Servers continue to honour `historyLength` and populate `history` throughout the 1.x line; both are removed in 2.0." | DEP |
| T5 | `timeline_length` (`optional int32`) added: `SendMessageConfiguration` 6, `GetTaskRequest` 5, `ListTasksRequest` 9. "The server MUST NOT return more entries than this value, but MAY apply a lower limit." | proto@rc:167-171, 815-819, 849-851 | ADD + NORM. **Contested**, see 5.3 |
| T6 | §3.2.8 Task Timeline Semantics: "Servers **MUST** assign each appended entry the task's `generation` after the append"; "consumers **MUST NOT** assume timeline entries have consecutive `generation` values"; "Servers **SHOULD** populate `timeline`; servers that also support 1.0 clients continue to populate `history`" | spec@rc:617-627 | NORM |
| T7 | Fail-open: "Clients **MUST** fail open — a client that encounters a `TimelineEntry` whose `entry` is a kind it does not recognise **MUST** skip that entry" | spec@rc:630; proto@rc:356-358 | NORM (client) |
| T8 | New message `TaskMessageUpdateEvent { task_id=1 REQUIRED; context_id=2 REQUIRED; message=3 REQUIRED; metadata=4; generation=5 }` and `StreamResponse.message_update = 5`. "Emitted only to clients that negotiated a compatible protocol version (>= 1.1)." | proto@rc:378-392, 967 | ADD, version-gated |
| T9 | `SubscribeToTask` output list gains "(for `A2A-Version: 1.1` or later) `TaskMessageUpdateEvent`" | spec@dev:303 | NORM |
| T10 | Deprecation notes on §3.2.4 and §3.7 (`history`) | spec@rc:547; hunk at "@@ -757,6 +915,8" | DEP/ED |
| T11 | ADR-002 "Coherent Task History" added, **Status: Proposed**. It also plans an `elicitations` collection and soft-deprecation of `TaskStatus.message` (ADR §Implementation items 4, 5). **Neither is in the proto at rc or dev-1.1** (`grep -in elicitation` on both files: no hit; `TaskStatus.message` carries no `deprecated` option, proto@rc:262) | `adrs/adr-002-coherent-task-history.md` at dev-1.1, lines 3, "Implementation" list | document/artifact disagreement; artifact wins |

### 2.4 Client-written artifacts

| # | Change | Upstream evidence | Class |
|---|---|---|---|
| A1 | `AgentCapabilities.user_artifacts` (`optional bool`, 5) | proto@rc:528-530 | ADD |
| A2 | `SendMessageRequest.artifact_updates` (`repeated TaskArtifactUpdateEvent`, 5). "clients MUST leave [generation] unset and servers MUST ignore any value sent" | proto@rc:773-780 | ADD + NORM |
| A3 | **`SendMessageRequest.message` loses `[(google.api.field_behavior) = REQUIRED]`.** "Required when creating a task ...; may be omitted only when sending `artifact_updates` to an existing task." | proto@1.0.1 `Message message = 2 [(google.api.field_behavior) = REQUIRED];` -> proto@rc:767-770 `Message message = 2;` | WB for typed SDKs (a REQUIRED field becomes optional); additive on the wire |
| A4 | Capability validation: `userArtifacts` false/absent -> `artifactUpdates` "**MUST** return `UnsupportedOperationError` (or `StreamError` on a live stream)" | spec@rc:723 | NORM |
| A5 | "`message` **MUST** be present when creating a new task"; ordering: message appended to timeline first, then `artifactUpdates` in list order, "advancing `generation` for each" | spec@rc:493-499 | NORM |
| A6 | `Artifact.start_generation` (7), `Artifact.end_generation` (8), `Artifact.role` (`Role`, 9) | proto@rc:339-347 | ADD |
| A7 | dev-1.1 only: `role` semantics hardened — "servers **MUST** populate it on the stored `Artifact` and on any emitted `TaskArtifactUpdateEvent.artifact` ... consumers **MUST** treat `ROLE_UNSPECIFIED` as `ROLE_AGENT`"; cross-role update the agent refuses "**MUST** reject the update (`InvalidParamsError` ... or `StreamError`)" | spec@dev:502; proto@dev comment on `role = 9` | NORM, added after rc |

### 2.5 Stream envelope types

| # | Change | Upstream evidence | Class |
|---|---|---|---|
| S1 | New `StreamRequest { string tenant = 1; oneof payload { SendMessageRequest send_message = 2; SubscribeToTaskRequest subscribe = 3; } }` | proto@rc:788-800 | ADD |
| S2 | **dev-1.1 renames `send_message` -> `request`** (same field number 2). Binary-compatible; ProtoJSON key changes `sendMessage` -> `request` | proto@rc:795 vs proto@dev:842; spec@dev:468 "Opening with `request`" | **WB between rc and dev-1.1 (JSON)** |
| S3 | New `StreamError { task_id=1; code=2 REQUIRED; message=3; reference_id=4; metadata=5 }` and `StreamResponse.error = 6`. `code` is a **string** "using the same codes as the equivalent unary operation (e.g. `INVALID_ARGUMENT` ...)" | proto@rc:971-991, 969 | ADD |
| S4 | dev-1.1: StreamError "**MUST NOT** mutate task state, **MUST NOT** be appended to `Task.timeline`, and **MUST NOT** advance `Task.generation` ... **MUST NOT** be delivered to other subscribers or to push notification webhooks" | spec@dev:486 | NORM, added after rc |
| S5 | Forward compatibility of `StreamResponse.payload`: "A client that receives a `StreamResponse` whose payload kind it does not recognise **MUST** skip it and continue reading the stream. It **MUST NOT** treat the unknown payload as an error, as the end of the stream" | spec@rc:534-540; proto@rc:952-954 | NORM (client) |
| S6 | Server: unknown `StreamRequest` payload kind "**MUST** reject that request ... and **MUST NOT** terminate the stream" | spec@rc:538; proto@rc:783-787 | NORM |
| S7 | "Servers **MAY** still withhold a new payload kind from clients that negotiated an older protocol version" | spec@rc:542 | NORM |
| S8 | `SubscribeToTaskRequest.last_seen_generation` (`optional int64`, 3). **rc**: "the server replays only what followed it and does NOT open with a `Task` snapshot" (§3.1.12: "**MUST NOT** open with a `Task` snapshot"). **dev-1.1 reverses it to a hint**: "the server **MUST** either continue the stream from that `generation` or open with a `Task` snapshot ... failing the subscription is not an option" | proto@rc:912-915, spec@rc:466 vs proto@dev:962-966, spec@dev:317, 471 | ADD; semantics changed after rc |
| S9 | rc internal contradiction: §3.1.6 at rc still said "The operation MUST return a `Task` object as the first event" unconditionally while proto@rc:913 said the opposite when `last_seen_generation` is set. Resolved in dev-1.1 (spec@dev:315-317) | spec@rc:313 (no §3.1.6 hunk in the rc diff) | rc defect, fixed |

### 2.6 Task state and stream lifetime

| # | Change | Upstream evidence | Class |
|---|---|---|---|
| L1 | §4.1.3: terminal state — "the server **MUST NOT** accept further messages or emit further updates for it" | spec@rc:952 | NORM (was implied by CORE-SEND-002 in 1.0) |
| L2 | Non-terminal incl. interrupted: server "**MAY** continue processing the task and transition it to any other state — including out of an interrupted state — without a message from the client, and **MAY** continue to accept messages directed to the task" | spec@rc:953; proto@rc:226-233 | NORM |
| L3 | §3.1.2: "A non-terminal state, including an interrupted state (`TASK_STATE_INPUT_REQUIRED`, `TASK_STATE_AUTH_REQUIRED`), SHOULD NOT by itself close the stream." | spec@rc:210 | **NORM, reverses 1.0.1 §11.7** |
| L4 | §11.7: 1.0.1 "until the task reaches a terminal or interrupted state, at which point the stream closes" -> 1.1 "until the task reaches a terminal state ... A non-terminal state, including an interrupted state ..., does not necessitate closing the stream" | spec@1.0.1 §11.7; spec@rc:3225 | NORM |

### 2.7 Timestamps and ListTasks ordering (dev-1.1 only, #2426, not in the rc)

| # | Change | Upstream evidence | Class |
|---|---|---|---|
| D1 | `Task.updated_at` (9), `Task.created_at` (10), `Artifact.updated_at` (10), `TimelineEntry.timestamp` (4), `TaskMessageUpdateEvent.timestamp` (6), `TaskStatusUpdateEvent.timestamp` (6), all `google.protobuf.Timestamp` | proto@dev:229, 234, 374, 409, 430, 453 | ADD |
| D2 | `TaskStatus.timestamp` (3) `[deprecated = true]`; "Servers continue to populate this field throughout the v1.x line" | proto@dev:273-281 | DEP |
| D3 | `ListTasksRequest.status_timestamp_after` (7) `[deprecated = true]`; "Servers continue to honour it throughout the v1.x line" | proto@dev:899-904 | DEP |
| D4 | "A server **MUST** set `timestamp` on every `TimelineEntry` it returns and on every event that carries one, **SHOULD** set `updatedAt` ..., **SHOULD** set `createdAt` ..., and **MUST** keep `Task.updatedAt` non-decreasing ... While the deprecated `status.timestamp` is still populated it **MUST** equal the `timestamp` on the `TaskStatusUpdateEvent` carrying it." | spec@dev:641 | NORM |
| D5 | "a client **MUST NOT** order a timeline by wall-clock time" | spec@dev:643 | NORM (client) |
| D6 | **ListTasks ordering relaxed and re-keyed.** v1.0.1/rc: "Tasks MUST be sorted by last update time in descending order" and "Implementations MUST return tasks sorted by their status timestamp time in descending order". dev-1.1: first sentence deleted; "Implementations **SHOULD** return tasks sorted by `Task.createdAt` in descending order ... Implementations **MAY** use a different order, such as `Task.updatedAt` ... Clients **MUST NOT** assume a stable order across pages unless the order is by `createdAt`." | spec@1.0.1 §3.1.4 (ledger rows spec_line 254, 262); spec@dev:256, 264-266 | **NORM: two MUSTs removed, default sort key changes** |
| D7 | Proto/spec disagreement inside dev-1.1: proto says of `Task.updated_at` "The server MUST set it when the task is created and on every mutation" and of `Artifact.updated_at` "the server MUST set it"; spec@dev:641 says "**SHOULD** set `updatedAt` on every `Artifact` and `Task`". §1.4 of the spec makes the proto normative | proto@dev:223-229, 368-374 vs spec@dev:641 | upstream inconsistency |

### 2.8 Agent Card, discovery, security, versioning, IANA, editorial

| # | Change | Upstream evidence | Class |
|---|---|---|---|
| C1 | `TaskPushNotificationConfig.protocol_version` (`string`, 7): "The A2A protocol version to use when delivering the notification. Defaults to the version of the request that created the configuration, or "1.0" if unset." | proto@rc:595-598 | ADD + NORM |
| C2 | `AgentInterface.url` comment: gRPC "address should be in the format "hostname:port"", example `grpc.example.com:443` (also on `main`) | proto@rc:444-446 | ED (guidance) |
| C3 | §8.2 "Discovery Mechanisms" **removed** from the spec; §8.3-8.6 renumbered to §8.2-8.5. The well-known URI bullet ("Accessing `https://{server_domain}/.well-known/agent-card.json`") leaves the normative document | spec@1.0.1 §8.2; spec@rc §8 headings | ED/renumber; **every ledger row citing §8.3-§8.6 moves** |
| C4 | New media type `application/a2a-agent-card+json` (§8.1 sentence + §14.1.2 registration template). "These templates do not imply that IANA registration has been completed." | spec@rc:2173, 3534; §14.1 intro | ADD |
| C5 | Discovery topic: AI Catalog (`/.well-known/ai-catalog.json`) is now "Preferred"; well-known agent card demoted to "A simple option for single-agent deployments"; multi-tenancy topic: shared domain "**SHOULD** publish an AI Catalog" | `docs/topics/agent-discovery.md`, `docs/topics/multi-tenancy.md` at rc (diff read) | guidance (topics are non-normative), ADD |
| C6 | IANA well-known entry: "It is limited to advertising a single Agent Card per domain."; "Specification document: Section 8" | spec@rc:3643 | ED |
| C7 | New §7.6.4 In-Task Authorization Scope: "Agents MUST NOT treat the `TASK_STATE_AUTH_REQUIRED` state transition, by itself, as authorization ..."; three more MUST/MUST NOT sentences | spec@rc:2155-2163 | NORM |
| C8 | §3.6 Versioning text **unchanged** (no hunk in either diff). `A2A-Version` examples still say `1.0` or `0.3`; every Agent Card example still declares `protocolVersion: "1.0"` (spec@dev:1501, 2255, 2382-2384). The only places the document says `1.1` as a version are prose gates (spec@dev:303, 553, 587, 621, 649, 937) | spec@dev:886-918 | no change |
| C9 | The long-poll example sends a 1.1-only field under `A2A-Version: 1.0`: `GET /tasks/task-abc?currentGeneration=3` with `A2A-Version: 1.0` | spec@dev:608-610 | upstream ambiguity (section 4.3) |
| C10 | `PushNotificationConfig` -> `TaskPushNotificationConfig` naming fixed across spec prose and anchors (#1981); §10.4.7 request text changed | spec@rc multiple hunks | ED |
| C11 | §8.3.1 canonicalization example replaced by a valid Agent Card (#2319); sample card `security` -> `securityRequirements` with `schemes`/`list` shape (#2046) | spec@rc §8.3.1, §8.4 | ED (examples; the second corrects a wrong example) |
| C12 | Appendix A: "`protocolVersions` in the `AgentCard`" -> "`protocolVersion` in the agent's `supportedInterfaces` entries" | spec@rc Appendix A hunks | ED |
| C13 | RFC links moved to datatracker.ietf.org; contact changed to `A2A-TSC@lists.a2aproject.org`; SSE examples gain `contextId`; §3.2.5 metadata grammar | spec@rc | ED |
| C14 | URI namespaces note: `https://a2a-protocol.org/bindings/{name}/v1` and `/extensions/{name}/v1` "are identifiers, HTTP access is not expected" | `docs/topics/custom-protocol-bindings.md`, `extensions.md`, `extension-and-binding-governance.md` at rc | ED (guidance) |
| C15 | §14.1.2 cites "Section 8.4" for Agent Card signing, which the same release renumbered to §8.3 | spec@rc §14.1.2 security considerations | upstream stale cross-reference |

Nothing was **removed** from the proto and no field was **renumbered** between v1.0.1 and the rc (VERIFIED by reading the whole `git diff v1.0.1 v1.1.0-rc -- specification/a2a.proto`). No new `TaskState` value, no new `Role` value, no new security scheme.

### 2.9 New conformance tests (`origin/conformance-spec-1.1`, `min_spec_version: "1.1"`), 60 in total

VERIFIED by script over `tests/acts/*.acts.yaml` at `3252da72`. Level in parentheses; `T` = tagged `tentative`.

- client-parsing: CLIENT-PARSE-007 (must) generation/timeline/artifact fields; -008 (must) skip unknown stream event; -009 (must) parse `messageUpdate`; -010 (must) skip unknown timeline entry; -011 (must, T) parse `StreamError`.
- discovery: CARD-DISC-008 (should) card lists an interface with `protocolVersion` 1.1; CARD-ROOT-001 (may, T, origin PR #2378) card served at the interface URL.
- generation: CORE-GEN-001..003 (must), CORE-GEN-004 (may), STREAM-GEN-001..003 (must), CORE-POLL-001 (must), -002/-003 (should), -004 (must), CORE-MATCH-001..003 (must).
- live-message: STREAM-LIVE-001..007 (must, T), -008 (should, T), -009/-010 (must, T). All ten tentative.
- push-notifications: PUSH-VER-001 (may), -002 (may, T), -003 (may).
- streaming: STREAM-MSGUPD-001 (should), -002 (must: a 1.0 subscriber is never sent `message_update`), STREAM-OPEN-001/-002 (should), -003 (may), STREAM-SUB-004 (must), -005 (must, T), STREAM-RESUB-002 (must, T).
- timeline: CORE-TIME-001..003 (should), -004/-005 (should, T, origin PR #2399).
- transport-bindings: JSONRPC-GEN-001, REST-GEN-001, GRPC-GEN-001 (must).
- user-artifacts: CORE-USERART-001, -002, -004, -005, -006 (must), -003 (should, T), -007 (should).
- version-negotiation: VER-NEG-003, -004, -005 (must).

New `tck-*` SUT behaviour prefixes are needed (e.g. `tck-live-echo`, `live-message.acts.yaml:32`); `docs/acts-specification.md:1616` on that branch: "The following prefixes are used only by tests for A2A 1.1". This repository declares its prefixes in `acts/sut-behaviors.yaml` (VERIFIED, 1.0 set only).

---

## 3. Where each change lands in this repository

### 3.0 Facts about the repo that every item depends on (all VERIFIED)

- Types are generated from the vendored proto: `crates/a2a-protocol-types/codegen.toml:4-12` ("The generator refuses an entry for a type, field or value the proto does not have, and a proto type this file does not mention"). 47 `#[non_exhaustive]` attributes across `src/generated/*.rs`; no `deny_unknown_fields` anywhere in `crates/a2a-protocol-types/src` (grep empty). So adding fields is source-compatible for readers of the Rust API, and unknown JSON fields from a 1.1 peer are already ignored.
- Six byte-identical proto copies pinned by hash: `proto/a2a_v1/a2a.proto`, `crates/a2a-protocol-{types,client,server}/proto/a2a_v1/a2a.proto`, `tck/proto/a2a_v1/a2a.proto`, `docs/implementation/a2a.proto` (`proto/UPSTREAM`, `proto/README.md` rules). Client and server compile the proto with tonic in `crates/a2a-protocol-client/build.rs:43` and `crates/a2a-protocol-server/build.rs:45`.
- `scripts/check_a2a_proto_upstream.sh:19-21` excludes pre-release tags by design, which is why nothing reported the rc.
- One protocol version exists in code: `crates/a2a-protocol-types/src/lib.rs:53` `pub const A2A_VERSION: &str = "1.0";`.
- `Method::ALL` has 11 entries and a test ties it to the proto's `service A2AService` (`crates/a2a-protocol-types/src/method.rs:57, 64-88, 99-111`).
- Ledger: 510 entries (455 TESTED, 32 APPLICATION, 19 NOT_APPLICABLE, 4 PARTIAL), anchored by `spec_line` to `docs/implementation/a2a-v1.0.1-specification.md` SHA-256 `627ccfe6...` (`docs/compliance/spec-ledger.json` keys `spec`, `entries`, `after_release`). The `after_release` block already holds post-1.0.1 requirements including §7.6.4 (C7), TESTED via `crates/a2a-protocol-server/tests/auth_required_state_tests.rs:174`.

### 3.1 Mapping table

| # | Lands in | Existing overlap | Design questions |
|---|---|---|---|
| R1-R8 SendLiveMessage | New `Method` variant: `crates/a2a-protocol-types/src/method.rs:64`. gRPC server trait gains a required bidi method the moment the 1.1 proto is compiled: `crates/a2a-protocol-server/src/dispatch/grpc/native.rs:289` (neighbour `subscribe_to_task`). JSON-RPC: `dispatch/jsonrpc/mod.rs:322, 417`. REST routes: `dispatch/rest/mod.rs:307-330` (colon routes). WebSocket: `dispatch/websocket.rs:1223, 1236`. Client: `crates/a2a-protocol-client/src/methods/tasks.rs:131-194`, `transport/rest/routing.rs:66`, `transport/grpc.rs:925`, `retry.rs:256`, span names `call_span.rs:327`, server `rpc_span/names.rs:131`. SLIMRPC method constants: `bindings/a2a-protocol-slimrpc/src/lib.rs:259`. Codegen: `tools/a2a-codegen` has never emitted a client-streaming RPC (INFERRED from `Method` docs; `tools/a2a-codegen/src` not read in full). | **Steering (ADR 0036)** already delivers a message to a running task through a per-turn inbox (`handler/messaging/steer.rs`, extension `https://a2a-rust.com/extensions/steering/v1`, `docs/adr/0036-steering-a-running-task.md:38-80`). **WebSocket binding (ADR 0055)** is the only native duplex transport here besides gRPC; §3.1.12 names WebSocket as an example of native duplex (spec@rc:459). | (a) Does a live uplink message to a `WORKING` task go to the steering inbox, and does steering stay an opt-in extension once the core protocol allows it (L2)? (b) HTTP uplink-to-downlink correlation is undefined upstream (5.2); do not invent one. (c) The custom WebSocket binding URI ends `/v1` and `lib.rs:76-80` says a breaking change needs `/v2`: is adding a duplex method additive? (d) Per-request `StreamError` vs this SDK's stream-terminating error frames (`transport/rest/error_frame`). |
| G1-G5 generation | Types: `src/generated/task.rs:68` (Task), `events.rs:15, 36`. Assignment point: the event writer that assigns `seq` (`docs/adr/0012-event-log-and-resumption.md:42-44` "assigned once, in `InMemoryQueueWriter::write`"). Stores: `store/sqlite_store/mod.rs:590` (`applied_seq` column already in the upsert), `store/postgres_store/store_impl.rs:615-730`, `store/tenant/store.rs:606-674`. | **ADR 0012 event log `seq`** is the closest thing, and it is **not** a generation: "`seq` is a position, not a counter" (ADR 0012:39); gaps are legal ("A lagged SSE consumer now produces a visible gap", ADR 0012:111-113; a failed append "leaves a gap too"); server-synthesised frames carry no `seq` (ADR 0012:46-49); client messages are not executor events so they have no position. v1.1 requires dense +1, starting at 1 at creation, advancing for client messages too (G1, G3). | Can `generation` be derived from `seq`, or must it be a second counter persisted with the snapshot in the same write? What does a `seq` gap mean for a `generation` that must not gap? Migration number for both SQL stores and the tenant tables. `int64` is a JSON **string** in ProtoJSON (INFERRED from ADR-001; ACTS says "They accept an integer or a string of decimal digits", `docs/acts-specification.md:488` on `conformance-spec-1.1`): reuse `crates/a2a-protocol-types/src/whole_number.rs`? |
| G6 long-poll | `handler/lifecycle/get_task.rs`; REST query parsing `dispatch/rest/query.rs`, `dispatch/rest/params.rs`; params type `src/generated/params.rs:68` (`TaskQueryParams`). | **ADR 0057** wakes a waiting subscriber on a log append (`store/log_wake.rs`, `store/log_notify.rs`); same wake source. | Hold-open consumes a connection and an admission slot: which limit bounds it (`handler/limits`)? Timeout default. Interaction with `connection_limit.rs`. |
| G7-G8 ifGenerationMatch, -32010 | `src/generated/params.rs:12` (`SendMessageConfiguration`); check site `handler/messaging/continuation.rs:173-190` (where a named task is loaded); error enum `crates/a2a-protocol-types/src/error.rs:46-63, 260-268`; gRPC/REST mappings under `dispatch/grpc/details.rs`, `dispatch/rest/error_response.rs`; client error decode. | `error.rs:909` uses `-32010` in a test as an "around A2A range boundaries" **unknown** code: that test's expectation inverts once the code is assigned. Idempotency (`handler/messaging/idempotency.rs`) is a different guard. ADR 0049 standard error reasons already emits `ErrorInfo` reasons. | Compare-and-append must be atomic with the generation bump in every store. Under which `A2A-Version` is `-32010` emitted (C9)? |
| T1-T7 timeline | `src/generated/task.rs:84-90`; history shaping `handler/messaging/respond.rs:37-172`, `handler/helpers.rs:331-361`; list `handler/lifecycle/list_tasks.rs`; codegen `oneof` handling `crates/a2a-protocol-types/src/oneof.rs` (already skips unknown keys, `oneof.rs:10-16`, but **errors when no known member is set**, `oneof.rs:116-124`). | The event log already holds status and artifact events in order; it does not hold client messages as events. `history` holds messages without order metadata. | Is `timeline` materialised in the snapshot or folded from the log on read? ADR 0012:35-37 rejects fold-on-read ("deliberately *not* event sourcing") and lists it as a revisit trigger (ADR 0012:177-179). T7 needs an "unknown entry" representation: skip-on-parse requires the `oneof` macro to yield "none" instead of an error for `TimelineEntry`. |
| T8-T9 TaskMessageUpdateEvent | `src/generated/events.rs:67-87` (`StreamResponse` enum + `oneof_deserialize!`); proto conversion `crates/a2a-protocol-types/src/proto/convert/`; fan-out `handler/subscribers.rs`; push `handler/event_processing/background/push_delivery/`. | None. Steering receipts are metadata on the returned Task, not events. | The subscriber set must remember each subscriber's negotiated version (does not exist: version is validated and dropped, section 4.2). STREAM-MSGUPD-002 is a MUST. |
| A1-A7 user artifacts | `src/generated/agent_card.rs:186-198` (`AgentCapabilities`); `src/generated/params.rs:49-61` (`MessageSendParams.message` is non-optional, line 55); validation `handler/messaging/validation.rs:94-164`; capability gate `handler/capability.rs:13-32`; `src/generated/artifact.rs:12`. | Artifact chunk assembly journal (`store/sqlite_store/journal.rs:98`). | **A3 breaks the public Rust type**: `MessageSendParams.message: Message` -> `Option<Message>`. `#[non_exhaustive]` does not cover a type change. This is the one change that cannot be made additively in the generated types; a 1.1 type surface needs either a breaking release or a parallel type. Executor API: how does an executor receive a client-written artifact? |
| S1-S7 envelope + forward compat | Client JSON stream: `crates/a2a-protocol-client/src/streaming/event_stream.rs:654-667` — a frame that fails to parse as `StreamResponse` returns `ClientError::Serialization`. gRPC: `transport/grpc.rs:1815-1836` test `grpc_stream_reader_task_rejects_empty_payload` asserts an empty `payload` is `InvalidAgentResponse` and **stops the stream**; an unknown `oneof` member decodes to `payload: None` in prost (INFERRED: standard proto3 behaviour). | — | **Present-day gap against S5**: this client fails the stream on an unknown payload kind on both JSON and gRPC, where 1.1 says MUST skip. It is not a 1.0.1 violation (the rule did not exist) and a conformant 1.1 server withholds `messageUpdate` from a 1.0 client (T8), but any later minor that relies on S5 breaks this client. Cheapest forward-compat item in the list; behaviour change for callers who relied on the error. |
| S8-S9 lastSeenGeneration | `src/generated/params.rs:183` (`TaskIdParams`); `handler/lifecycle/subscribe.rs:26-37, 272, 424`; `handler/lifecycle/subscribe/hold.rs:48`; client `methods/tasks.rs:169` (`subscribe_to_task_from`). | **Direct overlap**: `Last-Event-ID` resumption (ADR 0012:51-54), SSE-only by decision (ADR 0012:171-173 "the non-SSE bindings have no way to return a new offset"); the revisit trigger at ADR 0012:180-181 ("A transport other than SSE gains a standard resumption mechanism — then `StreamEvent::seq` is already there to carry") is now met. dev-1.1's "hint" semantics match what this SDK does when the log is absent (`subscribe.rs:424`). | Two cursors (`Last-Event-ID` = seq, `lastSeenGeneration` = generation) on one subscribe path: precedence when both are sent; whether to keep `id:` frames. Replay bound `subscribe_replay_limit` applies to both. |
| L1-L4 stream lifetime | Client: `streaming/event_stream.rs:729-746` `ends_stream` accepts a clean close on interrupted states (lenient, still correct under 1.1). Server: turn model parks a task at interrupted states (`handler/messaging/terminal.rs:424-434`, `handler/event_processing/sync_collector.rs:254, 306, 427`). | ADR 0056 (parked tasks may expire), ADR 0054 (turn hand-off). | INFERRED: this server closes a `SendStreamingMessage` stream when the turn parks; under L3 that becomes a SHOULD NOT. STREAM-OPEN-001/-002 are `should`. Keeping a stream open across turns changes what a "turn" owns. Ledger row spec_line 446 (blocking send waits for interrupted) is unaffected. |
| D1-D5 timestamps | `src/generated/task.rs:97-106` (`TaskStatus.timestamp: Option<String>`), stores already keep `updated_at` and `created_at` columns (`store/sqlite_store/mod.rs:146-147`). | Columns exist; not exposed on the wire type. | `updated_at` in SQLite has millisecond text format (`mod.rs:55`); `created_at` uses `datetime('now')` (second precision, `mod.rs:147`). D4's "MUST equal" between `status.timestamp` and the event `timestamp` needs one clock read per event. |
| D6 ListTasks ordering | `store/sqlite_store/mod.rs:1154`, `store/tenant_sqlite_store.rs:580`, `store/postgres_store/store_impl.rs:529` all `ORDER BY updated_at DESC, id DESC`; index `mod.rs:209`; cursor `store/cursor.rs`, `store/pagination.rs`. Ledger rows spec_line 254 and 262 (both MUST, TESTED). | Current behaviour is exactly the 1.0.1 MUST and is a permitted **MAY** under dev-1.1. | Per-version ordering (1.0 request -> updated_at MUST; 1.1 request -> createdAt SHOULD) or keep one order and record the SHOULD as a deliberate deviation? A `created_at` cursor needs a new index and a new page-token format. This text is 1 day old upstream; do nothing until it is released. |
| C1 push `protocol_version` | `src/generated/push.rs:36-61`; config stores `crates/a2a-protocol-server/src/push/{config_store,sqlite_config_store,postgres_config_store}.rs` and tenant variants; delivery `push/sender.rs`, `push/outbox.rs`. | ADR 0037 push outbox. | Schema migration for six config stores; delivery must filter `messageUpdate` and omit `generation`? PUSH-VER-002 is tentative. |
| C3 §8 renumber | `docs/compliance/spec-ledger.json` every entry with `section` §8.3-§8.6; `SPEC_COMPLIANCE.md`; code comments citing §8.x. | — | A 1.1 ledger is a re-anchor of all 510 `spec_line` values plus section renames, not an append. |
| C4-C5 media type, AI Catalog | `crates/a2a-protocol-types/src/lib.rs:60` (media-type constants); `crates/a2a-protocol-server/src/dispatch/well_known.rs`; `crates/a2a-protocol-client/src/discovery.rs`; crate `a2a-protocol-discovery`. | ADR 0031 (discovery beyond one link), ADR 0051 (card discovery at fleet scale). | Whether the card endpoint's `Content-Type` changes (no normative text requires it). AI Catalog is a topic-page recommendation, not a spec MUST. |
| C7 §7.6.4 | Already TESTED in `after_release` (`spec-ledger.json`, test `auth_required_state_tests.rs:174`). | Done. | Move from `after_release` into `entries` when re-anchoring. |
| C2 gRPC url form | `crates/a2a-protocol-types/src/agent_card/builders.rs:42-63`; client `builder/selection.rs`. | — | Does the client accept a bare `host:port` gRPC interface URL today? Not checked. |
| T11 ADR-002 extras | none | — | Do not build `elicitations`; it is not in the proto. |

### 3.2 Upstream drafts in `docs/upstream/` versus v1.1 (VERIFIED by reading each draft's header and grepping `spec@dev` / `proto@dev`)

| Draft | v1.1 status |
|---|---|
| `a2a-2200-patch-versioning-report.md` (filed as A2A#2200) | **Not addressed.** §3.6 has no hunk in `v1.0.1..origin/dev-1.1`. 1.1 leans harder on `Major.Minor` gating (T8, C1), which makes the report's question more relevant, not less. |
| `a2a-required-empty-array-report.md` (held) | **Not addressed, still valid.** "Arrays marked as required **MUST** contain at least one element" is unchanged (spec@dev:1473) and `repeated Task tasks = 1 [(google.api.field_behavior) = REQUIRED]` is unchanged (proto@dev:913). #2319 (C11) again applied §5.7 literally to an Agent Card example. |
| `a2a-standard-error-reasons-draft.md` | **Not adopted; one data point added.** 1.1 defines exactly one new reason, `TASK_GENERATION_MISMATCH`, by the existing rule (spec@dev:3206). No reason for invalid params etc. Separately, `StreamError.code` is a free string of gRPC-style names (`INVALID_ARGUMENT`, `UNAVAILABLE`, proto@rc:980-983), a third error vocabulary the draft does not cover. |
| `a2a-deadline-budget-propagation-draft.md` | **Untouched.** `grep -ic 'deadline\|budget'` on `spec@dev`: 0. |
| `a2a-task-lineage-halt-draft.md` | **Untouched.** No parent/root/lineage/halt in `spec@dev` or `proto@dev`. Its third topic (integers in `Struct` metadata) gains weight: 1.1 introduces `int64` protocol fields. The unmerged branch `feat/message-sender-attribution` (5.3) adds identity fields, not lineage. |
| `a2a-oversight-provenance-extensions-draft.md` | **Untouched.** §7.6.4 (C7) is adjacent (what `AUTH_REQUIRED` does not authorise) and compatible with the draft. `Artifact.role` (A6) records user-vs-agent origin, not which agent; no overlap with signed provenance. |
| Steering extension (ADR 0036, not an upstream draft) | **Partly obsoleted in intent, not contradicted.** L2 lets a server "continue to accept messages directed to the task" in any non-terminal state and §3.1.12 says an agent "that accepts input while `TASK_STATE_WORKING` ... MAY decline individual ones". The core protocol now permits what the extension gates; the extension's receipt (`Task.metadata["a2a-rust.com/steering"]`) has no 1.1 equivalent. |

---

## 4. Version negotiation

### 4.1 What the specification says (unchanged text, VERIFIED at spec@dev:886-918)

- Version identity is `Major.Minor`; "Patch version numbers ... MUST not be considered when clients and servers negotiate protocol versions."
- "Clients MUST send the `A2A-Version` header with each request"; MAY send it as a request parameter instead.
- "Agents MUST process requests using the semantics of the requested `A2A-Version` (matching `Major.Minor`). If the version is not supported by the interface, agents MUST return a `VersionNotSupportedError`." "Agents MUST interpret empty value as 0.3 version." "Agents CAN expose multiple interfaces for the same transport with different versions under the same or different URLs."
- Each `AgentInterface` declares one `protocolVersion`; §8.2.2 has the client pick an interface.

What 1.1 adds on top (VERIFIED):

- Server-side gating by requested version: `TaskMessageUpdateEvent` "emitted **only** to clients that request `A2A-Version: 1.1` or later" (spec@dev:649); "Servers **MAY** still withhold a new payload kind from clients that negotiated an older protocol version" (spec@rc:542).
- Push: per-config `protocol_version`, default "the version of the request that created the configuration, or "1.0"" (C1).
- Dual service is expected: "servers that also support 1.0 clients continue to populate `history`" (spec@rc:628); deprecated fields are served "throughout the 1.x line".
- Conformance reading (`conformance-spec-1.1`, `version-negotiation.acts.yaml`): VER-NEG-004 (must) a request as `1.99` is refused with `VersionNotSupportedError` ("Versions are matched on Major.Minor, so sharing the major version ... is not enough"); VER-NEG-005 (must) "An agent that lists a 1.0 interface serves a request sent as 1.0 ... not refuse it because it also serves a newer version"; CARD-DISC-008 (should) the card lists an interface with `protocolVersion` 1.1.

INFERRED interoperation model: a 1.1 server lists two interfaces per binding (1.0 and 1.1, same or different URL), dispatches each request by its `A2A-Version`, and a 1.0 request gets 1.0 behaviour (no `messageUpdate`; `history` populated). Whether additive 1.1 **fields** (`generation`, `timeline`) may appear in responses to a 1.0 request is not stated; the spec's own long-poll example uses a 1.1 field under `A2A-Version: 1.0` (C9), and G10 says 1.0 clients "continue to interoperate correctly" with servers that send `generation`, which implies they may.

### 4.2 What this repository does today (VERIFIED)

Server:

- `crates/a2a-protocol-server/src/dispatch/version.rs:105-129` `validate_version_header`: absent/empty -> `VersionNotSupported` when `require` (text "A2A version '0.3' is not supported by this server; expected '1.0'"), otherwise only a value for which `is_served_version` holds passes.
- `version.rs:136-149` `is_served_version`: parses `Major.Minor` and compares to `A2A_VERSION` — **exactly one served version, a constant**. Test at `version.rs:199` lists `"1.1"` among refused values. `SPEC_COMPLIANCE.md:75` states the same ("anything but `Major.Minor` 1.0, `1.1` included, since 2026-10-12").
- Call sites: `dispatch/rest/mod.rs:223-224`, `dispatch/jsonrpc/mod.rs:173-176`, `dispatch/websocket.rs:1007-1008`, `dispatch/axum_adapter/validate.rs:63-64`, gRPC `dispatch/grpc/helpers.rs:42` (via `validate_version_metadata`, `version.rs:60-71`), SLIMRPC re-exports the key (`bindings/a2a-protocol-slimrpc/src/binding.rs:68`).
- The validator returns `Result<(), A2aError>`: **the requested version is checked and discarded**. Nothing downstream (handler, subscribers, push) knows which version a request asked for.
- Card builder stamps every interface with the constant: `crates/a2a-protocol-types/src/agent_card/builders.rs:63`; SLIMRPC `binding.rs:209, 434, 463`.

Client:

- `crates/a2a-protocol-client/src/builder/mod.rs:64, 70`: `SUPPORTED_PROTOCOL_MAJOR = 1`, `SUPPORTED_PROTOCOL_MINOR = 0`.
- `builder/selection.rs:75-78` `is_compatible`: an interface is a candidate only if its `protocolVersion` is `1.0` (any patch). Module doc `selection.rs:16-20`: "an interface for `1.1` is not a candidate: until 2026-11-13 any `1.x` was". A card listing only 1.1 interfaces is an error (`selection.rs:133-160`, message built at `builder/mod.rs:542`).
- Every transport sends the constant: `transport/jsonrpc.rs:213-214`, `transport/rest/request.rs:170-171`, `transport/grpc.rs:565, 1399`, `transport/websocket.rs:638-640`, `discovery.rs:874-878`, `version_query.rs:64-68`, SLIMRPC `client/dispatch.rs:51-54`.

Net effect against a 1.1 peer (INFERRED from the above): this client talks to a dual-version 1.1 server through its 1.0 interface and is refused by a 1.1-only server at build time; a 1.1 client that sends `A2A-Version: 1.1` to this server gets `VersionNotSupportedError` (-32009), and one that reads this server's card finds only 1.0 interfaces. Both are the specified behaviour for a 1.0 implementation.

### 4.3 What dual-version (1.0 + 1.1) support requires

1. A set of served versions instead of one constant (`A2A_VERSION` stays as "preferred"/"latest"); `is_served_version` checks membership.
2. `validate_version_header` returns the negotiated `(major, minor)`; it is carried in the call context (`build_call_context`, used at `handler/lifecycle/subscribe.rs:667`) to the handler, the subscriber registry (`handler/subscribers.rs`) and push config creation.
3. Per-version response shaping at one choke point: withhold `StreamResponse::MessageUpdate` and `StreamError` from 1.0 streams (MUST per STREAM-MSGUPD-002); decide and document whether 1.1 fields are stripped for 1.0 requests (unspecified upstream, C9).
4. Agent card: two `AgentInterface` entries per binding, with an ordering rule (first entry is "preferred", §8.2.1).
5. Client: `is_compatible` accepts a set; prefer the newest version both sides speak (a2a-go sorts "newer versions first", noted at `selection.rs:32-35`); the transport sends the **selected interface's** version, not a constant — seven send sites listed in 4.2.
6. Push: store `protocol_version` per config; deliver in that version.
7. Types: one Rust type surface that is a superset (1.1 proto is a wire superset of 1.0.1 except A3's optionality), with 1.0 behaviour selected at runtime; or feature-gated 1.1 fields. A3 forces a decision (section 3.1).
8. gRPC: the service is `lf.a2a.v1.A2AService` in both versions; one tonic service must implement `SendLiveMessage` and refuse it (`UNIMPLEMENTED` or `VersionNotSupported`) for 1.0 requests.
9. Tests: `crates/a2a-protocol-sdk/tests/binding_equivalence/version.rs` and `crates/a2a-protocol-client/src/builder/selection_tests.rs` assert single-version behaviour today and must be parameterised.

---

## 5. Open upstream uncertainty

### 5.1 Stated release timeline

**None found.** Searched `origin/dev-1.1` and `origin/main` (`git grep -n -i 'v1\.1\|1\.1\.0\|release candidate'` over `*.md`, `*.json`, `*.yml`): the only non-spec hit is `docs/roadmap.md:7` (dated "Last updated: Sep 15, 2026"): "**v1.1 Protocol Enhancements & Core Robustness (#1991):** Advance protocol version 1.1 features by refining task timeline specifications, standardizing event filtering mechanisms for backward compatibility, and establishing clear guidelines for handling agent messages received during working states." No date. ADR-002 lists "**Ship v1.1.** Avoid scope creep that slips the release" as a decision driver and its status is "Proposed". GitHub issues, PR threads and release pages were not read (git clone only).

### 5.2 Visibly unfinished in dev-1.1 (VERIFIED)

1. **SendLiveMessage has no binding sections** (R8). Upstream's own conformance spec says so: `docs/acts-specification.md:1359` on `conformance-spec-1.1`: "**Tentative.** A2A 1.1 does not yet say how a server tells an HTTP uplink call from a call that opens a new exchange, and its binding sections do not list `SendLiveMessage`." All ten STREAM-LIVE tests are tagged `tentative`. JSON-RPC framing of a duplex method is undefined.
2. **Where a `StreamError` for a rejected HTTP uplink call is delivered** — "How it reaches that caller is defined by the protocol binding" (spec@dev:486), and no binding defines it.
3. **rc -> dev-1.1 already broke the rc's JSON** (S2 rename) and reversed a MUST NOT (S8). The rc is not a stable target.
4. **Proto vs prose MUST/SHOULD mismatch on `updated_at`** (D7).
5. **Version gating of additive fields** (C9) and no example anywhere of `A2A-Version: 1.1` or `protocolVersion: "1.1"`.
6. **ADR-002 promises `elicitations` and `TaskStatus.message` deprecation** that the proto does not contain (T11); roadmap lists "Elicitation & Multi-Turn Workflows (#2149, #2143)" as "Explore".
7. `int64` JSON encoding is not discussed in the spec text.
8. Stale cross-reference §14.1.2 -> "Section 8.4" (C15).

### 5.3 Contested: unmerged branches stacked on the rc (VERIFIED: each `contains-rc` by `git merge-base --is-ancestor v1.1.0-rc`)

| Branch (tip, date) | What it would change | Conflict |
|---|---|---|
| `origin/feat/get-task-include-artifacts` (`68ff604d`, 2026-11-03) | Adds `enum TaskView { UNSPECIFIED, FULL, METADATA }`, `GetTaskRequest.view = 5`, `ListTasksRequest.view = 9`, and **deletes all three `timeline_length` fields**, reusing field numbers 5 and 9 | Directly contradicts T5. CORE-TIME-004/-005 are tagged tentative with `origin: .../pull/2399`. `timeline_length` must be treated as unstable. |
| `origin/feat/message-sender-attribution` (`3ade6df2`, 2026-11-03) | `Message.sender = 9`, `Artifact.creator = 10`, `Artifact.contributors = 11` | `Artifact` field 10 is now `updated_at` on dev-1.1 (D1): the branch cannot merge as written. |
| `origin/get_card` (`02ce801f`, 2026-11-20) | Agent card served at the interface/root URL (discovery topic + 4 spec lines) | CARD-ROOT-001 (may, tentative, PR #2378): "the recommendation is not in the 1.1.0 release candidate". |
| `origin/docs/relax-list-tasks-ordering` (`022e0791`, 2026-11-03, #1263) | Relaxes ListTasks ordering | Overlaps D6, which landed by another route on 2026-11-24. |

### 5.4 Likely to change before final (INFERRED from 5.2-5.3)

`StreamRequest`/`StreamError` shape and HTTP live semantics; `timeline_length` vs `TaskView`; ListTasks ordering text; timestamp MUST/SHOULD levels; whether new fields get identity/attribution siblings. Least likely to change (merged before the rc, exercised by non-tentative `must` tests): `generation` on Task and events, `currentGeneration`, `ifGenerationMatch` + `-32010`/`ABORTED`/`409`, `TimelineEntry`, `messageUpdate` gating, unknown-payload skip rule, `userArtifacts` capability and `artifactUpdates`.

---

## 6. Recommended work breakdown: v1.1 readiness without committing to an unreleased spec

Ordered. Items 1-5 change no wire behaviour for 1.0 peers and are safe before any upstream release. Items 6+ are gated on a maintainer decision (ROADMAP.md:54-59 item 8) and, for the marked ones, on an upstream release.

1. **Make the pre-release visible to tooling.** Extend `scripts/check_a2a_proto_upstream.sh` to report (not fail on) pre-release tags and the `dev-1.1` tip hash.
   - Accept: with network, the script prints the rc tag, its commit `78e1d6a1` and the proto SHA-256 `8d9c363f...`; exit code stays 0 while `proto/UPSTREAM` says `v1.0.1`; exit code 3 without network is unchanged.

2. **Vendor the rc and the dev-1.1 tip as reference, outside every build path.** `docs/implementation/a2a-v1.1.0-rc-specification.md`, `...-a2a.proto`, with a provenance file (ref, commit, SHA-256, date) and the dev-1.1 tip beside it.
   - Accept: `scripts/check_proto_copies.sh` still passes (the six pinned copies are untouched); a new check fails if a vendored reference file's hash differs from its provenance entry; `grep -r 'v1.1.0-rc' crates/*/build.rs` is empty.

3. **A delta ledger, not a second 510-row ledger.** `docs/compliance/spec-ledger-1.1-delta.json`: one entry per keyword-bearing line added, changed or removed between the vendored 1.0.1 text and the vendored 1.1 text, each with `status` from the existing vocabulary plus `upstream_stability` (`released` / `rc` / `post-rc` / `tentative`) and the ACTS test ID where one exists. Section 2 of this report is the seed (R, G, T, A, S, L, D, C rows).
   - Accept: a checker mode fails if any line that carries a keyword in the 1.1 text and is absent from the 1.0.1 text has no delta entry (expected count from `diff` of the two keyword-line sets; the totals are 296 vs 342 at rc, 347 at dev-1.1); fails if a delta entry cites a line without a keyword; the existing `python3 scripts/check_spec_ledger.py` result for v1.0.1 is unchanged (510 entries).

4. **Client forward-compatibility: skip unknown stream payloads and unknown timeline entries (S5, T7).** Behaviour-only; needs no 1.1 type.
   - Accept: a test feeds `{"messageUpdate":{...}}` then a terminal `statusUpdate` over SSE (REST and JSON-RPC framing) and over gRPC (a `StreamResponse` with an unknown field number) and the stream yields the status update and ends cleanly; `grpc_stream_reader_task_rejects_empty_payload` is replaced by a test asserting skip; a frame that sets two known members still errors; CHANGELOG records the behaviour change.

5. **Carry the negotiated version through the request path, with one served version.** Refactor `validate_version_header` to return the parsed `Major.Minor` and put it in the call context; replace the single constant comparison with membership in a served set that contains only `1.0`.
   - Accept: `validate_version_metadata`'s public signature is either unchanged or changed with a deprecation shim (public API diff tool reports no unannounced break); the "1.1 is refused" assertions in `dispatch/version.rs` tests and `binding_equivalence/version.rs` still pass; a new test reads the negotiated version from the context on all bindings (JSON-RPC, REST, gRPC, WebSocket, axum adapter, SLIMRPC).

6. **Client selection by version set.** `is_compatible` takes the set; the transport sends the selected interface's version.
   - Accept: with the set `{1.0}`, `selection_tests.rs` passes unmodified; a test with a set `{1.0, 1.1}` (test-only constructor) picks the 1.1 interface from a card listing both and sends `A2A-Version: 1.1` at each of the seven send sites in section 4.2; a card listing only `1.1` still errors with the default set.

7. **Decision record (ADR) before any type work**, answering: superset types vs feature gate vs parallel module; the `MessageSendParams.message` optionality break (A3); `generation` vs `seq` (ADR 0012); steering extension vs core live messaging (ADR 0036); `Last-Event-ID` vs `lastSeenGeneration`; whether 1.1 fields are sent on 1.0 responses; which ROADMAP "minor or patch" rule applies (ROADMAP.md:49-51 item 6, `STABILITY.md:130`).
   - Accept: the ADR exists, each question has one chosen answer and one rejected alternative, and ROADMAP item 8 links it.

8. **Generated 1.1 types behind an off-by-default, explicitly unstable cargo feature** (e.g. `unstable-a2a-1-1`), compiled from the vendored rc/dev proto in a separate module path; no default build compiles them.
   - Accept: `cargo public-api` (or the repo's API-diff check) on default features shows no change; the codegen config for 1.1 has an entry for each of the 4 new messages and every new field (the generator already refuses unmapped proto items, `codegen.toml:10-12`); golden JSON round-trip fixtures exist for `Task` with `generation`/`timeline`, `messageUpdate`, `StreamError`, `StreamRequest` (both `sendMessage` and `request` spellings recorded, since rc and dev-1.1 differ); CLIENT-PARSE-007/-009/-010 payloads from `conformance-spec-1.1` parse.

9. **Server-side generation as the first 1.1 behaviour, behind the same feature** (the most stable part upstream: G1-G8).
   - Accept: STREAM-GEN-001..003, CORE-GEN-001..003, CORE-MATCH-001..003, JSONRPC/REST/GRPC-GEN-001 expectations reproduced as in-repo tests on in-memory, SQLite and PostgreSQL stores; a restart test shows generation continues, not restarts; with the feature off, serialized output is byte-identical to 0.16.0 fixtures.

10. **Hold until upstream publishes a non-rc 1.1 tag** (`scripts/check_a2a_proto_upstream.sh` item 3 goes red on its own when it does): SendLiveMessage on any binding (R8, 5.2.1-2), `timeline_length` (5.3), ListTasks re-ordering (D6), timestamps (D1-D7), `StreamError` delivery, the WebSocket/SLIMRPC duplex mapping, declaring a `protocolVersion: "1.1"` interface, re-pinning `proto/UPSTREAM`, re-anchoring the main ledger, declaring 1.1 `tck-*` prefixes in `acts/sut-behaviors.yaml`.
    - Accept (for "held"): `proto/UPSTREAM` tag is `v1.0.1`; no default-feature card built by this SDK contains `"protocolVersion":"1.1"` (a test asserts it); the delta ledger marks each of these `post-rc` or `tentative`.

11. **Re-diff on every upstream movement.** A scheduled job re-runs the keyword-line and proto diff between the vendored reference and `dev-1.1`.
    - Accept: the job fails when the `dev-1.1` proto hash differs from the recorded `626a6776...` or when the keyword-line set changes, and its output lists the changed lines; the delta ledger's recorded tip commit equals the one the job checked.

---

## 7. Disagreements between documents and artifacts found

| Document says | Artifact says | Winner |
|---|---|---|
| Spec 1.0.1 prose: `POST /tasks/{id}:subscribe` | proto@1.0.1:78 `get:` | proto; fixed in 1.1 (R9). This repo accepts both (`dispatch/rest/mod.rs:360`). |
| ADR-002: v1.1 adds `elicitations`, soft-deprecates `TaskStatus.message` | Neither in proto@rc or proto@dev | proto (T11) |
| spec@rc §3.1.6: Task MUST be the first event | proto@rc:913: no snapshot when `last_seen_generation` set | fixed in dev-1.1 (S9) |
| spec@dev:641: SHOULD set `updatedAt` | proto@dev:223-229, 368-374: MUST | open (D7) |
| spec@dev:608-610 example: `currentGeneration` under `A2A-Version: 1.0` | spec@dev:621: field "introduced in version **1.1**" | open (C9) |
| `docs/handoff.md:41-44` summary of the rc proto | Matches the diff; it omits user artifacts (`artifact_updates`, `user_artifacts`, `Artifact.role`), `if_generation_match`, `current_generation`, `last_seen_generation`, `TaskPushNotificationConfig.protocol_version`, the `message` REQUIRED removal, and everything after the rc | handoff is correct but incomplete |
| `crates/a2a-protocol-types/src/error.rs:909` treats `-32010` as outside the assigned A2A codes | 1.1 assigns `-32010` (G8) | correct for 1.0.1, stale for 1.1 |

## 8. Not checked

- GitHub issues, pull-request discussion and release pages (only git objects were read), so "no stated timeline" means "none in the repository".
- `tools/a2a-codegen/src` beyond a grep; `itk/` and `tck/` Rust sources beyond directory listings; SLIMRPC server method table beyond `lib.rs:259`.
- `docs/topics/life-of-a-task.md`, `key-concepts.md`, `what-is-a2a.md`, `a2a-and-mcp.md` diffs (commit subjects say readability rewording; not read line by line).
- Whether this client accepts a bare `host:port` gRPC interface URL (C2).
- No cargo build or test was run; every statement about runtime behaviour of this repository is from reading source.
