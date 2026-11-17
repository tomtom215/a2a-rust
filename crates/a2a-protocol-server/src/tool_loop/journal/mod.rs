// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! A durable journal of a tool-loop run: what a restarted replica needs to
//! carry on, and what an auditor needs to say what the run did (adversarial
//! review, "task durability does not yet establish durable arbitrary
//! workflows", 2026-11-12).
//!
//! # What was lost, and what is kept
//!
//! A run's conversation, its record of calls already answered and its trace
//! lived in memory for the run. A replica that died mid-run lost them, and the
//! next attempt began again: the model was asked afresh and every tool it chose
//! ran again. With a [`StepJournal`] installed
//! ([`ToolLoop::with_journal`](super::ToolLoop::with_journal)) the loop writes a
//! [`RunCheckpoint`] at each step, and a run begun again under the same key
//! (the task and message it answers) **resumes**: the conversation is restored,
//! calls already answered are not asked for or run again, and the model is
//! asked only for what comes next.
//!
//! * **Resume points.** After each model reply, before each call and after
//!   each call. A checkpoint carries the whole conversation, so a resume
//!   reads one record.
//! * **Pins.** The checkpoint records the model's name and digests of the
//!   preamble, the tool set (names, descriptions, schemas, read-only flags)
//!   and the loop's modes, plus any labels the deployment adds
//!   ([`with_pin`](super::ToolLoop::with_pin): a policy version, a skill
//!   bundle digest). A run resumed under different pins is **refused**
//!   ([`LoopError::PinMismatch`](super::LoopError::PinMismatch)), not
//!   continued under a model or a tool set it did not begin with.
//! * **Action receipts.** A receipt is written *before* a call runs
//!   ([`ReceiptState::Started`]) and updated when it ends. A call whose receipt
//!   is still `Started` on resume may have run: the loop re-runs it only if the
//!   idempotency secret was set explicitly
//!   ([`with_idempotency_secret`](super::ToolLoop::with_idempotency_secret)),
//!   so that its key is the same on every replica and the backend can
//!   deduplicate; otherwise the run fails
//!   ([`LoopError::Indeterminate`](super::LoopError::Indeterminate)) rather than
//!   guess. **External effects still need the backend's idempotency**: the
//!   journal makes a repeat rare and visible, not impossible.
//! * **Fail closed.** A checkpoint the journal cannot write stops the run
//!   *before* the call it was about to record
//!   ([`LoopError::Journal`](super::LoopError::Journal)).
//! * **Fencing.** Each checkpoint has a revision, and a write is accepted only
//!   if it follows the one stored, so a replica that lost its claim cannot
//!   overwrite a newer run's progress ([`JournalError::Conflict`]).
//! * **Pending approvals.** A call a policy says needs approval parks the run
//!   here with the digest of that exact call; see
//!   [`PolicyDecision::RequireApproval`](super::PolicyDecision::RequireApproval).
//!
//! * **A finished run is kept, not deleted.** Its final answer is written as a
//!   *completed* checkpoint, under the same revision fence as every other
//!   write, before the loop returns it. A run begun again under that key
//!   returns the stored answer, asks no model and runs no tool, so a process
//!   that died between the answer and the task's `Completed` publishes the
//!   same answer when the run is replayed (adversarial review N04,
//!   2026-11-13; until then the checkpoint was deleted before the executor
//!   published, and an unfenced delete of a stale run could remove a newer
//!   run's checkpoint). The loop never deletes; [`StepJournal::sweep`] does,
//!   on the deployment's retention schedule, and [`StepJournal::clear`] only
//!   the revision it is given.
//! * **Who a run is for, and how long it is kept** (N07). Each entry carries
//!   its owner (tenant, authenticated caller, task) in fields of their own,
//!   so [`StepJournal::runs_of`] can list a person's runs for an access
//!   request, [`StepJournal::erase`] can delete them, and
//!   [`StepJournal::sweep`] can drop completed runs past a replay window and
//!   unfinished ones past a longer one. A run's content (the conversation,
//!   tool arguments and results, the trace) is in the entry's body, which
//!   `ToolLoop::with_journal_cipher` (the `encryption` feature) seals with a
//!   `ContentCipher`; without one the body is JSON.
//!
//! The journal is the run's, not the task's: it is keyed by what the loop is
//! given as its scope. Neither the task store's erasure nor the audit log's
//! reaches it: call [`StepJournal::erase`] alongside them. Compensation
//! (undoing a call that ran) and retry policy beyond the model retry are not
//! here.

