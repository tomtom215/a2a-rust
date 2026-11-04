// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Suspension: failures in a row, the pause and its doubling, the events that
//! end a turn, and deliveries cut off before they are answered.

use super::*;

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
