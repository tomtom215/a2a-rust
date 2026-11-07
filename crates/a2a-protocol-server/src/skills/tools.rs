// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The two tools the model is given: `activate_skill` and `read_skill_file`.
//!
//! Each is an ordinary [`Tool`], so a call to either is traced as an
//! `execute_tool` span, counted among the run's tool calls and bounded by the
//! loop's turn limit like any other (ADR 0035). Both are read-only: running
//! one again changes nothing.

use std::future::Future;
use std::pin::Pin;

use serde_json::{Value, json};

use super::catalog::{Skill, SkillCatalog, xml_escape};
use super::config::SkillScope;
use super::reader::{self, Refusal};
use crate::tool_loop::{Tool, ToolError, ToolInvocation};

/// The name of the tool that loads a skill's instructions.
pub const ACTIVATE_SKILL_TOOL: &str = "activate_skill";

/// The name of the tool that reads a file a skill bundles.
pub const READ_SKILL_FILE_TOOL: &str = "read_skill_file";

/// `activate_skill`: returns a skill's instructions and lists its files.
///
/// Its `name` parameter is a JSON Schema `enum` of the catalog's names, so a
/// model cannot ask for anything else, and a name outside the catalog is
/// refused before any file is touched (the instructions were read at
/// discovery, so activation touches none at all).
#[derive(Debug)]
pub struct ActivateSkillTool {
    catalog: SkillCatalog,
}

impl ActivateSkillTool {
    pub(super) const fn new(catalog: SkillCatalog) -> Self {
        Self { catalog }
    }

    fn activate(&self, arguments: &Value) -> Result<String, ToolError> {
        let name = string_argument(arguments, "name")?;
        let skill = skill_named(&self.catalog, name)?;
        Ok(activation(skill))
    }
}

impl Tool for ActivateSkillTool {
    fn name(&self) -> &str {
        ACTIVATE_SKILL_TOOL
    }

    fn description(&self) -> &'static str {
        "Load the full instructions of one of the available skills. Call it when the task \
         matches the description of a skill in the system prompt, before you act on the task. \
         It returns the skill's instructions and the files the skill bundles."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "name": {
                    "type": "string",
                    "enum": names(&self.catalog, |_| true),
                    "description": "The name of the skill to load."
                }
            },
            "required": ["name"],
            "additionalProperties": false
        })
    }

    fn read_only(&self) -> bool {
        true
    }

    fn call<'a>(
        &'a self,
        invocation: ToolInvocation,
    ) -> Pin<Box<dyn Future<Output = Result<String, ToolError>> + Send + 'a>> {
        Box::pin(async move { self.activate(&invocation.arguments) })
    }
}

/// `read_skill_file`: returns the text of a file under a skill's
/// `references/` or `assets/`, through the confined reader.
///
/// It refuses `..`, an absolute path, a NUL, a backslash, anything outside
/// those two directories, a link that leaves them, a file over the configured
/// size, and a file that is not UTF-8. See the `reader` module for what is
/// left of a time-of-check to time-of-use window.
#[derive(Debug)]
pub struct ReadSkillFileTool {
    catalog: SkillCatalog,
}

impl ReadSkillFileTool {
    pub(super) const fn new(catalog: SkillCatalog) -> Self {
        Self { catalog }
    }
}

impl Tool for ReadSkillFileTool {
    fn name(&self) -> &str {
        READ_SKILL_FILE_TOOL
    }

    fn description(&self) -> &'static str {
        "Read a text file that a skill bundles, by its path relative to the skill's directory, \
         such as references/REFERENCE.md. Only files under references/ and assets/ can be read. \
         The skill's instructions say which files to read and when."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "skill": {
                    "type": "string",
                    "enum": names(&self.catalog, |skill| !skill.resources.is_empty()),
                    "description": "The name of the skill the file belongs to."
                },
                "path": {
                    "type": "string",
                    "description": "The file's path relative to the skill's directory, with \
                                    forward slashes: references/REFERENCE.md."
                }
            },
            "required": ["skill", "path"],
            "additionalProperties": false
        })
    }

    fn read_only(&self) -> bool {
        true
    }

    fn call<'a>(
        &'a self,
        invocation: ToolInvocation,
    ) -> Pin<Box<dyn Future<Output = Result<String, ToolError>> + Send + 'a>> {
        Box::pin(async move {
            let skill_name = string_argument(&invocation.arguments, "skill")?;
            let path = string_argument(&invocation.arguments, "path")?.to_owned();
            // Before any file is touched.
            let skill = skill_named(&self.catalog, skill_name)?;
            let (directory, root) = (skill.directory.clone(), skill.root.clone());
            let max = self.catalog.max_resource_bytes();
            let read = tokio::task::spawn_blocking(move || {
                reader::read_text(&directory, &root, &path, max)
            })
            .await
            .unwrap_or(Err(Refusal::Interrupted));
            read.map_err(|refusal| {
                trace_warn!(
                    skill = %skill_name,
                    refusal = ?refusal,
                    "a skill file was not returned"
                );
                ToolError::Failed(refusal.to_string())
            })
        })
    }
}

/// The names of the skills `keep` accepts, as a JSON array for an `enum`.
fn names(catalog: &SkillCatalog, keep: impl Fn(&Skill) -> bool) -> Vec<&str> {
    catalog
        .skills()
        .iter()
        .filter(|skill| keep(skill))
        .map(Skill::name)
        .collect()
}

fn string_argument<'a>(arguments: &'a Value, key: &str) -> Result<&'a str, ToolError> {
    arguments
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| ToolError::Failed(format!("'{key}' is required and must be a string")))
}

fn skill_named<'a>(catalog: &'a SkillCatalog, name: &str) -> Result<&'a Skill, ToolError> {
    catalog.get(name).ok_or_else(|| {
        ToolError::Failed(format!(
            "no skill named {name:?}; the skills are: {}",
            names(catalog, |_| true).join(", ")
        ))
    })
}

/// What `activate_skill` returns for `skill`: its instructions in a
/// `skill_content` element, then, when it has any, its compatibility note,
/// the files `read_skill_file` can return and the scripts that are present
/// and not run.
fn activation(skill: &Skill) -> String {
    let mut out = format!("<skill_content name=\"{}\"", skill.name);
    if skill.scope == SkillScope::Project {
        out.push_str(" origin=\"project\"");
    }
    out.push_str(">\n");
    out.push_str(&skill.body);
    out.push_str("\n\n");
    if skill.shortened {
        out.push_str("Description: ");
        out.push_str(&skill.description);
        out.push_str("\n\n");
    }
    if let Some(compatibility) = &skill.compatibility {
        out.push_str("Compatibility: ");
        out.push_str(compatibility);
        out.push_str("\n\n");
    }
    out.push_str(
        "Paths in this skill are relative to the skill's directory; read a file with \
         read_skill_file.\n",
    );
    if !skill.resources.is_empty() {
        out.push_str("<skill_resources>\n");
        for file in &skill.resources {
            out.push_str("  <file>");
            out.push_str(&xml_escape(file));
            out.push_str("</file>\n");
        }
        out.push_str("</skill_resources>\n");
    }
    if !skill.scripts.is_empty() {
        out.push_str(
            "These scripts are present. This agent cannot run them; do not try to call them.\n\
             <skill_scripts>\n",
        );
        for file in &skill.scripts {
            out.push_str("  <file>");
            out.push_str(&xml_escape(file));
            out.push_str("</file>\n");
        }
        out.push_str("</skill_scripts>\n");
    }
    out.push_str("</skill_content>");
    out
}

#[cfg(test)]
#[path = "tools_tests.rs"]
mod tests;
