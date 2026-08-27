// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The echo executor and the agent card it is served behind.

use a2a_example_harness::{interfaces, Endpoints};
use a2a_protocol_types::agent_card::{AgentCapabilities, AgentCard, AgentSkill};
use a2a_protocol_types::message::Part;
use a2a_protocol_types::task::TaskState;

use a2a_protocol_server::agent_executor;
use a2a_protocol_server::executor_helpers::EventEmitter;

/// Message-text prefix that makes the executor pause mid-task.
///
/// Used by the demo to create an observably-running task for
/// `SubscribeToTask`. Kept as a constant so the executor and the caller cannot
/// disagree about the spelling.
pub const SLOW_PREFIX: &str = "slow:";

/// A simple agent that echoes the first text part of the incoming message
/// back as an artifact, going through Working → Completed status transitions.
pub struct EchoExecutor;

agent_executor!(EchoExecutor, |ctx, queue| async {
    let emit = EventEmitter::new(ctx, queue);
    emit.status(TaskState::Working).await?;

    let input_text = ctx.message.text().unwrap_or("<no text>");

    // A deliberately slow turn, so a caller can observe a task that is
    // still running. Without one, every echo task is terminal by the
    // time its id is known, and `SubscribeToTask` can only ever be
    // observed being *refused* (spec: re-attaching to a terminal task
    // is `UnsupportedOperation`). The demo needs the success path too,
    // and a success path nothing can reach is not covered.
    if input_text.starts_with(SLOW_PREFIX) {
        tokio::time::sleep(std::time::Duration::from_millis(400)).await;
    }

    let echo_text = format!("Echo: {input_text}");
    emit.artifact(
        "echo-artifact",
        vec![Part::text(&echo_text)],
        None,
        Some(true),
    )
    .await?;

    emit.status(TaskState::Completed).await?;
    Ok(())
});

/// The card this example serves.
///
/// Advertises all four bindings and all three optional capabilities. That is
/// not decoration: the server refuses `GetExtendedAgentCard` unless
/// `extendedAgentCard` is set, and refuses the four push-config methods unless
/// `pushNotifications` is set (spec §3.1.11). Until 2026-08-11 this card
/// declared `pushNotifications(false)` and no extended card, so seven of the
/// eleven methods were not merely undriven by the demo — they were unavailable
/// on the server it started.
#[must_use]
pub fn make_agent_card(ep: &Endpoints) -> AgentCard {
    AgentCard {
        url: None,
        name: "Echo Agent".into(),
        description: "A simple echo agent that mirrors your input".into(),
        version: "1.0.0".into(),
        supported_interfaces: interfaces(ep),
        default_input_modes: vec!["text/plain".into()],
        default_output_modes: vec!["text/plain".into()],
        skills: vec![AgentSkill {
            id: "echo".into(),
            name: "Echo".into(),
            description: "Echoes your message back as an artifact".into(),
            tags: vec!["echo".into(), "demo".into()],
            examples: None,
            input_modes: None,
            output_modes: None,
            security_requirements: None,
        }],
        capabilities: AgentCapabilities::none()
            .with_streaming(true)
            .with_push_notifications(true)
            .with_extended_agent_card(true),
        provider: None,
        icon_url: None,
        documentation_url: None,
        security_schemes: None,
        security_requirements: None,
        signatures: None,
    }
}

#[cfg(test)]
mod tests {
    use super::{make_agent_card, EchoExecutor, SLOW_PREFIX};
    use a2a_example_harness::{Binding, Endpoints};
    use a2a_protocol_server::executor::AgentExecutor;
    use a2a_protocol_server::request_context::RequestContext;
    use a2a_protocol_server::streaming::event_queue::new_in_memory_queue;
    use a2a_protocol_server::streaming::EventQueueReader;
    use a2a_protocol_types::error::A2aResult;
    use a2a_protocol_types::events::StreamResponse;
    use a2a_protocol_types::message::{Message, MessageId, MessageRole, Part, PartContent};
    use a2a_protocol_types::task::{TaskId, TaskState};

    fn ctx_with(parts: Vec<Part>) -> RequestContext {
        RequestContext::new(
            Message {
                id: MessageId::new("m-1"),
                role: MessageRole::User,
                parts,
                task_id: None,
                context_id: None,
                reference_task_ids: None,
                extensions: None,
                metadata: None,
            },
            TaskId::new("t-1"),
            "ctx-1".to_owned(),
        )
    }

    fn ctx(text: &str) -> RequestContext {
        ctx_with(vec![Part::text(text)])
    }

    async fn drive(ctx: &RequestContext) -> (A2aResult<()>, Vec<StreamResponse>) {
        let (writer, mut reader) = new_in_memory_queue();
        let result = EchoExecutor.execute(ctx, &writer).await;
        drop(writer);
        let mut events = Vec::new();
        while let Some(item) = reader.read().await {
            events.push(item.expect("the queue delivered an error rather than an event"));
        }
        (result, events)
    }

    fn states(events: &[StreamResponse]) -> Vec<TaskState> {
        events
            .iter()
            .filter_map(|e| match e {
                StreamResponse::StatusUpdate(s) => Some(s.status.state),
                _ => None,
            })
            .collect()
    }

