<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# RequestContext

Every call to `execute` and `cancel` gets a `RequestContext`: the message, the
task it belongs to, the caller's budget and approval, and what the transport
learned about the caller. This page lists what it carries and how to read the
caller's identity and tenant from it.

## Fields and accessors

The `RequestContext` provides information about the incoming request:

| Field | Type | Description |
|-------|------|-------------|
| `task_id` | `TaskId` | Server-assigned task ID |
| `context_id` | `String` | Conversation context ID |
| `message` | `Message` | The incoming message with parts |
| `stored_task` | `Option<Task>` | Previously stored task snapshot (for continuations) |
| `referenced_tasks()` | `&[Task]` | The tasks the message's `referenceTaskIds` name, read in the caller's partition (at most 16; §3.4.3) |
| `metadata` | `Option<Value>` | Arbitrary metadata from the request |
| `cancellation_token` | `CancellationToken` | Token for cooperative cancellation |
| `call_context` | `Option<CallContext>` | The call this execution belongs to (see below) |
| `budget()` | `Option<&BudgetHandle>` | The caller's budget for this turn, its deadline and meters (ADR 0023); `None` when the message carried none |
| `approval()` | `Option<&VerifiedApproval>` | The approval decision this continuation carries, once the handler's `ApprovalGate` checked it; `None` when there is none, and always on a handler built without a gate |
| `accepted_output_modes()` | `&[String]` | The media types the caller accepts in the response (`acceptedOutputModes`); empty means no preference |
| `content_check()` | `&ContentCheck` | What the server found when it checked the message's provenance signature (ADR 0027): `NotChecked`, `Unsigned` or `Verified`. A signature that does not verify is refused before the executor runs |

`RequestContext` is `#[non_exhaustive]` as of 0.13 — build one with
`RequestContext::new` and the `with_*` methods, not a struct literal.

## What the caller sent

Before 0.13 an executor could see nothing about the caller. The handler built
a `CallContext` with the caller's identity, the resolved tenant, the HTTP
headers and the activated extensions, handed it to the interceptor chain, and
then dropped it. That ruled out a whole class of deployment — an executor
could not enforce "only this tenant may invoke this skill" — and left
`Message.metadata` as the only channel for anything caller-specific, which
the *caller* writes and so is not a fact about the caller at all.

Six accessors read it; the `Option` ones return `None` rather than a
default when nobody said (`activated_extensions()` returns an empty slice):

| Accessor | Returns |
|----------|---------|
| `ctx.caller_identity()` | Who the caller is, once an authenticating interceptor established it, named by how it authenticated: `bearer:<label>`, `api-key:<label>`, `jwt:<issuer>:<sub>` or `mtls:<name>`. Build a value to compare with the functions in `auth::caller_id` |
| `ctx.tenant()` | The tenant this call resolved to, after any `TenantResolver` |
| `ctx.http_header(name)` | One inbound header, matched case-insensitively |
| `ctx.activated_extensions()` | The URIs from the `A2A-Extensions` header that the agent card declares, in request order (spec §14.2.2, §4.6.3); an undeclared URI is left out. With no card, the request's list unchanged |
| `ctx.request_id()` | The caller's `X-Request-ID`, if they sent one |
| `ctx.trace_context()` | The W3C trace this hop continues (`Option<&TraceContext>`). With a recording `tracing-opentelemetry` layer (what `Telemetry` installs) it names this server's own span, a new root when the caller sent no `traceparent`, and the client then propagates it without any code of yours; without one, it is the caller's `traceparent` when it sent one the SDK accepted, else `None`. See [Distributed tracing across agents](../../deployment/observability/trace-context.md) |

```rust
# use a2a_protocol_sdk::prelude::*;
# fn entitled(_tenant: &str, _skill: &str) -> bool { true }
/// Refuse work the caller's tenant is not entitled to.
fn check_entitlement(ctx: &RequestContext) -> A2aResult<()> {
    let Some(tenant) = ctx.tenant() else {
        return Err(A2aError::invalid_params("this skill requires a tenant"));
    };
    if !entitled(tenant, "premium-analysis") {
        return Err(A2aError::invalid_params("skill not enabled for this tenant"));
    }
    Ok(())
}
# fn main() {
#     let ctx = RequestContext::new(
#         Message::user_text("m1", "hi"), TaskId::new("t1"), "c1".to_owned());
#     assert!(check_entitlement(&ctx).is_err(), "no tenant means refusal");
# }
```

Use `ctx.tenant()` rather than `TenantContext::current()` inside an executor.
The latter is a `tokio::task_local` and the executor runs in a spawned task;
`ctx.tenant()` is an owned copy taken before the spawn. (The spawn now
re-enters the tenant scope as well, so the task-local is correct too — but
the field is the one that cannot silently become empty.)

`call_context` is `None` when the executor is driven directly, as a unit test
or a conformance harness does. That is the honest answer there rather than a
synthetic context claiming a call that never happened.
