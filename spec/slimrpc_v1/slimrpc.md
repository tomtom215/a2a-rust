# SLIMRPC Custom Protocol Binding for A2A

This document specifies the SLIMRPC custom protocol binding for the Agent2Agent (A2A) protocol.

SLIMRPC is an RPC protocol built on top of [SLIM](https://github.com/agntcy/slim) (Secure Low-Latency Interactive Messaging), an open-source communication framework developed by [Agntcy](https://github.com/agntcy). SLIM provides a pub/sub messaging fabric with built-in session and identity management, enabling agents to securely communicate using structured names rather than traditional URLs and ports.

## 1. Protocol Requirements

- **Underlying Network:** SLIM (Secure Low-Latency Interactive Messaging)
- **RPC Protocol:** SLIMRPC (also referred to as `srpc`)
- **Definition:** Uses the same Protocol Buffers definition as the gRPC binding: `specification/a2a.proto`
- **Serialization:** Protocol Buffers version 3 (binary encoding)
- **Service Name:** `a2a.v1.A2AService`
- **Service Parameters:** Transmitted as a flat string key-value metadata map alongside each request
- **Streaming:** SLIM unary-stream RPC pattern (unary request, streaming response)
- **Authentication:** Provided by the SLIM identity layer (shared secrets or other SLIM-supported mechanisms)
- **Transport Identifier:** `https://a2a-protocol.org/bindings/experimental-slimrpc/v1` (used in Agent Card `protocolBinding` fields)

## 2. SLIM Network and Addressing

### 2.1. SLIM Names

SLIM uses a three-component hierarchical naming scheme instead of URL-based addressing. Every participant in the SLIM network is identified by a **Name** of the form:

```
<domain>/<namespace>/<service>
```

**Examples:**

| SLIM Name                       | Description                                                    |
| :------------------------------ | :------------------------------------------------------------- |
| `mydomain/demo/echo_agent`      | An echo agent in the `mydomain` domain and `demo` namespace       |
| `mydomain/demo/client`          | A client agent in the `mydomain` domain and `demo` namespace      |
| `mydomain/production/scheduler` | A scheduler agent in production                                |

The SLIM Name serves as the service endpoint address in the Agent Card. Clients resolve the agent's Name from the `url` field of the Agent Card's `supportedInterfaces` entry where `protocolBinding` is `"https://a2a-protocol.org/bindings/experimental-slimrpc/v1"` and where the `url` uses the `domain/namespace/service` naming format.

### 2.2. SLIM Node Connection

Before exchanging A2A messages, both clients and servers **MUST** connect to a SLIM node (also called a SLIM broker or gateway). The default SLIM node port is `46357`.

**Connection flow:**

1. Initialize the SLIM runtime
2. Create a local SLIM identity (App) with a Name and credentials
3. Connect to the SLIM node
4. Subscribe the App to its own Name so it can receive incoming messages
5. For clients: open a Channel to the remote agent's Name
6. For servers: register RPC method handlers and begin serving

### 2.3. Channels

A **Channel** is a directional communication handle from a local App to a remote SLIM Name. Clients create a Channel targeting the agent's SLIM Name to issue RPC calls. The channel handles authentication, serialization, framing, and delivery of Protocol Buffer messages over the SLIM network.

## 3. Service Parameter Transmission

A2A service parameters defined in [Section 3.2.6 of the specification](https://a2a-protocol.org/v1.0.0/specification/#326-service-parameters) **MUST** be transmitted using the SLIMRPC metadata mechanism.

**Service Parameter Requirements:**

- Service parameters **MUST** be passed as a flat string key-value metadata map alongside each RPC call
- Service parameter keys are treated as **case-insensitive** strings in SLIMRPC metadata, consistent with the A2A specification
- Service parameter values are **case-sensitive**
- Multiple values for the same service parameter (e.g., `A2A-Extensions`) **SHOULD** be comma-separated in a single metadata entry

**Value Constraints:**

- Metadata values **MUST** be UTF-8 encoded strings
- There is no prescribed size limit, but implementations **SHOULD** keep metadata compact

**Reserved Metadata Keys:**

SLIMRPC does not reserve any metadata key names for its own use. All keys prefixed with `A2A-` are reserved for A2A service parameters.

**Example service parameters as SLIMRPC metadata:**

```
a2a-version: 1.0
a2a-extensions: https://example.com/extensions/geolocation/v1
```

On the server side, metadata is extracted from the request context object provided to each handler method.

## 4. Service Definition

The SLIMRPC binding exposes the `A2AService` defined in `specification/a2a.proto` for the version of A2A being implemented. Client stubs and server handler scaffolding are generated from this proto definition using the SLIMRPC code generator plugin for `protoc`.

The SLIMRPC binding uses the **same Protocol Buffer message types** as the gRPC binding. Method names, request types, and response types are determined entirely by the proto definition for the A2A version being targeted — no translation or renaming is applied.

### 4.1. Method Inventory

The following table reflects the `A2AService` methods as defined in the current A2A proto (`specification/a2a.proto`). Implementations targeting an older version of A2A **MUST** use the method names, request types, and response types from the proto for that version.

| Method Name                          | RPC Type     | Request Type                              | Response Type                                  | A2A Operation                   |
| :----------------------------------- | :----------- | :---------------------------------------- | :--------------------------------------------- | :------------------------------ |
| `SendMessage`                        | Unary        | `SendMessageRequest`                      | `SendMessageResponse`                          | Send Message                    |
| `SendStreamingMessage`               | Unary-Stream | `SendMessageRequest`                      | `StreamResponse` (stream)                      | Stream Message                  |
| `GetTask`                            | Unary        | `GetTaskRequest`                          | `Task`                                         | Get Task                        |
| `ListTasks`                          | Unary        | `ListTasksRequest`                        | `ListTasksResponse`                            | List Tasks                      |
| `CancelTask`                         | Unary        | `CancelTaskRequest`                       | `Task`                                         | Cancel Task                     |
| `SubscribeToTask`                    | Unary-Stream | `SubscribeToTaskRequest`                  | `StreamResponse` (stream)                      | Subscribe to Task               |
| `CreateTaskPushNotificationConfig`   | Unary        | `TaskPushNotificationConfig`              | `TaskPushNotificationConfig`                   | Create Push Notification Config |
| `GetTaskPushNotificationConfig`      | Unary        | `GetTaskPushNotificationConfigRequest`    | `TaskPushNotificationConfig`                   | Get Push Notification Config    |
| `ListTaskPushNotificationConfigs`    | Unary        | `ListTaskPushNotificationConfigsRequest`  | `ListTaskPushNotificationConfigsResponse`      | List Push Notification Configs  |
| `DeleteTaskPushNotificationConfig`   | Unary        | `DeleteTaskPushNotificationConfigRequest` | `google.protobuf.Empty`                        | Delete Push Notification Config |
| `GetExtendedAgentCard`               | Unary        | `GetExtendedAgentCardRequest`             | `AgentCard`                                    | Get Extended Agent Card         |

## 5. Core Methods

### 5.1. SendMessage

Sends a message to initiate or continue a task.

**Request:** [`SendMessageRequest`](https://a2a-protocol.org/v1.0.0/specification/#321-sendmessagerequest)

**Response:** [`SendMessageResponse`](https://a2a-protocol.org/v1.0.0/specification/#322-sendmessageresponse) — contains either a `Task` or `Message`

### 5.2. SendStreamingMessage

Sends a message and returns a stream of update events. Used when the agent supports streaming (`capabilities.streaming = true`).

**Request:** [`SendMessageRequest`](https://a2a-protocol.org/v1.0.0/specification/#321-sendmessagerequest)

**Response:** Server-streaming sequence of [`StreamResponse`](https://a2a-protocol.org/v1.0.0/specification/#323-stream-response) objects

### 5.3. GetTask

Retrieves the current state of a task. The `name` field uses the resource format `tasks/{task_id}`.

**Request:** [`GetTaskRequest`](https://a2a-protocol.org/v1.0.0/specification/#313-get-task)

**Response:** [`Task`](https://a2a-protocol.org/v1.0.0/specification/#411-task)

### 5.4. ListTasks

Lists tasks with optional filtering and pagination.

**Request:** [`ListTasksRequest`](https://a2a-protocol.org/v1.0.0/specification/#314-list-tasks)

**Response:** [`ListTasksResponse`](https://a2a-protocol.org/v1.0.0/specification/#314-list-tasks)

### 5.5. CancelTask

Requests cancellation of an ongoing task. The `name` field uses the resource format `tasks/{task_id}`.

**Request:** [`CancelTaskRequest`](https://a2a-protocol.org/v1.0.0/specification/#315-cancel-task)

**Response:** [`Task`](https://a2a-protocol.org/v1.0.0/specification/#411-task)

### 5.6. SubscribeToTask

Subscribes to streaming updates for an existing task. Returns `UnsupportedOperationError` if the task is in a terminal state. The `name` field uses the resource format `tasks/{task_id}`.

**Request:** [`SubscribeToTaskRequest`](https://a2a-protocol.org/v1.0.0/specification/#316-subscribe-to-task)

**Response:** Server-streaming sequence of [`StreamResponse`](https://a2a-protocol.org/v1.0.0/specification/#323-stream-response) objects

### 5.7. Push Notification Configuration Methods

#### 5.7.1. CreateTaskPushNotificationConfig

Creates a push notification configuration for a task. Requires `capabilities.pushNotifications = true`. The `parent` field uses the resource format `tasks/{task_id}`.

**Request:** [`CreateTaskPushNotificationConfigRequest`](https://a2a-protocol.org/v1.0.0/specification/#317-create-task-push-notification-config)

**Response:** [`TaskPushNotificationConfig`](https://a2a-protocol.org/v1.0.0/specification/#431-pushnotificationconfig)

#### 5.7.2. GetTaskPushNotificationConfig

Retrieves a push notification configuration. The `name` field uses the resource format `tasks/{task_id}/pushNotificationConfigs/{config_id}`.

**Request:** [`GetTaskPushNotificationConfigRequest`](https://a2a-protocol.org/v1.0.0/specification/#318-get-task-push-notification-config)

**Response:** [`TaskPushNotificationConfig`](https://a2a-protocol.org/v1.0.0/specification/#431-pushnotificationconfig)

#### 5.7.3. ListTaskPushNotificationConfigs

Lists all push notification configurations for a task. The `parent` field uses the resource format `tasks/{task_id}`.

**Request:** [`ListTaskPushNotificationConfigsRequest`](https://a2a-protocol.org/v1.0.0/specification/#319-list-task-push-notification-configs)

**Response:** [`ListTaskPushNotificationConfigsResponse`](https://a2a-protocol.org/v1.0.0/specification/#319-list-task-push-notification-configs)

#### 5.7.4. DeleteTaskPushNotificationConfig

Removes a push notification configuration. The `name` field uses the resource format `tasks/{task_id}/pushNotificationConfigs/{config_id}`.

**Request:** [`DeleteTaskPushNotificationConfigRequest`](https://a2a-protocol.org/v1.0.0/specification/#3110-delete-task-push-notification-config)

**Response:** `google.protobuf.Empty`

### 5.8. GetExtendedAgentCard

Retrieves the agent's extended card after authentication.

**Request:** [`GetExtendedAgentCardRequest`](https://a2a-protocol.org/v1.0.0/specification/#3111-get-extended-agent-card)

**Response:** [`AgentCard`](https://a2a-protocol.org/v1.0.0/specification/#441-agentcard)

## 6. Error Handling

SLIMRPC errors use gRPC-compatible status codes (from `google.rpc.Code`). The error structure carries a numeric status code, a human-readable message string, and optional structured details.

### 6.1. A2A Error to SLIMRPC Code Mapping

All A2A-specific errors defined in [Section 3.3.2 of the specification](https://a2a-protocol.org/v1.0.0/specification/#332-error-handling) **MUST** be mapped to SLIMRPC status codes. The following table defines the canonical mapping:

| A2A Error Type                      | SLIMRPC Status Code  | Notes                                       |
| :---------------------------------- | :------------------- | :------------------------------------------ |
| `JSONParseError`                    | `INTERNAL`           | Internal serialization failure              |
| `InvalidRequestError`               | `INVALID_ARGUMENT`   | Malformed or invalid request                |
| `MethodNotFoundError`               | `UNIMPLEMENTED`      | Requested method does not exist             |
| `InvalidParamsError`                | `INVALID_ARGUMENT`   | Method parameters are invalid               |
| `InternalError`                     | `INTERNAL`           | Server-side internal error                  |
| `TaskNotFoundError`                 | `NOT_FOUND`          | Task ID does not exist or is not accessible |
| `TaskNotCancelableError`            | `FAILED_PRECONDITION` | Task is in a non-cancelable state          |
| `PushNotificationNotSupportedError` | `UNIMPLEMENTED`      | Agent does not support push notifications   |
| `UnsupportedOperationError`         | `UNIMPLEMENTED`      | Operation not supported by the agent        |
| `ContentTypeNotSupportedError`      | `UNIMPLEMENTED`      | Content type not accepted by the agent      |
| `InvalidAgentResponseError`         | `INTERNAL`           | Agent produced an unexpected response       |
| *(unrecognized)*                    | `UNKNOWN`            | Fallback for unclassified errors            |

### 6.2. Error Structure

SLIMRPC errors carry the following fields:

- **`code`**: A `google.rpc.Code` integer (gRPC-compatible status code)
- **`message`**: A human-readable error description string, prefixed with the A2A error type name (e.g., `"TaskNotFoundError: task-123 not found"`)
- **`details`**: Optional structured error details

## 7. Streaming

The SLIMRPC binding supports server-streaming via the **unary-stream** RPC pattern: the client sends a single request message, and the server streams back a sequence of response messages.

### 7.1. Stream Mechanism

SLIMRPC streaming uses a unary request with a streaming response. The stream terminates when:

- The server signals normal completion by closing the stream after all events have been sent
- The server signals an error condition, which the client receives when consuming the stream
- The client cancels the request context

### 7.2. Event Ordering

Streaming events are delivered in the order they are produced by the server. Implementations **MUST** emit events in the correct sequence:

1. An initial `Task` or `Message` in the `StreamResponse`
2. Zero or more `TaskStatusUpdateEvent` or `TaskArtifactUpdateEvent` updates
3. A `TaskStatusUpdateEvent` carrying a terminal state (`completed`, `failed`, `canceled`, `rejected`) or an interrupted state (`input_required`, `auth_required`), at which point the server closes the stream

### 7.3. Stream Termination

- **Normal termination:** The server closes the stream after emitting a terminal or interrupted state event
- **Error termination:** The server sends a status error on the stream; the client receives it as an error when consuming the stream
- **Client channel close:** The client closes the SLIM channel, terminating the stream. The underlying task continues executing on the server; clients **MAY** reconnect using `SubscribeToTask` to resume receiving events
- **Deadline exceeded:** If a deadline is configured on the request, the stream is terminated when that deadline is reached. As with a channel close, the underlying task is not affected

### 7.4. Reconnection

If a streaming connection is interrupted, clients **SHOULD** use the `SubscribeToTask` method to resubscribe to an in-progress task using the task ID obtained from the initial response. Implementations **SHOULD** tolerate duplicate events that may result from reconnection.

### 7.5. Capability Declaration

Agents that support streaming **MUST** declare `capabilities.streaming = true` in their Agent Card. The `SendStreamingMessage` and `SubscribeToTask` methods **MUST** return `UnsupportedOperationError` if streaming is not enabled.

## 8. Authentication and Authorization

SLIMRPC leverages SLIM built-in identity and authentication layer. Every SLIM App is associated with a **Name** and a **credential** (shared secrets or other SLIM-supported schemes such as mTLS).

### 8.1. SLIM Identity-Based Authentication

Authentication is established at the SLIM network layer before any A2A messages are exchanged:

1. Both the client and server establish SLIM App identities with their respective Names and credentials
2. The SLIM node authenticates both parties during connection and subscription
3. Subsequent RPC calls inherit the established SLIM identity context

As a result, A2A-level authentication headers (e.g., `Authorization: Bearer …`) are not required when using the SLIMRPC binding — the SLIM network provides the authentication guarantee.

### 8.2. Credential Transmission

SLIM credentials are configured when creating a SLIM App identity and are **not** transmitted in individual RPC request metadata. Credential management is handled entirely by the SLIM runtime.

### 8.3. Server-Side Authentication Context

Each RPC handler receives a request context that carries the authenticated caller's SLIM identity. Implementations **MAY** use this to perform additional authorization checks before processing the request.

### 8.4. Agent Card Security Declarations

When using the SLIMRPC binding, agents **SHOULD** document in their Agent Card that authentication is provided by the SLIM network layer. The `securitySchemes` field **MAY** include a reference to SLIM identity, or may be omitted if no additional application-level authentication is required.

## 9. Agent Card Declaration

Agents that support the SLIMRPC binding **MUST** declare it in their Agent Card using the `supportedInterfaces` field.

**Agent Card requirements for SLIMRPC:**

- `url`: The agent's SLIM Name as a `slim://` URL in `slim://[node-host[:port]/]domain/namespace/service` format. Two forms are supported:
  - `slim://domain/namespace/service` — when the SLIM node is managed by a controller or configured out-of-band
  - `slim://node.example.com:46357/domain/namespace/service` — when the node address needs to be explicitly specified
- `protocolBinding`: Set to `"https://a2a-protocol.org/bindings/experimental-slimrpc/v1"`

**Example Agent Card fragment:**

```json
{
  "name": "Echo Agent",
  "url": "slim://domain/demo/echo_agent",
  "supportedInterfaces": [
    {
      "url": "slim://domain/demo/echo_agent",
      "protocolBinding": "https://a2a-protocol.org/bindings/experimental-slimrpc/v1"
    }
  ],
  "capabilities": {
    "streaming": true,
    "pushNotifications": false
  }
}
```

**Multi-binding example** (agent supporting both JSON-RPC and SLIMRPC):

```json
{
  "name": "Travel Planner Agent",
  "url": "https://agent.example.com",
  "supportedInterfaces": [
    {
      "url": "https://agent.example.com/rpc",
      "protocolBinding": "JSONRPC"
    },
    {
      "url": "slim://node.example.com:46357/domain/production/travel_planner",
      "protocolBinding": "https://a2a-protocol.org/bindings/experimental-slimrpc/v1"
    }
  ]
}
```

## 10. Data Type Mappings

The SLIMRPC binding uses Protocol Buffer binary serialization. All A2A data types are represented exactly as defined in `specification/a2a.proto`, identical to the gRPC binding.

| A2A Data Type   | Wire Representation                                   |
| :-------------- | :---------------------------------------------------- |
| Messages        | Protocol Buffer binary encoding                       |
| Timestamps      | `google.protobuf.Timestamp` (seconds + nanoseconds)   |
| Binary data     | `bytes` field — raw binary, no base64 encoding needed |
| Enumerations    | Protocol Buffer enum integer wire values              |
| Optional fields | Protocol Buffer `optional` / `oneof` semantics        |

Timestamps are encoded natively as `google.protobuf.Timestamp`; ISO 8601 string encoding is not required, unlike JSON-based bindings. Binary content is transmitted as raw `bytes` with no base64 wrapping.
