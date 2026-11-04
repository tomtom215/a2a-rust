// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Noticing a `SQLite` file on a network or FUSE file system (audit E-F26).
//!
//! WAL mode keeps its index in a shared-memory file and relies on POSIX locks,
//! and neither is dependable on NFS, SMB or most FUSE file systems. The
//! failure is not an error at open: it is a database that two processes both
//! think they hold, which corrupts or loses writes, and a turn claim stored in
//! it that two replicas both read as free. `book/src/deployment/horizontal-scaling.md`
//! limits SQLite sharing to one host; nothing checked it.
//!
//! Detection reads `/proc/self/mountinfo` (Linux), because the crate forbids
//! `unsafe` and `statfs` needs it. The mount holding the file is the one with
//! the longest mount point that contains it; its file system type is compared
//! with a list of remote ones. It is a warning and not a refusal: the type
//! cannot be told apart from a local file system presented through FUSE, and a
//! single-process deployment on such a mount is fine.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex, PoisonError};

/// File system types whose locking and shared memory `SQLite`'s WAL mode cannot
/// rely on. `fuseblk` (a FUSE driver over a local block device, such as
/// `ntfs-3g`) is not here: it is local.
const REMOTE: &[&str] = &[
    "nfs",
    "nfs4",
    "cifs",
    "smb3",
    "smbfs",
    "9p",
    "ceph",
    "glusterfs",
    "lustre",
    "afs",
    "ncpfs",
    "beegfs",
    "gpfs",
    "orangefs",
    "panfs",
];

/// Whether `fstype`, as `/proc/self/mountinfo` names it, is remote.
fn is_remote(fstype: &str) -> bool {
    REMOTE.contains(&fstype) || fstype == "fuse" || fstype.starts_with("fuse.")
}

/// Undoes the escaping `mountinfo` applies to a mount point: a space, tab,
/// newline or backslash is written as a three-digit octal escape.
fn unescape(field: &str) -> String {
    let mut out = String::with_capacity(field.len());
    let mut rest = field;
    while let Some(at) = rest.find('\\') {
        out.push_str(&rest[..at]);
        let escape = &rest[at + 1..];
        let decoded = escape
            .get(..3)
            .and_then(|digits| u8::from_str_radix(digits, 8).ok())
            .filter(u8::is_ascii);
        if let Some(byte) = decoded {
            out.push(char::from(byte));
            rest = &escape[3..];
        } else {
            out.push('\\');
            rest = escape;
        }
    }
    out.push_str(rest);
    out
}

/// The file system type of the mount that holds `path`, read from the text of
/// `/proc/self/mountinfo`: the one with the longest mount point that contains
/// it, the later of two at the same mount point (a mount placed over another).
///
/// `path` must be absolute and free of symbolic links.
pub(super) fn filesystem_of(mountinfo: &str, path: &Path) -> Option<String> {
    let mut best: Option<(usize, &str)> = None;
    for line in mountinfo.lines() {
        // `<id> <parent> <dev> <root> <mount point> <options> [tags...] - <type> <source> <options>`
        let Some((before, after)) = line.split_once(" - ") else {
            continue;
        };
        let Some(mount_point) = before.split(' ').nth(4) else {
            continue;
        };
        let Some(fstype) = after.split(' ').next() else {
            continue;
        };
        let mount_point = PathBuf::from(unescape(mount_point));
        if path.starts_with(&mount_point) {
            let depth = mount_point.components().count();
            if best.is_none_or(|(deepest, _)| depth >= deepest) {
                best = Some((depth, fstype));
            }
        }
    }
    best.map(|(_, fstype)| fstype.to_owned())
}

/// `Some(type)` when `path` is on a remote file system according to
/// `mountinfo`.
pub(super) fn remote_filesystem(mountinfo: &str, path: &Path) -> Option<String> {
    filesystem_of(mountinfo, path).filter(|fstype| is_remote(fstype))
}

/// Paths already warned about, so the four stores that open one file say it
/// once between them.
static WARNED: LazyLock<Mutex<HashSet<PathBuf>>> = LazyLock::new(Mutex::default);

/// Says that `path` is on the remote file system `fstype`, the first time it
/// is asked about `path`. Whether it spoke is returned.
pub(super) fn warn_once(seen: &Mutex<HashSet<PathBuf>>, path: &Path, _fstype: &str) -> bool {
    let first = seen
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .insert(path.to_path_buf());
    if first {
        trace_warn!(
            path = %path.display(),
            filesystem = _fstype,
            "the SQLite database is on a network or FUSE file system: WAL mode and the \
             turn claims kept in it are not safe there, and a file shared by several \
             processes is supported only on one host's local disk"
        );
    }
    first
}

/// Whether `database` is a `file:` URI and not a path. `sqlx` names the database
/// behind `sqlite::memory:` `file:sqlx-in-memory-N` (a shared-cache in-memory
/// database), and a `file:` URI of one's own is handed to `SQLite` unparsed, so
/// neither is a file in the working directory: read as a path it would put an
/// in-memory database on whatever file system the working directory is on.
fn is_uri(database: &Path) -> bool {
    database
        .to_str()
        .is_some_and(|name| name.starts_with("file:"))
}

/// The absolute path of the file `database` names, with the directory's symbolic
/// links resolved; the file itself need not exist yet. `None` for an in-memory
/// database, a `file:` URI (not checked) or a directory that cannot be resolved.
fn resolved(database: &Path) -> Option<PathBuf> {
    if database == Path::new(":memory:") || database.as_os_str().is_empty() || is_uri(database) {
        return None;
    }
    let name = database.file_name()?;
    let directory = match database.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent,
        _ => Path::new("."),
    };
    Some(directory.canonicalize().ok()?.join(name))
}

/// The kernel's table of this process's mounts.
pub(super) const MOUNTINFO: &str = "/proc/self/mountinfo";

/// Warns, once per file, when `database` is on a network or FUSE file system
/// according to the mount table at `mountinfo` ([`MOUNTINFO`] outside tests).
/// Does nothing where the table cannot be read.
pub(super) fn warn_if_remote(mountinfo: &Path, database: &Path) {
    let Some(path) = resolved(database) else {
        return;
    };
    let Ok(table) = std::fs::read_to_string(mountinfo) else {
        return;
    };
    if let Some(fstype) = remote_filesystem(&table, &path) {
        warn_once(&WARNED, &path, &fstype);
    }
}

#[cfg(test)]
mod tests;
