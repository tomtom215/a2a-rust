// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! [`LoopError`]: why a run of the loop produced no answer.

use std::fmt;

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
