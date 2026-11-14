// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! How many bytes a run may hold (adversarial review N09, 2026-11-13).
//!
//! The loop bounded turns and calls per turn, and the model adapter bounded a
//! provider's response, but nothing bounded what a tool returned, what the
//! conversation grew to, or the checkpoint a journal wrote of it. One tool
//! returning a large report was kept whole in the conversation (and, for a
//! tool that is not read-only, in the record of answered calls), sent again on
//! every later model turn, and written again in every checkpoint.
//!
//! * **A tool's result** past [`DEFAULT_RESULT_LIMIT`]
//!   ([`ToolLoop::with_result_limit`](super::ToolLoop::with_result_limit)) is
//!   cut, at a character boundary, and says so in words the model reads: how
//!   many bytes there were and how many it is shown. The call's receipt is
//!   unaffected (it holds a digest of the arguments, never the result).
//! * **A model request** (preamble, conversation, tool catalogue) past
//!   [`DEFAULT_REQUEST_LIMIT`]
//!   ([`with_request_limit`](super::ToolLoop::with_request_limit)) is not
//!   sent: the run ends with [`LoopError::TooLarge`](super::LoopError::TooLarge).
//! * **A checkpoint** past [`DEFAULT_CHECKPOINT_LIMIT`]
//!   ([`with_checkpoint_limit`](super::ToolLoop::with_checkpoint_limit)) is not
//!   written: the run stops before the step it would have recorded, as for any
//!   checkpoint the journal cannot write.

use std::fmt::Write as _;

/// The bytes of one tool result the model is shown: 1 MiB.
pub const DEFAULT_RESULT_LIMIT: usize = 1024 * 1024;

/// The bytes one model request may hold: 8 MiB, the size the bundled adapter
/// accepts back from a provider
/// ([`DEFAULT_MAX_RESPONSE_BYTES`](super::DEFAULT_MAX_RESPONSE_BYTES)).
pub const DEFAULT_REQUEST_LIMIT: usize = 8 * 1024 * 1024;

/// The bytes of one checkpoint body: 32 MiB.
pub const DEFAULT_CHECKPOINT_LIMIT: usize = 32 * 1024 * 1024;

/// A loop's byte limits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct ByteLimits {
    pub(super) result: usize,
    pub(super) request: usize,
}

impl Default for ByteLimits {
    fn default() -> Self {
        Self {
            result: DEFAULT_RESULT_LIMIT,
            request: DEFAULT_REQUEST_LIMIT,
        }
    }
}

/// `content`, cut to `limit` bytes at a character boundary with a note of
/// what was cut, when it is longer.
pub(super) fn bound_result(content: String, limit: usize) -> String {
    if content.len() <= limit {
        return content;
    }
    let mut end = limit;
    while !content.is_char_boundary(end) {
        end -= 1;
    }
    let total = content.len();
    let mut out = content;
    out.truncate(end);
    let _ = write!(
        out,
        "\n[the tool's result was {total} bytes; only the first {end} are shown here]"
    );
    out
}

/// The bytes a model request holds: the preamble, the conversation and the
/// tool catalogue, as JSON.
pub(super) fn request_bytes(
    preamble: &str,
    messages: &[super::model::ChatMessage],
    specs: &[super::model::ToolSpec],
) -> usize {
    let conversation = serde_json::to_vec(messages).map_or(usize::MAX, |b| b.len());
    let catalogue: usize = specs
        .iter()
        .map(|s| s.name.len() + s.description.len() + s.parameters.to_string().len())
        .sum();
    preamble
        .len()
        .saturating_add(conversation)
        .saturating_add(catalogue)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_result_within_the_limit_is_unchanged() {
        assert_eq!(bound_result("abc".to_owned(), 3), "abc");
    }

    #[test]
    fn a_result_past_the_limit_is_cut_and_says_so() {
        let out = bound_result("abcdef".to_owned(), 4);
        assert_eq!(
            out,
            "abcd\n[the tool's result was 6 bytes; only the first 4 are shown here]"
        );
    }

    #[test]
    fn a_result_is_cut_at_a_character_boundary() {
        // "é" is two bytes: a cut at 2 would split it.
        let out = bound_result("aé…".to_owned(), 2);
        assert!(out.starts_with("a\n"), "{out}");
        assert!(out.contains("was 6 bytes; only the first 1"), "{out}");
    }
}
