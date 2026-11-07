// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Part of the tests in `mod.rs`.

use super::*;

#[test]
fn a_duplicate_name_keeps_the_first_in_directory_order() {
    let scratch = Scratch::new("dup");
    scratch.skill("skills/a-first", "same", "First.");
    scratch.skill("skills/b-second", "same", "Second.");
    let catalog = scratch.catalog();
    assert_eq!(catalog.len(), 1);
    assert_eq!(catalog.skills()[0].description(), "First.");
    let said = catalog.diagnostics();
    // The name is not either directory's: one mismatch for each, then the
    // drop of the second.
    assert_eq!(
        said.iter().map(SkillDiagnostic::kind).collect::<Vec<_>>(),
        [
            DiagnosticKind::NameMismatch,
            DiagnosticKind::NameMismatch,
            DiagnosticKind::DuplicateName
        ]
    );
    assert_eq!(said[2].severity(), Severity::Warning);
    assert_eq!(said[2].path(), scratch.join("skills/b-second/SKILL.md"));
    assert_eq!(
        said[2].message(),
        format!(
            "a skill named 'same' is already loaded from {}; this one is dropped",
            scratch.join("skills/a-first/SKILL.md").display()
        )
    );
}

#[test]
fn an_operators_skill_is_never_shadowed_by_a_projects_even_when_listed_after() {
    let scratch = Scratch::new("shadow");
    scratch.skill("project/deploy", "deploy", "The repository's own deploy.");
    scratch.skill("operator/deploy", "deploy", "The operator's deploy.");
    scratch.skill("project/extra", "extra", "Only the project has it.");
    let config = SkillsConfig::new()
        .with_source(SkillSource::project(scratch.join("project")).with_trusted(true))
        .with_source(SkillSource::operator(scratch.join("operator")));
    let catalog = SkillCatalog::discover(&config);
    assert_eq!(names(&catalog), ["deploy", "extra"]);
    let deploy = catalog.get("deploy").unwrap();
    assert_eq!(deploy.description(), "The operator's deploy.");
    assert_eq!(deploy.scope(), SkillScope::Operator);
    assert_eq!(catalog.get("extra").unwrap().scope(), SkillScope::Project);
    assert_eq!(
        kinds(&catalog),
        [(Severity::Warning, DiagnosticKind::DuplicateName)]
    );
}

#[test]
fn a_project_directory_is_not_read_until_it_is_marked_trusted() {
    let scratch = Scratch::new("trust");
    scratch.skill("project/theirs", "theirs", "From the repository.");
    // Not even looked into: a malformed file would be reported if it were.
    scratch.write("project/broken/SKILL.md", "garbage");
    let untrusted = SkillsConfig::new().with_source(SkillSource::project(scratch.join("project")));
    let catalog = SkillCatalog::discover(&untrusted);
    assert!(catalog.is_empty());
    assert_eq!(
        kinds(&catalog),
        [(Severity::Warning, DiagnosticKind::SourceNotTrusted)]
    );
    assert_eq!(catalog.diagnostics()[0].path(), scratch.join("project"));
    assert_eq!(
        catalog.diagnostics()[0].message(),
        "this directory is not marked trusted, so nothing was read from it"
    );

    let trusted = SkillsConfig::new()
        .with_source(SkillSource::project(scratch.join("project")).with_trusted(true));
    let catalog = SkillCatalog::discover(&trusted);
    assert_eq!(names(&catalog), ["theirs"]);
    assert_eq!(catalog.get("theirs").unwrap().scope(), SkillScope::Project);

    // And trust can be withdrawn from an operator directory too.
    scratch.skill("operator/mine", "mine", "Mine.");
    let withdrawn = SkillsConfig::new()
        .with_source(SkillSource::operator(scratch.join("operator")).with_trusted(false));
    assert!(SkillCatalog::discover(&withdrawn).is_empty());
}

#[test]
fn three_hundred_skills_make_a_catalog_of_32_and_one_diagnostic() {
    let scratch = Scratch::new("many");
    // The 32 that fit are valid; the rest are not even YAML. They are not
    // read, so they are not reported on.
    for i in 0..300 {
        let name = format!("skill-{i:03}");
        if i < 32 {
            scratch.skill(&format!("skills/{name}"), &name, "d");
        } else {
            scratch.write(&format!("skills/{name}/SKILL.md"), "garbage");
        }
    }
    let catalog = scratch.catalog();
    assert_eq!(catalog.len(), 32);
    assert_eq!(catalog.skills()[0].name(), "skill-000");
    assert_eq!(catalog.skills()[31].name(), "skill-031");
    assert_eq!(
        kinds(&catalog),
        [(Severity::Warning, DiagnosticKind::OverSkillCap)]
    );
    assert_eq!(
        catalog.diagnostics()[0].message(),
        "268 more directories were not read: the catalog holds at most 32 skills"
    );
    assert_eq!(catalog.diagnostics()[0].path(), scratch.join("skills"));
}

