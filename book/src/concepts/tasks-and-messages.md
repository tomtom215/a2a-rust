# Tasks & Messages

Tasks and messages are the core data model of the A2A protocol. Understanding their structure and lifecycle is essential for building agents.

## Tasks

A **Task** represents a unit of work. A `SendMessage` that names no task starts one; one that names a live task continues it; and an agent may answer with a direct `Message` instead of returning a task.

### Task Structure

```rust
# use a2a_protocol_sdk::types::task::{ContextId, TaskId, TaskStatus, TaskState};
# use a2a_protocol_sdk::types::message::{Message, MessageId, MessageRole, Part};
# use a2a_protocol_sdk::types::artifact::{Artifact, ArtifactId};
pub struct Task {
    pub id: TaskId,                          // Server-assigned unique ID
    pub context_id: ContextId,               // Conversation thread ID
    pub status: TaskStatus,                  // Current state + optional message
    pub history: Option<Vec<Message>>,       // The task's messages; omitted from a send response by default
    pub artifacts: Option<Vec<Artifact>>,    // Produced results
    pub metadata: Option<serde_json::Value>, // Arbitrary key-value data
}
# // The real type has exactly these fields: this literal of it names them all.
# let _ = a2a_protocol_sdk::types::task::Task {
#     id: TaskId::new("t"), context_id: ContextId::new("c"),
#     status: TaskStatus::new(TaskState::Working), history: None, artifacts: None, metadata: None,
# };
```

A `SendMessage` response omits `history` unless the request's
`configuration.historyLength` asks for it (`n` keeps the newest `n` messages;
`0` omits). `GetTask` returns the stored, size-capped history unless
`historyLength` trims it.

### Task States

Tasks follow a state machine with validated transitions; the proto's default,
`Unspecified`, is not a state a task is ever in:

| State | Meaning | Terminal? |
|-------|---------|-----------|
| `Unspecified` | The proto default (`TASK_STATE_UNSPECIFIED`): no real state. Never send it as a status: a server refuses the event and a client refuses a task carrying it | No |
| `Submitted` | Received, not yet started | No |
| `Working` | Actively processing | No |
| `InputRequired` | Needs more input from client | No |
| `AuthRequired` | Needs authentication | No |
| `Completed` | Finished successfully | Yes |
| `Failed` | Finished with error | Yes |
| `Canceled` | Canceled before completion (by `CancelTask`) | Yes |
| `Rejected` | The agent declined the task, at creation or later | Yes |

### Valid Transitions

Not all state transitions are allowed. `TaskState::can_transition_to` encodes
two rules, and the server applies them to every status update and task snapshot
an executor writes:

1. Terminal states (`Completed`, `Failed`, `Canceled`, `Rejected`) are final,
   including to themselves.
2. Nothing transitions into `Submitted` or `Unspecified`.

Everything else is allowed, `Submitted → Completed` and `Working → Working`
included. An event that breaks a rule fails a blocking send with
`InvalidStateTransition`; on the streaming and `returnImmediately` path the
task is marked `Failed`:

```rust,no_run
use a2a_protocol_sdk::prelude::TaskState;

// Check if a transition is valid
assert!(TaskState::Submitted.can_transition_to(TaskState::Working));
assert!(TaskState::Working.can_transition_to(TaskState::Completed));

// Terminal states cannot transition
assert!(!TaskState::Completed.can_transition_to(TaskState::Working));
assert!(!TaskState::Failed.can_transition_to(TaskState::Working));

// Check if a state is terminal
assert!(TaskState::Completed.is_terminal());
assert!(!TaskState::Working.is_terminal());
```

### Terminal State Constraints

Tasks in terminal states enforce strict invariants:

- **No new messages** — `SendMessage` to a terminal task returns
  `UnsupportedOperation`. Start a new task instead.
- **No subscription** — `SubscribeToTask` on a terminal task returns
  `UnsupportedOperation` since no new events will be emitted.
- **No cancellation** — `CancelTask` on a terminal task returns
  `TaskNotCancelable`.
- **Unknown taskId** — `SendMessage` with a `taskId` that doesn't reference an
  existing task returns `TaskNotFound`.

Two more rules apply to a `SendMessage` that names a task, whatever its state:

