// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Part of the tests in `mod.rs`.

use super::*;

fn name_findings(name: &str, directory: &str) -> (bool, Vec<(Severity, DiagnosticKind, String)>) {
    let text = file(&format!("name: \"{name}\"\ndescription: d"), "body");
    let (parsed, findings) = read_in(directory, &text);
    (
        parsed.is_some(),
        findings
            .into_iter()
            .map(|f| (f.severity, f.kind, f.message))
            .collect(),
    )
}

#[test]
fn a_name_that_follows_the_specification_says_nothing() {
    for name in [
        "pdf-processing",
        "a",
        "a1",
        "x-1-y",
        "名前",
        "é",
        "data-analysis",
    ] {
        assert_eq!(name_findings(name, name), (true, Vec::new()), "{name}");
    }
}

#[test]
fn a_name_longer_than_64_characters_is_used_with_a_warning() {
    let sixty_four = "a".repeat(64);
    assert_eq!(name_findings(&sixty_four, &sixty_four), (true, Vec::new()));

    let sixty_five = "a".repeat(65);
    let (loaded, findings) = name_findings(&sixty_five, &sixty_five);
    assert!(loaded);
    assert_eq!(
        findings,
        [(
            Severity::Warning,
            DiagnosticKind::NameNotSpec,
            format!(
                "the name '{sixty_five}' is longer than 64 characters (the specification as read on 2026-11-07)"
            )
        )]
    );

    let longest = "a".repeat(256);
    assert!(name_findings(&longest, &longest).0);
}

#[test]
fn a_name_over_256_characters_is_not_used() {
    let name = "a".repeat(257);
    let (loaded, findings) = name_findings(&name, &name);
    assert!(!loaded);
    assert_eq!(
        findings,
        [(
            Severity::Error,
            DiagnosticKind::NameInvalid,
            "the name is 257 characters; names over 256 are not used (the specification allows 64)"
                .to_owned()
        )]
    );
}

#[test]
fn a_name_with_a_character_a_prompt_cannot_carry_is_not_used() {
    for name in [
        "a b",
        "a\\\"b",
        "a<b",
        "a>b",
        "a&b",
        "a/b",
        "a\\\\b",
        "a\\u0001b",
        "a:b",
        "a\\u200bb",
        "a\\nb",
        "a'b",
        "a,b",
    ] {
        let (loaded, findings) = name_findings(name, "x");
        assert!(!loaded, "{name}");
        assert_eq!(
            findings.iter().map(|f| (f.0, f.1)).collect::<Vec<_>>(),
            [(Severity::Error, DiagnosticKind::NameInvalid)],
            "{name}"
        );
    }
    let (_, findings) = name_findings("a b", "x");
    assert_eq!(
        findings[0].2,
        "the name holds ' ', which a prompt cannot carry safely; only letters, digits, \
         hyphens, underscores and dots are used"
    );
}

#[test]
fn a_name_that_deviates_from_the_specification_is_used_and_each_deviation_is_said() {
    let said = |name: &str| {
        let (loaded, findings) = name_findings(name, name);
        assert!(loaded, "{name}");
        findings
            .into_iter()
            .map(|(severity, kind, message)| {
                assert_eq!(
                    (severity, kind),
                    (Severity::Warning, DiagnosticKind::NameNotSpec)
                );
                message
            })
            .collect::<Vec<_>>()
    };
    let suffix = "(the specification as read on 2026-11-07)";
    assert_eq!(
        said("PDF"),
        [format!("the name 'PDF' has an uppercase letter {suffix}")]
    );
    assert_eq!(
        said("Éclair"),
        [format!(
            "the name 'Éclair' has an uppercase letter {suffix}"
        )]
    );
    assert_eq!(
        said("my_skill"),
        [format!(
            "the name 'my_skill' has a character other than letters, digits and hyphens {suffix}"
        )]
    );
    assert_eq!(
        said("v1.2"),
        [format!(
            "the name 'v1.2' has a character other than letters, digits and hyphens {suffix}"
        )]
    );
    assert_eq!(
        said("-pdf"),
        [format!(
            "the name '-pdf' starts or ends with a hyphen {suffix}"
        )]
    );
    assert_eq!(
        said("pdf-"),
        [format!(
            "the name 'pdf-' starts or ends with a hyphen {suffix}"
        )]
    );
    assert_eq!(
        said("a--b"),
        [format!("the name 'a--b' has two hyphens together {suffix}")]
    );
    assert_eq!(
        said("-A--b_"),
        [format!(
            "the name '-A--b_' has an uppercase letter, and has a character other than letters, \
             digits and hyphens, and starts or ends with a hyphen, and has two hyphens together {suffix}"
        )]
    );
}

#[test]
fn a_name_that_is_not_the_directorys_is_used_under_its_own_with_a_warning() {
    let (loaded, findings) = name_findings("pdf-tools", "pdf");
    assert!(loaded);
    assert_eq!(
        findings,
        [(
            Severity::Warning,
            DiagnosticKind::NameMismatch,
            "the name 'pdf-tools' is not the directory's name 'pdf'; the skill is used under 'pdf-tools'"
                .to_owned()
        )]
    );
    let (parsed, _) = read_in("pdf", &file("name: pdf-tools\ndescription: d", "b"));
    assert_eq!(parsed.unwrap().name, "pdf-tools");
}

#[test]
fn a_description_over_1024_characters_is_used_with_a_warning() {
    let at_limit = "d".repeat(1024);
    let (parsed, findings) = read(&file(&format!("name: demo\ndescription: {at_limit}"), "b"));
    assert_eq!(findings, []);
    assert_eq!(parsed.unwrap().description.chars().count(), 1024);

    let over = "d".repeat(1025);
    let (parsed, findings) = read(&file(&format!("name: demo\ndescription: {over}"), "b"));
    assert!(parsed.is_some());
    assert_eq!(
        kinds(&findings),
        [(Severity::Warning, DiagnosticKind::DescriptionTooLong)]
    );
    assert_eq!(
        findings[0].message,
        "the description is 1025 characters; the specification allows 1024"
    );
    // Characters, not bytes.
    let wide = "é".repeat(1024);
    assert_eq!(
        read(&file(&format!("name: demo\ndescription: {wide}"), "b")).1,
        []
    );
}
