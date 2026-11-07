// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The confined reader of a skill's files (ADR 0035, decision 4).
//!
//! A model gives a path relative to a skill's directory. It is checked as
//! text first (no NUL, no backslash, no `..`, nothing absolute, and a first
//! component that is `references` or `assets`), then resolved on disk and
//! required to stay inside the directory it is read from, then opened and
//! checked again against what was actually opened.
//!
//! # What is left of a time-of-check to time-of-use window
//!
//! Resolving a path and opening it are two steps, and someone who can write
//! inside the skill's directory could change a component between them (swap
//! a directory for a symbolic link). The file is opened by its resolved path,
//! so no link is followed at the open that was not seen at the resolve, and
//! then [`verify`] resolves the path again and, on Unix, requires the open
//! file to be the file at that path (same device and inode). That closes the
//! window for a swap that is still in place, and for one that is put back,
//! because the file that was opened is then not the file at the path.
//!
//! What remains:
//!
//! - On a platform without Unix file identities (Windows) only the second
//!   resolution is compared, so a link swapped in and out again between the
//!   open and the second resolve is not seen. Creating a link there needs a
//!   privilege most processes do not hold.
//! - A hard link, which has no path to resolve, can name any file the
//!   linker can read. Someone who can write inside the directory and read
//!   the target already has the target.
//! - A regular file replaced by a FIFO between the check that it is a regular
//!   file and the open blocks the thread that opens it (the read runs on
//!   tokio's blocking pool, and the call is abandoned when the run is
//!   cancelled or times out) until something opens the FIFO for writing.
//!
//! A skill's directory is expected to be written by the operator, or by a
//! project's authors, and not by the process's other tenants.

use std::fmt;
use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Component, Path, PathBuf};

/// The directories of a skill whose files `read_skill_file` returns.
/// `scripts/` is not one of them: a script is listed as present and never run
/// (ADR 0035, decision 5).
pub(super) const READABLE_DIRS: [&str; 2] = ["references", "assets"];

/// Why a file was not returned. The `Display` is what the model is told, and
/// holds no path of the host's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Refusal {
    Empty,
    Nul,
    Backslash,
    Absolute,
    ParentDir,
    OutsideReadableDirs,
    NotFound,
    Unreadable(String),
    Escapes,
    NotAFile,
    TooLarge { max: usize },
    ChangedWhileOpening,
    SkillMoved,
    NotUtf8,
    Interrupted,
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => f.write_str("the path is empty"),
            Self::Nul => f.write_str("the path contains a NUL character"),
            Self::Backslash => f.write_str("the path contains a backslash; use forward slashes"),
            Self::Absolute => f.write_str(
                "the path is absolute; give a path relative to the skill's directory, such as \
                 references/REFERENCE.md",
            ),
            Self::ParentDir => f.write_str("the path contains `..`"),
            Self::OutsideReadableDirs => {
                f.write_str("only files under references/ and assets/ can be read")
            }
            Self::NotFound => f.write_str("there is no such file"),
            Self::Unreadable(why) => write!(f, "the file could not be read ({why})"),
            Self::Escapes => f.write_str(
                "the path leaves the directory it may be read from (a symbolic link points \
                 outside it)",
            ),
            Self::NotAFile => f.write_str("that is not a regular file"),
            Self::TooLarge { max } => {
                write!(
                    f,
                    "the file is larger than the {max} bytes this reader returns"
                )
            }
            Self::ChangedWhileOpening => f.write_str("the file changed while it was being opened"),
            Self::SkillMoved => f.write_str(
                "the skill's directory has changed since start-up; restart to load skills again",
            ),
            Self::NotUtf8 => f.write_str("the file is not UTF-8 text"),
            Self::Interrupted => f.write_str("the read did not complete"),
        }
    }
}

fn from_io(e: &io::Error) -> Refusal {
    if e.kind() == io::ErrorKind::NotFound {
        Refusal::NotFound
    } else {
        Refusal::Unreadable(e.kind().to_string())
    }
}

/// `path` checked as text and cleaned of `.` components: the relative path it
/// names, or why it is refused.
pub(super) fn relative(path: &str) -> Result<PathBuf, Refusal> {
    if path.is_empty() {
        return Err(Refusal::Empty);
    }
    if path.contains('\0') {
        return Err(Refusal::Nul);
    }
    if path.contains('\\') {
        return Err(Refusal::Backslash);
    }
    let mut clean = PathBuf::new();
    for component in Path::new(path).components() {
        match component {
            Component::Normal(part) => clean.push(part),
            Component::CurDir => {}
            Component::ParentDir => return Err(Refusal::ParentDir),
            Component::RootDir | Component::Prefix(_) => return Err(Refusal::Absolute),
        }
    }
    if clean.as_os_str().is_empty() {
        return Err(Refusal::Empty);
    }
    Ok(clean)
}

