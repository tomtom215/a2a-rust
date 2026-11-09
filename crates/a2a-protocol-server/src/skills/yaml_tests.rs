// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The YAML subset, read directly: every scalar style, the folding rules,
//! what is refused and what is merely not read.

use std::fmt::Write as _;

use super::*;

fn only(block: &str) -> Entry {
    let mut entries = parse(block).expect("the block parses");
    assert_eq!(entries.len(), 1, "one entry in {block:?}: {entries:?}");
    entries.remove(0)
}

fn text(block: &str) -> String {
    match only(block).value {
        Value::Text(text) => text,
        other => panic!("{block:?} is not text: {other:?}"),
    }
}

fn value(block: &str) -> Value {
    only(block).value
}

#[test]
fn an_empty_block_has_no_entries() {
    assert_eq!(parse("").unwrap(), []);
    assert_eq!(parse("\n\n# only a comment\n   \n").unwrap(), []);
}

#[test]
fn an_entry_is_a_key_a_value_and_its_line() {
    let entries = parse("name: pdf-processing\n\n# note\ndescription: Does PDFs\n").unwrap();
    assert_eq!(
        entries,
        [
            Entry {
                key: "name".to_owned(),
                value: Value::Text("pdf-processing".to_owned()),
                line: 1,
                lenient: false,
            },
            Entry {
                key: "description".to_owned(),
                value: Value::Text("Does PDFs".to_owned()),
                line: 4,
                lenient: false,
            },
        ]
    );
}

#[test]
fn a_blank_first_line_is_skipped_not_looped_on() {
    assert_eq!(only("\nname: a").line, 2);
}

#[test]
fn an_indented_first_line_loses_the_shape() {
    assert_eq!(
        parse("  name: a").unwrap_err(),
        YamlError {
            line: 1,
            reason: "an entry is indented"
        }
    );
    assert_eq!(
        parse("name: a\n# c\n\tbad: x").unwrap_err().line,
        3,
        "an indented line after a comment is not part of the entry above it"
    );
}

#[test]
fn a_line_without_a_key_loses_the_shape() {
    for bad in ["just words", "- a\n- b", "- a: b", "? a: b", ": a", "a:b"] {
        let error = parse(bad).unwrap_err();
        assert_eq!(error.line, 1, "{bad:?}");
        assert_eq!(
            error.reason, "a line is not a `key: value` entry",
            "{bad:?}"
        );
    }
    assert_eq!(parse("name: a\nnope").unwrap_err().line, 2);
}

#[test]
fn a_colon_must_end_the_line_or_precede_a_space_to_close_a_key() {
    let entry = only("url:http://x: y");
    assert_eq!(entry.key, "url:http://x");
    assert_eq!(entry.value, Value::Text("y".to_owned()));
    assert_eq!(only("key:\tvalue").value, Value::Text("value".to_owned()));
    assert_eq!(value("key:"), Value::Text(String::new()));
}

#[test]
fn quoted_keys_are_read() {
    assert_eq!(only("\"a b\": c").key, "a b");
    assert_eq!(only("'a b': c").key, "a b");
    assert_eq!(only("'it''s': c").key, "it's");
    assert_eq!(parse("'a'x: c").unwrap_err().line, 1);
    assert_eq!(parse("'a' b").unwrap_err().line, 1);
    assert_eq!(parse("'a: b").unwrap_err().line, 1);
    assert_eq!(only("'a':").value, Value::Text(String::new()));
}

#[test]
fn a_plain_scalar_is_trimmed_and_loses_its_comment() {
    assert_eq!(text("k:   hello world   "), "hello world");
    assert_eq!(text("k: hello # a comment"), "hello");
    assert_eq!(text("k: hello\t# a comment"), "hello");
    assert_eq!(text("k: C# is fine"), "C# is fine");
    assert_eq!(text("k: #only a comment\n"), "");
    assert_eq!(text("k: a #b # c"), "a");
}

#[test]
fn a_plain_scalar_folds_its_continuation_lines() {
    assert_eq!(text("k: one\n  two\n   three"), "one two three");
    assert_eq!(text("k: one\n\n  two"), "one\ntwo");
    assert_eq!(text("k: one\n\n\n  two"), "one\n\ntwo");
    assert_eq!(text("k: one\n  # a comment\n  two"), "one two");
    assert_eq!(text("k: one\n  two # trailing\n  three"), "one two three");
    assert_eq!(text("k: one\n\n"), "one");
}

