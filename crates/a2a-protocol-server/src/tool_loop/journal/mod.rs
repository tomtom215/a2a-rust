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
//! The journal is the run's, not the task's: it is keyed by what the loop is
//! given as its scope, and a finished run is cleared. Compensation (undoing a
//! call that ran) and retry policy beyond the model retry are not here.

use std::collections::{BTreeMap, HashMap};
use std::future::Future;
use std::pin::Pin;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use super::model::{ChatMessage, ToolCall};

#[cfg(feature = "postgres")]
mod postgres;
#[cfg(feature = "sqlite")]
mod sqlite;

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
    /// The provider's id for the call.
    pub call_id: String,
    /// The tool.
    pub tool: String,
    /// `sha256:<hex>` of the arguments, never the arguments.
    pub arguments: String,
    /// Where the call stands.
    pub state: ReceiptState,
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

/// Where a run's checkpoints are kept.
///
/// An implementation must make [`save`](Self::save) atomic: it stores
/// `checkpoint` only if the revision stored is `checkpoint.revision - 1` (none
/// stored counts as `0`), and returns [`JournalError::Conflict`] otherwise.
pub trait StepJournal: Send + Sync + 'static {
    /// The checkpoint stored under `key`, if there is one.
    fn load<'a>(&'a self, key: &'a str) -> JournalFuture<'a, Option<RunCheckpoint>>;

    /// Stores `checkpoint` under `key` if it follows the one stored.
    fn save<'a>(&'a self, key: &'a str, checkpoint: &'a RunCheckpoint) -> JournalFuture<'a, ()>;

    /// Forgets `key`: the run is over.
    fn clear<'a>(&'a self, key: &'a str) -> JournalFuture<'a, ()>;
}

/// A journal in this process's memory, for tests and for a deployment that
/// keeps a run on one replica and accepts losing it with the process.
#[derive(Debug, Default)]
pub struct InMemoryStepJournal {
    runs: Mutex<HashMap<String, String>>,
}

impl InMemoryStepJournal {
    /// An empty journal.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// How many runs it holds.
    #[must_use]
    pub fn len(&self) -> usize {
        self.runs
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .len()
    }

    /// Whether it holds none.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// The revision of a stored checkpoint's text, without reading the rest of it.
pub(super) fn revision_of(text: &str) -> Result<u64, JournalError> {
    #[derive(Deserialize)]
    struct Revision {
        revision: u64,
    }
    serde_json::from_str::<Revision>(text)
        .map(|r| r.revision)
        .map_err(|e| JournalError::Store(format!("a stored checkpoint is unreadable: {e}")))
}

pub(super) fn to_text(checkpoint: &RunCheckpoint) -> Result<String, JournalError> {
    serde_json::to_string(checkpoint)
        .map_err(|e| JournalError::Store(format!("a checkpoint could not be written: {e}")))
}

pub(super) fn from_text(text: &str) -> Result<RunCheckpoint, JournalError> {
    serde_json::from_str(text)
        .map_err(|e| JournalError::Store(format!("a stored checkpoint is unreadable: {e}")))
}

impl StepJournal for InMemoryStepJournal {
    fn load<'a>(&'a self, key: &'a str) -> JournalFuture<'a, Option<RunCheckpoint>> {
        Box::pin(async move {
            let text = self
                .runs
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .get(key)
                .cloned();
            text.map(|t| from_text(&t)).transpose()
        })
    }

    fn save<'a>(&'a self, key: &'a str, checkpoint: &'a RunCheckpoint) -> JournalFuture<'a, ()> {
        Box::pin(async move {
            let text = to_text(checkpoint)?;
            let mut runs = self
                .runs
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let stored = runs.get(key).map_or(Ok(0), |t| revision_of(t))?;
            if stored.checked_add(1) != Some(checkpoint.revision) {
                return Err(JournalError::Conflict);
            }
            runs.insert(key.to_owned(), text);
            drop(runs);
            Ok(())
        })
    }

    fn clear<'a>(&'a self, key: &'a str) -> JournalFuture<'a, ()> {
        Box::pin(async move {
            self.runs
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .remove(key);
            Ok(())
        })
    }
}
