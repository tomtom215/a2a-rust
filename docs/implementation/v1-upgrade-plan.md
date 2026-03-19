# A2A v0.3.0 → v1.0.0 Upgrade Plan

> **Historical Reference** — All migrations are complete (Phase 4). This document is retained as a record of the v0.3.0 → v1.0.0 change matrices and design decisions.

## Summary of Breaking Changes

Based on the v1.0.0 release notes, the authoritative proto definition (`specification/a2a.proto`),
and the specification document, here is every change required across the three crates.

---

## Phase A: `a2a-protocol-types` (Protocol Types)

### A1. Enum serialization format — ADR-001 ProtoJSON (MASSIVE change)

**Change**: All enums switch from kebab-case/lowercase to `SCREAMING_SNAKE_CASE` with type prefix.

| Type | v0.3.0 wire value | v1.0.0 wire value |
|------|-------------------|-------------------|
| `TaskState::Submitted` | `"submitted"` | `"TASK_STATE_SUBMITTED"` |
| `TaskState::Working` | `"working"` | `"TASK_STATE_WORKING"` |
| `TaskState::InputRequired` | `"input-required"` | `"TASK_STATE_INPUT_REQUIRED"` |
| `TaskState::AuthRequired` | `"auth-required"` | `"TASK_STATE_AUTH_REQUIRED"` |
| `TaskState::Completed` | `"completed"` | `"TASK_STATE_COMPLETED"` |
| `TaskState::Failed` | `"failed"` | `"TASK_STATE_FAILED"` |
| `TaskState::Canceled` | `"canceled"` | `"TASK_STATE_CANCELED"` |
| `TaskState::Rejected` | `"rejected"` | `"TASK_STATE_REJECTED"` |
| `TaskState::Unknown` | `"unknown"` | REMOVED (replaced by `TASK_STATE_UNSPECIFIED`) |
| `MessageRole::User` | `"user"` | `"ROLE_USER"` |
| `MessageRole::Agent` | `"agent"` | `"ROLE_AGENT"` |

**Files**: `task.rs`, `message.rs`
**Approach**: Replace `#[serde(rename_all = "kebab-case")]` with explicit `#[serde(rename = "...")]` on each variant.
Add `Unspecified` variant to both enums (proto convention for 0-value).

### A2. Remove `TaskState::Unknown`, add `TaskState::Unspecified`

The `Unknown` variant is removed. The proto defines `TASK_STATE_UNSPECIFIED = 0` as the default.

### A3. Remove `TaskStatusUpdateEvent.final` field (#1308)

**Change**: The `r#final: bool` field is removed from `TaskStatusUpdateEvent`.
The event now wraps `TaskStatus` directly instead of separate `state`/`message` fields.

**v0.3.0 (current)**:
```rust
pub struct TaskStatusUpdateEvent {
    pub task_id: TaskId,
    pub context_id: ContextId,
    pub state: TaskState,
    pub message: Option<Message>,
    pub metadata: Option<Value>,
    pub r#final: bool,  // REMOVED
}
```

**v1.0.0 (target)**:
```rust
pub struct TaskStatusUpdateEvent {
    pub task_id: TaskId,       // TaskId newtype
    pub context_id: ContextId, // ContextId newtype
    pub status: TaskStatus,    // wraps state+message+timestamp
    pub metadata: Option<Value>,
}
```

**File**: `events.rs`

### A4. Part type restructure — flatten to oneof (#1411)

**Change**: `Part` becomes a flat struct with a `content` oneof instead of discriminated `kind` tag.

**v0.3.0 (current)**:
```json
{"kind": "text", "text": "hello"}
{"kind": "file", "file": {"bytes": "..."}}
{"kind": "data", "data": {...}}
```

**v1.0.0 (actual)** — internally tagged with `"type"` discriminator:
```json
{"type": "text", "text": "hello"}
{"type": "file", "file": {"name": "foo.png", "mimeType": "image/png", "bytes": "base64..."}}
{"type": "data", "data": {...}}
```

**Proto definition**:
```protobuf
message Part {
  oneof content {
    string text = 1;
    bytes raw = 2;
    string url = 3;
    google.protobuf.Value data = 4;
  }
  google.protobuf.Struct metadata = 5;
  string filename = 6;
  string media_type = 7;
}
```

This eliminates `TextPart`, `FilePart`, `DataPart`, `FileWithBytes`, `FileWithUri` as separate types.
`FileContent` is retained as a nested struct inside `PartContent::File`.
The `Part` becomes a single struct with a `PartContent` enum + common metadata field.

**Files**: `message.rs`, `artifact.rs` (uses Part)

### A5. StreamResponse — remove `kind` tag, use oneof/untagged (#1384)

**Change**: `StreamResponse` no longer uses `#[serde(tag = "kind")]`. Instead, it uses an untagged
oneof pattern where exactly one field is populated:

```json
{"task": {...}}
{"message": {...}}
{"statusUpdate": {...}}
{"artifactUpdate": {...}}
```

