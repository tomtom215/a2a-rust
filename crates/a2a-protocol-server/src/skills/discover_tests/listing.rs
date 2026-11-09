// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Part of the tests in `mod.rs`.

use super::*;

#[test]
fn files_under_the_skill_are_listed_and_scripts_are_noted_as_not_run() {
    let scratch = Scratch::new("listing");
    scratch.skill("skills/demo", "demo", "d");
    scratch.write("skills/demo/references/b.md", "");
    scratch.write("skills/demo/references/a.md", "");
    scratch.write("skills/demo/references/sub/c.md", "");
    scratch.write("skills/demo/assets/logo.txt", "");
    scratch.write("skills/demo/scripts/run.sh", "");
    scratch.write("skills/demo/scripts/more/x.py", "");
    scratch.write("skills/demo/other/ignored.md", "");
    scratch.write("skills/demo/NOTES.md", "");
    let catalog = scratch.catalog();
    let skill = catalog.get("demo").unwrap();
    assert_eq!(
        skill.resources(),
        [
            "references/a.md",
            "references/b.md",
            "references/sub/c.md",
            "assets/logo.txt"
        ]
    );
    assert_eq!(skill.scripts(), ["scripts/more/x.py", "scripts/run.sh"]);
    assert_eq!(
        kinds(&catalog),
        [(Severity::Info, DiagnosticKind::ScriptsNotRun)]
    );
    assert_eq!(
        catalog.diagnostics()[0].message(),
        "scripts/ holds 2 file(s), listed to the model as present; this module never runs them (ADR 0035)"
    );
    assert_eq!(
        catalog.diagnostics()[0].path(),
        scratch.join("skills/demo/SKILL.md")
    );
}

#[test]
fn a_skill_with_no_extra_files_lists_none_and_says_nothing() {
    let scratch = Scratch::new("nolisting");
    scratch.skill("skills/demo", "demo", "d");
    let catalog = scratch.catalog();
    let skill = catalog.get("demo").unwrap();
    assert_eq!(skill.resources(), Vec::<String>::new());
    assert_eq!(skill.scripts(), Vec::<String>::new());
    assert_eq!(catalog.diagnostics(), []);
}

#[test]
fn a_listing_stops_at_100_files_in_a_directory_and_says_so() {
    let scratch = Scratch::new("list100");
    scratch.skill("skills/demo", "demo", "d");
    for i in 0..100 {
        scratch.write(&format!("skills/demo/references/f{i:03}.md"), "");
    }
    let catalog = scratch.catalog();
    assert_eq!(catalog.get("demo").unwrap().resources().len(), 100);
    assert_eq!(catalog.diagnostics(), [], "exactly 100 is not truncated");

    scratch.write("skills/demo/references/f100.md", "");
    let catalog = scratch.catalog();
    assert_eq!(catalog.get("demo").unwrap().resources().len(), 100);
    assert_eq!(
        kinds(&catalog),
        [(Severity::Info, DiagnosticKind::ListingTruncated)]
    );
    assert_eq!(
        catalog.diagnostics()[0].message(),
        "a listing of the skill's files stopped at 100 files in a directory, or at 3 levels"
    );
}

#[test]
fn each_of_the_three_directories_has_its_own_100() {
    let scratch = Scratch::new("list300");
    scratch.skill("skills/demo", "demo", "d");
    for i in 0..100 {
        scratch.write(&format!("skills/demo/references/f{i:03}.md"), "");
        scratch.write(&format!("skills/demo/assets/f{i:03}.md"), "");
        scratch.write(&format!("skills/demo/scripts/f{i:03}.sh"), "");
    }
    let catalog = scratch.catalog();
    let skill = catalog.get("demo").unwrap();
    assert_eq!((skill.resources().len(), skill.scripts().len()), (200, 100));
    assert_eq!(
        kinds(&catalog),
        [(Severity::Info, DiagnosticKind::ScriptsNotRun)]
    );
}

#[test]
fn a_listing_goes_three_directories_deep_and_no_further() {
    let scratch = Scratch::new("depth");
    scratch.skill("skills/demo", "demo", "d");
    scratch.write("skills/demo/references/one.md", "");
    scratch.write("skills/demo/references/a/two.md", "");
    scratch.write("skills/demo/references/a/b/three.md", "");
    scratch.write("skills/demo/references/a/b/c/four.md", "");
    let catalog = scratch.catalog();
    assert_eq!(
        catalog.get("demo").unwrap().resources(),
        [
            "references/a/b/three.md",
            "references/a/two.md",
            "references/one.md"
        ]
    );
    assert_eq!(
        kinds(&catalog),
        [(Severity::Info, DiagnosticKind::ListingTruncated)]
    );
}

