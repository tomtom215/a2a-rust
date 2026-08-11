// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Records which (method, binding) pairs the demo actually drove, and refuses
//! to report success unless the matrix is complete.
//!
//! # Why a claim is not enough
//!
//! `examples/README.md` used to say this example "demonstrates the complete
//! request lifecycle". Measured on 2026-08-11 it drove 4 of the 11 methods
//! over 2 of the 4 transports. The sentence was not a lie anyone told on
//! purpose; it was a claim with nothing checking it, which is the same thing
//! six months later.
//!
//! So the claim is a computation now. Every call the demo makes records
//! itself here, and [`Matrix::report`] prints the grid and returns a non-zero
//! exit code if any cell that *should* have been exercised was not.
//!
//! # Where the denominator comes from
//!
//! Not from this file. The rows are
//! [`a2a_protocol_types::method::Method::ALL`], which the types crate asserts
//! equal to `service A2AService` in the ratified `proto/a2a_v1/a2a.proto`, and
//! which `scripts/check_method_denominator.py` cross-checks against the
//! official `a2aproject/a2a-tck` suite on every Official TCK run. A reviewer
//! auditing "is this 11 the real 11?" reads the proto, not this example.
//!
//! The columns are the four bindings this agent serves. Unlike the rows they
//! *are* this project's choice — the spec names three (§9 JSON-RPC, §10 gRPC,
//! §11 HTTP+JSON) and WebSocket is a §12 custom binding — so the report says
//! so rather than presenting four as though the spec required four.

use std::collections::BTreeSet;

use a2a_protocol_types::method::Method;

/// The transports this example serves.
///
/// `WebSocket` is a §12 *custom* binding, not one of the three the spec
/// names. Kept distinct in the report so "4 of 4 bindings" is never read as
/// "4 of 4 spec-required bindings".
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Binding {
    /// Spec §9.
    JsonRpc,
    /// Spec §11.
    HttpJson,
    /// Spec §10.
    Grpc,
    /// Spec §12 custom binding.
    WebSocket,
}

impl Binding {
    /// Every binding this example serves.
    pub const ALL: &'static [Self] = &[Self::JsonRpc, Self::HttpJson, Self::Grpc, Self::WebSocket];

    /// Short label used in the report grid.
    pub const fn label(self) -> &'static str {
        match self {
            Self::JsonRpc => "JSONRPC",
            Self::HttpJson => "HTTP+JSON",
            Self::Grpc => "GRPC",
            Self::WebSocket => "WEBSOCKET",
        }
    }

    /// `false` for the §12 custom binding.
    pub const fn is_spec_named(self) -> bool {
        !matches!(self, Self::WebSocket)
    }
}

/// Why a cell is legitimately empty.
///
/// An excused cell still prints, with its reason. A blank cell and an excused
/// cell must never look the same — that collapse is what let the old feature
/// checklist in `agent-team` report `[x]` for things it never ran.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum Excuse {
    /// The binding has no transport-level notion of this operation.
    NotApplicable(&'static str),
}

/// Records what ran.
pub struct Matrix {
    exercised: BTreeSet<(String, Binding)>,
    excused: Vec<(Method, Binding, Excuse)>,
}

impl Default for Matrix {
    fn default() -> Self {
        Self::new()
    }
}

impl Matrix {
    /// An empty matrix.
    #[must_use]
    pub fn new() -> Self {
        Self {
            exercised: BTreeSet::new(),
            excused: Vec::new(),
        }
    }

    /// Records that `method` was successfully driven over `binding`.
    ///
    /// Call this only after the call returned a result the demo checked.
    /// Recording on attempt rather than on success would make the matrix a
    /// log of intentions.
    pub fn record(&mut self, method: Method, binding: Binding) {
        self.exercised
            .insert((method.wire_name().to_owned(), binding));
    }

    /// Marks a cell as legitimately unreachable, with a reason that prints.
    ///
    /// Unused by the current demo, which reaches every cell — kept because an
    /// excused cell must print its reason rather than vanish, and the day a
    /// binding genuinely cannot serve a method the alternative is deleting the
    /// row. Exercised by the unit tests below.
    #[allow(dead_code)]
    pub fn excuse(&mut self, method: Method, binding: Binding, why: Excuse) {
        self.excused.push((method, binding, why));
    }

    fn is_excused(&self, method: Method, binding: Binding) -> Option<Excuse> {
        self.excused
            .iter()
            .find(|(m, b, _)| *m == method && *b == binding)
            .map(|(_, _, w)| *w)
    }

    fn was_exercised(&self, method: Method, binding: Binding) -> bool {
        self.exercised
            .contains(&(method.wire_name().to_owned(), binding))
    }

    /// Prints the grid and returns the cells that should have run but did not.
    ///
    /// # Returns
    ///
    /// The missing `(method, binding)` pairs. Empty means complete.
    pub fn report(&self) -> Vec<(Method, Binding)> {
        let width = Method::ALL
            .iter()
            .map(|m| m.wire_name().len())
            .max()
            .unwrap_or(32);

        print!("{:<width$}", "METHOD", width = width);
        for b in Binding::ALL {
            print!(" {:^11}", b.label());
        }
        println!();

        let mut missing = Vec::new();
        for m in Method::ALL {
            print!("{:<width$}", m.wire_name(), width = width);
            for b in Binding::ALL {
                let cell = if self.was_exercised(*m, *b) {
                    "ok"
                } else if self.is_excused(*m, *b).is_some() {
                    "n/a"
                } else {
                    missing.push((*m, *b));
                    "MISSING"
                };
                print!(" {cell:^11}");
            }
            println!();
        }

        if !self.excused.is_empty() {
            println!("\nNot applicable, with reasons:");
            for (m, b, Excuse::NotApplicable(why)) in &self.excused {
                println!("  {} over {} — {why}", m.wire_name(), b.label());
            }
        }

        let total = Method::ALL.len() * Binding::ALL.len();
        let excused = self.excused.len();
        let done = self.exercised.len();
        println!(
            "\n{done} exercised, {excused} not applicable, {} missing, of {total} cells",
            missing.len()
        );
        println!(
            "Rows: the {} methods `service A2AService` declares in the ratified \
             proto (see a2a_protocol_types::method).",
            Method::ALL.len()
        );
        println!(
            "Columns: {} spec-named bindings (§9 §10 §11) plus WEBSOCKET, a §12 \
             custom binding this SDK adds.",
            Binding::ALL.iter().filter(|b| b.is_spec_named()).count()
        );

        missing
    }
}

#[cfg(test)]
mod tests {
    use super::{Binding, Excuse, Matrix};
    use a2a_protocol_types::method::Method;

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
}
