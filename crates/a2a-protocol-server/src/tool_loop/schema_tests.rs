// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

use serde_json::{Value, json};

use super::{ArgumentCheck, refusal};

fn checked(schema: &Value, args: &Value) -> Option<String> {
    refusal(ArgumentCheck::Checked, schema, args)
}

/// `args` satisfy `schema`. Takes them by value so a call reads as the schema
/// and the arguments it is tried with, without a `&` on each.
#[allow(clippy::needless_pass_by_value)]
fn ok(schema: Value, args: Value) {
    assert_eq!(checked(&schema, &args), None, "{schema} / {args}");
}

/// `args` break `schema`, and the refusal says `says`.
#[allow(clippy::needless_pass_by_value)]
fn bad(schema: Value, args: Value, says: &str) {
    let why = checked(&schema, &args).unwrap_or_else(|| panic!("accepted: {schema} / {args}"));
    assert!(why.contains(says), "{why:?} does not say {says:?}");
}

#[test]
fn off_checks_nothing() {
    let schema =
        json!({"type": "object", "required": ["a"], "properties": {"a": {"type": "string"}}});
    assert_eq!(refusal(ArgumentCheck::Off, &schema, &json!({"a": 1})), None);
    assert_eq!(ArgumentCheck::default(), ArgumentCheck::Off);
}

#[test]
fn a_type_is_one_name_or_several() {
    ok(json!({"type": "string"}), json!("x"));
    bad(
        json!({"type": "string"}),
        json!(1),
        "expected string, got number",
    );
    ok(json!({"type": ["string", "null"]}), json!(null));
    bad(
        json!({"type": ["string", "null"]}),
        json!(1),
        "expected string or null, got number",
    );
    bad(json!({"type": "nonsense"}), json!("x"), "expected nonsense");
    for (name, good, wrong) in [
        ("null", json!(null), json!(0)),
        ("boolean", json!(true), json!("true")),
        ("object", json!({}), json!([])),
        ("array", json!([]), json!({})),
        ("number", json!(1.5), json!("1.5")),
    ] {
        ok(json!({"type": name}), good);
        assert!(checked(&json!({"type": name}), &wrong).is_some(), "{name}");
    }
}

#[test]
fn an_integer_is_a_number_with_no_fraction() {
    let schema = json!({"type": "integer"});
    ok(schema.clone(), json!(3));
    ok(schema.clone(), json!(-3));
    ok(schema.clone(), json!(3.0));
    ok(schema.clone(), json!(u64::MAX));
    bad(schema.clone(), json!(3.5), "expected integer, got number");
    bad(schema, json!("3"), "expected integer, got string");
}

#[test]
fn required_and_properties_are_checked_at_their_path() {
    let schema = json!({
        "type": "object",
        "required": ["service"],
        "properties": {"service": {"type": "string"}, "n": {"type": "integer"}}
    });
    ok(schema.clone(), json!({"service": "checkout"}));
    ok(
        schema.clone(),
        json!({"service": "checkout", "n": 2, "extra": true}),
    );
    bad(
        schema.clone(),
        json!({}),
        "missing required property 'service'",
    );
    bad(
        schema.clone(),
        json!({"service": 7}),
        "'/service': expected string, got number",
    );
    bad(
        schema.clone(),
        json!({"service": "x", "n": 1.5}),
        "'/n': expected integer",
    );
    bad(
        schema,
        json!([]),
        "the arguments: expected object, got array",
    );
}

#[test]
fn additional_properties_is_false_or_a_schema() {
    let closed = json!({"properties": {"a": {}}, "additionalProperties": false});
    ok(closed.clone(), json!({"a": 1}));
    bad(
        closed,
        json!({"a": 1, "b": 2}),
        "has the property 'b', which is not allowed",
    );
    let typed = json!({"properties": {"a": {}}, "additionalProperties": {"type": "string"}});
    ok(typed.clone(), json!({"a": 1, "b": "x"}));
    bad(typed, json!({"a": 1, "b": 2}), "'/b': expected string");
    ok(json!({"additionalProperties": true}), json!({"z": 1}));
}

#[test]
fn enum_and_const_compare_values_not_spellings() {
    ok(json!({"enum": ["a", 1, null]}), json!(1.0));
    ok(json!({"enum": [{"k": [1, 2]}]}), json!({"k": [1.0, 2]}));
    bad(
        json!({"enum": ["a", "b"]}),
        json!("c"),
        "is not one of the allowed values",
    );
    ok(json!({"const": "x"}), json!("x"));
    bad(
        json!({"const": "x"}),
        json!("y"),
        "is not the required value",
    );
    bad(
        json!({"const": [1, 2]}),
        json!([1]),
        "is not the required value",
    );
}

