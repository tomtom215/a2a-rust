// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The confined reader: each way a path is refused, and the one way it is
//! read.

use super::*;
use crate::skills::test_support::Scratch;

/// A scratch directory holding a skill at `skills/demo` with a few files, and
/// the skill's resolved directory.
fn skill_dir(scratch: &Scratch) -> PathBuf {
    scratch.write(
        "skills/demo/SKILL.md",
        "---\nname: demo\ndescription: d\n---\nbody\n",
    );
    scratch.write("skills/demo/references/guide.md", "the guide");
    scratch.write("skills/demo/references/deep/er/note.md", "deeper");
    scratch.write("skills/demo/assets/template.txt", "a template");
    scratch.write("skills/demo/scripts/run.sh", "#!/bin/sh\necho no\n");
    scratch.write("outside.txt", "not the skill's");
    scratch
        .join("skills/demo")
        .canonicalize()
        .expect("resolves")
}

fn read(root: &Path, path: &str) -> Result<String, Refusal> {
    read_text(root, root, path, 1024)
}

mod disk;
mod text;
#[cfg(unix)]
mod unix;
mod verify;
