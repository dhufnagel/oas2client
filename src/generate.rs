//! Ties the layers together: spec → language contexts → template set → files.

use crate::core::{self, Spec};
use crate::lang;
use crate::template::{self, Rendered, TemplateSet, manifest::Each};
use serde_json::{Map, Value};
use std::path::{Path, PathBuf};

pub struct Request {
    pub spec: PathBuf,
    pub output: PathBuf,
    /// Built-in set name (`typescript-fetch`, `java`) or a directory with a manifest.
    pub set: String,
    /// Directory whose templates override the set's templates by name.
    pub overrides: Option<PathBuf>,
    /// Language options overriding the manifest's `[options]`.
    pub options: Map<String, Value>,
}

/// Generates all files and returns their paths relative to the output directory.
pub fn run(req: &Request) -> Result<Vec<String>, String> {
    let raw =
        std::fs::read_to_string(&req.spec).map_err(|e| format!("{}: {e}", req.spec.display()))?;
    let root = core::load(&raw)?;
    let spec = Spec::new(&root)?;

    let (manifest, source) = template::load_manifest(&req.set)?;
    let mut options = manifest.options_json();
    options.extend(req.options.clone());
    let language = lang::by_name(&manifest.language, options)?;
    let templates = TemplateSet::load(&source, &manifest.layers, req.overrides.as_deref())?;
    let ctx = language.contexts(&spec)?;
    if let Some(dir) = std::env::var_os("OAS2CLIENT_DUMP_CONTEXTS") {
        dump_contexts(Path::new(&dir), &ctx)?;
    }

    let mut out = Output {
        root: req.output.clone(),
        files: Vec::new(),
    };
    let lang::Contexts {
        mut global,
        models,
        apis,
        once: extra,
    } = ctx;
    for (each, list) in [(Each::Model, &models), (Each::Api, &apis)] {
        let rules: Vec<_> = manifest.files.iter().filter(|r| r.each == each).collect();
        if rules.is_empty() {
            continue;
        }
        for bundle in list {
            let mut c = global.clone();
            for (k, v) in bundle.as_object().unwrap() {
                c[k] = v.clone();
            }
            if each == Each::Model {
                // Shortcut to the bundle's single model, e.g. for `{model.classname}` in paths.
                c["model"] = bundle["models"][0]["model"].clone();
            }
            for rule in &rules {
                if rule.applies(&c) {
                    out.write(&rule.path(&c)?, templates.render(&rule.template, &c)?)?;
                }
            }
        }
    }

    // Once-files see all models and APIs; moved, not cloned, as they can be large.
    let all_models: Vec<Value> = models
        .into_iter()
        .map(|mut b| b["models"][0].take())
        .collect();
    global["models"] = Value::Array(all_models);
    global["apiInfo"] = serde_json::json!({ "apis": apis });
    if let Value::Object(m) = extra {
        for (k, v) in m {
            global[&k] = v;
        }
    }
    let once = global;
    for rule in manifest.files.iter().filter(|r| r.each == Each::Once) {
        if rule.applies(&once) {
            out.write(&rule.path(&once)?, templates.render(&rule.template, &once)?)?;
        }
    }

    // Bookkeeping files of openapi-generator, kept so the output stays drop-in compatible.
    let mut files = std::mem::take(&mut out.files);
    files.sort();
    files.dedup();
    out.write(
        ".openapi-generator/VERSION",
        Rendered::Text("7.8.0\n".into()),
    )?;
    let listed: Vec<&String> = files.iter().filter(|f| !manifest.unlisted(f)).collect();
    let list = listed
        .iter()
        .map(|f| f.as_str())
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    out.write(".openapi-generator/FILES", Rendered::Text(list))?;
    files.extend(out.files);
    Ok(files)
}

/// Writes generated files below `root` and remembers their relative paths.
struct Output {
    root: PathBuf,
    files: Vec<String>,
}

impl Output {
    fn write(&mut self, rel: &str, content: Rendered) -> Result<(), String> {
        let path = self.root.join(rel);
        write(&path, &content)?;
        self.files.push(rel.to_string());
        Ok(())
    }
}

fn write(path: &Path, content: &Rendered) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    let bytes = match content {
        Rendered::Text(t) => t.as_bytes(),
        Rendered::Bytes(b) => b,
    };
    std::fs::write(path, bytes).map_err(|e| format!("{}: {e}", path.display()))
}

/// Development aid: writes all contexts as JSON (set `OAS2CLIENT_DUMP_CONTEXTS=<dir>`).
fn dump_contexts(dir: &Path, ctx: &lang::Contexts) -> Result<(), String> {
    let put = |name: String, v: &Value| {
        write(
            &dir.join(name),
            &Rendered::Text(serde_json::to_string_pretty(v).unwrap()),
        )
    };
    put("global.json".into(), &ctx.global)?;
    put("once.json".into(), &ctx.once)?;
    for (i, m) in ctx.models.iter().enumerate() {
        put(
            format!(
                "model_{}.json",
                m.pointer("/model/classname")
                    .and_then(Value::as_str)
                    .map_or(i.to_string(), String::from)
            ),
            m,
        )?;
    }
    for (i, a) in ctx.apis.iter().enumerate() {
        put(
            format!(
                "api_{}.json",
                a.pointer("/operations/classname")
                    .and_then(Value::as_str)
                    .map_or(i.to_string(), String::from)
            ),
            a,
        )?;
    }
    Ok(())
}
