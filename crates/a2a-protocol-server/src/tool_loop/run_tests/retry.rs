// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! A rate-limited turn sent again (ADR 0048): only with a policy, only a
//! 429, after the provider's delay, within the budget, and not after a
//! cancel.

use std::time::Duration;

use a2a_protocol_types::budget::{Budget, BudgetHandle};

use super::*;
use crate::tool_loop::ModelRetry;

fn limited(after: Option<u64>) -> Result<ModelTurn, ModelError> {
    Err(ModelError::RateLimited {
        retry_after: after.map(Duration::from_secs),
        body: String::new(),
    })
}

fn retrying(model: Scripted, retry: ModelRetry) -> ToolLoop {
    let (status, _) = status_tool();
    ToolLoop::new(model, tools(vec![status])).with_model_retry(retry)
}

/// The provider says "in two seconds": the loop waits that long (spread by
/// at most a quarter) and sends the same conversation again, which answers.
#[tokio::test(start_paused = true)]
async fn a_rate_limited_turn_is_sent_again_after_the_providers_delay() {
    let (model, seen) = Scripted::new(vec![limited(Some(2)), text("fine")]);
    let tool_loop = retrying(model, ModelRetry::new());
    let started = tokio::time::Instant::now();
    let answer = run(&tool_loop).await.unwrap();
    let waited = started.elapsed();
    assert_eq!(answer.text, "fine");
    let seen = seen.lock().unwrap();
    assert_eq!(seen.len(), 2, "one refused turn, one answered");
    assert_eq!(seen[0].messages, seen[1].messages, "the same turn again");
    assert!(
        (Duration::from_secs(2)..=Duration::from_millis(2500)).contains(&waited),
        "the wait is the provider's 2 s spread by at most a quarter: {waited:?}"
    );
}

/// Without a policy a 429 ends the run as it always did, after one turn.
#[tokio::test(start_paused = true)]
async fn without_a_policy_a_rate_limited_turn_ends_the_run() {
    let (status, _) = status_tool();
    let (model, seen) = Scripted::new(vec![limited(Some(2)), text("fine")]);
    let err = run(&ToolLoop::new(model, tools(vec![status])))
        .await
        .unwrap_err();
    assert_eq!(err, LoopError::Model(limited(Some(2)).unwrap_err()));
    assert_eq!(seen.lock().unwrap().len(), 1);
}

/// Two retries allowed, three refusals: three turns sent, and the run ends
/// with the last refusal, after waiting 1 s twice (no more than 1.25 s each).
#[tokio::test(start_paused = true)]
async fn the_retries_end_at_the_policys_count() {
    let (model, seen) = Scripted::new(vec![
        limited(Some(1)),
        limited(Some(1)),
        limited(Some(1)),
        text("never"),
    ]);
    let tool_loop = retrying(model, ModelRetry::new().with_max_retries(2));
    let started = tokio::time::Instant::now();
    let err = run(&tool_loop).await.unwrap_err();
    let waited = started.elapsed();
    assert_eq!(err, LoopError::Model(limited(Some(1)).unwrap_err()));
    assert_eq!(seen.lock().unwrap().len(), 3);
    assert!(
        (Duration::from_secs(2)..=Duration::from_millis(2500)).contains(&waited),
        "{waited:?}"
    );
}

/// A delay past `max_delay`, or past what the budget leaves, is not waited:
/// the 429 ends the run at once.
#[tokio::test(start_paused = true)]
async fn a_delay_past_the_cap_or_the_budget_is_not_waited() {
    let (model, seen) = Scripted::new(vec![limited(Some(60)), text("never")]);
    let tool_loop = retrying(model, ModelRetry::new());
    let started = tokio::time::Instant::now();
    assert!(matches!(
        run(&tool_loop).await,
        Err(LoopError::Model(ModelError::RateLimited { .. }))
    ));
    assert_eq!(seen.lock().unwrap().len(), 1);
    assert_eq!(started.elapsed(), Duration::ZERO);

    let (model, seen) = Scripted::new(vec![limited(Some(5)), text("never")]);
    let tool_loop = retrying(model, ModelRetry::new());
    let budget = BudgetHandle::start(&Budget::new().with_time(Duration::from_secs(1)));
    let err = tool_loop
        .run_within("how is checkout?", &CancellationToken::new(), Some(&budget))
        .await
        .unwrap_err();
    assert_eq!(err, LoopError::Model(limited(Some(5)).unwrap_err()));
    assert_eq!(seen.lock().unwrap().len(), 1);
}

/// Only a rate-limited turn is retried: a 503 ends the run after one turn.
#[tokio::test(start_paused = true)]
async fn another_status_is_not_retried() {
    let unavailable = ModelError::Status {
        status: 503,
        body: String::new(),
    };
    let (model, seen) = Scripted::new(vec![Err(unavailable.clone()), text("never")]);
    let err = run(&retrying(model, ModelRetry::new())).await.unwrap_err();
    assert_eq!(err, LoopError::Model(unavailable));
    assert_eq!(seen.lock().unwrap().len(), 1);
}

/// A cancel during the wait ends the run as cancelled, then, not after the
/// wait.
#[tokio::test(start_paused = true)]
async fn a_cancel_during_the_wait_ends_the_run() {
    let (model, seen) = Scripted::new(vec![limited(Some(20)), text("never")]);
    let tool_loop = retrying(model, ModelRetry::new());
    let cancel = CancellationToken::new();
    let trigger = cancel.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(3)).await;
        trigger.cancel();
    });
    let started = tokio::time::Instant::now();
    let err = tool_loop
        .run("how is checkout?", &cancel)
        .await
        .unwrap_err();
    assert_eq!(err, LoopError::Cancelled);
    assert_eq!(started.elapsed(), Duration::from_secs(3));
    assert_eq!(seen.lock().unwrap().len(), 1);
}

/// The policy shows in the loop's `Debug`.
#[test]
fn the_policy_shows_in_debug() {
    let (model, _) = Scripted::new(vec![]);
    let plain = format!("{:?}", ToolLoop::new(model, ToolSet::new()));
    assert!(plain.contains("model_retry: None"), "{plain}");
    let (model, _) = Scripted::new(vec![]);
    let set = format!(
        "{:?}",
        ToolLoop::new(model, ToolSet::new())
            .with_model_retry(ModelRetry::new().with_max_retries(1))
    );
    assert!(
        set.contains("model_retry: Some(ModelRetry { max_retries: 1,"),
        "{set}"
    );
}
