//! Language layer: turns the core model into template contexts for one target
//! language. The contexts follow openapi-generator's shapes (CodegenModel,
//! CodegenOperation, ...) so its stock Mustache templates can be used unchanged;
//! Jinja templates read the same contexts.
//!
//! Every model context is a bundle `{models: [{model, importPath}], imports, ...}`,
//! every API context `{operations: {classname, operation: [...]}, imports, ...}`.
//! The generator merges the global context into both and adds `model` (the bundle's
//! model) to model contexts. Files rendered `each = "once"` get the global context,
//! `models` (all `{model, importPath}`), `apiInfo.apis` (all API contexts) and `once`.

pub mod java;
pub mod typescript;

use crate::core::Spec;
use serde_json::{Map, Value};

pub struct Contexts {
    pub global: Value,
    pub models: Vec<Value>,
    pub apis: Vec<Value>,
    /// Extra fields for `each = "once"` files (beyond `models` and `apiInfo`).
    pub once: Value,
}

pub trait Language {
    fn contexts(&self, spec: &Spec) -> Result<Contexts, String>;
}

pub fn by_name(name: &str, options: Map<String, Value>) -> Result<Box<dyn Language>, String> {
    match name {
        "typescript" => Ok(Box::new(typescript::TypeScript::new(options)?)),
        "java" => Ok(Box::new(java::Java::new(options)?)),
        other => Err(format!(
            "unknown language '{other}' (known: typescript, java)"
        )),
    }
}

/// Reads a string option, falling back to `default`.
pub fn opt_str(options: &Map<String, Value>, key: &str, default: &str) -> String {
    options
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or(default)
        .to_string()
}

pub fn opt_bool(options: &Map<String, Value>, key: &str, default: bool) -> bool {
    match options.get(key) {
        Some(Value::Bool(b)) => *b,
        Some(Value::String(s)) => s == "true",
        _ => default,
    }
}

/// Iteration order of a `java.util.HashSet<String>` filled with `items` in the given order.
/// openapi-generator collects some imports in a HashSet, so generated import order
/// depends on Java's String.hashCode bucket layout.
pub fn java_hash_set_order(items: Vec<String>) -> Vec<String> {
    let mut cap: usize = 16;
    while items.len() as f64 > cap as f64 * 0.75 {
        cap *= 2;
    }
    let bucket = |s: &str| {
        let h = java_string_hash(s) as u32;
        ((h ^ (h >> 16)) as usize) & (cap - 1)
    };
    let mut keyed: Vec<(usize, String)> = items.into_iter().map(|s| (bucket(&s), s)).collect();
    keyed.sort_by_key(|(b, _)| *b); // stable: same bucket keeps insertion order
    keyed.into_iter().map(|(_, s)| s).collect()
}

pub fn java_string_hash(s: &str) -> i32 {
    s.encode_utf16()
        .fold(0i32, |h, c| h.wrapping_mul(31).wrapping_add(c as i32))
}

/// `removeEnumValuePrefix`: the common prefix of all values, minus any trailing
/// alphanumerics (["STATUS_ON", "STATUS_OFF"] -> "STATUS_", ["SICK", "SICK_CHILD"] -> "").
pub fn common_enum_prefix<'s>(values: &[&'s str]) -> &'s str {
    if values.len() < 2 {
        return "";
    }
    let first = values[0];
    let mut len = first.len();
    for v in &values[1..] {
        len = first
            .bytes()
            .zip(v.bytes())
            .take(len)
            .take_while(|(a, b)| a == b)
            .count();
    }
    first[..len].trim_end_matches(|c: char| c.is_ascii_alphanumeric())
}
