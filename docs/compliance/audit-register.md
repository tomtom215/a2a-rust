<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Audit register — open items

The spec audit of `claude/tender-cray-bsfw83` kept a register, R1 to R67, of
findings against the A2A specification, the proto and the RFCs they cite. It
lived in that session's scratchpad and was never committed; the handoff,
the CHANGELOG and the spec ledger refer to its entries by number (54 distinct
ids). The entries it closed are recorded where they were fixed: each fix's
handoff item and CHANGELOG entry names its register id. This file holds the
items still open, so the register's open half is in the repository.

Each item gives what the code does now, read from source at `9b08a860` (each
file and line was opened, not recalled), the text it is measured against, the
options, and a recommendation. Under the maintainer's directive to be
completely v1.0.1-conformant, the recommendation is the conformant option
unless that would break something worse, in which case the item says what.
**Nothing below is implemented until the maintainer decides it.** Spec line
numbers are in `docs/implementation/a2a-v1.0.1-specification.md` (the v1.0.1
release text; the ledger's own numbering).

| Id | Area | Recommendation | Breaks |
|---|---|---|---|
| R12 | JWT caller identity | key by issuer and subject; refuse a token with no `sub` | re-keys stored partitions |
| R13 | SLIMRPC caller identity | move to agntcy-slim-rpc 3.x and record the verified peer | the binding's API (unpublished) |
| R19 | `Struct` error details | write ProtoJSON `Any` (`{"@type", "value"}`), read both shapes | a2a-go reading our `data` |
| R21 | JSON-RPC notifications | execute, never reply | a client that relied on a reply |
| R33 | first frame of a send stream | keep, documented | — |
| R36(b) | task snapshot moving back to `Submitted` | refuse at the queue, as a status update is | an executor that writes one |
| R41 | `x-forwarded-for` over SLIMRPC | strip forwarding headers in the binding | none known |
| R49 | types not generated from the proto | see §1.4 below | the public types' API |
| R50 | REST client `Content-Type` | send `application/a2a+json` | a2a-java ≤ 1.3 servers |
| R66 | multicast member lag | typed `streamLagged`, still terminal | code matching the old text |
| R67 | API key in the query string | carry query parameters on the HTTP transports | custom `Transport` impls, if the trait changes |

## R12 — a JWT caller is its `sub` alone

**Now.** `crates/a2a-protocol-server/src/auth/jwt.rs:763-765` records `principal.subject` as the caller;
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
(`jwt:<iss>:<sub>`, `apikey:<label>`, `bearer:<label>`), and refuse a JWT with
no `sub` when caller isolation is on. (c) (b) without the refusal.

**Recommendation: (b).** `sub` is only unique within its issuer, and a
sub-less token sharing the tenant partition is a cross-caller read under the
default isolation. It re-keys every stored partition: existing tasks become
invisible to their owners unless a migration maps the old keys, so it ships
with a documented migration (or a read-through of the old key for one
release).

## R13 — SLIMRPC records no caller identity

**Now.** The binding depends on agntcy-slim-rpc 2.3.0 (`bindings/a2a-protocol-slimrpc/Cargo.toml:66`,
`bindings/a2a-protocol-slimrpc/Cargo.lock`). It builds the request's headers from SLIM metadata only
(`bindings/a2a-protocol-slimrpc/src/server/methods/mod.rs:80`), drops transport keys and every `:`-prefixed key
(line 111), and never sets a caller identity or `:peer-addr`. 2.3.0's `Context`
exposes `session().source()` (the session's peer name) but no claims; whether
SLIM verified that name is not visible from the crate. A credential sent as
metadata (`authorization`) still reaches the handler's interceptors, which
can authenticate it, so per-caller scoping is available that way; what is
missing is the transport-verified identity.

**Text.** §13.1 (line 3075), as for R12; §12.6 (line 3033), custom bindings
**MUST** "Maintain Security".

**Options.** (a) Keep, document that SLIMRPC callers must present an A2A
credential. (b) Upgrade the four SLIM crates to the 3.x line, record the
MLS-verified peer subject and issuer under `:`-prefixed keys the caller
cannot forge (the binding already drops caller-sent `:` keys), and ship an
interceptor that authenticates from them. The 3.x accessors
(`Context::peer_subject`, `peer_issuer`, MLS sessions only) are as the
previous session read them and are to be confirmed against 3.3.1's source.

**Recommendation: (b).** It also removes RUSTSEC-2024-0449:
agntcy-slim-config 0.17.3, which agntcy-slim-rpc 3.3.1 requires, replaces
serde_yaml with yaml_serde (checked against the crates.io index,
2026-10-14). It does not fix RUSTSEC-2026-0285 or the 2026-10-14 rustls
advisories: mls-rs-crypto-awslc 0.25.0 still pins aws-lc-rs =1.16.3.

## R19 — `Struct` error details are written flat

**Now.** `crates/a2a-protocol-server/src/streaming/sse.rs:330-339` copies `A2aError.data`'s fields into the
detail and adds `"@type": "type.googleapis.com/google.protobuf.Struct"`; a
non-object goes under `"value"`. Used by JSON-RPC, HTTP+JSON and their
streams. The doc says it is written "the way a2a-go writes and reads one".

**Text.** §3.3.2 (line 544): each detail "**MUST** include a `@type` key …
(using ProtoJSON `Any` representation)"; §9 (line 2437) and §11.6 the same.
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

## R21 — JSON-RPC notifications are answered

**Now.** `crates/a2a-protocol-server/src/dispatch/jsonrpc/mod.rs:303` and `:372` take
`rpc_req.id.to_response_id()` with no check for an absent `id`, so a
notification is executed and answered with `"id": null`; the batch loop
(`:239-265`) and the WebSocket path (`crates/a2a-protocol-server/src/dispatch/websocket.rs:856`) do the same.
`crates/a2a-protocol-server/tests/jsonrpc_edge_tests.rs` `jsonrpc_absent_id_request_answered_with_null_id`
pins it.

**Text.** §9 (line 2270): "All JSON-RPC requests **MUST** follow the standard
JSON-RPC 2.0 format". JSON-RPC 2.0 §4.1: "The Server MUST NOT reply to a
Notification, including those that are within a batch request."

**Options.** (a) Keep. (b) Execute, and send no reply: HTTP `204 No Content`
for a single notification, a batch's array without its notifications (and
`204` when every member is one), nothing on WebSocket. (c) Refuse to execute
notifications, with no reply.

**Recommendation: (b).** It is what JSON-RPC 2.0 requires. A notification
`SendMessage` creates a task whose id its sender never learns, which is the
sender's choice.

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

## R36(b) — a task snapshot may move a task back to `Submitted`

**Now.** `crates/a2a-protocol-server/src/handler/event_processing/background/state_machine.rs:381-384` and
`crates/a2a-protocol-server/src/handler/event_processing/sync_collector.rs:524-526` store an executor's `Task` snapshot without
`can_transition_to`, which forbids re-entering `Submitted`
(`crates/a2a-protocol-types/src/task.rs:318`) and is applied only to status updates. The store refuses only
writes over a terminal state.

**Recommendation: refuse it at the queue write**, as a status update with the
same transition is refused, through the processors' existing mark-failed
path. No test pins the current acceptance.

## R41 — SLIMRPC believes a caller's `x-forwarded-for`

**Now.** The binding passes caller metadata named `x-forwarded-for` into the
request's headers (`bindings/a2a-protocol-slimrpc/src/server/methods/mod.rs:111` drops only transport and `:`
keys). With `trusted_proxy_hops > 0`, `crates/a2a-protocol-server/src/rate_limit/identity.rs:41,50` and
`crates/a2a-protocol-server/src/auth/throttle.rs:191-195` take the caller's own value as the client address:
no proxy appends to it on the SLIM path, so the caller picks its rate-limit
and throttle key. SLIMRPC sets no `:peer-addr`, so with hops = 0 the throttle
never throttles and every unidentified caller shares one rate-limit bucket.

