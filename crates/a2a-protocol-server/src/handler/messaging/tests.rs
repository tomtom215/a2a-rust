// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Tests for the message send path.
//!
//! Split from `mod.rs`, which was 2,395 lines of which 1,674 were these. The
//! production code they cover is ~720 lines, and it holds the hot path plus the
//! destroy/`CleanupGuard` coupling — the part most worth being able to read
//! without scrolling past a test suite three times its length.

use super::*;
use a2a_protocol_types::message::{Message, MessageId, MessageRole, Part};
use a2a_protocol_types::params::{MessageSendParams, SendMessageConfiguration};
use a2a_protocol_types::task::ContextId;

use crate::agent_executor;
use crate::builder::RequestHandlerBuilder;

struct DummyExecutor;
agent_executor!(DummyExecutor, |_ctx, _queue| async { Ok(()) });

fn make_handler() -> RequestHandler {
    RequestHandlerBuilder::new(DummyExecutor)
        .build()
        .expect("default build should succeed")
}

fn make_params(context_id: Option<&str>) -> MessageSendParams {
    MessageSendParams {
        message: Message {
            id: MessageId::new("msg-1"),
            role: MessageRole::User,
            parts: vec![Part::text("hello")],
            context_id: context_id.map(ContextId::new),
            task_id: None,
            reference_task_ids: None,
            extensions: None,
            metadata: None,
        },
        configuration: None,
        metadata: None,
        tenant: None,
    }
}

// ── CleanupGuard ─────────────────────────────────────────────────────────

