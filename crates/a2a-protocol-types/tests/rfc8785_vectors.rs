// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! RFC 8785 (JSON Canonicalization Scheme) test vectors, from the RFC itself.
//!
//! Every expected value below is copied from the RFC text
//! (<https://www.rfc-editor.org/rfc/rfc8785.txt>): Appendix B's number
//! serialization table, the §3.2.3 sort-order sample and the §3.2.4 UTF-8
//! bytes. None is computed by this crate, which is the point — the unit tests
//! beside `canonicalize` were written by the same hand as the canonicalizer,
//! and a vector the implementer chose cannot catch the implementer's
//! misreading. Audit escape class 7 (`docs/adopter-audit-2026-09-22.md`).
//!
//! Numbers are checked two ways, because a signer meets them two ways:
//!
//! - **from the IEEE 754 bits**, which isolates the ECMAScript number
//!   formatter (`Number::toString`) from any parser;
//! - **from the JSON text**, which is what a verifier holds: a peer's card
//!   arrives as text, and a parser that reads a double one ULP off makes the
//!   canonical bytes, and so the signature, disagree with the signer's.

#![cfg(feature = "signing")]

use a2a_protocol_types::signing::canonicalize;
use serde_json::{Number, Value};

/// RFC 8785 Appendix B, Table 1: (IEEE 754 bits, JSON representation).
///
/// The two rows the table leaves blank, NaN (`7fffffffffffffff`) and
/// Infinity (`7ff0000000000000`), are omitted: JSON cannot express them, and
/// `serde_json::Number::from_f64` refuses both (asserted below), so no
/// `Value` can carry one to the canonicalizer.
const APPENDIX_B: &[(u64, &str)] = &[
    (0x0000_0000_0000_0000, "0"),
    (0x8000_0000_0000_0000, "0"),
    (0x0000_0000_0000_0001, "5e-324"),
    (0x8000_0000_0000_0001, "-5e-324"),
    (0x7fef_ffff_ffff_ffff, "1.7976931348623157e+308"),
    (0xffef_ffff_ffff_ffff, "-1.7976931348623157e+308"),
    (0x4340_0000_0000_0000, "9007199254740992"),
    (0xc340_0000_0000_0000, "-9007199254740992"),
    (0x4430_0000_0000_0000, "295147905179352830000"),
    (0x44b5_2d02_c7e1_4af5, "9.999999999999997e+22"),
    (0x44b5_2d02_c7e1_4af6, "1e+23"),
    (0x44b5_2d02_c7e1_4af7, "1.0000000000000001e+23"),
    (0x444b_1ae4_d6e2_ef4e, "999999999999999700000"),
    (0x444b_1ae4_d6e2_ef4f, "999999999999999900000"),
    (0x444b_1ae4_d6e2_ef50, "1e+21"),
    (0x3eb0_c6f7_a0b5_ed8c, "9.999999999999997e-7"),
    (0x3eb0_c6f7_a0b5_ed8d, "0.000001"),
    (0x41b3_de43_5555_5553, "333333333.3333332"),
    (0x41b3_de43_5555_5554, "333333333.33333325"),
    (0x41b3_de43_5555_5555, "333333333.3333333"),
    (0x41b3_de43_5555_5556, "333333333.3333334"),
    (0x41b3_de43_5555_5557, "333333333.33333343"),
    (0xbecb_f647_612f_3696, "-0.0000033333333333333333"),
    (0x4314_3ff3_c1cb_0959, "1424953923781206.2"),
];

fn canonical(value: &Value) -> String {
    String::from_utf8(canonicalize(value).expect("canonicalize")).expect("UTF-8")
}

#[test]
fn rfc8785_appendix_b_from_ieee754_bits() {
    let mut wrong = Vec::new();
    for &(bits, expected) in APPENDIX_B {
        let f = f64::from_bits(bits);
        let value = Value::Number(Number::from_f64(f).expect("finite"));
        let got = canonical(&value);
        if got != expected {
            wrong.push(format!("{bits:016x}: expected {expected}, got {got}"));
        }
    }
    assert!(
        wrong.is_empty(),
        "{} of {} vectors wrong:\n{}",
        wrong.len(),
        APPENDIX_B.len(),
        wrong.join("\n")
    );
}

#[test]
fn rfc8785_appendix_b_from_json_text() {
    let mut wrong = Vec::new();
    for &(bits, expected) in APPENDIX_B {
        let value: Value = serde_json::from_str(expected).expect("valid JSON number");
        let got = canonical(&value);
        if got != expected {
            wrong.push(format!("{bits:016x}: {expected} re-canonicalized as {got}"));
        }
    }
    assert!(
        wrong.is_empty(),
        "{} of {} vectors wrong:\n{}",
        wrong.len(),
        APPENDIX_B.len(),
        wrong.join("\n")
    );
}

