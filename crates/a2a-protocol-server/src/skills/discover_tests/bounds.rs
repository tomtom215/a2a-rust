// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Part of the tests in `mod.rs`.

use super::*;

#[test]
fn the_frontmatter_and_body_bounds_decide_what_a_file_may_be() {
    // With a 100-byte body bound the file is read to 8192 + 100 + 64 = 8356
    // bytes. Within it a body over 100 is a body error; past it, a file error.
    let scratch = Scratch::new("filebound");
    let head = "---\nname: x\ndescription: d\n---\n";
    let write = |directory: &str, total: usize| {
        // The name is the directory's, one character as `x` was.
        let head = head.replace("name: x", &format!("name: {directory}"));
        scratch.write(
            &format!("skills/{directory}/SKILL.md"),
            format!("{head}{}", "b".repeat(total - head.len())),
        );
    };
    write("a", 8355);
    write("b", 8356);
    write("c", 8357);
    let config = scratch.config().with_max_body_bytes(100);
    let catalog = SkillCatalog::discover(&config);
    assert!(catalog.is_empty());
    assert_eq!(
        kinds(&catalog),
        [
            (Severity::Error, DiagnosticKind::BodyTooLarge),
            (Severity::Error, DiagnosticKind::BodyTooLarge),
            (Severity::Error, DiagnosticKind::SkillFileTooLarge),
        ]
    );
    assert_eq!(
        catalog.diagnostics()[2].message(),
        "SKILL.md is over 8356 bytes (a frontmatter of at most 8192 and a body of at most 100); skipped"
    );
}

#[test]
fn a_body_bound_is_exact_through_discovery() {
    let scratch = Scratch::new("bodybound");
    let head = "---\nname: x\ndescription: d\n---\n";
    for (directory, body) in [("under", 999), ("at", 1000), ("over", 1001)] {
        let head = head.replace("name: x", &format!("name: {directory}"));
        scratch.write(
            &format!("skills/{directory}/SKILL.md"),
            format!("{head}{}", "b".repeat(body)),
        );
    }
    let catalog = SkillCatalog::discover(&scratch.config().with_max_body_bytes(1000));
    assert_eq!(names(&catalog), ["at", "under"]);
    assert_eq!(catalog.get("at").unwrap().body().len(), 1000);
    assert_eq!(catalog.get("under").unwrap().body().len(), 999);
    assert_eq!(
        kinds(&catalog),
        [(Severity::Error, DiagnosticKind::BodyTooLarge)]
    );
    assert_eq!(
        catalog.diagnostics()[0].path(),
        scratch.join("skills/over/SKILL.md")
    );
}

#[test]
fn an_alias_bomb_and_a_huge_value_are_each_skipped_without_trouble() {
    let scratch = Scratch::new("hostile");
    let mut bomb = String::from("---\nname: bomb\ndescription: d\nlol0: &a0 [\"lol\",\"lol\"]\n");
    for level in 1..30 {
        let _ = writeln!(
            bomb,
            "lol{level}: &a{level} [*a{p},*a{p},*a{p},*a{p},*a{p},*a{p},*a{p},*a{p}]",
            p = level - 1
        );
    }
    bomb.push_str("---\nbody");
    scratch.write("skills/bomb/SKILL.md", bomb);
    scratch.write(
        "skills/huge/SKILL.md",
        format!(
            "---\nname: huge\ndescription: {}\n---\nbody",
            "x".repeat(2_000_000)
        ),
    );
    scratch.skill("skills/ok", "ok", "Fine.");
    let catalog = scratch.catalog();
    assert_eq!(
        names(&catalog),
        ["bomb", "ok"],
        "the bomb's fields are only unknown ones"
    );
    assert!(
        catalog
            .diagnostics()
            .iter()
            .any(|d| d.kind() == DiagnosticKind::FrontmatterTooLarge
                || d.kind() == DiagnosticKind::SkillFileTooLarge)
    );
    assert!(catalog.get("huge").is_none());
}

#[test]
fn unicode_names_and_directories_are_loaded() {
    let scratch = Scratch::new("unicode");
    scratch.skill("skills/名前", "名前", "日本語の説明。");
    scratch.skill("skills/émile", "émile", "Accented.");
    // A directory in one normalization form and a name in another is a
    // mismatch, which is reported and used.
    scratch.skill("skills/e\u{301}cole", "\u{e9}cole", "Decomposed directory.");
    let catalog = scratch.catalog();
    assert_eq!(catalog.len(), 3);
    assert!(catalog.get("名前").is_some());
    assert_eq!(catalog.get("名前").unwrap().description(), "日本語の説明。");
    // Some file systems store a name in a form of their own, so which of the
    // directories are reported is theirs to say; all that may be said is a
    // mismatch, and the decomposed one is among them.
    let said = catalog.diagnostics();
    assert!(
        said.iter()
            .all(|d| d.kind() == DiagnosticKind::NameMismatch),
        "{said:?}"
    );
    assert!(
        said.iter()
            .any(|d| d.path().to_string_lossy().contains("cole"))
    );
}

#[test]
fn bounds_set_to_the_largest_value_overflow_nothing() {
    let scratch = Scratch::new("hugebounds");
    scratch.skill("skills/demo", "demo", "A description.");
    scratch.write("skills/demo/references/r.md", "text");
    let config = scratch
        .config()
        .with_max_body_bytes(usize::MAX)
        .with_max_resource_bytes(usize::MAX)
        .with_max_skills(usize::MAX)
        .with_max_description_chars(usize::MAX);
    let catalog = SkillCatalog::discover(&config);
    assert_eq!(names(&catalog), ["demo"]);
    assert_eq!(catalog.diagnostics(), []);
    assert_eq!(
        catalog.get("demo").unwrap().catalog_description,
        "A description."
    );
}
