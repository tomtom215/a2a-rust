// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! How a [`ToolLoop`] writes and reads its [`StepJournal`]; what the journal is
//! and promises is in [`super::journal`].

use std::collections::HashMap;

use ring::digest;

use super::ToolLoop;
use super::idempotency::canonical;
use super::journal::{
    ActionReceipt, CHECKPOINT_VERSION, JournalError, PendingApproval, Pins, ReceiptState,
    RunCheckpoint, StepJournal,
};
use super::loop_error::{ApprovalNeeded, LoopError};
use super::model::{ChatMessage, ToolCall, ToolSpec};
use super::policy::TrustedIdentity;
use super::run::Progress;

/// `sha256:<hex>` of `bytes`.
pub(super) fn sha256_text(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    let mut out = String::from("sha256:");
    for byte in digest::digest(&digest::SHA256, bytes).as_ref() {
        let _ = write!(out, "{byte:02x}");
    }
    out
}

/// `sha256:<hex>` of a call's arguments in canonical form, or of the text when
/// it is not JSON: what a receipt holds in place of the arguments.
pub(super) fn arguments_digest(call: &ToolCall) -> String {
    let text = super::run_support::parse_arguments(&call.arguments).map_or_else(
        |_| call.arguments.clone(),
        |value| {
            let mut text = String::new();
            canonical(&value, &mut text);
            text
        },
    );
    sha256_text(text.as_bytes())
}

/// A person's decision on a parked call, as the loop is given it.
#[derive(Debug, Clone)]
pub(super) struct Verdict {
    /// The digest of the call the person was shown.
    pub(super) digest: String,
    /// Whether they approved it.
    pub(super) approved: bool,
    /// Who decided.
    pub(super) by: String,
}

/// The calls of a turn that the journal says are still to be made.
pub(super) struct Resume {
    pub(super) calls: Vec<ToolCall>,
    pub(super) recovered: bool,
}

/// A run's state at its start: new, or restored from a checkpoint.
pub(super) struct Restored {
    pub(super) messages: Vec<ChatMessage>,
    pub(super) trace: Vec<String>,
    pub(super) done: HashMap<String, String>,
    pub(super) first_turn: usize,
    pub(super) resume: Option<Resume>,
    /// The requester's identity, when the run is resumed after an approval:
    /// the run is theirs, not the approver's who continued it.
    pub(super) identity: Option<TrustedIdentity>,
}

impl Restored {
    /// The run's progress, with what it had already shown its caller shown
    /// again to `sink`; where it begins; what it still has to do; and whose it is.
    pub(super) fn begin(
        self,
        sink: Option<&super::trace::Sink>,
    ) -> (Progress, usize, Option<Resume>, Option<TrustedIdentity>) {
        let mut progress = Progress {
            messages: self.messages,
            trace: Vec::new(),
            done: self.done,
        };
        for line in self.trace {
            super::trace::record(&mut progress.trace, sink, line);
        }
        (progress, self.first_turn, self.resume, self.identity)
    }
}

/// One run's handle on its journal.
pub(super) struct RunJournal<'a> {
    store: Option<&'a dyn StepJournal>,
    key: &'a str,
    pins: Pins,
    revision: u64,
    receipts: Vec<ActionReceipt>,
    pending: Option<PendingApproval>,
}

fn mismatch(what: &str) -> LoopError {
    LoopError::PinMismatch(format!(
        "the run was begun with a different {what}; it is not continued under another"
    ))
}

fn journal_error(e: &JournalError) -> LoopError {
    LoopError::Journal(e.to_string())
}

