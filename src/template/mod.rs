//! Template layer: a template set is a stack of directories (built-in layers plus an
//! optional user directory on top). A template is addressed by its logical name; the
//! file found in the highest layer decides the engine:
//!
//! - `<name>.jinja`    → MiniJinja
//! - `<name>.mustache` → Mustache (jmustache-compatible)
//! - `<name>`          → copied verbatim (binary files such as `gradle-wrapper.jar`)
//!
//! So a single template of a Mustache set can be replaced by a Jinja one and vice versa.

pub mod jinja;
pub mod manifest;
pub mod mustache;

use crate::naming;
use include_dir::{Dir, include_dir};
use serde_json::Value;
use std::collections::{BTreeMap, HashMap};
use std::path::Path;

pub use manifest::Manifest;

static BUILTIN: Dir<'static> = include_dir!("$CARGO_MANIFEST_DIR/templates");

/// A file of the merged layer stack and the layer it came from (higher wins).
struct Entry {
    layer: usize,
    content: Vec<u8>,
}

pub struct TemplateSet {
    files: BTreeMap<String, Entry>,
    mustache: mustache::Engine,
    jinja: minijinja::Environment<'static>,
}

pub enum Rendered {
    Text(String),
    Bytes(Vec<u8>),
}

/// Where a template set comes from: a built-in set name (`java`) or a directory.
pub fn load_manifest(set: &str) -> Result<(Manifest, Source), String> {
    let path = Path::new(set);
    let source = if path.is_dir() {
        Source::Dir(path.to_path_buf())
    } else {
        Source::Builtin(set.to_string())
    };
    let text = source
        .read("manifest.toml")
        .ok_or_else(|| format!("template set '{set}' has no manifest.toml"))?;
    let manifest = Manifest::parse(&String::from_utf8_lossy(&text))
        .map_err(|e| format!("{set}/manifest.toml: {e}"))?;
    Ok((manifest, source))
}

pub enum Source {
    Builtin(String),
    Dir(std::path::PathBuf),
}

impl Source {
    fn read(&self, rel: &str) -> Option<Vec<u8>> {
        match self {
            Source::Builtin(set) => BUILTIN
                .get_file(format!("{set}/{rel}"))
                .map(|f| f.contents().to_vec()),
            Source::Dir(d) => std::fs::read(d.join(rel)).ok(),
        }
    }

    /// All files below `sub`, keyed by path relative to `sub`.
    fn files(&self, sub: &str) -> Result<Vec<(String, Vec<u8>)>, String> {
        let mut out = Vec::new();
        match self {
            Source::Builtin(set) => {
                let root = if sub.is_empty() || sub == "." {
                    set.clone()
                } else {
                    format!("{set}/{sub}")
                };
                let dir = BUILTIN
                    .get_dir(&root)
                    .ok_or_else(|| format!("no built-in template directory {root}"))?;
                collect_builtin(dir, &root, &mut out);
            }
            Source::Dir(d) => collect_fs(&d.join(sub), &d.join(sub), &mut out)?,
        }
        Ok(out)
    }
}

fn collect_builtin(dir: &Dir<'static>, root: &str, out: &mut Vec<(String, Vec<u8>)>) {
    for f in dir.files() {
        let rel = f.path().to_string_lossy();
        out.push((rel[root.len() + 1..].to_string(), f.contents().to_vec()));
    }
    for d in dir.dirs() {
        collect_builtin(d, root, out);
    }
}

fn collect_fs(root: &Path, dir: &Path, out: &mut Vec<(String, Vec<u8>)>) -> Result<(), String> {
    let entries = std::fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    for entry in entries {
        let path = entry.map_err(|e| e.to_string())?.path();
        if path.is_dir() {
            collect_fs(root, &path, out)?;
        } else {
            let rel = path
                .strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            out.push((
                rel,
                std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?,
            ));
        }
    }
    Ok(())
}

