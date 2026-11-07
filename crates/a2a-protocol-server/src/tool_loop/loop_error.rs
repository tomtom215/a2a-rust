// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! [`LoopError`]: why a run of the loop produced no answer.

use std::fmt;
#[cfg(test)]
use std::time::Duration;

use a2a_protocol_types::budget::Exhausted;
use a2a_protocol_types::failure::FailureClass;

use super::answer::FINISH_REASON_LENGTH;
use super::model::ModelError;

/// Why a run produced no answer.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum LoopError {
    /// The model provider failed a turn.
    Model(ModelError),
    /// A tool returned [`ToolError::Fatal`](super::ToolError::Fatal).
    Fatal {
        /// The tool that failed.
        tool: String,
        /// What it said.
        message: String,
    },
    /// The model asked for tools on every one of this many turns.
    TurnLimit(usize),
    /// The cancellation token fired.
    Cancelled,
    /// The caller's budget ran out: its deadline passed, or the model's
    /// reported usage spent the [`TOKENS_METER`](a2a_protocol_types::budget::TOKENS_METER) (ADR 0023).
    Budget(Exhausted),
    /// The model's final reply had no text: it was empty, or only
    /// whitespace. The usual cause is a reply cut at the output limit before
    /// the model had written anything, as a reasoning model whose limit goes
    /// on thinking does.
    EmptyAnswer {
        /// Why the reply stopped, as the provider spelled it, when it said.
        finish_reason: Option<String>,
    },
}

/// The `finish_reason` OpenAI-compatible providers send when a content filter
/// withheld the reply.
const FINISH_REASON_CONTENT_FILTER: &str = "content_filter";

impl fmt::Display for LoopError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Model(e) => e.fmt(f),
            Self::Fatal { tool, message } => write!(f, "tool '{tool}' failed: {message}"),
            Self::TurnLimit(turns) => write!(
                f,
                "the model asked for tools on all {turns} turns without answering"
            ),
            Self::Cancelled => f.write_str("cancelled"),
            Self::Budget(e) => e.fmt(f),
            Self::EmptyAnswer {
                finish_reason: Some(reason),
            } => write!(
                f,
                "the model returned an empty answer (finish_reason: {reason})"
            ),
            Self::EmptyAnswer {
                finish_reason: None,
            } => f.write_str("the model returned an empty answer"),
        }
    }
}

impl std::error::Error for LoopError {}

impl LoopError {
    /// Whether the run failed for a reason that may pass: the model
    /// provider overloaded or briefly away (HTTP 429, 502, 503, 504), not
    /// answering in time, or not reachable. The task then ends failure class
    /// `transient`, which tells the caller a retry may succeed; any other
    /// failure is `internal`. Seen live (2026-10-30): `llama-server` answers
    /// `503` while it loads a model, for minutes, and every task ended
    /// `internal`, which a client does not retry.
    pub(super) const fn is_transient(&self) -> bool {
        matches!(
            self,
            Self::Model(
                ModelError::Status {
                    status: 429 | 502 | 503 | 504,
                    ..
                } | ModelError::RateLimited { .. }
                    | ModelError::Timeout(_)
                    | ModelError::Transport(_)
            )
        )
    }

    /// What the A2A caller is told when the run fails.
    ///
    /// Not [`Display`](fmt::Display): that carries what the provider sent
    /// back, up to 512 bytes of an error body for a status (which can echo
    /// the prompt, an account or quota detail, or an internal address), and
    /// the text of a transport error (which names the provider's URL). The
    /// caller is another agent or a person at the other end of the network,
    /// not the operator, so it gets the status or the kind of failure and
    /// the operator gets the rest in the log. A tool's own message and a
    /// [`ToolModel`](super::ToolModel)'s [`Other`](ModelError::Other) are the
    /// integrator's words and go through unchanged.
    pub(super) fn caller_message(&self) -> String {
        match self {
            Self::Model(ModelError::Status { status, .. }) => {
                format!("the model provider answered HTTP {status}")
            }
            Self::Model(ModelError::RateLimited { .. }) => {
                "the model provider answered HTTP 429".to_owned()
            }
            Self::Model(ModelError::Transport(_)) => {
                "could not reach the model provider".to_owned()
            }
            Self::Model(ModelError::InvalidResponse(_)) => {
                "the model provider's answer could not be read".to_owned()
            }
            Self::Model(ModelError::Config(_)) => "the model client is misconfigured".to_owned(),
            // The reason is the provider's own string: named when it is one
            // of the two this crate knows, and left to the operator's log
            // otherwise.
            Self::EmptyAnswer { finish_reason } => match finish_reason.as_deref() {
                Some(FINISH_REASON_LENGTH) => {
                    "the model stopped at its output limit before it wrote any text".to_owned()
                }
                Some(FINISH_REASON_CONTENT_FILTER) => {
                    "the model provider withheld the answer (content filter)".to_owned()
                }
                _ => "the model returned an empty answer".to_owned(),
            },
            other => other.to_string(),
        }
    }

