// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! What to scan and how much to keep: [`SkillsConfig`] and [`SkillSource`].

use std::path::{Path, PathBuf};

/// The most skills a catalog holds when no other bound is set.
///
/// Thirty-two descriptions of [`DEFAULT_MAX_DESCRIPTION_CHARS`] are at most
/// 12,800 characters of prompt, about 3,200 tokens at four characters a token
/// (an estimate, not a count).
pub const DEFAULT_MAX_SKILLS: usize = 32;

/// The most characters of a description the prompt's catalog shows when no
/// other bound is set.
///
/// The specification sizes the catalog at about 100 tokens a skill, and a
/// description is most of that: 400 characters is about 100 tokens at four a
/// token. The whole description is in the activation result.
pub const DEFAULT_MAX_DESCRIPTION_CHARS: usize = 400;

/// The largest `SKILL.md` body kept when no other bound is set: 64 KiB.
///
/// That is about 16,000 tokens at four bytes a token, over three times the
/// specification's recommendation of 5,000. A body over the bound is not
/// truncated, because half a procedure is worse than none: the skill is
/// skipped.
pub const DEFAULT_MAX_BODY_BYTES: usize = 64 * 1024;

/// The largest file `read_skill_file` returns when no other bound is set:
/// 64 KiB. A larger file is refused, not truncated.
pub const DEFAULT_MAX_RESOURCE_BYTES: usize = 64 * 1024;

/// Where a skills directory comes from, which decides how far it is trusted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum SkillScope {
    /// A directory the operator configured: their own text.
    Operator,
    /// A directory of a project the process was pointed at, such as a
    /// repository's `.agents/skills`: text anyone who can commit to it
    /// wrote. See [`SkillSource::project`].
    Project,
}

/// One directory whose subdirectories are skills.
///
/// Only the directory's own children are examined (`<dir>/<name>/SKILL.md`),
/// not deeper; names that start with a dot are skipped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillSource {
    pub(super) path: PathBuf,
    pub(super) scope: SkillScope,
    pub(super) trusted: bool,
    pub(super) follow_symlinks: bool,
}

impl SkillSource {
    /// A directory the operator configured. Trusted: its skills' text is the
    /// operator's own.
    #[must_use]
    pub fn operator(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            scope: SkillScope::Operator,
            trusted: true,
            follow_symlinks: false,
        }
    }

    /// A project's skills directory.
    ///
    /// Not read until the operator says so with
    /// [`with_trusted(true)`](Self::with_trusted): a skill's description goes
    /// into the model's system prompt and its body into the conversation, so a
    /// directory anyone can write to is an instruction channel
    /// (`docs/injection-containment.md`). Marking it trusted is the operator's
    /// statement that they accept that. Even then a project's skill never
    /// replaces an operator's of the same name, its description is marked as
    /// the project's in the prompt, and it gets no capability the operator's
    /// skills lack: `allowed-tools` is never honoured and `scripts/` are
    /// never run.
    #[must_use]
    pub fn project(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            scope: SkillScope::Project,
            trusted: false,
            follow_symlinks: false,
        }
    }

    /// Whether this directory is read. Operator directories are trusted from
    /// the start and project directories are not.
    #[must_use]
    pub const fn with_trusted(mut self, trusted: bool) -> Self {
        self.trusted = trusted;
        self
    }

    /// Whether a symbolic link in this directory that leads to a skill
    /// directory is followed. Off by default: a link is how a directory
    /// someone else controls gets into this one. A followed link still
    /// cannot take a file read outside the skill's own directory.
    #[must_use]
    pub const fn with_follow_symlinks(mut self, follow: bool) -> Self {
        self.follow_symlinks = follow;
        self
    }

    /// The directory.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Where it comes from.
    #[must_use]
    pub const fn scope(&self) -> SkillScope {
        self.scope
    }
}

/// What [`SkillCatalog::discover`](super::SkillCatalog::discover) scans and
/// the bounds it keeps to.
///
/// Sources are read operator directories first, then project directories,
/// each in the order given and each directory's skills in name order, so the
/// catalog does not depend on the file system's listing order. When two skills
/// share a name the first is kept.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillsConfig {
    pub(super) sources: Vec<SkillSource>,
    pub(super) max_skills: usize,
    pub(super) max_description_chars: usize,
    pub(super) max_body_bytes: usize,
    pub(super) max_resource_bytes: usize,
}

impl Default for SkillsConfig {
    fn default() -> Self {
        Self::new()
    }
}

impl SkillsConfig {
    /// No sources, and the default bounds.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            sources: Vec::new(),
            max_skills: DEFAULT_MAX_SKILLS,
            max_description_chars: DEFAULT_MAX_DESCRIPTION_CHARS,
            max_body_bytes: DEFAULT_MAX_BODY_BYTES,
            max_resource_bytes: DEFAULT_MAX_RESOURCE_BYTES,
        }
    }

    /// Adds a directory to scan.
    #[must_use]
    pub fn with_source(mut self, source: SkillSource) -> Self {
        self.sources.push(source);
        self
    }

    /// Sets how many skills the catalog holds. Skills past it are not read
    /// and one diagnostic says how many were left. Default
    /// [`DEFAULT_MAX_SKILLS`].
    #[must_use]
    pub const fn with_max_skills(mut self, max: usize) -> Self {
        self.max_skills = max;
        self
    }

    /// Sets how many characters of a description the prompt's catalog shows.
    /// `0` is taken as `1`. Default [`DEFAULT_MAX_DESCRIPTION_CHARS`].
    #[must_use]
    pub const fn with_max_description_chars(mut self, max: usize) -> Self {
        self.max_description_chars = if max == 0 { 1 } else { max };
        self
    }

    /// Sets the largest body kept, in bytes; a skill with a larger one is
    /// skipped. Default [`DEFAULT_MAX_BODY_BYTES`].
    #[must_use]
    pub const fn with_max_body_bytes(mut self, max: usize) -> Self {
        self.max_body_bytes = max;
        self
    }

    /// Sets the largest file `read_skill_file` returns, in bytes. Default
    /// [`DEFAULT_MAX_RESOURCE_BYTES`].
    #[must_use]
    pub const fn with_max_resource_bytes(mut self, max: usize) -> Self {
        self.max_resource_bytes = max;
        self
    }
}
