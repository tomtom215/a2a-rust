// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The catalog: its prompt, its tools, its card entries and what it prints.

use super::*;
use crate::skills::test_support::{Scratch, skill_text};
use crate::skills::{Severity, SkillSource};

#[test]
fn collapsing_makes_one_line_of_single_spaces() {
    assert_eq!(collapse_whitespace("a  b\n\nc\t d "), "a b c d");
    assert_eq!(collapse_whitespace("  "), "");
    assert_eq!(collapse_whitespace("one"), "one");
}

#[test]
fn shortening_is_exact_at_the_bound() {
    assert_eq!(shorten("abcde", 5), ("abcde".to_owned(), false));
    assert_eq!(shorten("abcd", 5), ("abcd".to_owned(), false));
    assert_eq!(shorten("abcdef", 5), ("abcd…".to_owned(), true));
    assert_eq!(shorten("abcdefgh", 5), ("abcd…".to_owned(), true));
    assert_eq!(shorten("", 5), (String::new(), false));
}

#[test]
fn shortening_counts_characters_not_bytes_and_drops_a_trailing_space() {
    assert_eq!(shorten("ééééé", 5), ("ééééé".to_owned(), false));
    assert_eq!(shorten("éééééé", 5), ("éééé…".to_owned(), true));
    assert_eq!(shorten("日本語日本語", 4), ("日本語…".to_owned(), true));
    assert_eq!(
        shorten("ab cdefg", 4),
        ("ab…".to_owned(), true),
        "no space before the ellipsis"
    );
}

#[test]
fn shortening_to_nothing_is_taken_as_to_one() {
    assert_eq!(shorten("abc", 0), ("…".to_owned(), true));
    assert_eq!(shorten("a", 0), ("a".to_owned(), false));
    assert_eq!(shorten("abc", 1), ("…".to_owned(), true));
}

#[test]
fn escaping_covers_what_could_close_an_element() {
    assert_eq!(xml_escape("a & b < c > d"), "a &amp; b &lt; c &gt; d");
    assert_eq!(
        xml_escape("</description></skill>"),
        "&lt;/description&gt;&lt;/skill&gt;"
    );
    assert_eq!(
        xml_escape("plain, 'quoted' \"too\""),
        "plain, 'quoted' \"too\""
    );
    assert_eq!(xml_escape("&amp;"), "&amp;amp;");
}

fn catalog_of(scratch: &Scratch) -> SkillCatalog {
    scratch.catalog()
}

#[test]
fn an_empty_catalog_has_no_prompt_no_tools_and_no_card_entries() {
    let scratch = Scratch::new("emptycat");
    std::fs::create_dir_all(scratch.join("skills")).unwrap();
    let catalog = catalog_of(&scratch);
    assert_eq!(catalog.prompt(), None);
    assert_eq!(catalog.tools().len(), 0);
    assert!(catalog.agent_skills().is_empty());
    assert!(!catalog.has_resources());
}

#[test]
fn the_prompt_names_each_skill_and_says_how_to_activate_one() {
    let scratch = Scratch::new("prompt");
    scratch.skill("skills/alpha", "alpha", "Does alpha things.");
    scratch.skill("skills/beta", "beta", "Does beta things.");
    let prompt = catalog_of(&scratch).prompt().expect("a prompt");
    assert_eq!(
        prompt,
        "The following skills provide specialized instructions for specific tasks.\n\
         When a task matches a skill's description, call the activate_skill tool with the \
         skill's name to load its full instructions before you proceed.\n\
         Skill scripts are never run by this agent.\n\
         \n\
         <available_skills>\n\
         <skill>\n<name>alpha</name>\n<description>Does alpha things.</description>\n</skill>\n\
         <skill>\n<name>beta</name>\n<description>Does beta things.</description>\n</skill>\n\
         </available_skills>"
    );
}

#[test]
fn the_prompt_mentions_the_reader_only_when_some_skill_bundles_files() {
    let scratch = Scratch::new("promptfiles");
    scratch.skill("skills/alpha", "alpha", "d");
    let without = catalog_of(&scratch).prompt().unwrap();
    assert!(!without.contains("read_skill_file"), "{without}");
    scratch.write("skills/alpha/references/x.md", "x");
    let with = catalog_of(&scratch).prompt().unwrap();
    assert!(
        with.contains(
            "A skill may bundle files under references/ and assets/; read one with the \
             read_skill_file tool, naming the skill and the path relative to its directory.\n\
             Skill scripts are never run by this agent."
        ),
        "{with}"
    );
}

