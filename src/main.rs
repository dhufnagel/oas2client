use serde_json::{Map, Value};
use std::path::PathBuf;
use std::process;

const USAGE: &str =
    "usage: oas2client -i <spec.json> -o <dir> [-g <set>] [-t <templates>] [-p key=value]...

  -g <set>        built-in template set (typescript-fetch, java)
                  or a directory containing a manifest.toml; default typescript-fetch
  -t <dir>        templates overriding the set's templates by name; a .jinja file
                  replaces a .mustache one of the same name and vice versa
  -p key=value    language option, overrides the manifest's [options]

  oas2client render-context <set> <template> <context.json>
                  render one template with a given context (for template development)";

fn die(msg: &str) -> ! {
    eprintln!("oas2client: {msg}");
    process::exit(2)
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("render-context") {
        return render_context(&args[1..]);
    }
    let mut req = oas2client::generate::Request {
        spec: PathBuf::new(),
        output: PathBuf::new(),
        set: "typescript-fetch".into(),
        overrides: None,
        options: Map::new(),
    };
    let (mut has_input, mut has_output) = (false, false);
    let mut it = args.into_iter();
    while let Some(a) = it.next() {
        let mut value = || {
            it.next()
                .unwrap_or_else(|| die(&format!("{a} needs a value\n\n{USAGE}")))
        };
        match a.as_str() {
            "-i" => (req.spec, has_input) = (value().into(), true),
            "-o" => (req.output, has_output) = (value().into(), true),
            "-g" => req.set = value(),
            "-t" => req.overrides = Some(value().into()),
            "-p" => {
                let kv = value();
                let (k, v) = kv
                    .split_once('=')
                    .unwrap_or_else(|| die(&format!("-p {kv}: expected key=value")));
                req.options
                    .insert(k.to_string(), Value::String(v.to_string()));
            }
            "-h" | "--help" => {
                println!("{USAGE}");
                return;
            }
            other => die(&format!("unknown argument {other}\n\n{USAGE}")),
        }
    }
    if !has_input || !has_output {
        die(USAGE);
    }
    oas2client::generate::run(&req).unwrap_or_else(|e| die(&e));
}

fn render_context(args: &[String]) {
    let [set, template, context] = args else {
        die(USAGE)
    };
    let (manifest, source) = oas2client::template::load_manifest(set).unwrap_or_else(|e| die(&e));
    let set = oas2client::template::TemplateSet::load(&source, &manifest.layers, None)
        .unwrap_or_else(|e| die(&e));
    let text = std::fs::read_to_string(context).unwrap_or_else(|e| die(&format!("{context}: {e}")));
    let ctx: Value =
        serde_json::from_str(&text).unwrap_or_else(|e| die(&format!("{context}: {e}")));
    match set.render(template, &ctx).unwrap_or_else(|e| die(&e)) {
        oas2client::template::Rendered::Text(t) => print!("{t}"),
        oas2client::template::Rendered::Bytes(b) => {
            std::io::Write::write_all(&mut std::io::stdout(), &b).unwrap()
        }
    }
}
