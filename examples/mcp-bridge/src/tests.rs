// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Two layers, tested differently.
//!
//! [`crate::mapping`] is pure, so its awkward cases — a skill id that is not
//! a legal tool name, two that collide, a task state MCP has no word for —
//! are asserted directly on values. No server, no clock, no flake.
//!
//! [`crate::bridge`] is only meaningful against a real A2A agent, so these
//! start one on localhost and connect a real MCP client to the bridge over an
//! in-process pipe. Every frame in both directions is genuinely serialized.
//! What that leaves uncovered is the child-process spawn, which
//! `bin/demo.rs` exercises and the README's transcript records.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use a2a_protocol_server::builder::RequestHandlerBuilder;
use a2a_protocol_server::dispatch::JsonRpcDispatcher;
use a2a_protocol_server::executor::AgentExecutor;
use a2a_protocol_server::request_context::RequestContext;
use a2a_protocol_server::streaming::EventQueueWriter;
use a2a_protocol_types::agent_card::{AgentCapabilities, AgentCard, AgentInterface, AgentSkill};
use a2a_protocol_types::artifact::Artifact;
use a2a_protocol_types::error::{A2aError, A2aResult};
use a2a_protocol_types::events::{StreamResponse, TaskArtifactUpdateEvent, TaskStatusUpdateEvent};
use a2a_protocol_types::message::{Message, MessageId, MessageRole, Part};
use a2a_protocol_types::task::{ContextId, Task, TaskId, TaskState, TaskStatus};
use rmcp::ServiceExt;
use rmcp::model::{CallToolRequestParams, ContentBlock};
use serde_json::json;

use crate::bridge::{A2aBridge, BridgeError, SKILL_METADATA_KEY};
use crate::mapping::{self, MappingError};

// ── Fixtures ─────────────────────────────────────────────────────────────────

fn skill(id: &str) -> AgentSkill {
    AgentSkill {
        id: id.to_owned(),
        name: format!("{id} skill"),
        description: format!("does {id}"),
        tags: vec![],
        examples: None,
        input_modes: None,
        output_modes: None,
        security_requirements: None,
    }
}

fn card_with(url: &str, skills: Vec<AgentSkill>) -> AgentCard {
    AgentCard {
        url: Some(url.into()),
        name: "Test Agent".into(),
        description: "for tests".into(),
        version: "0.0.0".into(),
        supported_interfaces: vec![AgentInterface {
            url: url.into(),
            protocol_binding: "JSONRPC".into(),
            protocol_version: a2a_protocol_types::A2A_VERSION.into(),
            tenant: None,
        }],
        default_input_modes: vec!["text/plain".into()],
        default_output_modes: vec!["text/plain".into()],
        skills,
        capabilities: AgentCapabilities::none(),
        provider: None,
        icon_url: None,
        documentation_url: None,
        security_schemes: None,
        security_requirements: None,
        signatures: None,
    }
}

fn task_in(state: TaskState, artifact_text: Option<&str>, note: Option<&str>) -> Task {
    Task {
        id: TaskId::new("t-1"),
        context_id: ContextId::new("ctx-1"),
        status: TaskStatus {
            state,
            message: note.map(|text| Message {
                id: MessageId::new("m-note"),
                role: MessageRole::Agent,
                parts: vec![Part::text(text)],
                task_id: None,
                context_id: None,
                reference_task_ids: None,
                extensions: None,
                metadata: None,
            }),
            timestamp: None,
        },
        history: None,
        artifacts: artifact_text.map(|text| vec![Artifact::new("out", vec![Part::text(text)])]),
        metadata: None,
    }
}

