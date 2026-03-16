// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! Coordinator executor — orchestrates the team via A2A client calls.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;

use a2a_protocol_types::artifact::Artifact;
use a2a_protocol_types::error::A2aResult;
use a2a_protocol_types::events::{StreamResponse, TaskArtifactUpdateEvent, TaskStatusUpdateEvent};
use a2a_protocol_types::message::{Message, MessageId, MessageRole, Part, PartContent};
use a2a_protocol_types::params::MessageSendParams;
use a2a_protocol_types::responses::SendMessageResponse;
use a2a_protocol_types::task::{ContextId, TaskState, TaskStatus};

use a2a_protocol_client::ClientBuilder;
use a2a_protocol_server::executor::AgentExecutor;
use a2a_protocol_server::request_context::RequestContext;
use a2a_protocol_server::streaming::EventQueueWriter;

use crate::helpers::make_send_params;

/// Orchestrates the team: delegates tasks to other agents via A2A client calls,
/// aggregates results, and reports a unified summary.
pub struct CoordinatorExecutor {
    /// URLs of the other agents, keyed by name.
    pub agent_urls: HashMap<String, String>,
}

impl CoordinatorExecutor {
    pub fn new(agent_urls: HashMap<String, String>) -> Self {
        Self { agent_urls }
    }
}

impl AgentExecutor for CoordinatorExecutor {
    fn execute<'a>(
        &'a self,
        ctx: &'a RequestContext,
        queue: &'a dyn EventQueueWriter,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async move {
            // Working
            queue
                .write(StreamResponse::StatusUpdate(TaskStatusUpdateEvent {
                    task_id: ctx.task_id.clone(),
                    context_id: ContextId::new(ctx.context_id.clone()),
                    status: TaskStatus::new(TaskState::Working),
                    metadata: None,
                }))
                .await?;

            let command = ctx
                .message
                .parts
                .iter()
                .find_map(|p| match &p.content {
                    PartContent::Text { text } => Some(text.clone()),
                    _ => None,
                })
                .unwrap_or_else(|| "analyze".to_owned());

            let mut report_lines = vec![format!("Coordinator executing: {command}")];

            match command.as_str() {
                "analyze" | "full-check" => {
                    self.delegate_analysis(&mut report_lines).await;
                    if command == "full-check" {
                        self.delegate_build(&mut report_lines).await;
                    }
                }
                "health" => {
                    self.delegate_health(&mut report_lines).await;
                }
                _ => {
                    report_lines.push(format!("Unknown command: {command}"));
                }
            }

            // Emit unified report.
            let full_report = report_lines.join("\n");
            queue
                .write(StreamResponse::ArtifactUpdate(TaskArtifactUpdateEvent {
                    task_id: ctx.task_id.clone(),
                    context_id: ContextId::new(ctx.context_id.clone()),
                    artifact: Artifact::new("coordinator-report", vec![Part::text(&full_report)]),
                    append: None,
                    last_chunk: Some(true),
                    metadata: None,
                }))
                .await?;

            // Completed
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

impl CoordinatorExecutor {
    async fn delegate_analysis(&self, report_lines: &mut Vec<String>) {
        let Some(analyzer_url) = self.agent_urls.get("code_analyzer") else {
            return;
        };
        report_lines.push(String::new());
        report_lines.push("--- Code Analysis ---".to_owned());

        let Ok(client) = ClientBuilder::new(analyzer_url).build() else {
            report_lines.push("  Cannot connect to analyzer".to_string());
            return;
        };

        let sample_code = "fn main() {\n    println!(\"Hello from coordinator!\");\n    let x = 42;\n    let y = x * 2;\n    println!(\"Result: {y}\");\n}";
        match client.send_message(make_send_params(sample_code)).await {
            Ok(SendMessageResponse::Task(task)) => {
                report_lines.push(format!("  Task {}: {:?}", task.id, task.status.state));
                if let Some(artifacts) = &task.artifacts {
                    for art in artifacts {
                        for part in &art.parts {
                            if let PartContent::Text { text } = &part.content {
                                report_lines.push(format!("  {}", text));
                            }
                            if let PartContent::Data { data } = &part.content {
                                report_lines.push(format!("  JSON: {}", data));
                            }
                        }
                    }
                }
            }
            Ok(_) => report_lines.push("  Got non-task response".to_owned()),
            Err(e) => report_lines.push(format!("  Analysis failed: {e}")),
        }
    }

    async fn delegate_build(&self, report_lines: &mut Vec<String>) {
        let Some(build_url) = self.agent_urls.get("build_monitor") else {
            return;
        };
        report_lines.push(String::new());
        report_lines.push("--- Build Check ---".to_owned());

        let Ok(client) = ClientBuilder::new(build_url)
            .with_protocol_binding("REST")
            .build()
        else {
            return;
        };

        match client.send_message(make_send_params("check")).await {
            Ok(SendMessageResponse::Task(task)) => {
                report_lines.push(format!("  Build {}: {:?}", task.id, task.status.state));
            }
            Ok(_) => {}
            Err(e) => report_lines.push(format!("  Build check failed: {e}")),
        }
    }

    async fn delegate_health(&self, report_lines: &mut Vec<String>) {
        let Some(health_url) = self.agent_urls.get("health_monitor") else {
            return;
        };
        report_lines.push(String::new());
        report_lines.push("--- Health Check ---".to_owned());

        let Ok(client) = ClientBuilder::new(health_url).build() else {
            return;
        };

        let urls: Vec<String> = self.agent_urls.values().cloned().collect();
        let parts = vec![
            Part::text("coordinator-initiated health check"),
            Part::data(serde_json::json!(urls)),
        ];
        let params = MessageSendParams {
            tenant: None,
            message: Message {
                id: MessageId::new(uuid::Uuid::new_v4().to_string()),
                role: MessageRole::User,
                parts,
                task_id: None,
                context_id: None,
                reference_task_ids: None,
                extensions: None,
                metadata: None,
            },
            configuration: None,
            metadata: None,
        };
        match client.send_message(params).await {
            Ok(SendMessageResponse::Task(task)) => {
                if let Some(artifacts) = &task.artifacts {
                    for art in artifacts {
                        for part in &art.parts {
                            if let PartContent::Text { text } = &part.content {
                                report_lines.push(format!("  {text}"));
                            }
                        }
                    }
                }
            }
            Ok(_) => {}
            Err(e) => report_lines.push(format!("  Health check failed: {e}")),
        }
    }
}
