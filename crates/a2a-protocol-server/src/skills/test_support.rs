// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Helpers the skills tests share: a scratch directory that removes itself,
//! and the skills to put in one.

use std::fs;
#[cfg(unix)]
use std::path::Path;
use std::path::PathBuf;

use super::{SkillCatalog, SkillSource, SkillsConfig};

/// A directory under the system's temporary directory, removed on drop.
pub struct Scratch {
    root: PathBuf,
}

impl Scratch {
    pub fn new(tag: &str) -> Self {
        let root = std::env::temp_dir().join(format!("a2a-skills-{tag}-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).expect("a scratch directory");
        // Resolved, because the temporary directory is a link on some systems
        // (macOS's `/var`) and the code under test compares resolved paths.
        let root = root.canonicalize().expect("a resolvable scratch directory");
        Self { root }
    }

    pub fn join(&self, relative: &str) -> PathBuf {
        self.root.join(relative)
    }

    /// Writes `contents` to `relative`, making the directories above it.
    pub fn write(&self, relative: &str, contents: impl AsRef<[u8]>) -> PathBuf {
        let path = self.join(relative);
        fs::create_dir_all(path.parent().expect("a parent")).expect("a directory");
        fs::write(&path, contents).expect("a file");
        path
    }

    /// A skill `name` in `directory` (a path under the scratch directory),
    /// with `description`.
    pub fn skill(&self, directory: &str, name: &str, description: &str) -> PathBuf {
        self.write(
            &format!("{directory}/SKILL.md"),
            skill_text(name, description, "Do the thing."),
        )
    }

    /// The catalog of the scratch directory's `skills` as an operator
    /// directory.
    pub fn catalog(&self) -> SkillCatalog {
        SkillCatalog::discover(&self.config())
    }

    pub fn config(&self) -> SkillsConfig {
        SkillsConfig::new().with_source(SkillSource::operator(self.join("skills")))
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

/// A `SKILL.md`.
pub fn skill_text(name: &str, description: &str, body: &str) -> String {
    format!("---\nname: {name}\ndescription: {description}\n---\n{body}\n")
}

/// A symbolic link at `link` to `target`.
#[cfg(unix)]
pub fn symlink(target: &Path, link: &Path) {
    std::os::unix::fs::symlink(target, link).expect("a symbolic link");
}
