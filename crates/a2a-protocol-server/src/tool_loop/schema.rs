// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Checking a call's arguments against the schema the tool advertised before
//! the call is put to the policy and run (adversarial review T2).
//!
//! The loop used to check that arguments were a JSON object and that the tool
//! was in the set, and no more: a model that sent `{"amount": "everything"}` to
//! a tool whose schema says `amount` is a number reached the tool, which then
//! had to defend itself. Here the schema is enforced for the keywords below.
//! It is a checker for the subset of JSON Schema that tool parameters use, not
//! a general validator, and says so: a keyword it does not implement is named,
//! and [`ArgumentCheck::Strict`] refuses a call to a tool whose schema uses
//! one, rather than let a constraint the tool's author wrote go unchecked.
//!
//! **Checked:** `type` (one name or several), `enum`, `const`, `properties`,
//! `required`, `additionalProperties` (`false` or a schema), `items` (one
//! schema), `minItems`, `maxItems`, `uniqueItems`, `minLength`, `maxLength`
//! (in characters), `minimum`, `maximum`, `exclusiveMinimum`,
//! `exclusiveMaximum`, `minProperties`, `maxProperties`, `anyOf`, `oneOf`,
//! `allOf`, `not`, and the booleans `true` and `false` as schemas.
//!
//! **Annotations, ignored by definition:** `$schema`, `$id`, `$comment`,
//! `title`, `description`, `default`, `examples`, `deprecated`, `readOnly`,
//! `writeOnly`, `format` (an annotation unless a validator is asked to assert
//! it, as in JSON Schema 2019-09 and later), `$defs` and `definitions`.
//!
//! **Not implemented, and named as such:** everything else, among them `$ref`,
//! `pattern`, `patternProperties`, `multipleOf`, `if`/`then`/`else`,
//! `contains`, `prefixItems`, `propertyNames`, `dependentRequired`,
//! `unevaluatedProperties` and a tuple-form `items`.
//!
//! Numbers are compared as `f64`, so an integer past 2^53 is compared by its
//! nearest double.

use std::collections::BTreeSet;

use serde_json::Value;

/// How many violations a refusal lists before it says how many more there are.
const MAX_REPORTED: usize = 5;

/// Whether, and how strictly, a call's arguments are checked against the
/// tool's schema ([`Tool::parameters`](super::Tool::parameters)); see
/// [`ToolLoop::with_argument_check`](super::ToolLoop::with_argument_check).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum ArgumentCheck {
    /// The arguments must be a JSON object and nothing more, as in every
    /// earlier release. The default.
    #[default]
    Off,
    /// The arguments must satisfy the keywords this module implements. A
    /// keyword it does not implement is not checked.
    Checked,
    /// As [`Checked`](Self::Checked), and a call to a tool whose schema uses a
    /// keyword that is not implemented is refused without being run, because
    /// the constraint it states would otherwise go unchecked.
    Strict,
}

/// Keywords that describe a schema and never constrain a value.
const ANNOTATIONS: [&str; 13] = [
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
];

/// Why `arguments` may not be run against `schema`, if they may not.
pub(super) fn refusal(mode: ArgumentCheck, schema: &Value, arguments: &Value) -> Option<String> {
    if mode == ArgumentCheck::Off {
        return None;
    }
    if mode == ArgumentCheck::Strict {
        let mut unsupported = BTreeSet::new();
        scan(schema, &mut unsupported);
        if !unsupported.is_empty() {
            let names: Vec<_> = unsupported.into_iter().collect();
            return Some(format!(
                "this tool's schema uses {}, which the loop cannot check, so the call was not run",
                names.join(", ")
            ));
        }
    }
    let mut found = Vec::new();
    check(schema, arguments, "", &mut found);
    let extra = found.len().saturating_sub(MAX_REPORTED);
    found.truncate(MAX_REPORTED);
    if found.is_empty() {
        return None;
    }
    let tail = if extra > 0 {
        format!(" (and {extra} more)")
    } else {
        String::new()
    };
    Some(format!(
        "the arguments do not match this tool's schema: {}{tail}",
        found.join("; ")
    ))
}

