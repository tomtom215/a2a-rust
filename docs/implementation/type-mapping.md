# A2A Protocol → Rust Type Mapping

Complete field-by-field mapping from A2A v1.0.0 JSON schema to Rust types.
All structs use `#[serde(rename_all = "camelCase")]` unless noted.
All `Option<T>` fields use `#[serde(skip_serializing_if = "Option::is_none")]`.

---

## Primitive / Alias Types

| Spec type | Rust type | Notes |
|---|---|---|
| `TaskID` (string) | `struct TaskId(String)` | Newtype; `Display`, `From<String>`, `AsRef<str>` |
| `ArtifactID` (string) | `struct ArtifactId(String)` | Newtype |
| `MessageID` (string) | `struct MessageId(String)` | Newtype |
| `ContextID` (string) | `struct ContextId(String)` | Newtype |
| `TaskVersion` (u64) | `struct TaskVersion(u64)` | Newtype; monotonic counter for optimistic concurrency |
| `Metadata` (any JSON) | `serde_json::Value` | Untyped per spec |
| Protocol version string | `struct ProtocolVersion(String)` | Validates `"0.3.0"` format |

---

## `task.rs`

### `TaskState`

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TaskState {
    Submitted,      // "submitted"
    Working,        // "working"
    InputRequired,  // "input-required"
    AuthRequired,   // "auth-required"
    Completed,      // "completed"
    Failed,         // "failed"
    Canceled,       // "canceled"
    Rejected,       // "rejected"
    Unknown,        // "unknown"
}
```

Terminal states: `Completed | Failed | Canceled | Rejected`.

### `TaskStatus`

| Spec field | Rust field | Type | Required |
|---|---|---|---|
| `state` | `state` | `TaskState` | Yes |
| `message` | `message` | `Option<Message>` | No |
| `timestamp` | `timestamp` | `Option<String>` | No (ISO 8601) |

### `Task`

| Spec field | Rust field | Type | Required |
|---|---|---|---|
| `id` | `id` | `TaskId` | Yes |
| `contextId` | `context_id` | `ContextId` | Yes |
| `status` | `status` | `TaskStatus` | Yes |
| `history` | `history` | `Option<Vec<Message>>` | No |
| `artifacts` | `artifacts` | `Option<Vec<Artifact>>` | No |
| `metadata` | `metadata` | `Option<serde_json::Value>` | No |
| `kind` | `kind` | `TaskKind` | Yes (always `"task"`) |

```rust
// Phantom discriminator — always serializes as "task"
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TaskKind;
impl TaskKind {
    const VALUE: &'static str = "task";
}
// OR simpler:
#[serde(rename = "kind")]
pub kind: &'static str,  // hardcoded "task"
```

---

## `message.rs`

### `MessageRole`

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MessageRole {
    User,   // "user"
    Agent,  // "agent"
}
```

### `Message`

| Spec field | Rust field | Type | Required |
|---|---|---|---|
| `messageId` | `id` | `MessageId` | Yes |
| `taskId` | `task_id` | `Option<TaskId>` | No |
| `contextId` | `context_id` | `Option<ContextId>` | No |
| `role` | `role` | `MessageRole` | Yes |
| `parts` | `parts` | `Vec<Part>` | Yes (non-empty) |
| `referenceTaskIds` | `reference_task_ids` | `Option<Vec<TaskId>>` | No |
| `extensions` | `extensions` | `Option<Vec<String>>` | No (URIs) |
| `metadata` | `metadata` | `Option<serde_json::Value>` | No |
| `kind` | `kind` | `MessageKind` | Yes (always `"message"`) |

### `Part` (discriminated union, tag = `"kind"`)

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Part {
    Text(TextPart),
    File(FilePart),
    Data(DataPart),
}
```

### `TextPart`

| Spec field | Rust field | Type | Required |
|---|---|---|---|
| `text` | `text` | `String` | Yes |
| `metadata` | `metadata` | `Option<serde_json::Value>` | No |

### `FilePart`

| Spec field | Rust field | Type | Required |
|---|---|---|---|
| `file` | `file` | `FileContent` | Yes |
| `metadata` | `metadata` | `Option<serde_json::Value>` | No |

### `FileContent` (untagged union — presence of `bytes` vs `uri` field)

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum FileContent {
    Bytes(FileWithBytes),  // deserializes when `bytes` key present
    Uri(FileWithUri),      // deserializes when `uri` key present
}
```

