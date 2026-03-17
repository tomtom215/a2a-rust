// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! Concurrency tests: multiple simultaneous `send_message` calls and
//! concurrent reads interleaved with writes to the task store.

use super::*;

#[tokio::test]
async fn concurrency_multiple_send_message_calls_on_same_handler() {
    let handler = Arc::new(
        RequestHandlerBuilder::new(QuickExecutor)
            .build()
            .expect("build handler"),
    );

    let mut handles = Vec::new();
    for i in 0..10 {
        let h = Arc::clone(&handler);
        handles.push(tokio::spawn(async move {
            h.on_send_message(make_send_params(&format!("msg-{i}")), false, None)
                .await
        }));
    }

    let mut completed_count = 0;
    for handle in handles {
        let result = handle.await.expect("join");
        match result {
            Ok(SendMessageResult::Response(SendMessageResponse::Task(task))) => {
                assert_eq!(task.status.state, TaskState::Completed);
                completed_count += 1;
            }
            Ok(_) => panic!("expected Response(Task)"),
            Err(e) => panic!("unexpected error: {e:?}"),
        }
    }
    assert_eq!(completed_count, 10, "all 10 messages should complete");
}

#[tokio::test]
async fn concurrency_reads_while_writing_to_task_store() {
    let store = Arc::new(InMemoryTaskStore::new());

    // Pre-populate some tasks.
    for i in 0..5 {
        store
            .save(make_task(&format!("pre-{i}"), "ctx", TaskState::Completed))
            .await
            .unwrap();
    }

    let mut handles = Vec::new();

    // Spawn writers.
    for i in 0..10 {
        let s = Arc::clone(&store);
        handles.push(tokio::spawn(async move {
            s.save(make_task(&format!("w-{i}"), "ctx", TaskState::Working))
                .await
                .expect("concurrent save");
        }));
    }

    // Spawn readers concurrently.
    for i in 0..10 {
        let s = Arc::clone(&store);
        handles.push(tokio::spawn(async move {
            let params = ListTasksParams {
                tenant: None,
                context_id: None,
                status: None,
                page_size: Some(50),
                page_token: None,
                status_timestamp_after: None,
                include_artifacts: None,
                history_length: None,
            };
            let result = s.list(&params).await.expect("concurrent list");
            // Should always return at least the pre-populated tasks.
            assert!(
                !result.tasks.is_empty(),
                "concurrent read {i} should find at least some tasks"
            );
        }));
    }

    for handle in handles {
        handle.await.expect("join");
    }

    // After all writes, verify total count.
    let params = ListTasksParams {
        tenant: None,
        context_id: None,
        status: None,
        page_size: Some(50),
        page_token: None,
        status_timestamp_after: None,
        include_artifacts: None,
        history_length: None,
    };
    let final_result = store.list(&params).await.unwrap();
    assert_eq!(
        final_result.tasks.len(),
        15,
        "should have 5 pre-populated + 10 written tasks"
    );
}
