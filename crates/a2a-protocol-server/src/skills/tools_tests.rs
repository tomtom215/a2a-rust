// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The two tools as the model calls them: what they advertise, what they
//! return and what they refuse.

use std::sync::Arc;

use serde_json::json;

use super::*;
use crate::skills::test_support::Scratch;
use crate::skills::{SkillSource, SkillsConfig};

async fn call(tool: &dyn Tool, arguments: Value) -> Result<String, ToolError> {
    tool.call(ToolInvocation::new(arguments, "key")).await
}

fn failed(message: &str) -> Result<String, ToolError> {
    Err(ToolError::Failed(message.to_owned()))
}

/// Two skills: `alpha` bundles a reference and an asset and has a script;
/// `beta` bundles nothing.
fn two_skills(scratch: &Scratch) -> SkillCatalog {
    scratch.skill("skills/alpha", "alpha", "Does alpha.");
    scratch.write("skills/alpha/references/guide.md", "the guide");
    scratch.write("skills/alpha/assets/a&b.txt", "an asset");
    scratch.write("skills/alpha/scripts/run.sh", "echo no");
    scratch.skill("skills/beta", "beta", "Does beta.");
    scratch.catalog()
}

fn tools(catalog: &SkillCatalog) -> (Arc<dyn Tool>, Arc<dyn Tool>) {
    let mut tools = catalog.tools();
    assert_eq!(tools.len(), 2);
    let read = tools.pop().unwrap();
    let activate = tools.pop().unwrap();
    (activate, read)
}

#[test]
fn activate_skill_says_what_it_is_and_takes_a_name_from_the_catalog() {
    let scratch = Scratch::new("activateschema");
    let (activate, _) = tools(&two_skills(&scratch));
    assert_eq!(activate.name(), "activate_skill");
    assert_eq!(ACTIVATE_SKILL_TOOL, "activate_skill");
    assert!(activate.read_only());
    assert_eq!(
        activate.description(),
        "Load the full instructions of one of the available skills. Call it when the task \
         matches the description of a skill in the system prompt, before you act on the task. \
         It returns the skill's instructions and the files the skill bundles."
    );
    assert_eq!(
        activate.parameters(),
        json!({
            "type": "object",
            "properties": {
                "name": {
                    "type": "string",
                    "enum": ["alpha", "beta"],
                    "description": "The name of the skill to load."
                }
            },
            "required": ["name"],
            "additionalProperties": false
        })
    );
}

#[test]
fn read_skill_file_takes_a_skill_that_has_files_and_a_path() {
    let scratch = Scratch::new("readschema");
    let (_, read) = tools(&two_skills(&scratch));
    assert_eq!(read.name(), "read_skill_file");
    assert_eq!(READ_SKILL_FILE_TOOL, "read_skill_file");
    assert!(read.read_only());
    assert_eq!(
        read.description(),
        "Read a text file that a skill bundles, by its path relative to the skill's directory, \
         such as references/REFERENCE.md. Only files under references/ and assets/ can be read. \
         The skill's instructions say which files to read and when."
    );
    assert_eq!(
        read.parameters(),
        json!({
            "type": "object",
            "properties": {
                "skill": {
                    "type": "string",
                    "enum": ["alpha"],
                    "description": "The name of the skill the file belongs to."
                },
                "path": {
                    "type": "string",
                    "description": "The file's path relative to the skill's directory, with forward slashes: references/REFERENCE.md."
                }
            },
            "required": ["skill", "path"],
            "additionalProperties": false
        })
    );
}

#[tokio::test]
async fn a_skill_is_activated_by_name_and_returns_its_instructions() {
    let scratch = Scratch::new("activate");
    scratch.skill("skills/demo", "demo", "Does a demo.");
    let activate = scratch.catalog().tools().remove(0);
    assert_eq!(
        call(&*activate, json!({"name": "demo"})).await,
        Ok(
            "<skill_content name=\"demo\">\nDo the thing.\n\nPaths in this skill are relative to \
             the skill's directory; read a file with read_skill_file.\n</skill_content>"
                .to_owned()
        )
    );
}

#[tokio::test]
async fn an_activation_lists_the_files_to_read_and_the_scripts_it_will_not_run() {
    let scratch = Scratch::new("activatefiles");
    let catalog = two_skills(&scratch);
    let (activate, _) = tools(&catalog);
    assert_eq!(
        call(&*activate, json!({"name": "alpha"})).await,
        Ok("<skill_content name=\"alpha\">\nDo the thing.\n\nPaths in this skill are relative to \
             the skill's directory; read a file with read_skill_file.\n\
             <skill_resources>\n  <file>references/guide.md</file>\n  <file>assets/a&amp;b.txt</file>\n</skill_resources>\n\
             These scripts are present. This agent cannot run them; do not try to call them.\n\
             <skill_scripts>\n  <file>scripts/run.sh</file>\n</skill_scripts>\n</skill_content>"
            .to_owned())
    );
}