Note: field names are `camelCase` in JSON per ProtoJSON convention.

**File**: `events.rs`

### A6. SendMessageResponse — same untagged pattern

**Change**: `SendMessageResponse` also drops the `kind` tag:

```json
{"task": {...}}
{"message": {...}}
```

**File**: `responses.rs`

### A7. Combine TaskPushNotificationConfig and PushNotificationConfig (#1500)

**v0.3.0**: Two separate types:
- `PushNotificationConfig { id, url, token, authentication }`
- `TaskPushNotificationConfig { task_id, push_notification_config }`

**v1.0.0**: Single flat type:
```protobuf
message TaskPushNotificationConfig {
  string tenant = 1;
  string id = 2;
  string task_id = 3;
  string url = 4;
  string token = 5;
  AuthenticationInfo authentication = 6;
}
```

**File**: `push.rs`

### A8. AuthenticationInfo replaces PushNotificationAuthInfo

**v0.3.0**: `PushNotificationAuthInfo { schemes: Vec<String>, credentials: Option<String> }`
**v1.0.0**: `AuthenticationInfo { scheme: String, credentials: String }` (singular `scheme`, not `schemes`)

**File**: `push.rs`

### A9. Remove `id` from create push config request (#1487)

The create request does not include an `id` — the server assigns it.
Already partially correct in v0.3.0 (id is `Option`), but the request type should not carry it.

### A10. Pluralize `configs` in ListTaskPushNotificationConfigs response (#1486)

Response field name is `configs` (already correct in proto).
Also add `next_page_token` to the response.

### A11. Switch to non-complex IDs in requests (#1389)

**Change**: Request params use plain `String` for IDs instead of newtype wrappers.
- `GetTaskRequest { tenant, id: String, history_length }`
- `CancelTaskRequest { tenant, id: String, metadata }`
- `SubscribeToTaskRequest { tenant, id: String }`

The Task object itself still has `id: String` and `context_id: String`.

### A12. Move `extendedAgentCard` to `AgentCapabilities` (#1307)

**v0.3.0**: `AgentCard.supports_authenticated_extended_card: Option<bool>`
**v1.0.0**: `AgentCapabilities.extended_agent_card: Option<bool>`

