//! serde_json silently keeps the last of duplicate object keys; openapi-generator rejects
//! such specs. This walks the raw JSON once and reports the first duplicate key.

use serde::de::{self, Deserialize, Deserializer, MapAccess, SeqAccess, Visitor};
use std::collections::HashSet;
use std::fmt;

struct NoDuplicates;

impl<'de> Deserialize<'de> for NoDuplicates {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        d.deserialize_any(NoDuplicates)
    }
}

impl<'de> Visitor<'de> for NoDuplicates {
    type Value = NoDuplicates;

    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("any JSON value")
    }
    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self, A::Error> {
        let mut seen = HashSet::new();
        while let Some(key) = map.next_key::<String>()? {
            if !seen.insert(key.clone()) {
                return Err(de::Error::custom(format!("duplicate key '{key}'")));
            }
            map.next_value::<NoDuplicates>()?;
        }
        Ok(NoDuplicates)
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self, A::Error> {
        while seq.next_element::<NoDuplicates>()?.is_some() {}
        Ok(NoDuplicates)
    }
    fn visit_bool<E>(self, _: bool) -> Result<Self, E> {
        Ok(NoDuplicates)
    }
    fn visit_i64<E>(self, _: i64) -> Result<Self, E> {
        Ok(NoDuplicates)
    }
    fn visit_u64<E>(self, _: u64) -> Result<Self, E> {
        Ok(NoDuplicates)
    }
    fn visit_f64<E>(self, _: f64) -> Result<Self, E> {
        Ok(NoDuplicates)
    }
    fn visit_str<E>(self, _: &str) -> Result<Self, E> {
        Ok(NoDuplicates)
    }
    fn visit_unit<E>(self) -> Result<Self, E> {
        Ok(NoDuplicates)
    }
}

pub fn check(raw: &str) -> Result<(), String> {
    serde_json::from_str::<NoDuplicates>(raw)
        .map(|_| ())
        .map_err(|e| e.to_string())
}