#[tokio::test]
async fn an_activation_carries_the_whole_description_when_the_catalog_shortened_it() {
    let scratch = Scratch::new("activatelong");
    scratch.write(
        "skills/demo/SKILL.md",
        "---\nname: demo\ndescription: A description of some length.\ncompatibility: Needs git\n---\nSteps.\n",
    );
    let config = scratch.config().with_max_description_chars(10);
    let catalog = SkillCatalog::discover(&config);
    let activate = catalog.tools().remove(0);
    assert_eq!(
        call(&*activate, json!({"name": "demo"})).await,
        Ok("<skill_content name=\"demo\">\nSteps.\n\n\
             Description: A description of some length.\n\n\
             Compatibility: Needs git\n\n\
             Paths in this skill are relative to the skill's directory; read a file with read_skill_file.\n\
             </skill_content>"
            .to_owned())
    );
    // Not shortened: no repeat of it.
    let catalog = scratch.catalog();
    let text = call(&*catalog.tools().remove(0), json!({"name": "demo"}))
        .await
        .unwrap();
    assert!(!text.contains("Description:"), "{text}");
    assert!(text.contains("Compatibility: Needs git"), "{text}");
}

#[tokio::test]
async fn a_project_skill_is_activated_with_its_origin_marked() {
    let scratch = Scratch::new("activateproject");
    scratch.skill("project/theirs", "theirs", "d");
    scratch.skill("operator/mine", "mine", "d");
    let config = SkillsConfig::new()
        .with_source(SkillSource::operator(scratch.join("operator")))
        .with_source(SkillSource::project(scratch.join("project")).with_trusted(true));
    let catalog = SkillCatalog::discover(&config);
    let activate = catalog.tools().remove(0);
    let theirs = call(&*activate, json!({"name": "theirs"})).await.unwrap();
    assert!(
        theirs.starts_with("<skill_content name=\"theirs\" origin=\"project\">\n"),
        "{theirs}"
    );
    let mine = call(&*activate, json!({"name": "mine"})).await.unwrap();
    assert!(
        mine.starts_with("<skill_content name=\"mine\">\n"),
        "{mine}"
    );
}

#[tokio::test]
async fn a_name_outside_the_catalog_is_refused_with_the_names_there_are() {
    let scratch = Scratch::new("unknown");
    let (activate, read) = tools(&two_skills(&scratch));
    assert_eq!(
        call(&*activate, json!({"name": "gamma"})).await,
        failed("no skill named \"gamma\"; the skills are: alpha, beta")
    );
    // A path is not a name.
    assert_eq!(
        call(&*activate, json!({"name": "../skills/alpha"})).await,
        failed("no skill named \"../skills/alpha\"; the skills are: alpha, beta")
    );
    assert_eq!(
        call(
            &*read,
            json!({"skill": "gamma", "path": "references/guide.md"})
        )
        .await,
        failed("no skill named \"gamma\"; the skills are: alpha, beta")
    );
}

#[tokio::test]
async fn an_unknown_skill_is_refused_before_any_file_is_asked_for() {
    let scratch = Scratch::new("unknownfirst");
    let (_, read) = tools(&two_skills(&scratch));
    // The skills' directories are gone: a read of a known skill now fails on
    // that, and an unknown one is still refused by name, not by the disk.
    std::fs::remove_dir_all(scratch.join("skills")).unwrap();
    assert_eq!(
        call(
            &*read,
            json!({"skill": "nope", "path": "references/guide.md"})
        )
        .await,
        failed("no skill named \"nope\"; the skills are: alpha, beta")
    );
    assert_eq!(
        call(
            &*read,
            json!({"skill": "alpha", "path": "references/guide.md"})
        )
        .await,
        failed("the skill's directory has changed since start-up; restart to load skills again")
    );
}

