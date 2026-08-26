// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Coverage matrix shared by the examples: which A2A methods ran over which
//! bindings, and whether that is complete.
//!
//! # Why this is a crate rather than a copy in each example
//!
//! Two examples make the same "covers everything" claim, and a duplicated
//! scorer is a scorer that will eventually disagree with itself — one copy
//! quietly loses a row and the example built on it reports a full matrix. The
//! denominator is already shared (it comes from the ratified proto via
//! `a2a_protocol_types::method::Method`); the *scoring* should be too.
//!
//! # Why a claim is not enough
//!
//! `examples/README.md` used to say `echo-agent` "demonstrates the complete
//! request lifecycle" and that `incident-response` showed the agent lifecycle
//! end to end. Measured on 2026-08-11 the first drove 4 of the 11 methods over
//! 2 of the 4 transports and the second drove 4 over 1. Neither sentence was a
//! lie anyone told on purpose; both were claims with nothing checking them,
//! which is the same thing six months later.
//!
//! So the claim is a computation now. Every call an example makes records
//! itself here, and [`Matrix::report`] prints the grid and returns the cells
//! that were never exercised, so the caller can exit non-zero.
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
//! The columns are the bindings the example serves. Unlike the rows they *are*
//! this project's choice — the spec names three (§9 JSON-RPC, §10 gRPC,
//! §11 HTTP+JSON) and WebSocket is a §12 custom binding — so the report says
//! so rather than presenting four as though the spec required four.

use std::collections::BTreeSet;

use a2a_protocol_types::method::Method;

/// The transports an example can serve.
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
    /// Every binding the examples can serve.
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
pub enum Excuse {
    /// The binding has no transport-level notion of this operation.
    NotApplicable(&'static str),
}

/// How one cell of the grid reads.
///
/// Private — the report's shape is not this crate's API. It exists so the
/// classification happens exactly once and both the printed grid and the
/// summary counts come from the same walk. They did not before, and the
/// summary could print a total larger than the number of cells.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Cell {
    /// Driven successfully over this binding.
    Exercised,
    /// Excused, with a reason that prints.
    NotApplicable,
    /// Should have run and did not.
    Missing,
}

impl Cell {
    const fn label(self) -> &'static str {
        match self {
            Self::Exercised => "ok",
            Self::NotApplicable => "n/a",
            Self::Missing => "MISSING",
        }
    }
}