#[test]
fn arrays_have_item_schemas_and_sizes() {
    let schema = json!({"type": "array", "items": {"type": "integer"}, "minItems": 1, "maxItems": 3, "uniqueItems": true});
    ok(schema.clone(), json!([1, 2, 3]));
    bad(schema.clone(), json!([]), "has 0 items, needs at least 1");
    bad(
        schema.clone(),
        json!([1, 2, 3, 4]),
        "has 4 items, needs at most 3",
    );
    bad(schema.clone(), json!([1, "x"]), "'/1': expected integer");
    bad(schema, json!([1, 1.0]), "items must be unique");
    ok(json!({"uniqueItems": false}), json!([1, 1]));
}

#[test]
fn strings_are_measured_in_characters() {
    let schema = json!({"minLength": 2, "maxLength": 3});
    ok(schema.clone(), json!("éé"));
    ok(schema.clone(), json!("😀😀😀"));
    bad(
        schema.clone(),
        json!("é"),
        "has 1 characters, needs at least 2",
    );
    bad(
        schema,
        json!("😀😀😀😀"),
        "has 4 characters, needs at most 3",
    );
    ok(json!({"minLength": 5}), json!(7));
}

#[test]
fn numbers_have_bounds_and_exclusive_bounds() {
    let schema = json!({"minimum": 1, "maximum": 10});
    ok(schema.clone(), json!(1));
    ok(schema.clone(), json!(10.0));
    bad(schema.clone(), json!(0), "needs at least 1");
    bad(schema, json!(11), "needs at most 10");
    let open = json!({"exclusiveMinimum": 0, "exclusiveMaximum": 1});
    ok(open.clone(), json!(0.5));
    bad(open.clone(), json!(0), "needs more than 0");
    bad(open, json!(1), "needs less than 1");
}

#[test]
fn objects_have_property_counts() {
    let schema = json!({"minProperties": 1, "maxProperties": 2});
    ok(schema.clone(), json!({"a": 1}));
    bad(
        schema.clone(),
        json!({}),
        "has 0 properties, needs at least 1",
    );
    bad(
        schema,
        json!({"a": 1, "b": 2, "c": 3}),
        "has 3 properties, needs at most 2",
    );
}

#[test]
fn schemas_combine() {
    let any = json!({"anyOf": [{"type": "string"}, {"type": "integer"}]});
    ok(any.clone(), json!("x"));
    ok(any.clone(), json!(1));
    bad(
        any,
        json!(true),
        "matches 0 of the 2 shapes allowed, at least one is needed",
    );
    let one = json!({"oneOf": [{"type": "number"}, {"type": "integer"}]});
    ok(one.clone(), json!(1.5));
    bad(
        one.clone(),
        json!(1),
        "matches 2 of the 2 shapes allowed, exactly one is needed",
    );
    bad(
        one,
        json!("x"),
        "matches 0 of the 2 shapes allowed, exactly one is needed",
    );
    let all = json!({"allOf": [{"type": "object"}, {"required": ["a"]}]});
    ok(all.clone(), json!({"a": 1}));
    bad(all, json!({}), "missing required property 'a'");
    ok(json!({"not": {"type": "string"}}), json!(1));
    bad(
        json!({"not": {"type": "string"}}),
        json!("x"),
        "matches a shape that is not allowed",
    );
}

#[test]
fn a_boolean_is_a_schema() {
    ok(json!(true), json!({"any": "thing"}));
    ok(json!({}), json!(1));
    bad(json!(false), json!(1), "no value is allowed here");
    bad(
        json!({"properties": {"a": false}}),
        json!({"a": 1}),
        "'/a': no value is allowed here",
    );
}

#[test]
fn a_refusal_lists_five_and_counts_the_rest() {
    let props: serde_json::Map<String, Value> = (0..8)
        .map(|i| (format!("p{i}"), json!({"type": "string"})))
        .collect();
    let args: serde_json::Map<String, Value> =
        (0..8).map(|i| (format!("p{i}"), json!(i))).collect();
    let why = checked(&json!({"properties": props}), &Value::Object(args)).unwrap();
    assert!(
        why.starts_with("the arguments do not match this tool's schema: "),
        "{why}"
    );
    assert_eq!(
        why.matches("expected string, got number").count(),
        5,
        "{why}"
    );
    assert!(why.ends_with("(and 3 more)"), "{why}");
}

