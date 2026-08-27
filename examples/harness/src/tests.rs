// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Tests for the coverage matrix and the exit code it implies.
//!
//! In a file of their own because `lib.rs` crossed the repository's 500-line
//! limit when they were added to it, and because the thing under test is the
//! scorer that decides whether an example may claim it covered everything —
//! it deserves more than a footnote at the bottom of the module it grades.

use super::{Binding, Excuse, Matrix, SurfaceOutcome};
use a2a_protocol_client::ClientError;
use a2a_protocol_types::method::Method;
use a2a_protocol_types::task::TaskId;
use std::collections::BTreeSet;

/// An empty matrix must report every cell missing. If it reported zero
/// missing, a demo that made no calls at all would exit 0 — the exact
/// failure this module exists to prevent.
#[test]
fn an_empty_matrix_is_not_complete() {
    let m = Matrix::new();
    let missing = m.report();
    assert_eq!(missing.len(), Method::ALL.len() * Binding::ALL.len());
}

#[test]
fn recorded_cells_stop_being_missing() {
    let mut m = Matrix::new();
    m.record(Method::GetTask, Binding::JsonRpc);
    let missing = m.report();
    assert!(!missing.contains(&(Method::GetTask, Binding::JsonRpc)));
    assert!(missing.contains(&(Method::GetTask, Binding::Grpc)));
}

/// An excuse must remove the cell from `missing` *and* stay visible. A
/// silent excuse is indistinguishable from coverage.
#[test]
fn excused_cells_are_not_missing_but_are_still_listed() {
    let mut m = Matrix::new();
    m.excuse(
        Method::SubscribeToTask,
        Binding::Grpc,
        Excuse::NotApplicable("test"),
    );
    let missing = m.report();
    assert!(!missing.contains(&(Method::SubscribeToTask, Binding::Grpc)));
    assert_eq!(m.excused.len(), 1);
}

/// Recording the same cell twice must not inflate the count.
#[test]
fn recording_is_idempotent() {
    let mut m = Matrix::new();
    m.record(Method::CancelTask, Binding::HttpJson);
    m.record(Method::CancelTask, Binding::HttpJson);
    assert_eq!(m.exercised.len(), 1);
}

// ── The partition, which the summary line depends on ─────────────────

const TOTAL: usize = Method::ALL.len() * Binding::ALL.len();

/// `(exercised, not applicable, missing)` exactly as `report` counts them —
/// through the same `tally`, so a test here pins the printed summary rather
/// than a parallel reimplementation of it.
fn counts(m: &Matrix) -> (usize, usize, usize) {
    let grid = m.grid();
    assert_eq!(grid.len(), TOTAL, "the grid must classify every cell");
    let (done, na, missing) = super::tally(&grid);
    (done, na, missing.len())
}

/// The three buckets must partition the grid — for any matrix, however it
/// was built. Before the counts were taken from one walk they came from
/// `exercised.len()` and `excused.len()`, two collections a single cell
/// can be in at once, and the summary printed "45 ... of 44 cells".
#[test]
fn the_three_counts_always_sum_to_the_grid() {
    let mut plain = Matrix::new();
    plain.record(Method::GetTask, Binding::JsonRpc);
    plain.excuse(
        Method::SubscribeToTask,
        Binding::Grpc,
        Excuse::NotApplicable("no transport notion"),
    );

    let mut excused_twice = Matrix::new();
    excused_twice.excuse(Method::GetTask, Binding::Grpc, Excuse::NotApplicable("a"));
    excused_twice.excuse(Method::GetTask, Binding::Grpc, Excuse::NotApplicable("b"));

    let mut both = Matrix::new();
    both.record(Method::GetTask, Binding::Grpc);
    both.excuse(Method::GetTask, Binding::Grpc, Excuse::NotApplicable("a"));

    for (name, m) in [
        ("empty", &Matrix::new()),
        ("plain", &plain),
        ("excused twice", &excused_twice),
        ("recorded and excused", &both),
    ] {
        let (done, na, missing) = counts(m);
        assert_eq!(
            done + na + missing,
            TOTAL,
            "{name}: {done} + {na} + {missing} != {TOTAL}"
        );
    }
}

