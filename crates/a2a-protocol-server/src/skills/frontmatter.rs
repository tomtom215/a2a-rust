// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! `SKILL.md`: split into frontmatter and body, and the frontmatter checked
//! against the specification, leniently (ADR 0035, decision 2).
//!
//! The size of the frontmatter is checked before any of it is read as YAML.
//! A finding of [`Severity::Error`] means the skill is skipped; the rest are
//! reported and the skill is kept.

use super::diagnostic::{DiagnosticKind, Finding, Severity};
use super::spec::{
    self, KNOWN_FIELDS, MAX_COMPATIBILITY_CHARS, MAX_DESCRIPTION_CHARS, MAX_FRONTMATTER_BYTES,
    MAX_NAME_CHARS, MAX_NAME_CHARS_USABLE, RECOMMENDED_BODY_LINES, SPEC_SNAPSHOT,
};
use super::yaml::{self, Entry, Value};

/// A `SKILL.md` that is usable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Parsed {
    pub(super) name: String,
    pub(super) description: String,
    pub(super) license: Option<String>,
    pub(super) compatibility: Option<String>,
    pub(super) allowed_tools: Option<String>,
    pub(super) metadata: Vec<(String, String)>,
    pub(super) body: String,
}

/// Reads a `SKILL.md`, leaving what it finds in `findings`.
///
/// `directory_name` is the name of the skill's directory, which the `name`
/// should equal. `None` when the skill cannot be used.
pub(super) fn parse(
    text: &str,
    directory_name: &str,
    max_body_bytes: usize,
    findings: &mut Vec<Finding>,
) -> Option<Parsed> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let (block, body) = match split(text) {
        Ok(parts) => parts,
        Err(finding) => {
            findings.push(finding);
            return None;
        }
    };
    let entries = match yaml::parse(block) {
        Ok(entries) => entries,
        Err(e) => {
            findings.push(Finding::new(
                Severity::Error,
                DiagnosticKind::FrontmatterInvalid,
                format!("line {} of the file: {}", e.line + 1, e.reason),
            ));
            return None;
        }
    };
    let fields = Fields::new(&entries, findings);
    let name = fields.required("name", findings);
    let description = fields.required("description", findings);
    let (name, description) = (name?, description?);
    if !check_name(&name, directory_name, findings) {
        return None;
    }
    check_description(&description, findings);
    let body = body.trim_start_matches(['\r', '\n']).trim_end();
    if !check_body(body, max_body_bytes, findings) {
        return None;
    }
    Some(Parsed {
        name,
        description,
        license: fields.optional_text("license", None, findings),
        compatibility: fields.optional_text(
            "compatibility",
            Some(MAX_COMPATIBILITY_CHARS),
            findings,
        ),
        allowed_tools: fields.allowed_tools(findings),
        metadata: fields.metadata(findings),
        body: body.to_owned(),
    })
}

/// The frontmatter block and the body of `text`.
fn split(text: &str) -> Result<(&str, &str), Finding> {
    let missing = || {
        Finding::new(
            Severity::Error,
            DiagnosticKind::FrontmatterMissing,
            "the file does not start with a `---` line",
        )
    };
    let (first, rest) = text.split_once('\n').ok_or_else(missing)?;
    if first.trim_end() != "---" {
        return Err(missing());
    }
    let mut offset = 0;
    for line in rest.split_inclusive('\n') {
        if line.trim_end() == "---" {
            return Ok((&rest[..offset], &rest[offset + line.len()..]));
        }
        offset += line.len();
        if offset > MAX_FRONTMATTER_BYTES {
            return Err(Finding::new(
                Severity::Error,
                DiagnosticKind::FrontmatterTooLarge,
                format!("the frontmatter is over {MAX_FRONTMATTER_BYTES} bytes and was not parsed"),
            ));
        }
    }
    Err(Finding::new(
        Severity::Error,
        DiagnosticKind::FrontmatterUnclosed,
        "the frontmatter has no closing `---` line",
    ))
}

/// The entries of a frontmatter, the first of each key, with the findings
/// about the block as a whole made on the way.
struct Fields<'a> {
    first: Vec<&'a Entry>,
}

