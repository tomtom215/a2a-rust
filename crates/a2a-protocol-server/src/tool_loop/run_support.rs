// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! What a run of the loop asks of its budget and of a call's arguments: the
//! bound checked before a turn, the usage charged after one, and the
//! arguments a [`Tool`](super::Tool) is given. Free functions, kept out of
//! `run.rs` (the 500-line ratchet) so the loop itself stays in one file.

use a2a_protocol_types::budget::{BudgetHandle, Exhausted, TOKENS_METER};
use serde_json::Value;

use super::model::ToolCall;
use super::run::LoopError;
use super::trace::{self, Sink};

/// [`LoopError::Budget`] once `budget`'s deadline has passed or its
/// [`TOKENS_METER`] has nothing left: a turn would only overdraw it.
pub(super) fn out_of_time(budget: Option<&BudgetHandle>) -> Result<(), LoopError> {
    let Some(budget) = budget else { return Ok(()) };
    if budget.remaining_time() == Some(std::time::Duration::ZERO) {
        return Err(LoopError::Budget(Exhausted::Time));
    }
    if budget.remaining(TOKENS_METER) == Some(0) {
        return Err(LoopError::Budget(Exhausted::Meter {
            name: TOKENS_METER.to_owned(),
            overdraft: 0,
        }));
    }
    Ok(())
}

/// Charges `reply`'s reported usage, input plus output, to `budget`'s
/// [`TOKENS_METER`]; [`LoopError::Budget`] once that is spent.
pub(super) fn charge_usage(
    budget: Option<&BudgetHandle>,
    reply: &super::model::ModelTurn,
) -> Result<(), LoopError> {
    if let (Some(budget), Some(usage)) = (budget, reply.usage) {
        let used = usage
            .input_tokens
            .unwrap_or(0)
            .saturating_add(usage.output_tokens.unwrap_or(0));
        budget
            .charge(TOKENS_METER, used)
            .map_err(LoopError::Budget)?;
    }
    Ok(())
}

/// A call's arguments as the object a [`Tool`](super::Tool) is given. Empty
/// text is taken as no arguments, which some providers send for a tool that
/// takes none.
pub(super) fn parse_arguments(raw: &str) -> Result<Value, String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Ok(Value::Object(serde_json::Map::new()));
    }
    match serde_json::from_str::<Value>(raw) {
        Ok(object @ Value::Object(_)) => Ok(object),
        Ok(_) => Err("the arguments must be a JSON object".to_owned()),
        Err(e) => Err(format!("the arguments are not valid JSON ({e})")),
    }
}

/// Keeps the first `max` of `calls`; when it drops any, says so in the trace
/// (and the sink), first, ahead of the calls it names. The dropped calls are
/// not in the conversation either: each would need a result, and the point of
/// the bound is that a reply of thousands cannot be held.
pub(super) fn limit_calls(
    calls: &mut Vec<ToolCall>,
    max: usize,
    trace: &mut Vec<String>,
    sink: Option<&Sink>,
) {
    if calls.len() > max {
        let note = format!(
            "(a reply asked for {} tool calls; the first {max} ran)",
            calls.len()
        );
        calls.truncate(max);
        trace::record(trace, sink, note);
    }
}

/// Gives each of `calls` that has no id one, `call-<turn>-<position>`, so its
/// result can be matched to it.
pub(super) fn give_ids(calls: &mut [ToolCall], turn: usize) {
    for (i, call) in calls.iter_mut().enumerate() {
        if call.id.is_empty() {
            call.id = format!("call-{turn}-{i}");
        }
    }
}
