# Testing Your Agent

a2a-rust makes it easy to test agents at multiple levels: unit testing executors, integration testing with real HTTP, and property-based testing with fuzz targets.

## Grading an executor against the protocol

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
a2a-protocol-server = { version = "0.13", features = ["conformance"] }
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
  pass  cancel_emits_terminal              emitted a terminal status
  9 of 11 graded checks passed, 2 not applicable
```

**A check that did not apply is not graded**, and a report that grades
nothing fails rather than passing vacuously — the same two rules the TCK
follows, for the reason its README gives: a run that measured nothing once
reported full marks.

What it cannot tell you: it runs each check once, so it will not find a race;
it supplies its own message, so use `conformance::check_with` if your
executor only misbehaves on particular input; it supplies its own
`CallContext`, so use `conformance::check_with_context` if your executor
reads the caller's tenant or identity; it bounds each drive at 30 seconds and
grades an overrun as a failure, so a very slow but correct executor is
reported as broken; and it grades the executor, not the deployment — the TCK
is still what says your *server* conforms.

## Unit Testing Executors

Test your executor logic directly by creating a `RequestContext` and mock `EventQueueWriter`:

```rust,no_run
use a2a_protocol_sdk::prelude::*;
use a2a_protocol_server::streaming::event_queue::new_in_memory_queue;

#[tokio::test]
async fn test_calculator_executor() {
    let executor = CalcExecutor;

    // Create a writer/reader pair directly for unit testing
    let (writer, mut reader) = new_in_memory_queue();

    // Build the request context. `RequestContext` is `#[non_exhaustive]`
    // since 0.13, so build it with `new` and the `with_*` methods rather
    // than a struct literal.
    let ctx = RequestContext::new(
        Message::user_text("msg-1", "3 + 5"),
        TaskId::new("test-task"),
        "ctx-1".to_owned(),
    );

    // Run the executor
    executor.execute(&ctx, &writer).await.unwrap();

    // Read events from the queue
    let events: Vec<_> = collect_events(&mut reader).await;

    // Verify: Working → ArtifactUpdate → Completed
    assert!(matches!(&events[0],
        StreamResponse::StatusUpdate(e) if e.status.state == TaskState::Working));
    assert!(matches!(&events[1],
        StreamResponse::ArtifactUpdate(e) if extract_text(&e.artifact) == "8"));
    assert!(matches!(&events[2],
        StreamResponse::StatusUpdate(e) if e.status.state == TaskState::Completed));
}
# fn main() {}
# async fn collect_events(reader: &mut a2a_protocol_server::streaming::event_queue::InMemoryQueueReader) -> Vec<StreamResponse> {
#     use a2a_protocol_server::streaming::EventQueueReader;
#     let mut out = Vec::new();
#     while let Some(Ok(e)) = reader.read().await { out.push(e.event); }
#     out
# }
# fn extract_text(a: &Artifact) -> String { a.text().unwrap_or_default().to_owned() }
# struct CalcExecutor;
# agent_executor!(CalcExecutor, |_ctx, _queue| async { Ok(()) });
```

## Integration Testing with HTTP

Test the full stack by starting a real server and using a client:

```rust,no_run
# use a2a_protocol_sdk::prelude::*;
# struct CalcExecutor;
# agent_executor!(CalcExecutor, |_ctx, _queue| async { Ok(()) });
use a2a_protocol_sdk::server::{RequestHandlerBuilder, JsonRpcDispatcher};
use a2a_protocol_sdk::client::ClientBuilder;
use std::sync::Arc;

#[tokio::test]
async fn test_end_to_end() {
    // Build handler and server
    let handler = Arc::new(
        RequestHandlerBuilder::new(CalcExecutor).build().unwrap()
    );
    let dispatcher = Arc::new(JsonRpcDispatcher::new(handler));
    let addr = start_test_server(dispatcher).await;

    // Build client
    let client = ClientBuilder::new(format!("http://{addr}"))
        .build()
        .unwrap();

    // Send a message
    let response = client
        .send_message(MessageSendParams::new(Message::user_text(
            "test-msg",
            "10 + 20",
        )))
        .await
        .unwrap();

    // Verify
    if let SendMessageResponse::Task(task) = response {
        assert_eq!(task.status.state, TaskState::Completed);
    } else {
        panic!("expected task response");
    }
}