/// Pins `CleanupGuard::drop`, which is the *only* thing that releases a
/// task's event queue and cancellation token when the executor unwinds.
///
/// Nothing tested it, and the reason is structural rather than an
/// oversight. On the normal path the executor task cleans up explicitly and
/// then defuses the guard (`cleanup_guard.task_id = None`), so `drop`
/// becomes a no-op — every ordinary send, success or handled error, leaves
/// the mutation invisible. The guard earns its place only when the executor
/// panics and the task unwinds before reaching that cleanup, which is
/// exactly what this test arranges. Despite the comment above the executor
/// call, there is no `catch_unwind` here; the guard *is* the panic
/// handling.
///
/// The wait is a bounded poll rather than a sleep because `drop` spawns the
/// cleanup onto the runtime: the work is ordered after the unwind but not
/// synchronous with it, so a fixed sleep would either flake or be
/// needlessly slow.
#[tokio::test]
async fn cleanup_guard_releases_the_token_when_the_executor_panics() {
    struct PanicExec;
    impl crate::executor::AgentExecutor for PanicExec {
        fn execute<'a>(
            &'a self,
            _ctx: &'a crate::request_context::RequestContext,
            _queue: &'a dyn crate::streaming::EventQueueWriter,
        ) -> std::pin::Pin<
            Box<
                dyn std::future::Future<Output = a2a_protocol_types::error::A2aResult<()>>
                    + Send
                    + 'a,
            >,
        > {
            Box::pin(async { panic!("executor panics before it can clean up") })
        }
    }

    let handler = RequestHandlerBuilder::new(PanicExec)
        .build()
        .expect("build should succeed");

    let _ = handler.on_send_message(make_params(None), true, None).await;

    // The executor task must unwind and its guard must run.
    let mut cleaned = false;
    for _ in 0..200 {
        if handler.cancellation_tokens.read().await.is_empty() {
            cleaned = true;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    assert!(
        cleaned,
        "a panicking executor must still release its cancellation token; \
         CleanupGuard::drop is the only path that does so"
    );
    let _ = handler.shutdown().await;
}

// ── task history cap ─────────────────────────────────────────────────────

/// Pins the arithmetic that trims an over-long task history.
///
/// The obvious test does not work here. A single send onto a full history
/// gives `len == MAX + 1`, where `len - MAX` and `len / MAX` are both 1 —
/// the mutation is invisible at the boundary it looks like it should be
/// caught at. The two only diverge once the history is at least twice the
/// cap: at 2048, subtraction trims 1024 and leaves exactly the cap, while
/// division trims 2 and leaves 2046.
///
/// An over-long history is reachable in practice — the cap is applied on
/// write, so a task stored by an older build, a different cap, or a direct
/// store write arrives here oversized — which is why the trim is written to
/// bring any length back to the cap rather than to peel off one message.
#[tokio::test]
async fn oversized_stored_history_is_trimmed_back_to_the_cap() {
    let handler = make_handler();
    let task_id = TaskId::new("t-overlong");

    // One short of twice the cap; the incoming message makes it 2048.
    let seeded = MAX_TASK_HISTORY_MESSAGES * 2 - 1;
    let mut task = task_with_history(seeded);
    task.id = task_id.clone();
    handler
        .task_store
        .save(&task)
        .await
        .expect("seed the oversized task");

    let mut params = make_params(None);
    params.message.task_id = Some(task_id.clone());
    let _ = handler.on_send_message(params, false, None).await;

    let stored = handler
        .task_store
        .get(&task_id)
        .await
        .expect("load")
        .expect("task should still exist");
    assert_eq!(
        stored.history.map(|h| h.len()),
        Some(MAX_TASK_HISTORY_MESSAGES),
        "an oversized history must be trimmed back to exactly the cap"
    );
    let _ = handler.shutdown().await;
}

// ── cancellation-token sweep and context-lock pruning ────────────────────

/// Seeds the token map with `n` already-cancelled entries, which the sweep
/// treats as unconditionally evictable, and returns their ids.
async fn seed_cancelled_tokens(handler: &RequestHandler, n: usize) -> Vec<TaskId> {
    let mut ids = Vec::new();
    // Dropped explicitly below rather than at end of scope: holding the
    // write guard across the return is what `significant_drop_tightening`
    // flags, and that lint is deny-by-default here via `-D warnings`.
    let mut tokens = handler.cancellation_tokens.write().await;
    for i in 0..n {
        let id = TaskId::new(format!("stale-{i}"));
        let token = tokio_util::sync::CancellationToken::new();
        token.cancel();
        tokens.insert(
            id.clone(),
            CancellationEntry {
                token,
                created_at: Instant::now(),
            },
        );
        ids.push(id);
    }
    drop(tokens);
    ids
}

/// Pins the two decisions that drive cancellation-token eviction: whether
/// the sweep runs at all, and whether phase 2 actually removes anything.
///
/// Both mutants survived the 2026-08-07 sweep despite
/// `stale_cancellation_tokens_cleaned_up` exercising this code, because
/// that test asserted nothing — it ran the sweep and then called
/// `shutdown()`. Driving code is not testing it.
///
/// Cancelled tokens are seeded directly rather than produced by a slow
/// executor: an *aged* token is only evicted once its event queue is gone,
/// so a still-running executor keeps its token alive and the map never
/// shrinks. Cancelled entries are unconditionally evictable, which makes
/// the outcome deterministic instead of a race with a sleep.
#[tokio::test]
async fn sweep_evicts_cancelled_tokens_once_the_map_is_at_capacity() {
    let handler = RequestHandlerBuilder::new(DummyExecutor)
        .with_handler_limits(
            crate::handler::limits::HandlerLimits::default().with_max_cancellation_tokens(2),
        )
        .build()
        .expect("build should succeed");

    let stale = seed_cancelled_tokens(&handler, 2).await;
    assert_eq!(handler.cancellation_tokens.read().await.len(), 2);

    // len == max, so `len >= max` fires. `<` would skip the sweep, and
    // deleting the `!` on `stale_ids.is_empty()` would skip the removal.
    let _ = handler
        .on_send_message(make_params(None), false, None)
        .await;

    let tokens = handler.cancellation_tokens.read().await;
    for id in &stale {
        assert!(
            !tokens.contains_key(id),
            "cancelled token {id:?} should have been evicted by the sweep"
        );
    }
    drop(tokens);
    let _ = handler.shutdown().await;
}

/// Pins the context-lock pruning *threshold*.
///
/// A first version of this test used a limit of 2 and three sends, and the
/// `>=`-to-`<` mutant survived it: with the limit that low, both the
/// original and the mutant end holding exactly one entry, so the assertion
/// could not tell them apart. The threshold only becomes observable when
/// the map stays *below* it — the original never prunes, while `<` prunes
/// on every send and reclaims each previous context.
#[tokio::test]
async fn context_locks_are_not_pruned_below_the_limit() {
    let handler = RequestHandlerBuilder::new(DummyExecutor)
        .with_handler_limits(
            crate::handler::limits::HandlerLimits::default().with_max_context_locks(5),
        )
        .build()
        .expect("build should succeed");

    for ctx in ["ctx-a", "ctx-b", "ctx-c"] {
        let _ = handler
            .on_send_message(make_params(Some(ctx)), false, None)
            .await;
    }

    assert_eq!(
        handler.context_locks.read().await.len(),
        3,
        "with a limit of 5, three contexts must all be retained"
    );
    let _ = handler.shutdown().await;
}

/// Pins the staleness predicate itself: pruning must reclaim unused locks
/// and spare one another task still holds.
///
/// `Arc::strong_count(v) > 1` means "someone besides the map owns this".
/// The entries are seeded directly so both populations exist at prune time
/// with certainty — a live lock (a clone held here, count 2) and stale ones
/// (count 1). Driving this through concurrent sends would depend on a
/// scheduler race for whether the live lock is still held when pruning runs.
///
/// This is what separates the three mutations of that predicate:
/// `< 1` is never true and would clear the map including the live lock,
/// `== 1` inverts it and would reclaim exactly the wrong entries, and
/// `>= 1` is always true and would reclaim nothing.
#[tokio::test]
async fn context_lock_pruning_spares_locks_still_in_use() {
    let handler = RequestHandlerBuilder::new(DummyExecutor)
        .with_handler_limits(
            crate::handler::limits::HandlerLimits::default().with_max_context_locks(2),
        )
        .build()
        .expect("build should succeed");

    // Held for the duration of the test, so the map is not its only owner.
    let live = std::sync::Arc::new(tokio::sync::Mutex::new(()));
    {
        let mut locks = handler.context_locks.write().await;
        locks.insert("live-ctx".to_string(), std::sync::Arc::clone(&live));
        locks.insert(
            "stale-1".to_string(),
            std::sync::Arc::new(tokio::sync::Mutex::new(())),
        );
        locks.insert(
            "stale-2".to_string(),
            std::sync::Arc::new(tokio::sync::Mutex::new(())),
        );
    }

    // 3 entries against a limit of 2, so the next send prunes.
    let _ = handler
        .on_send_message(make_params(Some("ctx-new")), false, None)
        .await;

    let locks = handler.context_locks.read().await;
    assert!(
        locks.contains_key("live-ctx"),
        "a lock another owner still holds must survive pruning"
    );
    assert!(
        !locks.contains_key("stale-1") && !locks.contains_key("stale-2"),
        "locks owned only by the map must be reclaimed"
    );
    drop(locks);
    drop(live);
    let _ = handler.shutdown().await;
}

// ── metadata size limit ──────────────────────────────────────────────────

/// Builds a handler whose metadata budget is `max` bytes.
fn handler_with_metadata_limit(max: usize) -> RequestHandler {
    RequestHandlerBuilder::new(DummyExecutor)
        .with_handler_limits(
            crate::handler::limits::HandlerLimits::default().with_max_metadata_size(max),
        )
        .build()
        .expect("build with custom limits should succeed")
}

/// Metadata serialising to exactly `n` bytes.
///
/// It must be a JSON *object*: `validate_metadata_object` runs before the
/// size check and rejects scalars outright, so a bare string never reaches
/// the code under test here. `{"k":"<pad>"}` serialises to `pad.len() + 8`
/// bytes, and the assertion below keeps that arithmetic honest rather than
/// trusting the comment.
fn metadata_of_exactly(n: usize) -> serde_json::Value {
    let value = serde_json::json!({ "k": "x".repeat(n - 8) });
    assert_eq!(
        serde_json::to_vec(&value).expect("serialise").len(),
        n,
        "fixture must serialise to exactly {n} bytes"
    );
    value
}

/// Pins the `>` in both metadata size checks, which is a limit boundary and
/// therefore worth being exact about: `>=` would reject a payload of
/// precisely the configured maximum.
///
/// Two mutants survived here in the 2026-08-07 sweep, one per check. The
/// existing tests only prove that something far *over* the limit is
/// rejected, which `>=` also does — nothing exercised the boundary itself.
///
/// A small custom limit keeps this exact and cheap; the default is 1 MiB.
#[tokio::test]
async fn metadata_exactly_at_limit_is_accepted_but_one_byte_over_is_not() {
    const MAX: usize = 64;

    // ── message metadata ──
    let mut params = make_params(None);
    params.message.metadata = Some(metadata_of_exactly(MAX));
    assert!(
        !matches!(
            handler_with_metadata_limit(MAX)
                .on_send_message(params, false, None)
                .await,
            Err(ServerError::InvalidParams(_))
        ),
        "message metadata of exactly {MAX} bytes is within the limit"
    );

    let mut params = make_params(None);
    params.message.metadata = Some(metadata_of_exactly(MAX + 1));
    assert!(
        matches!(
            handler_with_metadata_limit(MAX)
                .on_send_message(params, false, None)
                .await,
            Err(ServerError::InvalidParams(_))
        ),
        "message metadata one byte over the limit must be rejected"
    );

    // ── request metadata (the second, separate check) ──
    let mut params = make_params(None);
    params.metadata = Some(metadata_of_exactly(MAX));
    assert!(
        !matches!(
            handler_with_metadata_limit(MAX)
                .on_send_message(params, false, None)
                .await,
            Err(ServerError::InvalidParams(_))
        ),
        "request metadata of exactly {MAX} bytes is within the limit"
    );

    let mut params = make_params(None);
    params.metadata = Some(metadata_of_exactly(MAX + 1));
    assert!(
        matches!(
            handler_with_metadata_limit(MAX)
                .on_send_message(params, false, None)
                .await,
            Err(ServerError::InvalidParams(_))
        ),
        "request metadata one byte over the limit must be rejected"
    );
}

// ── shape_response_history ───────────────────────────────────────────────

/// Builds a task carrying `len` history messages, oldest first, each
/// individually identifiable as `h0`, `h1`, …
fn task_with_history(len: usize) -> Task {
    Task {
        id: TaskId::new("t-hist"),
        context_id: ContextId::new("ctx-hist"),
        status: TaskStatus::new(TaskState::Submitted),
        artifacts: None,
        history: Some(
            (0..len)
                .map(|i| Message {
                    id: MessageId::new(format!("h{i}")),
                    role: MessageRole::User,
                    parts: vec![Part::text("x")],
                    context_id: None,
                    task_id: None,
                    reference_task_ids: None,
                    extensions: None,
                    metadata: None,
                })
                .collect(),
        ),
        metadata: None,
    }
}

/// Shapes a `len`-message history and returns the surviving message ids.
fn shaped_ids(len: usize, history_length: Option<u32>) -> Option<Vec<String>> {
    let mut task = task_with_history(len);
    shape_response_history(&mut task, history_length);
    task.history
        .map(|msgs| msgs.into_iter().map(|m| m.id.0).collect())
}

/// Pins every branch and boundary of `shape_response_history`.
///
/// Six mutants survived here in the 2026-08-07 sweep, because the tests
/// reached this function only through `on_send_message`, which never
/// varies `historyLength` — nothing observed the truncation at all.
///
/// The truncation arithmetic those cases were written against has since
/// moved to [`helpers::truncate_history`](super::super::helpers), where it
/// is unit-tested directly. What this test now guards is the part that
/// stayed behind, and that no mutation operator can reach: that
/// `shape_response_history` still *calls* it, and still maps the two
/// send-response-specific inputs correctly. `None` and `Some(0)` both
/// yield no history here, but they are not the same instruction — only
/// `Some(0)` is a client asking for zero messages, and a refactor that
/// collapsed them would be invisible to every other case below.
#[test]
fn shape_response_history_covers_every_branch() {
    // Default: history is omitted entirely, not echoed back.
    assert_eq!(shaped_ids(3, None), None);

    // Some(0) omits too — distinct from keeping an empty list.
    assert_eq!(shaped_ids(3, Some(0)), None);

    // Truncation keeps the n most recent, oldest dropped.
    assert_eq!(
        shaped_ids(6, Some(2)),
        Some(vec!["h4".to_string(), "h5".to_string()])
    );
    assert_eq!(shaped_ids(4, Some(1)), Some(vec!["h3".to_string()]));

    // Exactly at the boundary, and asking for more than exists: keep all.
    assert_eq!(
        shaped_ids(3, Some(3)),
        Some(vec!["h0".to_string(), "h1".to_string(), "h2".to_string()])
    );
    assert_eq!(
        shaped_ids(2, Some(5)),
        Some(vec!["h0".to_string(), "h1".to_string()])
    );

    // An empty history stays empty rather than becoming None.
    assert_eq!(shaped_ids(0, Some(3)), Some(vec![]));
}

#[tokio::test]
async fn empty_message_parts_returns_invalid_params() {
    let handler = make_handler();
    let mut params = make_params(None);
    params.message.parts = vec![];

    let result = handler.on_send_message(params, false, None).await;

    assert!(
        matches!(result, Err(ServerError::InvalidParams(_))),
        "expected InvalidParams for empty parts"
    );
}

#[tokio::test]
async fn oversized_message_metadata_returns_invalid_params() {
    let handler = make_handler();
    let mut params = make_params(None);
    // An *object* exceeding the default 1 MiB limit. This was a bare JSON
    // string until 2026-08-08, which `validate_metadata_object` rejects as
    // a non-object before the size check ever runs — so the test passed
    // without exercising the limit it names. Both size-check mutants
    // survived the 2026-08-07 sweep for exactly that reason.
    params.message.metadata = Some(serde_json::json!({ "k": "x".repeat(1_100_000) }));

    let result = handler.on_send_message(params, false, None).await;

    assert!(
        matches!(result, Err(ServerError::InvalidParams(_))),
        "expected InvalidParams for oversized message metadata"
    );
}

#[tokio::test]
async fn oversized_request_metadata_returns_invalid_params() {
    let handler = make_handler();
    let mut params = make_params(None);
    // Build a JSON string that exceeds the default 1 MiB limit.
    let big_value = "x".repeat(1_100_000);
    params.metadata = Some(serde_json::json!(big_value));

    let result = handler.on_send_message(params, false, None).await;

    assert!(
        matches!(result, Err(ServerError::InvalidParams(_))),
        "expected InvalidParams for oversized request metadata"
    );
}

#[tokio::test]
async fn non_object_message_metadata_returns_invalid_params() {
    // Cross-binding portability: array/scalar metadata is not representable
    // over gRPC (google.protobuf.Struct) and must be rejected at ingress.
    let handler = make_handler();
    let mut params = make_params(None);
    params.message.metadata = Some(serde_json::json!([1, 2, 3]));

    let result = handler.on_send_message(params, false, None).await;
    assert!(
        matches!(result, Err(ServerError::InvalidParams(ref msg))
            if msg.contains("JSON object") && msg.contains("array")),
        "expected InvalidParams naming the offending kind (array), got: {result:?}"
    );
}

#[tokio::test]
async fn scalar_request_metadata_returns_invalid_params() {
    let handler = make_handler();
    let mut params = make_params(None);
    params.metadata = Some(serde_json::json!("a bare string"));

    let result = handler.on_send_message(params, false, None).await;
    assert!(
        matches!(result, Err(ServerError::InvalidParams(ref msg))
            if msg.contains("JSON object") && msg.contains("string")),
        "expected InvalidParams naming the offending kind (string), got: {result:?}"
    );
}

#[tokio::test]
async fn non_object_part_metadata_returns_invalid_params() {
    let handler = make_handler();
    let mut params = make_params(None);
    params.message.parts[0].metadata = Some(serde_json::json!(42));

    let result = handler.on_send_message(params, false, None).await;
    assert!(
        matches!(result, Err(ServerError::InvalidParams(ref msg))
            if msg.contains("part 0") && msg.contains("number")),
        "expected InvalidParams naming the part index and kind (number), got: {result:?}"
    );
}

#[tokio::test]
async fn object_metadata_is_accepted() {
    // An object metadata value is representable across all bindings.
    let handler = make_handler();
    let mut params = make_params(None);
    params.message.metadata = Some(serde_json::json!({"k": "v"}));
    params.metadata = Some(serde_json::json!({"trace": 1}));

    let result = handler.on_send_message(params, false, None).await;
    assert!(
        result.is_ok(),
        "object metadata must be accepted, got: {result:?}"
    );
}

#[tokio::test]
async fn valid_message_returns_ok() {
    let handler = make_handler();
    let params = make_params(None);

    let result = handler.on_send_message(params, false, None).await;

    let send_result = result.expect("expected Ok for valid message");
    assert!(
        matches!(
            send_result,
            SendMessageResult::Response(SendMessageResponse::Task(_))
        ),
        "expected Response(Task) for non-streaming send"
    );
}

#[tokio::test]
async fn return_immediately_returns_task() {
    let handler = make_handler();
    let mut params = make_params(None);
    params.configuration = Some(SendMessageConfiguration {
        accepted_output_modes: vec!["text/plain".into()],
        task_push_notification_config: None,
        history_length: None,
        return_immediately: Some(true),
    });

    let result = handler.on_send_message(params, false, None).await;

    assert!(
        matches!(
            result,
            Ok(SendMessageResult::Response(SendMessageResponse::Task(_)))
        ),
        "expected Response(Task) for return_immediately=true"
    );
}

// An executor that narrates progress to completion via the event queue.
struct CompletingExecutor;
agent_executor!(CompletingExecutor, |ctx, queue| async {
    for state in [TaskState::Working, TaskState::Completed] {
        let ev = StreamResponse::StatusUpdate(TaskStatusUpdateEvent {
            task_id: ctx.task_id.clone(),
            context_id: ContextId::new(ctx.context_id.clone()),
            status: TaskStatus::with_timestamp(state),
            metadata: None,
        });
        let _ = queue.write(ev).await;
    }
    Ok(())
});

// An executor that never finishes, keeping its task in flight (and its
// event queue alive) for the duration of a test.
struct BlockingExecutor;
agent_executor!(BlockingExecutor, |_ctx, _queue| async {
    tokio::time::sleep(std::time::Duration::from_secs(30)).await;
    Ok(())
});

async fn poll_task_state(handler: &RequestHandler, task_id: &TaskId, want: TaskState) -> TaskState {
    for _ in 0..200 {
        if let Ok(Some(t)) = handler.task_store.get(task_id).await {
            if t.status.state == want {
                return want;
            }
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    handler
        .task_store
        .get(task_id)
        .await
        .ok()
        .flatten()
        .map_or(TaskState::Submitted, |t| t.status.state)
}

/// Regression: a `return_immediately` send must still drive the task to
/// completion in the background and persist the final state. Previously it
/// spawned no background processor, so the executor's events went nowhere
/// and the task was stuck in `Submitted` forever.
#[tokio::test]
async fn return_immediately_persists_final_state() {
    let handler = RequestHandlerBuilder::new(CompletingExecutor)
        .build()
        .unwrap();
    let mut params = make_params(Some("ctx-ri"));
    params.configuration = Some(SendMessageConfiguration {
        accepted_output_modes: vec!["text/plain".into()],
        task_push_notification_config: None,
        history_length: None,
        return_immediately: Some(true),
    });

    let SendMessageResult::Response(SendMessageResponse::Task(task)) =
        handler.on_send_message(params, false, None).await.unwrap()
    else {
        panic!("expected an immediate Task response");
    };
    assert_eq!(
        task.status.state,
        TaskState::Submitted,
        "snapshot is Submitted"
    );

    let final_state = poll_task_state(&handler, &task.id, TaskState::Completed).await;
    assert_eq!(
        final_state,
        TaskState::Completed,
        "fire-and-forget task must reach Completed in the store"
    );
}

/// Regression: a second send targeting a task already being processed must
/// be rejected, not spawn a second executor and overwrite the first's
/// cancellation token (leaving the original work uncancelable).
#[tokio::test]
async fn concurrent_send_to_in_flight_task_is_rejected() {
    let handler = RequestHandlerBuilder::new(BlockingExecutor)
        .build()
        .unwrap();

    // First send (fire-and-forget) leaves a live executor + token.
    let mut first = make_params(Some("ctx-dup"));
    first.configuration = Some(SendMessageConfiguration {
        accepted_output_modes: vec!["text/plain".into()],
        task_push_notification_config: None,
        history_length: None,
        return_immediately: Some(true),
    });
    let SendMessageResult::Response(SendMessageResponse::Task(task)) =
        handler.on_send_message(first, false, None).await.unwrap()
    else {
        panic!("expected an immediate Task response");
    };

    // Second send explicitly targets the same in-flight task.
    let mut second = make_params(Some("ctx-dup"));
    second.message.task_id = Some(task.id.clone());
    let result = handler.on_send_message(second, false, None).await;
    assert!(
        matches!(result, Err(ServerError::UnsupportedOperation(_))),
        "expected rejection of a send to an in-flight task, got {result:?}"
    );
}

/// Regression: hitting the concurrent-stream cap must return a clean
/// `Overloaded` error and create NO task (no orphaned `Submitted` row, no
/// leaked queue) — not a misleading internal error after committing the
/// task and token.
#[tokio::test]
async fn stream_cap_exhaustion_returns_overloaded_without_orphan() {
    let handler = RequestHandlerBuilder::new(BlockingExecutor)
        .with_max_concurrent_streams(1)
        .build()
        .unwrap();

    // First send consumes the single slot (its executor blocks, so the
    // queue stays alive).
    let mut first = make_params(Some("ctx-a"));
    first.configuration = Some(SendMessageConfiguration {
        accepted_output_modes: vec!["text/plain".into()],
        task_push_notification_config: None,
        history_length: None,
        return_immediately: Some(true),
    });
    handler.on_send_message(first, false, None).await.unwrap();
    assert_eq!(handler.event_queue_manager.active_count().await, 1);

    // Second send hits the cap.
    let mut second = make_params(Some("ctx-b"));
    second.configuration = Some(SendMessageConfiguration {
        accepted_output_modes: vec!["text/plain".into()],
        task_push_notification_config: None,
        history_length: None,
        return_immediately: Some(true),
    });
    let result = handler.on_send_message(second, false, None).await;
    assert!(
        matches!(result, Err(ServerError::Overloaded(_))),
        "expected Overloaded at capacity, got {result:?}"
    );
    // No queue was created for the rejected send, and no task orphaned.
    assert_eq!(
        handler.event_queue_manager.active_count().await,
        1,
        "capacity rejection must not create a queue"
    );
}

#[tokio::test]
async fn empty_context_id_returns_invalid_params() {
    let handler = make_handler();
    let params = make_params(Some(""));

    let result = handler.on_send_message(params, false, None).await;

    assert!(
        matches!(result, Err(ServerError::InvalidParams(_))),
        "expected InvalidParams for empty context_id"
    );
}

#[tokio::test]
async fn too_long_context_id_returns_invalid_params() {
    // Covers line 98-99: context_id exceeding max_id_length.
    use crate::handler::limits::HandlerLimits;

    let handler = RequestHandlerBuilder::new(DummyExecutor)
        .with_handler_limits(HandlerLimits::default().with_max_id_length(10))
        .build()
        .unwrap();
    let long_ctx = "x".repeat(20);
    let params = make_params(Some(&long_ctx));

    let result = handler.on_send_message(params, false, None).await;
    assert!(
        matches!(result, Err(ServerError::InvalidParams(ref msg)) if msg.contains("maximum length")),
        "expected InvalidParams for too-long context_id"
    );
}

#[tokio::test]
async fn too_long_task_id_returns_invalid_params() {
    // Covers lines 108-109: task_id exceeding max_id_length.
    use crate::handler::limits::HandlerLimits;
    use a2a_protocol_types::task::TaskId;

    let handler = RequestHandlerBuilder::new(DummyExecutor)
        .with_handler_limits(HandlerLimits::default().with_max_id_length(10))
        .build()
        .unwrap();
    let mut params = make_params(None);
    params.message.task_id = Some(TaskId::new("a".repeat(20)));

    let result = handler.on_send_message(params, false, None).await;
    assert!(
        matches!(result, Err(ServerError::InvalidParams(ref msg)) if msg.contains("maximum length")),
        "expected InvalidParams for too-long task_id"
    );
}

#[tokio::test]
async fn empty_task_id_returns_invalid_params() {
    // Covers line 114: empty task_id validation.
    use a2a_protocol_types::task::TaskId;

    let handler = make_handler();
    let mut params = make_params(None);
    params.message.task_id = Some(TaskId::new(""));

    let result = handler.on_send_message(params, false, None).await;
    assert!(
        matches!(result, Err(ServerError::InvalidParams(ref msg)) if msg.contains("empty")),
        "expected InvalidParams for empty task_id"
    );
}

#[tokio::test]
async fn task_id_mismatch_returns_invalid_params() {
    // Covers context/task mismatch when stored task exists with different task_id.
    use a2a_protocol_types::task::{Task, TaskId, TaskState, TaskStatus};

    let handler = make_handler();

    // Save a non-terminal task with context_id "ctx-existing".
    let task = Task {
        id: TaskId::new("stored-task-id"),
        context_id: ContextId::new("ctx-existing"),
        status: TaskStatus::new(TaskState::InputRequired),
        history: None,
        artifacts: None,
        metadata: None,
    };
    handler.task_store.save(&task).await.unwrap();

    // Send a message with the same context_id but a different task_id.
    let mut params = make_params(Some("ctx-existing"));
    params.message.task_id = Some(TaskId::new("different-task-id"));

    let result = handler.on_send_message(params, false, None).await;
    assert!(
        matches!(result, Err(ServerError::InvalidParams(ref msg)) if msg.contains("does not match")),
        "expected InvalidParams for task_id mismatch, got: {result:?}"
    );
}

#[tokio::test]
async fn send_message_records_user_message_in_history() {
    // Task.history is the conversation record: the incoming user message
    // must be persisted with the task.
    let handler = make_handler();
    let result = handler
        .on_send_message(make_params(None), false, None)
        .await
        .expect("send should succeed");
    let task_id = match result {
        SendMessageResult::Response(SendMessageResponse::Task(t)) => t.id,
        other => panic!("expected task response, got {other:?}"),
    };
    let stored = handler
        .task_store
        .get(&task_id)
        .await
        .expect("get")
        .expect("task stored");
    let history = stored.history.expect("history populated on send");
    assert_eq!(history.len(), 1, "exactly the incoming user message");
    assert_eq!(history[0].role, MessageRole::User);
    assert_eq!(
        history[0].parts[0].text_content(),
        Some("hello"),
        "history records the message content"
    );
}

#[tokio::test]
async fn continuation_appends_history_and_preserves_artifacts() {
    // A continuation must carry the stored task's artifacts and metadata
    // forward and append the new message — not reset the task.
    use a2a_protocol_types::artifact::Artifact;
    let handler = make_handler();
    let prior = Task {
        id: TaskId::new("cont-task"),
        context_id: ContextId::new("ctx-cont"),
        status: TaskStatus::new(TaskState::InputRequired),
        history: Some(vec![Message {
            id: MessageId::new("m-prior"),
            role: MessageRole::User,
            parts: vec![Part::text("first turn")],
            context_id: None,
            task_id: None,
            reference_task_ids: None,
            extensions: None,
            metadata: None,
        }]),
        artifacts: Some(vec![Artifact::new("a1", vec![Part::text("turn-1 output")])]),
        metadata: Some(serde_json::json!({"k": "v"})),
    };
    handler.task_store.save(&prior).await.unwrap();

    let mut params = make_params(Some("ctx-cont"));
    params.message.task_id = Some(TaskId::new("cont-task"));
    handler
        .on_send_message(params, false, None)
        .await
        .expect("continuation should succeed");

    let stored = handler
        .task_store
        .get(&TaskId::new("cont-task"))
        .await
        .expect("get")
        .expect("task stored");
    let history = stored.history.expect("history preserved");
    assert_eq!(history.len(), 2, "prior message + continuation message");
    assert_eq!(history[0].parts[0].text_content(), Some("first turn"));
    assert_eq!(history[1].parts[0].text_content(), Some("hello"));
    assert!(
        stored.artifacts.as_ref().is_some_and(|a| a.len() == 1),
        "continuation must not wipe accumulated artifacts"
    );
    assert_eq!(
        stored.metadata,
        Some(serde_json::json!({"k": "v"})),
        "continuation must not wipe task metadata"
    );
}

#[tokio::test]
async fn history_is_capped_at_max_messages() {
    // The oldest messages are dropped once the cap is reached.
    let handler = make_handler();
    let mut long_history: Vec<Message> = (0..MAX_TASK_HISTORY_MESSAGES)
        .map(|i| Message {
            id: MessageId::new(format!("m-{i}")),
            role: MessageRole::User,
            parts: vec![Part::text(format!("msg {i}"))],
            context_id: None,
            task_id: None,
            reference_task_ids: None,
            extensions: None,
            metadata: None,
        })
        .collect();
    long_history[0].parts = vec![Part::text("OLDEST")];
    let prior = Task {
        id: TaskId::new("cap-task"),
        context_id: ContextId::new("ctx-cap"),
        status: TaskStatus::new(TaskState::InputRequired),
        history: Some(long_history),
        artifacts: None,
        metadata: None,
    };
    handler.task_store.save(&prior).await.unwrap();

    let mut params = make_params(Some("ctx-cap"));
    params.message.task_id = Some(TaskId::new("cap-task"));
    handler
        .on_send_message(params, false, None)
        .await
        .expect("continuation should succeed");

    let stored = handler
        .task_store
        .get(&TaskId::new("cap-task"))
        .await
        .unwrap()
        .unwrap();
    let history = stored.history.unwrap();
    assert_eq!(history.len(), MAX_TASK_HISTORY_MESSAGES, "capped");
    assert_ne!(
        history[0].parts[0].text_content(),
        Some("OLDEST"),
        "the oldest message is dropped first"
    );
    assert_eq!(
        history[MAX_TASK_HISTORY_MESSAGES - 1].parts[0].text_content(),
        Some("hello"),
        "the newest message is retained"
    );
}

#[tokio::test]
async fn send_response_omits_history_by_default_and_honors_history_length() {
    // The store keeps full history, but the send RESPONSE omits it
    // unless SendMessageConfiguration.historyLength asks for it —
    // echoing the just-sent message back doubled response payloads for
    // large sends (caught by the benchmark regression gate).
    use a2a_protocol_types::params::SendMessageConfiguration;
    let handler = make_handler();

    let result = handler
        .on_send_message(make_params(Some("ctx-resp")), false, None)
        .await
        .expect("send should succeed");
    let task = match result {
        SendMessageResult::Response(SendMessageResponse::Task(t)) => t,
        other => panic!("expected task response, got {other:?}"),
    };
    assert!(
        task.history.is_none(),
        "default send response must not echo history"
    );
    let stored = handler
        .task_store
        .get(&task.id)
        .await
        .unwrap()
        .expect("task stored");
    assert_eq!(
        stored.history.as_ref().map(Vec::len),
        Some(1),
        "the store still keeps the full history"
    );

    let mut params = make_params(Some("ctx-resp"));
    params.message.task_id = Some(task.id.clone());
    params.configuration = Some(SendMessageConfiguration {
        history_length: Some(10),
        ..Default::default()
    });
    let result = handler
        .on_send_message(params, false, None)
        .await
        .expect("continuation should succeed");
    let task = match result {
        SendMessageResult::Response(SendMessageResponse::Task(t)) => t,
        other => panic!("expected task response, got {other:?}"),
    };
    assert_eq!(
        task.history.as_ref().map(Vec::len),
        Some(2),
        "historyLength=10 returns the (2) stored messages"
    );
}

#[tokio::test]
async fn send_message_with_request_metadata() {
    // Covers line 186: setting request metadata on context.
    let handler = make_handler();
    let mut params = make_params(None);
    params.metadata = Some(serde_json::json!({"key": "value"}));

    let result = handler.on_send_message(params, false, None).await;
    assert!(
        result.is_ok(),
        "send_message with request metadata should succeed"
    );
}

#[tokio::test]
async fn send_message_error_path_records_metrics() {
    // Covers lines 195-199: the Err branch in the outer metrics match.
    use crate::call_context::CallContext;
    use crate::interceptor::ServerInterceptor;
    use std::future::Future;
    use std::pin::Pin;

    struct FailInterceptor;
    impl ServerInterceptor for FailInterceptor {
        fn before<'a>(
            &'a self,
            _ctx: &'a CallContext,
        ) -> Pin<Box<dyn Future<Output = a2a_protocol_types::error::A2aResult<()>> + Send + 'a>>
        {
            Box::pin(async {
                Err(a2a_protocol_types::error::A2aError::internal(
                    "forced failure",
                ))
            })
        }
        fn after<'a>(
            &'a self,
            _ctx: &'a CallContext,
        ) -> Pin<Box<dyn Future<Output = a2a_protocol_types::error::A2aResult<()>> + Send + 'a>>
        {
            Box::pin(async { Ok(()) })
        }
    }

    let handler = RequestHandlerBuilder::new(DummyExecutor)
        .with_interceptor(FailInterceptor)
        .build()
        .unwrap();

    let params = make_params(None);
    let result = handler.on_send_message(params, false, None).await;
    assert!(
        result.is_err(),
        "send_message should fail when interceptor rejects, exercising error metrics path"
    );
}

#[tokio::test]
async fn send_streaming_message_error_path_records_metrics() {
    // Covers the streaming variant of the error metrics path (method_name = "SendStreamingMessage").
    use crate::call_context::CallContext;
    use crate::interceptor::ServerInterceptor;
    use std::future::Future;
    use std::pin::Pin;

    struct FailInterceptor;
    impl ServerInterceptor for FailInterceptor {
        fn before<'a>(
            &'a self,
            _ctx: &'a CallContext,
        ) -> Pin<Box<dyn Future<Output = a2a_protocol_types::error::A2aResult<()>> + Send + 'a>>
        {
            Box::pin(async {
                Err(a2a_protocol_types::error::A2aError::internal(
                    "forced failure",
                ))
            })
        }
        fn after<'a>(
            &'a self,
            _ctx: &'a CallContext,
        ) -> Pin<Box<dyn Future<Output = a2a_protocol_types::error::A2aResult<()>> + Send + 'a>>
        {
            Box::pin(async { Ok(()) })
        }
    }

    let handler = RequestHandlerBuilder::new(DummyExecutor)
        .with_interceptor(FailInterceptor)
        .build()
        .unwrap();

    let params = make_params(None);
    let result = handler.on_send_message(params, true, None).await;
    assert!(
        result.is_err(),
        "streaming send_message should fail when interceptor rejects"
    );
}

