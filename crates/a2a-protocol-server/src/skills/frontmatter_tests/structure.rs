// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Part of the tests in `mod.rs`.

use super::*;

#[test]
fn a_minimal_skill_is_loaded_with_nothing_said() {
    let parsed = loaded(
        "---\nname: demo\ndescription: Does a demo. Use when demoing.\n---\n# Demo\n\nSteps.\n",
    );
    assert_eq!(
        parsed,
        Parsed {
            name: "demo".to_owned(),
            description: "Does a demo. Use when demoing.".to_owned(),
            license: None,
            compatibility: None,
            allowed_tools: None,
            metadata: Vec::new(),
            body: "# Demo\n\nSteps.".to_owned(),
        }
    );
}

#[test]
fn the_specifications_own_example_is_loaded() {
    let parsed = loaded(
        "---\nname: demo\ndescription: Extract PDF text, fill forms, merge files. Use when handling PDFs.\nlicense: Apache-2.0\nmetadata:\n  author: example-org\n  version: \"1.0\"\n---\nbody",
    );
    assert_eq!(parsed.license.as_deref(), Some("Apache-2.0"));
    assert_eq!(
        parsed.metadata,
        [
            ("author".to_owned(), "example-org".to_owned()),
            ("version".to_owned(), "1.0".to_owned())
        ]
    );
}

#[test]
fn a_byte_order_mark_and_crlf_line_ends_are_read() {
    let parsed =
        loaded("\u{feff}---\r\nname: demo\r\ndescription: Works.\r\n---\r\nbody\r\nmore\r\n");
    assert_eq!(parsed.name, "demo");
    assert_eq!(parsed.description, "Works.");
    assert_eq!(parsed.body, "body\r\nmore");
}

#[test]
fn the_delimiter_lines_may_end_in_white_space() {
    let parsed = loaded("---  \nname: demo\ndescription: d\n---\t \nbody");
    assert_eq!(parsed.body, "body");
}

#[test]
fn a_file_without_a_frontmatter_opening_is_reported() {
    for text in [
        "",
        "no frontmatter",
        "# Title\n---\nname: x\n---\n",
        "----\nname: x\n---\n",
        "---",
    ] {
        let (parsed, findings) = read(text);
        assert_eq!(parsed, None, "{text:?}");
        assert_eq!(
            kinds(&findings),
            [(Severity::Error, DiagnosticKind::FrontmatterMissing)],
            "{text:?}"
        );
    }
    assert_eq!(
        read("x").1[0].message,
        "the file does not start with a `---` line"
    );
}

#[test]
fn a_frontmatter_that_is_not_closed_is_reported() {
    let (parsed, findings) = read("---\nname: demo\ndescription: d\nbody never comes");
    assert_eq!(parsed, None);
    assert_eq!(
        kinds(&findings),
        [(Severity::Error, DiagnosticKind::FrontmatterUnclosed)]
    );
}

/// A frontmatter of exactly `bytes` bytes between the delimiters: the two
/// fields and a comment to pad.
fn frontmatter_of(bytes: usize) -> String {
    let fields = "name: demo\ndescription: d\n";
    let pad = bytes - fields.len();
    format!("---\n{fields}#{}\n---\nbody", "x".repeat(pad - 2))
}

#[test]
fn the_frontmatter_bound_is_exactly_8192_bytes() {
    let (parsed, findings) = read(&frontmatter_of(8192));
    assert_eq!(findings, []);
    assert!(parsed.is_some());

    let (parsed, findings) = read(&frontmatter_of(8193));
    assert_eq!(parsed, None);
    assert_eq!(
        kinds(&findings),
        [(Severity::Error, DiagnosticKind::FrontmatterTooLarge)]
    );
    assert_eq!(
        findings[0].message,
        "the frontmatter is over 8192 bytes and was not parsed"
    );
    let (_, findings) = read(&frontmatter_of(8191));
    assert_eq!(findings, []);
}

#[test]
fn the_frontmatter_is_measured_before_it_is_read_as_yaml() {
    // Over the bound and not YAML at all: the size is what is reported.
    let hostile = format!("---\n{}\n---\nbody", "- [".repeat(4000));
    let (parsed, findings) = read(&hostile);
    assert_eq!(parsed, None);
    assert_eq!(
        kinds(&findings),
        [(Severity::Error, DiagnosticKind::FrontmatterTooLarge)]
    );
    // And with no closing line at all, the bound still stops the scan.
    let endless = format!("---\n{}", "#x\n".repeat(5000));
    assert_eq!(
        kinds(&read(&endless).1),
        [(Severity::Error, DiagnosticKind::FrontmatterTooLarge)]
    );
}