async fn start_test_server(
    dispatcher: Arc<JsonRpcDispatcher>,
) -> std::net::SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        loop {
            let (stream, _) = listener.accept().await.unwrap();
            let io = hyper_util::rt::TokioIo::new(stream);
            let d = Arc::clone(&dispatcher);
            tokio::spawn(async move {
                let svc = hyper::service::service_fn(move |req| {
                    let d = Arc::clone(&d);
                    async move { Ok::<_, std::convert::Infallible>(d.dispatch(req).await) }
                });
                let _ = hyper_util::server::conn::auto::Builder::new(
                    hyper_util::rt::TokioExecutor::new(),
                ).serve_connection(io, svc).await;
            });
        }
    });

    addr
}
# fn main() {}
```

## Testing Both Transports

Run the same tests against both JSON-RPC and REST:

```rust,no_run
# use a2a_protocol_sdk::prelude::*;
# async fn start_jsonrpc_server() -> std::net::SocketAddr { unimplemented!() }
# async fn start_rest_server() -> std::net::SocketAddr { unimplemented!() }
#[tokio::test]
async fn test_jsonrpc_transport() {
    let addr = start_jsonrpc_server().await;
    let client = ClientBuilder::new(format!("http://{addr}")).build().unwrap();
    run_test_suite(&client).await;
}

#[tokio::test]
async fn test_rest_transport() {
    let addr = start_rest_server().await;
    let client = ClientBuilder::new(format!("http://{addr}"))
        .with_protocol_binding("REST")
        .build().unwrap();
    run_test_suite(&client).await;
}

async fn run_test_suite(client: &A2aClient) {
    // Test send_message, stream_message, get_task, etc.
}
# fn main() {}
```

## Testing Streaming

```rust,no_run
#[tokio::test]
async fn test_streaming() {
    let addr = start_server().await;
    let client = ClientBuilder::new(format!("http://{addr}")).build().unwrap();

    let mut stream = client.stream_message(params).await.unwrap();
    let mut events = vec![];

    while let Some(event) = stream.next().await {
        events.push(event.unwrap());
    }

    // Verify event sequence
    assert!(events.len() >= 3); // Working + Artifact + Completed
}
```

## Wire Format Tests

Verify JSON serialization matches the A2A spec:

```rust,no_run
#[test]
fn task_state_wire_format() {
    let status = TaskStatus::new(TaskState::Completed);
    let json = serde_json::to_string(&status).unwrap();
    assert!(json.contains("\"TASK_STATE_COMPLETED\""));
}

#[test]
fn message_role_wire_format() {
    let msg = Message {
        id: MessageId::new("m1"),
        role: MessageRole::User,
        parts: vec![Part::text("hi")],
        // ...
    };
    let json = serde_json::to_string(&msg).unwrap();
    assert!(json.contains("\"ROLE_USER\""));
    assert!(json.contains("\"messageId\""));
}
```

## Fuzz Testing

The `fuzz/` directory contains fuzz targets for JSON parsing:

```bash
# Requires nightly Rust
cd fuzz
cargo +nightly fuzz run json_deser
```

Fuzz testing helps find edge cases in JSON deserialization that unit tests miss.

## Why No Single Test Type Is Enough

A key lesson from a2a-rust is that **no single testing technique — not even all
of them together minus one — is sufficient.** Each layer catches a different
class of bug, and the gaps between layers are where production incidents hide:

| Test type | What it proves | What it cannot prove |
|---|---|---|
| **Unit tests** | Individual functions return correct values | That calling code uses those values correctly |
| **Integration tests** | Components work together pairwise | Multi-hop and emergent system behavior |
| **Property tests** | Invariants hold for all generated inputs | That real-world inputs exercise those invariants |
| **Fuzz tests** | Parser doesn't crash on malformed input | Semantic correctness of valid input handling |
| **E2E dogfooding** | The full stack works under realistic conditions | That your *assertions* actually detect regressions |
| **Mutation tests** | Your assertions detect real code changes | Protocol-level emergent behavior |

**The a2a-rust experience:** After building ~1,630 unit/integration/property/fuzz
tests (with feature flags), an exhaustive E2E dogfood suite that caught 68 real bugs across 13
documented passes, and achieving full green CI — **mutation testing still found gaps.** Tests
that looked comprehensive were silently missing assertions on return values,
boundary conditions, and delegation correctness. The suite was green, but mutants
survived because no test *verified* the specific behavior being mutated.

This is why mutation testing is an important quality signal: it is the only
technique that measures test *effectiveness* rather than test *existence*. Every
other technique answers "does the code work?" — mutation testing answers "would
the tests catch it if the code broke?" In this repo the full sweep runs weekly
(Mondays 03:00 UTC) plus on-demand via the `workflow_dispatch` trigger on
`.github/workflows/mutants.yml`, rather than as a blocking PR gate, because a
full sweep can take 100+ minutes per crate; a separate incremental sweep
(`cargo-mutants --in-diff`) *is* a blocking PR gate, scoped to just the lines
a PR changes. See [CI/CD](./cicd.md#mutation-testing-workflow) for the full
policy.

## Mutation Testing

Mutation testing is the final, critical layer of test quality assurance. While
unit tests verify correctness and fuzz tests find edge cases, mutation testing
answers a fundamentally different question: **do your tests actually detect real
bugs?**

A *mutant* is a small, deliberate code change — replacing `+` with `-`, flipping
`true` to `false`, returning a default value instead of a computed one. If the
test suite still passes after a mutation, there is a gap: a real bug in that
exact location would go undetected.

### Why This Matters at Scale

At multi-data-center deployment scales, the bugs that slip through traditional
testing are precisely the kind that mutation testing catches:

- **Off-by-one errors** in pagination, retry logic, and timeout calculations
- **Swapped operands** in status comparisons (e.g., `==` vs `!=` on task state)
- **Missing boundary checks** where a default return looks plausible
- **Dead code paths** where a branch is never exercised by any test

These are the subtle, semantic correctness issues that only manifest under load,
across network partitions, or during multi-hop agent orchestration — exactly the
conditions that are hardest to reproduce in staging.

### What Mutation Testing Found in a2a-rust

Even with ~1,630 passing tests (with feature flags), 102 E2E dogfood tests on `agent-team`'s default feature set (87 with `--no-default-features`), property tests, and fuzz targets —
all green — the first mutation testing run surfaced gaps across every crate:

- **Delegation methods** returning `()` instead of forwarding calls (e.g.,
  `Arc<T>` metrics delegation, OTel instrument recording)
- **Hash function correctness** — replacing `^=` with `|=` in FNV-1a was
  undetected because no test verified specific hash values
- **Date arithmetic** — swapping `/` with `%` or `*` in HTTP date formatting
  was undetected because the only test used epoch (where all fields are 0)
- **Rate limiter logic** — replacing `>` with `>=` in window checks, `&&` with
  `||` in cleanup conditions, and `/` with `%` in window calculations
- **Builder patterns** — `builder()` returning `Default::default()` instead of
  a functional builder was undetected because existing tests used the built
  result without verifying builder-specific behavior
- **Debug formatting** — `fmt` returning `Ok(Default::default())` (empty string)
  instead of the real debug output
- **Cancellation token** — `is_cancelled()` returning a hardcoded `true` or
  `false` instead of delegating to the actual token

Every one of these mutations represents a real bug that could have been
introduced without any test catching it. The fix in each case was
straightforward: add a test that asserts the specific behavior.

### Running Mutation Tests

```bash
# Install the pinned, patched cargo-mutants and cargo-nextest CI runs
# (one-time setup). Stock cargo-mutants grades fewer mutants: it makes no
# viable body replacement for `*Result` aliases or boxed futures.
scripts/install_cargo_mutants.sh

