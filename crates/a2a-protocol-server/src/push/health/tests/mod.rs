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

mod gone;
mod parts;
mod suspension;