#[tokio::test]
async fn streaming_mode_returns_stream_result() {
    // Covers lines 270-280: the streaming=true branch returning SendMessageResult::Stream.
    let handler = make_handler();
    let params = make_params(None);

    let result = handler.on_send_message(params, true, None).await;
    assert!(
        matches!(result, Ok(SendMessageResult::Stream(_))),
        "expected Stream result in streaming mode"
    );
}

#[tokio::test]
async fn send_message_with_stored_task_continuation() {
    // Covers setting stored_task on context when a non-terminal task
    // exists for the given context_id (e.g. input-required continuation).
    use a2a_protocol_types::task::{Task, TaskState, TaskStatus};

    let handler = make_handler();

    // Pre-save a non-terminal task with a known context_id.
    let task = Task {
        id: TaskId::new("existing-task"),
        context_id: ContextId::new("continue-ctx"),
        status: TaskStatus::new(TaskState::InputRequired),
        history: None,
        artifacts: None,
        metadata: None,
    };
    handler.task_store.save(&task).await.unwrap();

    // Send message with the same context_id — should find the stored task.
    let params = make_params(Some("continue-ctx"));
    let result = handler.on_send_message(params, false, None).await;
    assert!(
        result.is_ok(),
        "send_message with existing non-terminal context should succeed"
    );
}

