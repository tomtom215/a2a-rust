// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Who stopped a canceled task, as something a caller can `match` on (COR-04,
//! E03 R3).
//!
//! A task reaches [`TaskState::Canceled`](crate::task::TaskState::Canceled)
//! for three different reasons, and a parent decides differently on each:
//!
//! | Cause | What happened | What a parent may do |
//! |---|---|---|
//! | [`CancelRequested`](CancelCause::CancelRequested) | A caller sent `CancelTask` | Nothing: someone meant it |
//! | [`Halted`](CancelCause::Halted) | An operator halted the server, the tenant or the job | Stop; do not start the work elsewhere |
//! | [`LeaseLapsed`](CancelCause::LeaseLapsed) | The parent stopped renewing the child's lease | Find out why the renewals stopped |
//!
//! # Wire shape
//!
//! Not part of A2A v1.0, so it ships as the extension
//! `https://a2a-rust.com/extensions/cancel/v1`, in the shape
//! [`failure`](crate::failure) uses: the cause travels in the canceled
//! status message's `metadata` under `a2a-rust.com/cancel`, and the message
//! names the URI in `extensions`. The server writes it; it does not declare
//! the extension on its agent card, since a signed card that lacked the entry
//! would then be refused at build. A reader that does not know the key reads
//! the canceled state as before.
//!
//! A cause this version does not know reads as no cause: unlike a failure
//! class, there is no catch-all a parent could act on, and a guessed cause
//! would be worse than none. No cause also means the peer wrote none (another
//! SDK, an older server, or an executor that canceled its own task).
//!
//! ```rust
//! use a2a_protocol_types::cancel_cause::{CancelCause, cause_of, set_cause};
//! use a2a_protocol_types::message::Message;
//!
//! let mut status = Message::agent_text("m1", "halted by oncall");
//! set_cause(&mut status, CancelCause::Halted);
//! assert_eq!(cause_of(&status), Some(CancelCause::Halted));
//! ```

use crate::message::Message;

/// The extension URI a message names in `extensions`.
pub const CANCEL_EXTENSION_URI: &str = "https://a2a-rust.com/extensions/cancel/v1";

/// The `Message.metadata` key the cause travels under.
pub const CANCEL_METADATA_KEY: &str = "a2a-rust.com/cancel";

/// Who stopped a canceled task.
///
/// `#[non_exhaustive]`: a later version may tell more causes apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum CancelCause {
    /// A caller asked for it with `CancelTask`.
    CancelRequested,
    /// An operator halted the server, the task's tenant or its job.
    Halted,
    /// The task's lease was not renewed within its time to live.
    LeaseLapsed,
}

impl CancelCause {
    /// Every cause, for exhaustiveness in tests and for rendering a table.
    pub const ALL: &'static [Self] = &[Self::CancelRequested, Self::Halted, Self::LeaseLapsed];

    /// The token this cause is written as on the wire.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::CancelRequested => "cancel-requested",
            Self::Halted => "halted",
            Self::LeaseLapsed => "lease-lapsed",
        }
    }

    /// The cause `token` names, or `None` for a token this version does not
    /// know.
    #[must_use]
    pub fn from_wire(token: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|c| c.as_str() == token)
    }
}

impl std::fmt::Display for CancelCause {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Records the cause on a status message, naming the extension URI.
///
/// Replaces a non-object `metadata` wholesale, as
/// [`failure::set_class`](crate::failure::set_class) does.
pub fn set_cause(message: &mut Message, cause: CancelCause) {
    let metadata = message
        .metadata
        .get_or_insert_with(|| serde_json::Value::Object(serde_json::Map::new()));
    if !metadata.is_object() {
        *metadata = serde_json::Value::Object(serde_json::Map::new());
    }
    if let Some(map) = metadata.as_object_mut() {
        map.insert(
            CANCEL_METADATA_KEY.to_owned(),
            serde_json::Value::String(cause.as_str().to_owned()),
        );
    }
    let extensions = message.extensions.get_or_insert_with(Vec::new);
    if !extensions.iter().any(|u| u == CANCEL_EXTENSION_URI) {
        extensions.push(CANCEL_EXTENSION_URI.to_owned());
    }
}

/// Reads the cause off a status message: `None` when none was written, or
/// the one written is not a cause this version knows.
#[must_use]
pub fn cause_of(message: &Message) -> Option<CancelCause> {
    let token = message
        .metadata
        .as_ref()?
        .get(CANCEL_METADATA_KEY)?
        .as_str()?;
    CancelCause::from_wire(token)
}

#[cfg(test)]
#[path = "cancel_cause_tests.rs"]
mod tests;