impl<'a> Fields<'a> {
    fn new(entries: &'a [Entry], findings: &mut Vec<Finding>) -> Self {
        let mut first: Vec<&Entry> = Vec::new();
        for entry in entries {
            if first.iter().any(|seen| seen.key == entry.key) {
                findings.push(Finding::new(
                    Severity::Warning,
                    DiagnosticKind::DuplicateField,
                    format!(
                        "`{}` appears twice (line {} of the file); the first is used",
                        entry.key,
                        entry.line + 1
                    ),
                ));
                continue;
            }
            if !KNOWN_FIELDS.contains(&entry.key.as_str()) {
                findings.push(Finding::new(
                    Severity::Warning,
                    DiagnosticKind::UnknownField,
                    format!(
                        "`{}` is not a field of the Agent Skills specification as read on \
                         {SPEC_SNAPSHOT}, which has no versioned releases; it is ignored",
                        entry.key
                    ),
                ));
            }
            if entry.lenient {
                findings.push(Finding::new(
                    Severity::Warning,
                    DiagnosticKind::LenientYaml,
                    format!(
                        "`{}` (line {} of the file) holds `: ` in a plain value; read as \
                         text, but a strict YAML parser refuses it: quote the value",
                        entry.key,
                        entry.line + 1
                    ),
                ));
            }
            first.push(entry);
        }
        Self { first }
    }

    fn get(&self, key: &str) -> Option<&'a Entry> {
        self.first.iter().copied().find(|entry| entry.key == key)
    }

    /// A required string field, trimmed; `None` with an error recorded when
    /// it is absent, empty or not a string.
    fn required(&self, key: &str, findings: &mut Vec<Finding>) -> Option<String> {
        let Some(entry) = self.get(key) else {
            findings.push(Finding::new(
                Severity::Error,
                DiagnosticKind::FieldMissing,
                format!("`{key}` is required and absent"),
            ));
            return None;
        };
        match &entry.value {
            Value::Text(text) if !text.trim().is_empty() => Some(text.trim().to_owned()),
            Value::Text(_) => {
                findings.push(Finding::new(
                    Severity::Error,
                    DiagnosticKind::FieldMissing,
                    format!("`{key}` is required and empty"),
                ));
                None
            }
            other => {
                findings.push(Finding::new(
                    Severity::Error,
                    DiagnosticKind::FieldInvalid,
                    format!("`{key}` is not a string: {}", describe(other)),
                ));
                None
            }
        }
    }

    /// An optional string field, trimmed and at most `limit` characters
    /// when a limit is given; ignored, with a warning, otherwise.
    fn optional_text(
        &self,
        key: &str,
        limit: Option<usize>,
        findings: &mut Vec<Finding>,
    ) -> Option<String> {
        let entry = self.get(key)?;
        let problem = match &entry.value {
            Value::Text(text) => {
                let text = text.trim();
                if text.is_empty() {
                    "it is empty".to_owned()
                } else if let Some(max) = limit.filter(|&max| text.chars().count() > max) {
                    format!("it is longer than the {max} characters the specification allows")
                } else {
                    return Some(text.to_owned());
                }
            }
            other => format!("it is not a string: {}", describe(other)),
        };
        ignored(key, &problem, findings);
        None
    }

    fn allowed_tools(&self, findings: &mut Vec<Finding>) -> Option<String> {
        let text = self.optional_text("allowed-tools", None, findings)?;
        findings.push(Finding::new(
            Severity::Info,
            DiagnosticKind::AllowedToolsIgnored,
            "`allowed-tools` is experimental in the specification; it is recorded and never \
             used, and no skill widens the tools the loop offers",
        ));
        Some(text)
    }

    fn metadata(&self, findings: &mut Vec<Finding>) -> Vec<(String, String)> {
        let Some(entry) = self.get("metadata") else {
            return Vec::new();
        };
        match &entry.value {
            Value::Map(pairs) => pairs.clone(),
            Value::Text(text) if text.trim().is_empty() => Vec::new(),
            other => {
                ignored(
                    "metadata",
                    &format!("it is not a mapping of strings: {}", describe(other)),
                    findings,
                );
                Vec::new()
            }
        }
    }
}

fn ignored(key: &str, problem: &str, findings: &mut Vec<Finding>) {
    findings.push(Finding::new(
        Severity::Warning,
        DiagnosticKind::OptionalFieldIgnored,
        format!("`{key}` is ignored: {problem}"),
    ));
}

