<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Why a Task Failed

A failed task is `TaskState::Failed` plus an error message. The message is
prose, written by whoever wrote the executor, so a caller deciding what to do
next is matching English — and the decisions are genuinely different:

| Class | What a caller should do |
|---|---|
| `InvalidRequest` | Never retry. Fix the request. |
| `Transient` | Retry with backoff. |
| `PolicyRefusal` | Stop and escalate to a person. |
| `BudgetExhausted` | Retry with more budget, or not at all. |
| `Internal` | The agent broke. Retry once, then escalate. |

Without a class, an orchestrator either retries what can never succeed or
abandons what would have worked on the second attempt. Both are visible as
bad agent behaviour to whoever is watching.

This is not part of A2A v1.0. It ships as the declared extension
`https://a2a-rust.com/extensions/failure/v1`, so servers stay conformant and
the TCK is unaffected.

## Reading it

```rust
# use a2a_protocol_types::failure::{FailureClass, set_class};
# use a2a_protocol_types::message::Message;
# use a2a_protocol_types::task::{ContextId, Task, TaskState, TaskStatus};
# fn decide(task: &Task) -> &'static str {
match task.failure_class() {
    Some(c) if c.is_retryable() => "back off and try the same request again",
    Some(c) if c.needs_human()  => "escalate; retrying is the wrong response",
    Some(FailureClass::BudgetExhausted) => "retry only with a larger budget",
    Some(_) => "give up; read task.status.message for the detail",
    None => "the agent classified nothing — older peer, or no extension",
}
# }
# let mut note = Message::agent_text("m1", "rate limited upstream");
# set_class(&mut note, FailureClass::Transient);
# let mut status = TaskStatus::new(TaskState::Failed);
# status.message = Some(note);
# let task = Task {
#     id: "t1".into(), context_id: ContextId::new("c1"), status,
#     history: None, artifacts: None, metadata: None,
# };
# assert_eq!(decide(&task), "back off and try the same request again");
```

`None` means the agent said nothing, not that the task succeeded — the state
says that. An unrecognised class from a newer peer reads as `Internal` rather
than as an error, so a peer classifying more finely than your build cannot
make its failures unreadable.

`BudgetExhausted` is deliberately **not** `is_retryable()`. The identical
request hits the identical bound; it is retryable with a *larger* budget,
which is a different request.

## Writing it

Most of the time you do not. When an executor returns `Err`, the server uses
a class recorded on the error with `failure::set_error_class`, and otherwise
classifies from the error code — which yields only `InvalidRequest` or
`Internal`, because no `ErrorCode` carries the meaning "transient" or
"refused on policy". (`?` on a client call records `Transient` for any
retryable `ClientError`; see [Error Handling](./error-handling.md).) An
executor deadline is classified `BudgetExhausted`, since a deadline is a bound
that was hit rather than an agent that broke.

The two classes no error code can express need the agent to say so, either
with `set_error_class` on the error it returns or by emitting its own
classified status:

```rust
# use a2a_protocol_server::{EventEmitter, agent_executor};
# use a2a_protocol_types::failure::FailureClass;
# use a2a_protocol_types::task::TaskState;
# struct RateLimitedAgent;
# fn upstream_returned_429() -> bool { true }
agent_executor!(RateLimitedAgent, |ctx, queue| async {
    let emit = EventEmitter::new(ctx, queue);
    emit.status(TaskState::Working).await?;

    if upstream_returned_429() {
        // Retry me: the model was rate limited, not wrong.
        return emit
            .fail(FailureClass::Transient, "upstream model returned 429")
            .await;
    }

    emit.status(TaskState::Completed).await
});
```

The prose stays where it was, on the status message, for the human reading
the incident. The class is for the caller deciding what to do about it.

## Advertising it

Every server built with `RequestHandlerBuilder` advertises the extension on
its card, because every server classifies — the classification happens in the
server's own failure path rather than in a store that may or may not support
it. It is never marked `required`: a client that does not know the extension
reads the prose exactly as before. An operator who declares the extension
themselves keeps their own entry, `required` flag included.

## Next Steps

- **[Idempotent Sends](./idempotency.md)** — the other declared extension
- **[Error Handling](./error-handling.md)** — transport and protocol errors,
  which are a different thing from a task that ran and failed