Also rename from `supportsAuthenticatedExtendedCard` to `extendedAgentCard` (#1222).

**File**: `agent_card.rs`

### A13. Remove `state_transition_history` from `AgentCapabilities` (#1396)

This field is removed entirely.

### A14. Rename `last_updated_after` → `status_timestamp_after` fixes (#1358)

The field in `ListTasksParams` is `status_timestamp_after` with type `Timestamp` (proto).
In JSON, this should serialize as an RFC3339 string. Verify current naming matches.

### A15. OAuth2 flow changes (#1303)

- Remove `ImplicitFlow` (deprecated, but keep for backward compat with `deprecated` annotation)
- Remove `PasswordOAuthFlow` (deprecated)
- Add `DeviceCodeOAuthFlow` (already present in v0.3.0)
- Add `pkce_required: bool` to `AuthorizationCodeOAuthFlow`
- `OAuthFlows` changes from multiple optional fields to a oneof (only one flow per scheme)

**File**: `security.rs`

### A16. AgentCard structure changes

- Remove `protocol_version` field (no longer on AgentCard)
- Remove `url` field (moved to `AgentInterface`)
- Remove `preferred_transport` (replaced by `supported_interfaces`)
- Remove `additional_interfaces` (replaced by `supported_interfaces`)
- Add `supported_interfaces: Vec<AgentInterface>` (replaces url + preferred_transport + additional)
- `AgentInterface` gains `tenant` and `protocol_version` fields
- `AgentInterface.transport` renamed to `protocol_binding: String` (free-form string, not enum)

**File**: `agent_card.rs`

### A17. AgentCardSignature structure change

**v0.3.0**: Newtype around `serde_json::Value`
**v1.0.0**: Structured type with `protected: String`, `signature: String`, `header: Struct`

**File**: `extensions.rs`

### A18. Add `tenant` field to all request types (#1195)

For multi-tenancy support on gRPC, all request types gain an optional `tenant: String` field:
- `SendMessageRequest`
- `GetTaskRequest`
- `ListTasksRequest`
- `CancelTaskRequest`
- `SubscribeToTaskRequest`
- Push config CRUD requests
- `GetExtendedAgentCardRequest`

**File**: `params.rs`

### A19. Error code changes

| v0.3.0 | v1.0.0 |
|--------|--------|
| `VersionNotSupported = -32006` | `VersionNotSupportedError = -32009` |
| `ExtensionSupportRequired = -32007` | `ExtensionSupportRequiredError = -32008` |
| `InvalidMessage = -32008` | REMOVED |
| `AuthenticationFailed = -32009` | REMOVED (use HTTP 401) |
| `AuthorizationFailed = -32010` | REMOVED (use HTTP 403) |
| `UnsupportedMode = -32011` | REMOVED |
| NEW | `InvalidAgentResponseError = -32006` |
| NEW | `ExtendedAgentCardNotConfiguredError = -32007` |

**File**: `error.rs`

### A20. JSON-RPC method names changed to PascalCase

| v0.3.0 | v1.0.0 |
|--------|--------|
| `message/send` | `SendMessage` |
| `message/stream` | `SendStreamingMessage` |
| `tasks/get` | `GetTask` |
| `tasks/list` | `ListTasks` |
| `tasks/cancel` | `CancelTask` |
| `tasks/resubscribe` | `SubscribeToTask` |
| `tasks/pushNotificationConfig/set` | `CreateTaskPushNotificationConfig` |
| `tasks/pushNotificationConfig/get` | `GetTaskPushNotificationConfig` |
| `tasks/pushNotificationConfig/list` | `ListTaskPushNotificationConfigs` |
| `tasks/pushNotificationConfig/delete` | `DeleteTaskPushNotificationConfig` |
| `agent/authenticatedExtendedCard` | `GetExtendedAgentCard` |

### A21. REST route changes

| v0.3.0 | v1.0.0 |
|--------|--------|
| `POST /messages/send` | `POST /message:send` |
| `POST /messages/stream` | `POST /message:stream` |
| `GET /tasks/{id}` | `GET /tasks/{id}` (same) |
| `GET /tasks` | `GET /tasks` (same) |
| `POST /tasks/{id}/cancel` | `POST /tasks/{id}:cancel` |
| `GET /tasks/{id}/subscribe` | `POST /tasks/{id}:subscribe` |
| `POST /tasks/{taskId}/push-config` | `POST /tasks/{id}/pushNotificationConfigs` |
| `GET /tasks/{taskId}/push-config/{id}` | `GET /tasks/{id}/pushNotificationConfigs/{configId}` |
| `GET /tasks/{taskId}/push-config` | `GET /tasks/{id}/pushNotificationConfigs` |
| `DELETE /tasks/{taskId}/push-config/{id}` | `DELETE /tasks/{id}/pushNotificationConfigs/{configId}` |
| `GET /agent/authenticatedExtendedCard` | `GET /extendedAgentCard` |
| `GET /.well-known/agent-card.json` | `GET /.well-known/agent.json` (verified) |

### A22. ListTasksResponse gains `total_size` and `page_size`

**v0.3.0**: `{ tasks, next_page_token }`
**v1.0.0**: `{ tasks, next_page_token, page_size, total_size }`

### A23. CancelTaskRequest gains `metadata` field (#1485)

### A24. Add `metadata` field to `CancelTaskRequest`

---

## Phase B: `a2a-protocol-client` (HTTP Client)

### B1. Update all JSON-RPC method name strings
All method names change from `message/send` → `SendMessage`, etc.

### B2. Update REST routes
All routes change per the table above.

### B3. Update SSE parsing
`StreamResponse` uses untagged oneof instead of `kind` discriminator.

### B4. Update param/response types
Use new combined `TaskPushNotificationConfig`, new `Part` format, etc.

---

## Phase C: `a2a-protocol-server` (Server Framework)

### C1. Update JSON-RPC dispatcher method routing
All method name strings change.

### C2. Update REST dispatcher routes
All routes change.

### C3. Update SSE response builder
Stream events use untagged format.

### C4. Update handler to use new types
New `Part` format, combined push config, etc.

---

## Implementation Order

1. **a2a-protocol-types** first (all type changes)
2. **a2a-protocol-client** second (depends on types)
3. **a2a-protocol-server** third (depends on types)

### Estimated scope per crate:
- **a2a-protocol-types**: ~15 files modified, ~800 lines changed
- **a2a-protocol-client**: ~8 files modified, ~200 lines changed
- **a2a-protocol-server**: ~5 files modified, ~150 lines changed

---

## Key Design Decisions

### D1. Part serialization strategy
The v1.0 `Part` is a proto oneof. In JSON (ProtoJSON), this means exactly one of
`text`, `raw`, `url`, `data` is present. Use `#[serde(untagged)]` or flatten approach.

Recommended: Use a `PartContent` enum with `#[serde(untagged)]` and flatten common fields:
```rust
pub struct Part {
    #[serde(flatten)]
    pub content: PartContent,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filename: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub media_type: Option<String>,
}

pub enum PartContent {
    // Discriminated by field presence
}
```

### D2. StreamResponse/SendMessageResponse serialization
Use a struct with optional fields (exactly one populated) rather than an enum, since ProtoJSON
oneofs serialize as flat objects with the field name as discriminator:
```json
{"task": {...}}
{"statusUpdate": {...}}
```

Recommended: Either use `#[serde(untagged)]` on an enum, or a custom serializer.
The cleanest approach: use adjacently-tagged enum with field-name tag but no separate tag key.

### D3. Backward compatibility
Consider using `#[serde(alias = "...")]` to accept both old and new enum value formats
during a transition period. The proto spec version can be checked at runtime.