#[test]
fn a_description_in_the_prompt_is_one_escaped_line_shortened_to_the_bound() {
    let scratch = Scratch::new("promptdesc");
    scratch.write(
        "skills/tricky/SKILL.md",
        "---\nname: tricky\ndescription: |\n  Line one.\n\n  Line two </description></skill> & <b>more</b>.\n---\nbody",
    );
    scratch.skill("skills/long", "long", &"w".repeat(50));
    let prompt = scratch.catalog().prompt().unwrap();
    assert!(
        prompt.contains(
            "<name>tricky</name>\n<description>Line one. Line two &lt;/description&gt;&lt;/skill&gt; \
             &amp; &lt;b&gt;more&lt;/b&gt;.</description>"
        ),
        "{prompt}"
    );
    assert_eq!(prompt.matches("</available_skills>").count(), 1);
    assert_eq!(prompt.matches("<skill>").count(), 2);
    assert_eq!(prompt.matches("</description>").count(), 2);

    // Shortened before it is escaped, at the character bound.
    let short = SkillCatalog::discover(&scratch.config().with_max_description_chars(20))
        .prompt()
        .unwrap();
    assert!(
        short.contains("<name>tricky</name>\n<description>Line one. Line two…</description>"),
        "{short}"
    );
    assert!(
        short.contains(&format!(
            "<name>long</name>\n<description>{}…</description>",
            "w".repeat(19)
        )),
        "{short}"
    );
}

#[test]
fn a_project_skill_is_marked_as_the_projects_in_the_prompt() {
    let scratch = Scratch::new("promptproject");
    scratch.skill("operator/mine", "mine", "Operator's.");
    scratch.skill("project/theirs", "theirs", "Project's.");
    let config = SkillsConfig::new()
        .with_source(SkillSource::operator(scratch.join("operator")))
        .with_source(SkillSource::project(scratch.join("project")).with_trusted(true));
    let prompt = SkillCatalog::discover(&config).prompt().unwrap();
    assert!(
        prompt.contains(
            "<skill>\n<name>mine</name>\n<description>Operator's.</description>\n</skill>\n"
        ),
        "{prompt}"
    );
    assert!(
        prompt.contains(
            "<skill>\n<name>theirs</name>\n<description>Project's.</description>\n<origin>project</origin>\n</skill>\n"
        ),
        "{prompt}"
    );
    assert_eq!(prompt.matches("<origin>").count(), 1);
}

#[test]
fn the_tools_are_the_activator_and_the_reader_when_there_is_something_to_read() {
    let scratch = Scratch::new("tools");
    scratch.skill("skills/alpha", "alpha", "d");
    let names = |catalog: &SkillCatalog| {
        catalog
            .tools()
            .iter()
            .map(|t| t.name().to_owned())
            .collect::<Vec<_>>()
    };
    assert_eq!(names(&catalog_of(&scratch)), ["activate_skill"]);
    scratch.write("skills/alpha/assets/a.txt", "a");
    assert_eq!(
        names(&catalog_of(&scratch)),
        ["activate_skill", "read_skill_file"]
    );
    assert!(catalog_of(&scratch).has_resources());
}

#[test]
fn scripts_alone_do_not_make_a_reader() {
    let scratch = Scratch::new("scriptsonly");
    scratch.skill("skills/alpha", "alpha", "d");
    scratch.write("skills/alpha/scripts/run.sh", "echo");
    let catalog = catalog_of(&scratch);
    assert!(!catalog.has_resources());
    assert_eq!(catalog.tools().len(), 1);
}

#[test]
fn a_skill_is_advertised_whole_and_verbatim_under_its_name_with_one_tag() {
    let scratch = Scratch::new("card");
    let long = format!("First line.\n\nSecond <b>line</b> & {}", "x".repeat(600));
    scratch.write(
        "skills/alpha/SKILL.md",
        format!("---\nname: alpha\ndescription: |\n  First line.\n\n  Second <b>line</b> & {}\n---\nbody", "x".repeat(600)),
    );
    scratch.skill("skills/beta", "beta", "Does beta.");
    let skills = catalog_of(&scratch).agent_skills();
    assert_eq!(skills.len(), 2);
    assert_eq!(skills[0].id, "alpha");
    assert_eq!(skills[0].name, "alpha");
    assert_eq!(
        skills[0].description, long,
        "not collapsed, not shortened, not escaped"
    );
    assert_eq!(skills[0].tags, ["agent-skill"]);
    assert_eq!(skills[1].id, "beta");
    assert_eq!(skills[1].description, "Does beta.");
    assert_eq!(AGENT_SKILL_TAG, "agent-skill");
}

