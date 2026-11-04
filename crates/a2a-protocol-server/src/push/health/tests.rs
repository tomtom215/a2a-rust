// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

use std::collections::VecDeque;
use std::sync::atomic::{AtomicUsize, Ordering};

use a2a_protocol_types::events::TaskStatusUpdateEvent;
use a2a_protocol_types::task::{ContextId, TaskId, TaskState, TaskStatus};

use super::*;
use crate::push::InMemoryPushConfigStore;

const URL: &str = "https://hooks.example.com/a";
const TEN: Duration = Duration::from_secs(10);

type Answer = fn() -> A2aResult<()>;

/// A sender that answers from a script, then with `after`, and counts the
/// deliveries that reach it.
struct Scripted {
    script: Mutex<VecDeque<A2aResult<()>>>,
    after: Answer,
    reached: Arc<AtomicUsize>,
}

fn failing() -> A2aResult<()> {
    Err(A2aError::internal("the webhook is down"))
}

fn gone() -> A2aResult<()> {
    Err(rejected(
        "push notification got non-retryable HTTP 410 Gone".to_owned(),
        410,
    ))
}

fn not_found() -> A2aResult<()> {
    Err(rejected(
        "push notification got non-retryable HTTP 404 Not Found".to_owned(),
        404,
    ))
}

impl Scripted {
    fn always(after: Answer) -> (Arc<Self>, Arc<AtomicUsize>) {
        let reached = Arc::new(AtomicUsize::new(0));
        let sender = Arc::new(Self {
            script: Mutex::default(),
            after,
            reached: Arc::clone(&reached),
        });
        (sender, reached)
    }

    fn then(&self, answers: impl IntoIterator<Item = A2aResult<()>>) {
        self.script.lock().unwrap().extend(answers);
    }
}

impl PushSender for Scripted {
    fn send<'a>(
        &'a self,
        _url: &'a str,
        _event: &'a StreamResponse,
        _config: &'a TaskPushNotificationConfig,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        self.reached.fetch_add(1, Ordering::SeqCst);
        let answer = self
            .script
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or_else(self.after);
        Box::pin(async move { answer })
    }

    fn allows_private_urls(&self) -> bool {
        true
    }

    fn allows_webhook_url(&self, url: &str) -> bool {
        url.ends_with("/a")
    }

    fn requires_https(&self) -> bool {
        true
    }

    fn max_delivery_duration(&self) -> Option<Duration> {
        Some(Duration::from_secs(38))
    }
}

fn status(state: TaskState) -> StreamResponse {
    StreamResponse::StatusUpdate(TaskStatusUpdateEvent::new(
        TaskId::new("t1"),
        ContextId::new("c1"),
        TaskStatus::new(state),
    ))
}

fn working() -> StreamResponse {
    status(TaskState::Working)
}

fn config(id: &str) -> TaskPushNotificationConfig {
    let mut config = TaskPushNotificationConfig::new("t1", URL);
    config.id = Some(id.to_owned());
    config
}

fn limits(after: usize, remove_gone: bool) -> HandlerLimits {
    HandlerLimits::default()
        .with_push_suspend_after(after)
        .with_push_suspend_for(TEN)
        .with_push_remove_gone(remove_gone)
}

async fn send(
    sender: &dyn PushSender,
    event: &StreamResponse,
    config: &TaskPushNotificationConfig,
) -> A2aResult<()> {
    sender.send(&config.url, event, config).await
}

fn suspended(result: &A2aResult<()>) -> bool {
    result
        .as_ref()
        .err()
        .and_then(outcome_of)
        .is_some_and(|label| label == push_outcome::SUSPENDED)
}

fn health(
    inner: Arc<dyn PushSender>,
    store: Arc<dyn PushConfigStore>,
    after: usize,
    remove_gone: bool,
) -> PushHealth {
    PushHealth {
        inner,
        store,
        suspend_after: after,
        suspend_for: TEN,
        remove_gone,
        watched: Mutex::default(),
    }
}

fn store() -> Arc<dyn PushConfigStore> {
    Arc::new(InMemoryPushConfigStore::new())
}

// -- suspension --------------------------------------------------------------

#[tokio::test(start_paused = true)]
async fn failures_in_a_row_suspend_a_config_until_its_pause_is_over_and_a_success_ends_it() {
    let (inner, reached) = Scripted::always(failing);
    let health = health(inner.clone(), store(), 3, true);
    let cfg = config("c1");

    for _ in 0..3 {
        assert!(send(&health, &working(), &cfg).await.is_err());
    }
    assert_eq!(reached.load(Ordering::SeqCst), 3, "three deliveries failed");

    let skipped = send(&health, &working(), &cfg).await;
    assert!(suspended(&skipped), "{skipped:?}");
    assert_eq!(
        reached.load(Ordering::SeqCst),
        3,
        "a suspended webhook is not sent to"
    );

    tokio::time::advance(Duration::from_millis(9_999)).await;
    assert!(
        suspended(&send(&health, &working(), &cfg).await),
        "still inside the pause one millisecond before it ends"
    );
    tokio::time::advance(Duration::from_millis(1)).await;
    inner.then([Ok(())]);
    assert!(
        send(&health, &working(), &cfg).await.is_ok(),
        "the probe is sent and succeeds"
    );
    assert_eq!(reached.load(Ordering::SeqCst), 4);

    // It succeeded, so the next failures start a new count.
    for _ in 0..2 {
        assert!(send(&health, &working(), &cfg).await.is_err());
    }
    assert!(
        !suspended(&send(&health, &working(), &cfg).await),
        "two failures after a success are not three in a row"
    );
}

