// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! `codegen.toml`: where each proto type goes, and how its Rust form differs
//! from the default translation (ADR 0016).
//!
//! Every key is checked against the proto by [`Config::validate`]: an entry
//! for a message, enum, field or value the proto does not have is an error,
//! and so is a proto message or enum the configuration does not mention.

use std::collections::BTreeMap;

use serde::Deserialize;

use crate::proto::Schema;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Every proto message and enum's public Rust path, relative to the
    /// crate root (`task::Task`). Field types refer to each other by it.
    pub paths: BTreeMap<String, String>,
    /// Proto types not generated yet, each with the reason.
    #[serde(default)]
    pub pending: BTreeMap<String, String>,
    #[serde(default)]
    pub enums: BTreeMap<String, EnumConfig>,
    #[serde(default)]
    pub messages: BTreeMap<String, MessageConfig>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EnumConfig {
    pub derives: Vec<String>,
    /// Further attributes, written as they appear (`#[non_exhaustive]`).
    #[serde(default)]
    pub attrs: Vec<String>,
    /// Replaces the proto comment.
    pub doc: Option<String>,
    /// Follows the proto comment.
    pub doc_append: Option<String>,
    #[serde(default)]
    pub values: BTreeMap<String, ValueConfig>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ValueConfig {
    /// Spellings also accepted on input.
    #[serde(default)]
    pub aliases: Vec<String>,
    pub doc: Option<String>,
    pub doc_append: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MessageConfig {
    pub derives: Vec<String>,
    #[serde(default)]
    pub attrs: Vec<String>,
    pub doc: Option<String>,
    pub doc_append: Option<String>,
    #[serde(default)]
    pub fields: BTreeMap<String, FieldConfig>,
    /// Fields the Rust type has and the proto does not, written after the
    /// proto's in the order given.
    #[serde(default)]
    pub extra: Vec<ExtraField>,
}

/// A Rust-only field. It must say why the type carries something the proto
/// does not.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExtraField {
    pub name: String,
    #[serde(rename = "type")]
    pub ty: String,
    #[serde(default)]
    pub serde: Vec<String>,
    pub doc: String,
    pub why: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FieldConfig {
    /// The Rust field name, when not the proto's.
    pub rename: Option<String>,
    /// The Rust type, written out, when not the default translation.
    #[serde(rename = "type")]
    pub ty: Option<String>,
    /// Names also accepted on input.
    #[serde(default)]
    pub aliases: Vec<String>,
    /// Further `#[serde(...)]` arguments, written out (`default`).
    #[serde(default)]
    pub serde: Vec<String>,
    pub doc: Option<String>,
    pub doc_append: Option<String>,
    /// Why this field differs from the default translation; required
    /// whenever `type` or `rename` is set, so each difference says why.
    pub why: Option<String>,
}

impl Config {
    pub fn parse(text: &str) -> Result<Self, String> {
        toml::from_str(text).map_err(|e| format!("codegen.toml: {e}"))
    }

    /// Every entry names something the proto has, and every proto message
    /// and enum is named.
    pub fn validate(&self, schema: &Schema) -> Result<(), String> {
        let mut problems = Vec::new();
        let names: Vec<&str> = schema
            .messages
            .iter()
            .map(|m| m.name.as_str())
            .chain(schema.enums.iter().map(|e| e.name.as_str()))
            .collect();
        for name in &names {
            if !self.paths.contains_key(*name) {
                problems.push(format!("[paths] has no entry for proto type {name}"));
            }
            let configured = self.messages.contains_key(*name) || self.enums.contains_key(*name);
            match (configured, self.pending.contains_key(*name)) {
                (false, false) => problems.push(format!(
                    "proto type {name} is neither generated nor [pending]"
                )),
                (true, true) => problems.push(format!("{name} is both generated and [pending]")),
                _ => {}
            }
        }
        for key in self
            .paths
            .keys()
            .chain(self.pending.keys())
            .chain(self.messages.keys())
            .chain(self.enums.keys())
        {
            if !names.contains(&key.as_str()) {
                problems.push(format!("{key} is not a type in the proto"));
            }
        }
        for (name, config) in &self.messages {
            let Some(message) = schema.messages.iter().find(|m| &m.name == name) else {
                continue;
            };
            for extra in &config.extra {
                let taken = message.fields.iter().any(|f| {
                    let rust = config.fields.get(&f.name).and_then(|c| c.rename.as_ref());
                    rust.unwrap_or(&f.name) == &extra.name
                });
                if taken {
                    problems.push(format!(
                        "{name}.{} is an extra field with a proto field's name",
                        extra.name
                    ));
                }
                if extra.why.trim().is_empty() {
                    problems.push(format!(
                        "{name}.{} is an extra field with an empty `why`",
                        extra.name
                    ));
                }
            }
            for (field, fc) in &config.fields {
                if !message.fields.iter().any(|f| &f.name == field) {
                    problems.push(format!("{name}.{field} is not a field in the proto"));
                }
                if (fc.ty.is_some() || fc.rename.is_some()) && fc.why.is_none() {
                    problems.push(format!(
                        "{name}.{field} changes the default translation without a `why`"
                    ));
                }
            }
        }
        for (name, config) in &self.enums {
            let Some(e) = schema.enums.iter().find(|e| &e.name == name) else {
                continue;
            };
            for value in config.values.keys() {
                if !e.values.iter().any(|v| &v.name == value) {
                    problems.push(format!("{name}.{value} is not a value in the proto"));
                }
            }
        }
        if problems.is_empty() {
            Ok(())
        } else {
            Err(problems.join("\n"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proto::{Field, FieldType, Message};

    fn schema() -> Schema {
        Schema {
            messages: vec![Message {
                name: "Thing".into(),
                comment: "A thing.".into(),
                fields: vec![Field {
                    name: "id".into(),
                    json_name: "id".into(),
                    number: 1,
                    ty: FieldType::String,
                    repeated: false,
                    map_key: None,
                    required: true,
                    optional: false,
                    oneof: None,
                    comment: "Its id.".into(),
                }],
                oneofs: Vec::new(),
            }],
            enums: Vec::new(),
        }
    }

    fn problems(toml: &str) -> String {
        Config::parse(toml)
            .unwrap()
            .validate(&schema())
            .unwrap_err()
    }

    #[test]
    fn a_complete_configuration_validates() {
        let ok = Config::parse(
            "[paths]\nThing = \"m::Thing\"\n[messages.Thing]\nderives = [\"Debug\"]\n",
        )
        .unwrap();
        ok.validate(&schema()).unwrap();
    }

    #[test]
    fn a_proto_type_the_configuration_omits_is_refused() {
        let why = problems("[paths]\nThing = \"m::Thing\"\n");
        assert!(why.contains("neither generated nor [pending]"), "{why}");
        let why = problems("[paths]\n[pending]\nThing = \"later\"\n");
        assert!(
            why.contains("[paths] has no entry for proto type Thing"),
            "{why}"
        );
    }

    #[test]
    fn an_entry_for_something_the_proto_lacks_is_refused() {
        let why = problems(
            "[paths]\nThing = \"m::Thing\"\nGone = \"m::Gone\"\n\
             [messages.Thing]\nderives = []\n[messages.Thing.fields.renamed_away]\naliases = []\n",
        );
        assert!(why.contains("Gone is not a type in the proto"), "{why}");
        assert!(
            why.contains("Thing.renamed_away is not a field in the proto"),
            "{why}"
        );
    }

    #[test]
    fn a_changed_type_must_say_why() {
        let why = problems(
            "[paths]\nThing = \"m::Thing\"\n[messages.Thing]\nderives = []\n\
             [messages.Thing.fields.id]\ntype = \"u64\"\n",
        );
        assert!(
            why.contains("Thing.id changes the default translation without a `why`"),
            "{why}"
        );
    }

    #[test]
    fn an_extra_field_may_not_shadow_a_proto_field() {
        let why = problems(
            "[paths]\nThing = \"m::Thing\"\n[messages.Thing]\nderives = []\n\
             [[messages.Thing.extra]]\nname = \"id\"\ntype = \"u8\"\ndoc = \"d\"\nwhy = \"w\"\n",
        );
        assert!(
            why.contains("Thing.id is an extra field with a proto field's name"),
            "{why}"
        );
        let no_why = "[paths]\nThing = \"m::Thing\"\n[messages.Thing]\nderives = []\n\
                      [[messages.Thing.extra]]\nname = \"old\"\ntype = \"u8\"\ndoc = \"d\"\n";
        let err = Config::parse(no_why).unwrap_err();
        assert!(err.contains("missing field `why`"), "{err}");
        let blank_why = problems(
            "[paths]\nThing = \"m::Thing\"\n[messages.Thing]\nderives = []\n\
             [[messages.Thing.extra]]\nname = \"old\"\ntype = \"u8\"\ndoc = \"d\"\nwhy = \" \"\n",
        );
        assert!(
            blank_why.contains("Thing.old is an extra field with an empty `why`"),
            "{blank_why}"
        );
    }

    #[test]
    fn unknown_keys_are_refused() {
        assert!(Config::parse("[paths]\n[messages.Thing]\nderives = []\ncolour = 1\n").is_err());
    }
}
