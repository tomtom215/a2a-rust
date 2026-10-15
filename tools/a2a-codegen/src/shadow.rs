// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The second half of `--check` (ADR 0016): no hand-written type shadows a
//! generated one.
//!
//! The byte comparison shows the generated files are what the proto makes; it
//! cannot see a second definition of `Task` written by hand elsewhere, which
//! would let the proto's type and a drifting copy live side by side. This
//! refuses a `struct`, `enum`, `union` or `type` declared outside
//! `src/generated/` under any name the configuration gives a generated type.
//! The generator's own model of the proto (`tools/a2a-codegen`) is exempt.

use std::collections::BTreeSet;
use std::path::Path;
use std::process::Command;

use crate::config::Config;

/// Paths, relative to the repository root, that may declare those names.
const EXEMPT: [&str; 2] = [
    "crates/a2a-protocol-types/src/generated/",
    "tools/a2a-codegen/",
];

/// The Rust name of every generated type: each proto type's, and each
/// `oneof` enum's that has a path of its own (`PartContent`).
pub fn generated_names(config: &Config) -> BTreeSet<String> {
    let last = |path: &str| path.rsplit("::").next().unwrap_or(path).to_owned();
    let mut names: BTreeSet<String> = config.paths.values().map(|p| last(p)).collect();
    for message in config.messages.values() {
        for oneof in message.oneofs.values() {
            if let Some(path) = &oneof.path {
                names.insert(last(path));
            }
        }
    }
    names
}

/// The name a line declares as a `struct`, `enum`, `union` or `type`, with
/// any visibility; `None` for every other line, comments and docs included.
pub fn declared_name(line: &str) -> Option<&str> {
    let mut rest = line.trim_start();
    if let Some(after) = rest.strip_prefix("pub") {
        rest = after.trim_start();
        if rest.starts_with('(') {
            rest = rest[rest.find(')')? + 1..].trim_start();
        }
    }
    let after = ["struct", "enum", "union", "type"]
        .iter()
        .find_map(|kw| rest.strip_prefix(kw))?;
    if !after.starts_with(char::is_whitespace) {
        return None;
    }
    let after = after.trim_start();
    let end = after
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .unwrap_or(after.len());
    (end > 0).then(|| &after[..end])
}

/// Each declaration, as `path:line: name`, of a generated type's name in a
/// Rust file git knows of (tracked, or untracked and not ignored) outside the
/// exempt paths.
pub fn find(root: &Path, names: &BTreeSet<String>) -> Result<Vec<String>, String> {
    let out = Command::new("git")
        .args([
            "ls-files",
            "--cached",
            "--others",
            "--exclude-standard",
            "-z",
            "--",
            "*.rs",
        ])
        .current_dir(root)
        .output()
        .map_err(|e| format!("cannot run git ls-files: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "git ls-files failed: {}",
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    let listing = String::from_utf8(out.stdout).map_err(|e| e.to_string())?;
    let mut found = Vec::new();
    for path in listing.split('\0').filter(|p| !p.is_empty()) {
        if EXEMPT.iter().any(|e| path.starts_with(e)) {
            continue;
        }
        // A file git lists may be deleted in the working tree.
        let Ok(text) = std::fs::read_to_string(root.join(path)) else {
            continue;
        };
        for (n, line) in text.lines().enumerate() {
            if let Some(name) = declared_name(line).filter(|name| names.contains(*name)) {
                found.push(format!("{path}:{}: {name}", n + 1));
            }
        }
    }
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::declared_name;

    #[test]
    fn declarations_are_found_with_any_visibility() {
        for (line, name) in [
            ("pub struct Task {", "Task"),
            ("    pub(crate) enum Part {", "Part"),
            ("pub(in crate::x) union Message {", "Message"),
            ("struct Artifact;", "Artifact"),
            ("type TaskState = u8;", "TaskState"),
            ("pub struct Task<T> {", "Task"),
        ] {
            assert_eq!(declared_name(line), Some(name), "{line}");
        }
    }

    #[test]
    fn other_lines_declare_nothing() {
        for line in [
            "/// pub struct Task {",
            "// struct Task",
            "let task = Task {",
            "pub use crate::generated::Task;",
            "impl Task {",
            "pub structure Task",
            "pub fn struct_task() {}",
            "typeset",
        ] {
            assert_eq!(declared_name(line), None, "{line}");
        }
    }
}
