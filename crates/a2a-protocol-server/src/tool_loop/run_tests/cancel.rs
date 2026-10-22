// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Cancellation at each point a run checks it, and the tool set's
//! refusal of a duplicate name.

use super::*;

#[tokio::test]
async fn cancellation_between_turns_stops_before_the_next_turn() {
    let cancel = CancellationToken::new();
    let token = cancel.clone();
    let (tool, used) = TestTool::new("service_status", move |_| {
        token.cancel();
        Ok("ok".to_owned())
    });
    let (model, seen) = Scripted::new(vec![
        calls(vec![call("c1", "service_status", "{}")]),
        text("too late"),
    ]);
    let tool_loop = ToolLoop::new(model, tools(vec![tool]));
    let err = tool_loop.run("go", &cancel).await.unwrap_err();
    assert_eq!(err, LoopError::Cancelled);
    assert_eq!(used.load(Ordering::SeqCst), 1);
    assert_eq!(seen.lock().unwrap().len(), 1, "no second turn");
}

#[tokio::test]
async fn cancellation_between_calls_stops_before_the_next_call() {
    let cancel = CancellationToken::new();
    let token = cancel.clone();
    let (first, _) = TestTool::new("first", move |_| {
        token.cancel();
        Ok("ok".to_owned())
    });
    let (second, used) = TestTool::new("second", |_| Ok("ok".to_owned()));
    let (model, _) = Scripted::new(vec![calls(vec![
        call("c1", "first", "{}"),
        call("c2", "second", "{}"),
    ])]);
    let tool_loop = ToolLoop::new(model, tools(vec![first, second]));
    assert_eq!(
        tool_loop.run("go", &cancel).await.unwrap_err(),
        LoopError::Cancelled
    );
    assert_eq!(used.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn an_already_cancelled_run_asks_the_model_nothing() {
    let cancel = CancellationToken::new();
    cancel.cancel();
    let (model, seen) = Scripted::new(vec![text("hello")]);
    let tool_loop = ToolLoop::new(model, ToolSet::new());
    assert_eq!(
        tool_loop.run("go", &cancel).await.unwrap_err(),
        LoopError::Cancelled
    );
    assert!(seen.lock().unwrap().is_empty());
}

#[tokio::test]
async fn cancellation_abandons_a_turn_in_progress() {
    let cancel = CancellationToken::new();
    let tool_loop = ToolLoop::new(Hanging, ToolSet::new());
    let token = cancel.clone();
    tokio::spawn(async move {
        tokio::task::yield_now().await;
        token.cancel();
    });
    let outcome = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        tool_loop.run("go", &cancel),
    )
    .await
    .expect("the loop returned once cancelled");
    assert_eq!(outcome.unwrap_err(), LoopError::Cancelled);
}

#[test]
fn a_second_tool_of_the_same_name_is_refused_and_the_set_unchanged() {
    let (a, _) = TestTool::new("service_status", |_| Ok("a".to_owned()));
    let (b, _) = TestTool::new("service_status", |_| Ok("b".to_owned()));
    let mut set = ToolSet::new().with(a).unwrap();
    assert_eq!(
        set.add(b).unwrap_err(),
        DuplicateTool("service_status".to_owned())
    );
    assert_eq!(set.len(), 1);
    let (c, _) = TestTool::new("service_status", |_| Ok("c".to_owned()));
    assert!(set.clone().with(c).is_err());
    let (d, _) = TestTool::new("list_services", |_| Ok("d".to_owned()));
    set.add(d).unwrap();
    assert_eq!(set.names(), ["service_status", "list_services"]);
    assert_eq!(set.len(), 2);
    assert!(!set.is_empty());
}
