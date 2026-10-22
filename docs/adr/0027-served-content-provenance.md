<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# ADR 0027: Content is signed when served and verified when received

**Date:** 2026-10-22
**Status:** Accepted
**Author:** Tom F.

---

## Context

`a2a_protocol_types::provenance` can mark a message or artifact as
AI-generated and sign it (a detached JWS over its RFC 8785 canonical JSON,
ES256 or EdDSA), and verify it against a JWK Set. Nothing in the server
called it: an agent that wanted its output signed had to sign every event
itself, and the store then broke what it signed (below).

The maintainer's decision of 2026-10-21: receivers verify present
signatures by default and refuse altered content, signing turns on when a
key is configured, unsigned peers still interoperate (marked unverified),
and the production profile requires a key. This ADR records all four.

## Decision

- **`RequestHandlerBuilder::with_content_signer(ContentSigner)`** (feature
  `signing`) turns signing on. Without it nothing changes.
- **Signed on the way out, over exactly what is sent.** Every stream frame
  as the reader yields it, every push (the configured `PushSender` is
  wrapped), and the tasks and messages `SendMessage`, `GetTask`,
  `ListTasks`, `CancelTask` and `SubscribeToTask` return, after
  `historyLength` and `includeArtifacts` have trimmed them
  (`src/provenance.rs`).
- **What is signed:** what only the executor writes: artifacts, status
  messages and direct replies, whatever role a message claims. Not a task's
  history: it holds the caller's messages as they arrived, and a caller may
  send one with role `agent`, so signing agent-role history signed the
  caller's words as the agent's (review finding H1, below).
- **What is left alone:** content that already carries a signature. An
  agent forwarding another agent's signed artifact passes that signature on,
  which is what provenance is for; re-signing it would replace the original
  author with the forwarder.
- **Malformed provenance is refused when written.** An executor's event
  whose `a2a-rust.com/provenance` value is not provenance is refused by the
  queue writer, with the other content checks there, so a stored task can
  always be signed when it is read.

- **The server verifies what it is sent.**
  `RequestHandlerBuilder::with_content_keys(Jwks)` checks the provenance
  signature of each message sent to the agent as it arrived: before
  validation rewrites it (empty strings unset, base64 re-padded), before an
  approval vote is counted, and before a send key's replay is answered.
  A signature that does not verify (the message was altered, its marker
  included, or it was signed by a key not in the set) is refused with
  `InvalidParams` and the executor never runs. An unsigned message is
  accepted and the executor reads `ContentCheck::Unsigned` from
  `RequestContext::content_check`; a verified one reads `Verified` with the
  provenance it vouches for; without keys, `NotChecked`.
  `require_signed_content()` refuses unsigned messages too.
- **The client verifies what it receives.** `ClientBuilder::with_content_keys`
  checks artifacts, status messages and direct replies, whatever role a
  message claims, in stream events and in what `send_message`, `get_task`,
  `list_tasks` and `cancel_task` return; history messages are verified when
  signed and never required to be. One that fails ends the call, or the
  stream, with `ClientError::ContentNotVerified`, which is not retried;
  nothing buffered behind a refused frame is delivered, and the refused
  frame does not count as delivered. Unsigned content is returned as it is
  unless `require_signed_content()` is set.
- **Every binding serves the same bytes.** gRPC reads an empty string or
  list as absent, so the queue writer unsets empty optional strings and
  lists in messages, artifacts and parts, and a signature verifies after a
  protobuf round trip.
- **"By default", read as: no switch beyond the keys.** A signature cannot
  be checked without the signer's key, and A2A gives no way to learn a
  peer's content keys, so verification starts when keys are given and has
  no separate opt-in. Unsigned content is unverified, never refused unless
  asked: anyone able to alter content can also remove its signature.
- **`Profile::Production` requires a signer.** Without the `signing`
  feature the profile cannot be met, and `build()` says to enable it.

Alternatives not taken:

- *Signing when the executor writes, once.* Cheaper, and one place. But an
  artifact streamed in chunks (`append: true`) is merged by the store, and
  the merged artifact would carry one chunk's signature over parts it does
  not cover: the in-memory store copies the last chunk's metadata, and the
  SQLite and PostgreSQL append deltas write the new parts alone and keep the
  first chunk's. Every `GetTask` of such a task would serve a signature that
  fails, which a verifying client must refuse as altered.
- *Re-signing the merged artifact in the event processors.* Every append
  would then change the artifact's metadata, which the append deltas do not
  write, so each would fall back to rewriting the task whole: quadratic in
  the length of a stream.

## Consequences

- Each item is signed each time it is served: one signature per artifact
  and agent message per response or frame (Ed25519 or P-256 ECDSA; not
  measured here). A stream's frames are signed per subscriber.
- A chunk on a stream verifies as that chunk; the artifact `GetTask` returns
  verifies as the whole. Content at rest is unsigned, unless the executor
  signed it; at-rest integrity is the encrypted store's (`store::encrypted`).
- Marking content AI-generated stays the executor's call
  (`mark_ai_generated`, before the server signs); the SDK cannot tell
  generated content from any other.
- Not covered, by design or not yet:
  - Only content is signed: a task's state, an event's `append` and
    `lastChunk`, and which task an artifact belongs to are not, so a signed
    artifact can be replayed into another task.
  - The client tells the caller nothing about unsigned content; only the
    server's `ContentCheck` marks it. Neither side finds a peer's keys: the
    caller supplies them, and the production profile does not require
    inbound keys.
  - An artifact whose chunks the executor signed itself, or forwarded
    signed, is merged by the store and then fails verification, since a
    signed chunk is passed on as it is.
  - Each item is signed each time it is served, per response and per
    subscriber, and nothing is cached; a caller who reads a task with large
    artifacts repeatedly makes the server sign it repeatedly.
  - A message another sender signed with an empty optional string does not
    verify when it arrives over gRPC, which reads that string as absent.
  - gRPC and WebSocket are not exercised end to end (the stream reader they
    consume signs, the client checks every `EventStream`, and the gRPC
    conversion of a signed artifact is tested); content another
    implementation extended with fields these types do not model will not
    verify after a round trip through them.
- **Independent review, 2026-10-22.** A reviewer reading the code without
  our conclusions found, and these fixes address, each with a test that
  fails on the old code: the caller's agent-role message signed as the
  agent's (H1); the check run after an approval vote, a send key's replay
  and validation's rewrites (H2, L1, M2); signatures broken by gRPC's empty
  strings (M3); verification passing a member added inside the provenance
  object, or a removed extension URI (M4, in `verify_content`, unreleased);
  the client skipping a signed message relabelled to another role (M5); a
  stream delivering buffered frames after a refusal (L2). A stored task
  made unreadable by a caller's malformed provenance (M1) went with H1:
  history is no longer signed. The quorum path (H2) is fixed by the same
  reordering but has no test of its own. The rest are listed above.
- Tests (`tests/provenance_signing.rs`): every stream frame and push, the
  blocking send, `GetTask` (the merged artifact verifying whole), `ListTasks`,
  `CancelTask` and `SubscribeToTask` verify against the signer's key after a
  JSON round trip; a forwarded artifact keeps its author's signature; the
  caller's message stays unsigned; nothing is signed without a signer; and
  malformed provenance is refused. `tests/provenance_verify.rs`: a known
  key verifies; an altered message, a removed marker and an unknown key are
  refused before the executor runs; unsigned is `Unsigned`, refused when
  required; nothing is checked without keys. In the client,
  `tests/content_verification.rs` (a stub serving signed, altered and
  unsigned content) and `tests/content_end_to_end.rs` (this server signing,
  this client verifying, over JSON-RPC and REST, the merged artifact
  included). Each hand mutant of 2026-10-22 fails at least one test: on the
  server, no signing on the stream, `GetTask`, `ListTasks`, `CancelTask`,
  `SubscribeToTask` or push, signed content re-signed, no malformed check,
  the check's result dropped, a failed verification accepted, the
  requirement ignored; in the client, no stream check, no `GetTask` check,
  the requirement ignored.