#[tokio::test(start_paused = true)]
async fn a_failed_probe_doubles_the_pause_up_to_thirty_two_times_as_long() {
    let (inner, _) = Scripted::always(failing);
    let health = health(inner, store(), 1, true);
    let cfg = config("c1");

    // The pauses a config earns by failing every probe: 10 s, doubling to
    // 320 s, then no longer.
    let mut pauses = Vec::new();
    for _ in 0..8 {
        assert!(send(&health, &working(), &cfg).await.is_err());
        let began = Instant::now();
        while suspended(&send(&health, &working(), &cfg).await) {
            tokio::time::advance(Duration::from_secs(1)).await;
        }
        pauses.push(began.elapsed().as_secs());
    }
    assert_eq!(pauses, [10, 20, 40, 80, 160, 320, 320, 320], "{pauses:?}");
}

#[tokio::test(start_paused = true)]
async fn an_event_that_ends_a_turn_is_sent_to_a_suspended_webhook() {
    let (inner, reached) = Scripted::always(failing);
    let health = health(inner.clone(), store(), 2, true);
    let cfg = config("c1");
    for _ in 0..2 {
        let _ = send(&health, &working(), &cfg).await;
    }
    assert!(suspended(&send(&health, &working(), &cfg).await));
    let before = reached.load(Ordering::SeqCst);

    for ending in [TaskState::Completed, TaskState::InputRequired] {
        inner.then([Ok(())]);
        assert!(
            send(&health, &status(ending), &cfg).await.is_ok(),
            "{ending:?} reaches the webhook while it is suspended"
        );
    }
    assert_eq!(reached.load(Ordering::SeqCst), before + 2);
    assert!(
        !suspended(&send(&health, &working(), &cfg).await),
        "and its success ended the suspension"
    );
}

#[tokio::test(start_paused = true)]
async fn a_failed_probe_on_an_ending_event_suspends_for_longer() {
    let (inner, reached) = Scripted::always(failing);
    let health = health(inner, store(), 1, true);
    let cfg = config("c1");
    assert!(send(&health, &working(), &cfg).await.is_err());
    assert!(
        send(&health, &status(TaskState::Completed), &cfg)
            .await
            .is_err()
    );
    tokio::time::advance(TEN).await;
    assert!(
        suspended(&send(&health, &working(), &cfg).await),
        "the second suspension is twice as long, so ten seconds is not enough"
    );
    assert_eq!(reached.load(Ordering::SeqCst), 2);
}

#[tokio::test(start_paused = true)]
async fn configs_are_watched_apart_by_id_task_and_tenant() {
    let (inner, reached) = Scripted::always(failing);
    let health = health(inner, store(), 1, true);
    let a = config("a");
    let _ = send(&health, &working(), &a).await;
    assert!(suspended(&send(&health, &working(), &a).await));

    assert!(
        !suspended(&send(&health, &working(), &config("b")).await),
        "another config of the task is not suspended"
    );
    let mut other_task = config("a");
    other_task.task_id = Some("t2".to_owned());
    assert!(
        !suspended(&send(&health, &working(), &other_task).await),
        "the same id on another task is not suspended"
    );
    let in_tenant =
        TenantContext::scope("acme", async { send(&health, &working(), &a).await }).await;
    assert!(
        !suspended(&in_tenant),
        "the same config in another tenant is not suspended"
    );
    assert_eq!(reached.load(Ordering::SeqCst), 4);
}

#[tokio::test(start_paused = true)]
async fn a_config_with_no_id_is_watched_by_its_address() {
    let (inner, _) = Scripted::always(failing);
    let health = health(inner, store(), 1, true);
    let mut bare = TaskPushNotificationConfig::new("t1", URL);
    bare.id = None;
    let _ = send(&health, &working(), &bare).await;
    assert!(suspended(&send(&health, &working(), &bare).await));

    let mut elsewhere = TaskPushNotificationConfig::new("t1", "https://hooks.example.com/b");
    elsewhere.id = None;
    assert!(
        !suspended(&send(&health, &working(), &elsewhere).await),
        "another address of the same task is not suspended with it"
    );
}

