// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Part of the tests in `mod.rs`.

use super::*;

#[test]
fn an_empty_skills_directory_is_an_empty_catalog_with_nothing_said() {
    let scratch = Scratch::new("zero");
    std::fs::create_dir_all(scratch.join("skills")).unwrap();
    let catalog = scratch.catalog();
    assert!(catalog.is_empty());
    assert_eq!(catalog.len(), 0);
    assert_eq!(catalog.diagnostics(), []);
}

#[test]
fn no_source_at_all_is_an_empty_catalog() {
    let catalog = SkillCatalog::discover(&SkillsConfig::new());
    assert!(catalog.is_empty());
    assert_eq!(catalog.diagnostics(), []);
}

#[test]
fn one_valid_skill_is_loaded_with_its_fields_and_location() {
    let scratch = Scratch::new("one");
    scratch.skill("skills/pdf-processing", "pdf-processing", "Handles PDFs.");
    let catalog = scratch.catalog();
    assert_eq!(catalog.diagnostics(), []);
    assert_eq!(catalog.len(), 1);
    let skill = &catalog.skills()[0];
    assert_eq!(skill.name(), "pdf-processing");
    assert_eq!(skill.description(), "Handles PDFs.");
    assert_eq!(skill.body(), "Do the thing.");
    assert_eq!(skill.scope(), SkillScope::Operator);
    assert_eq!(
        skill.location(),
        scratch.join("skills/pdf-processing/SKILL.md")
    );
    assert!(catalog.get("pdf-processing").is_some());
    assert!(catalog.get("other").is_none());
}

#[test]
fn a_source_that_cannot_be_listed_is_reported_and_start_up_goes_on() {
    let scratch = Scratch::new("unlistable");
    scratch.write("a-file", "not a directory");
    scratch.skill("good/ok", "ok", "Fine.");
    let config = SkillsConfig::new()
        .with_source(SkillSource::operator(scratch.join("missing")))
        .with_source(SkillSource::operator(scratch.join("a-file")))
        .with_source(SkillSource::operator(scratch.join("good")));
    let catalog = SkillCatalog::discover(&config);
    assert_eq!(names(&catalog), ["ok"]);
    assert_eq!(
        kinds(&catalog),
        [
            (Severity::Error, DiagnosticKind::SourceUnreadable),
            (Severity::Error, DiagnosticKind::SourceUnreadable)
        ]
    );
    assert_eq!(catalog.diagnostics()[0].path(), scratch.join("missing"));
    assert!(
        catalog.diagnostics()[0]
            .message()
            .starts_with("the directory cannot be listed (")
    );
}

#[test]
fn entries_that_are_not_skill_directories_are_ignored_or_noted() {
    let scratch = Scratch::new("entries");
    scratch.skill("skills/real", "real", "Real.");
    scratch.write("skills/README.md", "# not a skill");
    scratch.skill("skills/.hidden", "hidden", "Hidden.");
    scratch.write("skills/empty-dir/notes.txt", "no SKILL.md here");
    scratch.write(
        "skills/other-dir/references/only.md",
        "references and no SKILL.md",
    );
    let catalog = scratch.catalog();
    assert_eq!(names(&catalog), ["real"]);
    let said = catalog.diagnostics();
    assert_eq!(said.len(), 2, "{said:?}");
    assert!(
        said.iter()
            .all(|d| (d.severity(), d.kind()) == (Severity::Info, DiagnosticKind::NoSkillFile))
    );
    assert_eq!(said[0].path(), scratch.join("skills/empty-dir"));
    assert_eq!(said[1].path(), scratch.join("skills/other-dir"));
    assert_eq!(
        said[0].message(),
        "no SKILL.md here (the name is case-sensitive); ignored"
    );
}

#[test]
fn skills_are_in_name_order_whatever_order_they_were_made_in() {
    let scratch = Scratch::new("order");
    for name in ["zebra", "apple", "mango", "banana"] {
        scratch.skill(&format!("skills/{name}"), name, "d");
    }
    assert_eq!(
        names(&scratch.catalog()),
        ["apple", "banana", "mango", "zebra"]
    );
}

