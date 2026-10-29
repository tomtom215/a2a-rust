<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Streams and executors

Part of [Upgrading from 0.14 to 0.15](../0.14-to-0.15.md). Each section names what breaks and what to change; the [upgrade guide](../../upgrading.md) has the policy and how to check your own code.

## What an executor may write is checked

The event queue every executor writes through now holds what it writes to
rules the server already held a client to. A write that breaks one returns
`InvalidParams` naming the field, before the event is stored or sent:

- A message with no `messageId`, role or parts, or in `ROLE_UNSPECIFIED`; an
  artifact with no `artifactId` or parts; a status in
  `TASK_STATE_UNSPECIFIED`. These are refused wherever they sit: in the event
  itself, or inside a status update or a task snapshot.
- A status, artifact or task event that names another task or context than the
  running one. An empty task or context id gets the running task's.
- What the gRPC binding cannot carry: a `raw` part that is not base64 (one in
  URL-safe or unpadded base64 is kept in standard padded form), an integer
  beyond ±2^53 in metadata or a `data` part, and `metadata` that is not a JSON
  object. Over JSON they were stored and served, and the task then answered
  `INTERNAL` over gRPC.

Five more things change for an executor and the streams it feeds:

- A `Message` written after the task stream began is streamed as a
  `TaskStatusUpdateEvent` that keeps the state the turn last wrote and carries
  the message in `status.message`. A follow-up (a message naming the task's
  `taskId`) answered with a `Message` gets a task stream, a `Task` and then
  that update, and a blocking follow-up gets the `Task` with the message as
  its status message. A new task's first-event `Message` is still a direct
  reply.
- A streaming send whose first event is a `Message` is the message-only stream
  §3.1.2 describes: that one `Message`, then the stream closes. It used to
  lead with a `Task` snapshot, so the stream was `[Task, Message]`. An
  executor that wants a task stream and narrates first writes a status update
  before its first `Message`.
- Events written after the terminal status are not streamed, and a stream ends
  once the terminal event is delivered.
- A non-append artifact update for an id the task already holds replaces that
  artifact, in the blocking and background paths alike; it stored a second one.
- `RequestContext::activated_extensions()` lists only the extensions the card
  declares, in request order. An executor that read an undeclared one no
  longer finds it: declare it on the card. Without a card the list passes
  through.

A client that read a mid-task `Message` event reads `status.message` on the
status update instead:

```rust
use a2a_protocol_types::{Message, StreamResponse};

fn narration(event: &StreamResponse) -> Option<&Message> {
    match event {
        // 0.14 also delivered a mid-task `StreamResponse::Message`.
        StreamResponse::StatusUpdate(update) => update.status.message.as_ref(),
        _ => None,
    }
}
```

## A parked subscription stays open until its task finishes

`HandlerLimits::subscribe_max_idle` is an `Option<Duration>` and `None` by
default: a `SubscribeToTask` stream to a task parked in `input-required` or
`auth-required` runs until the task reaches a terminal state or the client
goes, as §3.1.6 requires. It used to end after five minutes with no terminal
state, and the client resubscribed. `with_subscribe_max_idle(d)` still sets a
bound and takes a `Duration` as before; code that read the field compares
against `Some(d)` now. A bound trades conformance for reclaiming subscriptions
whose clients are connected but no longer reading; a client that has gone is
noticed without one (the field's rustdoc says how, per binding).

## A stream that falls behind catches up instead of ending

A reader slower than the agent used to end with `streamLagged` as soon as the
broadcast queue overwrote an event it had not read. It now reads what it
missed from the task's event log and goes on (spec §3.5.2), on every store that
keeps a log, which is every built-in one. A test that provokes `streamLagged`
with a small `with_event_queue_capacity` no longer sees it there; also bound
the log below what the stream misses
(`TaskStoreConfig::with_max_events_per_task`) and, since an attached stream
now holds what it owes past that bound, leave it no room to
(`with_max_held_events_factor(Some(1))`). Lowering
`HandlerLimits::subscribe_replay_limit` no longer provokes it either: since
0.15.0 the limit is the size of each read of the log, and a longer replay is
read in pages (next subsection).

## A replay longer than `subscribe_replay_limit` is read in pages

Spec §3.5.2: each stream MUST receive the same events in the same order.
`HandlerLimits::subscribe_replay_limit` (1,000 by default) bounds each read of
the task's event log now, not the replay. A stream that lagged by more than the
limit used to end with `streamLagged`, and a `Last-Event-ID` resume that
needed more than the limit stopped at it and went on live, past the positions
between the replay's end and where its receiver attached, with no error. Both
now read the log a page at a time, from where each page ended, delivering each
page before the next is read, and go to the live channel only after the last
page. The same holds for a turn a `SubscribeToTask` stream missed between
polls. A page the log cannot supply is still reported as `streamLagged`, once,
after what it did supply.

Memory stays bounded by the limit, but a client that resumes with
`Last-Event-ID: 0` now receives the task's whole retained history on one
connection, where it received at most one page. A proxy, a client buffer or a
test that assumed the old cap sees more frames; the limit still sets the size
of each read, and `with_subscribe_replay_limit` sets it:

```rust
use a2a_protocol_server::HandlerLimits;

let limits = HandlerLimits::default().with_subscribe_replay_limit(500);
# let _ = limits;
```

## An appended artifact chunk needs a part

An executor's `TaskArtifactUpdateEvent` with `append: true` and no parts is
refused by the event queue (`InvalidParams`), as one without `append` already
was: a2a.proto's `Artifact.parts` "Must contain at least one part" and makes
no exception for a chunk. An executor that ended a streamed artifact with an
empty `lastChunk: true` chunk sets `lastChunk: true` on the chunk carrying the
last part instead.

## A task snapshot cannot move a task back to `Submitted`

An executor's `Task` snapshot that changes the task's state is held to the
transitions a status update is. For one moving a task back to `Submitted`,
the background processor marks the task failed, and a blocking send answers
`InvalidStateTransition` (an `InvalidParams` error), as each does for such a
status update. One leaving a terminal state is refused. A snapshot in the
task's current state is stored as before.