#[tokio::test(start_paused = true)]
async fn zero_turns_suspension_off() {
    let (inner, reached) = Scripted::always(failing);
    let health = health(inner, store(), 0, true);
    let cfg = config("c1");
    for _ in 0..50 {
        assert!(!suspended(&send(&health, &working(), &cfg).await));
    }
    assert_eq!(
        reached.load(Ordering::SeqCst),
        50,
        "every delivery was tried"
    );
    assert!(health.lock().is_empty(), "and nothing was kept about it");
}

#[tokio::test(start_paused = true)]
async fn what_is_kept_about_failing_configs_is_bounded() {
    let (inner, _) = Scripted::always(failing);
    let health = health(inner, store(), 5, true);
    for n in 0..TRACKED + 10 {
        let _ = send(&health, &working(), &config(&format!("c{n}"))).await;
    }
    assert_eq!(health.lock().len(), TRACKED);
}

/// A sender that never answers: its delivery can only be cut off.
struct Silent;

impl PushSender for Silent {
    fn send<'a>(
        &'a self,
        _url: &'a str,
        _event: &'a StreamResponse,
        _config: &'a TaskPushNotificationConfig,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(std::future::pending())
    }
}

/// A webhook that never answers is cut off by the caller's own bound
/// (`push_delivery_timeout`), so the sender never returns an error for it. It
/// has to count all the same, or the commonest dead webhook is never suspended.
#[tokio::test(start_paused = true)]
async fn deliveries_cut_off_before_an_answer_count_as_failures() {
    let health = health(Arc::new(Silent), store(), 3, true);
    let cfg = config("c1");
    for n in 1..=3 {
        let cut =
            tokio::time::timeout(Duration::from_secs(30), send(&health, &working(), &cfg)).await;
        assert!(cut.is_err(), "delivery {n} never answers and is cut off");
    }
    let fourth = tokio::time::timeout(Duration::from_secs(30), send(&health, &working(), &cfg))
        .await
        .expect("the fourth is answered at once, not cut off");
    assert!(suspended(&fourth), "{fourth:?}");
}

/// A delivery that is answered is not also counted when its future is dropped.
#[tokio::test(start_paused = true)]
async fn an_answered_delivery_is_not_counted_again_when_it_is_dropped() {
    let (inner, _) = Scripted::always(|| Ok(()));
    let health = health(inner, store(), 1, true);
    let cfg = config("c1");
    for _ in 0..5 {
        assert!(send(&health, &working(), &cfg).await.is_ok());
    }
    assert!(
        health.lock().is_empty(),
        "five successes leave nothing counted"
    );
}

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

// -- the module's own parts --------------------------------------------------

#[test]
fn a_refusal_carries_its_status_and_a_decision_is_marked_on_the_error() {
    let err = rejected("refused".to_owned(), 410);
    assert_eq!(status_of(&err), Some(410));
    assert_eq!(outcome_of(&err), None, "a bare refusal decides nothing");
    assert_eq!(status_of(&A2aError::internal("no data")), None);

    let gone = marked(err, push_outcome::GONE);
    assert_eq!(outcome_of(&gone), Some(push_outcome::GONE));
    assert_eq!(status_of(&gone), Some(410), "the status survives the mark");
    let suspended = marked(A2aError::internal("skipped"), push_outcome::SUSPENDED);
    assert_eq!(outcome_of(&suspended), Some(push_outcome::SUSPENDED));

    let mut unknown = A2aError::internal("x");
    unknown.data = Some(serde_json::json!({ OUTCOME_KEY: "something-else" }));
    assert_eq!(outcome_of(&unknown), None);
}

#[test]
fn both_rules_off_leaves_the_sender_as_it_was() {
    let (inner, _) = Scripted::always(failing);
    let inner: Arc<dyn PushSender> = inner;
    let off = watched(Arc::clone(&inner), store(), &limits(0, false));
    assert!(Arc::ptr_eq(&inner, &off), "nothing to watch for");
    for (after, remove) in [(1, false), (0, true), (5, true)] {
        let on = watched(Arc::clone(&inner), store(), &limits(after, remove));
        assert!(!Arc::ptr_eq(&inner, &on), "after={after} remove={remove}");
    }
}

#[test]
fn what_the_wrapped_sender_says_about_urls_is_passed_on() {
    let (inner, _) = Scripted::always(failing);
    let health = health(inner, store(), 5, true);
    assert!(health.allows_private_urls());
    assert!(health.allows_webhook_url("https://x/a"));
    assert!(!health.allows_webhook_url("https://x/b"));
    assert!(health.requires_https());
    assert_eq!(
        health.max_delivery_duration(),
        Some(Duration::from_secs(38))
    );
}

#[test]
fn the_debug_form_names_the_policy_and_not_the_state() {
    let (inner, _) = Scripted::always(failing);
    let shown = format!("{:?}", health(inner, store(), 7, false));
    assert!(shown.starts_with("PushHealth"), "{shown}");
    assert!(shown.contains("suspend_after: 7"), "{shown}");
    assert!(shown.contains("remove_gone: false"), "{shown}");
    assert!(shown.contains("suspend_for: 10s"), "{shown}");
}
