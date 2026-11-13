// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! A call's arguments are checked against the tool's schema before the policy
//! is asked and the tool runs (adversarial review T2).

use super::strict::Gate;
use super::*;
use crate::tool_loop::ArgumentCheck;
use std::sync::Mutex as StdMutex;

/// A transfer tool whose schema says `amount` is a positive number.
fn transfer() -> (TestTool, Arc<AtomicUsize>) {
    let (tool, used) = TestTool::new("transfer", |_| Ok("done".to_owned()));
    let tool = tool.with_schema(json!({
        "type": "object",
        "required": ["amount"],
        "properties": {"amount": {"type": "number", "exclusiveMinimum": 0}},
        "additionalProperties": false
    }));
    (tool, used)
}

fn script(arguments: &str) -> Vec<Result<ModelTurn, ModelError>> {
    vec![calls(vec![call("c1", "transfer", arguments)]), text("done")]
}

async fn run_with(check: ArgumentCheck, arguments: &str) -> (usize, Vec<(String, String)>, Answer) {
    let (tool, used) = transfer();
    let (model, seen) = Scripted::new(script(arguments));
    let tool_loop = ToolLoop::new(model, tools(vec![tool])).with_argument_check(check);
    let answer = run(&tool_loop).await.unwrap();
    let told = results(&seen.lock().unwrap().clone(), 1);
    (used.load(Ordering::SeqCst), told, answer)
}

/// What the loop has always done: arguments that are an object are run.
#[tokio::test]
async fn by_default_the_schema_is_not_enforced() {
    let (ran, _, _) = run_with(ArgumentCheck::Off, r#"{"amount": "everything"}"#).await;
    assert_eq!(ran, 1);
}

/// A call that breaks the schema is not run, and the model is told what, in
/// words it can act on.
#[tokio::test]
async fn a_call_that_breaks_the_schema_is_not_run() {
    for arguments in [
        r#"{"amount": "everything"}"#,
        r"{}",
        r#"{"amount": -5}"#,
        r#"{"amount": 5, "memo": "x"}"#,
    ] {
        let (ran, told, answer) = run_with(ArgumentCheck::Checked, arguments).await;
        assert_eq!(ran, 0, "{arguments}");
        assert!(
            told[0]
                .1
                .starts_with("error: the arguments do not match this tool's schema"),
            "{arguments}: {told:?}"
        );
        assert_eq!(answer.text, "done", "the run goes on");
    }
    let (ran, _, _) = run_with(ArgumentCheck::Checked, r#"{"amount": 5}"#).await;
    assert_eq!(ran, 1, "a call that fits runs");
}

/// A policy is asked about calls that are well formed, not about the rest.
#[tokio::test]
async fn the_policy_is_asked_only_about_calls_that_fit() {
    let asked = Arc::new(StdMutex::new(Vec::new()));
    for (arguments, put_to_policy) in [(r#"{"amount": "x"}"#, 0), (r#"{"amount": 5}"#, 1)] {
        asked.lock().unwrap().clear();
        let (tool, _) = transfer();
        let (model, _) = Scripted::new(script(arguments));
        let tool_loop = ToolLoop::new(model, tools(vec![tool]))
            .with_argument_check(ArgumentCheck::Checked)
            .with_policy(Gate {
                asked: Arc::clone(&asked),
            });
        run(&tool_loop).await.unwrap();
        assert_eq!(asked.lock().unwrap().len(), put_to_policy, "{arguments}");
    }
}

/// Native, repeated and recovered calls are held to the same schema.
#[tokio::test]
async fn a_call_recovered_from_text_is_checked_too() {
    let (tool, used) = transfer();
    let (model, _) = Scripted::new(vec![
        text(r#"{"name": "transfer", "arguments": {"amount": "all of it"}}"#),
        text("done"),
    ]);
    let tool_loop =
        ToolLoop::new(model, tools(vec![tool])).with_argument_check(ArgumentCheck::Checked);
    run(&tool_loop).await.unwrap();
    assert_eq!(used.load(Ordering::SeqCst), 0);
}

/// `Strict` refuses a tool whose schema states what the loop cannot check;
/// `Checked` runs it with what it can.
#[tokio::test]
async fn strict_refuses_a_schema_it_cannot_fully_check() {
    for (check, ran) in [(ArgumentCheck::Strict, 0), (ArgumentCheck::Checked, 1)] {
        let (tool, used) = TestTool::new("transfer", |_| Ok("done".to_owned()));
        let tool = tool.with_schema(json!({
            "type": "object",
            "properties": {"account": {"type": "string", "pattern": "^[0-9]{8}$"}}
        }));
        let (model, seen) = Scripted::new(script(r#"{"account": "12345678"}"#));
        let tool_loop = ToolLoop::new(model, tools(vec![tool])).with_argument_check(check);
        run(&tool_loop).await.unwrap();
        assert_eq!(used.load(Ordering::SeqCst), ran, "{check:?}");
        if ran == 0 {
            let told = results(&seen.lock().unwrap().clone(), 1);
            assert!(told[0].1.contains("uses pattern"), "{told:?}");
            assert!(told[0].1.contains("the call was not run"), "{told:?}");
        }
    }
}

/// The strict profile checks arguments, and the receipt names the call as
/// failed, not the arguments.
#[tokio::test]
async fn the_strict_profile_enforces_the_schema() {
    let (tool, used) = transfer();
    let (model, _) = Scripted::new(script(r#"{"amount": "everything"}"#));
    let answer = run(&ToolLoop::strict(model, tools(vec![tool])))
        .await
        .unwrap();
    assert_eq!(used.load(Ordering::SeqCst), 0);
    assert_eq!(answer.trace, ["transfer [c1]: failed"]);
}