# A live database. Without one the run does not produce weak results, it
# produces none: two `rate_limit::shared::postgres` tests fail in the
# UNMUTATED tree, cargo-mutants stops before testing a single mutant, and it
# says so as "cargo test failed in an unmutated tree", which reads like a
# broken checkout rather than a missing service.
export A2A_TEST_POSTGRES_URL=postgres://postgres:postgres@localhost:5432/postgres

# What CI actually runs, per crate. Reproduce this exactly before concluding
# anything about a survivor — a narrower invocation measures a smaller
# feature set and reports survivors that a full one kills.
#
# The -E filter is part of "exactly". Both invocations in mutants.yml carry
# it, because --run-ignored all otherwise picks up the soak and swarm-scale
# binaries, whose tests run for 30 to 140 seconds EACH, once per mutant.
cargo mutants -p a2a-protocol-server --test-tool=nextest --profile=mutants \
  -- --all-features --run-ignored all \
     -E 'not (binary(soak) or binary(soak_multi_replica) or binary(swarm_scale))'

# Test a specific file
cargo mutants --file crates/a2a-protocol-types/src/task.rs \
  --test-tool=nextest -- --all-features

# Dry-run: list all mutants without running tests
cargo mutants --list --workspace
```

### Configuration

**There is a `mutants.toml` at the workspace root and cargo-mutants does not
read it.** Measured 2026-08-14 against cargo-mutants 27.1.0, which discovers
`.cargo/mutants.toml` rather than a root-level file. Nothing in it — the
`examine_globs`, the `exclude_globs`, the timeout multiplier and cap — has ever
applied to a run. Two proofs and the consequences are recorded in
[Mutation Testing History](../reference/mutation-history.md); the file's own
header carries them too, and `ROADMAP.md` carries the plan for activating it
(which changes both the scope and the timeout, so it needs a sweep of its own).

Read that as: **generated protobuf code under `crates/*/src/proto/` is mutated
like any other source**, and the per-mutant timeout is cargo-mutants' default
5x the baseline with no cap.

What genuinely configures a sweep is the command line in
`.github/workflows/mutants.yml`, plus one file that *is* read:

```toml
# .config/nextest.toml — nextest reads this normally.
[profile.default]
slow-timeout = { period = "15s", terminate-after = 3 }
```

That kills any single test at 45 seconds. It is not a tuning knob: without it, a
mutant that makes a test *hang* rather than fail wedges the whole run until
cargo-mutants' own timeout, and is reported `TIMEOUT` — a result the workflow's
score counts in neither the numerator nor the denominator. The file's comments
name the mutant that established this and the measurement behind the numbers.

### CI Integration

- **Weekly**: a full mutation sweep runs every Monday at 03:00 UTC, sharded
  across parallel runners (`.github/workflows/mutants.yml`). Any surviving
  mutant fails the build.
- **On-demand**: the same full sweep can also be triggered via
  `workflow_dispatch`, e.g. to re-run it against `main` outside the weekly
  schedule.
- **Every pull request** runs the incremental gate (`--in-diff`): only changed
  source lines are mutated, and any missed mutant fails the PR.

### Interpreting Results

```text
Found 247 mutants to test
 247 caught   ✓     # Test suite detected the mutation
   0 missed   ✗     # ALERT: test gap — add or strengthen tests
   0 timeout  ?     # No verdict — the suite hung rather than answering
   3 unviable ⊘     # Mutation caused compile error (not a gap)
```

- **Caught**: The test suite correctly detected the mutation. Good.
- **Missed**: A real bug in this location would go undetected. Add tests.
- **Unviable**: The mutation produced a compile error. Not a test gap.
- **Timeout**: the run was killed before it finished. Not a pass and not a
  failure — the mutation score is `caught / (caught + missed)`, so a timeout is
  in neither term and the score does not describe it at all. Treat a non-zero
  count as unfinished work, and note that `0 missed` and `0 timeout` are
  different statements: this project has read one as the other and published
  the mistake.

**Target: 100% mutation score** (zero missed mutants across all library crates).

### Fixing Surviving Mutants

When a mutant survives, `cargo mutants` prints the exact source location and
mutation. For example:

```text
MISSED: crates/a2a-protocol-types/src/task.rs:42: replace TaskState::is_terminal -> bool with false
```

This tells you that replacing the body of `is_terminal()` with `false` did not
cause any test to fail. The fix is to add a test that asserts `is_terminal()`
returns `true` for terminal states.

## Performance Benchmarks

The `benches/` directory contains Criterion.rs benchmarks across all 14 suites (see the [benchmark results](../reference/benchmarks.md) page for the current count)
measuring SDK overhead independently of agent logic:

| Suite | Coverage |
|-------|----------|
| Transport Throughput | HTTP round-trip, payload scaling, SSE streaming drain |
| Protocol Overhead | Serde ser/de per A2A type, JSON-RPC envelope, batch scaling, payload scaling (64B-1MB, `to_vec` vs `SerBuffer`, `from_slice` vs `from_str`) |
| Task Lifecycle | TaskStore save/get/list, EventQueue throughput, E2E lifecycle |
| Concurrent Agents | 1–64 parallel sends/streams, store contention, mixed workloads |
| Cross-Language | Standardized workloads reproducible across all A2A SDK languages |
| Realistic Workloads | Multi-turn conversations, interceptor chains, connection reuse |
| Error Paths | Happy vs error path latency ratio, rejection throughput |
| Backpressure | Stream volume scaling, slow consumer, concurrent streams |
| Data Volume | Store ops at 1K–100K tasks, context filtering, history depth |
| Memory Overhead | Heap allocations per operation via counting allocator |
| Enterprise Scenarios | Multi-tenant, push configs, eviction, rate limiting, CORS |
| Production Scenarios | Cold start, reconnection, agent burst, dispatch routing |
| Advanced Scenarios | Tenant resolvers, hot-reload, fan-out, pagination, artifacts |

```bash
# Run all benchmarks
cargo bench -p a2a-benchmarks

# Run a specific suite
cargo bench -p a2a-benchmarks --bench transport_throughput

# Save baseline, make changes, compare for regression detection
./benches/scripts/run_benchmarks.sh --save
# ... make changes ...
./benches/scripts/run_benchmarks.sh --compare
```

Results are auto-published to the [benchmark results page](../reference/benchmarks.md)
in the GH Book via CI. Full HTML reports with violin plots are archived as
CI artifacts.

## Running the Test Suite

> **Current status:** the workspace carries 2,000+ passing tests — run
> `cargo test --workspace --all-features` for the live count.
> across all crates (unit, integration, property, TCK conformance, and E2E dogfood).

```bash
# All tests
cargo test --workspace

# Specific crate
cargo test -p a2a-protocol-server

# With output
cargo test --workspace -- --nocapture

# Specific test
cargo test test_calculator_executor
```

## Next Steps

- **[Production Hardening](./production.md)** — Preparing for deployment
- **[Pitfalls & Lessons Learned](../reference/pitfalls.md)** — Common testing mistakes
