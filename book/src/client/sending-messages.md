# Sending Messages

The most common operation: send a message to an agent and get a response.

## Synchronous Send

`send_message` sends a message and waits for the task to complete:

```rust
use a2a_sdk::prelude::*;

let params = MessageSendParams {
    tenant: None,
    message: Message {
        id: MessageId::new(uuid::Uuid::new_v4().to_string()),
        role: MessageRole::User,
        parts: vec![Part::text("What is the capital of France?")],
        task_id: None,
        context_id: None,
        reference_task_ids: None,
        extensions: None,
        metadata: None,
    },
    configuration: None,
    metadata: None,
};

let response = client.send_message(params).await?;
```

## Handling the Response

`SendMessageResponse` is an enum with two variants:

```rust
match response {
    SendMessageResponse::Task(task) => {
        println!("Task ID: {}", task.id);
        println!("Status: {:?}", task.status.state);

        // Extract text from artifacts
        if let Some(artifacts) = &task.artifacts {
            for artifact in artifacts {
                for part in &artifact.parts {
                    if let a2a_types::message::PartContent::Text { text } = &part.content {
                        println!("Result: {text}");
                    }
                }
            }
        }
    }
    SendMessageResponse::Message(msg) => {
        // Some agents respond with a direct message instead of a task
        println!("Direct message: {:?}", msg);
    }
    _ => {}
}
```

## Configuration

Customize the send with `SendMessageConfiguration`:

```rust
use a2a_sdk::types::params::SendMessageConfiguration;

let params = MessageSendParams {
    tenant: None,
    message: make_message("Translate to French"),
    configuration: Some(SendMessageConfiguration {
        accepted_output_modes: vec!["text/plain".into()],
        task_push_notification_config: None,
        history_length: Some(5),       // Include last 5 messages
        return_immediately: Some(false), // Wait for completion
    }),
    metadata: None,
};
```

## Continuing a Conversation

To continue a conversation, include the `context_id` from a previous task:

```rust
let first_response = client.send_message(MessageSendParams {
    message: Message {
        id: MessageId::new(uuid::Uuid::new_v4().to_string()),
        role: MessageRole::User,
        parts: vec![Part::text("Tell me about Rust")],
        context_id: None,  // New conversation
        ..Default::default()
    },
    ..Default::default()
}).await?;

// Get the context ID from the first response
let context_id = if let SendMessageResponse::Task(task) = &first_response {
    Some(task.context_id.clone())
} else {
    None
};

// Continue the conversation
let follow_up = client.send_message(MessageSendParams {
    message: Message {
        id: MessageId::new(uuid::Uuid::new_v4().to_string()),
        role: MessageRole::User,
        parts: vec![Part::text("What about error handling?")],
        context_id: context_id.map(|c| ContextId::new(c.to_string())),
        ..Default::default()
    },
    ..Default::default()
}).await?;
```

## Multi-Part Messages

Send messages with multiple content types:

```rust
let message = Message {
    id: MessageId::new(uuid::Uuid::new_v4().to_string()),
    role: MessageRole::User,
    parts: vec![
        Part::text("Analyze this image:"),
        Part::url("https://example.com/chart.png"),
        Part::data(serde_json::json!({
            "analysis_type": "detailed",
            "language": "en"
        })),
    ],
    task_id: None,
    context_id: None,
    reference_task_ids: None,
    extensions: None,
    metadata: None,
};
```

## Next Steps

- **[Streaming Responses](./streaming.md)** — Real-time event streams
- **[Task Management](./task-management.md)** — Querying and canceling tasks
- **[Error Handling](./error-handling.md)** — Handling failures gracefully
