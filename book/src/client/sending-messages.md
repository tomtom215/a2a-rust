# Sending Messages

The most common operation: send a message to an agent and get a response.

## Synchronous Send

`send_message` sends a message and waits for the task to complete:

```rust,ignore
use a2a_protocol_sdk::prelude::*;

let params = MessageSendParams::new(Message::user_text(
    uuid::Uuid::new_v4().to_string(),
    "What is the capital of France?",
));

let response = client.send_message(params).await?;
```

## Handling the Response

`SendMessageResponse` is an enum with two variants:

```rust,ignore
match response {
    SendMessageResponse::Task(task) => {
        println!("Task ID: {}", task.id);
        println!("Status: {:?}", task.status.state);

        // `task.text()` is the first text across every artifact, and
        // `task.texts()` is all of them. Walk the parts yourself only when
        // you need the non-text ones too, as below.
        println!("Result: {:?}", task.text());

        if let Some(artifacts) = &task.artifacts {
            for artifact in artifacts {
                for part in &artifact.parts {
                    if let a2a_protocol_types::message::PartContent::Text(text) = &part.content {
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
}
```

## Configuration

Customize the send with `SendMessageConfiguration`:

```rust,ignore
use a2a_protocol_sdk::types::params::SendMessageConfiguration;

let params = MessageSendParams::new(make_message("Translate to French")).with_configuration(
    SendMessageConfiguration {
        accepted_output_modes: vec!["text/plain".into()],
        task_push_notification_config: None,
        history_length: Some(5),       // Include last 5 messages
        return_immediately: Some(false), // Wait for completion
    },
);
```

## Continuing a Conversation

To continue a conversation, include the `context_id` from a previous task:

```rust,ignore
// No context id on the message: this starts a new conversation.
let first_response = client
    .send_message(MessageSendParams::new(Message::user_text(
        uuid::Uuid::new_v4().to_string(),
        "Tell me about Rust",
    )))
    .await?;

// Get the context ID from the first response
let context_id = if let SendMessageResponse::Task(task) = &first_response {
    Some(task.context_id.clone())
} else {
    None
};

// Continue the conversation — put the context id on the Message itself.
// `Message::with_context_id` takes a `ContextId`; here it arrives as an
// `Option`, so the public field is assigned directly.
let mut message = Message::user_text(
    uuid::Uuid::new_v4().to_string(),
    "What about error handling?",
);
message.context_id = context_id.clone();

let follow_up = client
    .send_message(MessageSendParams::new(message))
    .await?;
```

## Error Conditions

`SendMessage` returns specific errors for invalid requests:

| Condition | Error |
|-----------|-------|
| Task in terminal state (completed, failed, etc.) | `UnsupportedOperation` |
| Client-provided `taskId` doesn't exist | `TaskNotFound` |
| `taskId`/`contextId` mismatch | `InvalidParams` |
| Empty message parts | `InvalidParams` |

## Multi-Part Messages

Send messages with multiple content types:

```rust,ignore
let message = Message::user(
    uuid::Uuid::new_v4().to_string(),
    vec![
        Part::text("Analyze this image:"),
        Part::url("https://example.com/chart.png")
            .with_media_type("image/png"),
        Part::data(serde_json::json!({
            "analysis_type": "detailed",
            "language": "en"
        })),
    ],
);
```

## Next Steps

- **[Streaming Responses](./streaming.md)** — Real-time event streams
- **[Task Management](./task-management.md)** — Querying and canceling tasks
- **[Error Handling](./error-handling.md)** — Handling failures gracefully
