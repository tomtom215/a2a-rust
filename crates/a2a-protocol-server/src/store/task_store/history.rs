// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! How much of a conversation a task keeps: [`HistoryLimit`].
//!
//! One type and one [`trim`](HistoryLimit::trim), used by every place a
//! history grows — a send's turn appended by the store, and an agent's
//! messages appended by the event processors. There were four copies of a
//! count-only trim, and none bounded bytes (release audit F2).

use a2a_protocol_types::message::Message;

/// The most a task's `history` keeps: a number of messages and a number of
/// bytes, whichever is reached first. The oldest messages go first.
///
/// # Why bytes as well as messages
///
/// A count alone bounds nothing in bytes: at the default 4 MiB request body,
/// 1,024 messages is 4 GiB of one task's history, all of it from remote
/// input. Bytes are each message's serialized JSON length, the form it is
/// sent and stored in.
///
/// # The newest message is always kept
///
/// A turn is never stored without the message that made it, so a message
/// larger than `max_bytes` on its own is kept and everything older goes. The
/// request body limit bounds that message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct HistoryLimit {
    /// At most this many messages.
    pub max_messages: usize,
    /// At most this many bytes of serialized messages; the newest message is
    /// kept even when it alone is larger.
    pub max_bytes: usize,
}

impl HistoryLimit {
    /// A limit of `max_messages` messages and `max_bytes` bytes.
    #[must_use]
    pub const fn new(max_messages: usize, max_bytes: usize) -> Self {
        Self {
            max_messages,
            max_bytes,
        }
    }

    /// Drops the oldest messages of `history` until it is within this limit,
    /// keeping the newest message whatever its size.
    ///
    /// A cheap upper bound on the history's size is taken first, from string
    /// lengths and structure without serializing anything; when it is within
    /// `max_bytes` nothing can need trimming and nothing is measured. Only
    /// otherwise are sizes measured exactly, from the newest message
    /// backwards, stopping at the first that does not fit, so that cost is
    /// bounded by `max_bytes` (plus one message) however long the history
    /// was.
    ///
    /// Measured 2026-10-18 (release build, 4-vCPU Xeon at 2.8 GHz, 60 runs
    /// each, a timing harness not kept in the tree): 1,024 messages of 100
    /// bytes, median 10.2 µs (p90 16.6) with the bound against 108 µs (p90
    /// 149) for the exact walk alone, which every turn of a long conversation
    /// would otherwise pay; a history of sixteen 1 MiB messages, which the
    /// bound cannot clear, median 11.0 ms either way.
    pub fn trim(self, history: &mut Vec<Message>) {
        let _dropped = self.trim_returning(history);
    }

    /// [`trim`](Self::trim), returning the messages it dropped, oldest
    /// first, so a caller whose write then fails can put the history back
    /// exactly as it was. Allocates only when something is dropped.
    pub(crate) fn trim_returning(self, history: &mut Vec<Message>) -> Vec<Message> {
        // `drain(..0)` skips its memmove, so this is O(1) under the count.
        let excess = history.len().saturating_sub(self.max_messages);
        let mut dropped: Vec<Message> = history.drain(..excess).collect();
        let bound = history
            .iter()
            .map(upper_bound_len)
            .fold(0usize, usize::saturating_add);
        if bound <= self.max_bytes {
            return dropped;
        }
        let mut total = 0usize;
        let mut keep_from = history.len();
        for (i, message) in history.iter().enumerate().rev() {
            let size = serialized_len(message);
            let newest = i + 1 == history.len();
            if !newest && total.saturating_add(size) > self.max_bytes {
                break;
            }
            total = total.saturating_add(size);
            keep_from = i;
        }
        dropped.extend(history.drain(..keep_from));
        dropped
    }
}

/// Room for a message's member names and punctuation: `{"messageId":`,
/// `"contextId":`, `"taskId":`, `"role":"ROLE_UNSPECIFIED"`, `"parts":[]`,
/// `"metadata":`, `"extensions":[]` and `"referenceTaskIds":[]` come to 131
/// bytes; this leaves room to spare.
const MESSAGE_OVERHEAD: usize = 256;

