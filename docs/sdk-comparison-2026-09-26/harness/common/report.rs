// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

// Shared result reporting. One JSON line per check.
use std::time::Instant;

pub struct Report {
    pub driver: &'static str,
    pub binding: String,
    pub failures: usize,
}

impl Report {
    pub fn rec(&mut self, check: &str, ok: bool, detail: impl Into<String>, started: Instant) {
        if !ok { self.failures += 1; }
        let line = serde_json::json!({
            "driver": self.driver, "binding": self.binding, "check": check,
            "ok": ok, "ms": started.elapsed().as_secs_f64() * 1000.0, "detail": detail.into(),
        });
        println!("{line}");
    }
}

/// Parses CLI: <card_base_url> <binding: JSONRPC|HTTP+JSON|GRPC> [--llm]
pub fn args() -> (String, String, bool) {
    let a: Vec<String> = std::env::args().collect();
    (a[1].clone(), a[2].clone(), a.iter().any(|x| x == "--llm"))
}

pub fn text_ok(text: &str, llm: bool, input: &str) -> bool {
    if llm { !text.trim().is_empty() } else { text == format!("Echo: {input}") }
}
