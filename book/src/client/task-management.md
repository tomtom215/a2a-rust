# Task Management

Beyond sending messages, the client provides methods for querying, listing, and canceling tasks.

## Get a Task

Retrieve a task by ID:

```rust
use a2a_protocol_sdk::types::params::TaskQueryParams;

let task = client.get_task(TaskQueryParams {
    tenant: None,
    id: "task-abc".into(),
    history_length: Some(10),  // Include last 10 messages
}).await?;

println!("Task: {} ({:?})", task.id, task.status.state);

if let Some(artifacts) = &task.artifacts {
    println!("Artifacts: {}", artifacts.len());
}

if let Some(history) = &task.history {
    println!("Messages: {}", history.len());
}
```

## List Tasks

Query tasks with filtering and pagination:

```rust
use a2a_protocol_sdk::types::params::ListTasksParams;

let response = client.list_tasks(ListTasksParams {
    tenant: None,
    context_id: Some("ctx-123".into()),       // Filter by context
    status: Some(TaskState::Completed),         // Filter by state
    page_size: Some(20),                        // 20 per page
    page_token: None,                           // First page
    status_timestamp_after: None,
    include_artifacts: Some(true),
    history_length: None,
}).await?;

for task in &response.tasks {
    println!("{}: {:?}", task.id, task.status.state);
}

// Paginate
if let Some(token) = &response.next_page_token {
    let next_page = client.list_tasks(ListTasksParams {
        page_token: Some(token.clone()),
        ..Default::default()
    }).await?;
}
```

### Filtering Options

| Parameter | Description |
|-----------|-------------|
| `context_id` | Tasks in a specific conversation |
| `status` | Tasks in a specific state |
| `status_timestamp_after` | Tasks updated after a timestamp (ISO 8601) |
| `page_size` | Results per page (1-100, default 50; capped by server's `max_page_size`) |
| `page_token` | Cursor for the next page |
| `include_artifacts` | Include artifact data in results |
| `history_length` | Number of messages per task |

## Cancel a Task

Request cancellation of a running task:

```rust
let task = client.cancel_task("task-abc").await?;

println!("Task state: {:?}", task.status.state);
// → Canceled (if the agent supports cancellation)
```

Cancellation is cooperative — the agent's executor must implement the `cancel` method. If the agent doesn't support cancellation, you'll get an error response.

### Cancellation States

| Current State | Can Cancel? |
|---------------|-------------|
| `Submitted` | Yes → `Canceled` |
| `Working` | Yes → `Canceled` (if agent supports it) |
| `InputRequired` | Yes → `Canceled` |
| `AuthRequired` | Yes → `Canceled` |
| `Completed` | No (terminal state) |
| `Failed` | No (terminal state) |
| `Canceled` | No (already canceled) |
| `Rejected` | No (terminal state) |

## Next Steps

- **[Error Handling](./error-handling.md)** — Handling API errors
- **[Streaming Responses](./streaming.md)** — Real-time event streams
