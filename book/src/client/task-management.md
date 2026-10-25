# Task Management

Beyond sending messages, the client provides methods for querying, listing, and canceling tasks.

## Get a Task

Retrieve a task by ID:

```rust,no_run
# use a2a_protocol_sdk::prelude::*;
# use std::sync::Arc;
# use std::time::Duration;
# async fn doc() -> Result<(), Box<dyn std::error::Error>> {
# let url = "http://agent.example.com";
# let params = MessageSendParams::new(Message::user_text("m1", "hi"));
# let (params1, params2) = (params.clone(), params.clone());
# let client = ClientBuilder::new(url).build()?;
# let task_id = "task-abc";
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
# Ok(())
# }
```

## List Tasks

Query tasks with filtering and pagination:

```rust,no_run
# use a2a_protocol_sdk::prelude::*;
# use std::sync::Arc;
# use std::time::Duration;
# async fn doc() -> Result<(), Box<dyn std::error::Error>> {
# let url = "http://agent.example.com";
# let params = MessageSendParams::new(Message::user_text("m1", "hi"));
# let (params1, params2) = (params.clone(), params.clone());
# let client = ClientBuilder::new(url).build()?;
# let task_id = "task-abc";
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

// Paginate (next_page_token is empty string when no more pages)
if !response.next_page_token.is_empty() {
    let next_page = client.list_tasks(ListTasksParams {
        page_token: Some(response.next_page_token.clone()),
        ..Default::default()
    }).await?;
}
# Ok(())
# }
```

Tasks are returned **most-recently-updated first** (spec §3.1.4): the first
page holds the tasks whose state changed most recently. `page_token` is an
opaque cursor — pass it back verbatim to fetch the next page; do not parse or
construct it yourself.

### Filtering Options

| Parameter | Description |
|-----------|-------------|
| `context_id` | Tasks in a specific conversation |
| `status` | Tasks in a specific state |
| `status_timestamp_after` | Tasks whose status timestamp is at or after this ISO 8601 time (the bound is included). The client sends it as UTC with a `Z` (an offset is rewritten); a value that is not ISO 8601 fails with `ClientError::Protocol` (`InvalidParams`) before anything is sent |
| `page_size` | Results per page, 1 to 100 (a server refuses any other value with `InvalidParams` naming `pageSize`); unset lists 50. A store configured with a smaller `max_page_size` returns at most that many |
| `page_token` | Cursor for the next page |
| `include_artifacts` | Include artifact data in results |
| `history_length` | Max number of most recent messages per task (`0` = no history) |

## Cancel a Task

Request cancellation of a running task:

```rust,no_run
# use a2a_protocol_sdk::prelude::*;
# use std::sync::Arc;
# use std::time::Duration;
# async fn doc() -> Result<(), Box<dyn std::error::Error>> {
# let url = "http://agent.example.com";
# let params = MessageSendParams::new(Message::user_text("m1", "hi"));
# let (params1, params2) = (params.clone(), params.clone());
# let client = ClientBuilder::new(url).build()?;
# let task_id = "task-abc";
let task = client.cancel_task("task-abc").await?;

println!("Task state: {:?}", task.status.state);
// → Canceled (if the agent supports cancellation)
# Ok(())
# }
```

Cancellation is cooperative. The handler triggers the task's cancellation token, which a running `execute` should observe, and then calls `AgentExecutor::cancel`; the default records `Canceled`. Override `cancel` to release what the task holds or to stop downstream work; if the override writes no terminal state, the handler writes `Canceled` after it returns. A task already in a terminal state answers `TaskNotCancelable`.

### Cancellation States

| Current State | Can Cancel? |
|---------------|-------------|
| `Submitted` | Yes → `Canceled` |
| `Working` | Yes → `Canceled` |
| `InputRequired` | Yes → `Canceled` |
| `AuthRequired` | Yes → `Canceled` |
| `Completed` | No (terminal state) |
| `Failed` | No (terminal state) |
| `Canceled` | No (already canceled) |
| `Rejected` | No (terminal state) |

## Next Steps

- **[Error Handling](./error-handling.md)** — Handling API errors
- **[Streaming Responses](./streaming.md)** — Real-time event streams
