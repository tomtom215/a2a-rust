// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Asserts that every test in this suite is actually run, and actually claimed.
//!
//! # The hole this closes
//!
//! The suite is ~100 `pub async fn test_*` functions in eight modules, invoked
//! by ~100 hand-written `results.push(module::test_name(&ctx).await)` lines in
//! `main.rs`. Nothing tied the two lists together.
//!
//! `features::audit` cross-checks the claim table against the run in both
//! directions and is genuinely strict — but it can only see tests that *ran*.
//! A `pub async fn test_*` that nobody added to `main.rs` never runs, so it is
//! absent from `results`, so `unclaimed_tests` cannot report it. It is caught
//! only if some claim happens to name it, in which case it surfaces as
//! `unknown_tests`. A test that is neither registered nor claimed — which is
//! precisely the state a newly written one starts in — is invisible: the suite
//! prints "All N tests passed" and the new one is not among the N.
//!
//! These checks read the source rather than the run, so they hold for
//! feature-gated tests too and cost milliseconds instead of the two minutes
//! the dogfood run needs to start four servers.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// Every `.rs` file under a path relative to the crate root, resolved from
/// the manifest directory so the check does not depend on the working
/// directory.
fn rs_files_under(rel: &str) -> Vec<PathBuf> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        for entry in std::fs::read_dir(dir).expect("read source directory") {
            let path = entry.expect("dir entry").path();
            if path.is_dir() {
                walk(&path, out);
            } else if path.extension().is_some_and(|e| e == "rs") {
                out.push(path);
            }
        }
    }
    let mut out = Vec::new();
    walk(&Path::new(env!("CARGO_MANIFEST_DIR")).join(rel), &mut out);
    out.sort();
    out
}

/// Test name -> the file that defines it.
fn defined_tests() -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for path in rs_files_under("src/tests") {
        let src = std::fs::read_to_string(&path).expect("read suite file");
        let short = path
            .strip_prefix(env!("CARGO_MANIFEST_DIR"))
            .unwrap_or(&path)
            .display()
            .to_string();
        for line in src.lines() {
            if let Some(rest) = line.trim().strip_prefix("pub async fn test_") {
                let name: String = rest
                    .chars()
                    .take_while(|c| c.is_alphanumeric() || *c == '_')
                    .collect();
                out.insert(format!("test_{name}"), short.clone());
            }
        }
    }
    out
}

/// Test names appearing in a `results.push(...)` call in `main.rs`.
fn registered_tests() -> BTreeSet<String> {
    let src = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("src/main.rs"))
        .expect("read main.rs");
    let mut out = BTreeSet::new();
    for line in src.lines() {
        let Some(after) = line.trim().strip_prefix("results.push(") else {
            continue;
        };
        // `module::test_name(&ctx).await)` — take the segment after the last
        // `::` up to the opening parenthesis.
        let call = after.split('(').next().unwrap_or_default();
        if let Some(name) = call.rsplit("::").next() {
            if name.starts_with("test_") {
                out.insert(name.to_owned());
            }
        }
    }
    out
}

/// A test nobody registered never runs, and the summary still says "All N
/// tests passed" — with the new one absent from N.
#[test]
fn every_test_the_suite_defines_is_registered_in_main() {
    let defined = defined_tests();
    assert!(
        defined.len() > 50,
        "only {} tests found — the parser stopped matching the suite's shape, \
         which would make this check pass by finding nothing",
        defined.len()
    );

    let registered = registered_tests();
    let unregistered: Vec<_> = defined
        .iter()
        .filter(|(name, _)| !registered.contains(*name))
        .map(|(name, file)| format!("{name} (defined in {file})"))
        .collect();

    assert!(
        unregistered.is_empty(),
        "{} test(s) are defined but never run — add a `results.push(...)` line \
         for each in src/main.rs:\n  {}",
        unregistered.len(),
        unregistered.join("\n  ")
    );
}

/// The reverse: a `results.push` naming something the suite no longer defines.
/// This would fail to compile today, but only while the name is spelled in
/// full — the assertion keeps the two lists symmetric rather than relying on
/// that.
#[test]
fn every_registered_test_still_exists() {
    let defined = defined_tests();
    let orphans: Vec<_> = registered_tests()
        .into_iter()
        .filter(|name| !defined.contains_key(name))
        .collect();
    assert!(
        orphans.is_empty(),
        "main.rs registers test(s) the suite does not define: {orphans:?}"
    );
}

/// The names a test reports itself under, as passed to `TestResult::pass`
/// or `::fail`. These — not the function names — are what the claim table
/// references and what `features::audit` matches against at run time.
///
/// Extracted rather than derived: the obvious convention (`test_a_b` reports
/// `"a-b"`) holds for only 62 of the 100 tests. The rest carry a number
/// (`"51-ws-send-message"`) or underscores (`"79_signing_e2e"`), so deriving
/// the name would silently miss 38 of them and the check would pass by
/// comparing two empty-ish sets.
fn reported_names_under(rel: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for path in rs_files_under(rel) {
        // This file is not part of the suite, and it necessarily contains the
        // very markers the scan looks for — reading itself invents names out
        // of its own source. Found the first time this check was widened from
        // `src/tests` to `src`.
        if path.ends_with("tests/registration.rs") {
            continue;
        }
        let src = std::fs::read_to_string(&path).expect("read suite file");
        let mut rest = src.as_str();
        while let Some(i) = rest.find("TestResult::") {
            rest = &rest[i + "TestResult::".len()..];
            let Some(open) = rest.find('(') else { break };
            let head = &rest[..open];
            if head != "pass" && head != "fail" {
                continue;
            }
            let after = &rest[open + 1..];
            let Some(q1) = after.find('"') else { continue };
            // Only a literal directly after the parenthesis, allowing the
            // whitespace rustfmt inserts when the call is wrapped.
            if !after[..q1].trim().is_empty() {
                continue;
            }
            let Some(q2) = after[q1 + 1..].find('"') else {
                continue;
            };
            out.insert(after[q1 + 1..q1 + 1 + q2].to_owned());
        }
    }
    out
}