/// JCS numbers are IEEE 754 doubles, integers included, so an integer
/// literal beyond 2^53 canonicalizes to the double it parses to, exactly as
/// `JSON.parse` then `JSON.stringify` would render it. Appendix B's
/// `295147905179352830000` row is this rule for a literal too large for any
/// integer type; this is the same rule for one that fits in a `u64`.
#[test]
fn rfc8785_integers_are_doubles() {
    for (text, expected) in [
        ("9007199254740993", "9007199254740992"),
        ("-9007199254740993", "-9007199254740992"),
        ("18446744073709551615", "18446744073709552000"),
        ("12345678901234567", "12345678901234568"),
        ("9007199254740991", "9007199254740991"),
        ("9007199254740992", "9007199254740992"),
        // 2^56 + 32: its exact digits are one longer than its shortest form
        // and end in 8, and the even truncation (…960) also reads back — only
        // a tie may choose that. V8 renders it …970.
        ("72057594037927968", "72057594037927970"),
        ("0", "0"),
        ("-0", "0"),
    ] {
        let value: Value = serde_json::from_str(text).expect("valid JSON number");
        assert_eq!(canonical(&value), expected, "{text}");
    }
}

/// Values beyond the RFC's table, each chosen for a branch of the tie rule,
/// with V8's rendering (`JSON.stringify`, Node 22.22.2) as the expected
/// value: RFC 8785 defines numbers as ECMAScript renders them, so V8 is the
/// reference the RFC itself points to (Appendix B's closing paragraph).
///
/// - `0.3`: the shortest form rounds up, and the truncation (`2`) is even —
///   no tie, so the even truncation must not be taken.
/// - `1424953923781206.75`: a tie whose truncation is odd; the even
///   neighbour is the upper one.
/// - `2^-24`: a tie whose even truncation does not read back, because the
///   double below a power of two is half as far away.
/// - `1.8211544543517955`: not a tie, though the even truncation (`…954`)
///   also reads back as the same double; it is farther away, so only a tie
///   may choose it.
#[test]
fn v8_rendering_of_tie_rule_edges() {
    for (f, expected) in [
        (0.3, "0.3"),
        // Written as a sum: the literal `…206.75` trips clippy's
        // excessive_precision, though it is exact at this magnitude.
        (1_424_953_923_781_206.0 + 0.75, "1424953923781206.8"),
        (2f64.powi(-24), "5.960464477539063e-8"),
        (-(2f64.powi(-24)), "-5.960464477539063e-8"),
        (1.821_154_454_351_795_5, "1.8211544543517955"),
    ] {
        let value = Value::Number(Number::from_f64(f).expect("finite"));
        assert_eq!(canonical(&value), expected, "{f:e}");
    }
}

#[test]
fn rfc8785_nan_and_infinity_cannot_reach_the_canonicalizer() {
    assert!(Number::from_f64(f64::from_bits(0x7fff_ffff_ffff_ffff)).is_none());
    assert!(Number::from_f64(f64::from_bits(0x7ff0_0000_0000_0000)).is_none());
}

/// RFC 8785 §3.2.3: property names sort by UTF-16 code units.
#[test]
fn rfc8785_section_3_2_3_sort_order() {
    let input = r#"{
        "\u20ac": "Euro Sign",
        "\r": "Carriage Return",
        "\ufb33": "Hebrew Letter Dalet With Dagesh",
        "1": "One",
        "\ud83d\ude00": "Emoji: Grinning Face",
        "\u0080": "Control",
        "\u00f6": "Latin Small Letter O With Diaeresis"
    }"#;
    let value: Value = serde_json::from_str(input).expect("valid JSON");
    let text = canonical(&value);
    let expected = [
        "Carriage Return",
        "One",
        "Control",
        "Latin Small Letter O With Diaeresis",
        "Euro Sign",
        "Emoji: Grinning Face",
        "Hebrew Letter Dalet With Dagesh",
    ];
    let positions: Vec<usize> = expected
        .iter()
        .map(|v| {
            text.find(v)
                .unwrap_or_else(|| panic!("{v} missing from {text}"))
        })
        .collect();
    assert!(
        positions.windows(2).all(|w| w[0] < w[1]),
        "values not in RFC 8785 §3.2.3 order: {text}"
    );
}

/// RFC 8785 §3.2.2's sample input and the bytes §3.2.4 gives for it.
#[test]
fn rfc8785_section_3_2_4_utf8_bytes() {
    let input = r#"{
       "numbers": [333333333.33333329, 1E30, 4.50,
                   2e-3, 0.000000000000000000000000001],
       "string": "\u20ac$\u000F\u000aA'\u0042\u0022\u005c\\\"\/",
       "literals": [null, true, false]
     }"#;
    let expected_hex = "\
        7b 22 6c 69 74 65 72 61 6c 73 22 3a 5b 6e 75 6c 6c 2c 74 72 \
        75 65 2c 66 61 6c 73 65 5d 2c 22 6e 75 6d 62 65 72 73 22 3a \
        5b 33 33 33 33 33 33 33 33 33 2e 33 33 33 33 33 33 33 2c 31 \
        65 2b 33 30 2c 34 2e 35 2c 30 2e 30 30 32 2c 31 65 2d 32 37 \
        5d 2c 22 73 74 72 69 6e 67 22 3a 22 e2 82 ac 24 5c 75 30 30 \
        30 66 5c 6e 41 27 42 5c 22 5c 5c 5c 5c 5c 22 2f 22 7d";
    let expected: Vec<u8> = expected_hex
        .split_whitespace()
        .map(|h| u8::from_str_radix(h, 16).expect("hex"))
        .collect();
    let value: Value = serde_json::from_str(input).expect("valid JSON");
    let got = canonicalize(&value).expect("canonicalize");
    assert_eq!(
        String::from_utf8_lossy(&got),
        String::from_utf8_lossy(&expected),
        "canonical bytes differ from RFC 8785 §3.2.4"
    );
}