/// Room for a part's member names and punctuation: `{"text":` (or `raw`,
/// `url`, `data`), `"metadata":`, `"filename":`, `"mediaType":` and the
/// separators come to 47 bytes.
const PART_OVERHEAD: usize = 96;

/// An upper bound on `message`'s serialized JSON length, from its structure
/// and string lengths alone. Never below the exact length, which is what
/// lets [`HistoryLimit::trim`] skip measuring: a JSON string is at most six
/// bytes per byte of content (`\u001f`) plus its quotes; numbers are
/// measured exactly; everything else is a fixed allowance per member.
fn upper_bound_len(message: &Message) -> usize {
    let mut n = MESSAGE_OVERHEAD
        .saturating_add(str_bound(&message.id.0))
        .saturating_add(message.context_id.as_ref().map_or(0, |c| str_bound(&c.0)))
        .saturating_add(message.task_id.as_ref().map_or(0, |t| str_bound(&t.0)))
        .saturating_add(message.metadata.as_ref().map_or(0, value_bound));
    for uri in message.extensions.iter().flatten() {
        n = n.saturating_add(str_bound(uri)).saturating_add(1);
    }
    for id in message.reference_task_ids.iter().flatten() {
        n = n.saturating_add(str_bound(&id.0)).saturating_add(1);
    }
    for part in &message.parts {
        n = n.saturating_add(part_bound(part)).saturating_add(1);
    }
    n
}

fn part_bound(part: &a2a_protocol_types::message::Part) -> usize {
    use a2a_protocol_types::message::PartContent;
    let content = match &part.content {
        PartContent::Text(s) | PartContent::Raw(s) | PartContent::Url(s) => str_bound(s),
        PartContent::Data(v) => value_bound(v),
        // A variant added later is measured exactly rather than guessed at.
        other => serde_json::to_vec(other).map_or(usize::MAX, |v| v.len()),
    };
    PART_OVERHEAD
        .saturating_add(content)
        .saturating_add(part.metadata.as_ref().map_or(0, value_bound))
        .saturating_add(part.filename.as_deref().map_or(0, str_bound))
        .saturating_add(part.media_type.as_deref().map_or(0, str_bound))
}

const fn str_bound(s: &str) -> usize {
    s.len().saturating_mul(6).saturating_add(2)
}

fn value_bound(value: &serde_json::Value) -> usize {
    use serde_json::Value;
    match value {
        Value::Null | Value::Bool(_) => 5,
        Value::Number(n) => n.to_string().len(),
        Value::String(s) => str_bound(s),
        Value::Array(items) => items
            .iter()
            .map(|v| value_bound(v).saturating_add(1))
            .fold(2, usize::saturating_add),
        Value::Object(map) => map
            .iter()
            .map(|(k, v)| {
                str_bound(k)
                    .saturating_add(value_bound(v))
                    .saturating_add(2)
            })
            .fold(2, usize::saturating_add),
    }
}

