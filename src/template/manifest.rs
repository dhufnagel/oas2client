//! `manifest.toml` of a template set: which language builds the contexts, which
//! directories form the template layers, and which files are rendered.
//!
//! ```toml
//! language = "java"
//! layers = [".", "libraries/okhttp-gson"]    # later layers override earlier ones
//!
//! [options]                                  # language options, see lang::*
//! invokerPackage = "org.openapitools.client"
//!
//! [[files]]
//! each = "model"                             # model | api | once
//! template = "model"                         # model.jinja / model.mustache / model
//! path = "src/main/java/{modelFolder}/{model.classname}.java"
//! when = "model.isEnum"                      # optional: render only if truthy
//! unless = "hasAuthMethods"                  # optional: render only if falsy
//! ```

use serde::Deserialize;
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub language: String,
    #[serde(default = "default_layers")]
    pub layers: Vec<String>,
    #[serde(default)]
    pub options: BTreeMap<String, toml::Value>,
    #[serde(default)]
    pub files: Vec<FileRule>,
    /// Path prefixes left out of `.openapi-generator/FILES` (e.g. tests).
    #[serde(default)]
    pub unlisted: Vec<String>,
}

#[derive(Debug, Deserialize, Clone, Copy, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Each {
    Model,
    Api,
    Once,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileRule {
    pub each: Each,
    pub template: String,
    pub path: String,
    pub when: Option<String>,
    pub unless: Option<String>,
}

fn default_layers() -> Vec<String> {
    vec![".".into()]
}

impl Manifest {
    pub fn parse(text: &str) -> Result<Self, String> {
        toml::from_str(text).map_err(|e| e.to_string())
    }

    pub fn unlisted(&self, path: &str) -> bool {
        self.unlisted.iter().any(|p| path.starts_with(p.as_str()))
    }

    /// Options as JSON, for the language layer.
    pub fn options_json(&self) -> serde_json::Map<String, Value> {
        self.options
            .iter()
            .map(|(k, v)| (k.clone(), serde_json::to_value(v).unwrap()))
            .collect()
    }
}

impl FileRule {
    pub fn applies(&self, ctx: &Value) -> bool {
        self.when.as_deref().is_none_or(|p| truthy(lookup(ctx, p)))
            && self
                .unless
                .as_deref()
                .is_none_or(|p| !truthy(lookup(ctx, p)))
    }

    /// Output path with `{dotted.name}` placeholders filled from the context.
    pub fn path(&self, ctx: &Value) -> Result<String, String> {
        let mut out = String::new();
        let mut rest = self.path.as_str();
        while let Some(start) = rest.find('{') {
            out.push_str(&rest[..start]);
            let end = rest[start..]
                .find('}')
                .ok_or_else(|| format!("unclosed '{{' in path {}", self.path))?
                + start;
            let key = &rest[start + 1..end];
            match lookup(ctx, key) {
                Some(Value::String(s)) => out.push_str(s),
                _ => {
                    return Err(format!(
                        "path {}: '{key}' is not a string in the context",
                        self.path
                    ));
                }
            }
            rest = &rest[end + 1..];
        }
        out.push_str(rest);
        Ok(out)
    }
}

fn lookup<'a>(ctx: &'a Value, path: &str) -> Option<&'a Value> {
    path.split('.').try_fold(ctx, |v, k| v.get(k))
}

fn truthy(v: Option<&Value>) -> bool {
    match v {
        None | Some(Value::Null) | Some(Value::Bool(false)) => false,
        Some(Value::Array(a)) => !a.is_empty(),
        Some(Value::String(s)) => !s.is_empty() && s != "false",
        _ => true,
    }
}
