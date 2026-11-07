// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! [`Skill`] and [`SkillCatalog`]: what discovery found, the prompt that
//! announces it, and the tools that serve it.

use std::fmt;
use std::path::PathBuf;
use std::sync::Arc;

use a2a_protocol_types::agent_card::AgentSkill;

use super::config::{SkillScope, SkillsConfig};
use super::diagnostic::SkillDiagnostic;
use super::discover;
use super::tools::{ActivateSkillTool, ReadSkillFileTool};
use crate::tool_loop::Tool;

/// The tag on the [`AgentSkill`] a skill is advertised as.
pub const AGENT_SKILL_TAG: &str = "agent-skill";

/// One skill: a directory with a `SKILL.md`, read at discovery.
#[derive(Clone)]
pub struct Skill {
    pub(super) name: String,
    pub(super) description: String,
    pub(super) catalog_description: String,
    /// Whether the catalog shows less of the description than there is.
    pub(super) shortened: bool,
    pub(super) license: Option<String>,
    pub(super) compatibility: Option<String>,
    pub(super) allowed_tools: Option<String>,
    pub(super) metadata: Vec<(String, String)>,
    pub(super) body: String,
    pub(super) scope: SkillScope,
    /// The directory as it was configured, for reporting.
    pub(super) directory: PathBuf,
    /// The directory resolved, which every read is confined to.
    pub(super) root: PathBuf,
    /// Files under `references/` and `assets/`, as `references/a.md`.
    pub(super) resources: Vec<String>,
    /// Files under `scripts/`: present, and never run.
    pub(super) scripts: Vec<String>,
}

impl fmt::Debug for Skill {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Skill")
            .field("name", &self.name)
            .field("scope", &self.scope)
            .field("location", &self.location())
            .finish_non_exhaustive()
    }
}

impl Skill {
    /// The skill's name: the frontmatter's `name`, which is the key the
    /// catalog and `activate_skill` use.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The description, whole, as the frontmatter has it. The prompt's
    /// catalog shows a shortened one; see
    /// [`SkillsConfig::with_max_description_chars`].
    #[must_use]
    pub fn description(&self) -> &str {
        &self.description
    }

    /// The path of the skill's `SKILL.md`, under the directory as it was
    /// configured.
    #[must_use]
    pub fn location(&self) -> PathBuf {
        self.directory.join("SKILL.md")
    }

    /// Where the skill came from.
    #[must_use]
    pub const fn scope(&self) -> SkillScope {
        self.scope
    }

    /// The instructions: `SKILL.md` after the frontmatter, trimmed.
    #[must_use]
    pub fn body(&self) -> &str {
        &self.body
    }

    /// The `license` field, when there is one.
    #[must_use]
    pub fn license(&self) -> Option<&str> {
        self.license.as_deref()
    }

    /// The `compatibility` field, when there is one.
    #[must_use]
    pub fn compatibility(&self) -> Option<&str> {
        self.compatibility.as_deref()
    }

    /// The `metadata` field's keys and values, in file order.
    #[must_use]
    pub fn metadata(&self) -> &[(String, String)] {
        &self.metadata
    }

    /// The `allowed-tools` field, recorded. It is experimental in the
    /// specification and this module never acts on it: no skill adds a tool
    /// to the loop or approves one.
    #[must_use]
    pub fn allowed_tools(&self) -> Option<&str> {
        self.allowed_tools.as_deref()
    }

    /// The files `read_skill_file` can return, as `references/a.md`.
    #[must_use]
    pub fn resources(&self) -> &[String] {
        &self.resources
    }

    /// The files under `scripts/`, which are listed to the model as present
    /// and never run.
    #[must_use]
    pub fn scripts(&self) -> &[String] {
        &self.scripts
    }
}

struct Inner {
    skills: Vec<Skill>,
    diagnostics: Vec<SkillDiagnostic>,
    max_resource_bytes: usize,
}

/// The skills found in the configured directories, and what was noticed on
/// the way.
///
/// Cheap to clone: the skills are shared. Built once, at start-up, by
/// [`discover`](Self::discover); it does not change afterwards, so the prompt
/// and the tools' schemas stay what the model was told. To pick up changed
/// files, discover again and build a new executor.
///
/// ```no_run
/// use a2a_protocol_server::skills::{SkillCatalog, SkillSource, SkillsConfig};
///
/// let catalog = SkillCatalog::discover(
///     &SkillsConfig::new().with_source(SkillSource::operator("/etc/my-agent/skills")),
/// );
/// for problem in catalog.diagnostics() {
///     eprintln!("{problem}");
/// }
/// println!("{} skills", catalog.len());
/// ```
#[derive(Clone)]
pub struct SkillCatalog {
    inner: Arc<Inner>,
}

impl fmt::Debug for SkillCatalog {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SkillCatalog")
            .field(
                "skills",
                &self
                    .inner
                    .skills
                    .iter()
                    .map(Skill::name)
                    .collect::<Vec<_>>(),
            )
            .field("diagnostics", &self.inner.diagnostics.len())
            .finish()
    }
}

impl SkillCatalog {
    /// Reads the skills `config` names.
    ///
    /// Never fails and never panics: a directory that cannot be read, a
    /// `SKILL.md` that is malformed, a name that is taken, are each a
    /// [`SkillDiagnostic`] and the rest is still loaded. It reads files, so
    /// call it at start-up, or from a blocking task.
    #[must_use]
    pub fn discover(config: &SkillsConfig) -> Self {
        let (skills, diagnostics) = discover::discover(config);
        Self {
            inner: Arc::new(Inner {
                skills,
                diagnostics,
                max_resource_bytes: config.max_resource_bytes,
            }),
        }
    }

