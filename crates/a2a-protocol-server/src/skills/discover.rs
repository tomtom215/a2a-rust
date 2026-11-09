// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Finding skills: the configured directories scanned into [`Skill`]s and
//! diagnostics, never panicking and never failing (ADR 0035, decision 2).

use std::ffi::OsString;
use std::fs::{self, FileType};
use std::io;
use std::path::{Path, PathBuf};

use super::catalog::{Skill, collapse_whitespace, shorten};
use super::config::{SkillScope, SkillSource, SkillsConfig};
use super::diagnostic::{DiagnosticKind, Severity, SkillDiagnostic};
use super::frontmatter;
use super::reader::{self, READABLE_DIRS, Refusal};
use super::spec::MAX_FRONTMATTER_BYTES;

/// How many entries of one skills directory are examined. A directory of
/// more than this is not a skills directory, and listing it all would be a
/// way to make start-up slow.
pub(super) const MAX_ENTRIES_PER_SOURCE: usize = 1024;

/// How many files of one of a skill's directories are listed to the model.
pub(super) const MAX_LISTED_FILES: usize = 100;

/// How many directories deep below `references/`, `assets/` or `scripts/` a
/// listing goes.
const MAX_LISTING_DEPTH: usize = 3;

/// What `SKILL.md` may be over the frontmatter's bound and the body's: the
/// two `---` lines and the white space around the body.
pub(super) const FILE_SLACK: usize = 64;

/// The skills and diagnostics `config` yields.
pub(super) fn discover(config: &SkillsConfig) -> (Vec<Skill>, Vec<SkillDiagnostic>) {
    let mut scan = Scan {
        config,
        skills: Vec::new(),
        diagnostics: Vec::new(),
    };
    // Operator directories first, so a project's skill can never take the
    // name of one of theirs.
    let operator = config
        .sources
        .iter()
        .filter(|source| source.scope == SkillScope::Operator);
    let project = config
        .sources
        .iter()
        .filter(|source| source.scope == SkillScope::Project);
    for source in operator.chain(project) {
        scan.source(source);
    }
    (scan.skills, scan.diagnostics)
}

struct Scan<'a> {
    config: &'a SkillsConfig,
    skills: Vec<Skill>,
    diagnostics: Vec<SkillDiagnostic>,
}