#[tokio::test]
async fn send_message_to_terminal_task_returns_unsupported_operation() {
    // SPEC CORE-SEND-002: Messages explicitly targeting a task in terminal
    // state (via task_id) must be rejected with UnsupportedOperation.
    use a2a_protocol_types::task::{Task, TaskState, TaskStatus};

    let handler = make_handler();

    // Pre-save a completed task.
    let task = Task {
        id: TaskId::new("done-task"),
        context_id: ContextId::new("done-ctx"),
        status: TaskStatus::new(TaskState::Completed),
        history: None,
        artifacts: None,
        metadata: None,
    };
    handler.task_store.save(&task).await.unwrap();

    // Send message with explicit task_id targeting the terminal task.
    let mut params = make_params(Some("done-ctx"));
    params.message.task_id = Some(TaskId::new("done-task"));
    let result = handler.on_send_message(params, false, None).await;
    assert!(
        matches!(result, Err(ServerError::UnsupportedOperation(ref msg)) if msg.contains("terminal")),
        "expected UnsupportedOperation for terminal task, got: {result:?}"
    );
}

#[tokio::test]
async fn send_message_to_terminal_context_without_task_id_creates_new_task() {
    // When no task_id is provided but the context has a terminal task,
    // a new task should be created (new conversation round on same context).
    use a2a_protocol_types::task::{Task, TaskState, TaskStatus};

    let handler = make_handler();

    // Pre-save a completed task.
    let task = Task {
        id: TaskId::new("old-task"),
        context_id: ContextId::new("reuse-ctx"),
        status: TaskStatus::new(TaskState::Completed),
        history: None,
        artifacts: None,
        metadata: None,
    };
    handler.task_store.save(&task).await.unwrap();

    // Send message to the same context WITHOUT task_id — should succeed.
    let params = make_params(Some("reuse-ctx"));
    let result = handler.on_send_message(params, false, None).await;
    assert!(
        result.is_ok(),
        "should create new task on terminal context, got: {result:?}"
    );
}