fn text_of(blocks: &[ContentBlock]) -> String {
    blocks
        .iter()
        .filter_map(|b| match b {
            ContentBlock::Text(t) => Some(t.text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

// ── Mapping ──────────────────────────────────────────────────────────────────

#[test]
fn a_skill_id_becomes_a_legal_tool_name() {
    assert_eq!(mapping::tool_name("service_status"), "service_status");
    assert_eq!(mapping::tool_name("read-logs"), "read-logs");
    // Everything outside [A-Za-z0-9_-] becomes `_`: some MCP clients reject
    // such a name outright and others mangle it, so normalizing here is the
    // only way the published name is the one the caller sees.
    assert_eq!(mapping::tool_name("ops.read logs!"), "ops_read_logs_");
}

#[test]
fn two_skills_that_collide_are_refused_by_name() {
    // `a.b` and `a/b` both sanitize to `a_b`. Publishing one and dropping the
    // other, or publishing both, would send one skill's calls to the other.
    let card = card_with("http://x", vec![skill("a.b"), skill("a/b")]);
    let refused = mapping::tools_from_card(&card).expect_err("the names collide");
    assert_eq!(
        refused,
        MappingError::NameCollision {
            tool: "a_b".to_owned(),
            first: "a.b".to_owned(),
            second: "a/b".to_owned(),
        }
    );
    // Both ids are named, because "a collision" that does not say between
    // what is not actionable.
    let shown = refused.to_string();
    assert!(shown.contains("a.b") && shown.contains("a/b"), "{shown}");
}

#[test]
fn a_card_with_no_skills_is_refused() {
    let card = card_with("http://x", vec![]);
    assert_eq!(
        mapping::tools_from_card(&card).expect_err("nothing to publish"),
        MappingError::NoSkills
    );
}

#[test]
fn a_published_tool_carries_the_agents_own_description() {
    let card = card_with("http://x", vec![skill("report")]);
    let tools = mapping::tools_from_card(&card).expect("one skill maps");
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].name, "report");
    let description = tools[0].description.as_deref().unwrap_or_default();
    assert!(description.starts_with("does report"), "{description}");
    // Plus the one thing the skill cannot know about itself.
    assert!(description.contains("remote agent"), "{description}");
    assert_eq!(tools[0].input_schema.get("type"), Some(&json!("object")));
    assert_eq!(
        tools[0].input_schema.get("required"),
        Some(&json!([mapping::MESSAGE_ARG]))
    );
}

#[test]
fn a_completed_task_is_a_result_and_not_an_error() {
    let (blocks, is_error) =
        mapping::task_to_result(&task_in(TaskState::Completed, Some("the answer"), None));
    assert!(!is_error);
    assert_eq!(text_of(&blocks), "the answer");
}

#[test]
fn a_failed_task_is_an_error_carrying_the_agents_reason() {
    let (blocks, is_error) =
        mapping::task_to_result(&task_in(TaskState::Failed, None, Some("upstream refused")));
    assert!(is_error);
    assert!(text_of(&blocks).contains("upstream refused"));
}

#[test]
fn a_terminal_task_with_no_output_still_says_something() {
    // An empty error result tells the caller nothing at all; naming the state
    // is the minimum that lets them act.
    let (blocks, is_error) = mapping::task_to_result(&task_in(TaskState::Rejected, None, None));
    assert!(is_error);
    assert!(
        text_of(&blocks).contains("Rejected"),
        "{:?}",
        text_of(&blocks)
    );
}

#[test]
fn an_input_required_task_says_the_bridge_cannot_continue_it() {
    // The gap is reported, not approximated. A caller that reads this knows
    // the agent is waiting rather than broken, which is the distinction a
    // bare error would lose.
    let (blocks, is_error) = mapping::task_to_result(&task_in(
        TaskState::InputRequired,
        None,
        Some("which environment?"),
    ));
    assert!(is_error);
    let text = text_of(&blocks);
    assert!(text.contains("which environment?"), "{text}");
    assert!(text.contains("waiting for a follow-up"), "{text}");
}

#[test]
fn non_text_parts_are_counted_rather_than_silently_dropped() {
    let mut task = task_in(TaskState::Completed, Some("text part"), None);
    if let Some(artifacts) = task.artifacts.as_mut() {
        artifacts[0].parts.push(Part::raw("aGk="));
    }
    assert_eq!(mapping::dropped_part_count(&task), 1);
    let (blocks, _) = mapping::task_to_result(&task);
    assert_eq!(text_of(&blocks), "text part");
}

// ── A real A2A agent, bridged ────────────────────────────────────────────────

/// Completes unless the bridge's skill hint names the failing skill, and
/// echoes the hint back so a test can prove it crossed.
struct TestAgent;

impl AgentExecutor for TestAgent {
    fn execute<'a>(
        &'a self,
        ctx: &'a RequestContext,
        queue: &'a dyn EventQueueWriter,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async move {
            let hint = ctx
                .metadata
                .as_ref()
                .and_then(|m| m.get(SKILL_METADATA_KEY))
                .and_then(serde_json::Value::as_str)
                .unwrap_or("none")
                .to_owned();

            if hint == "boom" {
                return Err(A2aError::internal("the agent refused"));
            }

            queue
                .write(StreamResponse::ArtifactUpdate(TaskArtifactUpdateEvent {
                    task_id: ctx.task_id.clone(),
                    context_id: ContextId::new(ctx.context_id.clone()),
                    artifact: Artifact::new("out", vec![Part::text(format!("hint={hint}"))]),
                    append: None,
                    last_chunk: Some(true),
                    metadata: None,
                }))
                .await?;
            queue
                .write(StreamResponse::StatusUpdate(TaskStatusUpdateEvent {
                    task_id: ctx.task_id.clone(),
                    context_id: ContextId::new(ctx.context_id.clone()),
                    status: TaskStatus::new(TaskState::Completed),
                    metadata: None,
                }))
                .await?;
            Ok(())
        })
    }
}