#[test]
fn strict_names_a_keyword_it_cannot_check_and_checked_ignores_it() {
    for keyword in [
        "$ref",
        "pattern",
        "patternProperties",
        "multipleOf",
        "if",
        "then",
        "else",
        "contains",
        "prefixItems",
        "propertyNames",
        "dependentRequired",
        "unevaluatedProperties",
        "somethingNew",
    ] {
        let mut schema = json!({"type": "object"});
        schema[keyword] = json!("x");
        let why = refusal(ArgumentCheck::Strict, &schema, &json!({})).expect(keyword);
        assert!(why.contains(keyword), "{why}");
        assert!(why.contains("the call was not run"), "{why}");
        assert_eq!(
            refusal(ArgumentCheck::Checked, &schema, &json!({})),
            None,
            "{keyword}"
        );
    }
}

/// The scan does not depend on the call: a constraint on a property the call
/// leaves out is still a constraint nobody checked.
#[test]
fn strict_finds_an_unsupported_keyword_wherever_a_schema_applies() {
    for schema in [
        json!({"properties": {"a": {"pattern": "^x"}}}),
        json!({"items": {"pattern": "^x"}}),
        json!({"additionalProperties": {"pattern": "^x"}}),
        json!({"anyOf": [{"type": "string"}, {"pattern": "^x"}]}),
        json!({"oneOf": [{"pattern": "^x"}]}),
        json!({"allOf": [{"properties": {"a": {"pattern": "^x"}}}]}),
        json!({"not": {"pattern": "^x"}}),
    ] {
        let why = refusal(ArgumentCheck::Strict, &schema, &json!({})).expect("flagged");
        assert!(why.contains("pattern"), "{schema}: {why}");
    }
    let why = refusal(
        ArgumentCheck::Strict,
        &json!({"items": [{"type": "string"}]}),
        &json!([]),
    )
    .unwrap();
    assert!(why.contains("items as a list"), "{why}");
    let why = refusal(
        ArgumentCheck::Strict,
        &json!({"exclusiveMinimum": true}),
        &json!(1),
    )
    .unwrap();
    assert!(why.contains("exclusiveMinimum as a boolean"), "{why}");
    let why = refusal(ArgumentCheck::Strict, &json!({"properties": 7}), &json!({})).unwrap();
    assert!(why.contains("properties"), "{why}");
    let why = refusal(ArgumentCheck::Strict, &json!({"anyOf": "x"}), &json!({})).unwrap();
    assert!(why.contains("anyOf"), "{why}");
    let why = refusal(ArgumentCheck::Strict, &json!(7), &json!({})).unwrap();
    assert!(why.contains("neither an object nor a boolean"), "{why}");
}

#[test]
fn strict_names_every_keyword_it_cannot_check_once() {
    let schema = json!({"pattern": "x", "multipleOf": 2, "properties": {"a": {"pattern": "y"}}});
    let why = refusal(ArgumentCheck::Strict, &schema, &json!({})).unwrap();
    assert_eq!(why.matches("pattern").count(), 1, "{why}");
    assert!(why.contains("multipleOf, pattern"), "sorted: {why}");
}

/// Annotations constrain nothing and are never refused, and a schema using
/// only implemented keywords passes `Strict`.
#[test]
fn strict_accepts_annotations_and_the_implemented_keywords() {
    let mut schema = json!({
        "type": "object", "required": ["a"],
        "properties": {"a": {"type": "string", "minLength": 1, "enum": ["x"]}},
        "additionalProperties": false
    });
    // The names the documentation lists, not the constant the code reads.
    for annotation in [
        "$schema",
        "$id",
        "$comment",
        "title",
        "description",
        "default",
        "examples",
        "deprecated",
        "readOnly",
        "writeOnly",
        "format",
        "$defs",
        "definitions",
    ] {
        schema[annotation] = json!("x");
    }
    assert_eq!(
        refusal(ArgumentCheck::Strict, &schema, &json!({"a": "x"})),
        None
    );
    let why = refusal(ArgumentCheck::Strict, &schema, &json!({"a": "y"})).unwrap();
    assert!(why.contains("is not one of the allowed values"), "{why}");
}
