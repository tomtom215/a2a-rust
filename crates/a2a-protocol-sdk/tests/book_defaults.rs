// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The book's defaults tables, checked against the defaults the code has.
//!
//! `book/src/reference/configuration.md` tells an operator what every knob is
//! set to if they leave it alone, and nothing compared it with the code: it
//! gave `with_executor_timeout` a default of None while the builder sets one
//! hour (audit S12; escape class 1). This test reads each table named below,
//! takes the struct's real `Default` through its `Debug` output, and compares
//! every row whose default the page states — after normalising the units the
//! page writes (`1 hour`, `4 MiB`, `10,000`, `250ms`) and the ones `Debug`
//! prints (`3600s`, `4194304`) to the same number.
//!
//! A row that names a field the struct does not have fails too, as does a
//! struct field with no row: a table that silently drops the knob someone was
//! looking for is the same defect as one that misstates it.

#![cfg(all(feature = "grpc", feature = "grpc-tls"))]

use std::collections::BTreeMap;

use a2a_protocol_sdk::client::transport::grpc::GrpcTransportConfig;
use a2a_protocol_sdk::client::{ClientConfig, RetryPolicy};
use a2a_protocol_sdk::server::store::TaskStoreConfig;
use a2a_protocol_sdk::server::{
    DispatchConfig, GrpcConfig, HandlerLimits, PushRetryPolicy, RateLimitConfig,
};

fn page() -> String {
    std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../book/src/reference/configuration.md"
    ))
    .expect("configuration page")
}

/// The rows of the first table under `### {heading}`: first cell -> Default cell.
fn table(page: &str, heading: &str) -> BTreeMap<String, String> {
    let start = page
        .find(&format!("\n### {heading}\n"))
        .unwrap_or_else(|| panic!("no `### {heading}` on the page"));
    let mut lines = page[start..].lines().skip_while(|l| !l.starts_with('|'));
    let header: Vec<String> = cells(lines.next().expect("header"));
    let col = header
        .iter()
        .position(|h| h == "Default")
        .unwrap_or_else(|| panic!("`{heading}` has no Default column"));
    lines
        .skip(1)
        .take_while(|l| l.starts_with('|'))
        .map(|l| {
            let c = cells(l);
            (c[0].trim_matches('`').to_owned(), c[col].clone())
        })
        .collect()
}

fn cells(line: &str) -> Vec<String> {
    line.trim()
        .trim_matches('|')
        .split('|')
        .map(|c| c.trim().to_owned())
        .collect()
}

