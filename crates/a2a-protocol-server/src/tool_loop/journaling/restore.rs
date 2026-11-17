// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Restoring a run from its checkpoint: that it is the run being asked for
//! (pins, message, an approval for the call that waits), and where it goes on.

use super::super::ToolLoop;
use super::super::idempotency::canonical;
use super::super::journal::{CHECKPOINT_VERSION, Pins, ReceiptState, RunCheckpoint};
use super::super::loop_error::LoopError;
use super::super::model::{ChatMessage, ToolCall, ToolSpec};
use super::super::policy::TrustedIdentity;
use super::{Restored, Resume, RunJournal, Verdict, sha256_text};

fn mismatch(what: &str) -> LoopError {
    LoopError::PinMismatch(format!(
        "the run was begun with a different {what}; it is not continued under another"
    ))
}

impl ToolLoop {
    /// What this loop is, for a checkpoint to pin.
    pub(super) fn pins(&self, specs: &[ToolSpec]) -> Pins {
        let mut tools: Vec<serde_json::Value> = specs
            .iter()
            .map(|s| {
                let read_only = self.tools.get(&s.name).is_some_and(|t| t.read_only());
                serde_json::json!({
                    "name": s.name, "description": s.description,
                    "parameters": s.parameters, "readOnly": read_only,
                })
            })
            .collect();
        tools.sort_by(|a, b| a["name"].as_str().cmp(&b["name"].as_str()));
        let mut text = String::new();
        canonical(&serde_json::Value::Array(tools), &mut text);
        let modes = format!(
            "{:?}|{:?}|{:?}|{}|{}",
            self.text_recovery,
            self.argument_check,
            self.trace_content,
            self.max_turns,
            self.max_calls_per_turn
        );
        Pins {
            model: self.model.model_name().to_owned(),
            preamble: sha256_text(self.preamble.as_bytes()),
            tools: sha256_text(text.as_bytes()),
            modes: sha256_text(modes.as_bytes()),
            labels: self.pin_labels.clone(),
        }
    }
}

/// The calls of the last assistant turn that no tool result answers.
fn unanswered(messages: &[ChatMessage]) -> Vec<ToolCall> {
    let Some(at) = messages
        .iter()
        .rposition(|m| matches!(m, ChatMessage::Assistant { .. }))
    else {
        return Vec::new();
    };
    let Some((ChatMessage::Assistant { tool_calls, .. }, after)) = messages[at..].split_first()
    else {
        return Vec::new();
    };
    let answered: Vec<&str> = after
        .iter()
        .filter_map(|m| match m {
            ChatMessage::ToolResult { call_id, .. } => Some(call_id.as_str()),
            _ => None,
        })
        .collect();
    tool_calls
        .iter()
        .filter(|c| !answered.contains(&c.id.as_str()))
        .cloned()
        .collect()
}

impl RunJournal<'_> {
    /// The run `cp` recorded, after checking it is the run being asked for.
    pub(super) fn restore(
        &mut self,
        cp: RunCheckpoint,
        tool_loop: &ToolLoop,
        user_text: &str,
        verdict: Option<&Verdict>,
        current: Option<&TrustedIdentity>,
    ) -> Result<Restored, LoopError> {
        if cp.version != CHECKPOINT_VERSION {
            return Err(LoopError::Journal(format!(
                "the checkpoint is version {}, this loop reads {CHECKPOINT_VERSION}",
                cp.version
            )));
        }
        self.check_pins(&cp, tool_loop)?;
        if verdict.is_none()
            && cp.messages.first() != Some(&ChatMessage::User(user_text.to_owned()))
        {
            return Err(mismatch("message"));
        }
        if cp.answer.is_some() {
            return self.finished(cp, verdict.is_some());
        }
        let identity = match verdict {
            Some(v) => Some(Self::check_approval(&cp, v, current)?),
            None => None,
        };
        let calls = unanswered(&cp.messages);
        // A call that may have run is re-run only where its key is the same
        // on every replica, so the backend can tell it was asked before.
        if let Some(call) = calls.iter().find(|c| {
            cp.receipts
                .iter()
                .any(|r| r.is(cp.turn, &c.id) && r.state == ReceiptState::Started)
        }) && !tool_loop.secret_set
        {
            return Err(LoopError::Indeterminate {
                tool: call.name.clone(),
                call_id: call.id.clone(),
            });
        }
        self.revision = cp.revision;
        self.receipts = cp.receipts;
        self.pending = cp.pending_approval;
        let resume = (!calls.is_empty()).then_some(Resume {
            calls,
            recovered: cp.recovered,
        });
        let first_turn = if resume.is_some() {
            cp.turn
        } else {
            cp.turn + 1
        };
        Ok(Restored {
            messages: cp.messages,
            trace: cp.trace,
            done: cp.done,
            first_turn,
            resume,
            identity,
            answer: None,
        })
    }

    /// A run that has its answer: given again, unless what arrived is an
    /// approval, which has nothing left to release.
    fn finished(&mut self, cp: RunCheckpoint, approving: bool) -> Result<Restored, LoopError> {
        if approving {
            return Err(LoopError::Journal(
                "the run this approval is for has finished".to_owned(),
            ));
        }
        self.revision = cp.revision;
        self.receipts = cp.receipts;
        Ok(Restored {
            messages: cp.messages,
            trace: cp.trace,
            done: cp.done,
            first_turn: cp.turn,
            resume: None,
            identity: None,
            answer: cp.answer,
        })
    }

    fn check_pins(&self, cp: &RunCheckpoint, tool_loop: &ToolLoop) -> Result<(), LoopError> {
        let (a, b) = (&cp.pins, &self.pins);
        if a.model != b.model {
            return Err(mismatch("model"));
        }
        if a.preamble != b.preamble {
            return Err(mismatch("preamble"));
        }
        if a.tools != b.tools {
            return Err(mismatch("tool set"));
        }
        if a.modes != b.modes {
            return Err(mismatch("configuration"));
        }
        if a.labels != tool_loop.pin_labels {
            return Err(mismatch("pinned version"));
        }
        Ok(())
    }

    /// Checks a decision against the call that waits for it, and returns the
    /// identity of the requester the run belongs to.
    fn check_approval(
        cp: &RunCheckpoint,
        verdict: &Verdict,
        current: Option<&TrustedIdentity>,
    ) -> Result<TrustedIdentity, LoopError> {
        let Some(pending) = &cp.pending_approval else {
            return Err(LoopError::Journal(
                "no call is waiting for this approval".to_owned(),
            ));
        };
        if pending.digest != verdict.digest {
            return Err(LoopError::Journal(
                "the approval answers a different action than the one waiting".to_owned(),
            ));
        }
        // Another tenant's or another task's approval does not release this one.
        if let Some(now) = current
            && (now.tenant != pending.tenant || now.task != pending.task)
        {
            return Err(LoopError::Journal(
                "the approval comes from another tenant or task than the run's".to_owned(),
            ));
        }
        let mut identity = TrustedIdentity::default();
        identity.tenant.clone_from(&pending.tenant);
        identity.principal.clone_from(&pending.principal);
        identity.task.clone_from(&pending.task);
        Ok(identity)
    }
}
