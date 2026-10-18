// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Rust source for a message with a `oneof` (ADR 0016).
//!
//! A message that is only its `oneof` becomes an enum with one variant per
//! member. A message with other fields becomes a struct whose field, named
//! after the `oneof`, holds the `oneof`'s enum flattened, as `ProtoJSON`
//! writes the member beside the other fields. A variant's JSON name is the
//! member's; `Deserialize` is derived, generated with
//! `crate::oneof::oneof_deserialize!`, or written by hand beside the type.

use std::fmt::Write as _;

use crate::config::{Config, FieldConfig, MessageConfig, OneofConfig, VariantConfig};
use crate::emit::{default_type, derives, field, rust_name, split_path};
use crate::proto::{Field, Message};
use crate::text::{camel, doc};

/// The message's items: the enum, or the struct followed by its enum.
pub fn emit(m: &Message, mc: &MessageConfig, config: &Config) -> Result<String, String> {
    let [(oneof, members)] = m.oneofs.as_slice() else {
        return Err(format!(
            "{}: only one oneof per message is generated",
            m.name
        ));
    };
    let oc = mc
        .oneofs
        .get(oneof)
        .ok_or_else(|| format!("{}.{oneof} has no `oneofs` entry", m.name))?;
    let variants = |f: &Field| members.contains(&f.name);
    let Some(path) = &oc.path else {
        let name = rust_name(config, &m.name)?;
        let mut out = doc(
            &m.name,
            &m.comment,
            mc.doc.as_deref(),
            mc.doc_append.as_deref(),
            0,
        )?;
        out.push_str(&enum_body(
            m,
            &name,
            &mc.derives,
            &mc.attrs,
            oc,
            m.fields.iter().filter(|f| variants(f)),
            config,
        )?);
        return Ok(out);
    };
    let (module, enum_name) = path
        .rsplit_once("::")
        .ok_or_else(|| format!("{}.{oneof}: path {path:?} names no module", m.name))?;
    let (message_module, name) = split_path(config, &m.name)?;
    if module != message_module {
        return Err(format!(
            "{}.{oneof}: the enum must be in the message's module, {message_module}",
            m.name
        ));
    }
    let serde = has_serde(&mc.derives);
    let mut out = doc(
        &m.name,
        &m.comment,
        mc.doc.as_deref(),
        mc.doc_append.as_deref(),
        0,
    )?;
    let _ = writeln!(out, "#[derive({})]", derives(&mc.derives));
    if serde {
        out.push_str("#[serde(rename_all = \"camelCase\")]\n");
    }
    for attr in &mc.attrs {
        let _ = writeln!(out, "{attr}");
    }
    let _ = writeln!(out, "pub struct {name} {{");
    let default = FieldConfig::default();
    let mut placed = false;
    for f in &m.fields {
        if variants(f) {
            if !placed {
                placed = true;
                out.push_str(&doc(
                    &format!("{}.{oneof}", m.name),
                    "",
                    oc.field_doc.as_deref(),
                    None,
                    1,
                )?);
                let flatten = if serde {
                    vec!["flatten".to_owned()]
                } else {
                    Vec::new()
                };
                field(&mut out, oneof, &format!("crate::{path}"), flatten, &[]);
            }
            continue;
        }
        let fc = mc.fields.get(&f.name).unwrap_or(&default);
        let field_name = fc.rename.clone().unwrap_or_else(|| f.name.clone());
        let ty = match &fc.ty {
            Some(ty) => ty.clone(),
            None => {
                default_type(f, config).map_err(|why| format!("{}.{}: {why}", m.name, f.name))?
            }
        };
        out.push_str(&doc(
            &format!("{}.{}", m.name, f.name),
            &f.comment,
            fc.doc.as_deref(),
            fc.doc_append.as_deref(),
            1,
        )?);
        let mut attrs = Vec::new();
        if serde && camel(&field_name) != f.json_name {
            attrs.push(format!("rename = {:?}", f.json_name));
        }
        for alias in &fc.aliases {
            attrs.push(format!("alias = {alias:?}"));
        }
        if serde {
            field(&mut out, &field_name, &ty, attrs, &fc.serde);
        } else {
            let _ = writeln!(out, "    pub {field_name}: {ty},");
        }
    }
    out.push_str("}\n\n");
    out.push_str(&doc(
        &format!("{}.{oneof}", m.name),
        "",
        oc.doc.as_deref(),
        None,
        0,
    )?);
    out.push_str(&enum_body(
        m,
        enum_name,
        &oc.derives,
        &oc.attrs,
        oc,
        m.fields.iter().filter(|f| variants(f)),
        config,
    )?);
    Ok(out)
}