#[tokio::test]
async fn a_missing_or_mistyped_argument_is_said() {
    let scratch = Scratch::new("badargs");
    let (activate, read) = tools(&two_skills(&scratch));
    for arguments in [
        json!({}),
        json!({"name": 3}),
        json!({"name": null}),
        json!({"name": ["alpha"]}),
        json!({"nom": "alpha"}),
    ] {
        assert_eq!(
            call(&*activate, arguments.clone()).await,
            failed("'name' is required and must be a string"),
            "{arguments}"
        );
    }
    assert_eq!(
        call(&*read, json!({"path": "references/guide.md"})).await,
        failed("'skill' is required and must be a string")
    );
    assert_eq!(
        call(&*read, json!({"skill": "alpha"})).await,
        failed("'path' is required and must be a string")
    );
    assert_eq!(
        call(&*read, json!({"skill": "alpha", "path": 7})).await,
        failed("'path' is required and must be a string")
    );
}

#[tokio::test]
async fn a_file_a_skill_bundles_is_read() {
    let scratch = Scratch::new("readok");
    let (_, read) = tools(&two_skills(&scratch));
    assert_eq!(
        call(
            &*read,
            json!({"skill": "alpha", "path": "references/guide.md"})
        )
        .await,
        Ok("the guide".to_owned())
    );
    assert_eq!(
        call(&*read, json!({"skill": "alpha", "path": "assets/a&b.txt"})).await,
        Ok("an asset".to_owned())
    );
}

#[tokio::test]
async fn a_parent_directory_an_absolute_path_and_scripts_are_each_refused() {
    let scratch = Scratch::new("refuse");
    scratch.write("secret.txt", "the host's");
    let (_, read) = tools(&two_skills(&scratch));
    assert_eq!(
        call_read(&read, "../../secret.txt").await,
        failed("the path contains `..`")
    );
    assert_eq!(
        call_read(&read, "references/../../../secret.txt").await,
        failed("the path contains `..`")
    );
    assert_eq!(
        call_read(&read, "/etc/passwd").await,
        failed(
            "the path is absolute; give a path relative to the skill's directory, such as references/REFERENCE.md"
        )
    );
    assert_eq!(
        call_read(&read, "scripts/run.sh").await,
        failed("only files under references/ and assets/ can be read")
    );
    assert_eq!(
        call_read(&read, "SKILL.md").await,
        failed("only files under references/ and assets/ can be read")
    );
    assert_eq!(
        call_read(&read, "references/none.md").await,
        failed("there is no such file")
    );
    assert_eq!(
        call_read(&read, "references").await,
        failed("that is not a regular file")
    );
    assert_eq!(
        call_read(&read, "references\\guide.md").await,
        failed("the path contains a backslash; use forward slashes")
    );
    assert_eq!(
        call_read(&read, "references/guide\0.md").await,
        failed("the path contains a NUL character")
    );
    assert_eq!(call_read(&read, "").await, failed("the path is empty"));
}

async fn call_read(read: &Arc<dyn Tool>, path: &str) -> Result<String, ToolError> {
    call(&**read, json!({"skill": "alpha", "path": path})).await
}

#[cfg(unix)]
#[tokio::test]
async fn a_link_out_of_the_skill_is_refused() {
    use crate::skills::test_support::symlink;
    let scratch = Scratch::new("linkout");
    scratch.write("secret.txt", "the host's");
    let catalog = two_skills(&scratch);
    let (_, read) = tools(&catalog);
    symlink(
        &scratch.join("secret.txt"),
        &scratch.join("skills/alpha/references/leak.md"),
    );
    assert_eq!(
        call_read(&read, "references/leak.md").await,
        failed(
            "the path leaves the directory it may be read from (a symbolic link points outside it)"
        )
    );
}

#[tokio::test]
async fn a_file_over_the_configured_size_is_refused_and_one_at_it_is_not() {
    let scratch = Scratch::new("readsize");
    scratch.skill("skills/alpha", "alpha", "d");
    scratch.write("skills/alpha/references/at.md", "x".repeat(50));
    scratch.write("skills/alpha/references/over.md", "x".repeat(51));
    let catalog = SkillCatalog::discover(&scratch.config().with_max_resource_bytes(50));
    let read = catalog.tools().remove(1);
    assert_eq!(
        call_read(&read, "references/at.md").await.map(|t| t.len()),
        Ok(50)
    );
    assert_eq!(
        call_read(&read, "references/over.md").await,
        failed("the file is larger than the 50 bytes this reader returns")
    );
}

#[tokio::test]
async fn a_file_that_is_not_text_is_refused() {
    let scratch = Scratch::new("readbinary");
    scratch.skill("skills/alpha", "alpha", "d");
    scratch.write(
        "skills/alpha/assets/logo.png",
        [0x89, 0x50, 0xff, 0xfe, 0x00],
    );
    let read = scratch.catalog().tools().remove(1);
    assert_eq!(
        call_read(&read, "assets/logo.png").await,
        failed("the file is not UTF-8 text")
    );
}
