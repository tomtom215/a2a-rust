# A2A Protocol → Rust Type Mapping

> **Updated** — This document has been updated to reflect A2A v1.0.0 types.
> For the authoritative wire format, see:
> - The actual serde attributes in `crates/a2a-types/src/*.rs`
> - The TCK conformance tests in `crates/a2a-types/tests/tck_wire_format.rs`
> - The spec compliance verification in `docs/implementation/spec-compliance-gaps.md`

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
| Protocol version string | `String` | Stored as plain string in `AgentInterface::protocol_version` |

---

## `task.rs`

### `TaskState`

```rust
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TaskState {
    #[serde(rename = "unspecified", alias = "TASK_STATE_UNSPECIFIED")]
    Unspecified,       // proto default (0-value)
    #[serde(rename = "submitted", alias = "TASK_STATE_SUBMITTED")]
    Submitted,
    #[serde(rename = "working", alias = "TASK_STATE_WORKING")]
    Working,
    #[serde(rename = "input-required", alias = "TASK_STATE_INPUT_REQUIRED")]
    InputRequired,
    #[serde(rename = "auth-required", alias = "TASK_STATE_AUTH_REQUIRED")]
    AuthRequired,
    #[serde(rename = "completed", alias = "TASK_STATE_COMPLETED")]
    Completed,
    #[serde(rename = "failed", alias = "TASK_STATE_FAILED")]
    Failed,
    #[serde(rename = "canceled", alias = "TASK_STATE_CANCELED")]
    Canceled,
    #[serde(rename = "rejected", alias = "TASK_STATE_REJECTED")]
    Rejected,
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


In v1.0, `StreamResponse` uses externally-tagged serialization: `{"task": {...}}`. `SendMessageResponse` uses `#[serde(untagged)]` — it is either a `Task` or `Message`, discriminated by field presence.

---

## `message.rs`

### `MessageRole`

```rust
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MessageRole {
    #[serde(rename = "ROLE_UNSPECIFIED")]
    Unspecified,  // proto default (0-value)
    #[serde(rename = "ROLE_USER")]
    User,
    #[serde(rename = "ROLE_AGENT")]
    Agent,
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


In v1.0, enclosing enums use externally-tagged serialization: `{"message": {...}}`. There is no explicit `kind` field.

### `PartContent` (discriminated union, tag = `"type"`)

```rust
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum PartContent {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "file")]
    File { file: FileContent },
    #[serde(rename = "data")]
    Data { data: serde_json::Value },
}
```

`Part` is a wrapper struct containing `PartContent` plus optional `metadata`.

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

### `FileContent` (struct with optional `bytes` and `uri`)

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileContent {
    pub name: Option<String>,
    pub mime_type: Option<String>,
    pub bytes: Option<String>,   // base64-encoded
    pub uri: Option<String>,
}
```

A file may have inline `bytes`, a `uri` reference, or both.

| Spec field | Rust field | Type | Required |
|---|---|---|---|
| `name` | `name` | `Option<String>` | No |
| `mimeType` | `mime_type` | `Option<String>` | No |
| `bytes` | `bytes` | `Option<String>` | No (at least one of `bytes`/`uri`) |
| `uri` | `uri` | `Option<String>` | No (at least one of `bytes`/`uri`) |

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
| `status` | `status` | `TaskStatus` | Yes |
| `metadata` | `metadata` | `Option<serde_json::Value>` | No |

### `TaskArtifactUpdateEvent`

| Spec field | Rust field | Type | Required |
|---|---|---|---|
| `taskId` | `task_id` | `TaskId` | Yes |
| `contextId` | `context_id` | `ContextId` | Yes |
| `artifact` | `artifact` | `Artifact` | Yes |
| `append` | `append` | `Option<bool>` | No |
| `lastChunk` | `last_chunk` | `Option<bool>` | No |
| `metadata` | `metadata` | `Option<serde_json::Value>` | No |

### `StreamResponse` (externally tagged with camelCase variant names)

```rust
#[non_exhaustive]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StreamResponse {
    Task(Task),
    Message(Message),
    StatusUpdate(TaskStatusUpdateEvent),
    ArtifactUpdate(TaskArtifactUpdateEvent),
}
```

Externally tagged: each JSON object has a single key (`task`, `message`, `statusUpdate`, or `artifactUpdate`) wrapping the variant value.

