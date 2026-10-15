<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# swarm

A coordinator, a tier of supervisor agents and a tier of worker agents, every
hop a real A2A call between separate servers on loopback:

```text
coordinator ──SendMessage──▶ supervisor ×S ──SendStreamingMessage──▶ worker ×W
```

It measures what a swarm needs from the protocol layer, and fails if the
numbers do not add up. The analysis these runs feed is
[`docs/swarm-orchestration.md`](../../docs/swarm-orchestration.md).

## Run

```bash
cargo run --release -p swarm
```

It runs every scenario once, printing a header line and then one JSON line
per scenario, and exits. With the defaults that is 64 workers under 8
supervisors, 32 jobs in flight per supervisor, and 4,000 jobs per scenario.

## Scenarios

| Scenario | What it measures | Fails the run when |
|---|---|---|
| `fanout` | Jobs per second and per-job latency (p50/p90/p99/max) through two delegation hops | Any job does not complete |
| `faults` | One job in ten fails `Transient`, one in fifty `InvalidRequest`. A supervisor re-sends a `Transient` failure once, to a different worker, with the fault removed; any other class is final | The completed jobs, the `InvalidRequest` failures or the retries differ in number from what was injected |
| `cancel` | Cancelling the root tasks: how long until no worker is still executing. The supervisor cancels each child it created, through the client's `Delegation` | Any worker is still executing 5 s after the roots were cancelled |
| `cancel-control` | The same, with the supervisor walking away from its children instead of cancelling them | Never: it shows that A2A has no parent/child link between tasks, so cancelling a task cascades nothing unless the orchestrator does it |
| `llm` | The same tree with a model behind every worker | Any job does not complete. Skipped, and its JSON line says so, when `SWARM_LLM_URL` is unset |

A worker's job is the message text: `sleep:<ms>`, `flaky:<ms>` (fails
`Transient`), `bad:<ms>` (fails `InvalidRequest`) or `llm:<prompt>`. See
`src/worker.rs`.

## Environment

| Variable | Default | Effect |
|---|---|---|
| `SWARM_WORKERS` | `64` | Worker agents, split evenly into one pool per supervisor |
| `SWARM_SUPERVISORS` | `8` | Supervisor agents |
| `SWARM_JOBS` | `4000` | Jobs in the `fanout` and `faults` scenarios |
| `SWARM_INFLIGHT` | `32` | Jobs each supervisor keeps outstanding |
| `SWARM_LLM_URL` | unset | An OpenAI-compatible chat endpoint over plain HTTP, e.g. `http://127.0.0.1:11434/v1/chat/completions` (Ollama) or a `llama-server`. Enables the `llm` scenario |
| `SWARM_LLM_JOBS` | `32` | Jobs in the `llm` scenario |
| `SWARM_LLM_WORKERS` | `8` | Workers in the `llm` scenario |
| `SWARM_LLM_SUPERVISORS` | `2` | Supervisors in the `llm` scenario |
| `SWARM_LLM_INFLIGHT` | `2` | Jobs in flight per supervisor in the `llm` scenario |
| `SWARM_DEBUG` | unset | Print why a supervisor lost contact with a worker |

The `llm` leg sends the model name `swarm`, so the endpoint must serve one
model whatever the request names, as `llama-server` does.

## Exit status

`0` when every scenario's accounting adds up, `1` otherwise, with the reason
on stderr.

## CI

`ci.yml`'s `example-surface` job runs `cargo run -p swarm --release` with the
defaults. The `llm` scenario is skipped there: CI has no model.

## What it does not cover

- Durability. Every store is in memory, and a restarted agent forgets its
  tasks; [`resilient-agent`](../resilient-agent/) covers restarts.
- Agents on more than one machine. Everything is on loopback, so the latency
  figures are the SDK's overhead, not a network's.
- The orchestration above A2A: planning, judging a result, escalating to a
  person. The supervisor here is a fixed fan-out.
