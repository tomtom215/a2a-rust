<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Boundaries before 0.7

Before 0.8 the changelog was the migration guide. It still is for these; only
0.5 → 0.6 breaks nothing at compile time (0.4 → 0.5 changes the `TaskStore`
signatures, and 0.3 → 0.4 the wire format, `PartContent` and `OAuthFlows`),
and the 0.6 → 0.7 boundary has [a page of its own](./0.6-to-0.7.md):

- **0.5 → 0.6** (`## [0.6.0] - 2026-06-10`): no `Breaking` heading, and
  nothing that fails to compile — no public API signature changed, which is
  what makes this the easiest boundary to cross without noticing. A
  types-only consumer gets a clean build and a different service. What moved
  is the bytes on the wire and which requests are accepted, both breaking
  under [the policy](../upgrading.md#the-policy), so read `### Changed` and `### Fixed` for this
  release rather than looking for a section that is not there.
  `Task.history` is populated for the first time — nothing had ever appended
  to it — so `tasks/get` now returns real history subject to `historyLength`,
  capped at 1,024 messages with the oldest dropped first, while `message/send`
  responses and streaming snapshots omit it unless
  `SendMessageConfiguration.historyLength` asks; response sizes move in both
  directions. A dropped `message/stream` connection no longer cancels the
  task: work runs to completion and clients reattach with `SubscribeToTask`,
  so anything that relied on disconnect-kills-task must now call `CancelTask`
  explicitly. `Working → Working` became a valid transition, so an executor
  emitting more than one progress note is no longer marked
  `TASK_STATE_FAILED` by the background processor behind a stream that
  already said `Completed` — if you worked around that split-brain, the
  workaround can go. Continuations carry artifacts, metadata and history
  forward instead of overwriting them, with only the status returning to
  `Submitted`. `tasks/resubscribe` joins `SubscribeToTask` and
  `tasks/subscribe` as a routed alias. And on the client, a JSON-RPC error
  answering `message/stream` surfaces as `ClientError::Protocol` carrying the
  original code, instead of ending the stream with zero events and no error,
  so code that read an empty stream as "nothing happened" now sees the
  failure it was missing.
- **0.4 → 0.5** (`## [0.5.0] - 2026-04-02`, `### Breaking Changes`):
  `TaskStore::save()` and `TaskStore::insert_if_absent()` take `&Task`
  instead of an owned `Task`.
- **0.3 → 0.4** (`## [0.4.0] - 2026-03-31`, `### Breaking Changes`): the
  move to the A2A v1.0.0 wire format. Clients and servers on the old wire
  format need that section in full.

Back to [Upgrading Between Minor Versions](../upgrading.md).