/// Excusing twice is one excused cell, and one line under "Not
/// applicable" — `record` has always deduplicated and this now matches.
#[test]
fn excusing_the_same_cell_twice_counts_once() {
    let mut m = Matrix::new();
    m.excuse(
        Method::GetTask,
        Binding::Grpc,
        Excuse::NotApplicable("first"),
    );
    m.excuse(
        Method::GetTask,
        Binding::Grpc,
        Excuse::NotApplicable("second"),
    );

    assert_eq!(m.excused.len(), 1, "the second excuse was stored as well");
    // First reason wins, matching what `is_excused` has always returned.
    assert_eq!(
        m.is_excused(Method::GetTask, Binding::Grpc),
        Some(Excuse::NotApplicable("first"))
    );
    assert_eq!(counts(&m), (0, 1, TOTAL - 1));
}

/// A cell that was excused and then actually exercised reads as exercised,
/// and is counted once. The excuse is moot, not a second cell.
#[test]
fn a_recorded_cell_wins_over_an_excuse() {
    let mut m = Matrix::new();
    m.excuse(Method::GetTask, Binding::Grpc, Excuse::NotApplicable("a"));
    m.record(Method::GetTask, Binding::Grpc);

    assert_eq!(counts(&m), (1, 0, TOTAL - 1));
    assert!(!m.report().contains(&(Method::GetTask, Binding::Grpc)));
}

/// Every method must appear once per binding, with no cell repeated —
/// otherwise `missing` could name the same gap twice and the operator
/// would chase a second one that does not exist.
#[test]
fn the_grid_names_each_cell_exactly_once() {
    let cells: BTreeSet<_> = Matrix::new()
        .grid()
        .into_iter()
        .map(|(m, b, _)| (m.wire_name(), b))
        .collect();
    assert_eq!(cells.len(), TOTAL);
}

// ── Bindings ─────────────────────────────────────────────────────────

/// Four columns, three of which the spec names. The report says so
/// explicitly, so "4 of 4 bindings" is never read as "4 of 4 the spec
/// requires".
#[test]
fn three_of_the_four_bindings_are_spec_named() {
    assert_eq!(Binding::ALL.len(), 4);
    assert_eq!(Binding::ALL.iter().filter(|b| b.is_spec_named()).count(), 3);
    assert!(!Binding::WebSocket.is_spec_named());
}

/// Two bindings sharing a label would make one column unreadable and the
/// other unfindable.
#[test]
fn binding_labels_are_distinct() {
    let labels: BTreeSet<_> = Binding::ALL.iter().map(|b| b.label()).collect();
    assert_eq!(labels.len(), Binding::ALL.len());
}

// ── Exit codes ───────────────────────────────────────────────────────

fn outcome(failures: &[&str], missing: &[(Method, Binding)]) -> SurfaceOutcome {
    SurfaceOutcome {
        failures: failures.iter().map(|s| (*s).to_owned()).collect(),
        missing: missing.to_vec(),
    }
}

/// "Something broke" and "we never checked" are different findings, and
/// the exit code has to keep them apart — collapsing them into one
/// non-zero loses the more insidious of the two.
#[test]
fn exit_code_distinguishes_a_failure_from_a_gap() {
    assert_eq!(outcome(&[], &[]).exit_code(), 0);
    assert_eq!(outcome(&["boom"], &[]).exit_code(), 1);
    assert_eq!(
        outcome(&[], &[(Method::GetTask, Binding::Grpc)]).exit_code(),
        2
    );
}

/// With both, the failure wins: a run that broke has not measured its own
/// coverage, so reporting the gap as the headline would be reporting a
/// number that was never earned.
#[test]
fn a_failure_outranks_a_gap() {
    assert_eq!(
        outcome(&["boom"], &[(Method::GetTask, Binding::Grpc)]).exit_code(),
        1
    );
}

// ── Shared fixture ───────────────────────────────────────────────────────────

/// One representative of every `ClientError` variant that is *not* a protocol
/// answer. `Http(hyper::Error)` is absent because a `hyper::Error` cannot be
/// constructed outside hyper; it is a transport error by the same reasoning as
/// `HttpClient` below.
pub(crate) fn transport_failures() -> Vec<ClientError> {
    vec![
        ClientError::HttpClient("connection refused".into()),
        ClientError::Transport("tls handshake failed".into()),
        ClientError::InvalidEndpoint("not a url".into()),
        ClientError::Timeout("no answer".into()),
        ClientError::ProtocolBindingMismatch("rest server, jsonrpc client".into()),
        ClientError::UnexpectedStatus {
            status: 502,
            body: "bad gateway".into(),
            retry_after: None,
        },
        ClientError::AuthRequired {
            task_id: TaskId::new("t-1"),
        },
        ClientError::Serialization(serde_json::from_str::<u8>("nope").unwrap_err()),
    ]
}