- **One run at a time** — a `SendMessage` naming a task whose executor is still
  running returns `UnsupportedOperation`; wait for `INPUT_REQUIRED`,
  `AUTH_REQUIRED` or a terminal state.
- **Context must match** — a message whose `contextId` differs from the named
  task's returns `InvalidParams`.

### Task Status

The status combines a state with an optional message and timestamp:

```rust,no_run
use a2a_protocol_sdk::prelude::{TaskStatus, TaskState};

// Without timestamp
let status = TaskStatus::new(TaskState::Working);

// With automatic UTC timestamp
let status = TaskStatus::with_timestamp(TaskState::Completed);
```

### Wire Format

On the wire, task states use SCREAMING_SNAKE_CASE with a `TASK_STATE_` prefix:

```json
{
  "id": "task-abc",
  "contextId": "ctx-123",
  "status": {
    "state": "TASK_STATE_COMPLETED",
    "timestamp": "2026-03-15T10:30:00Z"
  },
  "artifacts": [...]
}
```

## Messages

A **Message** is a structured payload exchanged between client and agent:

```rust
# use a2a_protocol_sdk::types::task::{ContextId, TaskId, TaskStatus, TaskState};
# use a2a_protocol_sdk::types::message::{MessageId, MessageRole, Part};
# use a2a_protocol_sdk::types::artifact::{Artifact, ArtifactId};
pub struct Message {
    pub id: MessageId,                           // Unique message ID (wire name: messageId)
    pub role: MessageRole,                       // User or Agent
    pub parts: Vec<Part>,                        // Content (≥1 part)
    pub task_id: Option<TaskId>,                 // Associated task
    pub context_id: Option<ContextId>,           // Conversation thread
    pub reference_task_ids: Option<Vec<TaskId>>, // Related tasks
    pub extensions: Option<Vec<String>>,         // Extension URIs
    pub metadata: Option<serde_json::Value>,
}
# let _ = a2a_protocol_sdk::types::message::Message {
#     id: MessageId::new("m"), role: MessageRole::User, parts: vec![], task_id: None,
#     context_id: None, reference_task_ids: None, extensions: None, metadata: None,
# };
```

### Roles

| Role | Wire Value | Meaning |
|------|------------|---------|
| `User` | `"ROLE_USER"` | From the client/human side |
| `Agent` | `"ROLE_AGENT"` | From the agent/server side |
| `Unspecified` | `"ROLE_UNSPECIFIED"` | The proto default; a server refuses a message that carries it (`InvalidParams`) |

### Creating Messages

```rust,no_run
use a2a_protocol_sdk::prelude::*;

let message = Message {
    id: MessageId::new(uuid::Uuid::new_v4().to_string()),
    role: MessageRole::User,
    parts: vec![Part::text("What is 2 + 2?")],
    task_id: None,
    context_id: None,
    reference_task_ids: None,
    extensions: None,
    metadata: None,
};
```

## Parts

Parts are the content units within messages and artifacts. Four types are supported:

### Text

```rust
# use a2a_protocol_sdk::prelude::*;
let part = Part::text("Hello, agent!");
# assert_eq!(serde_json::to_value(&part).unwrap(), serde_json::json!({"text": "Hello, agent!"}));
```

Wire format: `{"text": "Hello, agent!"}`

### Raw (inline bytes)

```rust
# use a2a_protocol_sdk::prelude::*;
# let base64_encoded_string = "aGVsbG8=";
// Inline bytes (base64-encoded)
let part = Part::raw(base64_encoded_string);
```

Wire format: `{"raw": "aGVsbG8=", "filename": "doc.bin", "mediaType": "application/octet-stream"}`

`Part::raw` does not check its argument. A server refuses a `raw` that is not
base64 with `InvalidParams` naming the field; URL-safe and unpadded base64 are
accepted and returned in standard padded form. The same check applies to the
events an executor writes.

### Url (URI reference)

```rust
# use a2a_protocol_sdk::prelude::*;
// URI reference
let part = Part::url("https://example.com/document.pdf");
# assert_eq!(serde_json::to_value(&part).unwrap(), serde_json::json!({"url": "https://example.com/document.pdf"}));
```

Wire format: `{"url": "https://example.com/document.pdf"}`

### Structured Data

