# Swarm

A coordinator, a tier of supervisor agents and a tier of worker agents, every
hop a real A2A call between separate servers on loopback:

```text
coordinator
  └─ SendMessage ─▶ supervisor ×S
                      └─ SendStreamingMessage ─▶ worker ×W
```

It measures what a swarm needs from the protocol layer, and fails if the numbers
do not add up. It is a measurement of the SDK's overhead, not of an
orchestrator: the supervisor is a fixed fan-out, and everything is on one
machine.

```bash
cargo run --release -p swarm
```

It runs every scenario once, printing a header line and then one JSON line per
scenario, and exits. With the defaults that is 64 workers under 8 supervisors,
32 jobs in flight per supervisor, and 4,000 jobs per scenario. CI's
`example-surface` job runs it with the defaults.

## Scenarios

| Scenario | What it measures | Fails the run when |
|---|---|---|
| `fanout` | Jobs per second and per-job latency (p50, p90, p99, max) through two delegation hops | Any job does not complete |
| `faults` | One job in ten fails `Transient`, one in fifty `InvalidRequest`. A supervisor re-sends a `Transient` failure once, to a different worker; any other class is final | The completed jobs, the `InvalidRequest` failures or the retries differ in number from what was injected |
| `cancel` | Cancelling the root tasks: how long until no worker is still executing. The supervisor cancels each child it created, through the client's `Delegation` | Any worker is still executing 5 seconds after the roots were cancelled |
| `cancel-control` | The same, with the supervisor walking away from its children instead of cancelling them | Never: it demonstrates the next section |
| `llm` | The same tree with a model behind every worker | Any job does not complete. Skipped, and its JSON line says so, when `SWARM_LLM_URL` is unset |

## The lesson in `cancel-control`

A2A has no parent/child link between tasks. Cancelling a task cascades nothing
unless the orchestrator does it. In the control scenario the supervisor walks
away from its children, and the result shows that cancelling the roots did not
stop them. A supervisor that delegates has to cancel what it delegated, which is
what
[`Delegation`](../client/delegation.md) does when its handle is dropped or
cancelled.

The `faults` scenario makes the companion point: whether to retry is decided by
the *failure class* ([Why a Task Failed](../client/failure-classes.md)), not by
the fact of failure.

## Environment

| Variable | Default | Effect |
|---|---|---|
| `SWARM_WORKERS` | `64` | Worker agents, split evenly into one pool per supervisor |
| `SWARM_SUPERVISORS` | `8` | Supervisor agents |
| `SWARM_JOBS` | `4000` | Jobs in the `fanout` and `faults` scenarios |
| `SWARM_INFLIGHT` | `32` | Jobs each supervisor keeps outstanding |
| `SWARM_LLM_URL` | unset | An OpenAI-compatible chat endpoint over plain HTTP. Enables the `llm` scenario |
| `SWARM_LLM_JOBS`, `SWARM_LLM_WORKERS`, `SWARM_LLM_SUPERVISORS`, `SWARM_LLM_INFLIGHT` | `32`, `8`, `2`, `2` | The `llm` scenario's size |
| `SWARM_DEBUG` | unset | Print why a supervisor lost contact with a worker |

A worker's job is the message text: `sleep:<ms>`, `flaky:<ms>` (fails
`Transient`), `bad:<ms>` (fails `InvalidRequest`) or `llm:<prompt>`. The exit
status is `0` when every scenario's accounting adds up and `1` otherwise, with
the reason on stderr.

## What it does not cover

- **Durability.** Every store is in memory and a restarted agent forgets its
  tasks. [Resilient Agent](./resilient-agent.md) covers restarts.
- **More than one machine.** The latency figures are the SDK's overhead, not a
  network's.
- **The orchestration above A2A:** planning, judging a result, escalating to a
  person.

## Where next

- The example's
  [README](https://github.com/tomtom215/a2a-rust/blob/main/examples/swarm/README.md)
  has the same tables, and
  [`docs/swarm-orchestration.md`](https://github.com/tomtom215/a2a-rust/blob/main/docs/swarm-orchestration.md)
  is the analysis these runs feed.
- [Benchmark Results](../reference/benchmarks.md) reports measured figures.