#[test]
fn the_skill_cap_is_exact() {
    for (made, expected_len, expected_said) in [
        (2, 2, None),
        (3, 3, None),
        (
            4,
            3,
            Some("1 more directories were not read: the catalog holds at most 3 skills"),
        ),
        (
            6,
            3,
            Some("3 more directories were not read: the catalog holds at most 3 skills"),
        ),
    ] {
        let scratch = Scratch::new("cap");
        for i in 0..made {
            let name = format!("s{i}");
            scratch.skill(&format!("skills/{name}"), &name, "d");
        }
        let catalog = SkillCatalog::discover(&scratch.config().with_max_skills(3));
        assert_eq!(catalog.len(), expected_len, "{made} made");
        assert_eq!(
            catalog
                .diagnostics()
                .iter()
                .map(SkillDiagnostic::message)
                .collect::<Vec<_>>(),
            expected_said.into_iter().collect::<Vec<_>>(),
            "{made} made"
        );
    }
}

#[test]
fn a_skill_that_does_not_load_does_not_count_towards_the_cap() {
    let scratch = Scratch::new("capbad");
    scratch.write("skills/a-bad/SKILL.md", "garbage");
    scratch.skill("skills/b", "b", "d");
    scratch.skill("skills/c", "c", "d");
    let catalog = SkillCatalog::discover(&scratch.config().with_max_skills(2));
    assert_eq!(names(&catalog), ["b", "c"]);
    assert_eq!(
        kinds(&catalog),
        [(Severity::Error, DiagnosticKind::FrontmatterMissing)]
    );
}

#[test]
fn a_source_after_the_cap_is_filled_is_not_read() {
    let scratch = Scratch::new("capsources");
    scratch.skill("one/a", "a", "d");
    scratch.skill("two/b", "b", "d");
    let config = SkillsConfig::new()
        .with_max_skills(1)
        .with_source(SkillSource::operator(scratch.join("one")))
        .with_source(SkillSource::operator(scratch.join("two")));
    let catalog = SkillCatalog::discover(&config);
    assert_eq!(names(&catalog), ["a"]);
    assert_eq!(
        kinds(&catalog),
        [(Severity::Warning, DiagnosticKind::OverSkillCap)]
    );
    assert_eq!(catalog.diagnostics()[0].path(), scratch.join("two"));
    assert_eq!(
        catalog.diagnostics()[0].message(),
        "not read: the catalog already holds its 1 skills"
    );
    let none = SkillCatalog::discover(
        &SkillsConfig::new()
            .with_max_skills(0)
            .with_source(SkillSource::operator(scratch.join("one"))),
    );
    assert!(none.is_empty());
    assert_eq!(none.diagnostics().len(), 1);
    assert_eq!(
        none.diagnostics()[0].message(),
        "not read: the catalog already holds its 0 skills"
    );
}

#[test]
fn a_directory_with_more_than_1024_entries_is_examined_to_1024() {
    let scratch = Scratch::new("entries1024");
    scratch.skill("skills/real", "real", "d");
    std::fs::create_dir_all(scratch.join("skills")).unwrap();
    for i in 0..1023 {
        std::fs::write(scratch.join(&format!("skills/file-{i:04}")), "").unwrap();
    }
    // 1 skill directory + 1023 files = 1024 entries: all examined.
    let catalog = scratch.catalog();
    assert_eq!(names(&catalog), ["real"]);
    assert_eq!(catalog.diagnostics(), []);
    // One more entry: one is not examined, and it is said.
    std::fs::write(scratch.join("skills/file-1023"), "").unwrap();
    let catalog = scratch.catalog();
    assert_eq!(
        kinds(&catalog),
        [(Severity::Warning, DiagnosticKind::ScanTruncated)]
    );
    assert_eq!(
        catalog.diagnostics()[0].message(),
        "only the first 1024 entries of the directory were examined"
    );
}
