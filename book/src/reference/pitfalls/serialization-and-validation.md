<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Serialization and validation

Part of [Pitfalls & Lessons Learned](../pitfalls.md).

## Serde Pitfalls

### Untagged enums hide inner errors

`#[serde(untagged)]` swallows the real deserialization error and replaces it with a generic "data did not match any variant" message. `JsonRpcResponse<T>` hit this until 0.7; its `Deserialize` is now hand-written — it enforces exactly one of `result` / `error` (JSON-RPC 2.0 §5) and surfaces a mistyped `result`'s own error — and only its `Serialize` stays untagged. (`SendMessageResponse` and `StreamResponse` are externally tagged — a one-key object whose camelCase key names the variant — and their `Deserialize` is hand-written (`oneof_deserialize!`), so they name the problem too.)

**Workaround:** for an untagged enum of your own, hand-write `Deserialize`, or log the raw JSON before attempting deserialization.

### `#[serde(default)]` vs `Option<T>`

Using `#[serde(default)]` on a `Vec<T>` field means the field is present but empty when omitted from JSON. Using `Option<Vec<T>>` means it is absent (`None`).

The A2A spec distinguishes between "omitted" and "empty array" for fields like `history` and `artifacts`, so `Option<Vec<T>>` is the correct choice.

```rust
# use a2a_protocol_sdk::types::message::Message;
# #[derive(serde::Serialize, serde::Deserialize)]
# struct Wrong {
// Wrong: field is always present (empty vec if omitted)
#[serde(default)]
pub history: Vec<Message>,
# }
# #[derive(serde::Serialize, serde::Deserialize)]
# struct Correct {

// Correct: field is absent when not provided
#[serde(skip_serializing_if = "Option::is_none")]
pub history: Option<Vec<Message>>,
# }
# assert_eq!(serde_json::to_string(&Wrong { history: vec![] }).unwrap(), r#"{"history":[]}"#);
# assert_eq!(serde_json::to_string(&Correct { history: None }).unwrap(), "{}");
```

### `#[non_exhaustive]` on enums breaks downstream matches

Adding `#[non_exhaustive]` to `TaskState`, `ErrorCode`, etc. forces downstream crates to include a wildcard arm. This is intentional for forward compatibility:

```rust
# use a2a_protocol_sdk::prelude::*;
# fn f(task: Task) {
match task.status.state {
    TaskState::Completed => { /* ... */ }
    TaskState::Failed => { /* ... */ }
    _ => { /* Handle future states */ }
}
# }
```

## Validation Pitfalls

### Empty/whitespace-only IDs must be rejected

`TaskId` and `ContextId` should not accept empty or whitespace-only strings. Build them with `TaskId::try_new` / `ContextId::try_new`, which reject such values; `new`, `From<String>` and `From<&str>` accept them for backward compatibility.

### A file part needs a source

v1.0 carries a file as `Part::raw(bytes)` or `Part::url(uri)`. The v0.3 `FileContent` type is deprecated, and `Part::file(FileContent)` turns one with neither `bytes` nor `uri` into an empty raw part instead of refusing it, so call `FileContent::validate()` first if you still use it.

### Push notification URLs need validation

`TaskPushNotificationConfig` URLs should be validated for correct format. A `validate()` method on the config struct catches malformed URLs before they reach the push sender.

### Timestamps should be RFC 3339

`TaskStatus` timestamps should conform to RFC 3339. `has_valid_timestamp()` is only a coarse check (at least 19 characters and a `T` separator); `parse_iso8601_to_unix_millis(ts).is_some()` is the real test, and is what the server applies to `statusTimestampAfter`.

### Artifact parts must not be empty

`Artifact` must contain at least one `Part` per the A2A spec, and an
appended chunk (`append: true`) is an `Artifact` too: a2a.proto makes no
exception for it. Call `artifact.validate()` before emitting artifacts from
your executor. The event queue refuses an artifact update with no parts
(`InvalidParams` to the executor's `write`), and event processing drops one
that reaches it another way. To mark the end of a streamed artifact, set
`lastChunk: true` on the chunk that carries its last part, rather than
sending an empty chunk after it; until 0.15 an empty appended chunk was
accepted.

### Artifact append must merge metadata

When `TaskArtifactUpdateEvent` has `append=true`, metadata from the new event
must be merged into the existing artifact's metadata (new keys override
existing). Simply pushing a new artifact entry loses the append semantics.

### TaskListResponse fields are always present

`next_page_token`, `page_size`, and `total_size` are required per the proto
spec (`ListTasksResponse`) — the SDK's `TaskListResponse` holds them as
`String` and `u32`, not `Option`, and a reader of a peer's response treats an
absent one as empty/zero. An empty `next_page_token` means no more pages.
Custom `TaskStore` implementations must populate all three.

### SendMessage to terminal tasks must be rejected

A `SendMessage` targeting a task in a terminal state (`Completed`, `Failed`,
`Canceled`, `Rejected`) via explicit `taskId` must return `UnsupportedOperation`,
not silently create a new task. The server enforces this at the handler level.

### `CachingCardResolver` must not silently produce empty URLs

If `CachingCardResolver::new()` or `with_path()` receives an invalid base URL, silently producing an empty URL leads to confusing errors later. These constructors should return `ClientResult<Self>` so callers can handle the error immediately.