    fn artifacts(events: &[StreamResponse]) -> Vec<(String, String)> {
        events
            .iter()
            .filter_map(|e| match e {
                StreamResponse::ArtifactUpdate(a) => Some((
                    a.artifact.id.0.clone(),
                    a.artifact
                        .parts
                        .iter()
                        .filter_map(|p| match &p.content {
                            PartContent::Text(t) => Some(t.as_str()),
                            _ => None,
                        })
                        .collect::<Vec<_>>()
                        .join(" "),
                )),
                _ => None,
            })
            .collect()
    }

    /// Working before the artifact, Completed after it. A client that sees the
    /// artifact without a preceding Working has no non-terminal state to
    /// subscribe to.
    #[tokio::test]
    async fn an_echo_narrates_then_delivers_then_completes() {
        let (result, events) = drive(&ctx("hello")).await;
        assert!(result.is_ok(), "{result:?}");
        assert_eq!(
            states(&events),
            vec![TaskState::Working, TaskState::Completed]
        );
        let arts = artifacts(&events);
        assert_eq!(arts.len(), 1);
        assert_eq!(arts[0].0, "echo-artifact");
        assert_eq!(arts[0].1, "Echo: hello");
    }

    /// A textless message echoes a placeholder rather than failing.
    ///
    /// This is a deliberate difference from `genai-agent` and `rig-agent`,
    /// which refuse one with `InvalidParams`: those spend a provider call on
    /// an empty prompt, and this spends nothing. Pinned so the divergence is a
    /// decision rather than a drift.
    #[tokio::test]
    async fn a_textless_message_echoes_a_placeholder_rather_than_failing() {
        let (result, events) = drive(&ctx_with(vec![Part::raw("aGk=")])).await;
        assert!(result.is_ok(), "echo does not refuse a textless message");
        assert_eq!(artifacts(&events)[0].1, "Echo: <no text>");
    }

    /// The marker is echoed, not stripped. The demo matches on the text it
    /// gets back, so silently removing the prefix would make a slow turn
    /// indistinguishable from a fast one in the transcript.
    #[tokio::test]
    async fn the_slow_marker_is_echoed_not_consumed() {
        let (_, events) = drive(&ctx(&format!("{SLOW_PREFIX}wait"))).await;
        assert_eq!(artifacts(&events)[0].1, format!("Echo: {SLOW_PREFIX}wait"));
    }

    /// The marker holds the task open so `SubscribeToTask` has a non-terminal
    /// task to attach to — its success path is unreachable otherwise, because
    /// re-attaching to a terminal task is `UnsupportedOperation`. Asserted on
    /// tokio's paused clock, so it costs no wall-clock time.
    #[tokio::test(start_paused = true)]
    async fn the_slow_marker_holds_the_task_open() {
        let start = tokio::time::Instant::now();
        drive(&ctx("plain")).await.0.expect("plain turn");
        let plain = start.elapsed();

        let start = tokio::time::Instant::now();
        drive(&ctx(&format!("{SLOW_PREFIX}wait")))
            .await
            .0
            .expect("slow turn");
        let slow = start.elapsed();

        assert!(
            slow >= std::time::Duration::from_millis(400),
            "the slow marker did not delay the turn: {slow:?}"
        );
        assert!(plain < slow, "a plain turn took as long as a slow one");
    }

    /// Every binding binds loopback and reports the port it actually got.
    /// A demo that bound `0.0.0.0` would expose an unauthenticated agent on
    /// every interface of whatever machine ran it.
    #[tokio::test]
    async fn listeners_bind_loopback_on_an_ephemeral_port() {
        let (listener, addr) = crate::serve::bind_listener().await;
        assert!(
            addr.ip().is_loopback(),
            "bound a non-loopback address: {addr}"
        );
        assert_ne!(
            addr.port(),
            0,
            "reported the wildcard port, not the real one"
        );
        assert_eq!(listener.local_addr().expect("local_addr"), addr);
    }

    fn card() -> a2a_protocol_types::agent_card::AgentCard {
        make_agent_card(&Endpoints {
            jsonrpc: "http://127.0.0.1:1".into(),
            rest: "http://127.0.0.1:2".into(),
            grpc: "127.0.0.1:3".into(),
            websocket: "ws://127.0.0.1:4".into(),
        })
    }

    /// A binding the coverage matrix has a column for, but the card does not
    /// advertise, is undiscoverable: §5 makes the card the only way a client
    /// learns where to connect. The matrix would then report cells that no
    /// real client could reach.
    #[test]
    fn the_card_advertises_every_binding_the_matrix_scores() {
        let card = card();
        for b in Binding::ALL {
            assert!(
                card.supported_interfaces
                    .iter()
                    .any(|i| i.protocol_binding == b.label()),
                "coverage scores {} but the card does not advertise it",
                b.label()
            );
        }
    }

    /// Capability flags gate whole method families server-side. If any of the
    /// three is off, the matrix cannot be completed no matter what the demo
    /// does — so assert them here rather than discovering it as a wall of
    /// MISSING cells.
    #[test]
    fn every_optional_capability_is_advertised() {
        let c = card().capabilities;
        assert_eq!(c.streaming, Some(true), "streaming must be advertised");
        assert_eq!(
            c.push_notifications,
            Some(true),
            "pushNotifications must be advertised or the four push-config \
             methods answer UnsupportedOperation"
        );
        assert_eq!(
            c.extended_agent_card,
            Some(true),
            "extendedAgentCard must be advertised or GetExtendedAgentCard \
             answers UnsupportedOperation"
        );
    }
}