fn has_serde(derives: &[String]) -> bool {
    derives
        .iter()
        .any(|d| d == "Serialize" || d == "Deserialize")
}

/// `status_update` as `StatusUpdate`.
fn variant_name(member: &str) -> String {
    let camel = camel(member);
    let mut chars = camel.chars();
    chars
        .next()
        .map(|c| c.to_ascii_uppercase().to_string() + chars.as_str())
        .unwrap_or_default()
}

/// A variant's name as serde's `rename_all = "camelCase"` writes it:
/// `StatusUpdate` as `statusUpdate`.
fn serde_camel(variant: &str) -> String {
    let mut chars = variant.chars();
    chars
        .next()
        .map(|c| c.to_ascii_lowercase().to_string() + chars.as_str())
        .unwrap_or_default()
}

/// The enum from its derives and attributes on: its variants, then the
/// `oneof_deserialize!` call when configured.
fn enum_body<'a>(
    m: &Message,
    name: &str,
    derive_list: &[String],
    attrs: &[String],
    oc: &OneofConfig,
    members: impl Iterator<Item = &'a Field>,
    config: &Config,
) -> Result<String, String> {
    let serde = has_serde(derive_list);
    if oc.deserialize_macro && derive_list.iter().any(|d| d == "Deserialize") {
        return Err(format!(
            "{name}: `deserialize_macro` implements Deserialize; do not derive it too"
        ));
    }
    let mut out = String::new();
    let _ = writeln!(out, "#[derive({})]", derives(derive_list));
    if serde {
        out.push_str("#[serde(rename_all = \"camelCase\")]\n");
    }
    for attr in attrs {
        let _ = writeln!(out, "{attr}");
    }
    let _ = writeln!(out, "pub enum {name} {{");
    let default = VariantConfig::default();
    let mut arms = Vec::new();
    for f in members {
        let vc = oc.variants.get(&f.name).unwrap_or(&default);
        let variant = vc.rename.clone().unwrap_or_else(|| variant_name(&f.name));
        let ty = match &vc.ty {
            Some(ty) => ty.clone(),
            None => {
                let mut present = f.clone();
                present.required = true;
                default_type(&present, config)
                    .map_err(|why| format!("{}.{}: {why}", m.name, f.name))?
            }
        };
        out.push_str(&doc(
            &format!("{}.{}", m.name, f.name),
            &f.comment,
            vc.doc.as_deref(),
            vc.doc_append.as_deref(),
            1,
        )?);
        if let Some(note) = &vc.deprecated {
            let since = vc.deprecated_since.as_deref().unwrap_or_default();
            let _ = writeln!(out, "    #[deprecated(since = {since:?}, note = {note:?})]");
        }
        if serde && serde_camel(&variant) != f.json_name {
            let _ = writeln!(out, "    #[serde(rename = {:?})]", f.json_name);
        }
        let _ = writeln!(out, "    {variant}({ty}),");
        let proto = if f.name == f.json_name {
            String::new()
        } else {
            format!(" | {:?}", f.name)
        };
        arms.push(format!("{variant}({ty}) = {:?}{proto}", f.json_name));
    }
    out.push_str("}\n");
    if oc.deserialize_macro {
        let _ = write!(
            out,
            "\ncrate::oneof::oneof_deserialize!(\n    {name},\n    [\n        {},\n    ]\n);\n",
            arms.join(",\n        ")
        );
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::{serde_camel, variant_name};

    #[test]
    fn variant_names_round_trip_through_serde_camel_case() {
        for (member, variant, json) in [
            ("task", "Task", "task"),
            ("status_update", "StatusUpdate", "statusUpdate"),
            (
                "authorization_code",
                "AuthorizationCode",
                "authorizationCode",
            ),
        ] {
            assert_eq!(variant_name(member), variant);
            assert_eq!(serde_camel(variant), json);
        }
    }
}