---

## `agent_card.rs`

### `AgentInterface`

| Spec field | Rust field | Type | Required |
|---|---|---|---|
| `url` | `url` | `String` | Yes |
| `protocolBinding` | `protocol_binding` | `String` | Yes (e.g. `"JSONRPC"`, `"REST"`, `"GRPC"`) |
| `protocolVersion` | `protocol_version` | `String` | Yes (e.g. `"1.0.0"`) |
| `tenant` | `tenant` | `Option<String>` | No |

### `AgentCapabilities`

| Spec field | Rust field | Type | Required |
|---|---|---|---|
| `streaming` | `streaming` | `Option<bool>` | No |
| `pushNotifications` | `push_notifications` | `Option<bool>` | No |
| `extendedAgentCard` | `extended_agent_card` | `Option<bool>` | No |
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
| `securityRequirements` | `security_requirements` | `Option<Vec<SecurityRequirement>>` | No |

### `AgentCard`

| Spec field | Rust field | Type | Required |
|---|---|---|---|
| `name` | `name` | `String` | Yes |
| `url` | `url` | `Option<String>` | No |
| `description` | `description` | `String` | Yes |
| `version` | `version` | `String` | Yes |
| `supportedInterfaces` | `supported_interfaces` | `Vec<AgentInterface>` | Yes |
| `defaultInputModes` | `default_input_modes` | `Vec<String>` | Yes (MIME types) |
| `defaultOutputModes` | `default_output_modes` | `Vec<String>` | Yes (MIME types) |
| `skills` | `skills` | `Vec<AgentSkill>` | Yes |
| `capabilities` | `capabilities` | `AgentCapabilities` | Yes |
| `provider` | `provider` | `Option<AgentProvider>` | No |
| `iconUrl` | `icon_url` | `Option<String>` | No |
| `documentationUrl` | `documentation_url` | `Option<String>` | No |
| `securitySchemes` | `security_schemes` | `Option<NamedSecuritySchemes>` | No |
| `securityRequirements` | `security_requirements` | `Option<Vec<SecurityRequirement>>` | No |
| `signatures` | `signatures` | `Option<Vec<AgentCardSignature>>` | No |

`NamedSecuritySchemes` = `HashMap<String, SecurityScheme>`.
`SecurityRequirement` = struct with `schemes: HashMap<String, StringList>`.

---

## `security.rs`

### `SecurityScheme` (discriminated union, tag = `"type"`)

