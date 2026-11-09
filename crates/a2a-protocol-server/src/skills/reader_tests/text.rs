// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Part of the tests in `mod.rs`.

use super::*;

#[test]
fn a_path_is_refused_as_text_before_the_disk_is_asked() {
    assert_eq!(relative(""), Err(Refusal::Empty));
    assert_eq!(relative("."), Err(Refusal::Empty));
    assert_eq!(relative("./"), Err(Refusal::Empty));
    assert_eq!(relative("./."), Err(Refusal::Empty));
    assert_eq!(relative("references/a\0b"), Err(Refusal::Nul));
    assert_eq!(relative("\0"), Err(Refusal::Nul));
    assert_eq!(relative("references\\a.md"), Err(Refusal::Backslash));
    assert_eq!(relative("..\\x"), Err(Refusal::Backslash));
    assert_eq!(relative("/etc/passwd"), Err(Refusal::Absolute));
    assert_eq!(relative("/"), Err(Refusal::Absolute));
    for parent in [
        "..",
        "../x",
        "references/../x",
        "references/..",
        "a/b/../../..",
        "./..",
    ] {
        assert_eq!(relative(parent), Err(Refusal::ParentDir), "{parent}");
    }
}

#[test]
fn a_clean_path_loses_its_dots_and_doubled_separators() {
    assert_eq!(
        relative("references/a.md"),
        Ok(PathBuf::from("references").join("a.md"))
    );
    assert_eq!(
        relative("./references//deep/./a.md"),
        Ok(PathBuf::from("references").join("deep").join("a.md"))
    );
    assert_eq!(relative("references/"), Ok(PathBuf::from("references")));
    assert_eq!(
        relative("a.md.."),
        Ok(PathBuf::from("a.md..")),
        "a name that merely ends in dots"
    );
    assert_eq!(relative("..."), Ok(PathBuf::from("...")));
}

#[cfg(windows)]
#[test]
fn a_drive_or_share_prefix_is_absolute() {
    assert_eq!(relative("C:foo"), Err(Refusal::Absolute));
    assert_eq!(relative("C:\\foo"), Err(Refusal::Backslash));
    assert_eq!(relative("//server/share/x"), Err(Refusal::Absolute));
}

#[test]
fn only_references_and_assets_are_readable() {
    assert_eq!(
        readable("references/a.md"),
        Ok(("references", PathBuf::from("references").join("a.md")))
    );
    assert_eq!(
        readable("./assets/x/y.png"),
        Ok(("assets", PathBuf::from("assets").join("x").join("y.png")))
    );
    assert_eq!(
        readable("references"),
        Ok(("references", PathBuf::from("references")))
    );
    for outside in [
        "scripts/run.sh",
        "SKILL.md",
        "README.md",
        "referencesx/a",
        "References/a",
        "a/references/b",
        "assets2/a",
    ] {
        assert_eq!(
            readable(outside),
            Err(Refusal::OutsideReadableDirs),
            "{outside}"
        );
    }
    // The text checks come first.
    assert_eq!(readable("/references/a"), Err(Refusal::Absolute));
    assert_eq!(readable("scripts/../references/a"), Err(Refusal::ParentDir));
    assert_eq!(readable(""), Err(Refusal::Empty));
}
