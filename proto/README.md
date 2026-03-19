<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Protocol Buffer Definitions

gRPC service definition for the A2A protocol v1.0.

## Overview

The `a2a.proto` file defines the gRPC service interface for all 11 A2A methods. It uses **JSON-encoded payloads over gRPC framing** — the canonical JSON wire format from the A2A spec is preserved, wrapped in a `JsonPayload` message.

## Service Definition

```
package a2a.v1;

service A2aService {
    // Messaging
    rpc SendMessage(JsonPayload) returns (JsonPayload);
    rpc SendStreamingMessage(JsonPayload) returns (stream JsonPayload);

    // Task lifecycle
    rpc GetTask(JsonPayload) returns (JsonPayload);
    rpc ListTasks(JsonPayload) returns (JsonPayload);
    rpc CancelTask(JsonPayload) returns (JsonPayload);
    rpc SubscribeToTask(JsonPayload) returns (stream JsonPayload);

    // Push notifications
    rpc CreateTaskPushNotificationConfig(JsonPayload) returns (JsonPayload);
    rpc GetTaskPushNotificationConfig(JsonPayload) returns (JsonPayload);
    rpc ListTaskPushNotificationConfigs(JsonPayload) returns (JsonPayload);
    rpc DeleteTaskPushNotificationConfig(JsonPayload) returns (JsonPayload);

    // Agent discovery
    rpc GetExtendedAgentCard(JsonPayload) returns (JsonPayload);
}
```

## Design Decision

The proto uses a `JsonPayload` wrapper (UTF-8 JSON bytes) rather than native protobuf message definitions. This ensures wire-format compatibility with the A2A spec's JSON serialization rules (ProtoJSON naming, discriminated unions, etc.).

## Usage

This proto is compiled by `tonic-build` in `a2a-protocol-server` when the `grpc` feature is enabled. You do not need to compile it manually.

## License

Apache-2.0
