<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Audit register

The spec audit of `claude/tender-cray-bsfw83` kept a register, R1 to R67, of
findings against the A2A specification, the proto and the RFCs they cite. It
lived in that session's scratchpad and was never committed. This file holds
the items that were open when it was committed (2026-10-14), R12 to R67, and
those found since, R68 and R69, with what became of each. The handoff, the
CHANGELOG and the spec ledger refer to 54 of the R1-R67 ids; an entry closed
before this file existed is recorded in the handoff item that fixed it, and
those closed since also name their id in the CHANGELOG.

How to read an item: **Was** is what the code did when the item was found,
read from source at the commit named (`9b08a860` for R12 to R67; each file
and line was opened, not recalled). **Text** is what it is measured against,
then the options and the recommendation as they were put to the maintainer,
the decision, and, once implemented, **Done**: what the code does now. Under
the maintainer's directive to be completely v1.0.1-conformant, the
recommendation is the conformant option unless that would break something
worse, in which case the item says what. Spec line numbers are those of the
upstream v1.0.1 text, the ledger's own numbering; in the vendored copy,
`docs/implementation/a2a-v1.0.1-specification.md`, add 31 for its header.
Proto line numbers are in `proto/a2a_v1/a2a.proto`.

## Decisions

- **2026-10-14.** The recommendations for R12 to R67 and the ledger rows put
  to the maintainer (the last section) are approved as written, with two
  specifics: R49 / §1.4 takes option (b), the types generated from the proto
  now, not the check first; R12 migrates the stored partition keys once per
  store (decided the same day, replacing an earlier "read the old keys for
  one release"). R67 takes option (b), and the next release is 0.15.0. The
  dispositions recorded for §3.1.6, §3.5.2, §8.6 / §8.6.2, §11.1 (server),
  §12.4 and §12.6 are approved as recorded.
- **2026-10-14.** R68: follow the text.
- **2026-10-15.** R49's form: a generator in this repository writes the types
  out of the proto, checked in (ADR 0016).
- **2026-10-15.** R69: option (a).

## Open

| Item | What remains | Waiting on |
|---|---|---|
| R13 | upgrade the six SLIM crates to the line agntcy-slim-rpc 3.x requires, record the verified peer, ship an interceptor that authenticates from it | work (approved) |
| R41, second half | key an unidentified SLIMRPC caller by its verified peer | R13 |
| §3.5.2 catch-up paging | a lagged stream's catch-up stops after one `subscribe_replay_limit` read (1,000 events) and ends with `streamLagged`, where it could page through the log; a consumer slower than its producer would then read the store indefinitely | the maintainer's call (a change to an approved disposition) |
| §12.4 (line 3016), SLIMRPC error details | `agntcy-slim-rpc` sends an error's code and message only, in 2.3.0 (pinned) and 3.4.1 (latest) | upstream |
| §5.4 SHOULD (line 1194) | the SLIMRPC specification's mapping table has no row for -32007, -32008 or -32009 | upstream: `docs/upstream/slimrpc-a2a-error-mapping-report.md` |
| Reports to file | §12 / §5.4: `docs/upstream/slimrpc-a2a-error-mapping-report.md`; §5.7: `docs/upstream/a2a-required-empty-array-report.md`; R19 to a2a-go: not in the repository, because it cites a2a-go's code as an earlier session described it and was not checked against a2a-go's source | the maintainer, to check and send |

## Done

| Id | Area | What changed | Commit |
|---|---|---|---|
| R12 | JWT caller identity | a caller is `bearer:<label>`, `api-key:<label>` or `jwt:<issuer>:<sub>`; a JWT with no `sub` is refused under caller isolation; `migrate_caller_keys` per store | `67c30964` |
| R19 | `Struct` error details | written as ProtoJSON's `Any` (`{"@type", "value"}`), both shapes read | `5cd159c3` |
| R21 | JSON-RPC notifications | executed, never answered | `d57cec90` |
| R33 | first frame of a send stream | kept, with the reasoning recorded in the ledger's §3.1.2 entry | this file and the ledger |
| R36(b) | task snapshot moving back to `Submitted` | held to the state-transition rules at the queue | `14bd2d5f` |
| R41, first half | `x-forwarded-for` over SLIMRPC | forwarding headers dropped from caller metadata | `8e5cf1a7` |
| R49 | types not generated from the proto | generated from `a2a.proto` by `tools/a2a-codegen` (ADR 0016); the ledger's §1.4 entry is TESTED | `21cb5989` to `f0a2b40c` |
| R50 | REST client `Content-Type` | `application/a2a+json` with a body, none without | `8e5cf1a7` |
| R66 | multicast member lag | the typed `streamLagged`, still terminal | `8e5cf1a7` |
| R67 | API key in the query string | `ClientRequest::query_params` on the HTTP transports; the move to 0.15.0 | `edc8bbc0` |
| R68 | extended card declared, none configured | `ExtendedAgentCardNotConfiguredError` | `cd7c6d11` |
| R69 | a subscription across replicas loses an event | the store reports the log position a saved task reflects | `29c8966a` |

## R12 — a JWT caller is its `sub` alone

**Was (at `9b08a860`).** `crates/a2a-protocol-server/src/auth/jwt.rs:763-765` records `principal.subject` as the caller;
`principal.issuer` is never used for identity. `crates/a2a-protocol-server/src/store/tenant/context.rs:70`
keys the partition `{tenant}\u{1f}{caller}`. So one `sub` from two accepted
issuers shares a partition, and so does an API-key or bearer label equal to a
JWT `sub` (the partition is not split by scheme). A validated JWT with no
`sub` records no caller and, under the default `IdentifiedCallers`
isolation, is served from the tenant's shared partition
(`crates/a2a-protocol-server/src/handler/mod.rs:370-382`). No test covers two issuers.

**Text.** §13.1 (line 3075): "Implementations **MUST** ensure appropriate
scope limitation based on the authenticated caller's authorization
boundaries." RFC 7519 §4.1.2: the subject is "locally unique in the context of
the issuer or be globally unique".

**Options.** (a) Keep. (b) Key the caller as scheme + issuer + subject
(`jwt:<iss>:<sub>`, `api-key:<label>`, `bearer:<label>`), and refuse a JWT with
no `sub` when caller isolation is on. (c) (b) without the refusal.

**Recommendation: (b).** `sub` is only unique within its issuer, and a
sub-less token sharing the tenant partition is a cross-caller read under the
default isolation. It re-keys every stored partition: existing tasks become
invisible to their owners unless a migration maps the old keys, so it ships
with a documented migration (or a read-through of the old key for one
release).

The design pass (2026-10-14) found the partition read in 38 places across
six stores (tasks and push configs, each in memory, SQLite and PostgreSQL),
and a read-through having to settle what a write, a list and a page token do
for a caller with rows under both keys; it went back to the maintainer,
because the qualified key also changes what an approver allowlist,
`requested_by` and the audit actor compare against.

**Decided (2026-10-14): a one-shot migration per store** that rewrites the
old partition keys to the scheme+issuer+subject form; the stores read only
the new key. This replaces the earlier "read old keys one release".

**Decided (2026-10-14), the design.** The qualified identity is used
everywhere the identity is: partitions, the rate limiter, the audit actor,
`requested_by`, the four-eyes check and the approver allowlist. The
migration takes a map from old caller name to new identity, given by the
deployer, because the old key records neither scheme nor issuer.

**Done (2026-10-14).**
- `auth::caller_id` builds `bearer:<label>`, `api-key:<label>` and
  `jwt:<issuer>:<sub>`. The issuer is percent-encoded (`%`, `:`) so the
  form is injective: issuer `a:b` with subject `c` and issuer `a` with
  subject `b:c` would otherwise both be `jwt:a:b:c`.
- The three bundled interceptors record it.
- `enter_caller_partition` refuses a JWT with no `sub` with
  `PermissionDenied` unless callers share tasks.
- `migrate_caller_keys` on the four persistent tenant-aware stores, with
  `store::caller_keys` (the helpers pass over names already in the new
  form, so a second run moves nothing). A task store copies each moved
  task, moves its logged events and idempotency keys, then deletes the
  original, because the event log's foreign key has no `ON UPDATE`; a
  push-config store opens each config under the old partition and seals it
  under the new one. One transaction per run.
- Narrowed from the option as put to the maintainer: `requested_by` in
  stored pending approvals is not rewritten. The four-eyes check instead
  also treats a bare `requested_by` equal to the approver's unqualified
  name as the same caller. That covers the plain (non-tenant) stores a
  deployment whose callers share tasks keeps approvals in, which no
  partition migration reaches; it errs toward refusing (an `alice` from
  another issuer is refused too).
- Also found: the ITK's ACTS guard compared the identity with the bare
  label and would have stopped refusing its limited caller; it compares
  with `caller_id::bearer` now, under a new unit test that fails with the
  old comparison.
- Tests: `tests/jwt_caller_identity.rs` (two issuers, a sub-less token
  under isolation and when sharing; two of three fail on the old source,
  the third is the sharing baseline), `tests/caller_key_migration.rs`
  (SQLite: tasks, events, keys, a collision, a sealed config), two Postgres
  tests in `postgres_store_tests.rs` (run against a local PostgreSQL 16),
  and unit tests in `caller_id`, `caller_keys` and `approval::tests`.
  Hand mutants: 29, all killed (11 for the identities, 18 for the gate and
  the migrations).

## R13 — SLIMRPC records no caller identity

**Now (read 2026-10-15).** The binding depends on agntcy-slim-rpc 2.3.0 (`bindings/a2a-protocol-slimrpc/Cargo.toml:66`,
`bindings/a2a-protocol-slimrpc/Cargo.lock`). It builds the request's headers from SLIM metadata only
(`bindings/a2a-protocol-slimrpc/src/server/methods/mod.rs:88`), drops transport keys, every `:`-prefixed key and
the forwarding headers (line 125), and never sets a caller identity or `:peer-addr`. 2.3.0's `Context`
exposes `session().source()` (the session's peer name) but no claims; whether
SLIM verified that name is not visible from the crate. A credential sent as
metadata (`authorization`) still reaches the handler's interceptors, which
can authenticate it, so per-caller scoping is available that way; what is
missing is the transport-verified identity.

**Text.** §13.1 (line 3075), as for R12; §12.6 (line 3033), custom bindings
**MUST** "Maintain Security".

**Options.** (a) Keep, document that SLIMRPC callers must present an A2A
credential. (b) Upgrade the six SLIM crates the binding depends on (rpc,
auth, datapath, service, config, session) to the line agntcy-slim-rpc 3.x
requires, record the
MLS-verified peer subject and issuer under `:`-prefixed keys the caller
cannot forge (the binding already drops caller-sent `:` keys), and ship an
interceptor that authenticates from them. The 3.x accessors exist as
described: `Context::peer_subject` and `peer_issuer`, through
`SessionContext::peer_identity`, `None` on a session without MLS
(`src/context.rs` in agntcy-slim-rpc 3.3.1, read 2026-10-15; 3.4.1, the
latest 3.x, has both at lines 78 and 86).

**Recommendation: (b).** It also removes RUSTSEC-2024-0449:
agntcy-slim-config 0.17.3, which agntcy-slim-rpc 3.3.1 requires, replaces
serde_yaml with yaml_serde, and agntcy-slim-auth 0.17.3 takes regorus 0.12,
which drops serde_yaml too (checked against the crates.io index,
2026-10-14). It does not fix RUSTSEC-2026-0285 or the 2026-10-14 rustls
advisories: mls-rs-crypto-awslc 0.25.0 still pins aws-lc-rs =1.16.3.

## R19 — `Struct` error details are written flat

**Was (at `9b08a860`).** `crates/a2a-protocol-server/src/streaming/sse.rs:330-339` copies `A2aError.data`'s fields into the
detail and adds `"@type": "type.googleapis.com/google.protobuf.Struct"`; a
non-object goes under `"value"`. Used by JSON-RPC, HTTP+JSON and their
streams. The doc says it is written "the way a2a-go writes and reads one".

**Text.** §3.3.2 (line 544): each detail "**MUST** include a `@type` key …
(using ProtoJSON `Any` representation)"; §9.5 (line 2437) and §11.6 the same.
In ProtoJSON, an `Any` holding a well-known type with a special JSON mapping
is `{"@type": …, "value": <that type's JSON>}`; the flat form is the encoding
for an ordinary message. The ProtoJSON guide's "Special-cased well-known
types" lists `google.protobuf.Struct` among them
(<https://protobuf.dev/programming-guides/json/>, read 2026-10-14). The flat `Struct` detail is
therefore not a valid ProtoJSON `Any`.

**Options.** (a) Keep. (b) Write `{"@type": "…Struct", "value": {…}}` and
read both. (c) (b), and report the shape to a2a-go.

**Recommendation: (c).** A conformant reader of our errors cannot decode the
flat form; reading both keeps a2a-go's errors working here. An a2a-go client
reading ours would see the fields one level down, under `value`; the
interop suite (`scripts/go_sdk_interop.sh`) says whether anything it checks
depends on them.

**Done (2026-10-14), the (b) half of (c).** `sse::error_details` writes
`{"@type": "…/google.protobuf.Struct", "value": {…}}`; a non-object `data` is
the `Struct` `{"value": …}`, which is also what the gRPC binding sends, so the
two carry the same detail. `A2aError::lag_marker` and the REST client's
`error_frame::struct_detail` read both shapes; a flat `Struct` whose only
field is an object named `value` reads as the `Any` form, an ambiguity the
flat form cannot avoid. `scripts/go_sdk_interop.sh` passes both directions
(137 checks, 0 failures; its Go client does not inspect error details). The
report to a2a-go is not filed and not in the repository: it cites a2a-go's
code as the earlier audit session described it, not as read from a2a-go's
source, so the maintainer should check it before filing.

## R21 — JSON-RPC notifications are answered

**Was (at `9b08a860`).** `crates/a2a-protocol-server/src/dispatch/jsonrpc/mod.rs:303` and `:372` take
`rpc_req.id.to_response_id()` with no check for an absent `id`, so a
notification is executed and answered with `"id": null`; the batch loop
(`:239-265`) and the WebSocket path (`crates/a2a-protocol-server/src/dispatch/websocket.rs:856`) do the same.
`crates/a2a-protocol-server/tests/jsonrpc_edge_tests.rs` `jsonrpc_absent_id_request_answered_with_null_id`
pins it.

**Text.** §9.3 (line 2270): "All JSON-RPC requests **MUST** follow the standard
JSON-RPC 2.0 format". JSON-RPC 2.0 §4.1: "The Server MUST NOT reply to a
Notification, including those that are within a batch request."

**Options.** (a) Keep. (b) Execute, and send no reply: HTTP `204 No Content`
for a single notification, a batch's array without its notifications (and
`204` when every member is one), nothing on WebSocket. (c) Refuse to execute
notifications, with no reply.

**Recommendation: (b).** It is what JSON-RPC 2.0 requires. A notification
`SendMessage` creates a task whose id its sender never learns, which is the
sender's choice.

**Done (2026-10-14), as (b)** in `crates/a2a-protocol-server/src/dispatch/jsonrpc/notification.rs`, the
batch loop and `refusal.rs`, and the WebSocket dispatch. One narrowing: a
streaming method (`SendStreamingMessage`, `SubscribeToTask`) sent as a
notification is refused rather than run, as it already was inside a batch,
since a stream is the only answer it gives. A notification refused on its
headers is refused without an answer too. Tests:
`crates/a2a-protocol-server/tests/jsonrpc_notifications.rs` (HTTP) and `a_notification_is_run_and_sent_nothing`
in `crates/a2a-protocol-server/tests/websocket_binding_spec.rs`; the pin test is gone.

## R33 — a send stream's first frame waits for the executor

**Now.** `crates/a2a-protocol-server/src/handler/messaging/respond.rs:140` holds the task snapshot as
provisional; `crates/a2a-protocol-server/src/streaming/event_queue/in_memory.rs:1100-1102` sends it only
with the executor's first event, with no time bound.

**Text.** §3.1.2 (line 212): "The implementation **MUST** provide immediate
feedback on progress and intermediate results"; (line 208): a message-only
stream "**MUST** contain exactly one `Message` object".

**Recommendation: keep.** The two MUSTs can both hold only if the first frame
waits until the executor shows which kind of stream it is: a `Task` sent
first would break line 208 for an agent that answers with a `Message`. The
wait ends at the executor's first event, which is the earliest anything is
known. Record as a disposition, with the reasoning, in the ledger.

**Decided (2026-10-14): keep. Done (2026-10-15):** the ledger's §3.1.2 entry
for line 212 records the wait and this reasoning.

## R36(b) — a task snapshot may move a task back to `Submitted`

**Was (at `9b08a860`).** `crates/a2a-protocol-server/src/handler/event_processing/background/state_machine.rs:381-384` and
`crates/a2a-protocol-server/src/handler/event_processing/sync_collector.rs:524-526` store an executor's `Task` snapshot without
`can_transition_to`, which forbids re-entering `Submitted`
(`crates/a2a-protocol-types/src/task.rs:318`) and is applied only to status updates. The store refuses only
writes over a terminal state.

**Recommendation: refuse it at the queue write**, as a status update with the
same transition is refused, through the processors' existing mark-failed
path. No test pins the current acceptance.

**Done (2026-10-14).** Both processors now check a snapshot that changes the
task's state against `can_transition_to`: the background processor marks the
task failed through the status branch's path and refuses a move out of a
terminal state itself, and the sync collector answers
`InvalidStateTransition`. A snapshot in the task's current state is stored as
before. Tests: six, in `state_machine.rs` and `sync_collector.rs`; the two
"back to `Submitted`" tests fail on the old source.

## R41 — SLIMRPC believes a caller's `x-forwarded-for`

**Was (at `9b08a860`).** The binding passed caller metadata named `x-forwarded-for` into the
request's headers (`bindings/a2a-protocol-slimrpc/src/server/methods/mod.rs:111` dropped only transport and `:`
keys). With `trusted_proxy_hops > 0`, `crates/a2a-protocol-server/src/rate_limit/identity.rs:41,50` and
`crates/a2a-protocol-server/src/auth/throttle.rs:191-195` take the caller's own value as the client address:
no proxy appends to it on the SLIM path, so the caller picks its rate-limit
and throttle key. SLIMRPC sets no `:peer-addr`, so with hops = 0 the throttle
never throttles and every unidentified caller shares one rate-limit bucket.

**Recommendation: strip `x-forwarded-for`, `x-real-ip` and `forwarded` in the
binding**, and key unidentified SLIMRPC callers by the session's source name
(or the verified peer once R13 lands).

**Half done (2026-10-14).** The binding drops `x-forwarded-for`, `x-real-ip`
and `forwarded` from caller metadata, case-insensitively
(`is_forwarding_key` in `bindings/a2a-protocol-slimrpc/src/server/methods/mod.rs`; test
`a_callers_forwarding_headers_never_reach_the_handler`), so a SLIMRPC caller
no longer picks its rate-limit or throttle key. Keying an unidentified caller
by its peer waits for R13: the 3.x upgrade brings the verified peer, and a
source name read from 2.3.0's session is not known to be verified. Until
then every unidentified SLIMRPC caller shares the anonymous bucket and is
not throttled, as before with `trusted_proxy_hops = 0`.

## R49 / §1.4 — the types are not generated from the proto

**Was (at `9b08a860`).** Only the prost types (feature `proto`) are generated. The public
serde types are hand-written and guarded by golden fixtures, proptests and a
check that the vendored proto copies are identical, not by a check that the
types match the proto.

**Text.** §1.4 (line 107): "SDK language bindings, schemas, and any other
derived forms **MUST** be regenerated from the proto (directly or via code
generation) rather than edited manually."

**Options.** (a) Keep. (b) Generate the serde types from `a2a.proto` at build
time (a ProtoJSON code generator), with the hand-written API as thin
wrappers or replaced outright; the public types change. (c) Keep the
hand-written types, and add a mechanical check that every proto message,
field, JSON name, `REQUIRED` marker and enum value has its Rust counterpart
and nothing else exists.

**Recommendation** (as put to the maintainer; the decision below
superseded the (c)-first order). Only (b) meets the text. (c) is the
strongest check short of it and can land first, so drift fails CI while (b) is planned. This
is the largest change in the register; it needs the maintainer's decision on
the API break before any work.

**Decided (2026-10-15), the form of (b):** a generator in this repository
writes today's types out of the proto, checked in, with CI failing when they
drift (ADR 0016). **Phase 1 done (2026-10-15):** `tools/a2a-codegen`, its
configuration and validation, the CI check, and five generated types
(`TaskState`, `MessageRole`, `AgentInterface`, `AgentProvider`,
`AgentExtension`). The one API change the step makes, `TaskState`'s
discriminants becoming its proto numbers, was found by
`cargo semver-checks --baseline-rev HEAD` and is in the upgrading guide.
**Phase 2 done (2026-10-15):** the agent card family (`AgentCard`,
`AgentCapabilities`, `AgentSkill`, `AgentCardSignature`), with no API change
(`cargo semver-checks --baseline-rev HEAD`: 202 pass). `AgentCard.url`,
which v0.3 cards carry and the proto does not, is the first Rust-only
field, configured as an `extra` entry with its reason.
**Phase 3 done (2026-10-15):** `Task`, `TaskStatus`, `Message`, `Artifact`
and the two update events, with no API change (`cargo semver-checks
--baseline-rev HEAD`: 202 pass). The ID newtypes stay, as configured types
with their reasons.
**Phase 4 done (2026-10-15):** the 14 request, parameter and response
types, with no API change (202 pass). Counts and lengths the proto types
`int32` stay `u32`, and the two push types keep their hand `Debug`, which
redacts their secrets.
**Phase 5, first half (2026-10-15):** the 12 security-scheme and OAuth-flow
messages, with no API change (202 pass). `ApiKeySecurityScheme.location`
stays the `ApiKeyLocation` enum, two deprecated flows keep a required URL
the proto leaves optional, and `StringList` keeps its hand `Deserialize`
(a2a-go's bare array).
**Phase 5 done (2026-10-15):** the five `oneof`s, in the form ADR 0016 now
records, with no API change (202 pass) and the same wire behaviour: 35
inputs across the five, parsed and re-serialized by a build of each tree,
print the same lines except one error message's member order. Every proto
type is generated; phase 6 (the shadowing check, then this entry) remains.
**Phase 6 done, R49 closed (2026-10-15):** `a2a-codegen --check` also
fails when any Rust file outside `src/generated/` and the generator declares
a type under one of the 47 generated names, shown by four injections (a
`struct` in a tracked file, one in an untracked file, a `type` alias, a
`pub(crate) enum`). The ledger's §1.4 MUST moves from PARTIAL to TESTED, and
the configuration's empty `[pending]` table is removed.

## R50 — the REST client sends `application/json`

**Was (at `9b08a860`).** `crates/a2a-protocol-client/src/transport/rest/request.rs:133` sends
`Content-Type: application/json`, and `:112` sends the same `Accept`; GET and
DELETE send the `Content-Type` with no body. No test asserts the request
`Content-Type`.

**Text.** §11.1 (line 2750): "**Content-Type:** application/a2a+json
**SHOULD** be used for requests and responses".

**Recommendation: send `Content-Type: application/a2a+json` with a body and
none without, and `Accept: application/a2a+json, application/json`.** a2a-java
1.4.0.Final accepts it (pin-freshness run 38299384752); 1.3.0.Final and
older refuse anything but `application/json`, and that break goes in the
upgrading guide.

**Done (2026-10-14)**, as recommended, in `build_request`
(`crates/a2a-protocol-client/src/transport/rest/request.rs`): a POST body is labelled
`application/a2a+json`, a GET or DELETE has no `Content-Type`, and a
non-streaming `Accept` is `application/a2a+json, application/json`. Tests:
`build_request_labels_a_body_and_only_a_body` and
`build_request_non_streaming_accepts_a2a_json_first` (which replaces the test
that pinned `application/json`).

## R66 — a multicast member's lag report

**Was (at `9b08a860`).** `bindings/a2a-protocol-slimrpc/src/multicast/fanout.rs:128-129` sends the member an `Err`
that ends its stream (`crates/a2a-protocol-client/src/streaming/event_stream.rs:485-487`). The
error is `ClientError::Transport("N event(s) dropped…")`, not the typed
`streamLagged` error the other bindings send.

**Recommendation: keep it terminal and make it the typed `streamLagged`
error**, so one check (`is_stream_lagged`) recognises a lag on every binding.

**Done (2026-10-14).** `lag_report` returns
`ClientError::Protocol(A2aError::stream_lagged(missed))`. The binding's unit
test `a_lag_report_is_the_typed_stream_lagged_signal` pins `lag_report`, and
`tests/multicast_backpressure.rs`, which matched the message text, now
checks `is_stream_lagged` and the dropped count.

## R67 — an API key in the query string is refused

**Was (at `9b08a860`).** `crates/a2a-protocol-client/src/auth.rs:281-284` refuses `ApiKeyLocation::Query`; the
`Transport` trait carries headers only (`crates/a2a-protocol-client/src/transport/mod.rs:200`, `:212`).

**Text.** a2a.proto `APIKeySecurityScheme.location`: "Valid values are
"query", "header", or "cookie"."

**Options.** (a) Keep refusing. (b) Add query parameters to the request the
transports receive (a breaking change for custom `Transport`
implementations, unless added as a defaulted method). (c) A reserved
`:query:<name>` key in the existing header map, honoured by the HTTP
transports and refused by gRPC.

**Recommendation: (b) with a defaulted method**, so a custom transport keeps
compiling and refuses a query parameter it does not handle, rather than (c),
which overloads the header map.

**Blocked (2026-10-14), a release decision** (superseded by the decision
below). The query parameters have to
reach the transports through `ClientRequest`, whose fields are all public
and which is not `#[non_exhaustive]`: adding one breaks a struct literal,
and `cargo-semver-checks` (ci.yml, `semver`) fails a break the version does
not cover, with the client at 0.14.1 as published. So (b) needs the next
release to be 0.15, which is the maintainer's to decide; (c) needs no bump.

**Decided (2026-10-14): (b), and the next release is 0.15.0.**

**Done (2026-10-14), as (b) with a defaulted method.** `ClientRequest` has a
public `query_params`, which `AuthInterceptor::api_key` fills for
`ApiKeyLocation::Query` (replacing an earlier entry of the same name, so a
retried `before` does not send the key twice). `client.rs` hands them to
the new `Transport::send_request_with_query` and
`send_streaming_request_with_query`; the JSON-RPC and HTTP+JSON transports
append them to the URL through `transport::with_query`, percent-encoded,
after any query the REST route builds, and `RetryTransport` passes them to
every attempt. The trait's default refuses a non-empty query with
`InvalidAuthScheme`, which is what gRPC, WebSocket, the SLIMRPC binding and
any custom transport that does not override it answer. The crates are at
0.15.0 (the binding at 0.7.0), so `cargo-semver-checks` accepts the new
field as a minor-version break. Until 0.15.0 is tagged and becomes the
baseline, it also accepts any other accidental break, as it did before
0.14.0 was tagged. Tests:
`crates/a2a-protocol-client/tests/api_key_query.rs` (both transports, a
route with its own query, a retry, a stream with and without the retry
wrapper, a transport that refuses), `auth::tests` for the placement, and
`debug_redaction.rs` for the names-only `Debug`. All five integration tests
fail on the old source, at the refusal. Hand mutants: 19, one equivalent
(an early return for an empty query, since removed); the other 18 are
killed.

## R68 — a declared extended card with none configured served the public card

Found 2026-10-14 while implementing the §13.3 row below, and decided the
same day: follow the text.

**Was.** `handler/lifecycle/extended_card.rs` answered a card that declares
`capabilities.extendedAgentCard`, on a handler with no
`with_extended_agent_card`, by serving the public card. `-32007` was returned
only when the handler had no card at all, and the ledger marked §13.3 line
3165 TESTED on that path alone.

**Text.** §3.1.11 (line 419): `ExtendedAgentCardNotConfiguredError`: "The
agent declares support but does not have an extended agent card configured."
§13.3 (line 3165): "When support is declared but no extended card is
configured, the operation **MUST** return
[`ExtendedAgentCardNotConfiguredError`]."

**Now.** That case returns `-32007`; the examples and the TCK SUT that declare
the capability pass their public card to `with_extended_agent_card`, so what
they serve is unchanged. `a_declared_card_with_no_extended_card_is_not_configured`
failed before the change.

## R69 — a subscription across replicas can lose an event

Found 2026-10-15 while working the §3.5.2 entry (line 692). Decided the same
day by the maintainer: option (a).

**Now.** A `SubscribeToTask` on a replica with no live queue for the task
(`crates/a2a-protocol-server/src/handler/lifecycle/subscribe.rs`,
`snapshot_and_live`) reads the log's last position, then the task, and starts
the stream after that position; the snapshot is the task as read. Another
replica sharing the store may be running the task, and its processor records
each non-terminal event in the log before it stores the state the event leads
to (`handler/event_processing/background/processor.rs`, `handle`, and
`record_event` in `background/mod.rs`). Its last logged event can therefore
be unstored when the task is read: that event is at or below the stream's
start, so it is never replayed, and it is not in the snapshot. The subscriber
loses it, and no `streamLagged` says so. The ledger recorded the mirror case
only (an event logged and stored between the two reads arrives twice).

**Shown by** a test, written 2026-10-15 and failing on this tree, not
committed (a failing test cannot be, and one is not skipped): a `TaskStore`
wrapper over `InMemoryTaskStore` that, on the first `last_event_seq` call
after a flag is set, appends an `other-1` artifact event at the next position
without saving it, and saves the task with `other-1` right after the next
`get` has read it. In `tests/event_log_tests/across_turns.rs`'s setup, a
`start` turn parks the task, the flag is set, `on_resubscribe` runs, and a
`fast` turn follows. `other-1` reaches the subscriber 0 times: the snapshot
holds `start-1` only, the stream `fast-1` to `fast-3`, and no error.

**Text.** §3.5.2 (line 692): "Each stream MUST receive the same events in the
same order."

**Options.**
- (a) The store reports the log position a saved task reflects: the processor
  saves with the position of the event it folded, and a defaulted
  `TaskStore` method returns the task with that position (`None` for a store
  that does not keep it, which keeps today's behaviour). The subscription
  starts after the position the snapshot reflects, so it neither loses nor
  repeats an event. The in-memory, SQLite and PostgreSQL stores and their two
  tenant-aware SQL counterparts keep it (a column and a migration for each of
  the four SQL schemas); the tenant-aware in-memory store and the audit
  wrapper forward it. Exact; the largest change.
- (b) Replay the log's last event as well unless the snapshot is seen to
  reflect it, judged per event kind (a status equal to the stored one, an
  artifact chunk already present). No API change, but a judgment that can
  repeat an event or, misjudged, still lose one.
- (c) Leave it recorded in the ledger. Only deployments that run one task's
  turns on one replica and subscribe on another are exposed, in the window
  between one event's append and its save.

**Recommendation.** (a), which also removes the duplicate the ledger records.

**Done (option (a)).** `TaskStore::get_with_position` returns the task with
the position it reflects, and a subscription with no live queue starts there
(`crates/a2a-protocol-server/src/handler/lifecycle/subscribe.rs`,
`snapshot_and_live`). The processors run each event's writes inside
`AppliedPosition::scope` (`crates/a2a-protocol-server/src/store/applied.rs`),
and all six shipped stores record the position atomically with the write,
keeping it on a write outside a scope. SQLite's journal rows carry their own,
and a full save keeps the larger of the row's and the journal's. The SQL
stores add an `applied_seq` column (`SQLite` migration 8, PostgreSQL migration
7, and every `from_pool` for an earlier database). The test above is
committed as `an_event_logged_before_the_read_and_stored_after_it_is_not_lost`
(`crates/a2a-protocol-server/tests/event_log_tests/across_turns.rs`) and fails
with the subscription's old order; `crates/a2a-protocol-server/tests/applied_position.rs`
holds every store to the contract, PostgreSQL included when
`A2A_TEST_POSTGRES_URL` is set. 23 hand mutants, all killed.

An independent review of the change before it was committed found, and
reproduced, two defects it had introduced, both fixed in the same commit:

- **A failed append handed its position to the next turn.** The fold is
  stored with the position of an event whose append failed, so the stored
  position could be past the log's end; the next turn, numbered from the
  log's end, reused it, and a subscriber that started from the stored task
  dropped that turn's first event. The same reuse already lost the event for
  a subscriber that had seen the failed event live, before this change. A turn
  is now numbered after the later of the two (`resume_after`,
  `crates/a2a-protocol-server/src/handler/messaging/admission.rs`); tests in
  `tests/event_log_tests/failed_append.rs`.
- **`with_migrations` failed on a database `from_pool` had opened** ("duplicate
  column name"). The `SQLite` runner now skips an `ADD COLUMN` whose column
  exists; test `a_column_from_pool_already_added_does_not_fail_its_migration`.

It also found the PostgreSQL `from_pool` taking an `ACCESS EXCLUSIVE` lock on
every start (measured in `pg_locks`; now a check that takes none), and two
limits now documented rather than fixed: a stale write from another replica
keeps a position whose effect it overwrote (a lost update older than R69), and
during a rolling upgrade a replica on an earlier release leaves the position
behind, so a subscriber can receive events twice (not lose them).

## Ledger entries put to the maintainer

| Ledger entry | Now | Recommendation |
|---|---|---|
| §8.4.3 (line 2122), was medium: "Retrieve the public key using the `kid` and `jku` (or from a trusted key store)" | `kid` lookup in a caller-supplied JWKS, tested; `jku` decoded, never followed | **Done**: TESTED. The trusted-key-store branch is what is implemented. Following a card's own `jku` would let a card name the key that vouches for it. |
| §12 (line 2977) and §5.4 (line 1194), was medium/low: custom bindings define "equivalent error code mappings that preserve the semantic meaning" | SLIMRPC follows its own spec's table (UNIMPLEMENTED where A2A's gRPC column says FAILED_PRECONDITION) | **Done**: both MUSTs TESTED. The text asks for an equivalent mapping that preserves meaning, not the gRPC column. The upstream report is drafted for the maintainer, not filed. |
| §13.3 (line 3164): no card and no extended card | `-32007 ExtendedAgentCardNotConfigured` (`crates/a2a-protocol-server/src/handler/lifecycle/extended_card.rs:83-87`, before the change) | **Done**: `UnsupportedOperationError`; a handler with neither card cannot serve the operation, and `-32007` asserts a capability it cannot know is declared. |
| proto `Artifact.parts` (line 287): "Must contain at least one part" | an `append: true` chunk with no parts was accepted (`crates/a2a-protocol-server/src/streaming/event_queue/content.rs:36-37`, before the change), and one for an unknown artifact id stored a zero-part artifact | **Done**: refused at the queue write and dropped by both processors. |
| App. A.2 (line 3421): clients prefer new names, dual-handle, warn on legacy forms | legacy enum spellings and `AgentCard.url` parsed; no fallback for `supportsAuthenticatedExtendedCard`; no warning; `url` not folded into `supportedInterfaces` | **Done** (`crates/a2a-protocol-client/src/legacy_card.rs`): a fetched or extended card's `url`, `preferredTransport` and `additionalInterfaces` fold into `supportedInterfaces`, `supportsAuthenticatedExtendedCard` and A.2.2's `supportsExtendedAgentCard` into `capabilities.extendedAgentCard`, and each is named in a `tracing` warning. Not done: A.2.1's `kind` discriminator on parts and events, which only a v0.3 interface sends, and the client never selects a v0.3 interface. |
| §5.7 (line 1263): required arrays non-empty | `ListTasksResponse.tasks` may be empty (the spec contradicts itself); `HotReloadAgentCardHandler::update` does not validate | **Done**: `try_update` validates as `reload_from_json` does, and `update` is deprecated (one hand mutant, killed). The upstream report on `ListTasksResponse.tasks` is drafted for the maintainer, not filed; the entry stays PARTIAL on that contradiction alone. |
| §8.1 (line 1970): a server MUST make an agent card available | JSON-RPC, REST and axum serve it; WebSocket and gRPC serve none | **Done**: the WebSocket listener peeks the request line and answers a card fetch as HTTP/1.1 (`dispatch/websocket/card.rs`); the gRPC router gains a `.well-known` service and accepts HTTP/1.1 when there is a card (`dispatch/grpc/well_known.rs`). Both answer through `dispatch/well_known.rs` with the HTTP dispatchers' card handler. 8 hand mutants, all killed. |
| §14 (line 3342): implementations SHOULD support HTTPS | only gRPC terminates TLS | **Done** for `serve::Server` (`Server::with_tls`, `with_tls_pem`, under `tls-rustls`): the handshake runs per connection within the header read timeout, ALPN offers `h2` and `http/1.1`, and a failed handshake counts as a failed connection. 8 hand mutants, all killed. `serve`, `serve_with_addr` and the WebSocket listener stay plaintext, behind a proxy. |
| §14 (line 3344): clients SHOULD verify signatures when present | opt-in, per fetch; `CachingCardResolver` and `ClientBuilder::from_card` take no keys | **Done**: `CachingCardResolver::with_options(CardFetchOptions)`, and `ClientBuilder::from_signed_card(json, keys)`, which verifies the JSON as given (a typed `AgentCard` drops fields the signer covered). 3 hand mutants, all killed. |
| §3.3.2 (line 544): use `BadRequest` where applicable | the handler's own validation names the field in the message only | **Done** as a new `ServerError::InvalidField { field, message }`: at 0.14.1, when this was done, changing `InvalidParams`' payload would have failed the semver gate; the new variant answers exactly as `InvalidParams` does, plus the detail. |
| §3.1.6 (line 309), §3.5.2 (line 692), §8.6 / §8.6.2 (lines 2215, 2225), §11.1 server (line 2750), §12.4 (line 3016) | dispositions recorded in each entry's notes | **Approved as recorded (2026-10-14).** Since then: §3.5.2 had R69, now done, and the catch-up paging awaits a call (Open, above); §12.4 cannot close here (Open, above); §3.1.6's idle bound was reopened on 2026-10-15 and is off by default since, so that entry is TESTED. |
| §12.6 (line 3033): custom bindings MUST define how authentication challenges are communicated | approved as recorded: the WebSocket binding answers a refused credential `-32600` with no `data`, so its challenge is dropped | **Done** (2026-10-15), beyond the approved disposition: the `-32600` carries an `ErrorInfo` from `a2a-rust.com`, `UNAUTHENTICATED` with the challenge as `www-authenticate` metadata or `PERMISSION_DENIED` (binding specification §6), and the client reports them as `401` and `403`. The entry is TESTED; 9 hand mutants, all killed, and a tenth, on the mid-stream error path no refusal reaches, survived, so that path was left unchanged. |
| MAY, NOT_SUPPORTED: §3.1.9 pagination, §3.6.1, §8.4.2 `jku` in signers, §8.6.2 default cache lifetime, §11.5 repeated query fields, §11.7 final snapshot, §13.3 per-caller extended card | not implemented | **Done** for three, now SUPPORTED: §3.1.9 pagination (`RequestHandler::on_list_push_configs_page`, every binding; 10 hand mutants, all killed), §8.4.2 (`sign_card_es256_with_jku` and `sign_card_ed25519_with_jku`, https only; 5 killed) and §13.3 (`ExtendedCardProvider`, `RequestHandlerBuilder::with_extended_card_provider`; 10 killed). The other four stay NOT_SUPPORTED, with the reasons in their entries. |
