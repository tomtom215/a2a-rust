// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Part of the tests in `mod.rs`.

use super::*;

#[test]
fn the_optional_fields_are_read() {
    let parsed = loaded(&file(
        "name: demo\ndescription: d\nlicense: Proprietary. LICENSE.txt has complete terms\ncompatibility: Requires git and jq\nmetadata:\n  a: b",
        "b",
    ));
    assert_eq!(
        parsed.license.as_deref(),
        Some("Proprietary. LICENSE.txt has complete terms")
    );
    assert_eq!(parsed.compatibility.as_deref(), Some("Requires git and jq"));
    assert_eq!(parsed.metadata, [("a".to_owned(), "b".to_owned())]);
}

#[test]
fn compatibility_is_at_most_500_characters() {
    let ok = "c".repeat(500);
    let parsed = loaded(&file(
        &format!("name: demo\ndescription: d\ncompatibility: {ok}"),
        "b",
    ));
    assert_eq!(parsed.compatibility, Some(ok));

    let over = "c".repeat(501);
    let (parsed, findings) = read(&file(
        &format!("name: demo\ndescription: d\ncompatibility: {over}"),
        "b",
    ));
    assert_eq!(parsed.unwrap().compatibility, None);
    assert_eq!(
        kinds(&findings),
        [(Severity::Warning, DiagnosticKind::OptionalFieldIgnored)]
    );
    assert_eq!(
        findings[0].message,
        "`compatibility` is ignored: it is longer than the 500 characters the specification allows"
    );
}

#[test]
fn an_optional_field_that_is_empty_or_not_a_string_is_ignored_with_a_warning() {
    for (frontmatter, said) in [
        ("license:", "`license` is ignored: it is empty"),
        ("license: ''", "`license` is ignored: it is empty"),
        (
            "compatibility: \"\"",
            "`compatibility` is ignored: it is empty",
        ),
        (
            "license: [MIT]",
            "`license` is ignored: it is not a string: a flow collection",
        ),
        (
            "compatibility:\n  a: b",
            "`compatibility` is ignored: it is not a string: a mapping",
        ),
        (
            "allowed-tools: [Read]",
            "`allowed-tools` is ignored: it is not a string: a flow collection",
        ),
        (
            "allowed-tools:\n  - Read",
            "`allowed-tools` is ignored: it is not a string: a sequence",
        ),
        (
            "metadata: text",
            "`metadata` is ignored: it is not a mapping of strings: a string",
        ),
        (
            "metadata:\n  a: [b]",
            "`metadata` is ignored: it is not a mapping of strings: a mapping value that is not a string",
        ),
        (
            "metadata:\n  - a",
            "`metadata` is ignored: it is not a mapping of strings: a sequence",
        ),
    ] {
        let (parsed, findings) = read(&file(
            &format!("name: demo\ndescription: d\n{frontmatter}"),
            "b",
        ));
        let parsed = parsed.unwrap_or_else(|| panic!("{frontmatter:?} still loads"));
        assert_eq!(
            (
                parsed.license,
                parsed.compatibility,
                parsed.allowed_tools,
                parsed.metadata
            ),
            (None, None, None, Vec::new()),
            "{frontmatter:?}"
        );
        assert_eq!(
            kinds(&findings),
            [(Severity::Warning, DiagnosticKind::OptionalFieldIgnored)],
            "{frontmatter:?}"
        );
        assert_eq!(findings[0].message, said);
    }
}

#[test]
fn a_description_may_start_on_the_line_below_its_key() {
    let parsed = loaded(&file(
        "name: demo\ndescription:\n  Does a demo.\n  Use when demoing.",
        "b",
    ));
    assert_eq!(parsed.description, "Does a demo. Use when demoing.");
}

#[test]
fn an_empty_metadata_is_just_none_and_says_nothing() {
    let (parsed, findings) = read(&file("name: demo\ndescription: d\nmetadata:", "b"));
    assert_eq!(findings, []);
    assert_eq!(parsed.unwrap().metadata, Vec::<(String, String)>::new());
}