### `FileWithBytes`

| Spec field | Rust field | Type | Required |
|---|---|---|---|
| `name` | `name` | `Option<String>` | No |
| `mimeType` | `mime_type` | `Option<String>` | No |
| `bytes` | `bytes` | `String` | Yes (base64-encoded) |

Note: base64 encode/decode implemented in-tree in `a2a-protocol-types/src/base64.rs` (~40 lines). No `base64` crate dep.

### `FileWithUri`

| Spec field | Rust field | Type | Required |
|---|---|---|---|
| `name` | `name` | `Option<String>` | No |
| `mimeType` | `mime_type` | `Option<String>` | No |
| `uri` | `uri` | `String` | Yes (absolute URL preferred) |

### `DataPart`

| Spec field | Rust field | Type | Required |
|---|---|---|---|
| `data` | `data` | `serde_json::Value` | Yes (object) |
| `metadata` | `metadata` | `Option<serde_json::Value>` | No |

---

## `artifact.rs`

### `Artifact`

| Spec field | Rust field | Type | Required |
|---|---|---|---|
| `artifactId` | `id` | `ArtifactId` | Yes |
| `name` | `name` | `Option<String>` | No |
| `description` | `description` | `Option<String>` | No |
| `parts` | `parts` | `Vec<Part>` | Yes (non-empty) |
| `extensions` | `extensions` | `Option<Vec<String>>` | No (URIs) |
| `metadata` | `metadata` | `Option<serde_json::Value>` | No |

---

## `events.rs`

### `TaskStatusUpdateEvent`

| Spec field | Rust field | Type | Required |
|---|---|---|---|
| `taskId` | `task_id` | `TaskId` | Yes |
| `contextId` | `context_id` | `ContextId` | Yes |
| `state` | `state` | `TaskState` | Yes |
| `message` | `message` | `Option<Message>` | No |
| `metadata` | `metadata` | `Option<serde_json::Value>` | No |
| `kind` | `kind` | `"status-update"` | Yes |
| `final` | `r#final` | `bool` | Yes |

Note: `final` is a Rust keyword; use raw identifier `r#final`. In serde, use `#[serde(rename = "final")]`.

### `TaskArtifactUpdateEvent`

| Spec field | Rust field | Type | Required |
|---|---|---|---|
| `taskId` | `task_id` | `TaskId` | Yes |
| `contextId` | `context_id` | `ContextId` | Yes |
| `artifact` | `artifact` | `Artifact` | Yes |
| `append` | `append` | `Option<bool>` | No |
| `lastChunk` | `last_chunk` | `Option<bool>` | No |
| `metadata` | `metadata` | `Option<serde_json::Value>` | No |
| `kind` | `kind` | `"artifact-update"` | Yes |

### `StreamResponse` (discriminated union, tag = `"kind"`)

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum StreamResponse {
    Task(Task),                                    // "task"
    Message(Message),                              // "message"
    #[serde(rename = "status-update")]
    StatusUpdate(TaskStatusUpdateEvent),
    #[serde(rename = "artifact-update")]
    ArtifactUpdate(TaskArtifactUpdateEvent),
}
```

---

## `agent_card.rs`

### `TransportProtocol`

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum TransportProtocol {
    JsonRpc,   // "JSONRPC"
    Grpc,      // "GRPC"
    Rest,      // "REST"
}
```

### `AgentInterface`

| Spec field | Rust field | Type | Required |
|---|---|---|---|
| `url` | `url` | `String` | Yes |
| `transport` | `transport` | `TransportProtocol` | Yes |

### `AgentCapabilities`

| Spec field | Rust field | Type | Required |
|---|---|---|---|
| `streaming` | `streaming` | `Option<bool>` | No |
| `pushNotifications` | `push_notifications` | `Option<bool>` | No |
| `stateTransitionHistory` | `state_transition_history` | `Option<bool>` | No |
| `extensions` | `extensions` | `Option<Vec<AgentExtension>>` | No |

### `AgentProvider`