/// The keywords in `schema`, and in every schema it applies to a part of the
/// value, that are neither implemented nor annotations. Independent of any
/// value: a constraint on a property the call leaves out is still unchecked.
fn scan(schema: &Value, out: &mut BTreeSet<String>) {
    let object = match schema {
        Value::Bool(_) => return,
        Value::Object(o) => o,
        _ => {
            out.insert("a schema that is neither an object nor a boolean".to_owned());
            return;
        }
    };
    for (key, rule) in object {
        match key.as_str() {
            "properties" => match rule.as_object() {
                Some(props) => props.values().for_each(|s| scan(s, out)),
                None => drop(out.insert(key.clone())),
            },
            "items" if rule.is_array() => {
                out.insert("items as a list (tuple form)".to_owned());
            }
            "additionalProperties" | "items" | "not" => scan(rule, out),
            "anyOf" | "oneOf" | "allOf" => match rule.as_array() {
                Some(branches) => branches.iter().for_each(|s| scan(s, out)),
                None => drop(out.insert(key.clone())),
            },
            "exclusiveMinimum" | "exclusiveMaximum" if !rule.is_number() => {
                out.insert(format!("{key} as a boolean (draft 4)"));
            }
            k if IMPLEMENTED.contains(&k) || ANNOTATIONS.contains(&k) => {}
            other => {
                out.insert(other.to_owned());
            }
        }
    }
}

/// Keywords whose rule [`check`] applies, besides the applicators [`scan`]
/// descends into.
const IMPLEMENTED: [&str; 15] = [
    "type",
    "enum",
    "const",
    "required",
    "minItems",
    "maxItems",
    "uniqueItems",
    "minLength",
    "maxLength",
    "minimum",
    "maximum",
    "exclusiveMinimum",
    "exclusiveMaximum",
    "minProperties",
    "maxProperties",
];

fn place(at: &str) -> String {
    if at.is_empty() {
        "the arguments".to_owned()
    } else {
        format!("'{at}'")
    }
}

const fn type_of(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

fn is_type(name: &str, value: &Value) -> bool {
    match name {
        "null" => value.is_null(),
        "boolean" => value.is_boolean(),
        "object" => value.is_object(),
        "array" => value.is_array(),
        "string" => value.is_string(),
        "number" => value.is_number(),
        // Every integer a JSON number can hold converts to a finite `f64` with
        // no fraction, so this one test covers `i64`, `u64` and floats alike.
        "integer" => value
            .as_f64()
            .is_some_and(|f| f.is_finite() && f.fract() == 0.0),
        _ => false,
    }
}

/// JSON equality, with numbers equal when their values are.
fn same(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => x == y || x.as_f64() == y.as_f64(),
        (Value::Array(x), Value::Array(y)) => {
            x.len() == y.len() && x.iter().zip(y).all(|(p, q)| same(p, q))
        }
        (Value::Object(x), Value::Object(y)) => {
            x.len() == y.len() && x.iter().all(|(k, v)| y.get(k).is_some_and(|w| same(v, w)))
        }
        _ => a == b,
    }
}

/// Appends to `found` each way `value` breaks `schema`, which sits at `at`.
fn check(schema: &Value, value: &Value, at: &str, found: &mut Vec<String>) {
    let object = match schema {
        Value::Bool(false) => {
            found.push(format!("{}: no value is allowed here", place(at)));
            return;
        }
        Value::Object(o) => o,
        _ => return,
    };
    for (key, rule) in object {
        keyword(key, rule, object, value, at, found);
    }
}

fn keyword(
    key: &str,
    rule: &Value,
    schema: &serde_json::Map<String, Value>,
    value: &Value,
    at: &str,
    found: &mut Vec<String>,
) {
    let here = place(at);
    match key {
        "type" => type_keyword(rule, value, &here, found),
        "enum" => {
            if rule
                .as_array()
                .is_some_and(|o| !o.iter().any(|x| same(x, value)))
            {
                found.push(format!("{here}: is not one of the allowed values"));
            }
        }
        "const" if !same(rule, value) => {
            found.push(format!("{here}: is not the required value"));
        }
        "required" | "properties" | "additionalProperties" => {
            if let Some(object) = value.as_object() {
                object_keyword(key, rule, schema, object, at, found);
            }
        }
        "items" | "uniqueItems" => {
            if let Some(items) = value.as_array() {
                array_keyword(key, rule, items, at, found);
            }
        }
        "minItems" | "maxItems" | "minProperties" | "maxProperties" | "minLength" | "maxLength" => {
            sized(key, rule, value, &here, found);
        }
        "minimum" | "maximum" | "exclusiveMinimum" | "exclusiveMaximum" => {
            bounded(key, rule, value, &here, found);
        }
        "anyOf" | "oneOf" | "allOf" | "not" => combined(key, rule, value, at, found),
        _ => {}
    }
}

/// `type`: one name, or a list of which the value must match one.
fn type_keyword(rule: &Value, value: &Value, here: &str, found: &mut Vec<String>) {
    let names: Vec<&str> = match rule {
        Value::String(s) => vec![s],
        Value::Array(a) => a.iter().filter_map(Value::as_str).collect(),
        _ => return,
    };
    if !names.iter().any(|n| is_type(n, value)) {
        found.push(format!(
            "{here}: expected {}, got {}",
            names.join(" or "),
            type_of(value)
        ));
    }
}

