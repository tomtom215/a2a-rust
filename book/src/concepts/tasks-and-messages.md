# Tasks & Messages

Tasks and messages are the core data model of the A2A protocol. Understanding their structure and lifecycle is essential for building agents.

## Tasks

A **Task** represents a unit of work. Every `SendMessage` call creates one.

### Task Structure

```rust
pub struct Task {
    pub id: TaskId,                          // Server-assigned unique ID
    pub context_id: ContextId,               // Conversation thread ID
    pub status: TaskStatus,                  // Current state + optional message
    pub history: Option<Vec<Message>>,       // Previous messages in context
    pub artifacts: Option<Vec<Artifact>>,    // Produced results
    pub metadata: Option<serde_json::Value>, // Arbitrary key-value data
}
```

### Task States

Tasks follow a state machine with validated transitions:

| State | Meaning | Terminal? |
|-------|---------|-----------|
| `Submitted` | Received, not yet started | No |
| `Working` | Actively processing | No |
| `InputRequired` | Needs more input from client | No |
| `AuthRequired` | Needs authentication | No |
| `Completed` | Finished successfully | Yes |
| `Failed` | Finished with error | Yes |
| `Canceled` | Canceled by client | Yes |
| `Rejected` | Rejected before execution | Yes |

### Valid Transitions

Not all state transitions are allowed. The library enforces these rules:

```rust
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

### Task Status

The status combines a state with an optional message and timestamp:

```rust
use a2a_protocol_sdk::prelude::{TaskStatus, TaskState};

// Without timestamp
let status = TaskStatus::new(TaskState::Working);

// With automatic UTC timestamp
let status = TaskStatus::with_timestamp(TaskState::Completed);
```

### Wire Format

On the wire, task states use the `TASK_STATE_` prefix:

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
pub struct Message {
    pub id: MessageId,                           // Unique message ID
    pub role: MessageRole,                       // User or Agent
    pub parts: Vec<Part>,                        // Content (≥1 part)
    pub task_id: Option<TaskId>,                 // Associated task
    pub context_id: Option<ContextId>,           // Conversation thread
    pub reference_task_ids: Option<Vec<TaskId>>, // Related tasks
    pub extensions: Option<Vec<String>>,         // Extension URIs
    pub metadata: Option<serde_json::Value>,
}
```

### Roles

| Role | Wire Value | Meaning |
|------|------------|---------|
| `User` | `ROLE_USER` | From the client/human side |
| `Agent` | `ROLE_AGENT` | From the agent/server side |

### Creating Messages

```rust
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
let part = Part::text("Hello, agent!");
```

Wire format: `{"text": "Hello, agent!"}`

### Raw (Base64)

```rust
let part = Part::raw(base64_encoded_string);
```

Wire format: `{"raw": "aGVsbG8="}`

### URL Reference

```rust
let part = Part::url("https://example.com/document.pdf");
```

Wire format: `{"url": "https://example.com/document.pdf"}`

### Structured Data

```rust
let part = Part::data(serde_json::json!({
    "table": [
        {"name": "Alice", "score": 95},
        {"name": "Bob", "score": 87}
    ]
}));
```

Wire format: `{"data": {"table": [...]}}`

### Part Metadata

Any part can carry optional metadata and MIME type information:

```json
{
  "text": "Hello",
  "mediaType": "text/plain",
  "metadata": {"language": "en"}
}
```

## Artifacts

Artifacts are results produced by an agent, delivered as part of a task:

```rust
pub struct Artifact {
    pub id: ArtifactId,
    pub name: Option<String>,
    pub description: Option<String>,
    pub parts: Vec<Part>,                    // ≥1 part
    pub extensions: Option<Vec<String>>,
    pub metadata: Option<serde_json::Value>,
}
```

Create an artifact:

```rust
use a2a_protocol_sdk::prelude::*;

let artifact = Artifact::new(
    "result-1",
    vec![Part::text("The answer is 42")],
);
```

### Streaming Artifacts

Artifacts can be delivered incrementally during streaming:

```rust
// First chunk
queue.write(StreamResponse::ArtifactUpdate(TaskArtifactUpdateEvent {
    task_id: ctx.task_id.clone(),
    context_id: ContextId::new(ctx.context_id.clone()),
    artifact: Artifact::new("doc", vec![Part::text("First paragraph...")]),
    append: None,
    last_chunk: Some(false),  // More chunks coming
    metadata: None,
})).await?;

// Final chunk
queue.write(StreamResponse::ArtifactUpdate(TaskArtifactUpdateEvent {
    task_id: ctx.task_id.clone(),
    context_id: ContextId::new(ctx.context_id.clone()),
    artifact: Artifact::new("doc", vec![Part::text("Last paragraph.")]),
    append: Some(true),       // Append to previous
    last_chunk: Some(true),   // This is the last chunk
    metadata: None,
})).await?;
```

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