impl Scan<'_> {
    fn note(
        &mut self,
        severity: Severity,
        kind: DiagnosticKind,
        path: &Path,
        message: impl Into<String>,
    ) {
        self.diagnostics
            .push(SkillDiagnostic::new(severity, kind, path, message));
    }

    const fn full(&self) -> bool {
        self.skills.len() >= self.config.max_skills
    }

    fn source(&mut self, source: &SkillSource) {
        if !source.trusted {
            self.note(
                Severity::Warning,
                DiagnosticKind::SourceNotTrusted,
                &source.path,
                "this directory is not marked trusted, so nothing was read from it",
            );
            return;
        }
        if self.full() {
            self.note(
                Severity::Warning,
                DiagnosticKind::OverSkillCap,
                &source.path,
                format!(
                    "not read: the catalog already holds its {} skills",
                    self.config.max_skills
                ),
            );
            return;
        }
        let Some(directories) = self.listed(source) else {
            return;
        };
        let total = directories.len();
        for (index, (name, path)) in directories.iter().enumerate() {
            if self.full() {
                self.note(
                    Severity::Warning,
                    DiagnosticKind::OverSkillCap,
                    &source.path,
                    format!(
                        "{} more directories were not read: the catalog holds at most {} skills",
                        total - index,
                        self.config.max_skills
                    ),
                );
                break;
            }
            self.skill(source.scope, name, path);
        }
    }

    /// The skill directories of `source`, or `None` when it cannot be listed.
    fn listed(&mut self, source: &SkillSource) -> Option<Vec<(String, PathBuf)>> {
        let (children, truncated) = match list_children(&source.path) {
            Ok(listing) => listing,
            Err(e) => {
                self.note(
                    Severity::Error,
                    DiagnosticKind::SourceUnreadable,
                    &source.path,
                    format!("the directory cannot be listed ({})", e.kind()),
                );
                return None;
            }
        };
        if truncated {
            self.note(
                Severity::Warning,
                DiagnosticKind::ScanTruncated,
                &source.path,
                format!(
                    "only the first {MAX_ENTRIES_PER_SOURCE} entries of the directory were examined"
                ),
            );
        }
        Some(self.directories(source, children))
    }

    /// The children that are directories, by name and path, with the
    /// symbolic links among them dealt with as the source says.
    fn directories(
        &mut self,
        source: &SkillSource,
        children: Vec<(OsString, FileType)>,
    ) -> Vec<(String, PathBuf)> {
        let mut found = Vec::new();
        for (name, kind) in children {
            let path = source.path.join(&name);
            let directory = if kind.is_symlink() {
                self.link(source, &path)
            } else {
                kind.is_dir()
            };
            if directory {
                found.push((name.to_string_lossy().into_owned(), path));
            }
        }
        found
    }

    /// Whether the symbolic link at `path` is a directory to read as a skill.
    fn link(&mut self, source: &SkillSource, path: &Path) -> bool {
        match fs::metadata(path) {
            Ok(target) if !target.is_dir() => false,
            Ok(_) if source.follow_symlinks => true,
            Ok(_) => {
                self.note(
                    Severity::Warning,
                    DiagnosticKind::SymlinkNotFollowed,
                    path,
                    "a symbolic link to a directory, not followed (see \
                     SkillSource::with_follow_symlinks)",
                );
                false
            }
            Err(e) => {
                self.note(
                    Severity::Warning,
                    DiagnosticKind::BrokenSymlink,
                    path,
                    format!("a symbolic link that leads nowhere usable ({})", e.kind()),
                );
                false
            }
        }
    }

    /// Reads the skill in `directory`, if there is one.
    fn skill(&mut self, scope: SkillScope, directory_name: &str, directory: &Path) {
        let file = directory.join("SKILL.md");
        let root = match directory.canonicalize() {
            Ok(root) => root,
            Err(e) => {
                self.note(
                    Severity::Error,
                    DiagnosticKind::SkillFileUnreadable,
                    directory,
                    format!("the directory cannot be resolved ({})", e.kind()),
                );
                return;
            }
        };
        let Some(text) = self.read_skill_file(directory, &root, &file) else {
            return;
        };
        let mut findings = Vec::new();
        let parsed = frontmatter::parse(
            &text,
            directory_name,
            self.config.max_body_bytes,
            &mut findings,
        );
        self.diagnostics
            .extend(findings.into_iter().map(|finding| finding.at(&file)));
        let Some(parsed) = parsed else { return };
        if let Some(first) = self.skills.iter().find(|skill| skill.name == parsed.name) {
            let message = format!(
                "a skill named '{}' is already loaded from {}; this one is dropped",
                parsed.name,
                first.location().display()
            );
            self.note(
                Severity::Warning,
                DiagnosticKind::DuplicateName,
                &file,
                message,
            );
            return;
        }
        let skill = self.build(scope, directory, root, parsed, &file);
        self.skills.push(skill);
    }

    /// The text of `directory`'s `SKILL.md`, or why there is none.
    fn read_skill_file(&mut self, directory: &Path, root: &Path, file: &Path) -> Option<String> {
        let max_body = self.config.max_body_bytes;
        let bound = MAX_FRONTMATTER_BYTES
            .saturating_add(max_body)
            .saturating_add(FILE_SLACK);
        let (severity, kind, message) = match reader::read_skill_file(root, bound) {
            Ok(bytes) => match String::from_utf8(bytes) {
                Ok(text) => return Some(text),
                Err(_) => (
                    Severity::Error,
                    DiagnosticKind::NotUtf8,
                    "SKILL.md is not UTF-8; skipped".to_owned(),
                ),
            },
            Err(Refusal::NotFound) => {
                self.note(
                    Severity::Info,
                    DiagnosticKind::NoSkillFile,
                    directory,
                    "no SKILL.md here (the name is case-sensitive); ignored",
                );
                return None;
            }
            Err(Refusal::TooLarge { .. }) => (
                Severity::Error,
                DiagnosticKind::SkillFileTooLarge,
                format!(
                    "SKILL.md is over {bound} bytes (a frontmatter of at most \
                     {MAX_FRONTMATTER_BYTES} and a body of at most {max_body}); skipped"
                ),
            ),
            Err(other) => (
                Severity::Error,
                DiagnosticKind::SkillFileUnreadable,
                other.to_string(),
            ),
        };
        self.note(severity, kind, file, message);
        None
    }

    /// The [`Skill`] for `parsed`, with the notes its files and description
    /// call for.
    fn build(
        &mut self,
        scope: SkillScope,
        directory: &Path,
        root: PathBuf,
        parsed: frontmatter::Parsed,
        file: &Path,
    ) -> Skill {
        let listing = list_files(&root);
        self.note_listing(file, &listing);
        let (catalog_description, shortened) = shorten(
            &collapse_whitespace(&parsed.description),
            self.config.max_description_chars,
        );
        if shortened {
            self.note(
                Severity::Info,
                DiagnosticKind::DescriptionShortened,
                file,
                format!(
                    "the catalog shows the first {} characters of the description; the \
                     activation result carries all of it",
                    self.config.max_description_chars
                ),
            );
        }
        Skill {
            name: parsed.name,
            description: parsed.description,
            catalog_description,
            shortened,
            license: parsed.license,
            compatibility: parsed.compatibility,
            allowed_tools: parsed.allowed_tools,
            metadata: parsed.metadata,
            body: parsed.body,
            scope,
            directory: directory.to_path_buf(),
            root,
            resources: listing.resources,
            scripts: listing.scripts,
        }
    }

    fn note_listing(&mut self, file: &Path, listing: &Listing) {
        for dir in &listing.linked {
            self.note(
                Severity::Warning,
                DiagnosticKind::SymlinkNotFollowed,
                file,
                format!(
                    "`{dir}/` is a symbolic link, so its files are neither listed nor readable \
                     (a link is how a directory someone else controls gets into a skill)"
                ),
            );
        }
        if !listing.scripts.is_empty() {
            self.note(
                Severity::Info,
                DiagnosticKind::ScriptsNotRun,
                file,
                format!(
                    "scripts/ holds {} file(s), listed to the model as present; this module \
                     never runs them (ADR 0035)",
                    listing.scripts.len()
                ),
            );
        }
        if listing.truncated {
            self.note(
                Severity::Info,
                DiagnosticKind::ListingTruncated,
                file,
                format!(
                    "a listing of the skill's files stopped at {MAX_LISTED_FILES} files in a \
                     directory, or at {MAX_LISTING_DEPTH} levels"
                ),
            );
        }
    }
}

