# Resilient Agent

What happens to a task when the process dies, when something fails, and when
there are two of you? Those are the questions an operator asks before putting
an agent behind a load balancer. This example answers each with a check that
**asserts**, and it also asserts what the SDK does **not** do in each case, so
a later release that starts doing it fails the check and the sentence has to be
rewritten instead of quietly becoming untrue.

```bash
cargo run -p resilient-agent
```

Acts 1 to 3 run, and the two PostgreSQL checks report `[NOT RUN]`. Point it at a
PostgreSQL server to run all eight checks:

```bash
A2A_TEST_POSTGRES_URL=postgres://postgres:postgres@localhost:5432/postgres \
  cargo run -p resilient-agent
```

## What it demonstrates, and what it does not

| Act | Asserted to work | Asserted *not* to happen |
|---|---|---|
| **1 — Durability** | A task streamed to completion reads back byte-identical (`serde_json` equality) from a *fresh* handler with new SQLite pools over the same file, and its push config is still listed. A task cut off mid-stream is persisted as far as it got | The SDK does not resume an executor after a restart: the cut-off task is still `Working` afterwards |
| **2 — Failure injection** | An executor failing its first N attempts gives N `Failed` tasks, each carrying the error text in its status message, then a `Completed` one. A webhook refusing its first deliveries shows up in `Metrics` as `delivered` and `failed`. The client's `RetryPolicy` re-sends `GetTask` over dropped connections | A `Failed` task is not retried: it is a successful RPC, and a follow-up message on it is refused. A `failed` push delivery is not re-queued. `SendMessage` over a dropped connection is not re-sent, because it may have run; it is re-sent after a `429` or `503`, which show it did not |
| **3 — Horizontal scaling** | With in-memory stores, `GetTask` on replica B for a task on A is `TaskNotFound`. With a shared `PostgresTaskStore`, B reads A's task, identical to A's read. Two limiters sharing a `PostgresRateLimitCounter` admit 5 requests where two independent ones admit 10 | Event queues are not shared: B's subscription to a task running on A ends with `Completed` but saw none of A's artifact frames |

A "replica" or a "restart" here is a second `RequestHandler` with its own
executor, event queues, port and store handles. That is exactly what two
processes share (the database, and nothing in memory) without needing a second
OS process.

## `[NOT RUN]` is not a pass

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
cannot drift. The injectors (the refusing webhook, the faulting proxy) are
tested on their own: one that never refused or faulted would make every act
vacuous.

## Where next

- [Task & Config Stores](../building-agents/stores.md): the SQLite and
  PostgreSQL stores Acts 1 and 3 use.
- [Running More Than One Replica](../deployment/horizontal-scaling.md): what
  Act 3 shows, as guidance.
- [Why a Task Failed](../client/failure-classes.md): the failure classes Act 2
  injects.
- The example's
  [README](https://github.com/tomtom215/a2a-rust/blob/main/examples/resilient-agent/README.md)
  has a captured transcript of all eight checks, the exit codes and the file
  layout.