    /// The `error.type` a run that ended in this records
    /// (`rpc_span/tool_metrics.rs` lists them).
    pub(super) fn error_type(&self) -> String {
        match self {
            Self::Model(e) => e.error_type(),
            Self::Fatal { .. } => "tool_fatal".to_owned(),
            Self::TurnLimit(_) => "turn_limit".to_owned(),
            Self::Cancelled => "cancelled".to_owned(),
            Self::Budget(_) => "budget_exhausted".to_owned(),
            Self::EmptyAnswer { .. } => "empty_answer".to_owned(),
        }
    }

    /// The failure class a task ends in for this error (the failure-class
    /// extension; `FailureClass`'s own page says what each means to a
    /// caller). A provider that may come back is `transient`. An empty
    /// answer is a bound when the model hit its output limit (`length`:
    /// retrying the same request hits it again), a refusal when the
    /// provider's content filter withheld it, and `internal` when the model
    /// just sent nothing. [`TurnLimit`](Self::TurnLimit) and
    /// [`Budget`](Self::Budget) are bounds. [`Cancelled`](Self::Cancelled)
    /// ends no task as failed, so what it returns is not sent.
    pub(super) fn failure_class(&self) -> FailureClass {
        if self.is_transient() {
            return FailureClass::Transient;
        }
        match self {
            Self::TurnLimit(_) | Self::Budget(_) => FailureClass::BudgetExhausted,
            Self::EmptyAnswer { finish_reason } => match finish_reason.as_deref() {
                Some(FINISH_REASON_LENGTH) => FailureClass::BudgetExhausted,
                Some(FINISH_REASON_CONTENT_FILTER) => FailureClass::PolicyRefusal,
                _ => FailureClass::Internal,
            },
            Self::Model(_) | Self::Fatal { .. } | Self::Cancelled => FailureClass::Internal,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_provider_that_may_come_back_is_transient_and_nothing_else_is() {
        let status = |status| {
            LoopError::Model(ModelError::Status {
                status,
                body: String::new(),
            })
        };
        for s in [429, 502, 503, 504] {
            assert!(status(s).is_transient(), "{s}");
        }
        for s in [400, 401, 404, 500, 501] {
            assert!(!status(s).is_transient(), "{s}");
        }
        assert!(LoopError::Model(ModelError::Timeout(Duration::from_secs(1))).is_transient());
        assert!(LoopError::Model(ModelError::Transport("refused".into())).is_transient());
        assert!(!LoopError::Model(ModelError::InvalidResponse("x".into())).is_transient());
        assert!(!LoopError::Model(ModelError::Config("x".into())).is_transient());
        assert!(!LoopError::Model(ModelError::Other("x".into())).is_transient());
        assert!(!LoopError::TurnLimit(3).is_transient());
    }

    /// A rate-limited turn is a 429 to everything outside the loop: transient,
    /// told to the caller as the status alone (not the body, which can carry
    /// the account's quota), and recorded as `429` (audit H17).
    #[test]
    fn a_rate_limited_turn_is_a_transient_429_without_its_body() {
        let limited = LoopError::Model(ModelError::RateLimited {
            retry_after: Some(Duration::from_secs(2)),
            body: "quota for org-acme exceeded".to_owned(),
        });
        assert!(limited.is_transient());
        assert_eq!(
            limited.caller_message(),
            "the model provider answered HTTP 429"
        );
        assert_eq!(limited.error_type(), "429");
    }

    /// What a caller is told, per failure: the status or the kind, never the
    /// provider's body, URL or response text; the integrator's own words as
    /// they wrote them.
    #[test]
    fn the_caller_is_told_the_kind_of_failure_not_what_the_provider_sent() {
        let said = |e: LoopError| e.caller_message();
        assert_eq!(
            said(LoopError::Model(ModelError::Status {
                status: 429,
                body: "quota for org-acme exceeded; prompt: <the user's text>".to_owned(),
            })),
            "the model provider answered HTTP 429"
        );
        assert_eq!(
            said(LoopError::Model(ModelError::Transport(
                "connect to http://10.0.3.7:8080 refused".to_owned()
            ))),
            "could not reach the model provider"
        );
        assert_eq!(
            said(LoopError::Model(ModelError::InvalidResponse(
                "expected a string at line 1 column 9: sk-live-123".to_owned()
            ))),
            "the model provider's answer could not be read"
        );
        assert_eq!(
            said(LoopError::Model(ModelError::Config(
                "https endpoint https://llm.internal without TLS".to_owned()
            ))),
            "the model client is misconfigured"
        );
        // Words the integrator wrote, and facts the caller needs.
        assert_eq!(
            said(LoopError::Model(ModelError::Other(
                "quota exhausted for this tenant".to_owned()
            ))),
            "quota exhausted for this tenant"
        );
        assert_eq!(
            said(LoopError::Fatal {
                tool: "restart".to_owned(),
                message: "no such service".to_owned()
            }),
            "tool 'restart' failed: no such service"
        );
        assert_eq!(
            said(LoopError::Model(ModelError::Timeout(Duration::from_secs(
                30
            )))),
            "the model provider did not answer within 30s"
        );
        // The full text, for the operator, still carries the body.
        let full = LoopError::Model(ModelError::Status {
            status: 500,
            body: "details".to_owned(),
        });
        assert!(full.to_string().contains("details"));
    }

    fn empty(reason: Option<&str>) -> LoopError {
        LoopError::EmptyAnswer {
            finish_reason: reason.map(str::to_owned),
        }
    }

    /// An empty answer is not transient, is recorded as `empty_answer`, and
    /// says the reason in the operator's text only when there is one.
    #[test]
    fn an_empty_answer_is_recorded_and_worded() {
        assert!(!empty(Some("length")).is_transient());
        assert_eq!(empty(None).error_type(), "empty_answer");
        assert_eq!(empty(Some("stop")).error_type(), "empty_answer");
        assert_eq!(
            empty(Some("stop")).to_string(),
            "the model returned an empty answer (finish_reason: stop)"
        );
        assert_eq!(
            empty(None).to_string(),
            "the model returned an empty answer"
        );
    }

    /// The caller is told the cause when it is one of the two this crate
    /// knows, and otherwise nothing of what the provider sent as a reason.
    #[test]
    fn the_caller_is_told_why_an_answer_is_empty_without_the_providers_words() {
        assert_eq!(
            empty(Some("length")).caller_message(),
            "the model stopped at its output limit before it wrote any text"
        );
        assert_eq!(
            empty(Some("content_filter")).caller_message(),
            "the model provider withheld the answer (content filter)"
        );
        for other in [
            Some("stop"),
            Some("account acme is over quota"),
            Some(""),
            None,
        ] {
            assert_eq!(
                empty(other).caller_message(),
                "the model returned an empty answer",
                "{other:?}"
            );
        }
    }

    /// The failure class of every way a run can end: transient for a provider
    /// that may come back; a bound for the turn limit, the budget and a reply
    /// cut at the output limit; a refusal for a content filter; internal for
    /// the rest.
    #[test]
    fn each_error_maps_to_the_failure_class_a_caller_can_act_on() {
        use FailureClass::{BudgetExhausted, Internal, PolicyRefusal, Transient};
        let status = |status| {
            LoopError::Model(ModelError::Status {
                status,
                body: String::new(),
            })
        };
        let cases = [
            (status(503), Transient),
            (
                LoopError::Model(ModelError::Timeout(Duration::from_secs(1))),
                Transient,
            ),
            (status(500), Internal),
            (LoopError::Model(ModelError::Config("x".into())), Internal),
            (
                LoopError::Fatal {
                    tool: "t".into(),
                    message: "m".into(),
                },
                Internal,
            ),
            (LoopError::TurnLimit(3), BudgetExhausted),
            (LoopError::Budget(Exhausted::Time), BudgetExhausted),
            (LoopError::Cancelled, Internal),
            (empty(Some("length")), BudgetExhausted),
            (empty(Some("content_filter")), PolicyRefusal),
            (empty(Some("stop")), Internal),
            (empty(Some("Length")), Internal),
            (empty(None), Internal),
        ];
        for (error, class) in cases {
            assert_eq!(error.failure_class(), class, "{error:?}");
        }
    }
}
