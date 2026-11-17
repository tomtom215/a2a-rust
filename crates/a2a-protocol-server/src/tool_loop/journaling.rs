// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! How a [`ToolLoop`] writes and reads its [`StepJournal`]; what the journal is
//! and promises is in [`super::journal`].

use std::collections::HashMap;

use ring::digest;

use super::ToolLoop;
use super::idempotency::canonical;
use super::journal::{
    ActionReceipt, CHECKPOINT_VERSION, FinalAnswer, JournalEntry, JournalError, PendingApproval,
    Pins, ReceiptState, RunCheckpoint, RunOwner, StepJournal, now_ms,
};
use super::journal_codec::JournalHandle;

mod restore;
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
    /// The answer a finished run gave: a run begun again under its key gives
    /// it again and does nothing else (N04).
    pub(super) answer: Option<FinalAnswer>,
}

impl Restored {
    /// The run's progress, with what it had already shown its caller shown
    /// again to `sink`; where it begins; what it still has to do; and whose it is.
    pub(super) fn begin(
        self,
        sink: Option<&super::trace::Sink>,
    ) -> (
        Progress,
        usize,
        Option<Resume>,
        Option<TrustedIdentity>,
        Option<FinalAnswer>,
    ) {
        let mut progress = Progress {
            messages: self.messages,
            trace: Vec::new(),
            done: self.done,
        };
        for line in self.trace {
            super::trace::record(&mut progress.trace, sink, line);
        }
        (
            progress,
            self.first_turn,
            self.resume,
            self.identity,
            self.answer,
        )
    }
}

/// One run's handle on its journal.
pub(super) struct RunJournal<'a> {
    store: Option<&'a dyn StepJournal>,
    handle: &'a JournalHandle,
    key: &'a str,
    /// Whose run it is: from the request that began it, kept on resume.
    owner: RunOwner,
    pins: Pins,
    revision: u64,
    receipts: Vec<ActionReceipt>,
    pending: Option<PendingApproval>,
}

fn journal_error(e: &JournalError) -> LoopError {
    LoopError::Journal(e.to_string())
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
        let store = tool_loop.journal.store.as_deref();
        let mut journal = Self {
            store,
            handle: &tool_loop.journal,
            key,
            owner: RunOwner::new(
                current.and_then(|i| i.tenant.clone()),
                current.and_then(|i| i.principal.clone()),
                current.and_then(|i| i.task.clone()),
            ),
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
            answer: None,
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
        let Some(entry) = stored else {
            if verdict.is_some() {
                return Err(LoopError::Journal(
                    "no run is waiting for this approval".to_owned(),
                ));
            }
            return Ok((journal, fresh()));
        };
        let cp = journal
            .handle
            .decode(key, entry.revision, &entry.body)
            .await
            .map_err(|e| journal_error(&e))?;
        if cp.revision != entry.revision {
            return Err(LoopError::Journal(
                "a stored checkpoint's revision differs from its entry's".to_owned(),
            ));
        }
        journal.owner = entry.owner;
        let restored = journal.restore(cp, tool_loop, user_text, verdict, current)?;
        Ok((journal, restored))
    }

    /// Whether there is a journal to write to.
    pub(super) const fn enabled(&self) -> bool {
        self.store.is_some()
    }

    /// Notes that `call` is about to run, to be written with the next
    /// checkpoint.
    pub(super) fn started(&mut self, turn: usize, call: &ToolCall, arguments: String) {
        if self.store.is_none() {
            return;
        }
        match self.receipts.iter_mut().find(|r| r.is(turn, &call.id)) {
            Some(r) => r.state = ReceiptState::Started,
            None => self.receipts.push(ActionReceipt {
                turn,
                call_id: call.id.clone(),
                tool: call.name.clone(),
                arguments,
                state: ReceiptState::Started,
            }),
        }
    }

    /// Notes how `call` ended.
    pub(super) fn ended(&mut self, turn: usize, call_id: &str, state: ReceiptState) {
        if let Some(r) = self.receipts.iter_mut().find(|r| r.is(turn, call_id)) {
            r.state = state;
        }
        self.pending = None;
    }

    /// Parks the run on `call`.
    pub(super) fn park(
        &mut self,
        turn: usize,
        call: &ToolCall,
        needed: &ApprovalNeeded,
        identity: Option<&TrustedIdentity>,
    ) {
        self.ended(turn, &call.id, ReceiptState::AwaitingApproval);
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
        self.write(step, progress, None)
            .await
            .map_err(|e| journal_error(&e))
    }

    /// Writes the run's final answer as a completed checkpoint, before the
    /// loop returns it (N04). Nothing is deleted: a run begun again under the
    /// key gives this answer, and the journal's sweep removes the entry on
    /// the deployment's schedule.
    ///
    /// # Errors
    ///
    /// [`LoopError::Journal`] when another writer advanced the run
    /// ([`JournalError::Conflict`]): this invocation is stale, and must not
    /// publish an answer for a run another one owns. A store that fails
    /// otherwise is the operator's to see, not the run's: the answer is
    /// returned, and a replay asks the model for the last turn again (every
    /// call before it is answered from the journal, none is run again).
    pub(super) async fn complete(
        &mut self,
        step: (usize, bool),
        progress: &Progress,
        answer: &super::Answer,
    ) -> Result<(), LoopError> {
        let last = FinalAnswer {
            text: answer.text.clone(),
            finish_reason: answer.finish_reason.clone(),
        };
        match self.write(step, progress, Some(last)).await {
            Err(e @ JournalError::Conflict) => Err(journal_error(&e)),
            Err(_e) => {
                trace_warn!(error = %_e, "tool loop: a finished run's answer could not be journaled");
                Ok(())
            }
            Ok(()) => Ok(()),
        }
    }

    async fn write(
        &mut self,
        step: (usize, bool),
        progress: &Progress,
        answer: Option<FinalAnswer>,
    ) -> Result<(), JournalError> {
        let Some(store) = self.store else {
            return Ok(());
        };
        let completed = answer.is_some();
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
            answer,
        };
        let body = self.handle.encode(self.key, &checkpoint).await?;
        let entry = JournalEntry::new(
            checkpoint.revision,
            self.owner.clone(),
            completed,
            now_ms(),
            body,
        );
        store.save(self.key, &entry).await?;
        self.revision = checkpoint.revision;
        Ok(())
    }
}