#[test]
fn a_name_that_is_not_its_directorys_is_used_and_reported() {
    let scratch = Scratch::new("mismatch");
    scratch.skill("skills/old-name", "new-name", "Renamed.");
    let catalog = scratch.catalog();
    assert_eq!(names(&catalog), ["new-name"]);
    assert_eq!(
        kinds(&catalog),
        [(Severity::Warning, DiagnosticKind::NameMismatch)]
    );
    assert_eq!(
        catalog.diagnostics()[0].path(),
        scratch.join("skills/old-name/SKILL.md")
    );
    assert_eq!(
        catalog.get("new-name").unwrap().location(),
        scratch.join("skills/old-name/SKILL.md")
    );
}

#[test]
fn malformed_frontmatter_is_skipped_with_the_reason_and_the_rest_loads() {
    let scratch = Scratch::new("malformed");
    scratch.skill("skills/a-good", "a-good", "Good.");
    scratch.write(
        "skills/b-bad/SKILL.md",
        "---\nname: b-bad\nthis is not yaml\n---\nbody",
    );
    scratch.write(
        "skills/c-open/SKILL.md",
        "---\nname: c-open\ndescription: d\nnever closed",
    );
    scratch.write("skills/d-none/SKILL.md", "# just markdown");
    scratch.write("skills/e-nodesc/SKILL.md", "---\nname: e-nodesc\n---\nbody");
    scratch.skill("skills/f-good", "f-good", "Good.");
    let catalog = scratch.catalog();
    assert_eq!(names(&catalog), ["a-good", "f-good"]);
    assert_eq!(
        kinds(&catalog),
        [
            (Severity::Error, DiagnosticKind::FrontmatterInvalid),
            (Severity::Error, DiagnosticKind::FrontmatterUnclosed),
            (Severity::Error, DiagnosticKind::FrontmatterMissing),
            (Severity::Error, DiagnosticKind::FieldMissing),
        ]
    );
    assert_eq!(
        catalog.diagnostics()[0].path(),
        scratch.join("skills/b-bad/SKILL.md")
    );
}

#[test]
fn a_skill_md_that_is_not_utf8_is_skipped() {
    let scratch = Scratch::new("utf8");
    scratch.write(
        "skills/bin/SKILL.md",
        b"---\nname: bin\ndescription: \xff\xfe\n---\nbody",
    );
    let catalog = scratch.catalog();
    assert!(catalog.is_empty());
    assert_eq!(
        kinds(&catalog),
        [(Severity::Error, DiagnosticKind::NotUtf8)]
    );
    assert_eq!(
        catalog.diagnostics()[0].message(),
        "SKILL.md is not UTF-8; skipped"
    );
}

#[test]
fn a_skill_md_that_cannot_be_read_is_skipped_with_the_reason() {
    let scratch = Scratch::new("unreadable");
    // A directory where the file should be: the open fails whoever runs this.
    scratch.write("skills/dir/SKILL.md/inner", "");
    scratch.skill("skills/fine", "fine", "Fine.");
    let catalog = scratch.catalog();
    assert_eq!(names(&catalog), ["fine"]);
    assert_eq!(
        kinds(&catalog),
        [(Severity::Error, DiagnosticKind::SkillFileUnreadable)]
    );
    assert_eq!(
        catalog.diagnostics()[0].message(),
        "that is not a regular file"
    );
}

#[cfg(unix)]
#[test]
fn a_directory_whose_permissions_forbid_reading_is_skipped_unless_the_reader_is_root() {
    use std::os::unix::fs::PermissionsExt;
    let scratch = Scratch::new("chmod");
    scratch.skill("skills/locked", "locked", "Locked.");
    scratch.skill("skills/open", "open", "Open.");
    let locked = scratch.join("skills/locked");
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();
    let readable_anyway = std::fs::read_dir(&locked).is_ok();
    let catalog = scratch.catalog();
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).unwrap();
    if readable_anyway {
        eprintln!(
            "running as a user the permission bits do not bind; the other tests cover the same branches"
        );
        return;
    }
    assert_eq!(names(&catalog), ["open"]);
    assert_eq!(catalog.diagnostics().len(), 1);
    assert_eq!(catalog.diagnostics()[0].severity(), Severity::Error);
}
