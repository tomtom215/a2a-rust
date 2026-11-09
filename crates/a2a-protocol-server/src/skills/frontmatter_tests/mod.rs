// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! `SKILL.md` against the specification: what is loaded, what is skipped, and
//! what is said about it.

use super::*;
use crate::skills::DEFAULT_MAX_BODY_BYTES;

const MAX_BODY: usize = DEFAULT_MAX_BODY_BYTES;

fn read_in(directory: &str, text: &str) -> (Option<Parsed>, Vec<Finding>) {
    let mut findings = Vec::new();
    let parsed = parse(text, directory, MAX_BODY, &mut findings);
    (parsed, findings)
}

fn read(text: &str) -> (Option<Parsed>, Vec<Finding>) {
    read_in("demo", text)
}

fn file(frontmatter: &str, body: &str) -> String {
    format!("---\n{frontmatter}\n---\n{body}")
}

fn kinds(findings: &[Finding]) -> Vec<(Severity, DiagnosticKind)> {
    findings.iter().map(|f| (f.severity, f.kind)).collect()
}

fn loaded(text: &str) -> Parsed {
    let (parsed, findings) = read(text);
    assert_eq!(findings, [], "{text:?}");
    parsed.expect("the skill loads")
}

mod fields;
mod names;
mod random;
mod structure;