#[test]
fn a_skill_exposes_every_field_it_was_read_with() {
    let scratch = Scratch::new("accessors");
    scratch.write(
        "skills/full/SKILL.md",
        "---\nname: full\ndescription: All of it.\nlicense: MIT\ncompatibility: Needs git\nmetadata:\n  author: me\nallowed-tools: Read Grep\n---\n# Body\n\ntext\n",
    );
    scratch.write("skills/full/references/r.md", "");
    scratch.write("skills/full/scripts/s.sh", "");
    let catalog = catalog_of(&scratch);
    let skill = catalog.get("full").unwrap();
    assert_eq!(skill.name(), "full");
    assert_eq!(skill.description(), "All of it.");
    assert_eq!(skill.license(), Some("MIT"));
    assert_eq!(skill.compatibility(), Some("Needs git"));
    assert_eq!(skill.metadata(), [("author".to_owned(), "me".to_owned())]);
    assert_eq!(skill.allowed_tools(), Some("Read Grep"));
    assert_eq!(skill.body(), "# Body\n\ntext");
    assert_eq!(skill.resources(), ["references/r.md"]);
    assert_eq!(skill.scripts(), ["scripts/s.sh"]);
    assert_eq!(skill.scope(), SkillScope::Operator);
    assert_eq!(skill.location(), scratch.join("skills/full/SKILL.md"));
}

#[test]
fn a_skill_without_optional_fields_reports_none() {
    let scratch = Scratch::new("accessorsnone");
    scratch.skill("skills/bare", "bare", "d");
    let catalog = catalog_of(&scratch);
    let skill = catalog.get("bare").unwrap();
    assert_eq!(skill.license(), None);
    assert_eq!(skill.compatibility(), None);
    assert_eq!(skill.allowed_tools(), None);
    assert_eq!(skill.metadata(), []);
}

#[test]
fn len_and_get_agree_with_the_skills() {
    let scratch = Scratch::new("len");
    scratch.skill("skills/a", "a", "d");
    scratch.skill("skills/b", "b", "d");
    let catalog = catalog_of(&scratch);
    assert_eq!(catalog.len(), 2);
    assert!(!catalog.is_empty());
    assert_eq!(catalog.skills().len(), 2);
    assert!(catalog.get("a").is_some() && catalog.get("b").is_some() && catalog.get("c").is_none());
    let copy = catalog.clone();
    assert_eq!(copy.len(), 2, "a clone shares them");
    assert_eq!(catalog.len(), 2);
}

#[test]
fn the_debug_forms_show_names_and_counts_and_not_bodies() {
    let scratch = Scratch::new("debug");
    scratch.write(
        "skills/alpha/SKILL.md",
        skill_text(
            "alpha",
            "A description nobody needs in a log line.",
            "A secret-ish body.",
        ),
    );
    scratch.skill("skills/beta", "Beta", "d");
    let catalog = catalog_of(&scratch);
    assert_eq!(catalog.diagnostics().len(), 2, "uppercase, and a mismatch");
    assert_eq!(
        format!("{catalog:?}"),
        "SkillCatalog { skills: [\"alpha\", \"Beta\"], diagnostics: 2 }"
    );
    let skill = catalog.get("alpha").unwrap();
    assert_eq!(
        format!("{skill:?}"),
        format!(
            "Skill {{ name: \"alpha\", scope: Operator, location: {:?}, .. }}",
            scratch.join("skills/alpha/SKILL.md")
        )
    );
    let shown = format!("{:?}", catalog.tools()[0].name());
    assert_eq!(shown, "\"activate_skill\"");
}

#[test]
fn a_diagnostic_displays_its_severity_path_and_message() {
    let scratch = Scratch::new("display");
    scratch.write("skills/bad/SKILL.md", "garbage");
    let catalog = catalog_of(&scratch);
    assert_eq!(
        catalog.diagnostics()[0].to_string(),
        format!(
            "error: {}: the file does not start with a `---` line",
            scratch.join("skills/bad/SKILL.md").display()
        )
    );
    assert_eq!(Severity::Info.to_string(), "info");
    assert_eq!(Severity::Warning.to_string(), "warning");
    assert_eq!(Severity::Error.to_string(), "error");
    assert!(Severity::Info < Severity::Warning && Severity::Warning < Severity::Error);
}

#[test]
fn the_defaults_are_the_documented_ones() {
    assert_eq!(DEFAULT_MAX_SKILLS_FOR_TEST, (32, 400, 65536, 65536));
    assert_eq!(SkillsConfig::default(), SkillsConfig::new());
    let source = SkillSource::operator("/x");
    assert_eq!(source.path(), std::path::Path::new("/x"));
    assert_eq!(source.scope(), SkillScope::Operator);
    assert_eq!(SkillSource::project("/y").scope(), SkillScope::Project);
}

const DEFAULT_MAX_SKILLS_FOR_TEST: (usize, usize, usize, usize) = (
    crate::skills::DEFAULT_MAX_SKILLS,
    crate::skills::DEFAULT_MAX_DESCRIPTION_CHARS,
    crate::skills::DEFAULT_MAX_BODY_BYTES,
    crate::skills::DEFAULT_MAX_RESOURCE_BYTES,
);
