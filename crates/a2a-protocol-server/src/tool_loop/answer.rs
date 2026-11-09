// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! [`Answer`]: what a run of the loop produced, and when it counts as one.

use super::loop_error::LoopError;

/// The `finish_reason` of a reply that stopped at the model's output limit.
///
/// [`OpenAiChatModel`](super::OpenAiChatModel) passes the provider's value
/// through, and this is the OpenAI-compatible spelling; a
/// [`ToolModel`](super::ToolModel) of your own reports a cut-off reply as
/// this too, through
/// [`ModelTurn::with_finish_reason`](super::ModelTurn::with_finish_reason).
pub const FINISH_REASON_LENGTH: &str = "length";

/// The `metadata` key that marks an answer cut off at the output limit.
///
/// It is on the answer artifact, and on the status message of the task that
/// completed with it; its value is the model's `finish_reason`. Present only
/// when the reply was cut off ([`FINISH_REASON_LENGTH`]): the text is then a
/// prefix of an answer, and may end mid-sentence.
pub const FINISH_REASON_METADATA_KEY: &str = "a2a-rust.com/finish-reason";

/// What a run produced.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Answer {
    /// The model's final text. Never empty or blank: a run whose final reply
    /// has no text ends in [`LoopError::EmptyAnswer`] instead.
    pub text: String,
    /// One line per tool call, in the order they ran: `name(arguments) ->
    /// result`. Empty when the model answered without calling anything.
    pub trace: Vec<String>,
    /// Why the final reply stopped, as the provider spelled it (`stop`,
    /// `length`, …), when it said.
    pub finish_reason: Option<String>,
}

impl Answer {
    /// The answer a run ends in when its final reply was `text`, or
    /// [`LoopError::EmptyAnswer`] when that reply has no text to give: an
    /// empty reply is no answer, however the model came to send it (a
    /// reasoning model whose output limit went on thinking sends one).
    pub(super) fn from_reply(
        text: String,
        trace: Vec<String>,
        finish_reason: Option<String>,
    ) -> Result<Self, LoopError> {
        if text.trim().is_empty() {
            return Err(LoopError::EmptyAnswer { finish_reason });
        }
        Ok(Self {
            text,
            trace,
            finish_reason,
        })
    }

    /// Whether the reply stopped at the model's output limit, so that
    /// [`text`](Self::text) may end mid-sentence.
    #[must_use]
    pub fn truncated(&self) -> bool {
        self.finish_reason.as_deref() == Some(FINISH_REASON_LENGTH)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reply(text: &str, reason: Option<&str>) -> Result<Answer, LoopError> {
        Answer::from_reply(
            text.to_owned(),
            vec!["t() -> ok".to_owned()],
            reason.map(str::to_owned),
        )
    }

    /// The text, trace and reason arrive as given.
    #[test]
    fn an_answer_carries_what_the_reply_gave() {
        assert_eq!(
            reply("done", Some("stop")),
            Ok(Answer {
                text: "done".to_owned(),
                trace: vec!["t() -> ok".to_owned()],
                finish_reason: Some("stop".to_owned()),
            })
        );
    }

    /// Nothing, or only whitespace, is not an answer; one character is. The
    /// reason the model gave goes with the error, and the text is not kept.
    #[test]
    fn a_reply_with_no_text_is_not_an_answer() {
        for blank in ["", " ", "\n", " \t\r\n "] {
            assert_eq!(
                reply(blank, Some("length")),
                Err(LoopError::EmptyAnswer {
                    finish_reason: Some("length".to_owned())
                }),
                "{blank:?}"
            );
        }
        assert_eq!(
            reply("", None),
            Err(LoopError::EmptyAnswer {
                finish_reason: None
            })
        );
        assert_eq!(reply("a", None).unwrap().text, "a");
        // Surrounding whitespace is the model's, and kept.
        assert_eq!(reply(" a\n", None).unwrap().text, " a\n");
    }

    /// Only `length` is a cut-off; `stop`, a tool-call reason, no reason, and
    /// a near miss are not.
    #[test]
    fn only_the_output_limit_makes_an_answer_truncated() {
        let truncated = |reason: Option<&str>| reply("x", reason).unwrap().truncated();
        assert!(truncated(Some("length")));
        for other in [
            Some("stop"),
            Some("tool_calls"),
            Some("Length"),
            Some(""),
            None,
        ] {
            assert!(!truncated(other), "{other:?}");
        }
    }
}
