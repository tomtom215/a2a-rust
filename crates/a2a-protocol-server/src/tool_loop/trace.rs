// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The tool trace a run keeps: one line per call, each of them bounded, and
//! where the lines go when the run fails.

use std::sync::{Mutex, PoisonError};

/// The most bytes of one trace line: name, arguments and result together.
///
/// The result goes whole to the model, which a context window bounds; the
/// trace is a record for the caller, and with no bound a large result made
/// every `GetTask` body as large and, past the 16 MiB event limit, failed a run
/// that had finished (hostile campaign, wave 2, W2-LIVE-4). A longer line is
/// cut on a character boundary and says how much it lost.
pub const TRACE_LINE_LIMIT: usize = 16 * 1024;

/// `name(arguments) -> content`, cut to [`TRACE_LINE_LIMIT`] bytes.
pub(super) fn line(name: &str, arguments: &str, content: &str) -> String {
    let full = format!("{name}({arguments}) -> {content}");
    if full.len() <= TRACE_LINE_LIMIT {
        return full;
    }
    let mut end = TRACE_LINE_LIMIT;
    while !full.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}… [{} more bytes]", &full[..end], full.len() - end)
}

/// Where a run puts its trace lines as they are made. An `Err` carries no
/// [`Answer`](super::Answer) and so no trace, though the tools it ran have run:
/// the executor reads this when a run fails and sends the trace before the
/// failure, so the caller can see what was done (and does not repeat it).
#[derive(Default)]
pub(super) struct Sink(Mutex<Vec<String>>);

impl Sink {
    pub(super) fn push(&self, line: String) {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(line);
    }

    pub(super) fn take(&self) -> Vec<String> {
        std::mem::take(&mut *self.0.lock().unwrap_or_else(PoisonError::into_inner))
    }
}

/// Keeps `line` in the run's trace and, when the executor is watching, in its
/// sink too.
pub(super) fn record(trace: &mut Vec<String>, sink: Option<&Sink>, line: String) {
    if let Some(sink) = sink {
        sink.push(line.clone());
    }
    trace.push(line);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A line within the limit is as it always was.
    #[test]
    fn a_short_line_is_name_arguments_and_result() {
        assert_eq!(line("t", "{}", "ok"), "t({}) -> ok");
        // Exactly at the limit: kept whole.
        let fits = "x".repeat(TRACE_LINE_LIMIT - "t({}) -> ".len());
        let whole = line("t", "{}", &fits);
        assert_eq!(whole.len(), TRACE_LINE_LIMIT);
        assert!(!whole.contains("more bytes"));
    }

    /// One byte over: cut, and the marker counts what was lost.
    #[test]
    fn a_line_over_the_limit_is_cut_and_says_how_much() {
        let over = "x".repeat(TRACE_LINE_LIMIT - "t({}) -> ".len() + 1);
        let cut = line("t", "{}", &over);
        let kept = format!("t({{}}) -> {}", &over[..over.len() - 1]);
        assert_eq!(cut, format!("{kept}… [1 more bytes]"));
        let huge = line("t", "{}", &"y".repeat(8 * 1024 * 1024));
        assert!(huge.len() < TRACE_LINE_LIMIT + 32, "{}", huge.len());
        assert!(huge.ends_with("more bytes]"));
    }

    /// The cut falls on a character boundary: a three-byte character that
    /// straddles the limit is left out whole, and counted as lost.
    #[test]
    fn the_cut_never_splits_a_character() {
        let prefix = "t({}) -> ";
        // Fill to one byte short of the limit, then a three-byte character.
        let content = format!("{}€€", "x".repeat(TRACE_LINE_LIMIT - prefix.len() - 1));
        let cut = line("t", "{}", &content);
        assert!(cut.is_char_boundary(cut.len()));
        assert_eq!(
            cut,
            format!(
                "{prefix}{}… [6 more bytes]",
                "x".repeat(TRACE_LINE_LIMIT - prefix.len() - 1)
            )
        );
    }

    /// The sink hands back what was recorded, once.
    #[test]
    fn the_sink_keeps_lines_in_order_and_gives_them_up_once() {
        let sink = Sink::default();
        let mut trace = Vec::new();
        record(&mut trace, Some(&sink), "a".to_owned());
        record(&mut trace, Some(&sink), "b".to_owned());
        record(&mut trace, None, "c".to_owned());
        assert_eq!(trace, ["a", "b", "c"]);
        assert_eq!(sink.take(), ["a", "b"]);
        assert_eq!(sink.take(), Vec::<String>::new());
    }
}