/// Which names each `test_*` function reports itself under.
///
/// Built by walking each file in order and attributing every
/// `TestResult::pass`/`::fail` literal to the most recent `pub async fn
/// test_*` above it. Comparing bare *sets* is not enough: each function has
/// several such sites (a pass and one or more fails), so renaming one of them
/// leaves the set of distinct names exactly as large as it was, and a check on
/// size alone reports nothing.
fn names_by_function() -> BTreeMap<String, BTreeSet<String>> {
    let mut out: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for path in rs_files_under("src/tests") {
        let src = std::fs::read_to_string(&path).expect("read suite file");
        let mut current: Option<String> = None;
        for line in src.lines() {
            let trimmed = line.trim();
            if let Some(rest) = trimmed.strip_prefix("pub async fn test_") {
                let name: String = rest
                    .chars()
                    .take_while(|c| c.is_alphanumeric() || *c == '_')
                    .collect();
                current = Some(format!("test_{name}"));
            }
            for marker in ["TestResult::pass(", "TestResult::fail("] {
                let Some(after) = trimmed.find(marker) else {
                    continue;
                };
                let tail = &trimmed[after + marker.len()..];
                if !tail.starts_with('"') {
                    continue;
                }
                let Some(q) = tail[1..].find('"') else {
                    continue;
                };
                if let Some(f) = &current {
                    out.entry(f.clone())
                        .or_default()
                        .insert(tail[1..1 + q].to_owned());
                }
            }
        }
    }
    out
}

/// Each test reports itself under exactly one name.
///
/// A function whose pass and fail paths report different names is scored under
/// whichever one ran: the claim table sees a name that is sometimes absent, and
/// `audit` reports it as an unknown test only on the runs where the other path
/// was taken.
#[test]
fn each_test_reports_exactly_one_name() {
    let offenders: Vec<_> = names_by_function()
        .into_iter()
        .filter(|(_, names)| names.len() != 1)
        .map(|(f, names)| format!("{f} reports {names:?}"))
        .collect();
    assert!(
        offenders.is_empty(),
        "{} test(s) do not report a single consistent name:\n  {}",
        offenders.len(),
        offenders.join("\n  ")
    );
}

/// No two tests report the same name. `main.rs` builds `by_name` as a
/// `HashMap` keyed on these, so a shared name means one test silently
/// overwrites the other: it disappears from the claim table's evidence, and a
/// failure in it is scored as the other's pass.
#[test]
fn no_two_tests_report_the_same_name() {
    let mut owner: BTreeMap<String, String> = BTreeMap::new();
    let mut clashes = Vec::new();
    for (function, names) in names_by_function() {
        for name in names {
            if let Some(first) = owner.get(&name) {
                clashes.push(format!("{name:?} reported by both {first} and {function}"));
            } else {
                owner.insert(name, function.clone());
            }
        }
    }
    assert!(
        clashes.is_empty(),
        "{} name collision(s):\n  {}",
        clashes.len(),
        clashes.join("\n  ")
    );
}

/// Every test must be named by a claim, so the summary reports what the suite
/// measured. `features::audit` checks this too, but only for tests that ran —
/// which is exactly the set an unregistered test is missing from.
///
/// Scoped to all of `src/`, not just `src/tests/`: `surface.rs` pushes two
/// results of its own (`surface-sweep`, `surface-counter`) and they are
/// claimed like any other.
#[test]
fn every_test_the_suite_defines_is_named_by_a_claim() {
    let claimed: BTreeSet<&str> = crate::features::claims()
        .iter()
        .flat_map(|c| c.backed_by.iter().copied())
        .collect();

    let unclaimed: Vec<_> = reported_names_under("src")
        .into_iter()
        .filter(|name| !claimed.contains(name.as_str()))
        .collect();

    assert!(
        unclaimed.is_empty(),
        "{} test(s) are not named by any claim in features.rs, so the summary \
         does not report what they measure: {unclaimed:?}",
        unclaimed.len()
    );
}

/// A claim naming a test the suite does not report is a typo or a deletion.
/// `audit` reports it at run time; this reports it without starting servers.
#[test]
fn every_claimed_test_is_defined_by_the_suite() {
    let reported = reported_names_under("src");
    let mut missing = Vec::new();
    for claim in crate::features::claims() {
        for name in claim.backed_by {
            if !reported.contains(*name) {
                missing.push(format!("{:?} names {name:?}", claim.label));
            }
        }
    }
    assert!(
        missing.is_empty(),
        "claim(s) name tests the suite does not report:\n  {}",
        missing.join("\n  ")
    );
}