#[test]
fn a_colon_in_a_plain_value_is_read_as_text_and_noted() {
    let lenient = only("description: Use this when: the user asks");
    assert_eq!(
        lenient.value,
        Value::Text("Use this when: the user asks".to_owned())
    );
    assert!(lenient.lenient);
    assert!(only("k: ends with a colon:").lenient);
    assert!(only("k: tab:\there").lenient);
    assert!(only("k: first\n  second: line").lenient);
    assert!(!only("k: a:b").lenient);
    assert!(!only("k: plain").lenient);
    assert!(!only("k: \"quoted: colon\"").lenient);
    assert!(!only("k: | \n  block: colon").lenient);
}

#[test]
fn a_double_quoted_scalar_has_escapes() {
    assert_eq!(text(r#"k: "a\tb\nc""#), "a\tb\nc");
    for (escape, expected) in [
        ("0", '\0'),
        ("a", '\x07'),
        ("b", '\x08'),
        ("t", '\t'),
        ("n", '\n'),
        ("v", '\x0b'),
        ("f", '\x0c'),
        ("r", '\r'),
        ("e", '\x1b'),
        (" ", ' '),
        ("\"", '"'),
        ("/", '/'),
        ("\\", '\\'),
        ("N", '\u{85}'),
        ("_", '\u{a0}'),
        ("L", '\u{2028}'),
        ("P", '\u{2029}'),
        ("x41", 'A'),
        ("u00e9", 'é'),
        ("U0001F600", '😀'),
    ] {
        assert_eq!(
            text(&format!("k: \"[\\{escape}]\"")),
            format!("[{expected}]"),
            "\\{escape}"
        );
    }
    assert_eq!(text("k: \"a\\\tb\""), "a\tb", "an escaped tab");
}

#[test]
fn a_bad_escape_is_malformed_against_its_key() {
    for bad in [
        r#"k: "\q""#,
        r#"k: "\x4""#,
        r#"k: "\xZZ""#,
        r#"k: "\u12""#,
        r#"k: "\UFFFFFFFF""#,
        r#"k: "\uD800""#,
        "k: \"\\",
    ] {
        assert!(
            matches!(value(bad), Value::Malformed(_)),
            "{bad:?} gave {:?}",
            value(bad)
        );
    }
    assert_eq!(
        value(r#"k: "\q""#),
        Value::Malformed("a double-quoted value has an escape that is not valid")
    );
    // Single quotes have no escapes.
    assert_eq!(text(r"k: '\q'"), r"\q");
}

#[test]
fn a_single_quote_is_doubled_inside_single_quotes() {
    assert_eq!(text("k: 'it''s'"), "it's");
    assert_eq!(text("k: ''''"), "'");
    assert_eq!(text("k: ''"), "");
    assert_eq!(text("k: 'a'  # comment"), "a");
}

#[test]
fn a_quoted_scalar_that_is_not_closed_is_malformed() {
    assert_eq!(
        value("k: \"open"),
        Value::Malformed("a quoted value is not closed")
    );
    assert_eq!(
        value("k: 'open"),
        Value::Malformed("a quoted value is not closed")
    );
}

#[test]
fn text_after_the_closing_quote_is_malformed() {
    assert_eq!(
        value("k: \"a\" b"),
        Value::Malformed("text follows the closing quote")
    );
    assert_eq!(
        value("k: \"a\"\n  b"),
        Value::Malformed("text follows the closing quote")
    );
    assert_eq!(text("k: \"a\"\n\n  # comment\n"), "a");
}

#[test]
fn a_quoted_scalar_folds_its_lines() {
    assert_eq!(text("k: \"one\n  two\""), "one two");
    assert_eq!(
        text("k: \"one  \n   two\""),
        "one two",
        "trailing white space goes"
    );
    // White space at the end of a continuation line goes as well (the first
    // line's is trimmed before the scalar is read).
    assert_eq!(text("k: \"one\n  two  \n  three\""), "one two three");
    assert_eq!(text("k: \"one\n  two\t \t\n  three\""), "one two three");
    assert_eq!(text("k: 'one\n  two \n\n  three'"), "one two\nthree");
    // An escaped space is not trailing white space.
    assert_eq!(text("k: \"one\n  two\\ \n  three\""), "one two  three");
    assert_eq!(text("k: \"one\n\n  two\""), "one\ntwo");
    assert_eq!(text("k: \"one\n\n\n  two\""), "one\n\ntwo");
    assert_eq!(text("k: 'one\n  two'"), "one two");
    assert_eq!(text("k: \"keep\\t \n  this\""), "keep\t this");
    assert_eq!(
        text("k: \"a\\\n   b\""),
        "ab",
        "an escaped line break joins"
    );
    assert_eq!(text("k: \"a \\\n   b\""), "a b");
}

#[test]
fn a_literal_block_keeps_its_lines() {
    assert_eq!(text("k: |\n  one\n  two\n"), "one\ntwo\n");
    assert_eq!(text("k: |-\n  one\n  two\n"), "one\ntwo");
    assert_eq!(text("k: |+\n  one\n  two\n\n\n"), "one\ntwo\n\n\n");
    assert_eq!(text("k: |\n  one\n\n  two\n"), "one\n\ntwo\n");
    assert_eq!(text("k: |\n    deep\n  \n    deeper\n"), "deep\n\ndeeper\n");
    assert_eq!(text("k: |\n  a\n    more\n  b"), "a\n  more\nb\n");
}

#[test]
fn a_block_header_takes_a_chomp_and_an_indent_in_either_order() {
    assert_eq!(text("k: |2\n    x\n"), "  x\n");
    assert_eq!(text("k: |2-\n    x\n"), "  x");
    assert_eq!(text("k: |-2\n    x\n"), "  x");
    assert_eq!(text("k: |+1\n  x\n\n"), " x\n\n");
    assert_eq!(text("k: | # why\n  x\n"), "x\n");
    for bad in [
        "k: |x\n  a",
        "k: |--\n  a",
        "k: |22\n  a",
        "k: |0\n  a",
        "k: | text\n  a",
    ] {
        assert_eq!(
            value(bad),
            Value::Malformed("a block scalar's header is not valid"),
            "{bad:?}"
        );
    }
}

#[test]
fn a_block_line_indented_less_than_the_block_is_malformed() {
    assert_eq!(
        value("k: |4\n  short\n"),
        Value::Malformed("a block scalar has a line indented less than its first")
    );
    assert_eq!(
        value("k: |\n\tbad\n"),
        Value::Malformed("a block scalar has a line indented less than its first")
    );
}

#[test]
fn an_empty_block_is_empty() {
    assert_eq!(text("k: |\n"), "");
    assert_eq!(text("k: >-\n\n  \n"), "");
    assert_eq!(text("k: |+\n\n\n"), "\n\n");
    // With an indentation indicator the indent is known, and there is still
    // no content: clip adds no break, keep keeps the blank lines and no more.
    assert_eq!(text("k: |2\n\n"), "");
    assert_eq!(text("k: >2-\n\n"), "");
    assert_eq!(text("k: |2+\n\n\n"), "\n\n");
    assert_eq!(text("k: >2+\n\n"), "\n");
    // A block ends at the next line that is not indented.
    let entries = parse("k: |\n  x\nj: y").unwrap();
    assert_eq!(entries[0].value, Value::Text("x\n".to_owned()));
    assert_eq!(entries[1].key, "j");
}

#[test]
fn a_folded_block_folds_single_breaks_and_keeps_blank_lines() {
    assert_eq!(text("k: >\n  one\n  two\n"), "one two\n");
    assert_eq!(text("k: >-\n  one\n  two\n"), "one two");
    assert_eq!(text("k: >\n  one\n\n  two\n"), "one\ntwo\n");
    assert_eq!(text("k: >\n  one\n\n\n  two\n"), "one\n\ntwo\n");
    assert_eq!(text("k: >+\n  one\n\n"), "one\n\n");
    assert_eq!(
        text("k: >\n  one\n    indented\n  two\n"),
        "one\n  indented\ntwo\n"
    );
    assert_eq!(
        text("k: >\n  one\n\n    indented\n  two\n"),
        "one\n\n  indented\ntwo\n"
    );
    assert_eq!(text("k: >\n  a\n  b\n  c\n"), "a b c\n");
}

#[test]
fn a_nested_mapping_of_scalars_is_a_map() {
    assert_eq!(
        value("metadata:\n  author: example-org\n  version: \"1.0\"\n  tag: 'x y'\n"),
        Value::Map(vec![
            ("author".to_owned(), "example-org".to_owned()),
            ("version".to_owned(), "1.0".to_owned()),
            ("tag".to_owned(), "x y".to_owned()),
        ])
    );
    assert_eq!(
        value("metadata:\n  # note\n  a: b # c\n\n  d:\n"),
        Value::Map(vec![
            ("a".to_owned(), "b".to_owned()),
            ("d".to_owned(), String::new())
        ])
    );
    assert_eq!(
        value("metadata:\n    a: b\n    c: d"),
        Value::Map(vec![
            ("a".to_owned(), "b".to_owned()),
            ("c".to_owned(), "d".to_owned()),
        ])
    );
}

#[test]
fn what_is_nested_further_or_not_a_string_is_not_read() {
    let spans = Value::Unsupported("a mapping that is nested, spans lines or is tab-indented");
    assert_eq!(value("metadata:\n  a:\n    b: c\n"), spans);
    assert_eq!(value("metadata:\n  a: b\n   c: d\n"), spans);
    assert_eq!(value("metadata:\n  a: b\n c: d\n"), spans);
    assert_eq!(
        value("metadata:\n  just words\n"),
        Value::Text("just words".to_owned()),
        "a scalar below its key, which `metadata` then reports as not a mapping"
    );
    assert_eq!(value("metadata:\n\ta: b\n"), spans);
    assert_eq!(value("metadata:\n  a: b\n\n\tc: d\n"), spans);
    assert_eq!(
        value("metadata:\n  a: [b]\n"),
        Value::Unsupported("a mapping value that is not a string")
    );
    assert_eq!(
        value("metadata:\n  a: \"b\n"),
        Value::Unsupported("a mapping value that is not a string")
    );
}

#[test]
fn a_scalar_on_the_line_below_its_key_is_read_as_if_it_followed_the_colon() {
    assert_eq!(text("k:\n  hello world"), "hello world");
    assert_eq!(text("k:\n  hello\n  world"), "hello world");
    assert_eq!(text("k:\n  # a comment\n\n  hello # trailing\n"), "hello");
    assert_eq!(text("k:\n  \"quoted: text\""), "quoted: text");
    assert_eq!(text("k:\n  'it''s'"), "it's");
    assert_eq!(text("k: # about k\n  hello"), "hello");
    assert_eq!(
        text("k:\n  |\n    literal\n    lines\n"),
        "literal\nlines\n"
    );
    assert_eq!(text("k:\n  >-\n    folded\n    lines\n"), "folded lines");
    assert_eq!(
        value("k:\n  \"open"),
        Value::Malformed("a quoted value is not closed")
    );
    assert_eq!(
        value("k:\n  [a, b]"),
        Value::Unsupported("a flow collection")
    );
    // But a `key: value` below is a mapping, as in YAML.
    assert_eq!(
        value("k:\n  Use when: asked"),
        Value::Map(vec![("Use when".to_owned(), "asked".to_owned())])
    );
}

#[test]
fn a_sequence_is_named_and_skipped() {
    assert_eq!(
        value("allowed-tools:\n  - Read\n  - Grep\n"),
        Value::Unsupported("a sequence")
    );
    assert_eq!(
        value("allowed-tools:\n  -\n"),
        Value::Unsupported("a sequence")
    );
    let entries = parse("tools:\n  - a\n  - b: c\nname: x\n").unwrap();
    assert_eq!(entries.len(), 2, "the sequence's lines are not entries");
    assert_eq!(entries[1].key, "name");
}

#[test]
fn flow_collections_anchors_aliases_and_tags_are_named_and_not_read() {
    assert_eq!(value("k: [a, b]"), Value::Unsupported("a flow collection"));
    assert_eq!(value("k: {a: b}"), Value::Unsupported("a flow collection"));
    for indicator in ["&anchor x", "*alias", "!!str x", "!custom x"] {
        assert_eq!(
            value(&format!("k: {indicator}")),
            Value::Unsupported("an anchor, an alias or a tag"),
            "{indicator}"
        );
    }
}

#[test]
fn an_alias_bomb_is_not_expanded() {
    let mut block = String::from(
        "a0: &a0 [\"lol\", \"lol\", \"lol\", \"lol\", \"lol\", \"lol\", \"lol\", \"lol\", \"lol\"]\n",
    );
    for level in 1..40 {
        let previous = level - 1;
        let _ = writeln!(
            block,
            "a{level}: &a{level} [*a{previous}, *a{previous}, *a{previous}, *a{previous}, *a{previous}, *a{previous}, *a{previous}, *a{previous}, *a{previous}]"
        );
    }
    let entries = parse(&block).unwrap();
    assert_eq!(entries.len(), 40);
    assert!(
        entries
            .iter()
            .all(|e| e.value == Value::Unsupported("an anchor, an alias or a tag")),
        "every level starts with an anchor, so none is read and none is expanded"
    );
}

#[test]
fn deep_indentation_is_not_recursion() {
    let mut block = String::from("a:\n");
    for depth in 1..20_000 {
        block.push_str(&" ".repeat(depth.min(200)));
        block.push_str("k:\n");
    }
    let entries = parse(&block).unwrap();
    assert_eq!(entries.len(), 1);
    assert!(matches!(entries[0].value, Value::Unsupported(_)));
}

#[test]
fn an_unreadable_value_does_not_cost_the_next_entry() {
    let entries = parse("a: \"open\nb: ok\n").unwrap();
    // The line after a quote that never closes is not indented, so it is
    // its own entry and the quote ran to the end of the first line.
    assert_eq!(entries.len(), 2);
    assert!(matches!(entries[0].value, Value::Malformed(_)));
    assert_eq!(entries[1].value, Value::Text("ok".to_owned()));
}
