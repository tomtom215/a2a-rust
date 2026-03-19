// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! T-10 concurrency tests: concurrent saves, queue creation, message sends,
//! and `insert_if_absent` atomicity.

use super::*;

#[tokio::test]
async fn concurrent_save_to_same_task_id() {
    let store = Arc::new(InMemoryTaskStore::new());
    let mut handles = vec![];

    for i in 0..50 {
        let store = Arc::clone(&store);
        handles.push(tokio::spawn(async move {
            let task = Task {
                id: TaskId::new("shared-task"),
                context_id: ContextId::new(format!("ctx-{i}")),
                status: TaskStatus::new(TaskState::Working),
                history: None,
                artifacts: None,
                metadata: None,
            };
            a2a_protocol_server::TaskStore::save(store.as_ref(), task)
                .await
                .unwrap();
        }));
    }

    for h in handles {
        h.await.unwrap();
    }

    // Should have exactly one task (last write wins)
    let result = a2a_protocol_server::TaskStore::get(store.as_ref(), &TaskId::new("shared-task"))
        .await
        .unwrap();
    assert!(result.is_some(), "task must exist after concurrent saves");
}

#[tokio::test]
async fn concurrent_get_or_create_event_queue() {
    let mgr = Arc::new(a2a_protocol_server::EventQueueManager::new());
    let task_id = TaskId::new("concurrent-queue");
    let mut handles = vec![];

    for _ in 0..10 {
        let mgr = Arc::clone(&mgr);
        let tid = task_id.clone();
        handles.push(tokio::spawn(async move { mgr.get_or_create(&tid).await }));
    }

    let mut reader_count = 0;
    for h in handles {
        let (_writer, reader) = h.await.unwrap();
        if reader.is_some() {
            reader_count += 1;
        }
    }

    // Only one should have gotten a reader (the first to create)
    assert_eq!(
        reader_count, 1,
        "exactly one concurrent create should get a reader, got {reader_count}"
    );
    assert_eq!(mgr.active_count().await, 1, "only one queue must be active");
}

#[tokio::test]
async fn concurrent_send_message() {
    let handler = Arc::new(RequestHandlerBuilder::new(EchoExecutor).build().unwrap());
    let mut handles = vec![];

    for i in 0..10 {
        let handler = Arc::clone(&handler);
        handles.push(tokio::spawn(async move {
            handler
                .on_send_message(make_send_params(&format!("msg-{i}")), false, None)
                .await
        }));
    }

    let mut success_count = 0;
    for h in handles {
        if h.await.unwrap().is_ok() {
            success_count += 1;
        }
    }
    assert_eq!(
        success_count, 10,
        "all 10 concurrent sends should succeed, got {success_count}"
    );
}

#[tokio::test]
async fn insert_if_absent_atomicity() {
    let store = Arc::new(InMemoryTaskStore::new());
    let mut handles = vec![];

    // 20 concurrent attempts to insert the same task ID
    for _ in 0..20 {
        let store = Arc::clone(&store);
        handles.push(tokio::spawn(async move {
            let task = Task {
                id: TaskId::new("atomic-task"),
                context_id: ContextId::new("ctx"),
                status: TaskStatus::new(TaskState::Submitted),
                history: None,
                artifacts: None,
                metadata: None,
            };
            a2a_protocol_server::TaskStore::insert_if_absent(store.as_ref(), task)
                .await
                .unwrap()
        }));
    }

    let mut insert_count = 0;
    for h in handles {
        if h.await.unwrap() {
            insert_count += 1;
        }
    }

    // Exactly one should succeed
    assert_eq!(
        insert_count, 1,
        "exactly one insert_if_absent should succeed, got {insert_count}"
    );
}
