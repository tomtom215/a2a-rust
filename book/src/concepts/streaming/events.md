<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Events on the wire

This page shows what a stream looks like on the wire: the SSE framing and the
four event types a stream can carry.

## How SSE streaming works

When a client calls `SendStreamingMessage`, the server holds the HTTP connection open and sends events as they occur:

```text
HTTP/1.1 200 OK
Content-Type: text/event-stream

data: {"statusUpdate":{"taskId":"t-1","contextId":"ctx-1","status":{"state":"TASK_STATE_WORKING"}}}

data: {"artifactUpdate":{"taskId":"t-1","contextId":"ctx-1","artifact":{"artifactId":"a-1","parts":[{"text":"partial..."}]},"lastChunk":false}}

data: {"artifactUpdate":{"taskId":"t-1","contextId":"ctx-1","artifact":{"artifactId":"a-1","parts":[{"text":"complete result"}]},"lastChunk":true}}

data: {"statusUpdate":{"taskId":"t-1","contextId":"ctx-1","status":{"state":"TASK_STATE_COMPLETED"}}}

```

Each `data:` line is a complete JSON object. Events are separated by blank lines.

Shown in the HTTP+JSON form, trimmed to the update events; over JSON-RPC each
`data:` is a JSON-RPC success response whose `result` is this object. This
repository's server also sends the task's `Task` snapshot first, and precedes
each frame with `event: message` and, for agent-emitted events, an `id:` line
(see [Resuming from where you left off](./resubscribe.md#resuming-from-where-you-left-off)).

## Stream event types

Four types of events can appear in a stream:

### StatusUpdate

Reports a task state transition:

```json
{
  "statusUpdate": {
    "taskId": "task-abc",
    "contextId": "ctx-123",
    "status": {
      "state": "TASK_STATE_WORKING",
      "timestamp": "2026-03-15T10:30:00Z"
    }
  }
}
```

### ArtifactUpdate

Delivers artifact content (potentially in chunks):

```json
{
  "artifactUpdate": {
    "taskId": "task-abc",
    "contextId": "ctx-123",
    "artifact": {
      "artifactId": "result-1",
      "parts": [{"text": "The answer is..."}]
    },
    "lastChunk": false,
    "append": false
  }
}
```

#### Append semantics

When `append: true`, the server merges the event into the existing artifact with
the same ID:

- **Parts** are appended to the existing artifact's parts list
- **Metadata** is merged key by key at the top level: new keys override existing keys
- If no artifact with the matching ID exists, a new artifact is created
- A chunk must carry at least one part, as every `Artifact` must; the queue
  refuses an empty one, so `lastChunk: true` goes on the chunk with the last part
- An append that would take the artifact past `max_parts_per_artifact`
  (10,000) is not stored in the task; a warning is logged

An update without `append: true` (or with `append: false`) for an artifact ID
the task already holds **replaces** that artifact; for a new ID it adds one. A
new artifact past `max_artifacts_per_task` (1,000) is not stored either.

### Task

A complete task snapshot: the first event of a task stream (and of a
`SubscribeToTask` stream). A server also sends one after the terminal state, as
a final event, only when `HandlerLimits::with_final_task_snapshot(true)` is set
(off by default, spec §11.7):

```json
{
  "task": {
    "id": "task-abc",
    "contextId": "ctx-123",
    "status": {"state": "TASK_STATE_COMPLETED"},
    "artifacts": [...]
  }
}
```

### Message

A direct message response (for simple request/reply patterns):

```json
{
  "message": {
    "messageId": "msg-456",
    "role": "ROLE_AGENT",
    "parts": [{"text": "Quick answer"}]
  }
}
```

A `Message` is the whole stream only when it is the executor's first event
for a new task (spec §3.1.2: such a stream holds exactly one `Message`, with
"no task tracking"). A follow-up to an existing task has no direct reply:
every `Message` its executor writes, the first included, is sent as below,
so the follow-up's stream begins with the `Task` (§3.7: agents "attach
Messages to status update events"). If the executor answers a follow-up
with messages alone and sets no state, the task goes back to waiting where
it waited (`INPUT_REQUIRED` or `AUTH_REQUIRED`), with the last message as
its status message. A `Message` the
executor writes once the task stream has begun, after a status update, an
artifact or a task snapshot, is sent as a status update that keeps the
task's state and carries the message in `status.message`: a task stream
holds status and artifact updates only. The task store appends it to the
task's `history` as before and, while the task is not finished, holds it as
the status message, as a client applying the update does: written while the
task waits in `INPUT_REQUIRED` or `AUTH_REQUIRED`, it replaces the status
message that asked for the input. Before any status the state is the one
the turn began in, `SUBMITTED`. A `Message` written after the task's terminal
status is stored in `history` and reaches no stream, as every stream has
ended.
