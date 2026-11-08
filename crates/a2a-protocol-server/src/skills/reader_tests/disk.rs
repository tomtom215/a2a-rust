// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Part of the tests in `mod.rs`.

use super::*;

#[test]
fn a_file_under_references_or_assets_is_returned() {
    let scratch = Scratch::new("read");
    let root = skill_dir(&scratch);
    assert_eq!(
        read(&root, "references/guide.md").as_deref(),
        Ok("the guide")
    );
    assert_eq!(
        read(&root, "assets/template.txt").as_deref(),
        Ok("a template")
    );
    assert_eq!(
        read(&root, "references/deep/er/note.md").as_deref(),
        Ok("deeper")
    );
    assert_eq!(
        read(&root, "./references//guide.md").as_deref(),
        Ok("the guide")
    );
}

#[test]
fn scripts_and_skill_md_are_not_returned() {
    let scratch = Scratch::new("scripts");
    let root = skill_dir(&scratch);
    assert_eq!(
        read(&root, "scripts/run.sh"),
        Err(Refusal::OutsideReadableDirs)
    );
    assert_eq!(read(&root, "SKILL.md"), Err(Refusal::OutsideReadableDirs));
}

#[test]
fn a_path_that_leaves_the_directory_is_refused() {
    let scratch = Scratch::new("dots");
    let root = skill_dir(&scratch);
    for path in [
        "references/../../outside.txt",
        "../outside.txt",
        "references/../SKILL.md",
        "assets/..",
    ] {
        assert_eq!(read(&root, path), Err(Refusal::ParentDir), "{path}");
    }
    assert_eq!(read(&root, "/outside.txt"), Err(Refusal::Absolute));
    assert_eq!(read(&root, "/etc/passwd"), Err(Refusal::Absolute));
    assert_eq!(read(&root, "//host/share/x"), Err(Refusal::Absolute));
}

#[test]
fn a_file_that_is_not_there_is_not_found() {
    let scratch = Scratch::new("missing");
    let root = skill_dir(&scratch);
    assert_eq!(read(&root, "references/none.md"), Err(Refusal::NotFound));
    assert_eq!(read(&root, "assets/none/none.md"), Err(Refusal::NotFound));
}

#[test]
fn a_directory_is_not_a_file() {
    let scratch = Scratch::new("directory");
    let root = skill_dir(&scratch);
    assert_eq!(read(&root, "references"), Err(Refusal::NotAFile));
    assert_eq!(read(&root, "references/deep"), Err(Refusal::NotAFile));
    assert_eq!(read(&root, "assets/"), Err(Refusal::NotAFile));
}

// Unix only: a path below a regular file fails there with `ENOTDIR`, which is
// not "not found". Windows answers it with another kind (this assertion
// failed on both Windows CI jobs), so the mapping itself is tested below on
// every platform.
#[cfg(unix)]
#[test]
fn another_failure_to_resolve_is_unreadable_not_not_found() {
    let scratch = Scratch::new("notdir");
    let root = skill_dir(&scratch);
    // `guide.md` is a file, so nothing can be below it.
    assert!(matches!(
        read(&root, "references/guide.md/below"),
        Err(Refusal::Unreadable(_))
    ));
}

#[test]
fn only_a_missing_file_is_not_found() {
    assert_eq!(
        from_io(&io::Error::from(io::ErrorKind::NotFound)),
        Refusal::NotFound
    );
    for (kind, said) in [
        (io::ErrorKind::PermissionDenied, "permission denied"),
        (io::ErrorKind::NotADirectory, "not a directory"),
        (io::ErrorKind::InvalidInput, "invalid input parameter"),
    ] {
        assert_eq!(
            from_io(&io::Error::from(kind)),
            Refusal::Unreadable(said.to_owned())
        );
    }
}

