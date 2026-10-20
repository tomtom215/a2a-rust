// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Compares a URI's contract document with its checked-in snapshot.
//!
//! A twin of the types crate's `tests/uri_contracts/golden.rs`, which pins
//! the extension URIs. They are copies rather than a shared module because a
//! test reaching into another crate's directory breaks the moment either
//! crate is packaged on its own.

use std::path::PathBuf;

use serde_json::Value;

/// Setting this to `1` writes the snapshots instead of comparing them.
pub const BLESS_ENV: &str = "A2A_BLESS_URI_CONTRACTS";

/// `value` with every object's keys sorted, at every depth.
///
/// Explicit, because some builds of this workspace turn on
/// `serde_json/preserve_order`, and then a `Map` keeps insertion order.
pub fn canonical(value: Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut entries: Vec<(String, Value)> = map.into_iter().collect();
            entries.sort_by(|a, b| a.0.cmp(&b.0));
            Value::Object(
                entries
                    .into_iter()
                    .map(|(k, v)| (k, canonical(v)))
                    .collect(),
            )
        }
        Value::Array(items) => Value::Array(items.into_iter().map(canonical).collect()),
        other => other,
    }
}

/// Checks `{"uri": uri, "contract": contract}` against
/// `tests/golden/uri_contracts/<name>.json`, or writes it when blessing.
pub fn check(name: &str, uri: &str, contract: Value) {
    let doc = canonical(serde_json::json!({ "uri": uri, "contract": contract }));
    let rendered = serde_json::to_string_pretty(&doc).expect("a JSON value renders") + "\n";
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden/uri_contracts")
        .join(format!("{name}.json"));
    let shown = format!(
        "{}/tests/golden/uri_contracts/{name}.json",
        env!("CARGO_PKG_NAME")
    );
    let bless = format!(
        "{BLESS_ENV}=1 cargo test -p {} --features websocket --test {}",
        env!("CARGO_PKG_NAME"),
        module_path!().split("::").next().unwrap_or_default()
    );

    if std::env::var(BLESS_ENV).as_deref() == Ok("1") {
        std::fs::write(&path, &rendered).expect("the snapshot is writable");
        return;
    }
    let on_disk = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!("no contract snapshot for {uri} at {shown} ({e}); review the contract and run\n    {bless}")
    });
    let golden: Value = serde_json::from_str(&on_disk)
        .unwrap_or_else(|e| panic!("{shown} is not JSON ({e}); re-bless it with\n    {bless}"));

    let pinned = golden["uri"].as_str().unwrap_or("<missing>");
    assert!(
        pinned == uri,
        "{shown} pins the contract of {pinned}, but the constant is now {uri}.\n\
         A new URI is a new contract: review it as a whole (spec §4.6.3 / §5.8), keep\n\
         serving {pinned} while peers move, and record the new snapshot with\n    {bless}"
    );
    if golden == doc {
        return;
    }
    panic!(
        "the wire contract of {uri} changed, but the URI did not.\n\
         A2A v1.0.1 §4.6.3: \"A new URI MUST be created for breaking changes to an extension.\"\n\
         §5.8: \"When a breaking change is introduced to a binding, a new URI MUST be used.\"\n\
         Decide which this is (RELEASING.md):\n\
         - breaking (an older peer cannot follow it: a key, field or token renamed or removed,\n  \
           a field made required, a meaning changed): bump the URI's version (/v1 -> /v2),\n  \
           keep serving the old URI, then bless the snapshot for the new one;\n\
         - not breaking (an older peer ignores it, e.g. a new optional field): re-bless and\n  \
           say why in the commit message.\n\
         Bless with\n    {bless}\n\n\
         --- {shown}\n+++ current\n{}",
        diff(&on_disk, &rendered)
    );
}

/// A line diff of `old` against `new` (`-` removed, `+` added, two lines of
/// context), small enough to read in a test failure.
fn diff(old: &str, new: &str) -> String {
    let a: Vec<&str> = old.lines().collect();
    let b: Vec<&str> = new.lines().collect();
    // Longest common subsequence table, from the end.
    let mut lcs = vec![vec![0usize; b.len() + 1]; a.len() + 1];
    for i in (0..a.len()).rev() {
        for j in (0..b.len()).rev() {
            lcs[i][j] = if a[i] == b[j] {
                lcs[i + 1][j + 1] + 1
            } else {
                lcs[i + 1][j].max(lcs[i][j + 1])
            };
        }
    }
    let mut ops: Vec<(char, &str)> = Vec::new();
    let (mut i, mut j) = (0, 0);
    while i < a.len() || j < b.len() {
        if i < a.len() && j < b.len() && a[i] == b[j] {
            ops.push((' ', a[i]));
            i += 1;
            j += 1;
        } else if i < a.len() && (j == b.len() || lcs[i + 1][j] >= lcs[i][j + 1]) {
            ops.push(('-', a[i]));
            i += 1;
        } else {
            ops.push(('+', b[j]));
            j += 1;
        }
    }
    let near = |k: usize| {
        ops[k.saturating_sub(2)..(k + 3).min(ops.len())]
            .iter()
            .any(|(op, _)| *op != ' ')
    };
    let mut out = String::new();
    let mut skipped = false;
    for (k, (op, line)) in ops.iter().enumerate() {
        if near(k) {
            if skipped {
                out.push_str("  ...\n");
                skipped = false;
            }
            out.push_str(&format!("{op} {line}\n"));
        } else {
            skipped = true;
        }
    }
    out
}