```rust
#[non_exhaustive]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum SecurityScheme {
    #[serde(rename = "apiKey")]
    ApiKey(ApiKeySecurityScheme),
    #[serde(rename = "http")]
    Http(HttpAuthSecurityScheme),
    #[serde(rename = "oauth2")]
    OAuth2(Box<OAuth2SecurityScheme>),
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
    pub password:           Option<PasswordOAuthFlow>,
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

### `AuthenticationInfo`

| Spec field | Rust field | Type | Required |
|---|---|---|---|
| `scheme` | `scheme` | `String` | Yes (e.g., `"bearer"`) |
| `credentials` | `credentials` | `String` | Yes |

### `TaskPushNotificationConfig`

Single flat type in v1.0 (combines the previous `PushNotificationConfig` and `TaskPushNotificationConfig`).

| Spec field | Rust field | Type | Required |
|---|---|---|---|
| `tenant` | `tenant` | `Option<String>` | No |
| `id` | `id` | `Option<String>` | No (server-assigned) |
| `taskId` | `task_id` | `String` | Yes |
| `url` | `url` | `String` | Yes (HTTPS webhook) |
| `token` | `token` | `Option<String>` | No (shared secret) |
| `authentication` | `authentication` | `Option<AuthenticationInfo>` | No |

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
| `tenant` | `tenant` | `Option<String>` | No |
| `message` | `message` | `Message` | Yes |
| `contextId` | `context_id` | `Option<String>` | No |
| `configuration` | `configuration` | `Option<SendMessageConfiguration>` | No |
| `metadata` | `metadata` | `Option<serde_json::Value>` | No |

### `TaskQueryParams`

| Spec field | Rust field | Type | Required |
|---|---|---|---|
| `tenant` | `tenant` | `Option<String>` | No |
| `id` | `id` | `String` | Yes |
| `historyLength` | `history_length` | `Option<u32>` | No |

### `TaskIdParams`

| Spec field | Rust field | Type | Required |
|---|---|---|---|
| `tenant` | `tenant` | `Option<String>` | No |
| `id` | `id` | `String` | Yes |

### `CancelTaskParams`

| Spec field | Rust field | Type | Required |
|---|---|---|---|
| `tenant` | `tenant` | `Option<String>` | No |
| `id` | `id` | `String` | Yes |
| `metadata` | `metadata` | `Option<serde_json::Value>` | No |

### `ListTasksParams`

| Spec field | Rust field | Type | Required |
|---|---|---|---|
| `tenant` | `tenant` | `Option<String>` | No |
| `contextId` | `context_id` | `Option<String>` | No |
| `status` | `status` | `Option<TaskState>` | No (filter) |
| `pageSize` | `page_size` | `Option<u32>` | No (1–100, default 50) |
| `pageToken` | `page_token` | `Option<String>` | No (cursor) |
| `statusTimestampAfter` | `status_timestamp_after` | `Option<String>` | No (ISO 8601) |
| `includeArtifacts` | `include_artifacts` | `Option<bool>` | No |
| `historyLength` | `history_length` | `Option<u32>` | No |

### `GetPushConfigParams`

| Spec field | Rust field | Type | Required |
|---|---|---|---|
| `tenant` | `tenant` | `Option<String>` | No |
| `taskId` | `task_id` | `String` | Yes |
| `id` | `id` | `String` | Yes (config ID) |

### `DeletePushConfigParams`

Same fields as `GetPushConfigParams`.

---

## `responses.rs`

### `SendMessageResponse` (untagged — either Task or Message, discriminated by field presence)

```rust
#[non_exhaustive]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum SendMessageResponse {
    Task(Task),
    Message(Message),
}
```

### `TaskListResponse`

| Field | Type | Notes |
|---|---|---|
| `tasks` | `Vec<Task>` | Paginated results |
| `next_page_token` | `Option<String>` | Absent on last page |
| `page_size` | `Option<u32>` | Number of results in this page |
| `total_size` | `Option<u32>` | Total number of matching tasks |

### `AuthenticatedExtendedCardResponse`

The full (private) `AgentCard` returned only to authenticated callers via the `GetExtendedAgentCard` method.
Type alias: `AgentCard` (same structure; no additional fields in spec v1.0).

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

In v1.0, this is a structured type with JWS-style fields:

| Spec field | Rust field | Type | Required |
|---|---|---|---|
| `protected` | `protected` | `String` | Yes (base64url-encoded JWS protected header) |
| `signature` | `signature` | `String` | Yes (base64url-encoded JWS signature) |
| `header` | `header` | `Option<serde_json::Value>` | No (additional unprotected header params) |

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
| `#[serde(rename = "TASK_STATE_...")]` | `TaskState` enum (SCREAMING_SNAKE_CASE per ProtoJSON) |
| `#[serde(rename = "ROLE_...")]` | `MessageRole` enum (SCREAMING_SNAKE_CASE per ProtoJSON) |
| `#[serde(rename_all = "lowercase")]` | `ApiKeyLocation` |
| `#[serde(tag = "type")]` | `PartContent`, `SecurityScheme` |
| `#[serde(rename_all = "camelCase")]` | `StreamResponse` (externally tagged) |
| `#[serde(untagged)]` | `JsonRpcResponse<T>`, `SendMessageResponse` |
| `#[serde(skip_serializing_if = "Option::is_none")]` | All `Option<T>` fields |
| `#[serde(rename = "in")]` | `ApiKeySecurityScheme.location` (`in` is a keyword) |
| `#[serde(default)]` | Boolean fields defaulting to `false` |

---

## Non-Obvious Rust-Specific Decisions

### `in` as a Keyword

The `in` field in `ApiKeySecurityScheme` conflicts with a Rust keyword:
- `in` (in `ApiKeySecurityScheme`) → Rust field `location`, serde `#[serde(rename = "in")]`

Note: The v0.3.0 `final` field was removed in v1.0 (see `TaskStatusUpdateEvent` which now wraps `TaskStatus` directly).

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

`TaskState`, `MessageRole`, `ApiKeyLocation` derive `Copy`. They are discriminants with no heap allocation.

---

*Document version: 1.0*
*Last updated: 2026-03-15*
