<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Resilient Agent

Durability across a restart, failure injection with the numbers the SDK
reports, and two replicas over a shared store — the three questions an
operator asks before putting an agent behind a load balancer, each answered by
a check that **asserts** and names the wrong answer it rules out.

The other examples answer *what is an agent* and *does every method work on
every binding*. This one answers *what happens to a task when the process
dies, when something fails, and when there are two of me* — and, just as
deliberately, what the SDK does **not** do in each case. Every "does not" below
is pinned by an assertion, so if a future release starts doing it, the check
fails and the sentence has to be rewritten rather than quietly becoming a lie.

## What it demonstrates / what it does not

| Act | What it demonstrates (asserted) | What the SDK does **not** do (also asserted) |
|---|---|---|
| **1 — Durability** | A task streamed to completion through one handler comes back **byte-identical** (`serde_json` value equality: status, history, artifacts, timestamps) from a *fresh* handler with fresh `SqliteTaskStore` / `SqlitePushConfigStore` pools over the same file, and its push config is still listed. Zero `Metrics::on_persistence_error` reports along the way | Resume an executor. A task whose executor was cut off after its first artifact chunk reads back `Working` with exactly that chunk — and is still `Working` 500 ms later on the new handler. Restarting with tasks in flight leaves them in flight forever unless something outside the SDK reconciles them |
| **2 — Failure injection** | An executor failing its first N attempts yields N `Failed` tasks then a `Completed` one; the executor is invoked exactly once per send even with a 5-retry client `RetryPolicy`; a follow-up message on a `Failed` task is refused. A webhook refusing its first M deliveries produces exactly M `failed` outcomes at `Metrics::on_push_delivery` with a one-attempt sender, and M+1 attempts absorb them. A proxy faulting its first K requests: `GetTask` over dropped connections is retried, `SendMessage` over 503s is retried | Retry the executor — the retry is the caller's, and it is a **new task**. Put the error text anywhere a blocking caller can see it — the `Failed` task has no status message and no metadata; `metadata.error` exists only on the *streamed* status event. Re-queue a `failed` push delivery. Retry `SendMessage` over a dropped connection (ambiguous: the work may have run). At the shipped defaults, run the push sender's own retries at all: `HttpPushSender::new()` schedules 98 s per delivery against a 5 s `push_delivery_timeout` |
| **3 — Horizontal scaling** | Two replicas with in-memory stores: `GetTask` on B for A's task is `TaskNotFound`. Two over a shared `PostgresTaskStore`: B reads A's task, identical to A's read; B's subscription to a task running on A ends with `Completed`. Two limiters at 5 per window admit 10 alone and **5** sharing `PostgresRateLimitCounter` | Share event queues. B's subscriber sees the terminal state and **0** of the 2 artifact frames A streamed — a client that reconnects to the other replica mid-stream keeps a correct task and loses the frames in between |

A "replica" or a "restart" here is a second `RequestHandler` with its own
executor, event queues, port and store handles. That is exactly the sharing two
processes have — the database, and nothing in memory — without needing a
second OS process, and every structure that matters is per-handler.

## Run it

```bash
cargo run -p resilient-agent                 # Acts 1-3; Act 3's PostgreSQL checks report [NOT RUN]
A2A_TEST_POSTGRES_URL=postgres://postgres:postgres@localhost:5432/postgres \
  cargo run -p resilient-agent               # all eight checks
cargo test -p resilient-agent --all-features # each act as a test, plus the injectors on their own
```

Exit codes: `0` every check that ran passed, `3` a check failed, `4` a check
went unexercised while `RESILIENT_REQUIRE_ALL` was set.