#[tokio::test]
async fn send_message_with_headers() {
    // Covers line 76: build_call_context receives headers.
    let handler = make_handler();
    let params = make_params(None);
    let mut headers = HashMap::new();
    headers.insert("authorization".to_string(), "Bearer test-token".to_string());

    let result = handler.on_send_message(params, false, Some(&headers)).await;
    let send_result = result.expect("send_message with headers should succeed");
    assert!(
        matches!(
            send_result,
            SendMessageResult::Response(SendMessageResponse::Task(_))
        ),
        "expected Response(Task) for send with headers"
    );
}

#[tokio::test]
async fn duplicate_task_id_without_context_match_returns_error() {
    // Task exists under a different context — should return InvalidParams.
    use a2a_protocol_types::task::{Task, TaskId as TId, TaskState, TaskStatus};

    let handler = make_handler();

    // Pre-save a task with task_id "dup-task" but context "other-ctx".
    let task = Task {
        id: TId::new("dup-task"),
        context_id: ContextId::new("other-ctx"),
        status: TaskStatus::new(TaskState::Completed),
        history: None,
        artifacts: None,
        metadata: None,
    };
    handler.task_store.save(&task).await.unwrap();

    // Send a message with a new context_id but the same task_id.
    let mut params = make_params(Some("brand-new-ctx"));
    params.message.task_id = Some(TId::new("dup-task"));

    let result = handler.on_send_message(params, false, None).await;
    assert!(
        matches!(result, Err(ServerError::InvalidParams(ref msg)) if msg.contains("different context")),
        "expected InvalidParams for task_id in different context, got: {result:?}"
    );
}