/// The top-level `field: value` pairs of a `Debug` rendering.
fn debug_fields(debug: &str) -> BTreeMap<String, String> {
    let body = &debug[debug.find('{').expect("a struct") + 1..debug.rfind('}').expect("a struct")];
    let mut out = BTreeMap::new();
    let (mut depth, mut start) = (0_i32, 0);
    let mut parts = Vec::new();
    for (i, ch) in body.char_indices() {
        match ch {
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth -= 1,
            ',' if depth == 0 => {
                parts.push(&body[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    parts.push(&body[start..]);
    for part in parts {
        if let Some((k, v)) = part.split_once(':') {
            out.insert(k.trim().to_owned(), v.trim().to_owned());
        }
    }
    out
}

/// A default as a comparable string: numbers (durations in seconds, sizes in
/// bytes) in one canonical form, `Some(x)` as `x`, text lowercased.
fn canon(raw: &str) -> String {
    let s = raw.replace(['`', '*'], "");
    let s = s.split(" (").next().unwrap_or("").trim().to_owned();
    let s = s
        .strip_prefix("Some(")
        .and_then(|t| t.strip_suffix(')'))
        .map_or(s.clone(), str::to_owned);
    if let Some(list) = s.strip_prefix('[').and_then(|t| t.strip_suffix(']')) {
        return format!(
            "[{}]",
            list.split(',').map(canon).collect::<Vec<_>>().join(",")
        );
    }
    let lower = s.to_ascii_lowercase().replace([',', '_'], "");
    let units: [(&str, f64); 10] = [
        (" hours", 3600.0),
        (" hour", 3600.0),
        (" min", 60.0),
        ("ms", 0.001),
        ("µs", 0.000_001),
        ("ns", 0.000_000_001),
        ("s", 1.0),
        (" mib", 1_048_576.0),
        (" kib", 1024.0),
        ("", 1.0),
    ];
    for (suffix, scale) in units {
        if let Some(num) = lower.strip_suffix(suffix)
            && let Ok(v) = num.trim().parse::<f64>()
        {
            return format!("{}", v * scale);
        }
    }
    lower
}

/// Compares every row of a field table — or, given `setters`, only the rows
/// they name, each mapped to the field that holds its default.
fn compare(
    heading: &str,
    page: &str,
    actual: &BTreeMap<String, String>,
    setters: Option<&[(&str, &str)]>,
    wrong: &mut Vec<String>,
) {
    let rows = table(page, heading);
    for (row, documented) in &rows {
        let field = match setters {
            None => row.as_str(),
            Some(map) => match map.iter().find(|(r, _)| r == row) {
                Some((_, f)) => f,
                None => continue,
            },
        };
        if documented == "—" {
            continue;
        }
        match actual.get(field) {
            None => wrong.push(format!("{heading}: `{row}` names no field of the default")),
            Some(real) if canon(real) != canon(documented) => wrong.push(format!(
                "{heading}: `{row}` is documented as {documented:?}; the default is {real}"
            )),
            Some(_) => {}
        }
    }
}

#[test]
fn every_documented_default_is_the_default() {
    let page = page();
    let mut wrong = Vec::new();

    for (heading, debug) in [
        ("HandlerLimits", format!("{:?}", HandlerLimits::default())),
        (
            "TaskStoreConfig",
            format!("{:?}", TaskStoreConfig::default()),
        ),
        ("DispatchConfig", format!("{:?}", DispatchConfig::default())),
        ("GrpcConfig", format!("{:?}", GrpcConfig::default())),
        (
            "PushRetryPolicy",
            format!("{:?}", PushRetryPolicy::default()),
        ),
        (
            "RateLimitConfig",
            format!("{:?}", RateLimitConfig::default()),
        ),
        (
            "GrpcTransportConfig",
            format!("{:?}", GrpcTransportConfig::default()),
        ),
        ("RetryPolicy", format!("{:?}", RetryPolicy::default())),
    ] {
        let fields = debug_fields(&debug);
        compare(heading, &page, &fields, None, &mut wrong);
        let documented = table(&page, heading);
        for field in fields.keys() {
            if !documented.contains_key(field) {
                wrong.push(format!("{heading}: field `{field}` has no row"));
            }
        }
    }

    // The builder tables name setters; each row that maps onto a default the
    // code holds in one place is checked against it.
    let client = debug_fields(&format!("{:?}", ClientConfig::default()));
    compare(
        "ClientBuilder",
        &page,
        &client,
        Some(&[
            ("with_timeout", "request_timeout"),
            ("with_connection_timeout", "connection_timeout"),
            ("with_stream_connect_timeout", "stream_connect_timeout"),
            (
                "with_stream_first_event_timeout",
                "stream_first_event_timeout",
            ),
            ("with_max_event_size", "max_event_size"),
            ("with_stream_idle_timeout", "stream_idle_timeout"),
            ("with_accepted_output_modes", "accepted_output_modes"),
            ("with_history_length", "history_length"),
            ("with_return_immediately", "return_immediately"),
            ("with_tenant", "tenant"),
        ]),
        &mut wrong,
    );
    let builder = debug_fields(&format!(
        "{:?}",
        a2a_protocol_sdk::server::RequestHandlerBuilder::new(Noop)
    ));
    let executor_timeout = table(&page, "RequestHandlerBuilder")["with_executor_timeout"].clone();
    if canon(&builder["executor_timeout"]) != canon(&executor_timeout) {
        wrong.push(format!(
            "RequestHandlerBuilder: `with_executor_timeout` is documented as {executor_timeout:?}; \
             the default is {}",
            builder["executor_timeout"]
        ));
    }

    assert!(
        wrong.is_empty(),
        "{} documented default(s) wrong:\n  {}",
        wrong.len(),
        wrong.join("\n  ")
    );
}

struct Noop;
a2a_protocol_sdk::server::agent_executor!(Noop, |_ctx, _queue| async { Ok(()) });

#[test]
fn canon_reads_the_units_the_page_and_debug_write() {
    assert_eq!(canon("1 hour"), canon("3600s"));
    assert_eq!(canon("4 MiB"), canon("4194304"));
    assert_eq!(canon("10,000"), canon("Some(10000)"));
    assert_eq!(canon("250ms"), canon("250ms"));
    assert_eq!(canon("5 min"), canon("300s"));
    assert_eq!(canon("`[1s, 2s]`"), canon("[1s, 2s]"));
    assert_eq!(canon("None (bundled Mozilla roots)"), canon("None"));
    assert_ne!(canon("None"), canon("Some(3600s)"));
}
