// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! A JSON value with every object's keys in sorted order, whatever order
//! `serde_json::Map` keeps.
//!
//! `serde_json::Map` is a `BTreeMap` unless the `preserve_order` feature is
//! on, and any crate in a build can turn it on (genai 0.7 does); with it, a
//! `Value` keeps the order a `HashMap` field was written in, which each map
//! instance draws at random. The card's `ETag` hashed a `Value` on the
//! assumption that its keys were sorted, so with the feature on the same card
//! had a different `ETag` from one serialization to the next (8 distinct of
//! 8 in `the_extended_card_s_etag_is_the_same_for_the_same_card`, 2026-10-20).

use serde_json::{Map, Value};

/// `value` with each object's entries inserted in key order, recursively.
pub(super) fn sorted(value: Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut entries: Vec<(String, Value)> = map.into_iter().collect();
            entries.sort_by(|a, b| a.0.cmp(&b.0));
            Value::Object(
                entries
                    .into_iter()
                    .map(|(k, v)| (k, sorted(v)))
                    .collect::<Map<String, Value>>(),
            )
        }
        Value::Array(items) => Value::Array(items.into_iter().map(sorted).collect()),
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::sorted;
    use serde_json::json;

    /// Holds whether or not `preserve_order` is on: built in reverse order,
    /// nested in an object and in an array, the keys come out sorted.
    #[test]
    fn keys_come_out_sorted_at_every_depth() {
        let mut inner = serde_json::Map::new();
        inner.insert("z".into(), 1.into());
        inner.insert("a".into(), 2.into());
        let mut outer = serde_json::Map::new();
        outer.insert("y".into(), serde_json::Value::Object(inner.clone()));
        outer.insert("b".into(), json!([serde_json::Value::Object(inner)]));
        let text = serde_json::to_string(&sorted(serde_json::Value::Object(outer))).unwrap();
        assert_eq!(text, r#"{"b":[{"a":2,"z":1}],"y":{"a":2,"z":1}}"#);
    }
}