#[test]
fn allowed_tools_is_recorded_and_said_to_be_unused() {
    let (parsed, findings) = read(&file(
        "name: demo\ndescription: d\nallowed-tools: Bash(git:*) Read",
        "b",
    ));
    assert_eq!(
        parsed.unwrap().allowed_tools.as_deref(),
        Some("Bash(git:*) Read")
    );
    assert_eq!(
        kinds(&findings),
        [(Severity::Info, DiagnosticKind::AllowedToolsIgnored)]
    );
    assert!(findings[0].message.contains("never used"));
}

#[test]
fn a_field_the_specification_does_not_define_is_named_with_the_date_it_was_read() {
    let (parsed, findings) = read(&file(
        "name: demo\ndescription: d\ndisable-model-invocation: true\nversion: 2",
        "b",
    ));
    assert!(parsed.is_some());
    assert_eq!(
        kinds(&findings),
        [
            (Severity::Warning, DiagnosticKind::UnknownField),
            (Severity::Warning, DiagnosticKind::UnknownField)
        ]
    );
    assert_eq!(
        findings[0].message,
        "`disable-model-invocation` is not a field of the Agent Skills specification as read on \
         2026-11-07, which has no versioned releases; it is ignored"
    );
    assert!(findings[1].message.starts_with("`version` "));
}

#[test]
fn an_unknown_field_in_a_style_that_is_not_read_costs_nothing() {
    let (parsed, findings) = read(&file(
        "name: demo\ndescription: d\nx: &a [1, 2]\ny:\n  - *a\n  - {z: 1}\nz: !!binary |\n  R0lG",
        "b",
    ));
    assert!(parsed.is_some());
    assert_eq!(findings.len(), 3);
    assert!(
        findings
            .iter()
            .all(|f| f.kind == DiagnosticKind::UnknownField)
    );
}

#[test]
fn a_duplicate_field_keeps_the_first_and_says_so_with_its_line() {
    let (parsed, findings) = read(&file(
        "name: demo\ndescription: first\ndescription: second",
        "b",
    ));
    assert_eq!(parsed.unwrap().description, "first");
    assert_eq!(
        kinds(&findings),
        [(Severity::Warning, DiagnosticKind::DuplicateField)]
    );
    assert_eq!(
        findings[0].message,
        "`description` appears twice (line 4 of the file); the first is used"
    );
}

#[test]
fn a_colon_in_a_plain_value_is_read_and_said_with_its_line() {
    let (parsed, findings) = read(&file(
        "name: demo\ndescription: Use this skill when: the user asks about PDFs",
        "b",
    ));
    assert_eq!(
        parsed.unwrap().description,
        "Use this skill when: the user asks about PDFs"
    );
    assert_eq!(
        kinds(&findings),
        [(Severity::Warning, DiagnosticKind::LenientYaml)]
    );
    assert_eq!(
        findings[0].message,
        "`description` (line 3 of the file) holds `: ` in a plain value; read as text, but a \
         strict YAML parser refuses it: quote the value"
    );
}

#[test]
fn an_alias_in_name_or_description_is_refused_not_expanded() {
    let bomb =
        "x: &a [\"lol\",\"lol\"]\ny: &b [*a,*a,*a,*a,*a,*a,*a,*a,*a]\nname: *b\ndescription: *a";
    let (parsed, findings) = read(&file(bomb, "body"));
    assert_eq!(parsed, None);
    assert_eq!(
        findings
            .iter()
            .filter(|f| f.kind == DiagnosticKind::FieldInvalid)
            .map(|f| f.message.as_str())
            .collect::<Vec<_>>(),
        [
            "`name` is not a string: an anchor, an alias or a tag",
            "`description` is not a string: an anchor, an alias or a tag"
        ]
    );
}

