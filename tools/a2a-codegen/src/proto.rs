// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! `a2a.proto` as a plain model: what the generator reads, and nothing else.

use std::path::Path;

use prost_reflect::{Cardinality, DescriptorPool, Kind, MessageDescriptor, Value};

/// The `google.api.FieldBehavior` number for `REQUIRED`.
const FIELD_BEHAVIOR_REQUIRED: i32 = 2;

/// The A2A package; messages outside it (the well-known types) are not
/// generated.
pub const PACKAGE: &str = "lf.a2a.v1";

#[derive(Debug)]
pub struct Schema {
    pub messages: Vec<Message>,
    pub enums: Vec<Enum>,
}

#[derive(Debug)]
pub struct Message {
    /// The name inside the package, `Task` or `Outer.Inner`.
    pub name: String,
    pub comment: String,
    pub fields: Vec<Field>,
    /// Each `oneof` that is not a proto3 `optional`, with its field names.
    pub oneofs: Vec<(String, Vec<String>)>,
}

#[derive(Debug)]
pub struct Field {
    pub name: String,
    pub json_name: String,
    pub number: u32,
    pub ty: FieldType,
    pub repeated: bool,
    /// A `map<K, V>`, with `ty` the value type and `map_key` the key.
    pub map_key: Option<FieldType>,
    pub required: bool,
    /// Declared `optional` (proto3 explicit presence).
    pub optional: bool,
    pub oneof: Option<String>,
    pub comment: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FieldType {
    String,
    Bool,
    I32,
    I64,
    U32,
    U64,
    F32,
    F64,
    Bytes,
    /// A message, by its full name (`lf.a2a.v1.Task`, `google.protobuf.Struct`).
    Message(String),
    /// An enum, by its full name.
    Enum(String),
}

#[derive(Debug)]
pub struct Enum {
    pub name: String,
    pub comment: String,
    pub values: Vec<EnumValue>,
}

#[derive(Debug)]
pub struct EnumValue {
    pub name: String,
    pub number: i32,
    pub comment: String,
}

/// Parses `proto` (with `includes` for its imports) into the model.
pub fn load(proto: &Path, includes: &[&Path]) -> Result<Schema, String> {
    let mut compiler = protox::Compiler::new(includes).map_err(|e| e.to_string())?;
    compiler.include_source_info(true);
    compiler.open_file(proto).map_err(|e| format!("{e:?}"))?;
    let pool = compiler.descriptor_pool();
    let behavior = pool
        .get_extension_by_name("google.api.field_behavior")
        .ok_or("google.api.field_behavior is not in the descriptor pool")?;

    let mut messages = Vec::new();
    for message in pool.all_messages() {
        if message.package_name() != PACKAGE || message.is_map_entry() {
            continue;
        }
        messages.push(message_of(&message, &behavior)?);
    }
    let mut enums = Vec::new();
    for e in pool.all_enums() {
        if e.package_name() != PACKAGE {
            continue;
        }
        enums.push(Enum {
            name: relative(e.full_name()),
            comment: comment(&pool, e.path(), e.parent_file().name()),
            values: e
                .values()
                .map(|v| EnumValue {
                    name: v.name().to_owned(),
                    number: v.number(),
                    comment: comment(&pool, v.path(), e.parent_file().name()),
                })
                .collect(),
        });
    }
    Ok(Schema { messages, enums })
}

fn message_of(
    message: &MessageDescriptor,
    behavior: &prost_reflect::ExtensionDescriptor,
) -> Result<Message, String> {
    let pool = message.parent_pool();
    let file = message.parent_file();
    let mut fields = Vec::new();
    for field in message.fields() {
        let options = field.options();
        let required = match options.get_extension(behavior).as_ref() {
            Value::List(values) => values
                .iter()
                .any(|v| matches!(v, Value::EnumNumber(n) if *n == FIELD_BEHAVIOR_REQUIRED)),
            _ => false,
        };
        let (ty, map_key) = if field.is_map() {
            let Kind::Message(entry) = field.kind() else {
                return Err(format!("{}: a map without an entry", field.full_name()));
            };
            let key = entry.map_entry_key_field();
            let value = entry.map_entry_value_field();
            (type_of(&value.kind()), Some(type_of(&key.kind())))
        } else {
            (type_of(&field.kind()), None)
        };
        let oneof = field
            .containing_oneof()
            .filter(|o| !o.is_synthetic())
            .map(|o| o.name().to_owned());
        fields.push(Field {
            name: field.name().to_owned(),
            json_name: field.json_name().to_owned(),
            number: field.number(),
            ty,
            repeated: field.cardinality() == Cardinality::Repeated && !field.is_map(),
            map_key,
            required,
            optional: field.field_descriptor_proto().proto3_optional(),
            oneof,
            comment: comment(pool, field.path(), file.name()),
        });
    }
    let oneofs = message
        .oneofs()
        .filter(|o| !o.is_synthetic())
        .map(|o| {
            (
                o.name().to_owned(),
                o.fields().map(|f| f.name().to_owned()).collect(),
            )
        })
        .collect();
    Ok(Message {
        name: relative(message.full_name()),
        comment: comment(pool, message.path(), file.name()),
        fields,
        oneofs,
    })
}

fn type_of(kind: &Kind) -> FieldType {
    match kind {
        Kind::String => FieldType::String,
        Kind::Bool => FieldType::Bool,
        Kind::Int32 | Kind::Sint32 | Kind::Sfixed32 => FieldType::I32,
        Kind::Int64 | Kind::Sint64 | Kind::Sfixed64 => FieldType::I64,
        Kind::Uint32 | Kind::Fixed32 => FieldType::U32,
        Kind::Uint64 | Kind::Fixed64 => FieldType::U64,
        Kind::Float => FieldType::F32,
        Kind::Double => FieldType::F64,
        Kind::Bytes => FieldType::Bytes,
        Kind::Message(m) => FieldType::Message(m.full_name().to_owned()),
        Kind::Enum(e) => FieldType::Enum(e.full_name().to_owned()),
    }
}

/// `lf.a2a.v1.Task` as `Task`.
fn relative(full_name: &str) -> String {
    full_name
        .strip_prefix(PACKAGE)
        .and_then(|s| s.strip_prefix('.'))
        .unwrap_or(full_name)
        .to_owned()
}

/// The leading comment the proto gives the element at `path`, without the
/// `//` markers and what is addressed to tools or maintainers rather than
/// readers: `protolint:` directives, `Next ID:` counters, and the internal
/// `(-- ... --)` spans of AIP-192; empty when it has none.
fn comment(pool: &DescriptorPool, path: &[i32], file: &str) -> String {
    let Some(file) = pool.get_file_by_name(file) else {
        return String::new();
    };
    let Some(info) = file.file_descriptor_proto().source_code_info.as_ref() else {
        return String::new();
    };
    info.location
        .iter()
        .find(|l| l.path == path)
        .and_then(|l| l.leading_comments.as_deref())
        .map(|c| {
            without_internal(c)
                .lines()
                .map(|l| l.strip_prefix(' ').unwrap_or(l).trim_end())
                .filter(|l| !l.starts_with("protolint:") && !l.starts_with("Next ID:"))
                .collect::<Vec<_>>()
                .join("\n")
                .trim()
                .to_owned()
        })
        .unwrap_or_default()
}

/// `text` without its AIP-192 internal comments, `(-- ... --)`, which may
/// span lines. An unclosed `(--` runs to the end.
fn without_internal(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find("(--") {
        out.push_str(&rest[..start]);
        rest = rest[start..]
            .find("--)")
            .map_or("", |end| &rest[start + end + 3..]);
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::without_internal;

    #[test]
    fn internal_comments_are_left_out() {
        let text = " (-- api-linter: x=disabled\n     aip.dev/not-precedent: y --)\n Required. The header.\n";
        assert_eq!(without_internal(text), " \n Required. The header.\n");
        assert_eq!(without_internal("a (-- b --) c"), "a  c");
        assert_eq!(without_internal("a (-- never closed"), "a ");
    }
}
