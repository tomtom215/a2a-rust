// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! A run's checkpoint to and from the body a [`StepJournal`] stores: JSON, or
//! sealed with the loop's journal cipher (adversarial review N07,
//! 2026-11-13).
//!
//! A sealed body is [`SEALED_CHECKPOINT_PREFIX`] then base64 of what the
//! [`ContentCipher`](crate::store::encrypted::ContentCipher) returned, with
//! `["a2a-run-journal/v1", key, revision]` as its associated data: a body
//! moved to another run, or rolled back to another revision of its own, does
//! not open.

use std::sync::Arc;

use super::journal::{JournalError, RunCheckpoint, SEALED_CHECKPOINT_PREFIX, StepJournal};

/// What [`JournalHandle::encode`] and [`JournalHandle::decode`] return.
type Coded<'a, T> =
    std::pin::Pin<Box<dyn std::future::Future<Output = Result<T, JournalError>> + Send + 'a>>;

/// A loop's journal: where checkpoints go, if anywhere, what seals them, and
/// the largest body it writes.
#[derive(Clone)]
pub(super) struct JournalHandle {
    pub(super) store: Option<Arc<dyn StepJournal>>,
    #[cfg(feature = "encryption")]
    pub(super) cipher: Option<Arc<dyn crate::store::encrypted::ContentCipher>>,
    pub(super) max_body: usize,
}

impl Default for JournalHandle {
    fn default() -> Self {
        Self {
            store: None,
            #[cfg(feature = "encryption")]
            cipher: None,
            max_body: super::limits::DEFAULT_CHECKPOINT_LIMIT,
        }
    }
}

impl std::fmt::Debug for JournalHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JournalHandle")
            .field("store", &self.store.is_some())
            .field("sealed", &self.seals())
            .finish_non_exhaustive()
    }
}

// Without the `encryption` feature there is no cipher: `self` holds nothing
// these read and nothing is awaited. The signatures are the feature's.
#[cfg_attr(not(feature = "encryption"), allow(clippy::unused_self))]
impl JournalHandle {
    /// Whether checkpoints are sealed.
    pub(super) const fn seals(&self) -> bool {
        #[cfg(feature = "encryption")]
        {
            self.cipher.is_some()
        }
        #[cfg(not(feature = "encryption"))]
        {
            false
        }
    }

    /// The body to store for `checkpoint` under `key`.
    pub(super) fn encode<'a>(
        &'a self,
        key: &'a str,
        checkpoint: &'a RunCheckpoint,
    ) -> Coded<'a, String> {
        Box::pin(async move {
            let text = super::journal::to_text(checkpoint)?;
            if text.len() > self.max_body {
                return Err(JournalError::Store(format!(
                    "a checkpoint of {} bytes is past the loop's checkpoint limit of {}",
                    text.len(),
                    self.max_body
                )));
            }
            #[cfg(feature = "encryption")]
            if let Some(cipher) = &self.cipher {
                use base64::Engine as _;
                let sealed = cipher
                    .seal(text.into_bytes(), &context(key, checkpoint.revision))
                    .await
                    .map_err(|e| {
                        JournalError::Store(format!("a checkpoint could not be sealed: {e}"))
                    })?;
                return Ok(format!(
                    "{SEALED_CHECKPOINT_PREFIX}{}",
                    base64::engine::general_purpose::STANDARD.encode(sealed)
                ));
            }
            let _ = key;
            Ok(text)
        })
    }

    /// The checkpoint a stored `body` of `revision` under `key` holds.
    ///
    /// A JSON body is read whether or not a cipher is set, so a journal
    /// written before one was set can still be resumed (and is sealed from
    /// its next write on). A sealed body needs the cipher.
    pub(super) fn decode<'a>(
        &'a self,
        key: &'a str,
        revision: u64,
        body: &'a str,
    ) -> Coded<'a, RunCheckpoint> {
        Box::pin(async move {
            let Some(sealed) = body.strip_prefix(SEALED_CHECKPOINT_PREFIX) else {
                return super::journal::from_text(body);
            };
            #[cfg(feature = "encryption")]
            if let Some(cipher) = &self.cipher {
                use base64::Engine as _;
                let bytes = base64::engine::general_purpose::STANDARD
                    .decode(sealed)
                    .map_err(|_| {
                        JournalError::Store("a sealed checkpoint is not base64".to_owned())
                    })?;
                let plain = cipher
                    .open(bytes, &context(key, revision))
                    .await
                    .map_err(|e| {
                        JournalError::Store(format!("a sealed checkpoint did not open: {e}"))
                    })?;
                let text = String::from_utf8(plain).map_err(|_| {
                    JournalError::Store("a sealed checkpoint is not UTF-8".to_owned())
                })?;
                return super::journal::from_text(&text);
            }
            let _ = (key, revision, sealed);
            Err(JournalError::Store(
                "the checkpoint is sealed and this loop has no journal cipher".to_owned(),
            ))
        })
    }
}

/// What a sealed body is bound to.
#[cfg(feature = "encryption")]
fn context(key: &str, revision: u64) -> Vec<u8> {
    serde_json::json!(["a2a-run-journal/v1", key, revision])
        .to_string()
        .into_bytes()
}