/// The entries of `path` that are worth looking at (not hidden), by name, and
/// whether there were more than [`MAX_ENTRIES_PER_SOURCE`] to examine.
fn list_children(path: &Path) -> io::Result<(Vec<(OsString, FileType)>, bool)> {
    let mut entries = fs::read_dir(path)?;
    let mut children = Vec::new();
    for entry in entries.by_ref().take(MAX_ENTRIES_PER_SOURCE) {
        let Ok(entry) = entry else { continue };
        let name = entry.file_name();
        if name.to_string_lossy().starts_with('.') {
            continue;
        }
        if let Ok(kind) = entry.file_type() {
            children.push((name, kind));
        }
    }
    let truncated = entries.next().is_some();
    children.sort_by(|a, b| a.0.cmp(&b.0));
    Ok((children, truncated))
}

/// What a skill's directory holds besides `SKILL.md`.
#[derive(Debug, Default)]
pub(super) struct Listing {
    /// Files under `references/` and `assets/`, as `references/a.md`.
    pub(super) resources: Vec<String>,
    /// Files under `scripts/`.
    pub(super) scripts: Vec<String>,
    /// Whether a bound stopped a listing.
    pub(super) truncated: bool,
    /// The directories among `references/`, `assets/` and `scripts/` that are
    /// symbolic links, and so were not listed.
    pub(super) linked: Vec<&'static str>,
}

/// Lists the regular files below `references/`, `assets/` and `scripts/` of
/// the skill resolved to `root`. Links are not followed, so a link is not
/// listed, and neither is a directory of the three that is itself a link:
/// what it points at is somewhere else, and its entries' names are not the
/// skill's to tell the model (`read_skill_file` refuses every path through
/// it, since the path leaves the directory it may be read from).
pub(super) fn list_files(root: &Path) -> Listing {
    let mut listing = Listing::default();
    for dir in READABLE_DIRS {
        if is_link(root, dir) {
            listing.linked.push(dir);
        } else {
            listing.truncated |= walk(root, dir, &mut listing.resources);
        }
    }
    if is_link(root, "scripts") {
        listing.linked.push("scripts");
    } else {
        listing.truncated |= walk(root, "scripts", &mut listing.scripts);
    }
    listing
}

/// Whether `root/dir` is a symbolic link (to anything, or to nothing).
fn is_link(root: &Path, dir: &str) -> bool {
    fs::symlink_metadata(root.join(dir)).is_ok_and(|m| m.file_type().is_symlink())
}

/// Appends the files below `root/dir` to `out` as `dir/…`, in name order, and
/// says whether a bound stopped it. The walk keeps its own stack, so a deep
/// tree cannot overflow the call stack.
fn walk(root: &Path, dir: &str, out: &mut Vec<String>) -> bool {
    let mut found = Vec::new();
    let mut truncated = false;
    let mut stack = vec![(root.join(dir), dir.to_owned(), 1_usize)];
    'tree: while let Some((path, prefix, depth)) = stack.pop() {
        let Ok(entries) = fs::read_dir(&path) else {
            continue;
        };
        for entry in entries.filter_map(Result::ok) {
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            let name = entry.file_name();
            let shown = format!("{prefix}/{}", name.to_string_lossy());
            if kind.is_file() {
                if found.len() >= MAX_LISTED_FILES {
                    truncated = true;
                    break 'tree;
                }
                found.push(shown);
            } else if kind.is_dir() {
                if depth >= MAX_LISTING_DEPTH {
                    truncated = true;
                } else {
                    stack.push((path.join(&name), shown, depth + 1));
                }
            }
        }
    }
    found.sort();
    out.extend(found);
    truncated
}

#[cfg(test)]
#[path = "discover_tests/mod.rs"]
mod tests;
