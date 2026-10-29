# Resilient Agent

What happens to a task when the process dies, when something fails, and when
there are two of you? Those are the questions an operator asks before putting
an agent behind a load balancer. This example answers each with a check that
**asserts**, and it also asserts what the SDK does **not** do in each case, so
a later release that starts doing it fails the check and the sentence has to be
rewritten instead of quietly becoming untrue.

**Source:** [`examples/resilient-agent/`](https://github.com/tomtom215/a2a-rust/tree/main/examples/resilient-agent)

## What it shows

| Act | Asserted to work | Asserted *not* to happen |
|---|---|---|
| **1 — Durability** | A task streamed to completion reads back byte-identical (`serde_json` equality) from a *fresh* handler with new SQLite pools over the same file, and its push config is still listed. A task cut off mid-stream is persisted as far as it got | The SDK does not resume an executor after a restart: the cut-off task is still `Working` afterwards |
| **2 — Failure injection** | An executor failing its first N attempts gives N `Failed` tasks, each carrying the error text in its status message, then a `Completed` one. A webhook refusing its first deliveries shows up in `Metrics` as `delivered` and `failed`. The client's `RetryPolicy` re-sends `GetTask` over dropped connections | A `Failed` task is not retried: it is a successful RPC, and a follow-up message on it is refused. A `failed` push delivery is not re-queued. `SendMessage` over a dropped connection is not re-sent, because it may have run; it is re-sent after a `429` or `503`, which show it did not |
| **3 — Horizontal scaling** | With in-memory stores, `GetTask` on replica B for a task on A is `TaskNotFound`. With a shared `PostgresTaskStore`, B reads A's task, identical to A's read. Two limiters sharing a `PostgresRateLimitCounter` admit 5 requests where two independent ones admit 10 | Event queues are not shared: B's subscription to a task running on A ends with `Completed` but saw none of A's artifact frames |

A "replica" or a "restart" here is a second `RequestHandler` with its own
executor, event queues, port and store handles. That is exactly what two
processes share (the database, and nothing in memory) without needing a second
OS process. [The three acts](./resilient-agent/acts.md) page walks through
each check and the numbers it measured.

## Run it

```bash
cargo run -p resilient-agent
```

Acts 1 to 3 run, and the two PostgreSQL checks report `[NOT RUN]`. Point it at a
PostgreSQL server to run all eight checks:

```bash
A2A_TEST_POSTGRES_URL=postgres://postgres:postgres@localhost:5432/postgres \
  cargo run -p resilient-agent
```

No model or key is needed. The SQLite database lives in a temporary directory
the run creates and removes on every exit path, so a later run cannot pass on
a row an earlier one left.

| Variable | Effect |
|---|---|
| `A2A_TEST_POSTGRES_URL` | Runs Act 3's two PostgreSQL checks. It is the same variable `ci.yml`'s `test-postgres` job sets |
| `RESILIENT_REQUIRE_ALL=1` | A check that did not run exits `4` instead of `0` |

Exit codes: `0` every check that ran passed, `3` a check failed, `4` a check
went unexercised while `RESILIENT_REQUIRE_ALL` was set.

### `[NOT RUN]` is not a pass

The two PostgreSQL checks need a server the example cannot start. With
`A2A_TEST_POSTGRES_URL` unset they print `[NOT RUN]` and the process still
exits `0`, which suits a laptop. Set `RESILIENT_REQUIRE_ALL=1` and the same run
exits `4`, so a service that quietly stops being provisioned fails the job
instead of downgrading a check to a printed line. A URL that is set but
unreachable is a `[FAIL]` (exit `3`), because someone asked for the check and
it could not be done. CI's `example-surface` job runs this example with a
PostgreSQL service and `RESILIENT_REQUIRE_ALL=1`.

A build with a store feature switched off (`sqlite`, `postgres`; both are on by
default) prints `[NOT BUILT]` with the feature to enable, instead of dropping
the check.

## What you'll see

Captured from a real run with `A2A_TEST_POSTGRES_URL` set, as recorded in the
example's README (abridged here: each check's detail is cut to its first
clause; task ids and the store-lag figure vary):

```text
resilient-agent
===============

Act 1 — Durability: a task outlives its handler
------------------------------------------------
  [ok]        SQLite: a completed task survives a handler restart        task 887e9cce-…: Completed, 1 history message(s), 1 artifact(s) with 2 part(s) — …
  [ok]        SQLite: a task cut off mid-stream is persisted as far as it got task 896067a2-…: 3 frames seen before the cut; …

Act 2 — Failure injection: what the SDK reports, and what it does not do
-------------------------------------------------------------------------
  [ok]        Executor failing its first N attempts: the SDK does not retry it N=2: sends 1..=2 -> Failed, send 3 -> Completed; …
  [ok]        Push webhook refusing its first M deliveries: what Metrics reports M=2, 4 events: sender with 1 attempt -> delivered=2, failed=2 …
  [ok]        Client RetryPolicy: transport faults retried only where a re-send is safe K=2: GetTask over dropped connections -> ok …

Act 3 — Horizontal scaling: what "shared" means
--------------------------------------------------
  [ok]        In-memory stores: a task created on replica A is not on replica B task d81e584b-…: GetTask on A -> found; GetTask on B -> TaskNotFound — …
  [ok]        PostgreSQL shared store: B reads A's task; B's subscriber sees the end only task 4f07b014-…: GetTask on B -> Completed, …
  [ok]        PostgreSQL shared rate-limit counter: one limit across two replicas limit 5/300s per caller, 12 sends alternating A/B: …

  8 passed, 0 failed, 0 not compiled, 0 not run
```

