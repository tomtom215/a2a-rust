// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! [`LoopError`]: why a run of the loop produced no answer.

use std::fmt;
#[cfg(test)]
use std::time::Duration;

use a2a_protocol_types::budget::Exhausted;

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
}

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
                } | ModelError::Timeout(_)
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
            Self::Model(ModelError::Transport(_)) => {
                "could not reach the model provider".to_owned()
            }
            Self::Model(ModelError::InvalidResponse(_)) => {
                "the model provider's answer could not be read".to_owned()
            }
            Self::Model(ModelError::Config(_)) => "the model client is misconfigured".to_owned(),
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
}
