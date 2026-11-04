// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! A webhook that answers `410 Gone` has its config removed.

use super::*;

// -- 410 Gone ----------------------------------------------------------------

async fn stored(store: &Arc<dyn PushConfigStore>) -> TaskPushNotificationConfig {
    store.set(config("c1")).await.expect("stores")
}

#[tokio::test]
async fn a_webhook_that_answers_gone_has_its_config_removed() {
    let store = store();
    let cfg = stored(&store).await;
    let (inner, reached) = Scripted::always(gone);
    let health = health(inner, Arc::clone(&store), 5, true);

    let err = send(&health, &working(), &cfg)
        .await
        .expect_err("the delivery did not succeed");
    assert_eq!(outcome_of(&err), Some(push_outcome::GONE), "{err:?}");
    assert!(
        err.message.contains("410"),
        "the cause is kept: {}",
        err.message
    );
    assert!(
        store.list("t1").await.unwrap().is_empty(),
        "the config was removed"
    );
    assert_eq!(reached.load(Ordering::SeqCst), 1);
    assert!(health.lock().is_empty(), "and nothing is kept about it");
}

#[tokio::test]
async fn gone_is_acted_on_only_when_asked_and_only_for_410() {
    let cases: [(Answer, bool, bool); 4] = [
        (gone, false, false),
        (not_found, true, false),
        (failing, true, false),
        (gone, true, true),
    ];
    for (answer, remove_gone, removed) in cases {
        let store = store();
        let cfg = stored(&store).await;
        let (inner, _) = Scripted::always(answer);
        let health = health(inner, Arc::clone(&store), 5, remove_gone);
        let err = send(&health, &working(), &cfg).await.expect_err("fails");
        assert_eq!(
            outcome_of(&err) == Some(push_outcome::GONE),
            removed,
            "{err:?}"
        );
        assert_eq!(
            store.list("t1").await.unwrap().is_empty(),
            removed,
            "removal with remove_gone={remove_gone}"
        );
    }
}

#[tokio::test]
async fn a_gone_webhook_whose_config_cannot_be_removed_counts_as_a_failure() {
    // No id to delete by.
    let (inner, _) = Scripted::always(gone);
    let health_no_id = health(inner, store(), 1, true);
    let mut bare = TaskPushNotificationConfig::new("t1", URL);
    bare.id = None;
    let err = send(&health_no_id, &working(), &bare)
        .await
        .expect_err("fails");
    assert_eq!(outcome_of(&err), None, "{err:?}");
    assert!(
        suspended(&send(&health_no_id, &working(), &bare).await),
        "it was counted, and one failure suspends at 1"
    );

    // A store that refuses the delete.
    let (inner, _) = Scripted::always(gone);
    let health_refused = health(inner, Arc::new(Refusing), 1, true);
    let cfg = config("c1");
    let err = send(&health_refused, &working(), &cfg)
        .await
        .expect_err("fails");
    assert_eq!(
        outcome_of(&err),
        None,
        "not removed, so not reported as gone: {err:?}"
    );
    assert!(suspended(&send(&health_refused, &working(), &cfg).await));
}

/// A store whose deletes fail.
struct Refusing;

type Fut<'a, T> = Pin<Box<dyn Future<Output = A2aResult<T>> + Send + 'a>>;

impl PushConfigStore for Refusing {
    fn set(&self, c: TaskPushNotificationConfig) -> Fut<'_, TaskPushNotificationConfig> {
        Box::pin(async move { Ok(c) })
    }
    fn get<'a>(
        &'a self,
        _task_id: &'a str,
        _id: &'a str,
    ) -> Fut<'a, Option<TaskPushNotificationConfig>> {
        Box::pin(async { Ok(None) })
    }
    fn list<'a>(&'a self, _task_id: &'a str) -> Fut<'a, Vec<TaskPushNotificationConfig>> {
        Box::pin(async { Ok(Vec::new()) })
    }
    fn delete<'a>(&'a self, _task_id: &'a str, _id: &'a str) -> Fut<'a, ()> {
        Box::pin(async { Err(A2aError::internal("the database is down")) })
    }
}
