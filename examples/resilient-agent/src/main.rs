// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Resilient agent — durability, failure injection and horizontal scaling.
//!
//! The other examples answer "what is an agent" and "does every method work
//! on every binding". This one answers the three questions an operator asks
//! before putting an agent behind a load balancer:
//!
//! | Act | Question | How it is answered |
//! |-----|----------|--------------------|
//! | 1 | Does a task survive the process dying? | A task is created, streamed part-way, and the handler — store handles and all — is replaced by a fresh one over the **same SQLite file**. `GetTask` on the new handler must return the task with history, artifacts and push config intact |
//! | 2 | What happens when things fail? | An executor that fails its first N attempts, a webhook that refuses its first M deliveries, and a proxy that faults the first K requests — each with the numbers the SDK reports, and a plain statement of what it does **not** do |
//! | 3 | What does "two replicas" mean? | Two handlers with in-memory stores cannot see each other's tasks (shown). Two over a shared PostgreSQL can — and a shared rate-limit counter enforces one limit across both (shown, with the per-replica number beside it) |
//!
//! Every act **asserts** rather than narrates: each names the specific wrong
//! answer in its failure message, and the process exits non-zero when it sees
//! one. An act that needs a service this example cannot start reports
//! `[NOT RUN]` with what to set, following `incident-response`'s convention
//! exactly: the process still exits 0 unless [`REQUIRE_ALL_ENV`] is set, in
//! which case an unexercised act exits `4`.
//!
//! Exit codes: `0` every act that ran passed, `3` an act failed, `4` an act
//! went unexercised while `RESILIENT_REQUIRE_ALL` was set.

mod durability;
mod failure;
mod scaling;
mod support;

#[cfg(test)]
mod tests;

/// One demonstrated property and what happened to it.
pub struct Check {
    /// What is being demonstrated.
    pub label: &'static str,
    /// The verdict, with detail.
    pub outcome: Outcome,
}

/// Verdict for a single check.
pub enum Outcome {
    /// Exercised and correct.
    Pass(String),
    /// Exercised and wrong — the detail names the wrong answer.
    Fail(String),
    /// Behind a Cargo feature that is off in this build.
    ///
    /// Unconstructed in the default build, where both features are on.
    #[allow(dead_code)]
    NotCompiled(&'static str),
    /// Compiled in, but the service it needs was not available.
    ///
    /// Distinct from [`NotCompiled`](Self::NotCompiled) on purpose: "the
    /// binary cannot do this" and "the binary can do this and nobody tried"
    /// are different facts. Only the PostgreSQL checks construct this.
    #[allow(dead_code)]
    NotRun(String),
}

impl Check {
    fn pass(label: &'static str, detail: impl Into<String>) -> Self {
        Self {
            label,
            outcome: Outcome::Pass(detail.into()),
        }
    }

    fn fail(label: &'static str, detail: impl Into<String>) -> Self {
        Self {
            label,
            outcome: Outcome::Fail(detail.into()),
        }
    }

    /// A check compiled out of this build. Only reachable from the
    /// `#[cfg(not(feature = ...))]` arms.
    #[allow(dead_code)]
    const fn skipped(label: &'static str, feature: &'static str) -> Self {
        Self {
            label,
            outcome: Outcome::NotCompiled(feature),
        }
    }

    /// A check that is compiled in but had nothing to run against.
    #[allow(dead_code)]
    fn unavailable(label: &'static str, reason: impl Into<String>) -> Self {
        Self {
            label,
            outcome: Outcome::NotRun(reason.into()),
        }
    }

    /// Folds a `Result<detail, detail>` into a pass or a fail.
    fn from_result(label: &'static str, result: Result<String, String>) -> Self {
        match result {
            Ok(detail) => Self::pass(label, detail),
            Err(detail) => Self::fail(label, detail),
        }
    }

    const fn failed(&self) -> bool {
        matches!(self.outcome, Outcome::Fail(_))
    }

    const fn not_compiled(&self) -> bool {
        matches!(self.outcome, Outcome::NotCompiled(_))
    }

    const fn not_run(&self) -> bool {
        matches!(self.outcome, Outcome::NotRun(_))
    }
}

/// Environment variable that makes an unexercised check an error.
///
/// The same switch `incident-response` calls `INCIDENT_REQUIRE_ALL`, for the
/// same reason: on a laptop a `[NOT RUN]` is a note; in CI, which provisions
/// the PostgreSQL service, it means the service quietly stopped being
/// provisioned and the job stayed green.
pub const REQUIRE_ALL_ENV: &str = "RESILIENT_REQUIRE_ALL";

/// Runs the three acts in order and prints each as it finishes.
///
/// Sequential rather than concurrent so the transcript reads top to bottom
/// and one act's sockets and temp directories are gone before the next.
pub async fn run() -> Vec<Check> {
    let mut checks = Vec::new();

    println!("Act 1 — Durability: a task outlives its handler");
    println!("------------------------------------------------");
    for check in durability::run().await {
        report_one(&check);
        checks.push(check);
    }

    println!();
    println!("Act 2 — Failure injection: what the SDK reports, and what it does not do");
    println!("-------------------------------------------------------------------------");
    for check in failure::run().await {
        report_one(&check);
        checks.push(check);
    }

    println!();
    println!("Act 3 — Horizontal scaling: what \"shared\" means");
    println!("--------------------------------------------------");
    for check in scaling::run().await {
        report_one(&check);
        checks.push(check);
    }

    checks
}

fn report_one(check: &Check) {
    match &check.outcome {
        Outcome::Pass(detail) => println!("  [ok]        {:<58} {detail}", check.label),
        Outcome::Fail(detail) => println!("  [FAIL]      {:<58} {detail}", check.label),
        Outcome::NotCompiled(feature) => println!(
            "  [NOT BUILT] {:<58} needs --features {feature}",
            check.label
        ),
        Outcome::NotRun(reason) => println!("  [NOT RUN]   {:<58} {reason}", check.label),
    }
}

/// Prints the summary line and returns how many checks failed.
pub fn summarize(checks: &[Check]) -> usize {
    let failed = checks.iter().filter(|c| c.failed()).count();
    let not_built = checks.iter().filter(|c| c.not_compiled()).count();
    let not_run = checks.iter().filter(|c| c.not_run()).count();
    println!();
    println!(
        "  {} passed, {failed} failed, {not_built} not compiled, {not_run} not run",
        checks.len() - failed - not_built - not_run
    );
    if not_built > 0 {
        println!("  Rerun with --all-features to compile every act in.");
    }
    if not_run > 0 {
        println!("  A [NOT RUN] check is not a passing one — it is a gap this run did not close.");
    }
    failed
}

/// Exits `4` if [`REQUIRE_ALL_ENV`] is set and any check went unexercised.
pub fn require_all_exercised(checks: &[Check]) {
    if std::env::var(REQUIRE_ALL_ENV).is_err() {
        return;
    }
    let unexercised: Vec<&str> = checks
        .iter()
        .filter(|c| c.not_compiled() || c.not_run())
        .map(|c| c.label)
        .collect();
    if unexercised.is_empty() {
        return;
    }
    println!();
    println!(
        "{REQUIRE_ALL_ENV} is set, so {} unexercised check(s) is an error:",
        unexercised.len()
    );
    for label in &unexercised {
        println!("  - {label}");
    }
    std::process::exit(4);
}

#[tokio::main]
async fn main() {
    println!("resilient-agent");
    println!("===============");
    println!();
    let checks = run().await;
    if summarize(&checks) > 0 {
        std::process::exit(3);
    }
    require_all_exercised(&checks);
}
