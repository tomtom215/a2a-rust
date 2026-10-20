// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The wire names a type's `Deserialize` impl accepts, read from the impl.
//!
//! A sample value shows only the fields it populates, so a new optional
//! field skipped when empty would never reach a snapshot built from samples.
//! A derived `Deserialize` hands its full list of field (or variant) names,
//! after renaming and including aliases, to the deserializer it is given;
//! this deserializer records that list and stops.

use serde::de::{DeserializeOwned, Deserializer, Visitor};

#[derive(Debug)]
enum Seen {
    Names(&'static [&'static str]),
    Other(String),
}

impl std::fmt::Display for Seen {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for Seen {}

impl serde::de::Error for Seen {
    fn custom<T: std::fmt::Display>(msg: T) -> Self {
        Self::Other(msg.to_string())
    }
}

struct Probe;

impl<'de> Deserializer<'de> for Probe {
    type Error = Seen;

    fn deserialize_any<V: Visitor<'de>>(self, _: V) -> Result<V::Value, Seen> {
        Err(Seen::Other("neither a struct nor an enum".to_owned()))
    }

    fn deserialize_struct<V: Visitor<'de>>(
        self,
        _: &'static str,
        fields: &'static [&'static str],
        _: V,
    ) -> Result<V::Value, Seen> {
        Err(Seen::Names(fields))
    }

    fn deserialize_enum<V: Visitor<'de>>(
        self,
        _: &'static str,
        variants: &'static [&'static str],
        _: V,
    ) -> Result<V::Value, Seen> {
        Err(Seen::Names(variants))
    }

    serde::forward_to_deserialize_any! {
        bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string
        bytes byte_buf option unit unit_struct newtype_struct seq tuple
        tuple_struct map identifier ignored_any
    }
}

/// The field names of a struct, or the variant names of an enum, as `T`
/// reads them off the wire, sorted.
pub fn wire_names<T: DeserializeOwned>() -> Vec<&'static str> {
    match T::deserialize(Probe) {
        Err(Seen::Names(names)) => {
            let mut names = names.to_vec();
            names.sort_unstable();
            names
        }
        Err(Seen::Other(e)) => panic!(
            "{} is not a derived struct or enum: {e}",
            std::any::type_name::<T>()
        ),
        Ok(_) => panic!("{} deserialized from nothing", std::any::type_name::<T>()),
    }
}
