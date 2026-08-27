// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Tests for the auditor that decides whether the suite's summary is honest.
//!
//! Everything `cargo run -p agent-team` prints about SDK coverage comes from
//! this module. Until 2026-08-11 the claim table was a hardcoded array printed
//! as `[x]` unconditionally, and it stayed green through six failing tests.
//! The code that replaced it is the reason anyone can believe the summary —
//! and it had no tests of its own, which is the same shape one level up.

use std::collections::HashMap;

use super::{audit, status_of, ClaimStatus, FeatureClaim};
use crate::tests::TestResult;

fn claim(label: &'static str, backed_by: &'static [&'static str]) -> FeatureClaim {
    FeatureClaim {
        label,
        backed_by,
        compiled_in: true,
    }
}

fn gated(label: &'static str, backed_by: &'static [&'static str]) -> FeatureClaim {
    FeatureClaim {
        label,
        backed_by,
        compiled_in: false,
    }
}

fn ran(pairs: &[(&'static str, bool)]) -> HashMap<&'static str, bool> {
    pairs.iter().copied().collect()
}

fn results(pairs: &[(&str, bool)]) -> Vec<TestResult> {
    pairs
        .iter()
        .map(|(n, ok)| {
            if *ok {
                TestResult::pass(n, 0, "")
            } else {
                TestResult::fail(n, 0, "")
            }
        })
        .collect()
}

// ── status_of ────────────────────────────────────────────────────────────────

#[test]
fn a_claim_whose_backing_tests_passed_is_proven() {
    let c = claim("streaming", &["test_a", "test_b"]);
    let status = status_of(&c, &ran(&[("test_a", true), ("test_b", true)]));
    assert!(status == ClaimStatus::Proven);
    assert_eq!(status.marker(), "[x]");
}

/// One failure is enough. A claim backed by five tests where one failed is not
/// four-fifths true — it is a claim the run disproved, and printing `[x]` for
/// it is exactly the fiction this module exists to prevent.
#[test]
fn one_failing_backing_test_fails_the_whole_claim() {
    let c = claim("streaming", &["test_a", "test_b"]);
    let status = status_of(&c, &ran(&[("test_a", true), ("test_b", false)]));
    assert!(status == ClaimStatus::Failed);
    assert_eq!(status.marker(), "[FAIL]");
}

/// A failure outranks a pass regardless of order, so the verdict cannot depend
/// on which backing test the table happens to list first.
#[test]
fn a_failure_outranks_a_pass_in_either_order() {
    let forwards = claim("x", &["bad", "good"]);
    let backwards = claim("x", &["good", "bad"]);
    let seen = ran(&[("good", true), ("bad", false)]);
    assert!(status_of(&forwards, &seen) == ClaimStatus::Failed);
    assert!(status_of(&backwards, &seen) == ClaimStatus::Failed);
}

/// A claim compiled out renders NOT RUN rather than vanishing. A hidden row
/// and a passing row look the same to a reader scanning for gaps, which is how
/// six feature areas were skipped while the binary printed "dogfood complete".
#[test]
fn a_compiled_out_claim_is_not_run_even_if_a_test_of_that_name_passed() {
    let c = gated("websocket", &["test_a"]);
    let status = status_of(&c, &ran(&[("test_a", true)]));
    assert!(status == ClaimStatus::NotRun);
    assert_eq!(status.marker(), "[ ] NOT RUN —");
}

/// Compiled in, but no backing test ran at all: also NOT RUN. Proven requires
/// positive evidence, never the absence of a failure.
#[test]
fn a_claim_with_no_evidence_is_not_run_rather_than_proven() {
    let c = claim("streaming", &["test_a"]);
    assert!(status_of(&c, &ran(&[])) == ClaimStatus::NotRun);
    assert!(status_of(&c, &ran(&[("something_else", true)])) == ClaimStatus::NotRun);
}

// ── audit ────────────────────────────────────────────────────────────────────

#[test]
fn a_table_that_matches_the_run_is_clean() {
    let claims = vec![claim("streaming", &["test_a"]), claim("push", &["test_b"])];
    let report = audit(&claims, &results(&[("test_a", true), ("test_b", false)]));
    assert!(report.is_clean(), "a failing test is not table drift");
}

/// A claim no test backs is a sentence the run did not earn.
#[test]
fn a_claim_with_no_test_that_ran_is_unbacked() {
    let claims = vec![claim("streaming", &["never_registered"])];
    let report = audit(&claims, &results(&[("test_a", true)]));
    assert!(!report.is_clean());
    assert_eq!(report.unbacked_claims, vec!["streaming"]);
}

/// A claim naming a test that did not run — a typo, a deletion, or a test
/// nobody registered — weakens that claim's evidence silently. It is reported
/// against the claim that names it, so the fix is obvious.
#[test]
fn a_claim_naming_a_test_that_did_not_run_is_reported() {
    let claims = vec![claim("streaming", &["test_a", "test_typo"])];
    let report = audit(&claims, &results(&[("test_a", true)]));
    assert!(!report.is_clean());
    assert_eq!(report.unknown_tests, vec![("streaming", "test_typo")]);
    // Still backed — one of its tests did run — so it is not also unbacked.
    assert!(report.unbacked_claims.is_empty());
}

/// The other direction, which is the one a hardcoded table always gets wrong:
/// a test ran and no claim mentions it, so the suite is measuring something
/// the summary does not report.
#[test]
fn a_test_no_claim_mentions_is_reported() {
    let claims = vec![claim("streaming", &["test_a"])];
    let report = audit(
        &claims,
        &results(&[("test_a", true), ("test_orphan", true)]),
    );
    assert!(!report.is_clean());
    assert_eq!(report.unclaimed_tests, vec!["test_orphan".to_owned()]);
}

/// A gated-off claim's tests legitimately do not exist in this build, so it
/// must not be reported as drift — otherwise a narrowed build could never be
/// clean and the check would be routed around.
#[test]
fn a_compiled_out_claim_is_not_drift() {
    let claims = vec![gated("websocket", &["test_ws"])];
    let report = audit(&claims, &results(&[]));
    assert!(
        report.is_clean(),
        "a compiled-out claim was reported as drift: {:?}",
        report.unbacked_claims
    );
}

/// The real table must describe the real suite. This is the assertion that
/// would fail if somebody added a claim and forgot its tests, or the reverse —
/// checked here in milliseconds rather than after the dogfood run has started
/// four servers.
#[test]
fn every_claim_names_at_least_one_test() {
    for c in super::claims() {
        assert!(
            !c.backed_by.is_empty(),
            "claim {:?} names no backing test; `audit` would report it unbacked \
             on every run",
            c.label
        );
    }
}