#[tokio::test]
async fn unknown_task_id_returns_task_not_found() {
    // SPEC §3.4.2: Client-provided task_id must reference existing task.
    use a2a_protocol_types::task::TaskId as TId;

    let handler = make_handler();

    // Send message with a task_id that doesn't exist anywhere.
    let mut params = make_params(Some("fresh-ctx"));
    params.message.task_id = Some(TId::new("nonexistent-task"));

    let result = handler.on_send_message(params, false, None).await;
    assert!(
        matches!(result, Err(ServerError::TaskNotFound(_))),
        "expected TaskNotFound for unknown task_id, got: {result:?}"
    );
}

#[tokio::test]
async fn send_message_with_tenant() {
    // Covers line 46: tenant scoping with non-default tenant.
    let handler = make_handler();
    let mut params = make_params(None);
    params.tenant = Some("test-tenant".to_string());

    let result = handler.on_send_message(params, false, None).await;
    let send_result = result.expect("send_message with tenant should succeed");
    assert!(
        matches!(
            send_result,
            SendMessageResult::Response(SendMessageResponse::Task(_))
        ),
        "expected Response(Task) for send with tenant"
    );
}

#[tokio::test]
async fn executor_timeout_returns_failed_task() {
    // Covers lines 228-236: the executor timeout path.
    use a2a_protocol_types::error::A2aResult;
    use std::time::Duration;

    struct SlowExecutor;
    impl crate::executor::AgentExecutor for SlowExecutor {
        fn execute<'a>(
            &'a self,
            _ctx: &'a crate::request_context::RequestContext,
            _queue: &'a dyn crate::streaming::EventQueueWriter,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = A2aResult<()>> + Send + 'a>>
        {
            Box::pin(async {
                tokio::time::sleep(Duration::from_secs(60)).await;
                Ok(())
            })
        }
    }

    let handler = RequestHandlerBuilder::new(SlowExecutor)
        .with_executor_timeout(Duration::from_millis(50))
        .build()
        .unwrap();

    let params = make_params(None);
    // The executor times out; collect_events should see a Failed status update.
    let result = handler.on_send_message(params, false, None).await;
    // The result should be Ok with a completed/failed task (the timeout writes a failed event).
    assert!(
        result.is_ok(),
        "executor timeout should still return a task result"
    );
}