**Recommendation: strip `x-forwarded-for`, `x-real-ip` and `forwarded` in the
binding**, and key unidentified SLIMRPC callers by the session's source name
(or the verified peer once R13 lands).

## R49 / §1.4 — the types are not generated from the proto

**Now.** Only the prost types (feature `proto`) are generated. The public
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

**Recommendation.** Only (b) meets the text. (c) is the strongest check
short of it and can land first, so drift fails CI while (b) is planned. This
is the largest change in the register; it needs the maintainer's decision on
the API break before any work.

## R50 — the REST client sends `application/json`

**Now.** `crates/a2a-protocol-client/src/transport/rest/request.rs:133` sends
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

## R66 — a multicast member's lag report

**Now.** `bindings/a2a-protocol-slimrpc/src/multicast/fanout.rs:128-129` sends the member an `Err`
that ends its stream (`crates/a2a-protocol-client/src/streaming/event_stream.rs:485-487`). The
error is `ClientError::Transport("N event(s) dropped…")`, not the typed
`streamLagged` error the other bindings send.

**Recommendation: keep it terminal and make it the typed `streamLagged`
error**, so one check (`is_stream_lagged`) recognises a lag on every binding.

## R67 — an API key in the query string is refused

**Now.** `crates/a2a-protocol-client/src/auth.rs:281-284` refuses `ApiKeyLocation::Query`; the
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

