// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The handler's push sender applies the dead-webhook policy (audit D-05):
//! the builder wraps whatever sender it was given, a custom one included, and
//! leaves it alone when both rules are off.

use std::future::Future;
use std::pin::Pin;

use a2a_protocol_types::error::{A2aError, A2aResult};
use a2a_protocol_types::events::{StreamResponse, TaskStatusUpdateEvent};
use a2a_protocol_types::push::TaskPushNotificationConfig;
use a2a_protocol_types::task::{ContextId, TaskId, TaskState, TaskStatus};

use crate::agent_executor;
use crate::builder::RequestHandlerBuilder;
use crate::handler::HandlerLimits;
use crate::metrics::push_outcome;
use crate::push::PushSender;

struct Idle;
agent_executor!(Idle, |_ctx, _queue| async { Ok(()) });

/// A custom sender whose every delivery fails.
struct Down;

impl PushSender for Down {
    fn send<'a>(
        &'a self,
        _url: &'a str,
        _event: &'a StreamResponse,
        _config: &'a TaskPushNotificationConfig,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async { Err(A2aError::internal("down")) })
    }
}

fn working() -> StreamResponse {
    StreamResponse::StatusUpdate(TaskStatusUpdateEvent::new(
        TaskId::new("t1"),
        ContextId::new("c1"),
        TaskStatus::new(TaskState::Working),
    ))
}

/// How many deliveries in a row, up to `tries`, were refused as suspended.
async fn suspended_after(limits: HandlerLimits, tries: usize) -> Option<usize> {
    let handler = RequestHandlerBuilder::new(Idle)
        .without_agent_card()
        .with_push_sender(Down)
        .with_handler_limits(limits)
        .build()
        .expect("the handler builds");
    let sender = handler.push_sender.as_deref().expect("a sender was given");
    let mut config = TaskPushNotificationConfig::new("t1", "https://hooks.example.com/a");
    config.id = Some("c1".to_owned());
    for attempt in 0..tries {
        let outcome = sender
            .send(&config.url, &working(), &config)
            .await
            .expect_err("every delivery fails");
        if crate::push::health::outcome_of(&outcome) == Some(push_outcome::SUSPENDED) {
            return Some(attempt);
        }
    }
    None
}

#[tokio::test]
async fn the_handlers_sender_suspends_a_webhook_after_the_configured_failures() {
    assert_eq!(
        suspended_after(HandlerLimits::default(), 20).await,
        Some(5),
        "five failures at the default, the sixth is skipped"
    );
    assert_eq!(
        suspended_after(HandlerLimits::default().with_push_suspend_after(2), 20).await,
        Some(2)
    );
}

#[tokio::test]
async fn with_both_rules_off_the_sender_is_the_one_given() {
    let off = HandlerLimits::default()
        .with_push_suspend_after(0)
        .with_push_remove_gone(false);
    assert_eq!(suspended_after(off, 50).await, None, "never suspended");
}
