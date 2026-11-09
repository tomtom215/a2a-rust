// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Agent Skills through a real `RequestHandler` (ADR 0035): a skills
//! directory on disk, `ToolLoopExecutor::with_skills`, a scripted model that
//! activates a skill and reads one of its files, and the task a caller sees.

use std::collections::VecDeque;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use a2a_protocol_server::builder::RequestHandlerBuilder;
use a2a_protocol_server::handler::{RequestHandler, SendMessageResult};
use a2a_protocol_server::skills::{DiagnosticKind, SkillCatalog, SkillSource, SkillsConfig};
use a2a_protocol_server::tool_loop::{
    ANSWER_ARTIFACT_ID, ChatRequest, ModelError, ModelTurn, TRACE_ARTIFACT_ID, ToolCall, ToolLoop,
    ToolLoopExecutor, ToolModel, ToolSet, ToolSpec,
};
use a2a_protocol_types::message::Message;
use a2a_protocol_types::params::MessageSendParams;
use a2a_protocol_types::responses::SendMessageResponse;
use a2a_protocol_types::task::{Task, TaskState};

/// A directory under the temporary directory, removed on drop.
struct Dir(PathBuf);

impl Dir {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("a2a-skills-e2e-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&path).unwrap();
        Self(path.canonicalize().unwrap())
    }

    fn write(&self, relative: &str, contents: &str) {
        let path = self.0.join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, contents).unwrap();
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// What a turn was sent: the preamble and the tools.
type Seen = Arc<Mutex<Vec<(String, Vec<ToolSpec>)>>>;

/// Answers with the next scripted turn, recording what each turn was sent.
struct Recording {
    replies: Mutex<VecDeque<ModelTurn>>,
    seen: Seen,
}

impl ToolModel for Recording {
    fn model_name(&self) -> &'static str {
        "recording"
    }
    fn provider_name(&self) -> &'static str {
        "test"
    }
    fn complete<'a>(
        &'a self,
        request: ChatRequest<'a>,
    ) -> Pin<Box<dyn Future<Output = Result<ModelTurn, ModelError>> + Send + 'a>> {
        self.seen
            .lock()
            .unwrap()
            .push((request.preamble.to_owned(), request.tools.to_vec()));
        let turn = self
            .replies
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or_else(|| ModelTurn::new("script exhausted", Vec::new()));
        Box::pin(async move { Ok(turn) })
    }
}

fn asks(name: &str, arguments: &str) -> ModelTurn {
    ModelTurn::new("", vec![ToolCall::new("c1", name, arguments)])
}

fn handler(dir: &Dir, replies: Vec<ModelTurn>) -> (Arc<RequestHandler>, Seen) {
    let catalog = SkillCatalog::discover(
        &SkillsConfig::new().with_source(SkillSource::operator(dir.path().join("skills"))),
    );
    // The fixture is clean but for the note that a script is not run.
    let said: Vec<_> = catalog.diagnostics().iter().map(|d| d.kind()).collect();
    assert_eq!(said, [DiagnosticKind::ScriptsNotRun]);
    let seen = Seen::default();
    let model = Recording {
        replies: Mutex::new(replies.into()),
        seen: Arc::clone(&seen),
    };
    let executor = ToolLoopExecutor::new(
        ToolLoop::new(model, ToolSet::new()).with_preamble("You are a billing agent."),
    )
    .with_skills(&catalog)
    .expect("no tool of this name");
    let handler = RequestHandlerBuilder::new(executor)
        .without_agent_card()
        .build()
        .unwrap();
    (Arc::new(handler), seen)
}

async fn blocking(handler: &RequestHandler, text: &str) -> Task {
    let params = MessageSendParams::new(Message::user_text("m1", text));
    match handler.on_send_message(params, false, None).await {
        Ok(SendMessageResult::Response(SendMessageResponse::Task(task))) => task,
        other => panic!("expected a task, got {other:?}"),
    }
}

fn artifact(task: &Task, id: &str) -> Option<String> {
    task.artifacts
        .as_deref()?
        .iter()
        .find(|a| a.id.0 == id)
        .and_then(|a| a.parts[0].text_content())
        .map(str::to_owned)
}

