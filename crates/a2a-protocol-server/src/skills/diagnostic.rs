// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! What discovery reports: [`SkillDiagnostic`], with a [`Severity`] and a
//! [`DiagnosticKind`] a program can match on.

use std::fmt;
use std::path::{Path, PathBuf};

/// How much a diagnostic matters.
///
/// Ordered: `Info < Warning < Error`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum Severity {
    /// Worth knowing; nothing is wrong. A skill's `scripts/` being present
    /// and not run is the usual one.
    Info,
    /// The skill is loaded and deviates from the specification, or something
    /// the operator may have expected was left out (an untrusted project
    /// directory, a skill dropped for a duplicate name).
    Warning,
    /// The skill, or the directory, was skipped.
    Error,
}

impl fmt::Display for Severity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Info => "info",
            Self::Warning => "warning",
            Self::Error => "error",
        })
    }
}

/// What a diagnostic is about, stable enough to match on. The message says
/// the particulars.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum DiagnosticKind {
    /// A configured directory could not be listed. Nothing was loaded from it.
    SourceUnreadable,
    /// A directory was configured and not marked trusted (a project directory
    /// is not, until the operator says so). Nothing was read from it.
    SourceNotTrusted,
    /// A directory holds more entries than are examined; the rest were not.
    ScanTruncated,
    /// A symbolic link to a skill directory was not followed.
    SymlinkNotFollowed,
    /// A symbolic link in a skills directory leads nowhere usable: it dangles,
    /// or loops.
    BrokenSymlink,
    /// A directory has no `SKILL.md` and was ignored.
    NoSkillFile,
    /// `SKILL.md` could not be read, or is not a regular file inside its
    /// directory.
    SkillFileUnreadable,
    /// `SKILL.md` is over the size this module reads.
    SkillFileTooLarge,
    /// `SKILL.md` is not UTF-8.
    NotUtf8,
    /// `SKILL.md` does not start with a `---` frontmatter line.
    FrontmatterMissing,
    /// The frontmatter has no closing `---` line.
    FrontmatterUnclosed,
    /// The frontmatter is over the size this module parses; it was not read as
    /// YAML.
    FrontmatterTooLarge,
    /// The frontmatter is not a YAML mapping of the shape this module reads.
    FrontmatterInvalid,
    /// A field's value could not be read as the kind of value the field
    /// takes. For `name` and `description` the skill is skipped; for the
    /// optional fields the field is ignored.
    FieldInvalid,
    /// `name` or `description` is absent or empty. The skill is skipped.
    FieldMissing,
    /// The `name` has a character a prompt cannot carry safely, or is
    /// absurdly long. The skill is skipped.
    NameInvalid,
    /// The `name` deviates from the specification (uppercase, a hyphen at an
    /// end or two together, more than 64 characters); the skill is loaded.
    NameNotSpec,
    /// The `name` is not the name of the skill's directory; the skill is
    /// loaded under its `name`.
    NameMismatch,
    /// The `description` is longer than the specification's 1024 characters;
    /// the skill is loaded.
    DescriptionTooLong,
    /// The catalog shows a shortened description; the activation result
    /// carries the whole of it.
    DescriptionShortened,
    /// An optional field was empty, too long or not a string; it is ignored.
    OptionalFieldIgnored,
    /// A frontmatter field the specification (as of
    /// [`SPEC_SNAPSHOT`](super::SPEC_SNAPSHOT)) does not define.
    UnknownField,
    /// A field appears twice; the first is used.
    DuplicateField,
    /// A plain value contains `: `, which a strict YAML parser refuses and
    /// this module reads as text.
    LenientYaml,
    /// `allowed-tools` is recorded and never used: no skill widens the tools
    /// the loop offers.
    AllowedToolsIgnored,
    /// The body is over the size this module keeps. The skill is skipped.
    BodyTooLarge,
    /// The body is empty.
    BodyEmpty,
    /// The body is longer than the specification recommends.
    BodyLong,
    /// A second skill has a name already taken; it was dropped.
    DuplicateName,
    /// More skills were found than the catalog holds; the rest were not read.
    OverSkillCap,
    /// The skill has a `scripts/` directory. Its files are listed to the
    /// model as present and never run.
    ScriptsNotRun,
    /// A listing of a skill's files stopped at a bound.
    ListingTruncated,
}

/// One thing discovery noticed. See
/// [`SkillCatalog::diagnostics`](super::SkillCatalog::diagnostics).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillDiagnostic {
    severity: Severity,
    kind: DiagnosticKind,
    path: PathBuf,
    message: String,
}

impl SkillDiagnostic {
    pub(super) fn new(
        severity: Severity,
        kind: DiagnosticKind,
        path: impl Into<PathBuf>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            severity,
            kind,
            path: path.into(),
            message: message.into(),
        }
    }

    /// How much it matters.
    #[must_use]
    pub const fn severity(&self) -> Severity {
        self.severity
    }

    /// What it is about.
    #[must_use]
    pub const fn kind(&self) -> DiagnosticKind {
        self.kind
    }

    /// The directory or file it concerns.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// What was found, in words.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for SkillDiagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}: {}: {}",
            self.severity,
            self.path.display(),
            self.message
        )
    }
}

/// A finding in a `SKILL.md` before the file's path is known.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Finding {
    pub(super) severity: Severity,
    pub(super) kind: DiagnosticKind,
    pub(super) message: String,
}

impl Finding {
    pub(super) fn new(
        severity: Severity,
        kind: DiagnosticKind,
        message: impl Into<String>,
    ) -> Self {
        Self {
            severity,
            kind,
            message: message.into(),
        }
    }

    /// This finding as a diagnostic about `path`.
    pub(super) fn at(self, path: &Path) -> SkillDiagnostic {
        SkillDiagnostic::new(self.severity, self.kind, path, self.message)
    }
}