    /// The skills, operator directories first, each directory's in name order.
    #[must_use]
    pub fn skills(&self) -> &[Skill] {
        &self.inner.skills
    }

    /// The skill named `name`.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&Skill> {
        self.inner.skills.iter().find(|skill| skill.name == name)
    }

    /// How many skills there are.
    #[must_use]
    pub fn len(&self) -> usize {
        self.inner.skills.len()
    }

    /// Whether there are none.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.inner.skills.is_empty()
    }

    /// What discovery noticed, in the order it noticed it. Log it:
    /// [`SkillDiagnostic`] is `Display`, and a skill that is missing from the
    /// catalog is explained here and nowhere else.
    #[must_use]
    pub fn diagnostics(&self) -> &[SkillDiagnostic] {
        &self.inner.diagnostics
    }

    pub(super) fn max_resource_bytes(&self) -> usize {
        self.inner.max_resource_bytes
    }

    /// Whether any skill bundles a file `read_skill_file` can return.
    pub(super) fn has_resources(&self) -> bool {
        self.inner
            .skills
            .iter()
            .any(|skill| !skill.resources.is_empty())
    }

    /// The catalog as the model is told it: a short instruction and each
    /// skill's name and description, in the form the specification's client
    /// guide shows. `None` when there are no skills, because an empty catalog
    /// would only confuse the model.
    ///
    /// A description is on one line, shortened to the configured length,
    /// and XML-escaped, so it cannot close the element it is in. A project
    /// skill is marked `<origin>project</origin>`.
    #[must_use]
    pub fn prompt(&self) -> Option<String> {
        if self.is_empty() {
            return None;
        }
        let mut out = String::from(
            "The following skills provide specialized instructions for specific tasks.\n\
             When a task matches a skill's description, call the activate_skill tool with \
             the skill's name to load its full instructions before you proceed.\n",
        );
        if self.has_resources() {
            out.push_str(
                "A skill may bundle files under references/ and assets/; read one with the \
                 read_skill_file tool, naming the skill and the path relative to its directory.\n",
            );
        }
        out.push_str("Skill scripts are never run by this agent.\n\n<available_skills>\n");
        for skill in &self.inner.skills {
            out.push_str("<skill>\n<name>");
            out.push_str(&xml_escape(&skill.name));
            out.push_str("</name>\n<description>");
            out.push_str(&xml_escape(&skill.catalog_description));
            out.push_str("</description>\n");
            if skill.scope == SkillScope::Project {
                out.push_str("<origin>project</origin>\n");
            }
            out.push_str("</skill>\n");
        }
        out.push_str("</available_skills>");
        Some(out)
    }

    /// The tools that serve the catalog: `activate_skill`, and
    /// `read_skill_file` when some skill bundles files. None at all when the
    /// catalog is empty, because a tool whose `name` has no valid value
    /// would confuse the model.
    ///
    /// [`ToolLoopExecutor::with_skills`](crate::tool_loop::ToolLoopExecutor::with_skills)
    /// adds these and the [`prompt`](Self::prompt); this is for a loop run
    /// without it.
    #[must_use]
    pub fn tools(&self) -> Vec<Arc<dyn Tool>> {
        let mut tools: Vec<Arc<dyn Tool>> = Vec::new();
        if !self.is_empty() {
            tools.push(Arc::new(ActivateSkillTool::new(self.clone())));
        }
        if self.has_resources() {
            tools.push(Arc::new(ReadSkillFileTool::new(self.clone())));
        }
        tools
    }

    /// Each skill as an [`AgentSkill`] for the agent card, which is public:
    /// `id` and `name` are the skill's name, `description` is its
    /// description whole and verbatim, and the one tag is
    /// [`AGENT_SKILL_TAG`]. Nothing is advertised unless the card is built
    /// from this, and a skill's description is not necessarily meant to be
    /// public.
    ///
    /// ```no_run
    /// # use a2a_protocol_server::skills::SkillCatalog;
    /// # fn advertise(catalog: &SkillCatalog) {
    /// use a2a_protocol_types::agent_card::{AgentCard, AgentInterface};
    ///
    /// let mut card = AgentCard::new("agent", "1.0.0", AgentInterface::jsonrpc("http://127.0.0.1:3000"));
    /// for skill in catalog.agent_skills() {
    ///     card = card.with_skill(skill);
    /// }
    /// # drop(card); }
    /// ```
    #[must_use]
    pub fn agent_skills(&self) -> Vec<AgentSkill> {
        self.inner
            .skills
            .iter()
            .map(|skill| {
                AgentSkill::new(&skill.name, &skill.name, &skill.description)
                    .with_tags([AGENT_SKILL_TAG])
            })
            .collect()
    }
}

/// `text` with each run of white space one space, so a description cannot
/// lay out lines of its own in a prompt.
pub(super) fn collapse_whitespace(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// `text` cut to at most `max` characters, the last an ellipsis when it was
/// cut; and whether it was. `0` is taken as `1`.
pub(super) fn shorten(text: &str, max: usize) -> (String, bool) {
    let max = max.max(1);
    if text.chars().count() <= max {
        return (text.to_owned(), false);
    }
    let mut cut: String = text.chars().take(max - 1).collect();
    cut.truncate(cut.trim_end().len());
    cut.push('…');
    (cut, true)
}

/// `text` safe inside an XML element.
pub(super) fn xml_escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            other => out.push(other),
        }
    }
    out
}

#[cfg(test)]
#[path = "catalog_tests.rs"]
mod tests;
