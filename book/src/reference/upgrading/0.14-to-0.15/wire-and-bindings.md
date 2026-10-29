<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Wire format and bindings

Part of [Upgrading from 0.14 to 0.15](../0.14-to-0.15.md). Each section names what breaks and what to change; the [upgrade guide](../../upgrading.md) has the policy and how to check your own code.

## HTTP+JSON answers `application/a2a+json` unless asked otherwise

A response to a request that names no JSON type in `Accept` and sends no
`application/a2a+json` body is labelled `application/a2a+json` (§11.1's
SHOULD), on both `RestDispatcher` and the axum router; it was
`application/json`. A client that reads only `application/json` keeps getting
it by sending `Accept: application/json`. A request that asks only for
`text/event-stream` still gets `application/json` errors, so that released
a2a-go can read them. A JSON request body no longer decides the response
type on its own: an `application/json` body with no `Accept` is answered as
`application/a2a+json`.

## `TaskState` numbers its variants as the proto does

`TaskState` is now generated from `a2a.proto` (spec §1.4, ADR 0016), and each
variant's discriminant is its proto number. `TaskState::Completed as i32` was
5 (its place in the old declaration) and is now 3, as `TASK_STATE_COMPLETED`
is in the proto and in the prost enum. Only code that casts a `TaskState` to
an integer sees a difference; `TaskState::ALL` lists the variants as before.
The wire form, `"TASK_STATE_COMPLETED"`, is unchanged.

## The gRPC conversion refuses an absent REQUIRED field

proto3 reads an absent string as `""`, an absent enum as `0` and an absent
message as `None`. The `TryFrom<pb::…>` conversions of `a2a-protocol-types`
(the `proto` feature, which the gRPC binding uses) used to turn each into its
zero value, so a field the same JSON could not omit converted without it. They
now return a `ConvertError` whose `reason` is `missing required field` for
those fields: a task's `id` and its status's `state`, a message's `message_id`
and `role`, an artifact's `artifact_id`, a status or artifact update's
`task_id` and `context_id`, a card's `name`, `description`, `version` and
`capabilities` (no longer defaulted to the empty set), every interface's `url`,
`protocol_binding` and `protocol_version`, the provider's `organization` and
`url`, a skill's `id`, `name` and `description`, a signature's `protected` and
`signature`, an API key's `name`, an HTTP scheme's `scheme`, an
`openIdConnectUrl`, and the URLs of the authorization-code, client-credentials
and device-code flows.

A gRPC server answers a request carrying one `INVALID_ARGUMENT` with a
`google.rpc.BadRequest` violation naming the field; it used to reach the
handler, which refused most of them with the same code. A gRPC client reports
a response the conversion refuses as `InvalidAgentResponse`
([Client](./client.md#the-client-refuses-what-a2aproto-does-not-allow)). Code that
calls the conversions itself gets the `ConvertError` where it got a value:

```rust
use a2a_protocol_types::proto as pb;
use a2a_protocol_types::task::Task;

// A peer that never set the task's `id` sends proto3's `""`.
let err = Task::try_from(pb::Task::default()).unwrap_err();
assert_eq!(err.field, "task.id");
assert_eq!(err.reason, "missing required field");
```

## Only `A2A-Version: 1.0` is served

Spec §3.6.2: a request is processed with the semantics of the version it
asks for, and a server that does not support that version MUST answer
`VersionNotSupportedError`. This server implements 1.0 only, so a request
naming `1.1`, or a bare `1`, is now refused with an error that says it
supports `1.0`; a patch number is still ignored (`1.0.1` is 1.0). It used to
accept any `1.x` and serve it as 1.0, which is also what the official Python
SDK does. Every client in this SDK, and every peer the conformance suites
run against, sends `1.0`.

The Axum adapter (`A2aRouter`) now enforces the version, and the request media
type, as `RestDispatcher` does. A request that names no version, in the
`A2A-Version` header or the `A2A-Version` query parameter, is
`VersionNotSupportedError` (unless the `DispatchConfig` given to
`A2aRouter::with_config` has `with_require_version_header(false)`), and a
request body that is neither `application/json` nor `application/a2a+json` is
`ContentTypeNotSupportedError`. The agent card, `/health` and `/ready` stay
versionless. A client that called the router without the header must now send
it.

## ListTasks takes a `pageSize` from 1 to 100

A2A's proto bounds `page_size` to 1 through 100, and the server now refuses
anything else with `InvalidParams` (400 on HTTP+JSON). It used to clamp a
larger size to 1000 and read 0 as the default of 50. A client paging through
every task should ask for 100 at most; leaving `pageSize` out still lists 50.

## An error's `data` detail is a ProtoJSON `Any`

The JSON-RPC, HTTP+JSON and WebSocket bindings carry an error's `data` as a
`google.protobuf.Struct` detail in the error's details array. Its fields
were written beside `@type`; they are now under `value`, as ProtoJSON writes
an `Any` holding a `Struct`:

```json
{"@type": "type.googleapis.com/google.protobuf.Struct", "value": {"streamLagged": 6}}
```

`A2aError::is_stream_lagged` and `dropped_event_count` read both shapes, as
does the REST client's stream decoder. Code that read a detail's fields
directly, or a client in another language that reads them beside `@type`,
reads `value` instead.

## A SLIMRPC multicast lag is `is_stream_lagged`

A multicast member that fell behind ends with the typed `streamLagged` error,
as a lagging stream does on the JSON-RPC, HTTP+JSON, WebSocket and gRPC
bindings, so `ClientError::is_stream_lagged()` and `dropped_event_count()`
read it. Code that matched the old message text (`"event(s) dropped"`)
matches `is_stream_lagged()` instead. A unicast SLIMRPC stream's lag is
still the server's error text, without the marker.

## A JSON-RPC notification gets no reply

A JSON-RPC request with no `id` member is a notification, and "the Server
MUST NOT reply to a Notification, including those that are within a batch
request" (JSON-RPC 2.0 §4.1). The server ran one and answered it with
`"id": null`; it now runs it and answers nothing: `204 No Content` over HTTP
for a notification alone, a batch's array without the notifications' answers
(`204` when every member is one), and no frame over WebSocket. A streaming
method sent as a notification is not run, as in a batch: a stream is its
only answer. A client that sent requests without an `id` and read the reply
sends an `id`. `"id": null` is a call, and is still answered.