/// `message`'s serialized JSON length, counted without allocating it. A
/// message that cannot be serialized counts as too large to keep, so it is
/// dropped rather than silently costing nothing.
fn serialized_len(message: &Message) -> usize {
    struct Count(usize);
    impl std::io::Write for Count {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0 += buf.len();
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut count = Count(0);
    match serde_json::to_writer(&mut count, message) {
        Ok(()) => count.0,
        Err(_) => usize::MAX,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message(id: &str, text_len: usize) -> Message {
        Message::user_text(id, "x".repeat(text_len))
    }

    fn ids(history: &[Message]) -> Vec<&str> {
        history.iter().map(|m| m.id.0.as_str()).collect()
    }

    #[test]
    fn the_count_drops_the_oldest() {
        let mut h: Vec<_> = (0..5).map(|i| message(&format!("m{i}"), 1)).collect();
        HistoryLimit::new(3, usize::MAX).trim(&mut h);
        assert_eq!(ids(&h), ["m2", "m3", "m4"]);
    }

    /// Exactly at the byte bound everything stays; one byte under, the
    /// oldest goes.
    #[test]
    fn the_bytes_drop_the_oldest_at_the_exact_bound() {
        let h: Vec<_> = (0..3).map(|i| message(&format!("m{i}"), 100)).collect();
        let each = serialized_len(&h[0]);
        assert!(h.iter().all(|m| serialized_len(m) == each));

        let mut at = h.clone();
        HistoryLimit::new(10, 3 * each).trim(&mut at);
        assert_eq!(ids(&at), ["m0", "m1", "m2"]);

        let mut under = h;
        HistoryLimit::new(10, 3 * each - 1).trim(&mut under);
        assert_eq!(ids(&under), ["m1", "m2"]);
    }

    /// A newest message larger than the whole bound is kept, alone.
    #[test]
    fn the_newest_message_is_kept_whatever_its_size() {
        let mut h = vec![message("old", 10), message("huge", 10_000)];
        HistoryLimit::new(10, 100).trim(&mut h);
        assert_eq!(ids(&h), ["huge"]);
    }

    /// A message that does not fit stops the walk: an older, smaller one is
    /// not kept around a gap.
    #[test]
    fn no_gap_is_left_behind_a_message_that_did_not_fit() {
        let mut h = vec![
            message("small-old", 1),
            message("big", 500),
            message("new", 1),
        ];
        let budget = serialized_len(&h[2]) + serialized_len(&h[0]) + 10;
        HistoryLimit::new(10, budget).trim(&mut h);
        assert_eq!(ids(&h), ["new"]);
    }

    /// What `trim_returning` hands back, put in front of what it kept, is
    /// the history it was given.
    #[test]
    fn trim_returning_hands_back_exactly_what_it_dropped() {
        let original: Vec<_> = (0..6).map(|i| message(&format!("m{i}"), 50)).collect();
        let each = serialized_len(&original[0]);
        let mut kept = original.clone();
        let dropped = HistoryLimit::new(5, 2 * each).trim_returning(&mut kept);
        assert_eq!(ids(&kept), ["m4", "m5"]);
        assert_eq!(ids(&dropped), ["m0", "m1", "m2", "m3"]);
        let mut restored = dropped;
        restored.extend(kept);
        assert_eq!(restored, original);
    }

    #[test]
    fn an_empty_history_stays_empty() {
        let mut h = Vec::new();
        HistoryLimit::new(1, 1).trim(&mut h);
        assert_eq!(h, Vec::<Message>::new());
    }

    /// A seeded xorshift generator: the cases are random in shape and the
    /// same on every run.
    struct Gen(u64);

    impl Gen {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }

        fn below(&mut self, n: u64) -> usize {
            usize::try_from(self.next() % n).expect("small")
        }

        fn coin(&mut self) -> bool {
            self.next().is_multiple_of(2)
        }

        /// Up to 24 characters from an alphabet of JSON's costliest escapes
        /// (control characters, quotes, backslashes) and multi-byte text.
        fn string(&mut self) -> String {
            let alphabet: Vec<char> = "a\"\\\u{1}\u{1f}\n\t/é😀\u{7f}<>&'".chars().collect();
            (0..self.below(24))
                .map(|_| alphabet[self.below(alphabet.len() as u64)])
                .collect()
        }

        fn value(&mut self, depth: u32) -> serde_json::Value {
            use serde_json::{Value, json};
            match self.below(if depth > 2 { 6 } else { 8 }) {
                0 => Value::Null,
                1 => Value::Bool(self.coin()),
                2 => json!(self.next().cast_signed()),
                3 => json!(f64::from_bits(self.next() >> 2).clamp(-1e300, 1e300)),
                4 => json!(self.next()),
                5 => Value::String(self.string()),
                6 => Value::Array((0..self.below(4)).map(|_| self.value(depth + 1)).collect()),
                _ => Value::Object(
                    (0..self.below(4))
                        .map(|_| (self.string(), self.value(depth + 1)))
                        .collect(),
                ),
            }
        }

        fn part(&mut self) -> a2a_protocol_types::message::Part {
            use a2a_protocol_types::message::{Part, PartContent};
            let mut part = Part::text("");
            part.content = match self.below(4) {
                0 => PartContent::Text(self.string()),
                1 => PartContent::Raw(self.string()),
                2 => PartContent::Url(self.string()),
                _ => PartContent::Data(self.value(0)),
            };
            part.metadata = self.coin().then(|| self.value(0));
            part.filename = self.coin().then(|| self.string());
            part.media_type = self.coin().then(|| self.string());
            part
        }

        fn message(&mut self) -> Message {
            use a2a_protocol_types::message::{MessageId, MessageRole};
            use a2a_protocol_types::task::{ContextId, TaskId};
            Message {
                id: MessageId::new(self.string()),
                role: if self.coin() {
                    MessageRole::User
                } else {
                    MessageRole::Agent
                },
                parts: (0..=self.below(3)).map(|_| self.part()).collect(),
                context_id: self.coin().then(|| ContextId::new(self.string())),
                task_id: self.coin().then(|| TaskId::new(self.string())),
                reference_task_ids: self.coin().then(|| {
                    (0..self.below(3))
                        .map(|_| TaskId::new(self.string()))
                        .collect()
                }),
                extensions: self
                    .coin()
                    .then(|| (0..self.below(3)).map(|_| self.string()).collect()),
                metadata: self.coin().then(|| self.value(0)),
            }
        }
    }

    /// The fast path is sound only if the bound never falls below the
    /// exact length. Checked over 5,000 generated messages that include the
    /// worst escapes JSON has (control characters, quotes, backslashes),
    /// multi-byte text, nested data, numbers of every kind and every
    /// optional member, from a fixed seed.
    #[test]
    fn the_upper_bound_is_never_below_the_exact_length() {
        let mut generate = Gen(0x9E37_79B9_7F4A_7C15);
        for _ in 0..5_000 {
            let message = generate.message();
            let exact = serde_json::to_vec(&message).unwrap().len();
            assert_eq!(serialized_len(&message), exact);
            assert!(
                upper_bound_len(&message) >= exact,
                "bound {} below exact {exact} for {message:?}",
                upper_bound_len(&message)
            );
        }
    }

    /// Long runs of the costliest escape in every string a message carries,
    /// where the per-byte factor rather than the fixed allowances decides:
    /// `\u{1}` serializes as six bytes, so a bound of fewer than six per
    /// byte falls below the exact length here.
    #[test]
    fn the_upper_bound_holds_for_worst_case_escapes() {
        use a2a_protocol_types::message::{MessageId, MessageRole, Part, PartContent};
        use a2a_protocol_types::task::{ContextId, TaskId};
        let worst = "\u{1}".repeat(4_000);
        let mut data_part = Part::text("");
        data_part.content = PartContent::Data(serde_json::json!({ &worst: [&worst] }));
        data_part.metadata = Some(serde_json::json!(worst));
        data_part.filename = Some(worst.clone());
        data_part.media_type = Some(worst.clone());
        let cases = [
            Message::user_text(worst.clone(), worst.clone()),
            Message {
                id: MessageId::new("m"),
                role: MessageRole::Agent,
                parts: vec![data_part],
                context_id: Some(ContextId::new(worst.clone())),
                task_id: Some(TaskId::new(worst.clone())),
                reference_task_ids: Some(vec![TaskId::new(worst.clone())]),
                extensions: Some(vec![worst.clone()]),
                metadata: Some(serde_json::json!({ "k": worst })),
            },
        ];
        for message in &cases {
            let exact = serde_json::to_vec(message).unwrap().len();
            assert!(
                upper_bound_len(message) >= exact,
                "{} < {exact}",
                upper_bound_len(message)
            );
        }
    }

    /// The fast path skips nothing it should not: a history whose bound is
    /// over budget but whose exact size is within it is kept whole, so the
    /// exact walk still decides.
    #[test]
    fn a_history_within_budget_only_exactly_is_kept_whole() {
        let h: Vec<_> = (0..4).map(|i| message(&format!("m{i}"), 100)).collect();
        let exact: usize = h.iter().map(serialized_len).sum();
        let bound: usize = h.iter().map(upper_bound_len).sum();
        assert!(bound > exact);
        let mut kept = h.clone();
        HistoryLimit::new(10, exact).trim(&mut kept);
        assert_eq!(kept, h);
    }

    #[test]
    fn serialized_len_is_the_json_length() {
        let m = message("m", 7);
        assert_eq!(serialized_len(&m), serde_json::to_vec(&m).unwrap().len());
    }
}
