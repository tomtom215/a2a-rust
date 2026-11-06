// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Message parts as the `GenAI` conventions' `gen_ai.{input,output}.messages`
//! parts, within a byte budget that bounds the attribute and the memory held
//! for it (audit NEW-c-4).
//!
//! Until 2026-11-04 the budget was charged only for text, URIs and data: a
//! part's `mime_type`, `filename`, its `type` and the JSON around it were
//! free, and a part that arrived after the budget was spent still added an
//! object (a truncation marker for each). An agent streaming 100,000
//! `append` chunks held 100,000 values for the turn and recorded a
//! multi-megabyte attribute. Now every byte a part adds to the serialised
//! attribute is charged — escapes included — and once the budget is spent a
//! part is counted, not kept: [`Budget::omitted_marker`] says how many.
//!
//! The bound: the parts serialise to at most the budget plus
//! `MARKER_SLACK` (41) bytes, the room a truncated text's own marker may take
//! when the budget left is smaller than it.

use a2a_protocol_types::message::{Part, PartContent};
use serde_json::{Value, json};

/// The most a cut text's `…[truncated N bytes]` marker can run past the
/// budget: the marker itself, with a 20-digit count.
#[cfg(test)]
pub const MARKER_SLACK: usize = "…[truncated 18446744073709551615 bytes]".len();

/// Bytes left for one attribute's parts, and parts left out once they ran
/// out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Budget {
    left: usize,
    omitted: usize,
}

impl Budget {
    /// `limit` bytes for the serialised parts.
    pub const fn new(limit: usize) -> Self {
        Self {
            left: limit,
            omitted: 0,
        }
    }

    /// The part saying how many parts were left out, when any were.
    pub fn omitted_marker(&self) -> Option<Value> {
        (self.omitted > 0)
            .then(|| json!({ "type": "a2a.truncated", "parts_omitted": self.omitted }))
    }

    /// Whether `size` bytes fit, and if so charges them.
    const fn charge(&mut self, size: usize) -> bool {
        if size <= self.left {
            self.left -= size;
            true
        } else {
            false
        }
    }
}

/// What `value` adds to a JSON array: itself and a separating comma.
fn size(value: &Value) -> usize {
    value.to_string().len() + 1
}

/// `parts` as the conventions' message parts, each charged to `budget` for
/// every byte it adds to the serialised attribute. A text or URI that does
/// not fit is cut to what does and marked; data that does not fit is replaced
/// by its length; a part for which even that does not fit, and every part
/// after it, is counted in the budget and left out.
pub fn parts_json(parts: &[Part], budget: &mut Budget) -> Vec<Value> {
    let mut out = Vec::new();
    for part in parts {
        // Once a part has been left out `left` is 0 (it only ever falls), so
        // this also leaves out every part after it.
        if budget.left == 0 {
            budget.omitted += 1;
            continue;
        }
        if let Some(value) = fit(part, budget) {
            out.push(value);
        } else {
            budget.left = 0;
            budget.omitted += 1;
        }
    }
    out
}

/// `part` within what `budget` has left, charged; `None` when nothing of it
/// fits.
fn fit(part: &Part, budget: &mut Budget) -> Option<Value> {
    let full = part_json(part, None);
    if budget.charge(size(&full)) {
        return Some(full);
    }
    let text = match &part.content {
        PartContent::Text(t) => t.as_str(),
        PartContent::Url(u) => u.as_str(),
        // Data is replaced by its length rather than cut: a cut value is not
        // the value. Raw and unknown parts have nothing to cut.
        PartContent::Data(value) => {
            let replaced = json!({
                "type": "a2a.data",
                "truncated": true,
                "length": value.to_string().len(),
            });
            return budget.charge(size(&replaced)).then_some(replaced);
        }
        _ => return None,
    };
    let skeleton = part_json(part, Some(""));
    let overhead = size(&skeleton);
    if !budget.charge(overhead) {
        return None;
    }
    let cut = cut(text, budget.left);
    budget.left = 0;
    Some(part_json(part, Some(&cut)))
}

/// One part, with `content` (the text or URI) replaced when given.
fn part_json(part: &Part, content: Option<&str>) -> Value {
    let media = part.media_type.as_deref();
    match &part.content {
        PartContent::Text(text) => {
            json!({ "type": "text", "content": content.unwrap_or(text) })
        }
        PartContent::Url(url) => json!({
            "type": "uri",
            "uri": content.unwrap_or(url),
            "mime_type": media,
            "modality": modality(media),
        }),
        // Bytes are not content anyone reads in a trace, and an inline file
        // would dwarf every other attribute.
        PartContent::Raw(base64) => json!({
            "type": "a2a.raw",
            "mime_type": media,
            "filename": part.filename,
            "base64_length": base64.len(),
        }),
        PartContent::Data(value) => json!({ "type": "a2a.data", "content": value }),
        _ => json!({ "type": "a2a.unknown" }),
    }
}

/// The bytes `c` takes inside a JSON string, as `serde_json` escapes it.
const fn escaped_len(c: char) -> usize {
    match c {
        '"' | '\\' | '\n' | '\r' | '\t' | '\u{8}' | '\u{c}' => 2,
        c if (c as u32) < 0x20 => 6,
        c => c.len_utf8(),
    }
}

