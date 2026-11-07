// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Discovery over real directories: what is loaded, what is skipped, what is
//! said, and that nothing panics and nothing stops.

use std::fmt::Write as _;

use super::*;
use crate::skills::test_support::Scratch;
use crate::skills::{SkillCatalog, SkillSource};

fn kinds(catalog: &SkillCatalog) -> Vec<(Severity, DiagnosticKind)> {
    catalog
        .diagnostics()
        .iter()
        .map(|d| (d.severity(), d.kind()))
        .collect()
}

fn names(catalog: &SkillCatalog) -> Vec<&str> {
    catalog.skills().iter().map(Skill::name).collect()
}

mod basics;
mod bounds;
mod caps;
mod listing;