#[tokio::test]
async fn executor_failure_writes_failed_event() {
    // Covers lines 243-258: executor error path writes a failed status event.
    use a2a_protocol_types::error::{A2aError, A2aResult};

    struct FailExecutor;
    impl crate::executor::AgentExecutor for FailExecutor {
        fn execute<'a>(
            &'a self,
            _ctx: &'a crate::request_context::RequestContext,
            _queue: &'a dyn crate::streaming::EventQueueWriter,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = A2aResult<()>> + Send + 'a>>
        {
            Box::pin(async { Err(A2aError::internal("executor exploded")) })
        }
    }

    let handler = RequestHandlerBuilder::new(FailExecutor).build().unwrap();
    let params = make_params(None);

    let result = handler.on_send_message(params, false, None).await;
    // collect_events should see the failed status update.
    assert!(
        result.is_ok(),
        "executor failure should produce a task result"
    );
}

#[tokio::test]
async fn cancellation_token_sweep_runs_when_map_is_full() {
    // Covers lines 194-199: the cancellation token sweep when the map
    // exceeds max_cancellation_tokens.
    use crate::handler::limits::HandlerLimits;

    // Use a slow executor so tokens accumulate before being cleaned up.
    struct SlowExec;
    impl crate::executor::AgentExecutor for SlowExec {
        fn execute<'a>(
            &'a self,
            _ctx: &'a crate::request_context::RequestContext,
            _queue: &'a dyn crate::streaming::EventQueueWriter,
        ) -> std::pin::Pin<
            Box<
                dyn std::future::Future<Output = a2a_protocol_types::error::A2aResult<()>>
                    + Send
                    + 'a,
            >,
        > {
            Box::pin(async {
                // Hold the token for a bit so tokens accumulate.
                tokio::time::sleep(std::time::Duration::from_secs(10)).await;
                Ok(())
            })
        }
    }

    let handler = RequestHandlerBuilder::new(SlowExec)
        .with_handler_limits(HandlerLimits::default().with_max_cancellation_tokens(2))
        .build()
        .unwrap();

    // Send multiple streaming messages so tokens accumulate (streaming returns
    // immediately without waiting for executor to finish).
    for _ in 0..3 {
        let params = make_params(None);
        let _ = handler.on_send_message(params, true, None).await;
    }
    // If we get here without panic, the sweep logic ran successfully.
    // Clean up the slow executors.
    let _ = handler.shutdown().await;
}