**`[NOT RUN]` follows `incident-response`'s convention exactly.** The two
PostgreSQL checks need a server this example cannot start. With
`A2A_TEST_POSTGRES_URL` unset they print `[NOT RUN]` naming the variable and
the process **exits 0** — right for a laptop. Set `RESILIENT_REQUIRE_ALL=1`
(as CI should, since it provides the service) and the same run **exits 4**, so
a service that quietly stops being provisioned fails the job instead of
downgrading a check to a printed line. A URL that is set but unreachable is a
`[FAIL]` (exit `3`), not a `[NOT RUN]`: the operator asked for the check and
it could not be done. `A2A_TEST_POSTGRES_URL` is the same variable
`ci.yml`'s `test-postgres` job sets.

Features: `sqlite` (Act 1) and `postgres` (Act 3's shared checks) are both on
by default. A narrowed build prints `[NOT BUILT]` with the feature to enable
rather than dropping the check.

## Expected output

Captured from a real run with `A2A_TEST_POSTGRES_URL` set (task ids and the
store-lag figure vary):

```
resilient-agent
===============

Act 1 — Durability: a task outlives its handler
------------------------------------------------
  [ok]        SQLite: a completed task survives a handler restart        task 887e9cce-d969-402b-b024-0fe6da1dffbc: Completed, 1 history message(s), 1 artifact(s) with 2 part(s) — 5 frames streamed, 4 push delivery(ies) received, 0 persistence errors, store Completed 14.185899ms after the stream ended; read back byte-identical by a fresh handler, push config intact
  [ok]        SQLite: a task cut off mid-stream is persisted as far as it got task 896067a2-0389-4526-b555-ad966c313e54: 3 frames seen before the cut; the new handler reads Working, 1 history message(s), 1 artifact(s) with 1 part(s) — chunk one persisted, chunk two never written; still Working after 500ms: the SDK does NOT resume an executor after a restart

Act 2 — Failure injection: what the SDK reports, and what it does not do
-------------------------------------------------------------------------
  [ok]        Executor failing its first N attempts: the SDK does not retry it N=2: sends 1..=2 -> Failed, send 3 -> Completed; executor invoked 3x for 3 sends with a 5-retry client policy (a Failed task is a successful RPC); a follow-up on the Failed task is refused. The Failed task carries no status message and no metadata — the error text ("[-32603] injected failure on attempt 1") appears only as metadata.error on the streamed status event
  [ok]        Push webhook refusing its first M deliveries: what Metrics reports M=2, 4 events: sender with 1 attempt -> delivered=2, failed=2 (webhook refused 2, accepted 2); sender with 3 attempts -> delivered=4 (webhook refused 2, accepted 4, 4 delivered after in-sender retries). A `failed` delivery is not re-queued. Defaults: HttpPushSender::new() schedules 98s per delivery against push_delivery_timeout=5s, so its retries are cut short (`timeout_truncated`)
  [ok]        Client RetryPolicy: transport faults retried only where a re-send is safe K=2: GetTask over dropped connections -> ok (proxy faulted 2, forwarded 1); SendMessage over dropped connections -> error to caller (faulted 1, forwarded 0: not retried, ambiguous); SendMessage over 503s -> ok (faulted 2, forwarded 1); executor ran 2x

Act 3 — Horizontal scaling: what "shared" means
--------------------------------------------------
  [ok]        In-memory stores: a task created on replica A is not on replica B task d81e584b-320a-4af2-9043-9fa6033e296c: GetTask on A -> found; GetTask on B -> TaskNotFound — the default store is per-process, so a balancer in front of two replicas needs a shared store
  [ok]        PostgreSQL shared store: B reads A's task; B's subscriber sees the end only task 4f07b014-82a7-4a06-8d40-cd66180050e8: GetTask on B -> Completed, 2 artifact part(s), identical to A's; A's stream carried 2 artifact frame(s), B's subscription saw 0 and ended with Completed — the store is shared, the event queues are not
  [ok]        PostgreSQL shared rate-limit counter: one limit across two replicas limit 5/300s per caller, 12 sends alternating A/B: independent limiters admitted 10 (A 5, B 5); limiters sharing PostgresRateLimitCounter admitted 5 (A 3, B 2)

  8 passed, 0 failed, 0 not compiled, 0 not run
```

Without `A2A_TEST_POSTGRES_URL`, Act 3's last two lines become:

```
  [NOT RUN]   PostgreSQL shared store: B reads A's task; B's subscriber sees the end only set A2A_TEST_POSTGRES_URL to a PostgreSQL URL to exercise this
  [NOT RUN]   PostgreSQL shared rate-limit counter: one limit across two replicas set A2A_TEST_POSTGRES_URL to a PostgreSQL URL to exercise this

  6 passed, 0 failed, 0 not compiled, 2 not run
  A [NOT RUN] check is not a passing one — it is a gap this run did not close.
```

and the process exits `0` — or `4`, listing both, under `RESILIENT_REQUIRE_ALL=1`.

## Act 1 — Durability

`src/durability.rs`. The database is a per-run temp directory, removed on
every exit path so a later run cannot pass on a row an earlier one left.

**Completed before the restart.** Handler A (`SqliteTaskStore::with_migrations`,
`SqlitePushConfigStore::new`, an `HttpPushSender` so push configs are
accepted, and a recording `Metrics`) streams a task carrying an inline push
config to completion: `Working`, artifact chunk one, chunk two appended,
`Completed`. Every `Arc` to A is dropped. Handler B is opened over the same
file with new pools. `GetTask` on B must equal A's read as a whole JSON value,
and `ListTaskPushNotificationConfigs` on B must list exactly the webhook.

Two things this act measured rather than assumed:

- **The stream ends before the store is written.** The streaming reader and
  the persister are separate subscribers to the event queue, so the client
  sees `Completed` a few milliseconds before `GetTask` does — 11-14 ms on
  this machine. The first version of this check read the task straight after
  the stream and found it `Working`. The check now polls and prints the lag.
  A client that restarts an agent the instant its stream ends can lose the
  terminal write; `Metrics::on_persistence_error` is the SDK's only report of
  a write that failed outright, and the act asserts it stayed at zero.
- **A task cut off mid-stream is persisted as far as it got and is not
  resumed.** The executor emits chunk one and then parks forever — a process
  death from the store's point of view. B reads the task `Working` with
  exactly chunk one, and it is still `Working` after ten executor steps.
  Nothing in the SDK reconciles or re-drives an in-flight task after a
  restart; a deployment needs its own sweep (e.g. mark stale `Working` tasks
  `Failed`) or an idempotent re-send from the client.

## Act 2 — Failure injection

`src/failure.rs`, with the injectors in `src/support/injectors.rs`. Every
fault is on a real socket or in a real executor, in front of a real agent, so
the success path is a genuine agent reply.

**Executor failing its first N attempts** (`FlakyExecutor`, N=2). Each send
is a new task; the first N come back `Failed`, the third `Completed`; the
executor's invocation counter equals the number of sends — with a 5-retry
client policy on the calls, because a `Failed` task is a *successful* RPC and
nothing in `RetryTransport` or the handler re-runs it
(`crates/a2a-protocol-server/src/handler/messaging/mod.rs` writes a `Failed`
status and stops). A follow-up on the failed task's id is refused: terminal
tasks accept no new messages, so "retry" means "new task". The executor's
error text rides on the failing `TaskStatusUpdateEvent`'s `metadata.error`,
which the sync collector does not copy onto the task — a blocking caller
receives `state: Failed` and nothing else. The act streams a second failing
send to show where the text *is*.

**Webhook refusing its first M deliveries** (`webhook_sink`, M=2, answering
`503`). A recording `Metrics` collects `on_push_delivery` outcomes; the sink
tallies refusals and acceptances. Four events per task (`Working`, two
chunks, `Completed`), one delivery each:

| sender | Metrics outcomes | webhook saw |
|---|---|---|
| `PushRetryPolicy` 1 attempt | `delivered=2, failed=2` | refused 2, accepted 2 |
| 3 attempts, 10 ms backoff | `delivered=4` | refused 2, accepted 4 |

The only retry is the sender's own attempt schedule inside one delivery
(`crates/a2a-protocol-server/src/push/sender.rs`); a `failed` outcome is not
re-queued. And that schedule is bounded by the handler's
`push_delivery_timeout`: `HttpPushSender::new().max_delivery_duration()` is
98 s against a 5 s default, which the SDK itself labels `timeout_truncated`.
The act computes that arithmetic rather than waiting five seconds to watch it.

**Proxy faulting its first K requests** (`faulting_proxy`, K=2), twice: once
dropping the accepted connection unanswered, once answering `503`.
`RetryPolicy` classes both as retryable
(`crates/a2a-protocol-client/src/retry.rs`, `is_retryable`), but re-sends a
non-idempotent method only when the failure proves the server never processed
it (`safe_to_retry_non_idempotent`: `429` and `503`). Numbers from the run:
`GetTask` over dropped connections — faulted 2, forwarded 1, ok. `SendMessage`
over dropped connections — faulted 1, forwarded 0, error surfaced to the
caller. `SendMessage` over `503`s — faulted 2, forwarded 1, ok. The executor
ran twice in total (the seed task and the `503` case), so no dropped-connection
`SendMessage` reached it behind the caller's back. Note for anyone writing
their own injector: a `503` fault has to be decided per *request*, because
hyper's client keeps a `503`'d connection alive and sends the retry down it.

## Act 3 — Horizontal scaling

`src/scaling.rs`. Replica = handler with its own executor, queues and port.

**In-memory stores.** A creates a task; `GetTask` on B is `ErrorCode::TaskNotFound`
(asserted on the code, not just "an error"). The default store is per-process.

**Shared `PostgresTaskStore`.** Two handlers, two pools, one database. A
streams a task; from its first frame B subscribes (`SubscribeToTask`) while
the executor is still running on A. B's subscription ends with `Completed`
— the reattach hook polls the store for a terminal state — but sees 0 of the
2 artifact frames A's stream carried, because event queues live in process
memory. `GetTask` on B then equals `GetTask` on A as a JSON value. This is the
same pair of properties `crates/a2a-protocol-server/tests/multi_replica.rs`
pins; here they are shown over a socket with the numbers side by side.

**Shared rate-limit counter.** Two handlers each with a `RateLimitInterceptor`
at 5 requests per 300 s window, a fixed per-run caller identity (so the
counter table, which outlives the process, cannot carry a previous run's count
into this one), and 12 sends alternating A/B:

| limiters | admitted |
|---|---|
| independent (the default) | 10 (A 5, B 5) |
| each with its own `PostgresRateLimitCounter::new(url)` over the same table | 5 (A 3, B 2) |

Each replica gets its own counter *instance* and pool, as separate processes
would; what is shared is the `a2a_rate_limit` table and its atomic
`INSERT … ON CONFLICT DO UPDATE … RETURNING`
(`crates/a2a-protocol-server/src/rate_limit/shared.rs`).

## Tests

`src/tests.rs` runs each act through the same code `cargo run` does and
requires a `Pass`, so the transcript above and the suite cannot drift. The
PostgreSQL test prints `[NOT RUN]` and returns when the variable is unset
(visible with `--nocapture`), and fails on a configured-but-broken server. The
injectors are tested on their own — a webhook that never refuses, or a proxy
that never faults, would make every act above vacuous.

## Layout

| File | |
|---|---|
| `src/main.rs` | `Check`/`Outcome`, the runner, the report and exit codes |
| `src/durability.rs` | Act 1 |
| `src/failure.rs` | Act 2 |
| `src/scaling.rs` | Act 3 |
| `src/support/mod.rs` | serving on a loopback port, messages, the per-run caller identity |
| `src/support/executors.rs` | `StreamingExecutor` (can die after its first chunk), `FlakyExecutor` |
| `src/support/injectors.rs` | the refusing webhook and the faulting proxy |
| `src/support/metrics.rs` | the recording `Metrics` |

No dependency outside what the workspace already uses; the database lives in a
temp directory created and removed per run.

## License

Apache-2.0 — see the repository root.