| Spec field | Rust field | Type | Required |
|---|---|---|---|
| `organization` | `organization` | `String` | Yes |
| `url` | `url` | `String` | Yes |

### `AgentSkill`

| Spec field | Rust field | Type | Required |
|---|---|---|---|
| `id` | `id` | `String` | Yes |
| `name` | `name` | `String` | Yes |
| `description` | `description` | `String` | Yes |
| `tags` | `tags` | `Vec<String>` | Yes |
| `examples` | `examples` | `Option<Vec<String>>` | No |
| `inputModes` | `input_modes` | `Option<Vec<String>>` | No (MIME types) |
| `outputModes` | `output_modes` | `Option<Vec<String>>` | No (MIME types) |
| `security` | `security` | `Option<SecurityRequirements>` | No |

### `AgentCard`

| Spec field | Rust field | Type | Required |
|---|---|---|---|
| `protocolVersion` | `protocol_version` | `String` | Yes |
| `name` | `name` | `String` | Yes |
| `description` | `description` | `String` | Yes |
| `version` | `version` | `String` | Yes |
| `url` | `url` | `String` | Yes (base RPC endpoint) |
| `preferredTransport` | `preferred_transport` | `TransportProtocol` | Yes |
| `additionalInterfaces` | `additional_interfaces` | `Option<Vec<AgentInterface>>` | No |
| `defaultInputModes` | `default_input_modes` | `Vec<String>` | Yes (MIME types) |
| `defaultOutputModes` | `default_output_modes` | `Vec<String>` | Yes (MIME types) |
| `skills` | `skills` | `Vec<AgentSkill>` | Yes |
| `capabilities` | `capabilities` | `AgentCapabilities` | Yes |
| `provider` | `provider` | `Option<AgentProvider>` | No |
| `iconUrl` | `icon_url` | `Option<String>` | No |
| `documentationUrl` | `documentation_url` | `Option<String>` | No |
| `securitySchemes` | `security_schemes` | `Option<NamedSecuritySchemes>` | No |
| `security` | `security` | `Option<SecurityRequirements>` | No |
| `supportsAuthenticatedExtendedCard` | `supports_authenticated_extended_card` | `Option<bool>` | No |
| `signatures` | `signatures` | `Option<Vec<AgentCardSignature>>` | No |

`NamedSecuritySchemes` = `HashMap<String, SecurityScheme>`.
`SecurityRequirements` = `Vec<HashMap<String, Vec<String>>>` (OpenAPI style).

---

## `security.rs`