impl ToolLoop {
    /// What this loop is, for a checkpoint to pin.
    fn pins(&self, specs: &[ToolSpec]) -> Pins {
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
    let ChatMessage::Assistant { tool_calls, .. } = &messages[at] else {
        return Vec::new();
    };
    let answered: Vec<&str> = messages[at + 1..]
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

impl<'a> RunJournal<'a> {
    /// Begins a run: restores it from the journal when it was begun before.
    ///
    /// # Errors
    ///
    /// [`LoopError::Journal`] when the journal cannot be read, or an approval
    /// arrives for a run that is not waiting for one; [`LoopError::PinMismatch`]
    /// when the run was begun with something else; [`LoopError::Indeterminate`]
    /// when a call may have run and cannot safely be run again.
    pub(super) async fn open(
        tool_loop: &'a ToolLoop,
        key: &'a str,
        user_text: &str,
        specs: &[ToolSpec],
        verdict: Option<&Verdict>,
        current: Option<&TrustedIdentity>,
    ) -> Result<(Self, Restored), LoopError> {
        let store = tool_loop.journal.as_deref();
        let mut journal = Self {
            store,
            key,
            pins: tool_loop.pins(specs),
            revision: 0,
            receipts: Vec::new(),
            pending: None,
        };
        let fresh = || Restored {
            messages: vec![ChatMessage::User(user_text.to_owned())],
            trace: Vec::new(),
            done: HashMap::new(),
            first_turn: 0,
            resume: None,
            identity: None,
        };
        let Some(store) = store else {
            if verdict.is_some() {
                return Err(LoopError::Journal(
                    "an approval arrived for a loop with no journal, so no run is waiting for it"
                        .to_owned(),
                ));
            }
            return Ok((journal, fresh()));
        };
        let stored = store.load(key).await.map_err(|e| journal_error(&e))?;
        let Some(cp) = stored else {
            if verdict.is_some() {
                return Err(LoopError::Journal(
                    "no run is waiting for this approval".to_owned(),
                ));
            }
            return Ok((journal, fresh()));
        };
        let restored = journal.restore(cp, tool_loop, user_text, verdict, current)?;
        Ok((journal, restored))
    }

    /// The run `cp` recorded, after checking it is the run being asked for.
    fn restore(
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
                .any(|r| r.call_id == c.id && r.state == ReceiptState::Started)
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

    /// Whether there is a journal to write to.
    pub(super) const fn enabled(&self) -> bool {
        self.store.is_some()
    }

    /// Notes that `call` is about to run, to be written with the next
    /// checkpoint.
    pub(super) fn started(&mut self, call: &ToolCall, arguments: String) {
        if self.store.is_none() {
            return;
        }
        match self.receipts.iter_mut().find(|r| r.call_id == call.id) {
            Some(r) => r.state = ReceiptState::Started,
            None => self.receipts.push(ActionReceipt {
                call_id: call.id.clone(),
                tool: call.name.clone(),
                arguments,
                state: ReceiptState::Started,
            }),
        }
    }

    /// Notes how `call` ended.
    pub(super) fn ended(&mut self, call_id: &str, state: ReceiptState) {
        if let Some(r) = self.receipts.iter_mut().find(|r| r.call_id == call_id) {
            r.state = state;
        }
        self.pending = None;
    }

    /// Parks the run on `call`.
    pub(super) fn park(
        &mut self,
        call: &ToolCall,
        needed: &ApprovalNeeded,
        identity: Option<&TrustedIdentity>,
    ) {
        self.ended(&call.id, ReceiptState::AwaitingApproval);
        self.pending = Some(PendingApproval {
            call: call.clone(),
            digest: needed.digest.clone(),
            summary: needed.summary.clone(),
            principal: identity.and_then(|i| i.principal.clone()),
            tenant: identity.and_then(|i| i.tenant.clone()),
            task: identity.and_then(|i| i.task.clone()),
        });
    }

    /// Writes a checkpoint of the run as it stands.
    ///
    /// # Errors
    ///
    /// [`LoopError::Journal`] when it cannot be written, which stops the run
    /// before whatever it was about to record.
    pub(super) async fn save(
        &mut self,
        step: (usize, bool),
        progress: &Progress,
    ) -> Result<(), LoopError> {
        let Some(store) = self.store else {
            return Ok(());
        };
        let checkpoint = RunCheckpoint {
            version: CHECKPOINT_VERSION,
            revision: self.revision + 1,
            pins: self.pins.clone(),
            turn: step.0,
            recovered: step.1,
            messages: progress.messages.clone(),
            trace: progress.trace.clone(),
            done: progress.done.clone(),
            receipts: self.receipts.clone(),
            pending_approval: self.pending.clone(),
        };
        store
            .save(self.key, &checkpoint)
            .await
            .map_err(|e| journal_error(&e))?;
        self.revision = checkpoint.revision;
        Ok(())
    }

    /// The run is over: forgets it. A failure is the operator's to see and not
    /// the run's, which has its answer.
    pub(super) async fn clear(&self) {
        if let Some(store) = self.store
            && let Err(_e) = store.clear(self.key).await
        {
            trace_warn!(error = %_e, "tool loop: a finished run's checkpoint could not be cleared");
        }
    }
}