/// What a value is, in words, for a finding.
fn describe(value: &Value) -> String {
    match value {
        Value::Text(_) => "a string".to_owned(),
        Value::Map(_) => "a mapping".to_owned(),
        Value::Unsupported(what) | Value::Malformed(what) => (*what).to_owned(),
    }
}

/// Whether the name can be used; findings about it either way.
fn check_name(name: &str, directory_name: &str, findings: &mut Vec<Finding>) -> bool {
    let length = name.chars().count();
    if length > MAX_NAME_CHARS_USABLE {
        findings.push(Finding::new(
            Severity::Error,
            DiagnosticKind::NameInvalid,
            format!(
                "the name is {length} characters; names over {MAX_NAME_CHARS_USABLE} are \
                 not used (the specification allows {MAX_NAME_CHARS})"
            ),
        ));
        return false;
    }
    if let Some(c) = name.chars().find(|&c| !spec::is_usable_name_char(c)) {
        findings.push(Finding::new(
            Severity::Error,
            DiagnosticKind::NameInvalid,
            format!(
                "the name holds {c:?}, which a prompt cannot carry safely; only letters, \
                 digits, hyphens, underscores and dots are used"
            ),
        ));
        return false;
    }
    let mut breaks = Vec::new();
    if length > MAX_NAME_CHARS {
        breaks.push(format!("is longer than {MAX_NAME_CHARS} characters"));
    }
    if !name.chars().all(spec::is_lowercase_invariant) {
        breaks.push("has an uppercase letter".to_owned());
    }
    if !name.chars().all(spec::is_spec_name_char) {
        breaks.push("has a character other than letters, digits and hyphens".to_owned());
    }
    if name.starts_with('-') || name.ends_with('-') {
        breaks.push("starts or ends with a hyphen".to_owned());
    }
    if name.contains("--") {
        breaks.push("has two hyphens together".to_owned());
    }
    if !breaks.is_empty() {
        findings.push(Finding::new(
            Severity::Warning,
            DiagnosticKind::NameNotSpec,
            format!(
                "the name '{name}' {} (the specification as read on {SPEC_SNAPSHOT})",
                breaks.join(", and ")
            ),
        ));
    }
    if name != directory_name {
        findings.push(Finding::new(
            Severity::Warning,
            DiagnosticKind::NameMismatch,
            format!(
                "the name '{name}' is not the directory's name '{directory_name}'; the skill \
                 is used under '{name}'"
            ),
        ));
    }
    true
}

fn check_description(description: &str, findings: &mut Vec<Finding>) {
    let length = description.chars().count();
    if length > MAX_DESCRIPTION_CHARS {
        findings.push(Finding::new(
            Severity::Warning,
            DiagnosticKind::DescriptionTooLong,
            format!(
                "the description is {length} characters; the specification allows \
                 {MAX_DESCRIPTION_CHARS}"
            ),
        ));
    }
}

/// Whether the body can be kept; findings about it either way.
fn check_body(body: &str, max_bytes: usize, findings: &mut Vec<Finding>) -> bool {
    if body.len() > max_bytes {
        findings.push(Finding::new(
            Severity::Error,
            DiagnosticKind::BodyTooLarge,
            format!(
                "the instructions are {} bytes, over the {max_bytes} this catalog keeps; a \
                 truncated procedure would be worse than none, so the skill is skipped",
                body.len()
            ),
        ));
        return false;
    }
    if body.is_empty() {
        findings.push(Finding::new(
            Severity::Warning,
            DiagnosticKind::BodyEmpty,
            "the instructions after the frontmatter are empty",
        ));
    }
    let lines = body.lines().count();
    if lines > RECOMMENDED_BODY_LINES {
        findings.push(Finding::new(
            Severity::Warning,
            DiagnosticKind::BodyLong,
            format!(
                "the instructions are {lines} lines; the specification recommends under \
                 {RECOMMENDED_BODY_LINES} and moving the rest to `references/`"
            ),
        ));
    }
    true
}

/// Reads `text` as a `SKILL.md` and returns how many findings it made; for the
/// fuzz target, which asks only that no input panics.
#[cfg(any(fuzzing, test))]
#[must_use]
pub fn fuzz(text: &str) -> usize {
    let mut findings = Vec::new();
    parse(text, "fuzz", super::DEFAULT_MAX_BODY_BYTES, &mut findings);
    findings.len()
}

#[cfg(test)]
#[path = "frontmatter_tests/mod.rs"]
mod tests;