fn fixture() -> Dir {
    let dir = Dir::new();
    dir.write(
        "skills/refunds/SKILL.md",
        "---\nname: refunds\ndescription: How to refund a payment. Use when a customer asks for their money back.\n---\n# Refunds\n\nRead references/policy.md first.\n",
    );
    dir.write(
        "skills/refunds/references/policy.md",
        "Refunds within 30 days.",
    );
    dir.write(
        "skills/refunds/scripts/refund.sh",
        "#!/bin/sh\necho never run\n",
    );
    dir.write(
        "skills/invoices/SKILL.md",
        "---\nname: invoices\ndescription: How to issue an invoice.\n---\nIssue it.\n",
    );
    dir
}

#[tokio::test]
async fn a_skill_is_activated_and_one_of_its_files_read_in_one_task() {
    let dir = fixture();
    let (handler, seen) = handler(
        &dir,
        vec![
            asks("activate_skill", r#"{"name":"refunds"}"#),
            asks(
                "read_skill_file",
                r#"{"skill":"refunds","path":"references/policy.md"}"#,
            ),
            ModelTurn::new("Refunds are allowed within 30 days.", Vec::new()),
        ],
    );
    let task = blocking(&handler, "I want my money back").await;
    assert_eq!(task.status.state, TaskState::Completed);
    assert_eq!(
        artifact(&task, ANSWER_ARTIFACT_ID).as_deref(),
        Some("Refunds are allowed within 30 days.")
    );
    let trace = artifact(&task, TRACE_ARTIFACT_ID).expect("a trace");
    let lines: Vec<&str> = trace.lines().collect();
    assert!(
        lines[0]
            .starts_with(r#"activate_skill({"name":"refunds"}) -> <skill_content name="refunds">"#),
        "{trace}"
    );
    assert!(
        trace.contains("Read references/policy.md first."),
        "{trace}"
    );
    assert!(
        trace.contains("<file>references/policy.md</file>"),
        "{trace}"
    );
    assert!(trace.contains("<file>scripts/refund.sh</file>"), "{trace}");
    assert!(trace.contains("This agent cannot run them"), "{trace}");
    assert!(
        trace.ends_with(
            r#"read_skill_file({"skill":"refunds","path":"references/policy.md"}) -> Refunds within 30 days."#
        ),
        "{trace}"
    );

    // What the model was told.
    let seen = seen.lock().unwrap();
    assert_eq!(seen.len(), 3);
    let (preamble, tools) = &seen[0];
    assert!(
        preamble.starts_with("You are a billing agent.\n\n"),
        "{preamble}"
    );
    assert!(preamble.contains("<name>refunds</name>"), "{preamble}");
    assert!(preamble.contains("<name>invoices</name>"), "{preamble}");
    assert!(
        preamble.contains("Use when a customer asks for their money back."),
        "{preamble}"
    );
    let names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
    assert_eq!(names, ["activate_skill", "read_skill_file"]);
    assert_eq!(
        tools[0].parameters["properties"]["name"]["enum"],
        serde_json::json!(["invoices", "refunds"])
    );
    assert_eq!(
        tools[1].parameters["properties"]["skill"]["enum"],
        serde_json::json!(["refunds"])
    );
}

#[tokio::test]
async fn a_request_for_a_file_outside_the_skill_is_an_error_the_model_reads_and_the_task_completes()
{
    let dir = fixture();
    dir.write("secret.txt", "the host's secret");
    let (handler, _) = handler(
        &dir,
        vec![
            asks(
                "read_skill_file",
                r#"{"skill":"refunds","path":"../../secret.txt"}"#,
            ),
            asks(
                "read_skill_file",
                r#"{"skill":"refunds","path":"scripts/refund.sh"}"#,
            ),
            asks("activate_skill", r#"{"name":"../../secret.txt"}"#),
            ModelTurn::new("I could not.", Vec::new()),
        ],
    );
    let task = blocking(&handler, "read the secret").await;
    assert_eq!(task.status.state, TaskState::Completed);
    let trace = artifact(&task, TRACE_ARTIFACT_ID).expect("a trace");
    assert!(!trace.contains("the host's secret"), "{trace}");
    assert!(!trace.contains("never run"), "{trace}");
    let lines: Vec<&str> = trace.lines().collect();
    assert_eq!(lines.len(), 3, "{trace}");
    assert!(
        lines[0].ends_with("-> error: the path contains `..`"),
        "{trace}"
    );
    assert!(
        lines[1].ends_with("-> error: only files under references/ and assets/ can be read"),
        "{trace}"
    );
    assert!(
        lines[2].contains(
            "-> error: no skill named \"../../secret.txt\"; the skills are: invoices, refunds"
        ),
        "{trace}"
    );
}
