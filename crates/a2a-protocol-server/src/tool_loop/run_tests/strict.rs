// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The strict profile: what the caller's trace holds, which calls written as
//! text run, and a policy asked before every call (adversarial review T1, T2,
//! T3, 2026-11-12).

use super::*;
use crate::tool_loop::policy::{PolicyDecision, PolicyRequest, TextCallRecovery, ToolPolicy};
use std::sync::Mutex as StdMutex;

const SECRET_ARGUMENT: &str = "ssn-078-05-1120";
const SECRET_RESULT: &str = "internal-account-4417";

/// A tool that mutates (does not declare itself read-only) and returns a secret.
pub(super) fn mutating() -> (TestTool, Arc<AtomicUsize>) {
    TestTool::new("transfer", |_| Ok(format!("done {SECRET_RESULT}")))
}

fn secret_call() -> String {
    format!(r#"{{"name": "transfer", "arguments": {{"service": "{SECRET_ARGUMENT}"}}}}"#)
}

// ── T3: text-call recovery ───────────────────────────────────────────────────

/// A mutating tool named in an exact JSON answer runs by default, as it always
/// has, and does not under `Off` or `ReadOnly`: a requested JSON example stays
/// an example.
#[tokio::test]
async fn a_mutating_tool_runs_from_text_only_where_the_recovery_policy_allows() {
    for (recovery, runs) in [
        (TextCallRecovery::All, 1),
        (TextCallRecovery::ReadOnly, 0),
        (TextCallRecovery::Off, 0),
    ] {
        let (transfer, used) = mutating();
        let (model, _) = Scripted::new(vec![text(&secret_call()), text("done")]);
        let tool_loop =
            ToolLoop::new(model, tools(vec![transfer])).with_text_call_recovery(recovery);
        let answer = run(&tool_loop).await.unwrap();
        assert_eq!(used.load(Ordering::SeqCst), runs, "{recovery:?}");
        if runs == 0 {
            assert_eq!(
                answer.text,
                secret_call(),
                "{recovery:?}: the example is the answer"
            );
        }
    }
}

/// `ReadOnly` still recovers a tool that declares itself read-only.
#[tokio::test]
async fn read_only_recovery_still_runs_a_read_only_tool() {
    let (status, used) = status_tool();
    let written = r#"{"name": "service_status", "arguments": {"service": "checkout"}}"#;
    let (model, _) = Scripted::new(vec![text(written), text("healthy")]);
    let tool_loop = ToolLoop::new(model, tools(vec![status]))
        .with_text_call_recovery(TextCallRecovery::ReadOnly);
    assert_eq!(run(&tool_loop).await.unwrap().text, "healthy");
    assert_eq!(used.load(Ordering::SeqCst), 1);
}

/// `with_recover_text_calls(bool)` keeps its meaning.
#[tokio::test]
async fn the_boolean_setter_is_all_or_off() {
    let (model, _) = Scripted::new(vec![text("x")]);
    let all = ToolLoop::new(model, ToolSet::new()).with_recover_text_calls(true);
    assert!(format!("{all:?}").contains("All"));
    let (model, _) = Scripted::new(vec![text("x")]);
    let off = ToolLoop::new(model, ToolSet::new()).with_recover_text_calls(false);
    assert!(format!("{off:?}").contains("Off"));
}

// ── T1: what the caller's trace holds ────────────────────────────────────────

/// By default the trace is what it always was: arguments and result.
#[tokio::test]
async fn the_default_trace_holds_the_arguments_and_the_result() {
    let (transfer, _) = mutating();
    let (model, _) = Scripted::new(vec![
        calls(vec![call(
            "c1",
            "transfer",
            &format!(r#"{{"service":"{SECRET_ARGUMENT}"}}"#),
        )]),
        text("done"),
    ]);
    let answer = run(&ToolLoop::new(model, tools(vec![transfer])))
        .await
        .unwrap();
    let trace = answer.trace.join("\n");
    assert!(
        trace.contains(SECRET_ARGUMENT) && trace.contains(SECRET_RESULT),
        "{trace}"
    );
}

/// Under a receipt trace neither the arguments nor the result reach the
/// caller, on success, on a failed call and on a call that is not run again —
/// but the model still sees both.
#[tokio::test]
async fn a_receipt_trace_holds_neither_the_arguments_nor_the_result() {
    let arguments = format!(r#"{{"service":"{SECRET_ARGUMENT}"}}"#);
    let (transfer, _) = mutating();
    let (failing, _) = TestTool::new("audit", |_| {
        Err(ToolError::Failed(format!("{SECRET_RESULT} refused")))
    });
    let (model, seen) = Scripted::new(vec![
        calls(vec![
            call("c1", "transfer", &arguments),
            call("c2", "audit", &arguments),
            call("c3", "transfer", &arguments), // identical: not run again
            call("c4", "nosuchtool", &arguments),
        ]),
        text("done"),
    ]);
    let tool_loop = ToolLoop::strict(model, tools(vec![transfer, failing]));
    let answer = run(&tool_loop).await.unwrap();
    let trace = answer.trace.join("\n");
    assert!(
        !trace.contains(SECRET_ARGUMENT),
        "arguments reached the caller: {trace}"
    );
    assert!(
        !trace.contains(SECRET_RESULT),
        "a result reached the caller: {trace}"
    );
    assert_eq!(
        answer.trace,
        [
            "transfer [c1]: ran",
            "audit [c2]: failed",
            "transfer [c3]: not run again",
            "nosuchtool [c4]: failed",
        ]
    );
    // The model, which needs them, still gets the result.
    let results = results(&seen.lock().unwrap().clone(), 1);
    assert!(results[0].1.contains(SECRET_RESULT), "{results:?}");
}

// ── T2: a policy before every call ───────────────────────────────────────────

/// Records what it was asked and refuses the tool named `transfer` for
/// `mallory`.
pub(super) struct Gate {
    pub(super) asked: Arc<StdMutex<Vec<Asked>>>,
}

/// What a policy was asked: the tool, whether recovered from text, the tenant
/// and the principal.
type Asked = (String, bool, Option<String>, Option<String>);

impl ToolPolicy for Gate {
    fn authorize<'a>(
        &'a self,
        request: &'a PolicyRequest<'a>,
    ) -> Pin<Box<dyn Future<Output = PolicyDecision> + Send + 'a>> {
        self.asked.lock().unwrap().push((
            request.tool.to_owned(),
            request.recovered_from_text,
            request.tenant.map(str::to_owned),
            request.principal.map(str::to_owned),
        ));
        let deny = request.tool == "transfer" && request.principal == Some("mallory");
        Box::pin(async move {
            if deny {
                PolicyDecision::Deny("mallory may not transfer".to_owned())
            } else {
                PolicyDecision::Allow
            }
        })
    }
}

pub(super) fn identity(principal: &'static str) -> crate::tool_loop::TrustedIdentity {
    crate::tool_loop::TrustedIdentity {
        tenant: Some("acme".to_owned()),
        principal: Some(principal.to_owned()),
        task: Some("task-1".to_owned()),
    }
}

pub(super) async fn run_as(
    tool_loop: &ToolLoop,
    who: &crate::tool_loop::TrustedIdentity,
) -> Result<Answer, LoopError> {
    use crate::tool_loop::steering::Side;
    tool_loop
        .run_drafting(
            "scope",
            "go",
            &CancellationToken::new(),
            None,
            Side {
                identity: Some(who),
                ..Side::default()
            },
        )
        .await
}

/// A denied call is not run, for a native call and for one recovered from text,
/// and the model is told why; an allowed principal's identical call runs. The
/// policy is given the request's identity.
#[tokio::test]
async fn a_policy_is_asked_before_every_call_and_can_refuse_it() {
    for (native, label) in [(true, "native"), (false, "recovered")] {
        let (transfer, used) = mutating();
        let first = if native {
            calls(vec![call("c1", "transfer", "{}")])
        } else {
            text(r#"{"name": "transfer", "arguments": {}}"#)
        };
        let (model, seen) = Scripted::new(vec![first, text("ok")]);
        let asked = Arc::new(StdMutex::new(Vec::new()));
        let tool_loop = ToolLoop::new(model, tools(vec![transfer])).with_policy(Gate {
            asked: Arc::clone(&asked),
        });
        let answer = run_as(&tool_loop, &identity("mallory")).await.unwrap();
        assert_eq!(
            used.load(Ordering::SeqCst),
            0,
            "{label}: a refused call ran"
        );
        assert_eq!(answer.text, "ok");
        let told = results(&seen.lock().unwrap().clone(), 1);
        assert!(
            told[0].1.contains("mallory may not transfer"),
            "{label}: {told:?}"
        );
        assert_eq!(
            asked.lock().unwrap().as_slice(),
            [(
                "transfer".to_owned(),
                !native,
                Some("acme".to_owned()),
                Some("mallory".to_owned())
            )],
            "{label}"
        );

        // Another principal's identical call is allowed and runs.
        let (transfer, used) = mutating();
        let (model, _) = Scripted::new(vec![
            if native {
                calls(vec![call("c1", "transfer", "{}")])
            } else {
                text(r#"{"name": "transfer", "arguments": {}}"#)
            },
            text("ok"),
        ]);
        let tool_loop = ToolLoop::new(model, tools(vec![transfer])).with_policy(Gate {
            asked: Arc::default(),
        });
        run_as(&tool_loop, &identity("alice")).await.unwrap();
        assert_eq!(
            used.load(Ordering::SeqCst),
            1,
            "{label}: an allowed call did not run"
        );
    }
}

/// A name that is not a tool is answered by the loop before the policy is asked.
#[tokio::test]
async fn an_unknown_tool_is_not_put_to_the_policy() {
    let (model, _) = Scripted::new(vec![
        calls(vec![call("c1", "nosuchtool", "{}")]),
        text("ok"),
    ]);
    let asked = Arc::new(StdMutex::new(Vec::new()));
    let tool_loop = ToolLoop::new(model, ToolSet::new()).with_policy(Gate {
        asked: Arc::clone(&asked),
    });
    run(&tool_loop).await.unwrap();
    assert!(asked.lock().unwrap().is_empty());
}