/// [`relative`], and the first component one of [`READABLE_DIRS`]: that
/// directory and the whole path.
pub(super) fn readable(path: &str) -> Result<(&'static str, PathBuf), Refusal> {
    let clean = relative(path)?;
    let first = clean.components().next().map(Component::as_os_str);
    let dir = READABLE_DIRS
        .iter()
        .find(|dir| first == Some(std::ffi::OsStr::new(dir)))
        .ok_or(Refusal::OutsideReadableDirs)?;
    Ok((dir, clean))
}

/// Opens `relative` under `root` for reading, requiring that it resolve to a
/// regular file inside `boundary`. `root` and `boundary` are resolved paths.
pub(super) fn open_within(root: &Path, relative: &Path, boundary: &Path) -> Result<File, Refusal> {
    let resolved = root
        .join(relative)
        .canonicalize()
        .map_err(|e| from_io(&e))?;
    if !resolved.starts_with(boundary) {
        return Err(Refusal::Escapes);
    }
    let metadata = fs::metadata(&resolved).map_err(|e| from_io(&e))?;
    if !metadata.is_file() {
        return Err(Refusal::NotAFile);
    }
    let file = File::open(&resolved).map_err(|e| from_io(&e))?;
    verify(&file, &resolved, boundary)?;
    Ok(file)
}

/// Requires that `file`, opened from `resolved`, is still what `resolved`
/// names: the path resolves to itself, inside `boundary`, and (on Unix) is the
/// file that was opened.
pub(super) fn verify(file: &File, resolved: &Path, boundary: &Path) -> Result<(), Refusal> {
    let again = resolved
        .canonicalize()
        .map_err(|_| Refusal::ChangedWhileOpening)?;
    if again != resolved || !again.starts_with(boundary) {
        return Err(Refusal::ChangedWhileOpening);
    }
    let opened = file.metadata().map_err(|e| from_io(&e))?;
    let named = fs::metadata(&again).map_err(|_| Refusal::ChangedWhileOpening)?;
    if !opened.is_file() || identity(&opened) != identity(&named) {
        return Err(Refusal::ChangedWhileOpening);
    }
    Ok(())
}

/// What names a file where std says: its device and inode. Elsewhere every
/// file has the same, so [`verify`] has only the path comparison to go on.
fn identity(metadata: &fs::Metadata) -> (u64, u64) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        (metadata.dev(), metadata.ino())
    }
    #[cfg(not(unix))]
    {
        let _ = metadata;
        (0, 0)
    }
}

/// Reads at most `max` bytes of `file`: more than that is refused. The bound is
/// on what is read, not on the size the file had when it was looked at, so a
/// file that grows is still cut off.
fn read_capped(file: File, max: usize) -> Result<Vec<u8>, Refusal> {
    let mut bytes = Vec::new();
    file.take(u64::try_from(max).unwrap_or(u64::MAX).saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|e| from_io(&e))?;
    if bytes.len() > max {
        return Err(Refusal::TooLarge { max });
    }
    Ok(bytes)
}

/// The `SKILL.md` of the skill whose resolved directory is `root`, at most
/// `max` bytes. A `SKILL.md` that is a link must stay inside `root`.
pub(super) fn read_skill_file(root: &Path, max: usize) -> Result<Vec<u8>, Refusal> {
    read_capped(open_within(root, Path::new("SKILL.md"), root)?, max)
}

/// The text of the file `path` names, under the `references/` or `assets/` of
/// the skill discovered at `directory` and resolved to `root`; at most `max`
/// bytes of UTF-8.
pub(super) fn read_text(
    directory: &Path,
    root: &Path,
    path: &str,
    max: usize,
) -> Result<String, Refusal> {
    let (dir, relative) = readable(path)?;
    if directory.canonicalize().ok().as_deref() != Some(root) {
        return Err(Refusal::SkillMoved);
    }
    let file = open_within(root, &relative, &root.join(dir))?;
    String::from_utf8(read_capped(file, max)?).map_err(|_| Refusal::NotUtf8)
}

#[cfg(test)]
#[path = "reader_tests/mod.rs"]
mod tests;
