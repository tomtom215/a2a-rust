<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# JSON-RPC 2.0

This page covers the wire format of the JSON-RPC binding
(`protocolBinding: "JSONRPC"`): the envelope, the method names, batches, notifications and request IDs. To serve it, see
[Dispatchers](../../building-agents/dispatchers/jsonrpc.md); to call it, see
[Building a Client](../../client/builder.md).

The JSON-RPC transport sends all requests to a single endpoint as POST requests with a JSON-RPC 2.0 envelope:

```json
{
  "jsonrpc": "2.0",
  "method": "SendMessage",
  "id": "req-1",
  "params": {
    "message": {
      "messageId": "msg-1",
      "role": "ROLE_USER",
      "parts": [{"text": "Hello, agent!"}]
    }
  }
}
```

Response:

```json
{
  "jsonrpc": "2.0",
  "id": "req-1",
  "result": {
    "task": {
      "id": "task-abc",
      "contextId": "ctx-123",
      "status": { "state": "TASK_STATE_COMPLETED" },
      "artifacts": [...]
    }
  }
}
```

## Method names

| A2A Operation | JSON-RPC Method |
|---------------|-----------------|
| Send message | `SendMessage` |
| Stream message | `SendStreamingMessage` |
| Get task | `GetTask` |
| List tasks | `ListTasks` |
| Cancel task | `CancelTask` |
| Subscribe to task | `SubscribeToTask` |
| Create push config | `CreateTaskPushNotificationConfig` |
| Get push config | `GetTaskPushNotificationConfig` |
| List push configs | `ListTaskPushNotificationConfigs` |
| Delete push config | `DeleteTaskPushNotificationConfig` |
| Extended card | `GetExtendedAgentCard` |

## Batching

JSON-RPC supports batch requests — multiple operations in a single HTTP request:

```json
[
  {"jsonrpc": "2.0", "method": "GetTask", "id": "1", "params": {"id": "task-a"}},
  {"jsonrpc": "2.0", "method": "GetTask", "id": "2", "params": {"id": "task-b"}}
]
```

> **Note:** Streaming methods (`SendStreamingMessage`, `SubscribeToTask`) cannot be used in batch requests and will return an error.

A request with no `id` member is a notification (JSON-RPC 2.0 §4.1): the
server runs it and sends no reply, alone (`204 No Content`) or in a batch
(the array leaves it out; `204` when every member is one). A streaming
method sent as a notification is not run. `"id": null` is a call and is
answered.

## ID handling

JSON-RPC request IDs can be strings, numbers (including 0 and floats), or `null`. The server preserves the exact ID type in the response.

Streaming methods answer with SSE; see [Streaming with SSE](../streaming.md).
