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
    use super::make_agent_card;
    use a2a_example_harness::{Binding, Endpoints};

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
