//! MiniJinja environment for `.jinja` templates of a template set.

use crate::naming;
use minijinja::Environment;
use minijinja::syntax::SyntaxConfig;
use minijinja::value::{Serde, Value as JValue};
use serde_json::Value;
use std::collections::HashMap;

pub fn env(sources: HashMap<String, String>) -> Environment<'static> {
    let mut env = Environment::new();
    // Block tags on their own line vanish completely, like standalone tags in Mustache.
    let syntax = SyntaxConfig::builder()
        .trim_blocks(true)
        .lstrip_blocks(true)
        .keep_trailing_newline(true)
        .build()
        .expect("valid syntax config");
    env.set_syntax(syntax);
    env.set_loader(move |name| Ok(sources.get(name).cloned()));
    // Continue a doc comment on following lines: " * " for indent 1, "     * " for 5.
    env.add_filter("star_indent", |text: String, indent: usize| {
        super::indented(&text, &format!("{}* ", " ".repeat(indent)))
    });
    env.add_filter("indented", |text: String, indent: String| {
        super::indented(&text, &indent)
    });
    // HTML escaping exactly like {{var}} in (j)mustache.
    env.add_filter("html", |t: String| {
        let mut out = String::with_capacity(t.len());
        super::mustache::html_escape(&t, &mut out);
        out
    });
    env.add_filter("snakecase", |t: String| naming::underscore(&t));
    env.add_filter("kebabcase", |t: String| naming::kebab(&t));
    env.add_filter("camelcase", |t: String| naming::camelize(&t, true));
    env.add_filter("pascalcase", |t: String| naming::camelize(&t, false));
    env
}

pub fn render(env: &Environment, name: &str, ctx: &Value) -> Result<String, String> {
    env.get_template(name)
        .and_then(|t| t.render(JValue::from(Serde(ctx))))
        .map_err(|e| format!("{name}: {e:#}"))
}
