<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Grading an Executor

The TCK grades servers. This grades the thing you actually wrote.

The paths executors get wrong are the awkward ones — cancellation arriving
mid-work, a parked task reported as an error, an event emitted after a
terminal status — and they are exactly the cases people skip when writing
tests by hand. The harness drives your executor against a real event queue,
with no server, no ports and no model, and grades the protocol invariants
that hold for any agent whatever it does.

Add the `conformance` feature as a dev-dependency:

```toml
[dev-dependencies]
a2a-protocol-server = { version = "0.16", features = ["conformance"] }
```

```rust
# use std::sync::Arc;
# use a2a_protocol_server::conformance;
# use a2a_protocol_server::{EventEmitter, agent_executor};
# use a2a_protocol_types::task::TaskState;
# struct MyExecutor;
# agent_executor!(MyExecutor, |ctx, queue| async {
#     let emit = EventEmitter::new(ctx, queue);
#     if emit.is_cancelled() { return emit.status(TaskState::Canceled).await; }
#     emit.status(TaskState::Working).await?;
#     if emit.is_cancelled() { return emit.status(TaskState::Canceled).await; }
#     emit.status(TaskState::Completed).await
# });
# async fn conformance_test() {
let report = conformance::check(Arc::new(MyExecutor)).await;
report.assert_pass(); // panics with the full grid if anything failed
# }
# fn main() {
#     tokio::runtime::Builder::new_current_thread()
#         .enable_all()
#         .build()
#         .expect("runtime")
#         .block_on(conformance_test());
# }
```

A failing report names the invariant and says why it matters:

```text
executor conformance:
  pass  ends_in_terminal_or_interrupt      ended in TASK_STATE_COMPLETED
  pass  transitions_are_legal              1 transitions, all legal
  pass  nothing_after_terminal             TASK_STATE_COMPLETED was the last event
  n/a   artifacts_have_ids                 no artifacts emitted
  n/a   parking_is_not_an_error            the executor never parked
  pass  does_not_panic                     returned normally
  pass  returns_within_the_time_limit      returned within the time limit
  FAIL  honours_cancellation               ran to Completed with an already-cancelled
                                           token; cancellation is cooperative, so an
                                           executor that never checks
                                           ctx.cancellation_token cannot be cancelled
                                           at all
  pass  does_not_panic_when_cancelled      returned normally
  pass  cancellation_is_not_an_error       did not report cancellation as a failure
  FAIL  stops_when_cancelled_mid_run       emitted Completed after the token was
                                           cancelled during its first write;
                                           cancellation is cooperative, so an executor
                                           that checks the token on entry and never
                                           again cannot be cancelled once it has
                                           started — which is when cancellation almost
                                           always arrives
  pass  does_not_panic_when_cancelled_mid_run returned normally
  pass  cancel_succeeds                    emitted a terminal status
  9 of 11 graded checks passed, 2 not applicable
```

**A check that did not apply is not graded**, and a report that grades
nothing fails rather than passing vacuously — the same two rules the TCK
follows, for the reason its README gives: a check that never ran once counted
as a pass, so a run reported 22/22 while 21 checks ran.

What it cannot tell you: it runs each check once, so it will not find a race;
it supplies its own message, so use `conformance::check_with` if your
executor only misbehaves on particular input; it supplies its own
`CallContext`, so use `conformance::check_with_context` if your executor
reads the caller's tenant or identity; it bounds each drive at 30 seconds and
grades an overrun as a failure, so a very slow but correct executor is
reported as broken; and it grades the executor, not the deployment — the TCK
is still what says your *server* conforms.

## Kill-switch conformance

A halt (`RequestHandler::halt`) is an operator's last control over agents
that misbehave, and with a `HaltStore` it reaches every replica and
survives a restart. If you write your own store, grade it with
`conformance::halt`, behind the same feature. `open` must open the same
backing store on each call, starting empty, as another replica would:

```rust
# use std::sync::Arc;
# use a2a_protocol_server::conformance::halt::{check_handlers, check_store};
# use a2a_protocol_server::store::{HaltStore, InMemoryHaltStore};
# async fn kill_switch_test() {
let store = InMemoryHaltStore::new(); // yours here
let open = move || {
    let store = store.clone();
    async move { Arc::new(store) as Arc<dyn HaltStore> }
};
check_store(&open).await.assert_pass();
check_handlers(&open).await.assert_pass();
# }
# fn main() {
#     tokio::runtime::Builder::new_current_thread()
#         .enable_all()
#         .build()
#         .expect("runtime")
#         .block_on(kill_switch_test());
# }
```

`check_store` grades the store's contract: a write replaces the halt of its
scope, a removal removes exactly its scope, the halt of everything and a
tenant named `all` stay two halts, another handle reads what was written and
sees what was removed, and, if the store pushes (ADR 0024), a write and a
cancel reach the other handle's subscription. A store that does not push has
those two checks not applicable; handlers on it poll. `check_handlers` builds
two handlers on the store, as replicas, and grades the kill switch end to
end: a halt stops the running task on this replica and on the other, refuses
new work on both, is stored, holds on a replica built afterwards, is lifted
everywhere by a resume on either replica, and a tenant's halt spares the
other tenants.

What it cannot tell you: it cannot make your store fail, so the rule that an
unreadable store answers with an error, never with no halts, is not graded;
it uses its own poll interval (50 ms) and an executor that honours
cancellation, so it grades the store and the handler's wiring, not your
interval or your executor; and it runs each check once, so it will not find
a race. Every store this SDK ships passes it, PostgreSQL's included
(`tests/kill_switch_conformance.rs`).
