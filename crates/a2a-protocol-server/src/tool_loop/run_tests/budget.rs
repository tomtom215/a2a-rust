// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! A run held to the caller's budget (ADR 0023): reported usage charged to
//! the tokens meter, and the run ended once that or the deadline is spent.

use std::time::{Duration, Instant};

use a2a_protocol_types::budget::{Budget, BudgetHandle, Exhausted, TOKENS_METER};

use super::*;
use crate::tool_loop::model::TokenUsage;

fn costing(
    reply: Result<ModelTurn, ModelError>,
    input: u64,
    output: u64,
) -> Result<ModelTurn, ModelError> {
    reply.map(|t| t.with_usage(TokenUsage::new(Some(input), Some(output))))
}

fn tokens(n: u64) -> BudgetHandle {
    BudgetHandle::start(&Budget::new().with_meter(TOKENS_METER, n).unwrap())
}

async fn run_in(tool_loop: &ToolLoop, budget: &BudgetHandle) -> Result<Answer, LoopError> {
    tool_loop
        .run_within("how is checkout?", &CancellationToken::new(), Some(budget))
        .await
}

/// Each turn costs 60 tokens of a 100-token budget: the second turn spends
/// it, and the run stops there instead of running a third.
#[tokio::test]
async fn the_run_stops_once_reported_usage_spends_the_tokens() {
    let (status, used) = status_tool();
    let reply = costing(
        calls(vec![call(
            "c",
            "service_status",
            r#"{"service":"checkout"}"#,
        )]),
        40,
        20,
    );
    let (model, seen) = Scripted::forever(reply.unwrap());
    let tool_loop = ToolLoop::new(model, tools(vec![status]));
    let budget = tokens(100);
    let err = run_in(&tool_loop, &budget).await.unwrap_err();
    assert_eq!(
        err,
        LoopError::Budget(Exhausted::Meter {
            name: TOKENS_METER.to_owned(),
            overdraft: 20
        })
    );
    assert_eq!(seen.lock().unwrap().len(), 2, "no third turn");
    assert_eq!(
        used.load(Ordering::SeqCst),
        1,
        "the overspending turn's calls are not run"
    );
    assert_eq!(budget.status().spent[TOKENS_METER], 120);
}

/// Within the budget the answer comes back and the spend is recorded.
#[tokio::test]
async fn a_run_within_its_tokens_answers_and_records_the_spend() {
    let (status, _) = status_tool();
    let (model, _) = Scripted::new(vec![costing(text("fine"), 7, 3)]);
    let tool_loop = ToolLoop::new(model, tools(vec![status]));
    let budget = tokens(100);
    assert_eq!(run_in(&tool_loop, &budget).await.unwrap().text, "fine");
    assert_eq!(budget.remaining(TOKENS_METER), Some(90));
}

/// A provider that reports no usage charges nothing, and a budget without
/// a tokens meter does not limit the run.
#[tokio::test]
async fn unreported_usage_or_no_tokens_meter_charges_nothing() {
    let (status, _) = status_tool();
    let (model, _) = Scripted::new(vec![text("fine")]);
    let budget = tokens(1);
    let answer = run_in(&ToolLoop::new(model, tools(vec![status])), &budget).await;
    assert!(answer.is_ok());
    assert_eq!(budget.remaining(TOKENS_METER), Some(1));

    let (status, _) = status_tool();
    let (model, _) = Scripted::new(vec![costing(text("fine"), 1_000, 1_000)]);
    let time_only = BudgetHandle::start(&Budget::new().with_time(Duration::from_secs(60)));
    assert!(
        run_in(&ToolLoop::new(model, tools(vec![status])), &time_only)
            .await
            .is_ok()
    );
}

/// A meter spent exactly is spent: no further turn is asked for.
#[tokio::test]
async fn no_turn_starts_with_no_tokens_left() {
    let (status, _) = status_tool();
    let reply = costing(
        calls(vec![call(
            "c",
            "service_status",
            r#"{"service":"checkout"}"#,
        )]),
        30,
        20,
    );
    let (model, seen) = Scripted::forever(reply.unwrap());
    let budget = tokens(50);
    let err = run_in(&ToolLoop::new(model, tools(vec![status])), &budget)
        .await
        .unwrap_err();
    assert_eq!(
        err,
        LoopError::Budget(Exhausted::Meter {
            name: TOKENS_METER.to_owned(),
            overdraft: 0
        })
    );
    assert_eq!(
        seen.lock().unwrap().len(),
        1,
        "one turn spent it; none overdrew it"
    );
}

/// No turn starts once the deadline has passed.
#[tokio::test]
async fn no_turn_starts_past_the_deadline() {
    let (status, _) = status_tool();
    let (model, seen) = Scripted::new(vec![text("fine")]);
    let gone = BudgetHandle::start_at(
        &Budget::new().with_time(Duration::from_millis(1)),
        Instant::now().checked_sub(Duration::from_secs(1)).unwrap(),
    );
    let err = run_in(&ToolLoop::new(model, tools(vec![status])), &gone)
        .await
        .unwrap_err();
    assert_eq!(err, LoopError::Budget(Exhausted::Time));
    assert!(seen.lock().unwrap().is_empty());
    assert_eq!(err.to_string(), "the budget's deadline has passed");
}
