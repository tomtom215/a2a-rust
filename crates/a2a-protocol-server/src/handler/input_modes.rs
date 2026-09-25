// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Refusing message parts whose media type the agent card does not declare.
//!
//! Spec §3.1.1 lists `ContentTypeNotSupportedError` among `SendMessage`'s
//! errors: "A Media Type provided in the request's message parts is not
//! supported by the agent." What the agent supports is what its card
//! declares — `defaultInputModes`, and each skill's `inputModes`. The ACTS
//! conformance suite checks it as a MUST (CORE-SEND-004); until 2026-09-25
//! this server accepted any media type and left the executor to cope.
//!
//! The rules, all chosen to refuse only what the card plainly excludes:
//!
//! - Only a part that carries an explicit `mediaType` is checked. A text
//!   part without one is not assumed to be `text/plain`, and a data part
//!   without one is not assumed to be `application/json`.
//! - The accepted set is `defaultInputModes` together with every skill's
//!   `inputModes`: a message does not name the skill it is for, so a mode
//!   any skill declares is a mode the agent accepts.
//! - Nothing is enforced without a card, or when the card declares no modes
//!   at all: an empty declaration says nothing, rather than "nothing".
//! - Media types compare case-insensitively on their essence, ignoring
//!   parameters (`text/plain; charset=utf-8` is `text/plain`); a declared
//!   `*/*` or `type/*` matches as a wildcard.
//!
//! [`RequestHandlerBuilder::allow_undeclared_input_modes`](crate::builder::RequestHandlerBuilder::allow_undeclared_input_modes)
//! turns the check off, for an agent whose card under-declares what it
//! accepts.

use a2a_protocol_types::agent_card::AgentCard;
use a2a_protocol_types::error::A2aError;
use a2a_protocol_types::message::Message;

use super::RequestHandler;
use crate::error::{ServerError, ServerResult};

/// The media types a card accepts, or `None` when nothing is to be enforced.
pub(crate) fn accepted_input_modes(
    card: Option<&AgentCard>,
    allow_undeclared: bool,
) -> Option<Vec<String>> {
    if allow_undeclared {
        return None;
    }
    let card = card?;
    let mut modes: Vec<String> = card
        .default_input_modes
        .iter()
        .chain(
            card.skills
                .iter()
                .flat_map(|s| s.input_modes.iter().flatten()),
        )
        .map(|m| essence(m))
        .filter(|m| !m.is_empty())
        .collect();
    modes.sort();
    modes.dedup();
    (!modes.is_empty()).then_some(modes)
}

/// A media type's essence: lower-cased, parameters and whitespace removed.
fn essence(media_type: &str) -> String {
    media_type
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase()
}

/// Whether a declared mode (possibly a wildcard) admits `actual`, both
/// already reduced to their essence.
fn admits(declared: &str, actual: &str) -> bool {
    if declared == "*/*" || declared == actual {
        return true;
    }
    declared
        .strip_suffix("/*")
        .is_some_and(|ty| actual.split('/').next() == Some(ty))
}