/// Starts a real A2A agent on localhost with the given skills.
async fn start_agent(skills: Vec<AgentSkill>) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("an ephemeral port");
    let url = format!("http://{}", listener.local_addr().expect("a bound address"));
    let handler = Arc::new(
        RequestHandlerBuilder::new(TestAgent)
            .with_agent_card(card_with(&url, skills))
            .build()
            .expect("the handler config is static"),
    );
    let dispatcher = Arc::new(JsonRpcDispatcher::new(handler));
    tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                continue;
            };
            let io = hyper_util::rt::TokioIo::new(stream);
            let dispatcher = Arc::clone(&dispatcher);
            tokio::spawn(async move {
                let service = hyper::service::service_fn(move |req| {
                    let d = Arc::clone(&dispatcher);
                    async move { Ok::<_, std::convert::Infallible>(d.dispatch(req).await) }
                });
                let _ = hyper_util::server::conn::auto::Builder::new(
                    hyper_util::rt::TokioExecutor::new(),
                )
                .serve_connection(io, service)
                .await;
            });
        }
    });
    url
}

/// Serves a bridge over an in-process pipe and returns a connected MCP client.
async fn mcp_client_for(bridge: A2aBridge) -> rmcp::service::RunningService<rmcp::RoleClient, ()> {
    let (server_side, client_side) = tokio::io::duplex(8192);
    tokio::spawn(async move {
        if let Ok(service) = bridge.serve(server_side).await {
            let _ = service.waiting().await;
        }
    });
    ().serve(client_side).await.expect("the MCP session opens")
}

#[tokio::test]
async fn the_bridge_publishes_one_tool_per_advertised_skill() {
    let url = start_agent(vec![skill("alpha"), skill("beta")]).await;
    let bridge = A2aBridge::connect(&url).await.expect("discovery succeeds");
    let client = mcp_client_for(bridge).await;

    let mut names: Vec<String> = client
        .list_all_tools()
        .await
        .expect("tools/list answers")
        .into_iter()
        .map(|t| t.name.to_string())
        .collect();
    names.sort();
    assert_eq!(names, ["alpha", "beta"]);
}

#[tokio::test]
async fn a_bridged_call_returns_the_agents_artifact_and_carries_the_skill_hint() {
    let url = start_agent(vec![skill("alpha")]).await;
    let bridge = A2aBridge::connect(&url).await.expect("discovery succeeds");
    let client = mcp_client_for(bridge).await;

    let result = client
        .call_tool(
            CallToolRequestParams::new("alpha").with_arguments(
                json!({ "message": "hello" })
                    .as_object()
                    .expect("an object")
                    .clone(),
            ),
        )
        .await
        .expect("the agent completes");

    assert_ne!(result.is_error, Some(true));
    // A2A has no skill selector, so the hint travelling in message metadata
    // is the only way the chosen tool reaches the agent at all. The agent
    // echoes it back, which is how this test knows it arrived.
    assert_eq!(text_of(&result.content), "hint=alpha");
}

#[tokio::test]
async fn an_agent_failure_becomes_an_mcp_error_result_not_a_dead_session() {
    let url = start_agent(vec![skill("boom")]).await;
    let bridge = A2aBridge::connect(&url).await.expect("discovery succeeds");
    let client = mcp_client_for(bridge).await;

    let result = client
        .call_tool(
            CallToolRequestParams::new("boom").with_arguments(
                json!({ "message": "go" })
                    .as_object()
                    .expect("an object")
                    .clone(),
            ),
        )
        .await
        .expect("a failed A2A task is still an answered MCP call");

    assert_eq!(result.is_error, Some(true));
    assert!(
        text_of(&result.content).contains("the agent refused"),
        "{:?}",
        text_of(&result.content)
    );

    // The session survives a failed call, so the next one works.
    let tools = client.list_all_tools().await.expect("the session is alive");
    assert_eq!(tools.len(), 1);
}

#[tokio::test]
async fn an_unknown_tool_and_a_missing_argument_are_both_refused() {
    let url = start_agent(vec![skill("alpha")]).await;
    let bridge = A2aBridge::connect(&url).await.expect("discovery succeeds");
    let client = mcp_client_for(bridge).await;

    let unknown = client
        .call_tool(CallToolRequestParams::new("nope"))
        .await
        .expect_err("no such tool");
    assert!(unknown.to_string().contains("nope"), "{unknown}");

    let no_arg = client
        .call_tool(CallToolRequestParams::new("alpha"))
        .await
        .expect_err("the message argument is required");
    assert!(
        no_arg.to_string().contains(mapping::MESSAGE_ARG),
        "{no_arg}"
    );
}

#[tokio::test]
async fn a_card_the_bridge_cannot_map_refuses_to_start() {
    // An MCP server that came up with an empty tool list would look, to its
    // caller, exactly like an agent with nothing to offer.
    let url = start_agent(vec![]).await;
    match A2aBridge::connect(&url).await {
        Err(BridgeError::Mapping(MappingError::NoSkills)) => {}
        Err(other) => panic!("wrong refusal: {other}"),
        Ok(_) => panic!("a card with no skills is not bridgeable"),
    }
}

#[tokio::test]
async fn an_unreachable_agent_refuses_to_start() {
    match A2aBridge::connect("http://127.0.0.1:1").await {
        Err(BridgeError::Discovery(_)) => {}
        Err(other) => panic!("wrong refusal: {other}"),
        Ok(_) => panic!("nothing is listening on port 1"),
    }
}
