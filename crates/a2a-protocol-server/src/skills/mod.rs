// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Agent Skills in the tool loop, without running their scripts (ADR 0035;
//! feature `skills`).
//!
//! [Agent Skills](https://agentskills.io) is a format for reusable
//! procedures: a directory with a `SKILL.md` (YAML frontmatter with a `name`
//! and a `description`, then Markdown instructions) and optionally
//! `scripts/`, `references/` and `assets/`. A client tells the model each
//! skill's name and description, and loads a skill's instructions, then its
//! files, only when the task calls for them. It is not A2A's `AgentSkill`,
//! which an agent card advertises to other agents; the two meet in
//! [`SkillCatalog::agent_skills`].
//!
//! ```text
//!  skills directory ──discover──→ SkillCatalog ──┬─ prompt()  ─→ system prompt (names, descriptions)
//!  (SKILL.md files)               + diagnostics  ├─ activate_skill ─→ the instructions
//!                                                └─ read_skill_file ─→ references/, assets/ (confined)
//!                                                   scripts/ are listed as present, never run
//! ```
//!
//! # Use
//!
//! ```no_run
//! use a2a_protocol_server::skills::{SkillCatalog, SkillSource, SkillsConfig};
//! use a2a_protocol_server::tool_loop::{OpenAiChatModel, ToolLoop, ToolLoopExecutor, ToolSet};
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let catalog = SkillCatalog::discover(
//!     &SkillsConfig::new().with_source(SkillSource::operator("/etc/my-agent/skills")),
//! );
//! for problem in catalog.diagnostics() {
//!     eprintln!("{problem}");
//! }
//! let model = OpenAiChatModel::new("http://127.0.0.1:8080/v1", "qwen3:1.7b")?;
//! let executor = ToolLoopExecutor::new(ToolLoop::new(model, ToolSet::new()))
//!     .with_skills(&catalog)?;
//! # drop(executor); Ok(()) }
//! ```
//!
//! # What it does and does not do
//!
//! - **Discovery never fails.** [`SkillCatalog::discover`] returns a catalog
//!   and a list of [`SkillDiagnostic`]s. A `SKILL.md` that is malformed, or
//!   whose name is not its directory's, is reported and, where it can be
//!   used safely, still used; one that cannot is skipped, with the reason.
//!   Nothing panics and start-up is not refused.
//! - **Bounded.** At most [`DEFAULT_MAX_SKILLS`] skills, a description of at
//!   most [`DEFAULT_MAX_DESCRIPTION_CHARS`] characters in the prompt, a body
//!   of at most [`DEFAULT_MAX_BODY_BYTES`] and a file of at most
//!   [`DEFAULT_MAX_RESOURCE_BYTES`] returned; each is set on
//!   [`SkillsConfig`]. The frontmatter's size is checked before it is read as
//!   YAML.
//! - **No script is run.** `scripts/` is listed to the model as present and
//!   `read_skill_file` does not return it. Running them needs a sandbox this
//!   SDK does not have, and a skills directory is an injection channel
//!   (`docs/injection-containment.md`).
//! - **No skill widens the tool set.** `allowed-tools` is recorded and never
//!   acted on.
//! - **Trust by scope.** An operator directory ([`SkillSource::operator`]) is
//!   the operator's own text. A project directory ([`SkillSource::project`])
//!   is not read until the operator marks it trusted, and its skills never
//!   take the name of an operator's.
//! - **A snapshot.** Skills are read at [`discover`](SkillCatalog::discover)
//!   and not again: the prompt and the tools' schemas stay what the model was
//!   told. Discover again, and build a new executor, to pick up a change.
//!
//! # The specification
//!
//! Agent Skills has no versioned releases, so this module follows the
//! specification text as read on [`SPEC_SNAPSHOT`], and names that date in
//! the diagnostics about fields it does not know. The YAML frontmatter is read
//! by a reader of the subset the specification's reference parser accepts
//! (all scalars are strings; no anchors, aliases, tags or flow collections),
//! in this crate, because that is a small job and adds no dependency (ADR
//! 0035, decision 9).

mod catalog;
mod config;
mod diagnostic;
mod discover;
mod frontmatter;
mod reader;
mod spec;
mod tools;
mod yaml;

pub use catalog::{AGENT_SKILL_TAG, Skill, SkillCatalog};
pub use config::{
    DEFAULT_MAX_BODY_BYTES, DEFAULT_MAX_DESCRIPTION_CHARS, DEFAULT_MAX_RESOURCE_BYTES,
    DEFAULT_MAX_SKILLS, SkillScope, SkillSource, SkillsConfig,
};
pub use diagnostic::{DiagnosticKind, Severity, SkillDiagnostic};
pub use spec::SPEC_SNAPSHOT;
// For the fuzz target in `fuzz/`: compiled only for it (`cargo fuzz` sets
// `cfg(fuzzing)`) and for the tests of the function itself, and in no build an
// adopter makes. It is here and not in `crate::fuzzing` so that the lines of
// that file, which a ledger cites, do not move.
#[cfg(any(fuzzing, test))]
#[doc(hidden)]
pub use frontmatter::fuzz as fuzz_frontmatter;
pub use tools::{ACTIVATE_SKILL_TOOL, ActivateSkillTool, READ_SKILL_FILE_TOOL, ReadSkillFileTool};

#[cfg(test)]
pub(crate) mod test_support;
