// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The Agent Skills specification, as this module read it.
//!
//! There are no versioned releases of the specification (ADR 0035), so every
//! number here is pinned to a date: [`SPEC_SNAPSHOT`]. The text was
//! `https://agentskills.io/specification` and
//! `https://agentskills.io/client-implementation/adding-skills-support`; the
//! reference validator's behaviour (`skills-ref`, as summarised on that date)
//! is where the Unicode-letter and unknown-field rules come from.

/// The date the specification text this module follows was read, as
/// `YYYY-MM-DD`. Named in the diagnostics about fields this module does not
/// understand, because the specification may have changed since.
pub const SPEC_SNAPSHOT: &str = "2026-11-07";

/// The most characters a `name` may have under the specification.
pub(super) const MAX_NAME_CHARS: usize = 64;

/// The most characters a `name` may have for this module to use it at all.
/// The specification's limit is 64; a longer name is reported and used, up
/// to this bound, so a skill written for a client that does not check is not
/// lost. Past it the name would only cost prompt tokens.
pub(super) const MAX_NAME_CHARS_USABLE: usize = 256;

/// The most characters a `description` may have under the specification.
pub(super) const MAX_DESCRIPTION_CHARS: usize = 1024;

/// The most characters `compatibility` may have under the specification.
pub(super) const MAX_COMPATIBILITY_CHARS: usize = 500;

/// How many lines the specification recommends a `SKILL.md` stay under.
pub(super) const RECOMMENDED_BODY_LINES: usize = 500;

/// The frontmatter fields the specification defines, on the date above.
pub(super) const KNOWN_FIELDS: [&str; 6] = [
    "name",
    "description",
    "license",
    "compatibility",
    "metadata",
    "allowed-tools",
];

/// The largest frontmatter block this module parses, in bytes. A block over
/// it is refused before any of it is read as YAML. The specification's own
/// limits (1024 characters of description, 500 of compatibility, a name of
/// 64) fit in a few kilobytes even in four-byte characters; 8 KiB leaves room
/// for `metadata` and comments.
pub(super) const MAX_FRONTMATTER_BYTES: usize = 8 * 1024;

/// Whether `c` may appear in a skill name this module will use: a Unicode
/// letter or digit, a hyphen, an underscore or a dot. The specification
/// allows letters, digits and hyphens; the other two are common in skills
/// written for clients that do not check, are reported, and cannot do
/// harm in a prompt. Anything else (a space, a quote, markup, a control
/// character, a path separator) can, and the skill is skipped.
pub(super) fn is_usable_name_char(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '-' | '_' | '.')
}

/// Whether `c` is a character the specification allows in a name, case aside:
/// a Unicode letter or digit, or a hyphen.
pub(super) fn is_spec_name_char(c: char) -> bool {
    c == '-' || c.is_alphanumeric()
}

/// Whether `c` is unchanged by lowercasing, which is what "lowercase" means
/// for a character with no case (a digit, a hyphen, a CJK ideograph).
pub(super) fn is_lowercase_invariant(c: char) -> bool {
    let mut lower = c.to_lowercase();
    lower.next() == Some(c) && lower.next().is_none()
}
