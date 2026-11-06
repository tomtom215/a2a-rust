// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The worker given an outbox that fails: one that cannot end or release a
//! row, and one that will not take a record. The fixtures are `outbox`'s.

use super::*;

/// What `tracing` recorded at `WARN`.
#[cfg(feature = "tracing")]
#[derive(Clone, Default)]
struct Warnings(Arc<Mutex<Vec<String>>>);

#[cfg(feature = "tracing")]
impl<S: tracing::Subscriber> tracing_subscriber::Layer<S> for Warnings {
    fn on_event(&self, event: &tracing::Event<'_>, _: tracing_subscriber::layer::Context<'_, S>) {
        struct Text(String);
        impl tracing::field::Visit for Text {
            fn record_debug(&mut self, f: &tracing::field::Field, v: &dyn std::fmt::Debug) {
                if f.name() == "message" {
                    self.0 = format!("{v:?}");
                }
            }
        }
        if *event.metadata().level() == tracing::Level::WARN {
            let mut message = Text(String::new());
            event.record(&mut message);
            self.0.lock().unwrap().push(message.0);
        }
    }
}

/// An outbox that stores and hands out rows as the in-memory one does, but
/// cannot end one or release one.
#[cfg(feature = "tracing")]
struct CannotUpdate(Arc<InMemoryPushOutbox>);

#[cfg(feature = "tracing")]
impl PushOutbox for CannotUpdate {
    fn record(&self, rows: Vec<NewDelivery>, cap: usize) -> OutboxFuture<'_, usize> {
        self.0.record(rows, cap)
    }
    fn claim<'a>(
        &'a self,
        owner: &'a str,
        batch: usize,
        lease: Duration,
    ) -> OutboxFuture<'a, Vec<crate::push::outbox::ClaimedDelivery>> {
        self.0.claim(owner, batch, lease)
    }
    fn complete(&self, _: i64) -> OutboxFuture<'_, ()> {
        Box::pin(async { Err(A2aError::internal("database is down")) })
    }
    fn retry_after<'a>(&'a self, _: i64, _: &'a str, _: Duration) -> OutboxFuture<'a, ()> {
        Box::pin(async { Err(A2aError::internal("database is down")) })
    }
    fn pending(&self) -> OutboxFuture<'_, usize> {
        self.0.pending()
    }
}

/// A row the outbox cannot end after a delivery, or release after a failed
/// one, is claimed again once its lease runs out and delivered again; the
/// worker says so, so that the second delivery is not a surprise.
#[cfg(feature = "tracing")]
#[tokio::test(start_paused = true)]
async fn an_outbox_that_cannot_update_a_row_is_warned_about() {
    use tracing_subscriber::layer::SubscriberExt as _;

    let warnings = Warnings::default();
    let _guard =
        tracing::subscriber::set_default(tracing_subscriber::registry().with(warnings.clone()));
    for answers in [Vec::new(), vec![failed()]] {
        let mut s = setup(&["http://a.example.com/h"]).await;
        s.sender.answer("http://a.example.com/h", answers);
        let lanes = PushLanes::with_outbox(
            Arc::clone(&s.store) as Arc<dyn PushConfigStore>,
            Some(Arc::clone(&s.sender) as Arc<dyn PushSender>),
            HandlerLimits::default(),
            Arc::clone(&s.metrics) as Arc<dyn Metrics>,
            TaskTracker::new(),
            Some((
                Arc::new(CannotUpdate(Arc::clone(&s.outbox))) as Arc<dyn PushOutbox>,
                s.shutdown.clone(),
            )),
        );
        let before = warnings.0.lock().unwrap().len();
        lanes.push(
            &TaskId::new("t1"),
            &make_status_event("t1", TaskState::Completed),
        );
        s.seen.recv().await.unwrap();
        // A failed delivery says so first (`attempt`), and the update that
        // follows it says its own: look for that one, not for the next.
        let said_so = |w: &Warnings| {
            w.0.lock().unwrap()[before..]
                .iter()
                .any(|m| m.contains("the push outbox could not be updated"))
        };
        let waited = tokio::time::timeout(Duration::from_secs(10), async {
            while !said_so(&warnings) {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await;
        assert!(
            waited.is_ok(),
            "the update that failed was not warned about: {:?}",
            warnings.0.lock().unwrap()
        );
        s.shutdown.cancel();
    }
}

/// An outbox that will not take a delivery costs nothing more than the
/// lanes did: it is delivered from memory, now.
#[tokio::test(start_paused = true)]
async fn an_outbox_that_refuses_a_record_is_delivered_from_memory() {
    struct Refuses;
    impl PushOutbox for Refuses {
        fn record(&self, _: Vec<NewDelivery>, _: usize) -> OutboxFuture<'_, usize> {
            Box::pin(async { Err(A2aError::internal("database is down")) })
        }
        fn claim<'a>(
            &'a self,
            _: &'a str,
            _: usize,
            _: Duration,
        ) -> OutboxFuture<'a, Vec<crate::push::outbox::ClaimedDelivery>> {
            Box::pin(async { Ok(Vec::new()) })
        }
        fn complete(&self, _: i64) -> OutboxFuture<'_, ()> {
            Box::pin(async { Ok(()) })
        }
        fn retry_after<'a>(&'a self, _: i64, _: &'a str, _: Duration) -> OutboxFuture<'a, ()> {
            Box::pin(async { Ok(()) })
        }
        fn pending(&self) -> OutboxFuture<'_, usize> {
            Box::pin(async { Ok(0) })
        }
    }
    let mut s = setup(&["http://a.example.com/h"]).await;
    let lanes = PushLanes::with_outbox(
        Arc::clone(&s.store) as Arc<dyn PushConfigStore>,
        Some(Arc::clone(&s.sender) as Arc<dyn PushSender>),
        HandlerLimits::default(),
        Arc::clone(&s.metrics) as Arc<dyn Metrics>,
        TaskTracker::new(),
        Some((Arc::new(Refuses) as Arc<dyn PushOutbox>, s.shutdown.clone())),
    );
    lanes.push(
        &TaskId::new("t1"),
        &make_status_event("t1", TaskState::Working),
    );
    let seen = s.seen.recv().await.unwrap();
    assert_eq!(seen.delivery_id, None, "from memory, no outbox row");
    assert_eq!(seen.state, TaskState::Working);
}
