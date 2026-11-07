// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! `ListTasks` page tokens on the in-memory store: one the store did not issue
//! is `InvalidParams`, and one it did issue outlives the task it names. The
//! SQL stores' versions of both are in their own test files and
//! `postgres_store_tests.rs`.

use a2a_protocol_server::store::{InMemoryTaskStore, TaskStore};
use a2a_protocol_types::error::ErrorCode;
use a2a_protocol_types::params::ListTasksParams;
use a2a_protocol_types::task::{ContextId, Task, TaskId, TaskState, TaskStatus};

fn make_task(id: &str, ctx: &str, state: TaskState) -> Task {
    Task::new(TaskId::new(id), ContextId::new(ctx), TaskStatus::new(state))
}

fn default_list_params() -> ListTasksParams {
    ListTasksParams::new()
}

/// A token the store did not issue is `InvalidParams`: an empty page would
/// read as the end of the listing.
#[tokio::test]
async fn list_with_invalid_page_token_is_invalid_params() {
    let store = InMemoryTaskStore::new();
    store
        .save(&make_task("task-1", "ctx", TaskState::Working))
        .await
        .unwrap();

    let params = {
        let mut built = default_list_params();
        built.page_token = Some("nonexistent-token".into());
        built
    };
    let err = store.list(&params).await.unwrap_err();
    assert_eq!(err.code, ErrorCode::InvalidParams);
}

/// A token the store issued stays good when the task it names is deleted
/// before the next page is asked for: the position is a key, not the row.
#[tokio::test]
async fn a_token_for_a_deleted_task_still_resumes_the_listing() {
    let store = InMemoryTaskStore::new();
    for i in 0..5 {
        store
            .save(&make_task(&format!("task-{i}"), "ctx", TaskState::Working))
            .await
            .unwrap();
    }
    let page = |token: Option<String>| {
        let mut built = ListTasksParams::new().with_page_size(2);
        built.page_token = token;
        built
    };
    let first = store.list(&page(None)).await.unwrap();
    let ids = |r: &a2a_protocol_types::responses::TaskListResponse| {
        r.tasks.iter().map(|t| t.id.0.clone()).collect::<Vec<_>>()
    };
    let expected_second = ids(&store
        .list(&page(Some(first.next_page_token.clone())))
        .await
        .unwrap());
    assert_eq!(expected_second.len(), 2);

    // Delete the task the token names: the last of the first page.
    store.delete(&first.tasks[1].id).await.unwrap();
    let second = store
        .list(&page(Some(first.next_page_token.clone())))
        .await
        .unwrap();
    assert_eq!(ids(&second), expected_second);
}
