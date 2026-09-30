# A2A Collaborative Task

This document specifies the **collaborative task** extension for A2A. It defines how a relay entity can connect the tasks of multiple A2A agents so that each agent's output becomes input to its peers, enabling multi-agent collaboration without any agent needing to call another agent directly.

For a concrete implementation over SLIM group channels, see the [SLIMRPC Collaborative Task profile](slimrpc-collaborative-task.md).

## 1. Overview

In standard A2A, a client sends a request to one agent and receives a stream of events from that agent only. Agents do not interact with each other at the A2A layer; coordination between agents requires a client-level orchestrator.

The collaborative task extension changes this by introducing a **relay**: an entity — application code or the transport layer — that reads each agent's `StreamResponse` output and injects translated items as `StreamRequest` inputs to peer agents. From each agent's perspective, it receives a normal inbound stream of `StreamRequest` items and emits a normal `StreamResponse` stream; the relay handles the cross-agent routing transparently.

Combined with the [A2A Shared Task](a2a-shared-task.md) extension, agents can identify the sender of each inbound message and apply per-sender logic, even when messages arrive from peer agents rather than human clients.

The intended topology is **all-to-all (mesh)**: every agent receives every other agent's output, and the client's prompts are delivered to all agents simultaneously. Agents decide independently whether to respond to any given message, exactly as participants in a group chat. Use cases include incident response, planning sessions, and collaborative analysis where agents build on each other's contributions.

The relay routes every peer response to every other agent. Agents do not need to know how many peers are in the session; they see inbound messages and emit responses, and the relay handles delivery.

### 1.1. Relationship to A2A Shared Task

The [A2A Shared Task](a2a-shared-task.md) extension defines how multiple clients can send messages to the same active `Task` on one agent, and how the agent identifies each sender via `message-sender`. Collaborative task builds on this: the relay is one of those "clients" for each agent it manages, and it stamps each delivered item with the identity of the originating peer. An agent in a collaborative task session therefore sees exactly the same interface as a shared-task agent — it reads `message-sender` to distinguish peers and clients, with no additional API surface.

## 2. Transport Models

A collaborative task session can be implemented at three capability tiers. Bindings choose which they support; higher tiers are more efficient but require additional transport capabilities.

### 2.1. Basic Relay Model

Works with any A2A transport that supports task continuation — the ability to send follow-up messages to a non-terminal task by `context_id` or `task_id`. This is available with `SendMessage` and `SendStreamingMessage` (A2A 1.0+).

In this model, the relay:

1. Opens a streaming subscription to each agent via `SendStreamingMessage`
2. Reads each agent's `StreamResponse` event stream
3. Translates peer events into `StreamRequest` items per [Section 5.1](#51-translation-rules)
4. Injects translated items into peer agents by calling `SendMessage(context_id, return_immediately=True)`

The original streaming subscriber for each agent continues to receive that agent's events. The relay discards the response to its injected `SendMessage` calls.

### 2.2. Live Relay Model

Works with A2A transports that support `SendLiveMessage` (A2A 1.1), which holds an open bidirectional stream per agent.

The relay operates the same way as the basic model but injects translated items directly into each agent's open `SendLiveMessage` stream rather than issuing separate `SendMessage` calls. This avoids the overhead of per-item HTTP round trips and allows the relay to back-pressure on the inbound stream.

### 2.3. Native Fan-out Model

Delegates fan-out and peer routing to the transport layer, eliminating the application relay entirely. Requires a transport capability that can deliver one agent's `StreamResponse` items to all other session participants natively. The specific mechanism is binding-defined.

In this model, `SendLiveMessage` (A2A 1.1) is required on agents, and the transport **MUST** perform session ID rewriting (see [Section 5.3](#53-session-id-rewriting)) and echo suppression (see [Section 5.4](#54-echo-suppression)) before delivery.

### 2.4. Common Requirements

All three models share the following requirements:

- The relay or transport **MUST NOT** deliver a member's own events back to that member (echo suppression — see [Section 5.4](#54-echo-suppression))
- The relay or transport **MUST** carry sender identity on every item delivered to a receiving member (see [Section 5.2](#52-message-attribution))
- The relay or transport **MUST** rewrite both `contextId` and `taskId` in every translated item before delivery to the receiving agent — each agent independently creates its own `Task` with server-generated IDs; peer messages carry IDs that belong to the originating agent, not the receiving one (see [Section 5.3](#53-session-id-rewriting))

## 3. Extension Declaration

The collaborative task extension requires **both** the [A2A Shared Task](a2a-shared-task.md) extension and this extension. Agents that support collaborative task sessions **MUST** declare both extension URIs in `capabilities.extensions` in their Agent Card.

The extension URI for the A2A Collaborative Task extension is:

```
https://a2a-protocol.org/extensions/collaborative-task/v1
```

Bindings **MAY** additionally declare a binding-specific extension URI alongside this one to advertise support for a particular transport implementation (e.g. the SLIMRPC native mode). Clients **SHOULD** verify that all target agents declare both the shared-task URI and the collaborative-task URI before initiating a session. Agents that do not declare both **SHOULD NOT** be included in a collaborative task session.

**Example Agent Card fragment:**

```json
{
  "capabilities": {
    "extensions": [
      {
        "uri": "https://a2a-protocol.org/extensions/shared-task/v1",
        "description": "Supports multiple clients sending to the same task with per-message sender identity.",
        "required": false
      },
      {
        "uri": "https://a2a-protocol.org/extensions/collaborative-task/v1",
        "description": "Supports collaborative task sessions with peer agents via a relay.",
        "required": false
      }
    ]
  }
}
```

## 4. Session Model

### 4.1. Session Initiation

A collaborative task session is established when the relay has opened streams to all participating agents and each agent has created its initial `Task`. The mechanism — which A2A method is invoked, which metadata key or parameter activates the collaborative task extension, and how the relay is started — is defined by the binding.

Each agent **MUST** create a `Task` independently and assign its own server-generated `contextId` per the A2A specification (see [Section 3.4.1](https://a2a-protocol.org/v1.1.0/specification/#341-context-identifier-semantics)). Agent-generated `contextId` values are opaque to other participants and are not required to match across agents.

Each agent **MUST** return the initial `Task` object as its first response. The relay **MUST** deliver this to all other channel members (translated as a `StreamRequest { message }` with `Part.data` carrying the Task JSON — see [Section 5.1](#51-translation-rules)), so every participant learns every agent's task ID and `contextId` without additional signalling.

Session ID rewriting is performed on the **receive side**: the relay or transport injects the correct `contextId` and `taskId` for the receiving agent into every inbound translated item before passing it to the agent executor (see [Section 5.3](#53-session-id-rewriting)).

For new sessions, each agent's runtime caches its own `contextId` and `taskId` from the `Task` it creates at session initiation. If the client wants agents to continue prior contexts, it **MAY** include a context map in the session activation; the format is defined by the binding. Each agent's runtime reads its own entry from this map and uses those IDs instead.

### 4.2. Session Behaviour

Once a session is established:

- Any participant — client or agent — **MAY** send a message at any time
- Items sent by the client are delivered to all agents
- Response items from any agent are translated and delivered to all other agents
- Translated peer items are delivered to each receiving agent's inbound stream as if they were client-originated messages (see [Section 5](#5-stream-translation))
- Participants **SHOULD** record received peer messages in their own task's timeline (see [Section 4.3](#43-timeline-integration))
- Participants **MAY** choose to act on or ignore any received message according to their own logic; no response is required

### 4.3. Timeline Integration

The A2A 1.1 `timeline` field on `Task` is the coherent, generation-ordered interaction record (see [Task Timeline Semantics](https://a2a-protocol.org/v1.1.0/specification/#328-task-timeline-semantics)). In collaborative task sessions, each agent **MUST** append received peer messages to its own task's `timeline` as `TimelineEntry(Message)` items. This produces a per-agent record of the full session, in which peer outputs are literally recorded as inputs in the timeline — exactly as if they had been sent by a client in a standard point-to-point interaction.

The agent **SHOULD** preserve the sender attribution metadata on `TimelineEntry(Message)` items appended from peer messages so the sender is identifiable in the persisted timeline.

**Effect on `generation`:** each appended `TimelineEntry` advances the task's `generation` by 1, enabling downstream subscribers to detect peer-message arrivals as generation gaps and reconcile via `GetTask` (standard ADR-002 behaviour).

## 5. Stream Translation

The relay (or transport, in the native fan-out model) translates peer `StreamResponse` items into `StreamRequest` items before delivery to receiving agents. Application code sees a unified inbound stream of `StreamRequest` items; it does not interact with the relay routing directly.

### 5.1. Translation Rules

**Client-originated request items** are delivered to agents per the relay's routing policy without structural modification. The relay **MUST** populate the shared-task `message-sender` field (see [Section 5.2](#52-message-attribution)) before delivery.

**Peer response items** (emitted by an agent) are translated before delivery to other agents:

| Peer sends (`StreamResponse`) | Translated to (`StreamRequest`) | Parts |
| :--- | :--- | :--- |
| Initial `Task` | `StreamRequest { message }` | 1× `Part.data` (Task JSON) |
| `TaskStatusUpdateEvent` with `status.message` | `StreamRequest { message }` | Original `status.message` parts + appended `Part.data` (TaskStatusUpdateEvent JSON) |
| `TaskStatusUpdateEvent` without `status.message` | `StreamRequest { message }` | 1× `Part.data` (TaskStatusUpdateEvent JSON) |
| `TaskArtifactUpdateEvent` | `StreamRequest { artifact_update }` (unchanged) | — |
| `TaskMessageUpdateEvent` | `StreamRequest { message }` | Original parts + appended `Part.data` (TaskMessageUpdateEvent JSON) |

All translated peer items carry a `Part.data` containing the JSON-serialised proto event with `preserving_proto_field_name=True` (snake_case field names). The `Part.media_type` **MUST** be set to identify the event type:

| Event | `Part.media_type` |
| :--- | :--- |
| Initial `Task` | `application/vnd.a2a.task+json` |
| `TaskStatusUpdateEvent` | `application/vnd.a2a.task-status-update+json` |
| `TaskMessageUpdateEvent` | `application/vnd.a2a.task-message-update+json` |

For `TaskStatusUpdateEvent` with `status.message`, the translated item carries the original text parts so receiving agents can directly use the message content, and appends a `Part.data` so agents can also inspect the full event envelope (state, task ID, etc.).

`TaskMessageUpdateEvent` is a notification that a peer agent's task received an external input from outside the session (e.g. a direct `SendMessage` call from another client). The original parts are preserved so receivers can read the message content directly; a `Part.data` is appended carrying the full event envelope (including `task_id` and `context_id` of the peer task that received the external input). The relay **MUST NOT** overwrite the `message-sender` field on these items — the message already carries the original sender's identity from when it was delivered to the peer agent. Receiving agents **MUST NOT** treat a forwarded `TaskMessageUpdateEvent` as agent-generated content.

### 5.2. Message Attribution

The collaborative task extension requires both the [A2A Shared Task](a2a-shared-task.md) extension and this extension to be active. Sender identity is carried exclusively by the shared-task `message-sender` field. Peer task context (`task_id`, `context_id`, `status.state`) is carried in the `Part.data` of each translated item and does not require separate metadata fields.

The relay **MUST** populate the shared-task `message-sender` field before delivering any item to a receiving agent. Application code **MUST NOT** set or override it.

```json
{
  "https://a2a-protocol.org/extensions/shared-task/v1": {
    "message-sender": "<sender identity>"
  }
}
```

**`message-sender`** **MUST** be present on every item delivered to a receiving agent. For client-originated items and all translated peer items except `TaskMessageUpdateEvent`, the relay sets this to the sender's identity. For translated `TaskMessageUpdateEvent` items, `message-sender` **MUST** be preserved from the original message (the external client that sent the out-of-band input) and **MUST NOT** be replaced with the relay agent's identity.

### 5.3. Session ID Rewriting

Before passing a translated item to the receiving agent's executor, the relay **MUST** inject the agent's cached `contextId` and `taskId` (acquired at session initiation — see [Section 4.1](#41-session-initiation)) into the message. This ensures peer-originated messages arrive with the correct task identity for that agent, exactly as if they had been sent by a direct client.

### 5.4. Echo Suppression

The relay **MUST NOT** deliver a translated item back to the agent that originally produced it. The mechanism for identifying the originating sender is defined by the binding.

## 6. Message Flows

The following diagrams illustrate collaborative task behaviour with an all-to-all relay topology. Transport-layer operations are omitted for brevity. Sender identity labels (`src=Client`, `src=AgentA`) represent the `message-sender` field.

### 6.1. Session Initiation and Task Creation

```
Client          Relay           Agent A         Agent B
  |               |               |               |
  |-initiate----->|               |               |  (client starts session via binding)
  |               |-SendStream--->|               |
  |               |-SendStream------------------->|
  |               |               |               |
  |               |<--[Task A]----|               |  (Agent A: initial Task)
  |<--[Task A]----|               |               |
  |               |--[Task A, src=AgentA]-------->|  (translated StreamRequest)
  |               |               |               |
  |               |<--[Task B]---------------------|  (Agent B: initial Task)
  |<--[Task B]----|               |               |
  |               |--[Task B, src=AgentB]-------->|  (translated StreamRequest)
```

### 6.2. Agent-to-Agent Messaging

```
Client          Relay           Agent A         Agent B
  |               |               |               |
  |               |<--[StatusEvt]-|               |  (Agent A: status update with message)
  |<--[StatusEvt]-|src=AgentA     |               |
  |               |--[translated, src=AgentA]---->|  (Agent B receives peer message)
  |               |               |               |
  |               |<--------[StatusEvt]-----------|  (Agent B: responds)
  |<--[StatusEvt]-|src=AgentB     |               |
  |               |--[translated, src=AgentB]---->|  (Agent A receives peer message)
```

Agent A and Agent B each append the other's message as `TimelineEntry(Message)` in their own task timeline.

## 7. Error Handling

The following are per-agent failures and **MUST NOT** terminate the session or affect other agents:

- An agent's stream terminates with an error
- An agent's task transitions to a failed terminal state
- An agent does not respond to a received message (selective participation is valid)
- An agent is removed from the session while active

Session-level failure conditions (e.g. the relay cannot be established, an agent cannot be reached at session initiation) and their status codes are defined by the binding.
