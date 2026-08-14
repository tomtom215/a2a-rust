# Testing Your Agent

a2a-rust makes it easy to test agents at multiple levels: unit testing executors, integration testing with real HTTP, and property-based testing with fuzz targets.

## Unit Testing Executors

Test your executor logic directly by creating a `RequestContext` and mock `EventQueueWriter`:

```rust
use a2a_protocol_sdk::prelude::*;
use a2a_protocol_server::streaming::event_queue::new_in_memory_queue;
use tokio_util::sync::CancellationToken;

#[tokio::test]
async fn test_calculator_executor() {
    let executor = CalcExecutor;

    // Create a writer/reader pair directly for unit testing
    let (writer, mut reader) = new_in_memory_queue();

    // Build the request context
    let ctx = RequestContext {
        task_id: TaskId::new("test-task"),
        context_id: "ctx-1".into(),
        message: Message {
            id: MessageId::new("msg-1"),
            role: MessageRole::User,
            parts: vec![Part::text("3 + 5")],
            task_id: None,
            context_id: None,
            reference_task_ids: None,
            extensions: None,
            metadata: None,
        },
        stored_task: None,
        metadata: None,
        cancellation_token: CancellationToken::new(),
    };

    // Run the executor
    executor.execute(&ctx, &*writer).await.unwrap();

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
```

## Integration Testing with HTTP

Test the full stack by starting a real server and using a client:

```rust
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
    let response = client.send_message(MessageSendParams {
        tenant: None,
        context_id: None,
        message: Message {
            id: MessageId::new("test-msg"),
            role: MessageRole::User,
            parts: vec![Part::text("10 + 20")],
            task_id: None,
            context_id: None,
            reference_task_ids: None,
            extensions: None,
            metadata: None,
        },
        configuration: None,
        metadata: None,
    }).await.unwrap();

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
```

## Testing Both Transports

Run the same tests against both JSON-RPC and REST:

```rust
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
```

## Testing Streaming

```rust
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

```rust
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

Even with ~1,630 passing tests (with feature flags), 81 E2E dogfood tests (94 with all features), property tests, and fuzz targets —
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
# Install cargo-mutants and cargo-nextest (one-time setup)
cargo install cargo-mutants cargo-nextest --locked

# A live database, or every Postgres mutant survives for want of one
# rather than for want of a test:
export A2A_TEST_POSTGRES_URL=postgres://postgres:postgres@localhost:5432/postgres

# What CI actually runs, per crate. Reproduce this exactly before concluding
# anything about a survivor — a narrower invocation measures a smaller
# feature set and reports survivors that a full one kills.
cargo mutants -p a2a-protocol-server --test-tool=nextest --profile=mutants \
  -- --all-features --run-ignored all

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

```
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

```
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
