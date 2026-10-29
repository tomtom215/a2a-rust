<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Streaming Across Replicas

An agent's event queue lives in the process running its executor. A client
that subscribes on a different replica from the one running the task gets the
task's events from the store's event log instead, which the replicas share;
both SQL stores keep one. That client:

- **does** get a stream that terminates correctly when the task reaches a
  terminal state, with that state reported — spec §3.1.6 (`STREAM-SUB-002`) is
  satisfied across replicas;
- **does** see the artifact and status frames produced along the way, in the
  order the agent emitted them and at the positions the running replica's own
  subscribers see (spec §3.5.2). They arrive in batches: the subscription's
  reattach hook looks for new events every `subscribe_reattach_interval`
  (250 ms by default), so a frame can be up to one interval late.

The stream starts from a snapshot of the task and replays the log after the
position that snapshot reflects. Every shipped store returns the task together
with that position, in one read (`TaskStore::get_with_position`), so an event
the running replica logs and stores while the subscription starts arrives once;
before 0.15 it could arrive twice, and a client folding an `append: true`
artifact chunk would have folded it twice. Three edges remain. A replica still
on an earlier release does not write that position, so during a rolling
upgrade a subscriber can see an event its snapshot already holds a second time.
A custom store that does not return the position falls back to reading the
log's end and then the task, which can lose the one event another replica has
logged and not yet stored, or repeat one it logged and stored between the two
reads
([The log position a stored task reflects](../../building-agents/stores/replicas.md#the-log-position-a-stored-task-reflects)).
And a store without an event log has nothing to replay: its subscribers get
the ending and not the middle.

Where a poll's latency matters, or the store keeps no log, three ways to live
with it, in the order most deployments should consider them:

1. **Session affinity.** Have the load balancer route by task ID or by
   connection. A subscriber on the replica running the task reads its queue
   directly, with no poll in between.
2. **Poll instead of stream.** `GetTask` is replica-independent, because the
   store is. A client that polls is unaffected by any of this.
3. **Push notifications.** Configure a webhook; delivery is driven from the
   replica running the task and does not depend on where the client is.