### `SecurityScheme` (discriminated union, tag = `"type"`)

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum SecurityScheme {
    #[serde(rename = "apiKey")]
    ApiKey(ApiKeySecurityScheme),
    #[serde(rename = "http")]
    Http(HttpAuthSecurityScheme),
    #[serde(rename = "oauth2")]
    OAuth2(OAuth2SecurityScheme),
    #[serde(rename = "openIdConnect")]
    OpenIdConnect(OpenIdConnectSecurityScheme),
    #[serde(rename = "mutualTLS")]
    MutualTls(MutualTlsSecurityScheme),
}
```

### `ApiKeySecurityScheme`

| Spec field | Rust field | Type | Required |
|---|---|---|---|
| `in` | `location` | `ApiKeyLocation` | Yes |
| `name` | `name` | `String` | Yes |
| `description` | `description` | `Option<String>` | No |

```rust
#[serde(rename_all = "lowercase")]
pub enum ApiKeyLocation { Header, Query, Cookie }
```

### `HttpAuthSecurityScheme`

| Spec field | Rust field | Type | Required |
|---|---|---|---|
| `scheme` | `scheme` | `String` | Yes (e.g., `"bearer"`, `"basic"`) |
| `bearerFormat` | `bearer_format` | `Option<String>` | No (e.g., `"JWT"`) |
| `description` | `description` | `Option<String>` | No |

### `OAuth2SecurityScheme`

| Spec field | Rust field | Type | Required |
|---|---|---|---|
| `flows` | `flows` | `OAuthFlows` | Yes |
| `oauth2MetadataUrl` | `oauth2_metadata_url` | `Option<String>` | No |
| `description` | `description` | `Option<String>` | No |

### `OAuthFlows`

```rust
pub struct OAuthFlows {
    pub authorization_code: Option<AuthorizationCodeFlow>,
    pub client_credentials: Option<ClientCredentialsFlow>,
    pub device_code:        Option<DeviceCodeFlow>,
    pub implicit:           Option<ImplicitFlow>,
}
```

Each flow type mirrors the OpenAPI 3.x OAuth2 flow structure.

### `OpenIdConnectSecurityScheme`

| Spec field | Rust field | Type | Required |
|---|---|---|---|
| `openIdConnectUrl` | `open_id_connect_url` | `String` | Yes |
| `description` | `description` | `Option<String>` | No |

### `MutualTlsSecurityScheme`

| Spec field | Rust field | Type | Required |
|---|---|---|---|
| `description` | `description` | `Option<String>` | No |

---

## `push.rs`

### `PushNotificationAuthInfo`

| Spec field | Rust field | Type | Required |
|---|---|---|---|
| `schemes` | `schemes` | `Vec<String>` | Yes (e.g., `["bearer"]`) |
| `credentials` | `credentials` | `Option<String>` | No |

### `PushNotificationConfig`

| Spec field | Rust field | Type | Required |
|---|---|---|---|
| `id` | `id` | `Option<String>` | No (server-assigned) |
| `url` | `url` | `String` | Yes (HTTPS webhook) |
| `token` | `token` | `Option<String>` | No (shared secret) |
| `authentication` | `authentication` | `Option<PushNotificationAuthInfo>` | No |

### `TaskPushNotificationConfig`

| Spec field | Rust field | Type | Required |
|---|---|---|---|
| `taskId` | `task_id` | `TaskId` | Yes |
| `pushNotificationConfig` | `push_notification_config` | `PushNotificationConfig` | Yes |

---

## `params.rs`

### `SendMessageConfiguration`

| Spec field | Rust field | Type | Required |
|---|---|---|---|
| `acceptedOutputModes` | `accepted_output_modes` | `Vec<String>` | Yes (MIME types) |
| `taskPushNotificationConfig` | `task_push_notification_config` | `Option<TaskPushNotificationConfig>` | No |
| `historyLength` | `history_length` | `Option<u32>` | No |
| `returnImmediately` | `return_immediately` | `Option<bool>` | No |

### `MessageSendParams`

| Spec field | Rust field | Type | Required |
|---|---|---|---|
| `message` | `message` | `Message` | Yes |
| `configuration` | `configuration` | `Option<SendMessageConfiguration>` | No |
| `metadata` | `metadata` | `Option<serde_json::Value>` | No |

### `TaskQueryParams`

| Spec field | Rust field | Type | Required |
|---|---|---|---|
| `id` | `id` | `TaskId` | Yes |
| `historyLength` | `history_length` | `Option<u32>` | No |

### `TaskIdParams`

| Spec field | Rust field | Type | Required |
|---|---|---|---|
| `id` | `id` | `TaskId` | Yes |

### `ListTasksParams`

| Spec field | Rust field | Type | Required |
|---|---|---|---|
| `contextId` | `context_id` | `Option<ContextId>` | No |
| `status` | `status` | `Option<TaskState>` | No (filter) |
| `pageSize` | `page_size` | `Option<u32>` | No (1–100, default 50) |
| `pageToken` | `page_token` | `Option<String>` | No (cursor) |
| `statusTimestampAfter` | `status_timestamp_after` | `Option<String>` | No (ISO 8601) |
| `includeArtifacts` | `include_artifacts` | `Option<bool>` | No |

### `GetPushConfigParams`

| Spec field | Rust field | Type | Required |
|---|---|---|---|
| `taskId` | `task_id` | `TaskId` | Yes |
| `id` | `id` | `String` | Yes (config ID) |

### `DeletePushConfigParams`

Same fields as `GetPushConfigParams`.

---

## `responses.rs`

### `SendMessageResponse` (untagged union — either Task or Message, disambiguated by `kind` field)

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum SendMessageResponse {
    Task(Task),       // "task"
    Message(Message), // "message"
}
```

### `TaskListResponse`

| Field | Type | Notes |
|---|---|---|
| `tasks` | `Vec<Task>` | Paginated results |
| `next_page_token` | `Option<String>` | Absent on last page |

### `AuthenticatedExtendedCardResponse`

