// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Part of the tests in `mod.rs`: documents made of YAML's own punctuation,
//! in an order nobody chose, which must not panic anything. The fuzz target
//! (`fuzz/fuzz_targets/skill_frontmatter.rs`) does this open-endedly and only
//! under `cargo fuzz`; this is the same property, bounded and run by every
//! `cargo test`.

use super::*;
use crate::skills::yaml;

/// Pieces of a frontmatter that decide how it is read, and a few that make
/// text hard: multi-byte characters, a NUL, and escapes cut short.
const PIECES: [&str; 44] = [
    "name",
    "description",
    ": ",
    ":",
    "\n",
    "\r\n",
    "  ",
    "    ",
    "\t",
    "- ",
    "-",
    "|",
    ">",
    "|-",
    ">+",
    "|2",
    "'",
    "''",
    "\"",
    "\\",
    "\\n",
    "\\u00e9",
    "\\x4",
    "\\U0001F600",
    "#",
    " #",
    "&a",
    "*a",
    "!!str",
    "[",
    "]",
    "{",
    "}",
    ",",
    "---",
    "...",
    "?",
    "%",
    "é",
    "名",
    "\u{0}",
    "x",
    "1",
    "metadata",
];

/// A small deterministic generator, so a failure replays.
struct Xorshift(u64);

impl Xorshift {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, n: usize) -> usize {
        usize::try_from(self.next() % u64::try_from(n).unwrap_or(1)).unwrap_or(0)
    }
}

#[test]
fn random_frontmatters_never_panic() {
    let mut random = Xorshift(0x9E37_79B9_7F4A_7C15);
    let mut loaded = 0;
    let mut refused = 0;
    for _ in 0..20_000 {
        let mut block = String::new();
        for _ in 0..=random.below(60) {
            block.push_str(PIECES[random.below(PIECES.len())]);
        }
        // The YAML reader on its own, and the whole file around it.
        let _ = yaml::parse(&block);
        let text = format!("---\nname: demo\ndescription: d\n{block}\n---\nbody");
        let mut findings = Vec::new();
        if parse(&text, "demo", MAX_BODY, &mut findings).is_some() {
            loaded += 1;
        } else {
            refused += 1;
        }
        let bare = format!("---\n{block}\n---\n{block}");
        let _ = parse(&bare, "demo", MAX_BODY, &mut findings);
        let _ = parse(&block, "demo", MAX_BODY, &mut findings);
    }
    // Both outcomes happen, so the generator is not producing one kind of
    // document.
    assert!(loaded > 100, "{loaded} loaded");
    assert!(refused > 100, "{refused} refused");
}