use std::collections::{BTreeMap, HashMap};
use std::future::Future;
use std::pin::Pin;

use serde::{Deserialize, Serialize};

use super::model::{ChatMessage, ToolCall};

mod memory;
#[cfg(feature = "postgres")]
mod postgres;
#[cfg(feature = "sqlite")]
mod sqlite;

#[cfg(test)]
mod tests;

pub use memory::InMemoryStepJournal;
#[cfg(feature = "postgres")]
pub use postgres::PostgresStepJournal;
#[cfg(feature = "sqlite")]
pub use sqlite::SqliteStepJournal;

/// The checkpoint format's version. A checkpoint of another version is refused.
pub const CHECKPOINT_VERSION: u32 = 1;

/// What a run was begun with, which a resume must still be.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Pins {
    /// The model's name.
    pub model: String,
    /// `sha256:<hex>` of the preamble.
    pub preamble: String,
    /// `sha256:<hex>` of the tool set: each tool's name, description, schema
    /// and read-only flag, by name.
    pub tools: String,
    /// `sha256:<hex>` of the loop's modes (recovery, argument check, trace).
    pub modes: String,
    /// The deployment's own pins ([`ToolLoop::with_pin`](super::ToolLoop::with_pin)).
    pub labels: BTreeMap<String, String>,
}

/// Where a call stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ReceiptState {
    /// Written before the call ran; the call may or may not have.
    Started,
    /// The call ran and the tool answered.
    Ran,
    /// The call failed, or was refused before it ran.
    Failed,
    /// An identical call had already run; this one was answered from that.
    NotRun,
    /// Waiting for a person's decision.
    AwaitingApproval,
}

/// One call's record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct ActionReceipt {
    /// The turn whose reply asked for the call. With the id, what names one
    /// call in a run: a provider may use an id again in a later turn
    /// (adversarial review N08, 2026-11-13).
    #[serde(default)]
    pub turn: usize,
    /// The provider's id for the call.
    pub call_id: String,
    /// The tool.
    pub tool: String,
    /// `sha256:<hex>` of the arguments, never the arguments.
    pub arguments: String,
    /// Where the call stands.
    pub state: ReceiptState,
}

impl ActionReceipt {
    /// Whether this is the receipt of call `call_id` of turn `turn`.
    pub(crate) fn is(&self, turn: usize, call_id: &str) -> bool {
        self.turn == turn && self.call_id == call_id
    }
}

/// A call parked for a person's decision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct PendingApproval {
    /// The call, exactly as the model made it.
    pub call: ToolCall,
    /// `sha256:<hex>` of the call as an action: tool, arguments, requester,
    /// tenant and task ([`call_digest`](super::call_digest)).
    pub digest: String,
    /// What the person is shown.
    pub summary: String,
    /// The requester: the authenticated caller whose run this is.
    pub principal: Option<String>,
    /// The tenant the run belongs to.
    pub tenant: Option<String>,
    /// The task the run belongs to.
    pub task: Option<String>,
}