#[test]
fn a_huge_value_is_refused_not_parsed() {
    let huge = format!(
        "---\nname: demo\ndescription: {}\n---\nbody",
        "a".repeat(1_000_000)
    );
    let (parsed, findings) = read(&huge);
    assert_eq!(parsed, None);
    assert_eq!(
        kinds(&findings),
        [(Severity::Error, DiagnosticKind::FrontmatterTooLarge)]
    );
}

#[test]
fn a_frontmatter_whose_shape_is_lost_is_invalid_with_its_line_in_the_file() {
    let (parsed, findings) = read("---\nname: demo\njust some words\n---\nbody");
    assert_eq!(parsed, None);
    assert_eq!(
        kinds(&findings),
        [(Severity::Error, DiagnosticKind::FrontmatterInvalid)]
    );
    assert_eq!(
        findings[0].message,
        "line 3 of the file: a line is not a `key: value` entry"
    );
}

#[test]
fn name_and_description_are_required() {
    let (parsed, findings) = read(&file("license: x", "body"));
    assert_eq!(parsed, None);
    assert_eq!(
        kinds(&findings),
        [
            (Severity::Error, DiagnosticKind::FieldMissing),
            (Severity::Error, DiagnosticKind::FieldMissing)
        ],
        "both are said, not only the first"
    );
    assert_eq!(findings[0].message, "`name` is required and absent");
    assert_eq!(findings[1].message, "`description` is required and absent");

    let (parsed, findings) = read(&file("name: demo", "body"));
    assert_eq!(parsed, None);
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].message, "`description` is required and absent");

    let (parsed, findings) = read(&file("description: d", "body"));
    assert_eq!(parsed, None);
    assert_eq!(findings[0].message, "`name` is required and absent");
}

#[test]
fn an_empty_name_or_description_is_missing() {
    for frontmatter in [
        "name:\ndescription: d",
        "name: \"\"\ndescription: d",
        "name: '  '\ndescription: d",
        "name: demo\ndescription:",
        "name: demo\ndescription: >-\n  \n",
    ] {
        let (parsed, findings) = read(&file(frontmatter, "body"));
        assert_eq!(parsed, None, "{frontmatter:?}");
        assert_eq!(
            kinds(&findings),
            [(Severity::Error, DiagnosticKind::FieldMissing)],
            "{frontmatter:?}"
        );
        assert!(
            findings[0].message.ends_with("is required and empty"),
            "{frontmatter:?}"
        );
    }
}

#[test]
fn a_name_or_description_that_is_not_a_string_is_invalid() {
    for (frontmatter, said) in [
        (
            "name: [a, b]\ndescription: d",
            "`name` is not a string: a flow collection",
        ),
        (
            "name: *alias\ndescription: d",
            "`name` is not a string: an anchor, an alias or a tag",
        ),
        (
            "name:\n  a: b\ndescription: d",
            "`name` is not a string: a mapping",
        ),
        (
            "name: demo\ndescription:\n  - a",
            "`description` is not a string: a sequence",
        ),
        (
            "name: demo\ndescription: \"open",
            "`description` is not a string: a quoted value is not closed",
        ),
    ] {
        let (parsed, findings) = read(&file(frontmatter, "body"));
        assert_eq!(parsed, None, "{frontmatter:?}");
        assert_eq!(
            kinds(&findings),
            [(Severity::Error, DiagnosticKind::FieldInvalid)],
            "{frontmatter:?}"
        );
        assert_eq!(findings[0].message, said);
    }
}

#[test]
fn a_description_may_be_a_block_or_quoted_and_is_trimmed() {
    assert_eq!(
        loaded(&file(
            "name: demo\ndescription: >\n  Folds across\n  lines.\n",
            "b"
        ))
        .description,
        "Folds across lines."
    );
    assert_eq!(
        loaded(&file(
            "name: demo\ndescription: \"  Quoted: with a colon  \"",
            "b"
        ))
        .description,
        "Quoted: with a colon"
    );
}

/// The entry point the fuzz target calls, which is built for it and for this.
#[test]
fn the_fuzz_entry_counts_the_findings_a_file_makes() {
    // `A` is not lowercase and is not the directory the entry names: two.
    assert_eq!(fuzz("---\nname: A\ndescription: d\n---\nbody"), 2);
    assert_eq!(fuzz("no frontmatter"), 1);
    assert_eq!(fuzz("---\nname: fuzz\ndescription: d\n---\nbody"), 0);
}