Without `A2A_TEST_POSTGRES_URL`, Act 3's last two lines become (verbatim):

```text
  [NOT RUN]   PostgreSQL shared store: B reads A's task; B's subscriber sees the end only set A2A_TEST_POSTGRES_URL to a PostgreSQL URL to exercise this
  [NOT RUN]   PostgreSQL shared rate-limit counter: one limit across two replicas set A2A_TEST_POSTGRES_URL to a PostgreSQL URL to exercise this

  6 passed, 0 failed, 0 not compiled, 2 not run
  A [NOT RUN] check is not a passing one — it is a gap this run did not close.
```

and the process exits `0`, or `4`, listing both, under `RESILIENT_REQUIRE_ALL=1`.
The full detail of every line is on [the three acts](./resilient-agent/acts.md)
page.

## Key code

A "restart" is a second handler opened over the same SQLite file with fresh
pools. Act 1 opens each handler like this (the act also attaches a recording
`Metrics`, left out here):

```rust,no_run
# use std::time::Duration;
# use a2a_protocol_sdk::prelude::*;
# use a2a_protocol_server::push::{HttpPushSender, PushRetryPolicy};
# use a2a_protocol_server::store::SqliteTaskStore;
# use a2a_protocol_server::SqlitePushConfigStore;
# struct StreamingExecutor;
# agent_executor!(StreamingExecutor, |ctx, queue| async {
#     let emit = EventEmitter::new(ctx, queue);
#     emit.status(TaskState::Completed).await?;
#     Ok(())
# });
# async fn open(db_url: &str, executor: StreamingExecutor) -> Result<RequestHandler, String> {
let tasks = SqliteTaskStore::with_migrations(db_url)
    .await
    .map_err(|e| format!("opening the task store at {db_url}: {e}"))?;
let push_configs = SqlitePushConfigStore::new(db_url)
    .await
    .map_err(|e| format!("opening the push-config store at {db_url}: {e}"))?;
let handler = RequestHandlerBuilder::new(executor)
    .without_agent_card()
    .with_task_store(tasks)
    .with_push_config_store(push_configs)
    .with_push_sender(
        HttpPushSender::with_timeout(Duration::from_secs(1))
            .with_retry_policy(PushRetryPolicy::default().with_max_attempts(1))
            .allow_private_urls(),
    )
    .build()
    .map_err(|e| format!("building the handler: {e}"))?;
# Ok(handler)
# }
```

Every `Arc` to the first handler is dropped before the second is opened, so
the two share nothing but the file, which is what two processes would share.

## How it fits together

Act 3 is the one with more than one moving part. Two handlers stand in for two
replicas behind a balancer:

<figure class="diagram">
<a href="../images/diagrams/ex-resilient-replicas.light.svg"><img class="diagram-light" src="../images/diagrams/ex-resilient-replicas.light.svg" alt="A client streams a task from replica A and sees every frame, and subscribes to the same task on replica B and sees only the end; both replicas read and write one shared PostgresTaskStore, while each keeps its event queues in its own memory."></a>
<a href="../images/diagrams/ex-resilient-replicas.dark.svg"><img class="diagram-dark" src="../images/diagrams/ex-resilient-replicas.dark.svg" alt=""></a>
<figcaption>Act 3: the store is shared, the event queues are not.</figcaption>
</figure>

## Three things the run measured

- **The stream ends before the store is written.** The streaming reader and the
  persister are separate subscribers to the event queue, so a client sees
  `Completed` a few milliseconds before `GetTask` does (11 to 14 ms in the
  recorded run). A client that restarts an agent the instant its stream ends can
  lose the terminal write. `Metrics::on_persistence_error` is the SDK's only
  report of a write that failed outright, and the act asserts it stayed at zero.
- **A cut-off task is not resumed.** After a restart nothing reconciles or
  re-drives an in-flight task. A deployment needs its own sweep (for example,
  marking stale `Working` tasks `Failed`) or an idempotent re-send from the
  client. See [Idempotent Sends](../client/idempotency.md).
- **A `503` fault has to be decided per request** if you write your own
  injector, because hyper's client keeps a `503`'d connection alive and sends
  the retry down it.

## Tests

`cargo test -p resilient-agent --all-features` runs each act through the same
code `cargo run` does and requires a pass, so the transcript and the suite
cannot drift. The PostgreSQL test prints `[NOT RUN]` and returns when the
variable is unset (visible with `--nocapture`), and fails on a
configured-but-broken server. The injectors (the refusing webhook, the
faulting proxy) are tested on their own: one that never refused or faulted
would make every act vacuous.

## Where next

- [The three acts](./resilient-agent/acts.md): each check in detail, with
  the numbers from the run and the source file behind it.
- [Task & Config Stores](../building-agents/stores.md): the SQLite and
  PostgreSQL stores Acts 1 and 3 use.
- [Running More Than One Replica](../deployment/horizontal-scaling.md): what
  Act 3 shows, as guidance.
- [Why a Task Failed](../client/failure-classes.md): the failure classes Act 2
  injects.
- [Deploy Agent](./deploy-agent.md): the same shared PostgreSQL store, in a
  Kubernetes manifest.
