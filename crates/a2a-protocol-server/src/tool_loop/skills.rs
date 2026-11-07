// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! [`ToolLoopExecutor::with_skills`]: Agent Skills in the tool loop (ADR 0035;
//! feature `skills`). The module is the whole of the executor's side of the
//! feature; what a skill is and how it is read is `crate::skills`.

use super::executor::ToolLoopExecutor;
use super::tool::DuplicateTool;
use crate::skills::SkillCatalog;

impl ToolLoopExecutor {
    /// Gives the loop the skills in `catalog`: the catalog goes at the end of
    /// the system prompt, and the model gets `activate_skill` (its `name` a
    /// JSON Schema enum of the skills' names) and, when some skill bundles
    /// files, `read_skill_file`. A call to either is a tool call like any
    /// other: traced as an `execute_tool` span, counted among the run's tool
    /// calls and bounded by the turn limit and the caller's budget. No script
    /// is run, and no skill adds a tool or approves one.
    ///
    /// A catalog with no skills changes nothing: an empty catalog and a tool
    /// with no valid argument would only confuse the model.
    ///
    /// The prompt is appended to the loop's own preamble, after a blank line,
    /// so this is called on an executor whose loop already has its preamble
    /// (the executor has no way to set one afterwards).
    ///
    /// # Errors
    ///
    /// [`DuplicateTool`] when the loop's tools already include one named
    /// `activate_skill` or `read_skill_file`. The executor is consumed.
    pub fn with_skills(mut self, catalog: &SkillCatalog) -> Result<Self, DuplicateTool> {
        for tool in catalog.tools() {
            self.tool_loop.tools.add_arc(tool)?;
        }
        if let Some(prompt) = catalog.prompt() {
            let preamble = &mut self.tool_loop.preamble;
            if !preamble.is_empty() {
                preamble.push_str("\n\n");
            }
            preamble.push_str(&prompt);
        }
        Ok(self)
    }
}

#[cfg(test)]
#[path = "skills_tests.rs"]
mod tests;