impl RequestHandler {
    /// Refuses a message with a part whose explicit media type the card does
    /// not declare.
    ///
    /// # Errors
    ///
    /// `ContentTypeNotSupportedError`, naming the part and its media type.
    pub(crate) fn ensure_input_modes_supported(&self, message: &Message) -> ServerResult<()> {
        let Some(accepted) = &self.accepted_input_modes else {
            return Ok(());
        };
        for (i, part) in message.parts.iter().enumerate() {
            let Some(media_type) = part.media_type.as_deref() else {
                continue;
            };
            let actual = essence(media_type);
            if !accepted.iter().any(|declared| admits(declared, &actual)) {
                return Err(ServerError::Protocol(A2aError::content_type_not_supported(
                    format!(
                        "message part {i} has media type {media_type:?}, which the agent \
                         card does not declare (defaultInputModes or a skill's inputModes)"
                    ),
                )));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use a2a_protocol_types::agent_card::{
        AgentCapabilities, AgentCard, AgentInterface, AgentSkill,
    };
    use a2a_protocol_types::error::ErrorCode;
    use a2a_protocol_types::message::{Message, Part};

    use crate::agent_executor;
    use crate::builder::RequestHandlerBuilder;

    struct DummyExecutor;
    agent_executor!(DummyExecutor, |_ctx, _queue| async { Ok(()) });

    fn card(default: &[&str], skill: Option<&[&str]>) -> AgentCard {
        let mut s = AgentSkill::new("s", "S", "a skill");
        if let Some(modes) = skill {
            s = s.with_input_modes(modes.iter().copied());
        }
        AgentCard {
            url: None,
            name: "Test Agent".into(),
            description: "A test agent".into(),
            version: "1.0.0".into(),
            supported_interfaces: vec![AgentInterface {
                url: "http://localhost:8080".into(),
                protocol_binding: "JSONRPC".into(),
                protocol_version: "1.0.0".into(),
                tenant: None,
            }],
            default_input_modes: default.iter().map(|m| (*m).to_owned()).collect(),
            default_output_modes: vec!["text/plain".into()],
            skills: vec![s],
            capabilities: AgentCapabilities::none(),
            provider: None,
            icon_url: None,
            documentation_url: None,
            security_schemes: None,
            security_requirements: None,
            signatures: None,
        }
    }

    fn with_part(media_type: Option<&str>) -> Message {
        let mut part = Part::text("hi");
        part.media_type = media_type.map(str::to_owned);
        Message::user("m-1", vec![part])
    }

    /// `Ok`, or the error's code and message.
    fn check(
        card: Option<AgentCard>,
        opt_out: bool,
        media_type: Option<&str>,
    ) -> Result<(), (ErrorCode, String)> {
        let mut builder = RequestHandlerBuilder::new(DummyExecutor);
        if let Some(card) = card {
            builder = builder.with_agent_card(card);
        }
        if opt_out {
            builder = builder.allow_undeclared_input_modes();
        }
        let handler = builder.build().expect("handler");
        handler
            .ensure_input_modes_supported(&with_part(media_type))
            .map_err(|e| {
                let e = e.to_a2a_error();
                (e.code, e.message)
            })
    }

    #[test]
    fn a_declared_media_type_is_accepted() {
        let card = card(&["application/json"], None);
        assert!(check(Some(card), false, Some("application/json")).is_ok());
    }

    #[test]
    fn an_undeclared_media_type_is_refused_as_content_type_not_supported() {
        let card = card(&["text/plain"], None);
        let (code, message) =
            check(Some(card), false, Some("application/x-unsupported")).unwrap_err();
        assert_eq!(code, ErrorCode::ContentTypeNotSupported);
        assert!(message.contains("application/x-unsupported"), "{message}");
    }

    #[test]
    fn a_mode_only_a_skill_declares_is_accepted() {
        let card = card(&["text/plain"], Some(&["image/png"]));
        assert!(check(Some(card), false, Some("image/png")).is_ok());
    }

    #[test]
    fn parameters_and_case_are_ignored() {
        let card = card(&["Text/Plain"], None);
        assert!(check(Some(card), false, Some("text/plain; charset=utf-8")).is_ok());
    }

    #[test]
    fn wildcards_admit_their_range_and_nothing_else() {
        assert!(check(Some(card(&["image/*"], None)), false, Some("image/png")).is_ok());
        assert!(check(Some(card(&["image/*"], None)), false, Some("imagex/png")).is_err());
        assert!(check(Some(card(&["*/*"], None)), false, Some("application/x-any")).is_ok());
    }

    #[test]
    fn a_part_without_a_media_type_is_not_checked() {
        assert!(check(Some(card(&["application/json"], None)), false, None).is_ok());
    }

    #[test]
    fn nothing_is_enforced_without_a_card_or_a_declaration_or_when_opted_out() {
        assert!(check(None, false, Some("application/x-unsupported")).is_ok());
        assert!(
            check(
                Some(card(&[], None)),
                false,
                Some("application/x-unsupported")
            )
            .is_ok()
        );
        assert!(
            check(
                Some(card(&["text/plain"], None)),
                true,
                Some("application/x-unsupported")
            )
            .is_ok()
        );
    }
}
