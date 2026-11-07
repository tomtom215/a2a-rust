<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Live proof of steering across replicas

ADR 0036 (`docs/adr/0036-steering-a-running-task.md`): a steering message sent
to the replica that does not run a task reaches the executor on the one that
does. The tests drive two handlers in one process; this directory runs two
separate server processes on one PostgreSQL database, which is what two
replicas are.

| File | What it is |
|---|---|
| `src/main.rs` | `a2a-live-steering-agent`: a JSON-RPC agent over `PostgresTaskStore` with steering on. The message that starts a task picks what the executor does: `read` (reads its inbox, logs each message with the sender and a Unix-millisecond time, completes on `done`), `readlate` (the same after `$LATE_SECS`), `hold` (never reads), anything else completes. Settings in the environment: `STEER_CAP`, `STEER_NOTIFY`, `RENEWAL_MS`, `TTL_MS`, `LATE_SECS` |
| `steering_live.py` | the driver: a fresh database per scenario, two agents (A on 18101, B on 18102), exit 1 when an expectation fails |

Scenarios, each a task started on A and messages sent to B:

| Scenario | Expectation |
|---|---|
| `handoff` | four messages to B are answered `handed-off`, the executor on A reads them in order, the last (`done`) completes the task |
| `notify` | with `NOTIFY` on and a 3 s renewal interval, the first message is read in under 1.5 s (a poll alone would take up to 3 s); a control run with it off is reported, not asserted |
| `full` | an inbox of two and an executor that reads after 4 s: four messages are accepted (two in the inbox, two stored behind it), the fifth is refused with the full-inbox error and `RetryInfo` of one second, and the executor then reads the four in order |
| `died` | A is killed with `SIGKILL` while it runs the task: a message sent at once is accepted and nothing reads it (the limit in the ADR's Consequences); probes then measure how long after the kill a message is refused instead of accepted |

## Run it

From the repository root. PostgreSQL 16 is the Debian cluster the other live
scripts use (`scripts/live/delegation/README.md` says how it is set up; the
driver starts it if it is down).

    cp Cargo.lock scripts/live/steering/Cargo.lock     # the workspace's dependency versions
    CARGO_PROFILE_DEV_DEBUG=0 CARGO_INCREMENTAL=0 cargo build -j2 \
      --manifest-path scripts/live/steering/Cargo.toml --target-dir target/live/steering
    python3 scripts/live/steering/steering_live.py \
      --agent target/live/steering/debug/a2a-live-steering-agent

`--scenarios handoff,full` runs some of them. Logs of each agent go to
`target/live/steering-run` (`--out` or `A2A_LIVE_OUT`). The lock file is a copy
and is not committed.

## What it recorded

2026-11-07, this checkout (debug build of the agent, two processes on one
host, PostgreSQL 16.15 over TCP on the same host, JSON-RPC over HTTP, one
4-vCPU container shared with other jobs, load average about 20 while it ran).
One run, exit status 0; its output as printed:

```
PostgreSQL 16.15 (Ubuntu 16.15-0ubuntu0.24.04.1), logs in target/live/steering-run
handoff: messages to B reach the executor on A, in order
  ok    B answered handed-off each time: ['handed-off', 'handed-off', 'handed-off', 'handed-off']
  ok    A's executor read them in order: ['one', 'two', 'three', 'done']
  ok    all on A, for the task
    'one': sent at +0 ms, answered after 77 ms, read 1036 ms after it was sent
    'two': sent at +0 ms, answered after 10 ms, read 959 ms after it was sent
    'three': sent at +0 ms, answered after 96 ms, read 949 ms after it was sent
    'done': sent at +0 ms, answered after 16 ms, read 853 ms after it was sent
  ok    the task completed: TASK_STATE_COMPLETED
notify: NOTIFY on, renewal 3 s: the first message arrives far sooner than a poll
    NOTIFY on: the first message was read 157 ms after it was sent (renewal 3000 ms)
  ok    announced, not polled: 157 ms < 1500 ms
    control, NOTIFY off: the first message was read 2898 ms after it was sent (renewal 3000 ms)
full: an inbox of 2, an executor that reads late: 4 accepted, the 5th is a 503
  ok    four accepted: ['handed-off', 'handed-off', 'handed-off', 'handed-off']
  ok    the fifth is refused as a full inbox: the steering inbox of task bf357514-d543-43b6-a042-b3bcda40460e holds its 2 messages; retry later
  ok    with RetryInfo 1s
  ok    the executor read all four, in order: ['m1', 'm2', 'm3', 'm4']
died: A is killed with SIGKILL while it runs the task
  ok    the task is working on A when A is killed
    LIMIT: a message sent 83 ms after the kill was answered 'handed-off': the dead replica's claim is live until its time to live, and nothing reads what is handed to it
  ok    the claim of a replica that has just died is still live: accepted
    after 3225 ms and 7 probe(s) a message was refused: 'the turn of task e58a6f7b-7248-40ec-b721-b27ddfe7d514 ended before the steering message was delivered; nothing was delivered, retry to see what the task became'; the task is TASK_STATE_WORKING
  ok    once the claim lapsed a message is refused, not delivered
  ok    B's executor read none of them: they were for A

PASS: 0 expectation(s) failed
```

What it says, and what it does not:

- **`handoff`**: every message sent to B was answered `handed-off` within a
  few hundredths of a second and read by A's executor 0.85 to 1.04 s after it
  was sent. With the claim renewal interval at one second and `NOTIFY` off,
  that is the look every interval picking up what B stored; it is the
  delivery time of the default, not a measure of the store.
- **`notify`**: with `NOTIFY` on and a three-second interval the first message
  was read 157 ms after it was sent; the control with it off was read 2898 ms
  after, which is the interval's own bound. The 1.5 s expectation therefore
  fails if only the poll can deliver (the control shows it would take about
  3 s). One run of each, so the two numbers show the order of magnitude, not a
  distribution.
- **`full`**: the inbox of two plus the store's queue of two held four
  messages; the fifth was refused with the full-inbox error. On JSON-RPC that
  is HTTP 200 with error `-32603` and `RetryInfo` of `1s` (the REST binding
  answers `503` with `Retry-After`; `tests/steering/main.rs` pins both). The
  four were read in the order sent, so a full inbox delayed and never lost.
- **`died`**: A was killed with `SIGKILL` while `working`. A message sent 83 ms
  later was answered `handed-off` and nothing read it: **this is the limit**
  recorded in the ADR's Consequences, here with a three-second claim time to
  live. The probes, every half second, were refused 3.2 s after the kill (the
  claim's time to live, 3 s, plus the probe cadence) with "the turn of task ...
  ended before the steering message was delivered; nothing was delivered", and
  the task was still `working` in the store: refused because the claim is
  gone, not because the task ended.

Not covered: more than two processes, a database behind a pooler, steering
through the REST, gRPC or WebSocket binding across processes (the tests cover
REST in one process), and any number of messages beyond the inbox's bound.