/// The JSON-escaped length of `s`.
fn escaped(s: &str) -> usize {
    s.chars().map(escaped_len).sum()
}

/// As much of `s` as fits in `room` escaped bytes, on a character boundary,
/// with a marker saying how many bytes were cut; the marker too fits in
/// `room` when `room` holds it.
fn cut(s: &str, room: usize) -> String {
    let marker = |dropped: usize| format!("…[truncated {dropped} bytes]");
    // The marker's own length depends on the count; reserve the longest it
    // can be for this text.
    let reserve = escaped(&marker(s.len()));
    let mut used = 0;
    let mut end = 0;
    for (i, c) in s.char_indices() {
        let n = escaped_len(c);
        if used + n + reserve > room {
            break;
        }
        used += n;
        end = i + c.len_utf8();
    }
    format!("{}{}", &s[..end], marker(s.len() - end))
}

/// The conventions' `modality` for a media type: image, video and audio by
/// their top-level type, anything else a document.
pub fn modality(media_type: Option<&str>) -> &'static str {
    match media_type.and_then(|m| m.split('/').next()) {
        Some("image") => "image",
        Some("video") => "video",
        Some("audio") => "audio",
        _ => "document",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The serialised length of `parts` as an array, as an attribute holds it.
    fn serialised(parts: &[Value]) -> usize {
        Value::Array(parts.to_vec()).to_string().len()
    }

    #[test]
    fn every_part_kind_maps_to_a_conventions_part() {
        let parts = vec![
            Part::text("hi"),
            Part::url("https://x/y.png").with_media_type("image/png"),
            Part::raw("AAAA").with_media_type("application/pdf"),
            Part::data(json!({"k": 1})),
        ];
        let mut budget = Budget::new(64 * 1024);
        let got = parts_json(&parts, &mut budget);
        assert_eq!(got[0], json!({"type": "text", "content": "hi"}));
        assert_eq!(
            got[1],
            json!({"type": "uri", "uri": "https://x/y.png", "mime_type": "image/png", "modality": "image"})
        );
        assert_eq!(got[2]["type"], "a2a.raw");
        assert_eq!(got[2]["base64_length"], 4);
        assert!(
            got[2].get("content").is_none(),
            "raw bytes must never be recorded"
        );
        assert_eq!(got[3], json!({"type": "a2a.data", "content": {"k": 1}}));
        assert_eq!(budget.omitted_marker(), None);
    }

    /// Every byte a part adds is charged: its JSON, its media type and file
    /// name, and the comma after it.
    #[test]
    fn a_part_is_charged_for_every_byte_it_serialises_to() {
        let part = Part::raw("AAAA")
            .with_media_type("application/pdf")
            .with_filename("report.pdf");
        let mut budget = Budget::new(1_000);
        let got = parts_json(std::slice::from_ref(&part), &mut budget);
        assert_eq!(budget.left, 1_000 - got[0].to_string().len() - 1);
        assert!(got[0].to_string().contains("report.pdf"));
        let mut budget = Budget::new(1_000);
        let got = parts_json(&[Part::text("a\"b\n")], &mut budget);
        // {"content":"a\"b\n","type":"text"} and a comma.
        assert_eq!(budget.left, 1_000 - got[0].to_string().len() - 1);
    }

    /// Once the budget is spent, parts are counted, not kept: 100,000 chunks
    /// hold what fits and one count (audit NEW-c-4).
    #[test]
    fn parts_past_the_budget_are_counted_not_kept() {
        let limit = 64 * 1024;
        let mut budget = Budget::new(limit);
        let mut kept = Vec::new();
        for i in 0..100_000 {
            kept.extend(parts_json(
                &[Part::text(format!("chunk {i} "))],
                &mut budget,
            ));
        }
        let marker = budget.omitted_marker().expect("parts were left out");
        let omitted = marker["parts_omitted"].as_u64().unwrap();
        assert_eq!(kept.len() as u64 + omitted, 100_000);
        assert!(kept.len() < 10_000, "{}", kept.len());
        assert!(
            serialised(&kept) <= limit + MARKER_SLACK,
            "{}",
            serialised(&kept)
        );
        assert_eq!(marker["type"], "a2a.truncated");
    }

    /// The attribute stays within the budget plus the marker's slack whatever
    /// the parts are: escapes, multi-byte characters, file names, data.
    #[test]
    fn the_serialised_parts_never_exceed_the_budget_and_the_slack() {
        let texts = [
            "plain ascii text that goes on",
            "quotes \" and backslashes \\ and newlines \n\n\n",
            "控制 \u{1} \u{2} bytes that escape to six",
            "héllo wörld ✓✓✓",
        ];
        for limit in [0, 1, 7, 30, 64, 65, 100, 257, 1_000] {
            for text in texts {
                let parts = vec![
                    Part::text(text.repeat(20)),
                    Part::url(format!("https://x/{}", text.repeat(5))),
                    Part::raw("AAAA").with_filename(text.repeat(3)),
                    Part::data(json!({ "v": text.repeat(4) })),
                    Part::text(text),
                ];
                let mut budget = Budget::new(limit);
                let got = parts_json(&parts, &mut budget);
                assert!(
                    serialised(&got) <= limit + MARKER_SLACK + 1,
                    "limit {limit}, {text:?}: {} bytes",
                    serialised(&got)
                );
                assert_eq!(
                    got.len() + budget.omitted,
                    parts.len(),
                    "every part is kept or counted"
                );
            }
        }
    }

    #[test]
    fn text_past_the_budget_is_cut_on_a_char_boundary_and_marked() {
        // "héllo wörld" is 13 bytes; the marker for 13 is 23 bytes long
        // ("…" is three), and is reserved; five bytes are left for the text.
        let cut_text = cut("héllo wörld", 23 + 5);
        assert_eq!(cut_text, "héll…[truncated 8 bytes]");
        // Room for the marker only: nothing of the text, and the marker.
        assert_eq!(cut("rest", 0), "…[truncated 4 bytes]");
        // A budget ending inside a character cuts before it, not after.
        assert_eq!(cut("aé", 22 + 2), "a…[truncated 2 bytes]");
        // Escapes are charged at their escaped length.
        assert_eq!(cut("\"\"\"\"", 22 + 4), "\"\"…[truncated 2 bytes]");
    }

    #[test]
    fn escaped_lengths_match_serde_json() {
        for c in [
            'a', '"', '\\', '\n', '\r', '\t', '\u{8}', '\u{c}', '\u{1}', '\u{1f}', ' ', 'é', '✓',
            '😀', '\u{7f}',
        ] {
            let s = c.to_string();
            assert_eq!(
                escaped_len(c),
                serde_json::to_string(&s).unwrap().len() - 2,
                "{c:?}"
            );
        }
    }

    /// What fits is charged exactly, and a text that does not is cut so the
    /// part, marker and all, takes what was left.
    #[test]
    fn a_text_that_does_not_fit_is_cut_to_what_is_left() {
        let whole = json!({"type": "text", "content": "abcdefghij"})
            .to_string()
            .len()
            + 1;
        let mut budget = Budget::new(whole);
        let got = parts_json(&[Part::text("abcdefghij")], &mut budget);
        assert_eq!(got[0]["content"], "abcdefghij");
        assert_eq!(budget.left, 0);
        let skeleton = json!({"type": "text", "content": ""}).to_string().len() + 1;
        let long = "abcdefghij".repeat(10);
        let marker = "…[truncated 100 bytes]".len();
        let mut budget = Budget::new(skeleton + marker + 3);
        let got = parts_json(&[Part::text(long), Part::text("next")], &mut budget);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0]["content"], "abc…[truncated 97 bytes]");
        assert_eq!(budget.omitted, 1, "the part after a cut one is left out");
    }

    /// A URI is the other part with something to cut, and is cut as a text is.
    #[test]
    fn a_uri_that_does_not_fit_is_cut_to_what_is_left() {
        let part = Part::url(format!("https://{}", "u".repeat(92)));
        let skeleton = size(&part_json(&part, Some("")));
        let marker = "…[truncated 100 bytes]".len();
        let mut budget = Budget::new(skeleton + marker + 3);
        let got = parts_json(std::slice::from_ref(&part), &mut budget);
        assert_eq!(got.len(), 1, "the URI is cut, not left out");
        assert_eq!(got[0]["type"], "uri");
        assert_eq!(got[0]["uri"], "htt…[truncated 97 bytes]");
        assert_eq!(budget.left, 0);
    }

    #[test]
    fn data_that_does_not_fit_is_replaced_not_cut() {
        let value = json!({ "long": "value".repeat(20) });
        let length = value.to_string().len();
        assert_eq!(length, 111);
        let replaced = json!({"type": "a2a.data", "truncated": true, "length": length});
        let mut budget = Budget::new(size(&replaced));
        let got = parts_json(&[Part::data(value.clone())], &mut budget);
        assert_eq!(got, [replaced]);
        assert_eq!(budget.left, 0);
        // Not even the replacement fits: left out and counted.
        let mut budget = Budget::new(10);
        assert_eq!(
            parts_json(&[Part::data(value)], &mut budget),
            Vec::<Value>::new()
        );
        assert_eq!(budget.omitted, 1);
        assert_eq!(budget.left, 0);
    }

    #[test]
    fn a_raw_part_that_does_not_fit_is_left_out() {
        let mut budget = Budget::new(10);
        assert_eq!(
            parts_json(&[Part::raw("AAAA")], &mut budget),
            Vec::<Value>::new()
        );
        assert_eq!(
            budget.omitted_marker(),
            Some(json!({"type": "a2a.truncated", "parts_omitted": 1}))
        );
    }

    #[test]
    fn modality_follows_the_media_type() {
        assert_eq!(modality(Some("image/png")), "image");
        assert_eq!(modality(Some("audio/ogg")), "audio");
        assert_eq!(modality(Some("video/mp4")), "video");
        assert_eq!(modality(Some("text/plain")), "document");
        assert_eq!(modality(None), "document");
    }
}