```rust
# use a2a_protocol_sdk::prelude::*;
let part = Part::data(serde_json::json!({
    "table": [
        {"name": "Alice", "score": 95},
        {"name": "Bob", "score": 87}
    ]
}));
# assert!(serde_json::to_value(&part).unwrap()["data"]["table"].is_array());
```

Wire format: `{"data": {"table": [...]}}`

Protobuf carries these numbers as doubles, so a server refuses an integer
beyond ±2^53 in a `data` part or in `metadata` with `InvalidParams`, in a
client's message and in an executor's events alike.

### Part Metadata

Any part can carry optional metadata:

```json
{
  "text": "Hello",
  "metadata": {"language": "en"}
}
```

## Artifacts

Artifacts are results produced by an agent, delivered as part of a task:

```rust
# use a2a_protocol_sdk::types::task::{ContextId, TaskId, TaskStatus, TaskState};
# use a2a_protocol_sdk::types::message::{Message, MessageId, MessageRole, Part};
# use a2a_protocol_sdk::types::artifact::ArtifactId;
pub struct Artifact {
    pub id: ArtifactId,                      // wire name: artifactId
    pub name: Option<String>,
    pub description: Option<String>,
    pub parts: Vec<Part>,                    // ≥1 part
    pub extensions: Option<Vec<String>>,
    pub metadata: Option<serde_json::Value>,
}
# let _ = a2a_protocol_sdk::types::artifact::Artifact {
#     id: ArtifactId::new("a"), name: None, description: None, parts: vec![],
#     extensions: None, metadata: None,
# };
```

Create and validate an artifact:

```rust,no_run
use a2a_protocol_sdk::prelude::*;

let artifact = Artifact::new(
    "result-1",
    vec![Part::text("The answer is 42")],
);
// `Artifact::new` panics in debug builds on empty parts; `validate()` is the
// fallible check for an artifact built another way (a struct literal, or one
// you deserialized). A server refuses an artifact with no parts.
artifact.validate().expect("artifact should be valid");
```

### Streaming Artifacts

Artifacts can be delivered incrementally during streaming:

```rust,no_run
# use a2a_protocol_sdk::prelude::*;
# async fn f(ctx: &RequestContext, queue: &dyn EventQueueWriter) -> A2aResult<()> {
// First chunk
queue.write(StreamResponse::ArtifactUpdate(TaskArtifactUpdateEvent {
    task_id: ctx.task_id.clone(),
    context_id: ContextId::new(ctx.context_id.clone()),
    artifact: Artifact::new("doc", vec![Part::text("First paragraph...")]),
    append: None,
    last_chunk: Some(false),  // More chunks coming
    metadata: None,
})).await?;

// Final chunk — parts are appended, metadata is merged key by key
queue.write(StreamResponse::ArtifactUpdate(TaskArtifactUpdateEvent {
    task_id: ctx.task_id.clone(),
    context_id: ContextId::new(ctx.context_id.clone()),
    artifact: Artifact::new("doc", vec![Part::text("Last paragraph.")]),
    append: Some(true),       // Append parts to existing artifact by ID
    last_chunk: Some(true),   // This is the last chunk
    metadata: None,
})).await?;
# Ok(())
# }
```

When `append=true`, the server finds the existing artifact by ID and:
1. Appends the new parts to the existing parts list
2. Merges metadata key by key at the top level (new keys override existing keys)

If no artifact with the ID exists, it is created as a new artifact.

A chunk without `append` (or with `append: false`) for an ID the task already
holds **replaces** that artifact; it does not add a second one, so every chunk
after the first needs `append: Some(true)`. An append that would take the
artifact past `max_parts_per_artifact` (a `HandlerLimits` field, default
10,000) is dropped.

## ID Types

a2a-rust uses newtype wrappers for type safety:

| Type | Wraps | Example |
|------|-------|---------|
| `TaskId` | `String` | `TaskId::new("task-abc")` |
| `ContextId` | `String` | `ContextId::new("ctx-123")` |
| `MessageId` | `String` | `MessageId::new("msg-456")` |
| `ArtifactId` | `String` | Constructed inside `Artifact::new` |

These prevent accidentally passing a task ID where a context ID is expected.

## Next Steps

- **[Streaming with SSE](./streaming.md)** — Real-time event delivery
- **[The AgentExecutor Trait](../building-agents/executor.md)** — Using tasks and messages in your agent