/// Counts a classified grid: `(exercised, not applicable, missing)`.
///
/// The one place the summary's numbers come from. `report` prints from this
/// and does no counting of its own, so the printed grid and the printed
/// summary cannot disagree — they are the same data. Taking the numbers from
/// `exercised.len()` and `excused.len()` instead, as this once did, let a
/// single cell be counted in both and print "45 ... of 44 cells".
fn tally(grid: &[(Method, Binding, Cell)]) -> (usize, usize, Vec<(Method, Binding)>) {
    let mut done = 0;
    let mut not_applicable = 0;
    let mut missing = Vec::new();
    for (m, b, cell) in grid {
        match cell {
            Cell::Exercised => done += 1,
            Cell::NotApplicable => not_applicable += 1,
            Cell::Missing => missing.push((*m, *b)),
        }
    }
    (done, not_applicable, missing)
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
    /// An excused cell must print its reason rather than vanish: the day a
    /// binding genuinely cannot serve a method, the alternative is deleting
    /// the row, and a deleted row is indistinguishable from coverage.
    /// Excusing the same cell twice keeps the first reason and adds nothing.
    /// `record` has always deduplicated (it is a set); this did not, so a
    /// repeated excuse printed the cell twice under "Not applicable" and
    /// counted it twice in the summary.
    pub fn excuse(&mut self, method: Method, binding: Binding, why: Excuse) {
        if self.is_excused(method, binding).is_none() {
            self.excused.push((method, binding, why));
        }
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

    /// Every cell, classified exactly once, in row-major order.
    ///
    /// The three classes partition the grid, so counting them can never come
    /// to more than `Method::ALL.len() * Binding::ALL.len()` — which taking
    /// the counts from `exercised.len()` and `excused.len()` could, because a
    /// cell can be in both collections.
    fn grid(&self) -> Vec<(Method, Binding, Cell)> {
        let mut out = Vec::with_capacity(Method::ALL.len() * Binding::ALL.len());
        for m in Method::ALL {
            for b in Binding::ALL {
                let cell = if self.was_exercised(*m, *b) {
                    Cell::Exercised
                } else if self.is_excused(*m, *b).is_some() {
                    Cell::NotApplicable
                } else {
                    Cell::Missing
                };
                out.push((*m, *b, cell));
            }
        }
        out
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

        // One classification, printed and counted. `tally` is the only place
        // the summary's numbers come from, so they cannot disagree with the
        // grid above them.
        let grid = self.grid();
        let (done, not_applicable, missing) = tally(&grid);

        for row in grid.chunks(Binding::ALL.len()) {
            print!("{:<width$}", row[0].0.wire_name(), width = width);
            for (_, _, cell) in row {
                print!(" {:^11}", cell.label());
            }
            println!();
        }

        // Only the excuses that are actually in effect. An excuse for a cell
        // that was then exercised is moot, and printing it under "Not
        // applicable" while the grid shows `ok` for the same cell is the kind
        // of contradiction this module exists to stop.
        let in_effect: Vec<_> = self
            .excused
            .iter()
            .filter(|(m, b, _)| !self.was_exercised(*m, *b))
            .collect();
        if !in_effect.is_empty() {
            println!("\nNot applicable, with reasons:");
            for (m, b, Excuse::NotApplicable(why)) in in_effect {
                println!("  {} over {} — {why}", m.wire_name(), b.label());
            }
        }

        let total = Method::ALL.len() * Binding::ALL.len();
        println!(
            "\n{done} exercised, {not_applicable} not applicable, {} missing, of {total} cells",
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
mod tests;

pub mod counter;
pub mod sweep;

pub use counter::CounterOutcome;
pub use sweep::{make_send_params, sweep, SweepOutcome};

// ── One-call surface phase ───────────────────────────────────────────────────

/// Everything an example must supply to have its surface measured.
pub struct SurfaceRun<'a> {
    /// A client per binding, already connected. A binding absent from this
    /// list is *not* silently excused — [`run_surface`] reports its whole
    /// column as missing, because "we could not connect" and "this binding is
    /// covered" must never look the same.
    pub clients: Vec<(Binding, a2a_protocol_client::A2aClient)>,
    /// A client against the agent under test, for the counter-tests.
    pub main_client: &'a a2a_protocol_client::A2aClient,
    /// A client against an agent advertising *no* optional capabilities.
    pub restricted_client: &'a a2a_protocol_client::A2aClient,
    /// URL of something that answers, so push configs point somewhere real.
    pub webhook_url: String,
    /// Message-text marker that makes the agent pause mid-task.
    pub slow_prefix: &'a str,
}

/// What the surface phase concluded.
pub struct SurfaceOutcome {
    /// Calls that failed outright.
    pub failures: Vec<String>,
    /// Cells that should have been exercised but were not.
    pub missing: Vec<(a2a_protocol_types::method::Method, Binding)>,
}

impl SurfaceOutcome {
    /// The exit code this outcome implies: `0` complete, `1` a call failed,
    /// `2` the matrix has a gap.
    ///
    /// Distinct codes on purpose. "Something broke" and "we never checked"
    /// are different findings, and collapsing them into one non-zero loses
    /// the more insidious of the two.
    #[must_use]
    pub fn exit_code(&self) -> i32 {
        if !self.failures.is_empty() {
            1
        } else if !self.missing.is_empty() {
            2
        } else {
            0
        }
    }
}

/// Runs the sweep over every supplied binding, then the counter-tests, then
/// prints the matrix.
///
/// Shared by every example so their coverage claims are the same computation
/// rather than four private definitions of "complete".
pub async fn run_surface(run: SurfaceRun<'_>) -> SurfaceOutcome {
    let mut matrix = Matrix::new();
    let mut failures = Vec::new();

    for (binding, client) in &run.clients {
        println!("  --- {} ---", binding.label());
        let outcome = sweep(
            client,
            *binding,
            &run.webhook_url,
            run.slow_prefix,
            &mut matrix,
        )
        .await;
        for l in &outcome.lines {
            println!("  {l}");
        }
        failures.extend(outcome.failures);
    }

    println!("  --- counter-tests (calls that must be refused) ---");
    let counter_out = counter::run(run.main_client, run.restricted_client).await;
    for l in &counter_out.lines {
        println!("  {l}");
    }
    failures.extend(counter_out.failures);

    println!();
    let missing = matrix.report();

    if !failures.is_empty() {
        println!("\n{} call(s) failed:", failures.len());
        for f in &failures {
            println!("  - {f}");
        }
    }
    if !missing.is_empty() {
        println!("\n{} matrix cell(s) never ran:", missing.len());
        for (m, b) in &missing {
            println!("  - {} over {}", m.wire_name(), b.label());
        }
    }
    if failures.is_empty() && missing.is_empty() {
        println!("\nEvery A2A method was exercised over every binding, and every");
        println!("counter-test was refused as the specification requires.");
    }

    SurfaceOutcome { failures, missing }
}
