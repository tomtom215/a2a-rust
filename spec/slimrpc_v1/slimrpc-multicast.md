# SLIMRPC Multicast RPC

This document specifies multicast RPC behaviour for the [SLIMRPC custom protocol binding](slimrpc.md). It describes how clients can send a single A2A message to multiple agents simultaneously using SLIM's group channel mechanism, and how clients should collect responses.

## 1. Overview

Standard point-to-point A2A interactions target a single agent. Multicast RPC extends this model to allow a client to send one request to a group of agents at once, receiving an independent response from each. This is useful for patterns such as:

- Fanning out a query to multiple independent data sources or inventory agents
- Broadcasting a task to a pool of agents and selecting the best response
- Coordinating a set of agents that must all act on the same input

Multicast RPC in SLIMRPC is built on SLIM's native group channel support. A client creates a group channel, invites the target agents into it by their individual SLIM names, and sends a single RPC to the channel. SLIM delivers the request to every agent in the group. Each agent processes the request independently and sends its response back over the same group channel, which is bidirectional.

## 2. SLIM Group Channels

SLIM group channels follow the same hierarchical naming scheme as individual agent names (see [Section 2.1 of the SLIMRPC binding spec](slimrpc.md#21-slim-names)):

```
<domain>/<namespace>/<channel name>
```

**Examples:**

| SLIM Group Channel Name                | Description                                 |
| :------------------------------------- | :------------------------------------------ |
| `mydomain/demo/inventory-query`        | A channel for fanning out inventory queries |
| `mydomain/production/classify-request` | A channel for parallel classification       |

Group channels differ from individual agent names in how membership works. Agents **cannot** subscribe themselves to a group channel. Instead, the client that creates the channel **MUST** explicitly invite each agent into the group using the agent's individual SLIM name. Once invited, SLIM routes any message sent to the group channel to all current members.

Group channels are **bidirectional**: responses from agents are sent back over the same channel and are received **only by the client that sent the request**. There are no separate per-agent response addresses.

## 3. Protocol Requirements

- **Underlying Mechanism:** SLIM group channels (see Section 2)
- **Prerequisite:** All participating agents must support the base SLIMRPC binding (`https://a2a-protocol.org/bindings/experimental-slimrpc/v1`)
- **Request cardinality:** One request message sent to a group channel
- **Response cardinality:** One response per participating agent for `SendMessage`; a stream of events per participating agent for `SendStreamingMessage` — all returned over the group channel
- **Serialization:** Protocol Buffers version 3 (binary encoding), identical to the base SLIMRPC binding
- **Supported methods:** `SendMessage` and `SendStreamingMessage` only — task management methods (`GetTask`, `CancelTask`, etc.) use point-to-point SLIMRPC

## 4. Agent Card Declaration

No additional `supportedInterfaces` entry is required for multicast. Any agent that declares a SLIMRPC interface can be invited into a SLIM group channel using its local SLIM name. The `url` field from the agent's existing SLIMRPC `supportedInterfaces` entry is the name the client uses to invite the agent:

```json
{
  "name": "Inventory Agent – Warehouse A",
  "url": "slim://mydomain/demo/inventory-a",
  "supportedInterfaces": [
    {
      "url": "slim://mydomain/demo/inventory-a",
      "protocolBinding": "https://a2a-protocol.org/bindings/experimental-slimrpc/v1"
    }
  ],
  "capabilities": {
    "streaming": true
  }
}
```

Group channel names are chosen by the client at request time and are not declared in Agent Cards.

## 5. Client Discovery and Interface Selection

When a client intends to send the same message to multiple agents, it **SHOULD** prefer multicast over issuing parallel point-to-point requests.

**Discovery procedure:**

1. Collect the Agent Cards of all target agents.
2. For each agent, inspect `supportedInterfaces` for an entry with `protocolBinding` equal to `"https://a2a-protocol.org/bindings/experimental-slimrpc/v1"`.
3. If all target agents declare a SLIMRPC interface, the client **SHOULD** use multicast RPC.
4. If any target agent does not declare a SLIMRPC interface, the client **MUST** fall back to other available transports for those agents.

## 6. Sending a Multicast Request

When all target agents support multicast, the client proceeds as follows:

1. **Create a group channel** with a SLIM name of the client's choosing, following the `domain/namespace/channel-name` format.
2. **Invite each agent** into the group channel using the individual SLIM name from the agent's SLIMRPC `supportedInterfaces` `url` field. The invitation is a SLIM runtime operation; refer to the SLIM documentation for implementation details.
3. **Send the request** by invoking `SendMessage` or `SendStreamingMessage` on the group channel, as defined in the base SLIMRPC binding. The message payload and service parameters are identical in structure to a point-to-point request.
4. **Clean up** by leaving or deleting the group channel once the interaction is complete (see Section 7).

The client **MUST** use a distinct `messageId` for the multicast request. Because each agent processes the request independently, task IDs returned in responses will be agent-specific and **MUST NOT** be assumed to be the same across agents.

## 7. Response Collection

All agents respond back over the group channel. The client receives these as individual RPC replies, each attributed to the agent that sent it by SLIM.

### 7.1. Waiting for Responses

Clients **MUST** wait for an outcome (a response, an error, or a timeout) for every invited agent before considering the multicast interaction complete. A response from one agent does not indicate that others have finished.

Clients **MUST** apply a timeout to the overall collection window. The timeout **SHOULD** be configurable and chosen to account for the expected latency of the slowest participating agent. When the timeout expires, the client **SHOULD** treat any agents that have not yet responded as having failed, and proceed with the partial result set.

### 7.2. Agent Failure Isolation

If an individual agent returns an error or its connection is lost, the client **MUST NOT** cancel or abort the interaction for the remaining agents. Each agent's response is collected independently:

- Successful responses are accepted as-is.
- Error responses (including SLIMRPC status errors, see [Section 6 of the binding spec](slimrpc.md#6-error-handling)) are recorded per agent.
- Agents that do not respond before the timeout are recorded as timed out.

The client collects the full set of outcomes — successes, errors, and timeouts — and presents them together as the result of the multicast interaction.

### 7.3. Streaming Responses

When using `SendStreamingMessage`, each agent produces its own independent stream of `StreamResponse` events delivered over the group channel. The client collects events from all agents concurrently, applying the same failure isolation and timeout rules per agent stream. An individual agent's stream terminating (normally or with an error) does not affect the streams from other agents.

### 7.4. Response Attribution

SLIM identifies the sender of each message received on the group channel, allowing the client to attribute each response to the agent that produced it. Clients **SHOULD** associate each response with the originating agent's SLIM name and Agent Card identity for downstream processing.

## 8. Error Handling

Error responses from individual agents use the same SLIMRPC status codes and error structure defined in [Section 6 of the binding spec](slimrpc.md#6-error-handling). No additional multicast-specific error codes are defined.

The following conditions are treated as agent-level failures and **MUST NOT** propagate to other agents in the group:

- An agent returns a non-`OK` SLIMRPC status code
- An agent's stream terminates with an error
- An agent does not respond within the collection timeout

A multicast interaction is only considered to have failed at the interaction level if the client is unable to create the group channel, invite agents, or deliver the request to it (for example, the SLIM node is unreachable).