## Ledger entries that need a decision

| Ledger entry | Now | Recommendation |
|---|---|---|
| §8.4.3 (line 2122), medium: "Retrieve the public key using the `kid` and `jku` (or from a trusted key store)" | `kid` lookup in a caller-supplied JWKS, tested; `jku` decoded, never followed | **TESTED**: the trusted-key-store branch is what is implemented. Following a card's own `jku` would let a card name the key that vouches for it. |
| §12 (line 2977) and §5.4 (line 1194), medium/low: custom bindings define "equivalent error code mappings that preserve the semantic meaning" | SLIMRPC follows its own spec's table (UNIMPLEMENTED where A2A's gRPC column says FAILED_PRECONDITION) | **TESTED**, with the conflict reported upstream: the text asks for an equivalent mapping that preserves meaning, not the gRPC column. |
| §13.3 (line 3164): no card and no extended card | `-32007 ExtendedAgentCardNotConfigured` (`crates/a2a-protocol-server/src/handler/lifecycle/extended_card.rs:83-87`) | `UnsupportedOperationError`: a handler with neither card cannot serve the operation, and `-32007` asserts a capability it cannot know is declared. |
| proto `Artifact.parts` (line 287): "Must contain at least one part" | an `append: true` chunk with no parts is accepted (`crates/a2a-protocol-server/src/streaming/event_queue/content.rs:36-37`), and one for an unknown artifact id stores a zero-part artifact | Refuse it at the queue write. |
| App. A.2 (line 3421): clients prefer new names, dual-handle, warn on legacy forms | legacy enum spellings and `AgentCard.url` parsed; no fallback for `supportsAuthenticatedExtendedCard`; no warning; `url` not folded into `supportedInterfaces` | Implement: the fallback, a warning (client, `tracing`), and `url` plus `preferredTransport` folded into an interface. |
| §5.7 (line 1263): required arrays non-empty | `ListTasksResponse.tasks` may be empty (the spec contradicts itself); `HotReloadAgentCardHandler::update` does not validate | Report the contradiction upstream; add a validating `try_update`, deprecate `update`. |
| §8.1 (line 1970): a server MUST make an agent card available | JSON-RPC, REST and axum serve it; WebSocket and gRPC serve none | Serve `/.well-known/agent-card.json` from the WebSocket listener, and from the gRPC server through tonic's HTTP router. |
| §14 (line 3342): implementations SHOULD support HTTPS | only gRPC terminates TLS | TLS on the HTTP listeners under `tls-rustls`. |
| §14 (line 3344): clients SHOULD verify signatures when present | opt-in, per fetch; `CachingCardResolver` and `ClientBuilder::from_card` take no keys | Take keys in both. |
| §3.3.2 (line 544): use `BadRequest` where applicable | the handler's own validation names the field in the message only | Carry field violations on `ServerError::InvalidParams`. |
| §3.1.6 (line 309), §3.5.2 (line 692), §8.6 / §8.6.2 (lines 2215, 2225), §11.1 server (line 2750), §12.4 (line 3016), §12.6 (line 3033) | dispositions recorded in each entry's notes | Approve each as recorded, or name the ones to change. |
| MAY, NOT_SUPPORTED: §3.1.9 pagination, §3.6.1, §8.4.2 `jku` in signers, §8.6.2 default cache lifetime, §11.5 repeated query fields, §11.7 final snapshot, §13.3 per-caller extended card | not implemented | Implement §3.1.9 pagination, §8.4.2 (`jku` on the signer) and a §13.3 per-caller card hook; keep the rest NOT_SUPPORTED with the reasons in their entries. |