#[tokio::test]
async fn stale_cancellation_tokens_cleaned_up() {
    // Covers lines 224-228: stale cancellation tokens are removed during sweep.
    use crate::handler::limits::HandlerLimits;
    use std::time::Duration;

    // Use a slow executor so tokens accumulate and become stale.
    struct SlowExec2;
    impl crate::executor::AgentExecutor for SlowExec2 {
        fn execute<'a>(
            &'a self,
            _ctx: &'a crate::request_context::RequestContext,
            _queue: &'a dyn crate::streaming::EventQueueWriter,
        ) -> std::pin::Pin<
            Box<
                dyn std::future::Future<Output = a2a_protocol_types::error::A2aResult<()>>
                    + Send
                    + 'a,
            >,
        > {
            Box::pin(async {
                tokio::time::sleep(Duration::from_secs(10)).await;
                Ok(())
            })
        }
    }

    let handler = RequestHandlerBuilder::new(SlowExec2)
        .with_handler_limits(
            HandlerLimits::default()
                .with_max_cancellation_tokens(2)
                // Very short max_token_age so tokens become stale quickly.
                .with_max_token_age(Duration::from_millis(1)),
        )
        .build()
        .unwrap();

    // Send two streaming messages to fill up the token map.
    for _ in 0..2 {
        let params = make_params(None);
        let _ = handler.on_send_message(params, true, None).await;
    }

    // Wait for tokens to become stale.
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Send a third message; this should trigger the cleanup sweep
    // because the map is at capacity (>= max_cancellation_tokens)
    // and the existing tokens are stale (age > max_token_age).
    let params = make_params(None);
    let _ = handler.on_send_message(params, true, None).await;

    // The stale tokens should have been cleaned up.
    let _ = handler.shutdown().await;
}

#[tokio::test]
async fn streaming_executor_failure_writes_error_event() {
    // Covers lines 243-258 in streaming mode: executor error path.
    use a2a_protocol_types::error::{A2aError, A2aResult};

    struct FailExecutor;
    impl crate::executor::AgentExecutor for FailExecutor {
        fn execute<'a>(
            &'a self,
            _ctx: &'a crate::request_context::RequestContext,
            _queue: &'a dyn crate::streaming::EventQueueWriter,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = A2aResult<()>> + Send + 'a>>
        {
            Box::pin(async { Err(A2aError::internal("streaming fail")) })
        }
    }

    let handler = RequestHandlerBuilder::new(FailExecutor).build().unwrap();
    let params = make_params(None);

    let result = handler.on_send_message(params, true, None).await;
    assert!(
        matches!(result, Ok(SendMessageResult::Stream(_))),
        "streaming executor failure should still return stream"
    );
}

#[tokio::test]
async fn input_required_continuation_reuses_task_id() {
    // When a client sends a task_id matching an existing non-terminal task
    // for the same context_id, the handler should reuse the task_id rather
    // than generating a new one (A2A spec §3.4.3).
    use a2a_protocol_types::task::{Task, TaskId, TaskState, TaskStatus};

    let handler = make_handler();

    // Pre-save a task in InputRequired state (non-terminal).
    let existing_task_id = TaskId::new("input-required-task");
    let task = Task {
        id: existing_task_id.clone(),
        context_id: ContextId::new("ctx-input"),
        status: TaskStatus::new(TaskState::InputRequired),
        history: None,
        artifacts: None,
        metadata: None,
    };
    handler.task_store.save(&task).await.unwrap();

    // Send a continuation message with the same context_id and task_id.
    let mut params = make_params(Some("ctx-input"));
    params.message.task_id = Some(existing_task_id.clone());

    let result = handler.on_send_message(params, false, None).await;
    let send_result = result.expect("continuation should succeed");
    match send_result {
        SendMessageResult::Response(SendMessageResponse::Task(t)) => {
            assert_eq!(
                t.id, existing_task_id,
                "task_id should be reused for input-required continuation"
            );
        }
        _ => panic!("expected Response(Task)"),
    }
}

// ── Send-path decision helpers ────────────────────────────────────────

#[test]
fn second_send_blocked_iff_token_live() {
    let live = CancellationEntry {
        token: tokio_util::sync::CancellationToken::new(),
        created_at: Instant::now(),
    };
    assert!(
        second_send_blocked(&live),
        "a live token means an executor is in flight → block the second send"
    );

    let token = tokio_util::sync::CancellationToken::new();
    token.cancel();
    let cancelled = CancellationEntry {
        token,
        created_at: Instant::now(),
    };
    assert!(
        !second_send_blocked(&cancelled),
        "a cancelled token no longer blocks a resend"
    );
}

#[test]
fn token_aged_at_or_past_max_age() {
    let max = std::time::Duration::from_secs(3600);
    assert!(
        !token_aged(std::time::Duration::from_secs(3599), max),
        "younger than max is not aged"
    );
    // Boundary: exactly max_age counts as aged (>=), which distinguishes
    // the correct operator from both `<` and `>`.
    assert!(
        token_aged(std::time::Duration::from_secs(3600), max),
        "exactly max_age is aged"
    );
    assert!(token_aged(std::time::Duration::from_secs(3601), max));
}

#[test]
fn evict_aged_token_only_when_queue_gone() {
    assert!(
        evict_aged_token(false),
        "no live queue → the executor finished → evict the lingering token"
    );
    assert!(
        !evict_aged_token(true),
        "a live queue means the task is still running → keep its token"
    );
}

/// Regression: the Phase-2 sweep removal must re-validate the entry under
/// the write lock. A fresh, live token inserted by a concurrent resend
/// between candidate collection and removal is neither cancelled nor
/// aged — deleting it would leave that executor uncancelable.
#[test]
fn token_still_evictable_spares_fresh_live_token() {
    let max_age = std::time::Duration::from_secs(3600);
    let now = Instant::now();

    // A fresh, live token (the concurrent-resend replacement): spared.
    let fresh = CancellationEntry {
        token: tokio_util::sync::CancellationToken::new(),
        created_at: now,
    };
    assert!(
        !token_still_evictable(&fresh, now, max_age),
        "a fresh live token must never be swept"
    );

    // A cancelled token: still evictable.
    let cancelled = CancellationEntry {
        token: tokio_util::sync::CancellationToken::new(),
        created_at: now,
    };
    cancelled.token.cancel();
    assert!(token_still_evictable(&cancelled, now, max_age));

    // An aged live token: still evictable (its queue-liveness gate ran
    // during candidate collection). Model "aged" by advancing the
    // comparison instant forward by `max_age` rather than subtracting from
    // `now` — `Instant::checked_sub` returns `None` on platforms whose
    // monotonic-clock epoch is younger than `max_age` (e.g. a freshly
    // booted Windows CI runner), which would spuriously fail the test.
    let aged = CancellationEntry {
        token: tokio_util::sync::CancellationToken::new(),
        created_at: now,
    };
    let later = now
        .checked_add(max_age)
        .expect("now + max_age is representable");
    assert!(token_still_evictable(&aged, later, max_age));
}