#[test]
fn the_catalog_shows_a_description_shortened_to_the_bound_and_says_so() {
    let scratch = Scratch::new("shorten");
    scratch.skill("skills/at", "at", &"d".repeat(400));
    scratch.skill("skills/over", "over", &"d".repeat(401));
    let catalog = scratch.catalog();
    assert_eq!(
        catalog.get("at").unwrap().catalog_description,
        "d".repeat(400)
    );
    assert!(!catalog.get("at").unwrap().shortened);
    let over = catalog.get("over").unwrap();
    assert!(over.shortened);
    assert_eq!(over.catalog_description, format!("{}…", "d".repeat(399)));
    assert_eq!(
        over.description().len(),
        401,
        "the whole description is kept"
    );
    assert_eq!(
        kinds(&catalog),
        [(Severity::Info, DiagnosticKind::DescriptionShortened)]
    );
    assert_eq!(
        catalog.diagnostics()[0].message(),
        "the catalog shows the first 400 characters of the description; the activation result carries all of it"
    );
    let tight = SkillCatalog::discover(&scratch.config().with_max_description_chars(0));
    assert_eq!(tight.get("at").unwrap().catalog_description, "…");
}

#[cfg(unix)]
mod links {
    use super::*;
    use crate::skills::test_support::symlink;

    #[test]
    fn a_link_to_a_skill_directory_is_not_followed_unless_the_source_says_so() {
        let scratch = Scratch::new("follow");
        scratch.skill("shared/linked", "linked", "Linked in.");
        std::fs::create_dir_all(scratch.join("skills")).unwrap();
        symlink(
            &scratch.join("shared/linked"),
            &scratch.join("skills/linked"),
        );
        let catalog = scratch.catalog();
        assert!(catalog.is_empty());
        assert_eq!(
            kinds(&catalog),
            [(Severity::Warning, DiagnosticKind::SymlinkNotFollowed)]
        );
        assert_eq!(
            catalog.diagnostics()[0].path(),
            scratch.join("skills/linked")
        );

        let followed = SkillsConfig::new()
            .with_source(SkillSource::operator(scratch.join("skills")).with_follow_symlinks(true));
        let catalog = SkillCatalog::discover(&followed);
        assert_eq!(names(&catalog), ["linked"]);
        assert_eq!(catalog.diagnostics(), []);
        assert_eq!(
            catalog.get("linked").unwrap().location(),
            scratch.join("skills/linked/SKILL.md"),
            "reported at the path it was configured under"
        );
    }

    #[test]
    fn a_followed_link_still_cannot_take_a_read_outside_the_skill() {
        let scratch = Scratch::new("followleak");
        scratch.skill("shared/linked", "linked", "d");
        scratch.write("shared/linked/references/ok.md", "fine");
        scratch.write("shared/secret.txt", "secret");
        symlink(
            &scratch.join("shared/secret.txt"),
            &scratch.join("shared/linked/references/leak.md"),
        );
        std::fs::create_dir_all(scratch.join("skills")).unwrap();
        symlink(
            &scratch.join("shared/linked"),
            &scratch.join("skills/linked"),
        );
        let config = SkillsConfig::new()
            .with_source(SkillSource::operator(scratch.join("skills")).with_follow_symlinks(true));
        let catalog = SkillCatalog::discover(&config);
        let skill = catalog.get("linked").unwrap();
        assert_eq!(
            skill.resources(),
            ["references/ok.md"],
            "a link is not listed"
        );
        let read = |path: &str| super::reader::read_text(&skill.directory, &skill.root, path, 1024);
        assert_eq!(read("references/ok.md").as_deref(), Ok("fine"));
        assert_eq!(read("references/leak.md"), Err(super::Refusal::Escapes));
    }