The full (private) `AgentCard` returned only to authenticated callers via `agent/authenticatedExtendedCard`.
Type alias: `AgentCard` (same structure; no additional fields in spec 0.3.0).

---

## `extensions.rs`

### `AgentExtension`

| Spec field | Rust field | Type | Required |
|---|---|---|---|
| `uri` | `uri` | `String` | Yes (unique identifier) |
| `description` | `description` | `Option<String>` | No |
| `required` | `required` | `Option<bool>` | No |
| `params` | `params` | `Option<serde_json::Value>` | No |

### `AgentCardSignature`

Structure is spec-defined as an object; exact fields are extension-point. Initial implementation uses `serde_json::Value` until the spec firms up the format.

---

## `jsonrpc.rs`

### `JsonRpcVersion`

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcVersion;
// Always serializes/deserializes as the string "2.0"
// Deserializer rejects any other value
```

### `JsonRpcId`

```rust
pub type JsonRpcId = Option<serde_json::Value>;
// Valid values per JSON-RPC 2.0: string, number, or null
// Notification: id field absent entirely
```

### `JsonRpcRequest`

| Field | Rust type | Notes |
|---|---|---|
| `jsonrpc` | `JsonRpcVersion` | Must be `"2.0"` |
| `id` | `JsonRpcId` | `None` = notification (no response expected) |
| `method` | `String` | A2A method name |
| `params` | `Option<serde_json::Value>` | Method-specific params |

### `JsonRpcResponse<T>` (untagged — either success or error)

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum JsonRpcResponse<T> {
    Success(JsonRpcSuccessResponse<T>),
    Error(JsonRpcErrorResponse),
}
```

Disambiguation: `JsonRpcErrorResponse` has an `error` field; `JsonRpcSuccessResponse` has a `result` field. The `untagged` enum tries `Success` first; falls back to `Error` if `result` field is absent.

---

## Serde Annotation Summary

| Pattern | Usage |
|---|---|
| `#[serde(rename_all = "camelCase")]` | All protocol structs (spec uses camelCase) |
| `#[serde(rename_all = "kebab-case")]` | `TaskState` enum (spec uses `"kebab-case"`) |
| `#[serde(rename_all = "lowercase")]` | `MessageRole`, `ApiKeyLocation` |
| `#[serde(tag = "kind")]` | `Part`, `StreamResponse`, `SendMessageResponse` |
| `#[serde(tag = "type")]` | `SecurityScheme` |
| `#[serde(untagged)]` | `FileContent`, `JsonRpcResponse<T>` |
| `#[serde(skip_serializing_if = "Option::is_none")]` | All `Option<T>` fields |
| `#[serde(rename = "final")]` | `TaskStatusUpdateEvent.r#final` (keyword conflict) |
| `#[serde(rename = "in")]` | `ApiKeySecurityScheme.location` (`in` is a keyword) |
| `#[serde(default)]` | Boolean fields defaulting to `false` |

---

## Non-Obvious Rust-Specific Decisions

### `final` and `in` as Keywords

Two spec field names conflict with Rust keywords:
- `final` (in `TaskStatusUpdateEvent`) → Rust field `r#final`, serde `#[serde(rename = "final")]`
- `in` (in `ApiKeySecurityScheme`) → Rust field `location`, serde `#[serde(rename = "in")]`

### ID Newtypes vs. `String`

All ID types are newtypes over `String` rather than raw `String`. This provides:
- Compile-time prevention of passing a `MessageId` where a `TaskId` is expected.
- A place to add validation (UUID format) without breaking the public API.
- Clear intent in function signatures.

### `serde_json::Value` for Open-Ended Fields

Fields specified as `any JSON object` in the spec (`metadata`, `data`, `params`) use `serde_json::Value`. This avoids carrying an `serde_json::Map<String, Value>` explicitly and handles both `null` and structured data.

### `#[derive(Clone)]` on All Types

All protocol types derive `Clone`. This is necessary for server-side code that must hold tasks in stores while also returning them in responses.

### `Copy` on Enums

`TaskState`, `MessageRole`, `ApiKeyLocation`, `TransportProtocol` derive `Copy`. They are discriminants with no heap allocation.

---

*Document version: 1.0*
*Last updated: 2026-03-15*