/// Everything a run needs to carry on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct RunCheckpoint {
    /// [`CHECKPOINT_VERSION`].
    pub version: u32,
    /// Counts the checkpoint's writes; a write is accepted only as the
    /// successor of the revision stored (`0` stored means none).
    pub revision: u64,
    /// What the run was begun with.
    pub pins: Pins,
    /// The turn the checkpoint was written in.
    pub turn: usize,
    /// Whether the last assistant turn's calls were recovered from text.
    pub recovered: bool,
    /// The conversation so far.
    pub messages: Vec<ChatMessage>,
    /// The caller's trace so far.
    pub trace: Vec<String>,
    /// What each call that ran returned, by idempotency key.
    pub done: HashMap<String, String>,
    /// Every call, in order.
    pub receipts: Vec<ActionReceipt>,
    /// The call waiting for a decision, if one is.
    pub pending_approval: Option<PendingApproval>,
    /// The run's final answer, once it has one: the run is over, and a run
    /// begun again under its key returns this.
    #[serde(default)]
    pub answer: Option<FinalAnswer>,
}

/// What a finished run answered.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct FinalAnswer {
    /// The model's final text.
    pub text: String,
    /// Why the final reply stopped, as the provider said.
    pub finish_reason: Option<String>,
}

/// Why a journal did not do what it was asked.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum JournalError {
    /// The write did not follow the revision stored: another writer got there.
    Conflict,
    /// The store failed, or held something unreadable.
    Store(String),
}

impl std::fmt::Display for JournalError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Conflict => f.write_str("another writer advanced the run's checkpoint"),
            Self::Store(e) => write!(f, "the run journal failed: {e}"),
        }
    }
}

impl std::error::Error for JournalError {}

/// What a journal's methods return.
pub type JournalFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T, JournalError>> + Send + 'a>>;

/// Whose run an entry is: from the authenticated request, never from what the
/// model wrote. What [`StepJournal::runs_of`] and [`StepJournal::erase`] find
/// a run by.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct RunOwner {
    /// The request's tenant.
    pub tenant: Option<String>,
    /// The authenticated caller the run answers.
    pub principal: Option<String>,
    /// The task the run belongs to.
    pub task: Option<String>,
}

impl RunOwner {
    /// An owner from its three parts.
    #[must_use]
    pub const fn new(
        tenant: Option<String>,
        principal: Option<String>,
        task: Option<String>,
    ) -> Self {
        Self {
            tenant,
            principal,
            task,
        }
    }

    /// Whether this is `principal`'s, in `tenant` when one is given.
    #[must_use]
    pub fn is_of(&self, principal: &str, tenant: Option<&str>) -> bool {
        self.principal.as_deref() == Some(principal)
            && tenant.is_none_or(|t| self.tenant.as_deref() == Some(t))
    }
}

/// One run as a [`StepJournal`] stores it. The journal reads `revision`,
/// `owner`, `completed` and `updated_at_ms`; `body` is opaque to it.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct JournalEntry {
    /// Counts the run's writes; see [`StepJournal::save`].
    pub revision: u64,
    /// Whose run it is.
    pub owner: RunOwner,
    /// Whether the run has its final answer.
    pub completed: bool,
    /// When the entry was written, in Unix milliseconds.
    pub updated_at_ms: i64,
    /// The [`RunCheckpoint`]: JSON, or sealed by the loop's journal cipher
    /// (a string beginning [`SEALED_CHECKPOINT_PREFIX`]).
    pub body: String,
}

impl JournalEntry {
    /// An entry from its parts.
    #[must_use]
    pub const fn new(
        revision: u64,
        owner: RunOwner,
        completed: bool,
        updated_at_ms: i64,
        body: String,
    ) -> Self {
        Self {
            revision,
            owner,
            completed,
            updated_at_ms,
            body,
        }
    }

    /// The checkpoint, when the body is not sealed: for an operator reading
    /// a journal written without a cipher.
    ///
    /// # Errors
    ///
    /// [`JournalError::Store`] when the body is sealed or unreadable.
    pub fn checkpoint(&self) -> Result<RunCheckpoint, JournalError> {
        if self.body.starts_with(SEALED_CHECKPOINT_PREFIX) {
            return Err(JournalError::Store(
                "the checkpoint is sealed; the loop's journal cipher opens it".to_owned(),
            ));
        }
        from_text(&self.body)
    }