#[test]
fn the_size_bound_is_exact() {
    let scratch = Scratch::new("size");
    let root = skill_dir(&scratch);
    scratch.write("skills/demo/references/at.txt", "x".repeat(100));
    scratch.write("skills/demo/references/over.txt", "x".repeat(101));
    scratch.write("skills/demo/references/under.txt", "x".repeat(99));
    scratch.write("skills/demo/references/empty.txt", "");
    let at = |path: &str, max: usize| read_text(&root, &root, path, max);
    assert_eq!(at("references/at.txt", 100).map(|t| t.len()), Ok(100));
    assert_eq!(at("references/under.txt", 100).map(|t| t.len()), Ok(99));
    assert_eq!(
        at("references/over.txt", 100),
        Err(Refusal::TooLarge { max: 100 })
    );
    assert_eq!(at("references/empty.txt", 0).as_deref(), Ok(""));
    assert_eq!(
        at("references/under.txt", 0),
        Err(Refusal::TooLarge { max: 0 })
    );
    assert_eq!(
        at("references/guide.md", 8),
        Err(Refusal::TooLarge { max: 8 })
    );
    assert_eq!(at("references/guide.md", 9).as_deref(), Ok("the guide"));
}

#[test]
fn a_file_that_is_not_utf8_is_refused_as_text() {
    let scratch = Scratch::new("binary");
    let root = skill_dir(&scratch);
    scratch.write(
        "skills/demo/assets/logo.png",
        [0x89, b'P', b'N', b'G', 0xff, 0xfe],
    );
    assert_eq!(read(&root, "assets/logo.png"), Err(Refusal::NotUtf8));
}

#[test]
fn a_skill_directory_that_is_not_the_one_found_is_refused() {
    let scratch = Scratch::new("moved");
    let root = skill_dir(&scratch);
    scratch.write("elsewhere/x", "");
    let elsewhere = scratch.join("elsewhere");
    assert_eq!(
        read_text(&elsewhere, &root, "references/guide.md", 1024),
        Err(Refusal::SkillMoved)
    );
    assert_eq!(
        read_text(&scratch.join("gone"), &root, "references/guide.md", 1024),
        Err(Refusal::SkillMoved)
    );
    assert_eq!(
        read_text(&root, &root, "references/guide.md", 1024).as_deref(),
        Ok("the guide")
    );
    // The text checks do not wait for the disk.
    assert_eq!(
        read_text(&elsewhere, &root, "../x", 1024),
        Err(Refusal::ParentDir)
    );
}

#[test]
fn skill_md_is_read_to_its_bound() {
    let scratch = Scratch::new("skillmd");
    let root = skill_dir(&scratch);
    let bytes = read_skill_file(&root, 1024).expect("reads");
    assert_eq!(bytes, b"---\nname: demo\ndescription: d\n---\nbody\n");
    assert_eq!(
        read_skill_file(&root, bytes.len()).as_deref(),
        Ok(&bytes[..])
    );
    assert_eq!(
        read_skill_file(&root, bytes.len() - 1),
        Err(Refusal::TooLarge {
            max: bytes.len() - 1
        })
    );
    scratch.write("skills/none/references/x", "");
    let none = scratch
        .join("skills/none")
        .canonicalize()
        .expect("resolves");
    assert_eq!(read_skill_file(&none, 1024), Err(Refusal::NotFound));
}

#[test]
fn a_skill_md_that_is_a_directory_is_not_a_file() {
    let scratch = Scratch::new("skillmddir");
    scratch.write("skills/demo/SKILL.md/inner", "");
    let root = scratch
        .join("skills/demo")
        .canonicalize()
        .expect("resolves");
    assert_eq!(read_skill_file(&root, 1024), Err(Refusal::NotAFile));
}

#[test]
fn a_size_bound_of_usize_max_overflows_nothing() {
    let scratch = Scratch::new("readhuge");
    let root = skill_dir(&scratch);
    assert_eq!(
        read_text(&root, &root, "references/guide.md", usize::MAX).as_deref(),
        Ok("the guide")
    );
    assert!(read_skill_file(&root, usize::MAX).is_ok());
}