#[test]
fn the_body_is_trimmed_of_blank_lines_and_trailing_space_but_keeps_its_indent() {
    let parsed =
        loaded("---\nname: demo\ndescription: d\n---\n\n\n  indented first line\nsecond\n\n  \n");
    assert_eq!(parsed.body, "  indented first line\nsecond");
}

#[test]
fn an_empty_body_is_loaded_with_a_warning() {
    let (parsed, findings) = read("---\nname: demo\ndescription: d\n---\n");
    assert_eq!(parsed.unwrap().body, "");
    assert_eq!(
        kinds(&findings),
        [(Severity::Warning, DiagnosticKind::BodyEmpty)]
    );
    let (_, findings) = read("---\nname: demo\ndescription: d\n---\n \n\n");
    assert_eq!(
        kinds(&findings),
        [(Severity::Warning, DiagnosticKind::BodyEmpty)]
    );
    assert_eq!(
        read("---\nname: demo\ndescription: d\n---").1[0].kind,
        DiagnosticKind::BodyEmpty
    );
}

#[test]
fn the_body_bound_is_exact() {
    let text_with = |body: &str| format!("---\nname: demo\ndescription: d\n---\n{body}");
    let mut findings = Vec::new();
    let at = parse(&text_with(&"b".repeat(100)), "demo", 100, &mut findings);
    assert_eq!(findings, []);
    assert_eq!(at.unwrap().body.len(), 100);

    let under = parse(&text_with(&"b".repeat(99)), "demo", 100, &mut findings);
    assert!(under.is_some());
    assert_eq!(findings, []);

    let over = parse(&text_with(&"b".repeat(101)), "demo", 100, &mut findings);
    assert_eq!(over, None);
    assert_eq!(
        kinds(&findings),
        [(Severity::Error, DiagnosticKind::BodyTooLarge)]
    );
    assert_eq!(
        findings[0].message,
        "the instructions are 101 bytes, over the 100 this catalog keeps; a truncated procedure \
         would be worse than none, so the skill is skipped"
    );
}

#[test]
fn a_body_over_500_lines_warns_and_500_does_not() {
    let body = |lines: usize| vec!["line"; lines].join("\n");
    let (parsed, findings) = read(&file("name: demo\ndescription: d", &body(500)));
    assert_eq!(findings, []);
    assert!(parsed.is_some());

    let (parsed, findings) = read(&file("name: demo\ndescription: d", &body(501)));
    assert!(parsed.is_some());
    assert_eq!(
        kinds(&findings),
        [(Severity::Warning, DiagnosticKind::BodyLong)]
    );
    assert_eq!(
        findings[0].message,
        "the instructions are 501 lines; the specification recommends under 500 and moving the rest to `references/`"
    );
}

#[test]
fn a_skill_with_findings_of_several_kinds_reports_them_in_the_order_found() {
    let (parsed, findings) = read_in(
        "other",
        &file(
            "name: Demo\ndescription: d\nlicense: MIT\nextra: 1\nextra: 2",
            "b",
        ),
    );
    assert!(parsed.is_some());
    assert_eq!(
        kinds(&findings),
        [
            (Severity::Warning, DiagnosticKind::UnknownField),
            (Severity::Warning, DiagnosticKind::DuplicateField),
            (Severity::Warning, DiagnosticKind::NameNotSpec),
            (Severity::Warning, DiagnosticKind::NameMismatch),
        ]
    );
}

#[test]
fn the_finding_becomes_a_diagnostic_about_a_path() {
    let finding = Finding::new(Severity::Info, DiagnosticKind::BodyEmpty, "empty");
    let diagnostic = finding.at(std::path::Path::new("/skills/demo/SKILL.md"));
    assert_eq!(diagnostic.severity(), Severity::Info);
    assert_eq!(diagnostic.kind(), DiagnosticKind::BodyEmpty);
    assert_eq!(
        diagnostic.path(),
        std::path::Path::new("/skills/demo/SKILL.md")
    );
    assert_eq!(diagnostic.message(), "empty");
}