    /// What [`StepJournal::runs_of`] lists of this entry under `key`.
    #[must_use]
    pub fn summary(&self, key: &str) -> RunSummary {
        RunSummary {
            key: key.to_owned(),
            revision: self.revision,
            owner: self.owner.clone(),
            completed: self.completed,
            updated_at_ms: self.updated_at_ms,
        }
    }
}

/// What [`StepJournal::runs_of`] lists of a run: everything but its content.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct RunSummary {
    /// The run's key.
    pub key: String,
    /// Its revision.
    pub revision: u64,
    /// Whose run it is.
    pub owner: RunOwner,
    /// Whether it has its final answer.
    pub completed: bool,
    /// When it was last written, in Unix milliseconds.
    pub updated_at_ms: i64,
}

/// The prefix of a checkpoint body sealed by the loop's journal cipher.
pub const SEALED_CHECKPOINT_PREFIX: &str = "a2a-sealed-run:v1:";

/// Where a run's checkpoints are kept.
///
/// An implementation must make [`save`](Self::save) and
/// [`clear`](Self::clear) atomic compare-and-sets on the revision: that is
/// the fence that keeps a replica which lost its claim from overwriting, or
/// deleting, a newer run's progress.
pub trait StepJournal: Send + Sync + 'static {
    /// The entry stored under `key`, if there is one.
    fn load<'a>(&'a self, key: &'a str) -> JournalFuture<'a, Option<JournalEntry>>;

    /// Stores `entry` under `key` only if the revision stored is
    /// `entry.revision - 1` (none stored counts as `0`), and returns
    /// [`JournalError::Conflict`] otherwise.
    fn save<'a>(&'a self, key: &'a str, entry: &'a JournalEntry) -> JournalFuture<'a, ()>;

    /// Deletes the entry under `key` only if its revision is `revision`:
    /// [`JournalError::Conflict`] when another revision is stored, `Ok` when
    /// none is. A stale run's delete cannot remove a newer run's entry.
    fn clear<'a>(&'a self, key: &'a str, revision: u64) -> JournalFuture<'a, ()>;

    /// Deletes completed runs last written before `completed_before_ms` and
    /// unfinished ones last written before `unfinished_before_ms` (Unix
    /// milliseconds); returns how many went. A run still going when its entry
    /// is swept is refused its next write ([`JournalError::Conflict`]: none
    /// stored takes only revision 1), so the unfinished cutoff must be well
    /// past the longest run and its approvals.
    fn sweep(&self, completed_before_ms: i64, unfinished_before_ms: i64) -> JournalFuture<'_, u64>;

    /// Every run of `principal`, in `tenant` when one is given, without its
    /// content: for an access request, and to know what
    /// [`erase`](Self::erase) would delete.
    fn runs_of<'a>(
        &'a self,
        principal: &'a str,
        tenant: Option<&'a str>,
    ) -> JournalFuture<'a, Vec<RunSummary>>;

    /// Deletes every run of `principal`, in `tenant` when one is given;
    /// returns how many went. What a person's erasure must call for the
    /// journal: the task store's and the audit log's do not reach it. A
    /// person named only inside a run's content (a tool argument, a message)
    /// is not found by this.
    fn erase<'a>(&'a self, principal: &'a str, tenant: Option<&'a str>) -> JournalFuture<'a, u64>;
}

/// The time now in Unix milliseconds, for an entry's `updated_at_ms`.
pub(super) fn now_ms() -> i64 {
    i64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_millis()),
    )
    .unwrap_or(i64::MAX)
}

pub(super) fn to_text(checkpoint: &RunCheckpoint) -> Result<String, JournalError> {
    serde_json::to_string(checkpoint)
        .map_err(|e| JournalError::Store(format!("a checkpoint could not be written: {e}")))
}

pub(super) fn from_text(text: &str) -> Result<RunCheckpoint, JournalError> {
    serde_json::from_str(text)
        .map_err(|e| JournalError::Store(format!("a stored checkpoint is unreadable: {e}")))
}