    #[test]
    fn links_that_lead_nowhere_or_nowhere_useful_are_said_or_ignored() {
        let scratch = Scratch::new("badlinks");
        scratch.skill("skills/real", "real", "d");
        scratch.write("elsewhere/README.md", "x");
        std::fs::create_dir_all(scratch.join("elsewhere/plain-dir")).unwrap();
        symlink(Path::new("nowhere"), &scratch.join("skills/dangling"));
        symlink(Path::new("loop-b"), &scratch.join("skills/loop-a"));
        symlink(Path::new("loop-a"), &scratch.join("skills/loop-b"));
        symlink(
            &scratch.join("elsewhere/README.md"),
            &scratch.join("skills/readme-link"),
        );
        symlink(
            &scratch.join("elsewhere/plain-dir"),
            &scratch.join("skills/dir-without-skill"),
        );
        for follow in [false, true] {
            let config = SkillsConfig::new().with_source(
                SkillSource::operator(scratch.join("skills")).with_follow_symlinks(follow),
            );
            let catalog = SkillCatalog::discover(&config);
            assert_eq!(names(&catalog), ["real"], "follow {follow}");
            let said: Vec<_> = catalog
                .diagnostics()
                .iter()
                .map(|d| {
                    (
                        d.kind(),
                        d.path().file_name().unwrap().to_str().unwrap().to_owned(),
                    )
                })
                .collect();
            // The links are dealt with while the directory is listed, before
            // any skill in it is read, so a followed link's missing SKILL.md
            // is said last.
            let expected = if follow {
                vec![
                    (DiagnosticKind::BrokenSymlink, "dangling".to_owned()),
                    (DiagnosticKind::BrokenSymlink, "loop-a".to_owned()),
                    (DiagnosticKind::BrokenSymlink, "loop-b".to_owned()),
                    (DiagnosticKind::NoSkillFile, "dir-without-skill".to_owned()),
                ]
            } else {
                vec![
                    (DiagnosticKind::BrokenSymlink, "dangling".to_owned()),
                    (
                        DiagnosticKind::SymlinkNotFollowed,
                        "dir-without-skill".to_owned(),
                    ),
                    (DiagnosticKind::BrokenSymlink, "loop-a".to_owned()),
                    (DiagnosticKind::BrokenSymlink, "loop-b".to_owned()),
                ]
            };
            assert_eq!(said, expected, "follow {follow}");
        }
    }

    #[test]
    fn a_link_inside_a_resource_directory_that_loops_does_not_hang_the_listing() {
        let scratch = Scratch::new("listloop");
        scratch.skill("skills/demo", "demo", "d");
        scratch.write("skills/demo/references/real.md", "");
        symlink(Path::new(".."), &scratch.join("skills/demo/references/up"));
        symlink(Path::new("."), &scratch.join("skills/demo/references/here"));
        std::fs::create_dir_all(scratch.join("skills/demo/assets")).unwrap();
        symlink(Path::new("../.."), &scratch.join("skills/demo/assets/far"));
        let catalog = scratch.catalog();
        assert_eq!(
            catalog.get("demo").unwrap().resources(),
            ["references/real.md"]
        );
        assert_eq!(catalog.diagnostics(), []);
    }

    /// The entries inside a resource directory that are links are not listed;
    /// the directory itself being a link must not list what it points at
    /// either, or `activate_skill` would tell the model the names of files in
    /// a directory the skill's author chose (found by the live campaign,
    /// 2026-11-08: `references` linked to `/etc` listed 100 of its files).
    #[test]
    fn a_resource_directory_that_is_itself_a_link_is_not_listed_and_is_said() {
        let scratch = Scratch::new("linkeddir");
        scratch.skill("skills/demo", "demo", "d");
        scratch.write("elsewhere/name-of-a-file-outside.txt", "");
        scratch.write("skills/demo/assets/logo.txt", "");
        symlink(
            &scratch.join("elsewhere"),
            &scratch.join("skills/demo/references"),
        );
        symlink(
            &scratch.join("elsewhere"),
            &scratch.join("skills/demo/scripts"),
        );
        let catalog = scratch.catalog();
        let skill = catalog.get("demo").unwrap();
        assert_eq!(skill.resources(), ["assets/logo.txt"]);
        assert_eq!(skill.scripts(), Vec::<String>::new());
        let said: Vec<_> = catalog
            .diagnostics()
            .iter()
            .map(|d| (d.severity(), d.kind(), d.message().to_owned()))
            .collect();
        let message = |dir: &str| {
            format!(
                "`{dir}/` is a symbolic link, so its files are neither listed nor readable (a \
                 link is how a directory someone else controls gets into a skill)"
            )
        };
        assert_eq!(
            said,
            [
                (
                    Severity::Warning,
                    DiagnosticKind::SymlinkNotFollowed,
                    message("references")
                ),
                (
                    Severity::Warning,
                    DiagnosticKind::SymlinkNotFollowed,
                    message("scripts")
                ),
            ]
        );
    }

    /// A link to a directory inside the skill is a link too: the reader would
    /// refuse every read through it (the path leaves `references/`), so
    /// listing it would offer files that cannot be read.
    #[test]
    fn a_resource_directory_linked_to_a_directory_inside_the_skill_is_not_listed() {
        let scratch = Scratch::new("linkedinside");
        scratch.skill("skills/demo", "demo", "d");
        scratch.write("skills/demo/docs/guide.md", "");
        symlink(Path::new("docs"), &scratch.join("skills/demo/references"));
        let catalog = scratch.catalog();
        let skill = catalog.get("demo").unwrap();
        assert_eq!(skill.resources(), Vec::<String>::new());
        assert_eq!(
            kinds(&catalog),
            [(Severity::Warning, DiagnosticKind::SymlinkNotFollowed)]
        );
    }
}
