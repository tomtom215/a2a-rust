// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Part of the tests in `mod.rs`.

use super::*;

#[test]
fn a_path_that_is_not_its_own_resolution_is_caught_after_the_open() {
    let scratch = Scratch::new("verify");
    let root = skill_dir(&scratch);
    let boundary = root.join("references");
    let file = root.join("references/guide.md");
    let handle = File::open(&file).expect("opens");
    assert_eq!(verify(&handle, &file, &boundary), Ok(()));
    // Not in its resolved form.
    let roundabout = root.join("references/deep/../guide.md");
    assert_eq!(
        verify(&handle, &roundabout, &boundary),
        Err(Refusal::ChangedWhileOpening)
    );
    // Outside the boundary asked for.
    assert_eq!(
        verify(&handle, &file, &root.join("assets")),
        Err(Refusal::ChangedWhileOpening)
    );
    // Gone.
    assert_eq!(
        verify(&handle, &root.join("references/gone.md"), &boundary),
        Err(Refusal::ChangedWhileOpening)
    );
}

#[test]
fn what_the_model_is_told_holds_no_host_path_and_says_what_to_do() {
    let said = |r: Refusal| r.to_string();
    assert_eq!(said(Refusal::Empty), "the path is empty");
    assert_eq!(said(Refusal::Nul), "the path contains a NUL character");
    assert_eq!(
        said(Refusal::Backslash),
        "the path contains a backslash; use forward slashes"
    );
    assert_eq!(
        said(Refusal::Absolute),
        "the path is absolute; give a path relative to the skill's directory, such as references/REFERENCE.md"
    );
    assert_eq!(said(Refusal::ParentDir), "the path contains `..`");
    assert_eq!(
        said(Refusal::OutsideReadableDirs),
        "only files under references/ and assets/ can be read"
    );
    assert_eq!(said(Refusal::NotFound), "there is no such file");
    assert_eq!(
        said(Refusal::Unreadable("permission denied".to_owned())),
        "the file could not be read (permission denied)"
    );
    assert_eq!(
        said(Refusal::Escapes),
        "the path leaves the directory it may be read from (a symbolic link points outside it)"
    );
    assert_eq!(said(Refusal::NotAFile), "that is not a regular file");
    assert_eq!(
        said(Refusal::TooLarge { max: 65536 }),
        "the file is larger than the 65536 bytes this reader returns"
    );
    assert_eq!(
        said(Refusal::ChangedWhileOpening),
        "the file changed while it was being opened"
    );
    assert_eq!(
        said(Refusal::SkillMoved),
        "the skill's directory has changed since start-up; restart to load skills again"
    );
    assert_eq!(said(Refusal::NotUtf8), "the file is not UTF-8 text");
    assert_eq!(said(Refusal::Interrupted), "the read did not complete");
}