/// `required`, `properties` and `additionalProperties`, of an object.
fn object_keyword(
    key: &str,
    rule: &Value,
    schema: &serde_json::Map<String, Value>,
    object: &serde_json::Map<String, Value>,
    at: &str,
    found: &mut Vec<String>,
) {
    let here = place(at);
    match key {
        "required" => {
            for name in rule
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
            {
                if !object.contains_key(name) {
                    found.push(format!("{here}: missing required property '{name}'"));
                }
            }
        }
        "properties" => {
            for (name, sub) in rule.as_object().into_iter().flatten() {
                if let Some(v) = object.get(name) {
                    check(sub, v, &format!("{at}/{name}"), found);
                }
            }
        }
        _ => {
            let known = schema.get("properties").and_then(Value::as_object);
            for (name, v) in object
                .iter()
                .filter(|(n, _)| !known.is_some_and(|k| k.contains_key(*n)))
            {
                match rule {
                    Value::Bool(false) => found.push(format!(
                        "{here}: has the property '{name}', which is not allowed"
                    )),
                    Value::Bool(true) => {}
                    sub => check(sub, v, &format!("{at}/{name}"), found),
                }
            }
        }
    }
}

/// `items` (one schema, for every element) and `uniqueItems`, of an array.
fn array_keyword(key: &str, rule: &Value, items: &[Value], at: &str, found: &mut Vec<String>) {
    if key == "items" {
        // A list-form `items` is not a schema and constrains nothing here; a
        // `Strict` loop has refused the tool already (see `scan`).
        for (i, v) in items.iter().enumerate() {
            check(rule, v, &format!("{at}/{i}"), found);
        }
        return;
    }
    let repeated = items
        .iter()
        .enumerate()
        .any(|(i, a)| items[..i].iter().any(|b| same(a, b)));
    if rule.as_bool() == Some(true) && repeated {
        found.push(format!("{}: items must be unique", place(at)));
    }
}

/// `anyOf`, `oneOf`, `allOf` and `not`.
fn combined(key: &str, rule: &Value, value: &Value, at: &str, found: &mut Vec<String>) {
    let here = place(at);
    if key == "not" {
        let mut sub = Vec::new();
        check(rule, value, at, &mut sub);
        if sub.is_empty() {
            found.push(format!("{here}: matches a shape that is not allowed"));
        }
        return;
    }
    let Some(branches) = rule.as_array() else {
        return;
    };
    if key == "allOf" {
        for branch in branches {
            check(branch, value, at, found);
        }
        return;
    }
    let passing = branches
        .iter()
        .filter(|b| {
            let mut sub = Vec::new();
            check(b, value, at, &mut sub);
            sub.is_empty()
        })
        .count();
    let (broken, needs) = if key == "anyOf" {
        (passing == 0, "at least one is needed")
    } else {
        (passing != 1, "exactly one is needed")
    };
    if broken {
        found.push(format!(
            "{here}: matches {passing} of the {} shapes allowed, {needs}",
            branches.len()
        ));
    }
}

/// `minItems`, `maxItems`, `minProperties`, `maxProperties`, `minLength` and
/// `maxLength`.
fn sized(key: &str, rule: &Value, value: &Value, here: &str, found: &mut Vec<String>) {
    let Some(limit) = rule.as_u64() else { return };
    let (size, what) = match (key, value) {
        ("minItems" | "maxItems", Value::Array(a)) => (a.len(), "items"),
        ("minProperties" | "maxProperties", Value::Object(o)) => (o.len(), "properties"),
        ("minLength" | "maxLength", Value::String(s)) => (s.chars().count(), "characters"),
        _ => return,
    };
    let size = u64::try_from(size).unwrap_or(u64::MAX);
    let (broken, bound) = if key.starts_with("min") {
        (size < limit, "at least")
    } else {
        (size > limit, "at most")
    };
    if broken {
        found.push(format!("{here}: has {size} {what}, needs {bound} {limit}"));
    }
}

/// `minimum`, `maximum`, `exclusiveMinimum` and `exclusiveMaximum`.
fn bounded(key: &str, rule: &Value, value: &Value, here: &str, found: &mut Vec<String>) {
    let (Some(limit), Some(n)) = (rule.as_f64(), value.as_f64()) else {
        return;
    };
    let (broken, bound) = match key {
        "minimum" => (n < limit, "at least"),
        "maximum" => (n > limit, "at most"),
        "exclusiveMinimum" => (n <= limit, "more than"),
        _ => (n >= limit, "less than"),
    };
    if broken {
        found.push(format!("{here}: is {n}, needs {bound} {limit}"));
    }
}

#[cfg(test)]
#[path = "schema_tests.rs"]
mod tests;
