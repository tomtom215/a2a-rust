// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Reading a key without taking it (audit E-F24).

use super::*;

// -- reading the key ---------------------------------------------------------

fn handler_over_memory() -> RequestHandler {
    RequestHandlerBuilder::new(CountingExecutor(Arc::new(AtomicUsize::new(0))))
        .without_agent_card()
        .build()
        .expect("the handler builds")
}

#[tokio::test]
async fn no_key_and_a_free_key_name_no_replay() {
    let handler = handler_over_memory();
    let unkeyed = keyed_params("m1", None).message;
    assert!(
        handler
            .replay_of_held_key(&unkeyed)
            .await
            .expect("reads")
            .is_none(),
        "a message with no key replays nothing"
    );
    let keyed = keyed_params("m1", Some(KEY)).message;
    assert!(
        handler
            .replay_of_held_key(&keyed)
            .await
            .expect("reads")
            .is_none(),
        "a free key replays nothing"
    );
    assert_eq!(
        holder(&InMemoryTaskStore::new()).await,
        None,
        "and reading it did not take it"
    );
}

#[tokio::test]
async fn a_key_this_message_holds_names_the_task_it_holds() {
    let handler = handler_over_memory();
    let task = Task::new(
        TaskId::new("t-held"),
        ContextId::new("ctx"),
        TaskStatus::new(TaskState::Working),
    );
    handler.task_store.save(&task).await.expect("saves");
    handler
        .task_store
        .claim_idempotency_key(KEY, &MessageId::new("m1"), &TaskId::new("t-held"))
        .await
        .expect("claims");

    let replay = handler
        .replay_of_held_key(&keyed_params("m1", Some(KEY)).message)
        .await
        .expect("reads")
        .expect("the key is held by this message");
    assert_eq!(replay.id, TaskId::new("t-held"));

    let reused = handler
        .replay_of_held_key(&keyed_params("m2", Some(KEY)).message)
        .await
        .expect_err("held by another message");
    assert!(
        matches!(&reused, ServerError::InvalidParams(m) if m.contains("held by message `m1`")),
        "{reused:?}"
    );
}

/// A key the send could never claim is refused before anything is read: the
/// same refusals, in the same order, as the claim gives.
#[tokio::test]
async fn a_key_that_could_never_be_claimed_is_refused_before_it_is_read() {
    let handler = handler_over_memory();
    let mut malformed = keyed_params("m1", None).message;
    malformed.metadata = Some(serde_json::json!({ IDEMPOTENCY_METADATA_KEY: "short" }));
    let refused = handler
        .replay_of_held_key(&malformed)
        .await
        .expect_err("malformed");
    assert!(
        matches!(refused, ServerError::InvalidParams(_)),
        "{refused:?}"
    );

    let handler = RequestHandlerBuilder::new(CountingExecutor(Arc::new(AtomicUsize::new(0))))
        .without_agent_card()
        .with_task_store(NoKeys(InMemoryTaskStore::new()))
        .build()
        .expect("the handler builds");
    let refused = handler
        .replay_of_held_key(&keyed_params("m1", Some(KEY)).message)
        .await
        .expect_err("the store cannot honour keys");
    assert!(
        matches!(refused, ServerError::UnsupportedOperation(_)),
        "{refused:?}"
    );
}

/// A store that cannot answer is reported as the claim reports it: a
/// temporary refusal as `Overloaded`, anything else as the store's error.
#[tokio::test]
async fn a_failed_read_of_the_key_is_reported_as_a_failed_claim_is() {
    struct Failing(InMemoryTaskStore, A2aError);
    impl TaskStore for Failing {
        fn save<'a>(&'a self, t: &'a Task) -> Fut<'a, ()> {
            self.0.save(t)
        }
        fn get<'a>(&'a self, id: &'a TaskId) -> Fut<'a, Option<Task>> {
            self.0.get(id)
        }
        fn list<'a>(&'a self, p: &'a ListTasksParams) -> Fut<'a, TaskListResponse> {
            self.0.list(p)
        }
        fn insert_if_absent<'a>(&'a self, t: &'a Task) -> Fut<'a, bool> {
            self.0.insert_if_absent(t)
        }
        fn delete<'a>(&'a self, id: &'a TaskId) -> Fut<'a, ()> {
            self.0.delete(id)
        }
        fn supports_idempotency(&self) -> bool {
            true
        }
        fn idempotency_holder<'a>(&'a self, _key: &'a str) -> Fut<'a, Option<IdempotencyHolder>> {
            let error = self.1.clone();
            Box::pin(async move { Err(error) })
        }
    }
    for (error, overloaded) in [
        (A2aError::unavailable("the database is restarting"), true),
        (A2aError::internal("the key index is corrupt"), false),
    ] {
        let handler = RequestHandlerBuilder::new(CountingExecutor(Arc::new(AtomicUsize::new(0))))
            .without_agent_card()
            .with_task_store(Failing(InMemoryTaskStore::new(), error))
            .build()
            .expect("the handler builds");
        let refused = handler
            .replay_of_held_key(&keyed_params("m1", Some(KEY)).message)
            .await
            .expect_err("the read fails");
        assert_eq!(
            matches!(refused, ServerError::Overloaded(_)),
            overloaded,
            "{refused:?}"
        );
    }
}

/// A message with no key reads nothing from the store, and one with a key
/// reads it once: the wait asks before every attempt, and a send that carries
/// no key must not cost a database round trip each time.
#[tokio::test]
async fn only_a_message_with_a_key_reads_the_store() {
    struct Reads(InMemoryTaskStore, Arc<AtomicUsize>);
    impl TaskStore for Reads {
        fn save<'a>(&'a self, t: &'a Task) -> Fut<'a, ()> {
            self.0.save(t)
        }
        fn get<'a>(&'a self, id: &'a TaskId) -> Fut<'a, Option<Task>> {
            self.0.get(id)
        }
        fn list<'a>(&'a self, p: &'a ListTasksParams) -> Fut<'a, TaskListResponse> {
            self.0.list(p)
        }
        fn insert_if_absent<'a>(&'a self, t: &'a Task) -> Fut<'a, bool> {
            self.0.insert_if_absent(t)
        }
        fn delete<'a>(&'a self, id: &'a TaskId) -> Fut<'a, ()> {
            self.0.delete(id)
        }
        fn supports_idempotency(&self) -> bool {
            true
        }
        fn idempotency_holder<'a>(&'a self, key: &'a str) -> Fut<'a, Option<IdempotencyHolder>> {
            self.1.fetch_add(1, Ordering::SeqCst);
            self.0.idempotency_holder(key)
        }
    }
    let reads = Arc::new(AtomicUsize::new(0));
    let handler = RequestHandlerBuilder::new(CountingExecutor(Arc::new(AtomicUsize::new(0))))
        .without_agent_card()
        .with_task_store(Reads(InMemoryTaskStore::new(), Arc::clone(&reads)))
        .build()
        .expect("the handler builds");

    let unkeyed = keyed_params("m1", None).message;
    assert!(
        handler
            .replay_of_held_key(&unkeyed)
            .await
            .expect("reads")
            .is_none()
    );
    assert_eq!(reads.load(Ordering::SeqCst), 0, "no key, no read");

    let keyed = keyed_params("m1", Some(KEY)).message;
    assert!(
        handler
            .replay_of_held_key(&keyed)
            .await
            .expect("reads")
            .is_none()
    );
    assert_eq!(reads.load(Ordering::SeqCst), 1, "a key is read once");
}
