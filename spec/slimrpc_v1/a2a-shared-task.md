# A2A Shared Task

This document specifies the **shared task** extension for A2A. It defines how multiple clients can participate in a single running `Task` on the same agent, and how agents identify the sender of each message at the application layer.

The shared-task primitive is the foundation for the [A2A Collaborative Task](a2a-collaborative-task.md) extension, which generalises this model to multiple agents in a multi-agent session.

## 1. Overview

In standard A2A, a `Task` is typically initiated by one client. The A2A SDK already supports multiple callers sending to the same non-terminal task — any client that knows a `task_id` or `context_id` can send `SendMessage` or `SendStreamingMessage` to it, and will receive the full event stream from the agent. The SDK fans all `StreamResponse` events to all active subscribers.

What the base A2A spec does not define is how the agent distinguishes *which* client sent each message. Without a standardised sender identity field, the agent sees messages on its `input_queue` without knowing whether they came from different people, different systems, or the same caller using different connections.

This extension defines:

- A standard `message-sender` metadata field (namespaced under the extension URI) that carries the identity of the sender on each inbound message
- Agent behaviour rules when multiple senders contribute to one task

Use cases:

- **Human-in-the-loop approval:** a supervisor and a worker both connected to the same task; the agent applies different logic depending on who is sending
- **Multi-client consensus:** agent waits for input from any of N authorised clients before proceeding
- **Shared workspace:** multiple humans observe and direct the same running task; each sees all agent output

## 2. `message-sender` Metadata Field

This extension defines one metadata field, `message-sender`, namespaced under the extension URI. It carries the identity of the client that sent this specific message.

**Key structure:** Extension metadata is a dictionary nested under the extension URI as the parent key in `Message.metadata`:

```json
{
  "https://a2a-protocol.org/extensions/shared-task/v1": {
    "message-sender": "<sender identity>"
  }
}
```

**Presence:** `message-sender` **MUST** be present on every `Message` delivered to the executor's `input_queue` when the agent has declared the shared-task extension.

**Population:** The entity responsible for populating `message-sender` is implementation-defined. It **MAY** be:

- The transport layer, from an authenticated connection identity
- The protocol binding
- Application-layer middleware that stamps the value before the message is enqueued

Sending clients **MAY** set `message-sender` themselves if no lower layer provides it, subject to whatever trust model the agent enforces. Agents that rely on `message-sender` for authorisation decisions **SHOULD** document whether they trust client-supplied values.

**Format:** The value is an opaque string. Its format is defined by the binding or deployment (e.g. a SLIM name, a user ID, a session token, a connection identifier).

## 3. Agent Behaviour

### 3.1. Receiving Messages from Multiple Senders

The agent **MUST** handle the case where `message-sender` differs between messages. There is no guarantee that consecutive `input_queue` items come from the same sender.

The agent **MAY** maintain per-sender state keyed by `message-sender`. The agent **MAY** respond differently based on sender identity (e.g. restricting certain operations to authorised senders, personalising output, or routing subtasks).

### 3.2. Response Fan-out

All `StreamResponse` events emitted by the agent are delivered to all active subscribers — the A2A SDK fans every event to all callers subscribed to the task. Agents cannot target a response at a specific sender. If selective delivery is required, it must be implemented at the application layer (e.g. embedding the intended recipient in the message content or a binding-defined metadata field).

### 3.3. Timeline

The agent **SHOULD** preserve the `message-sender` value on `TimelineEntry(Message)` items appended to the task `timeline`, so the sender of each message is identifiable in the persisted record.

## 4. Extension Declaration

Agents that support shared tasks **MUST** declare the extension URI in their Agent Card using the A2A `AgentExtension` mechanism. The extension URI is:

```
https://a2a-protocol.org/extensions/shared-task/v1
```

Clients that intend to join an active task **SHOULD** verify the agent declares this extension before sending messages to an existing `context_id`. Agents that do not declare the extension **MAY** still receive messages from multiple senders but are not required to honour the `message-sender` semantics defined here.

**Example Agent Card fragment:**

```json
{
  "capabilities": {
    "extensions": [
      {
        "uri": "https://a2a-protocol.org/extensions/shared-task/v1",
        "description": "Supports multiple clients sending to the same task; identifies each sender via the message-sender metadata field.",
        "required": false
      }
    ]
  }
}
```

## 5. Error Handling

The following are per-sender failures and **MUST NOT** affect other active senders or the task:

- A sender's stream or connection terminates
- A sender stops sending messages (selective participation is valid behaviour)
- A sender sends a malformed or unauthorised message (the agent **MAY** respond with an error to that sender without terminating the task)

A task progresses to a terminal state normally regardless of how many senders are currently active.
