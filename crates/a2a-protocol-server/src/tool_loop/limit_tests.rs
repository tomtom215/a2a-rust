// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! [`InferenceLimit`] and [`LimitedModel`]: turns in flight never exceed the
//! limit, the limit is shared by clones, and a slot is held for one turn.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use super::*;
use crate::tool_loop::model::ChatMessage;

/// A model whose every turn takes 100 ms and records how many turns were in
/// flight at once; it fails when told to.
#[derive(Debug, Default)]
struct Slow {
    now: AtomicUsize,
    most: AtomicUsize,
    fail: bool,
}

impl ToolModel for Slow {
    fn model_name(&self) -> &'static str {
        "slow"
    }
    fn provider_name(&self) -> &'static str {
        "test"
    }
    fn server(&self) -> Option<(&str, u16)> {
        Some(("model.internal", 8080))
    }
    fn complete<'a>(
        &'a self,
        _request: ChatRequest<'a>,
    ) -> Pin<Box<dyn Future<Output = Result<ModelTurn, ModelError>> + Send + 'a>> {
        Box::pin(async move {
            let n = self.now.fetch_add(1, Ordering::SeqCst) + 1;
            self.most.fetch_max(n, Ordering::SeqCst);
            tokio::time::sleep(Duration::from_millis(100)).await;
            self.now.fetch_sub(1, Ordering::SeqCst);
            if self.fail {
                Err(ModelError::Other("refused".to_owned()))
            } else {
                Ok(ModelTurn::new("ok", Vec::new()))
            }
        })
    }
}

fn request(messages: &[ChatMessage]) -> ChatRequest<'_> {
    ChatRequest::new("", messages, &[])
}

/// Six turns at once through two models sharing a limit of two: never more
/// than two in flight, all six answered, in three rounds of 100 ms.
#[tokio::test(start_paused = true)]
async fn turns_in_flight_never_exceed_the_shared_limit() {
    let limit = InferenceLimit::new(2);
    let inner = Arc::new(Slow::default());
    let a = Arc::new(LimitedModel::new(Arc::clone(&inner), &limit));
    let b = Arc::new(LimitedModel::new(Arc::clone(&inner), &limit.clone()));
    let started = tokio::time::Instant::now();
    let mut turns = tokio::task::JoinSet::new();
    for i in 0..6 {
        let model = if i % 2 == 0 {
            Arc::clone(&a)
        } else {
            Arc::clone(&b)
        };
        turns.spawn(async move {
            let messages = [ChatMessage::User("hi".to_owned())];
            model.complete(request(&messages)).await
        });
    }
    let mut answered = 0;
    while let Some(turn) = turns.join_next().await {
        assert_eq!(turn.unwrap().unwrap().text, "ok");
        answered += 1;
    }
    assert_eq!(answered, 6);
    assert_eq!(inner.most.load(Ordering::SeqCst), 2);
    assert_eq!(started.elapsed(), Duration::from_millis(300));
    assert_eq!(limit.available(), 2, "every slot is given back");
}

/// A failed turn gives its slot back too, and the streaming path is held to
/// the limit as the plain one is.
#[tokio::test(start_paused = true)]
async fn a_failed_turn_and_a_streamed_turn_each_hold_one_slot() {
    let limit = InferenceLimit::new(1);
    let failing = LimitedModel::new(
        Slow {
            fail: true,
            ..Slow::default()
        },
        &limit,
    );
    let messages = [ChatMessage::User("hi".to_owned())];
    let err = failing.complete(request(&messages)).await.unwrap_err();
    assert_eq!(err, ModelError::Other("refused".to_owned()));
    assert_eq!(limit.available(), 1);

    let streaming = Arc::new(LimitedModel::new(Slow::default(), &limit));
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let sink = TextSink::new(1, tx);
    let held = {
        let model = Arc::clone(&streaming);
        tokio::spawn(async move {
            let messages = [ChatMessage::User("hi".to_owned())];
            model
                .complete_streaming(request(&messages), &sink)
                .await
                .map(|t| t.text)
        })
    };
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(limit.available(), 0, "the streamed turn holds the slot");
    assert_eq!(held.await.unwrap().unwrap(), "ok");
    assert_eq!(limit.available(), 1);
}

/// The wrapper is the model it wraps to the loop's telemetry.
#[test]
fn the_wrapper_names_the_inner_model_and_its_server() {
    let limit = InferenceLimit::new(3);
    let model = LimitedModel::new(Slow::default(), &limit);
    assert_eq!(model.model_name(), "slow");
    assert_eq!(model.provider_name(), "test");
    assert_eq!(model.server(), Some(("model.internal", 8080)));
    assert_eq!(model.inner().model_name(), "slow");
    assert_eq!(model.limit().max(), 3);
}

/// A limit of zero is one, and the `Debug` forms say the limit and what is
/// free of it.
#[test]
fn a_zero_limit_is_one_and_debug_says_what_is_free() {
    assert_eq!(InferenceLimit::new(0).max(), 1);
    assert_eq!(InferenceLimit::new(0).available(), 1);
    let limit = InferenceLimit::new(2);
    assert_eq!(
        format!("{limit:?}"),
        "InferenceLimit { max: 2, available: 2, .. }"
    );
    let model = LimitedModel::new(Slow::default(), &limit);
    assert_eq!(
        format!("{model:?}"),
        "LimitedModel { inner: Slow { now: 0, most: 0, fail: false }, \
         limit: InferenceLimit { max: 2, available: 2, .. } }"
    );
}

impl ToolModel for Arc<Slow> {
    fn model_name(&self) -> &'static str {
        "slow"
    }
    fn provider_name(&self) -> &'static str {
        "test"
    }
    fn complete<'a>(
        &'a self,
        request: ChatRequest<'a>,
    ) -> Pin<Box<dyn Future<Output = Result<ModelTurn, ModelError>> + Send + 'a>> {
        self.as_ref().complete(request)
    }
}