impl TemplateSet {
    /// Stacks the manifest's layers of `source`, then `overrides` (a plain directory) on top.
    pub fn load(
        source: &Source,
        layers: &[String],
        overrides: Option<&Path>,
    ) -> Result<Self, String> {
        let mut files = BTreeMap::new();
        let mut add = |layer: usize, list: Vec<(String, Vec<u8>)>| {
            for (rel, content) in list {
                files.insert(rel, Entry { layer, content });
            }
        };
        for (i, sub) in layers.iter().enumerate() {
            add(i, source.files(sub)?);
        }
        if let Some(dir) = overrides {
            add(layers.len(), Source::Dir(dir.to_path_buf()).files("")?);
        }

        let mut mustache = mustache::Engine::new(mustache_lambdas());
        for (rel, e) in &files {
            if let Some(name) = rel.strip_suffix(".mustache") {
                mustache.add(name, &String::from_utf8_lossy(&e.content))?;
            }
        }
        let sources: HashMap<String, String> = files
            .iter()
            .filter(|(rel, _)| rel.ends_with(".jinja"))
            .map(|(rel, e)| {
                (
                    rel.clone(),
                    String::from_utf8_lossy(&e.content).into_owned(),
                )
            })
            .collect();
        Ok(TemplateSet {
            files,
            mustache,
            jinja: jinja::env(sources),
        })
    }

    pub fn render(&self, name: &str, ctx: &Value) -> Result<Rendered, String> {
        let candidates = [
            format!("{name}.jinja"),
            format!("{name}.mustache"),
            name.to_string(),
        ];
        let found = candidates
            .iter()
            .filter_map(|c| self.files.get(c).map(|e| (c, e)))
            .max_by_key(|(_, e)| e.layer)
            .ok_or_else(|| format!("no template '{name}' (.jinja, .mustache or plain file)"))?;
        match found.0 {
            c if c.ends_with(".jinja") => jinja::render(&self.jinja, c, ctx).map(Rendered::Text),
            c if c.ends_with(".mustache") => self.mustache.render(name, ctx).map(Rendered::Text),
            _ => Ok(Rendered::Bytes(found.1.content.clone())),
        }
    }
}

/// openapi-generator's `IndentedLambda`: indent every line after the first.
pub fn indented(text: &str, indent: &str) -> String {
    if text.is_empty() {
        return String::new();
    }
    let mut lines: Vec<&str> = text
        .split('\n')
        .map(|l| l.strip_suffix('\r').unwrap_or(l))
        .collect();
    while lines.len() > 1 && lines.last() == Some(&"") {
        lines.pop(); // Java's String.split drops trailing empty strings
    }
    let mut out = String::with_capacity(text.len() + 16);
    for (i, l) in lines.iter().enumerate() {
        if i > 0 {
            out.push('\n');
            out.push_str(indent);
        }
        out.push_str(l);
    }
    out
}

/// The `lambda.*` sections openapi-generator registers for every generator.
fn mustache_lambdas() -> HashMap<&'static str, mustache::Lambda> {
    let mut l: HashMap<&'static str, mustache::Lambda> = HashMap::new();
    l.insert("lowercase", |t| t.to_lowercase());
    l.insert("uppercase", |t| t.to_uppercase());
    l.insert("snakecase", naming::underscore);
    l.insert("kebabcase", naming::kebab);
    l.insert("camelcase", |t| naming::camelize(t, true));
    l.insert("pascalcase", |t| naming::camelize(t, false));
    l.insert("indented", |t| indented(t, "    "));
    l.insert("indented_8", |t| indented(t, "        "));
    l.insert("indented_12", |t| indented(t, "            "));
    l.insert("indented_16", |t| indented(t, "                "));
    l.insert("indented_star_1", |t| indented(t, " * "));
    l.insert("indented_star_4", |t| indented(t, "     * "));
    l.insert("indented_1", |t| indented(t, " "));
    l
}
