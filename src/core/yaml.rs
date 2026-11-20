//! Minimal YAML writer for JSON values (block style, keys in document order).
//! Strings that YAML would read as something else are written double-quoted.

use serde_json::Value;

pub fn to_yaml(v: &Value) -> String {
    let mut out = String::new();
    write(v, 0, &mut out, false);
    out
}

fn write(v: &Value, indent: usize, out: &mut String, inline_first: bool) {
    let pad = " ".repeat(indent);
    match v {
        Value::Object(m) if !m.is_empty() => {
            for (i, (k, x)) in m.iter().enumerate() {
                if i > 0 || !inline_first {
                    out.push_str(&pad);
                }
                out.push_str(&scalar_str(k));
                out.push(':');
                child(x, indent, out);
            }
        }
        Value::Array(a) if !a.is_empty() => {
            for (i, x) in a.iter().enumerate() {
                if i > 0 || !inline_first {
                    out.push_str(&pad);
                }
                out.push_str("- ");
                match x {
                    Value::Object(m) if !m.is_empty() => write(x, indent + 2, out, true),
                    Value::Array(a) if !a.is_empty() => {
                        out.push('\n');
                        write(x, indent + 2, out, false)
                    }
                    _ => {
                        out.push_str(&scalar(x));
                        out.push('\n');
                    }
                }
            }
        }
        _ => {
            out.push_str(&scalar(v));
            out.push('\n');
        }
    }
}

fn child(x: &Value, indent: usize, out: &mut String) {
    match x {
        Value::Object(m) if !m.is_empty() => {
            out.push('\n');
            write(x, indent + 2, out, false);
        }
        Value::Array(a) if !a.is_empty() => {
            out.push('\n');
            write(x, indent, out, false);
        }
        _ => {
            out.push(' ');
            out.push_str(&scalar(x));
            out.push('\n');
        }
    }
}

fn scalar(v: &Value) -> String {
    match v {
        Value::String(s) => scalar_str(s),
        Value::Object(_) => "{}".into(),
        Value::Array(_) => "[]".into(),
        other => other.to_string(),
    }
}

fn scalar_str(s: &str) -> String {
    let plain = !s.is_empty()
        && !s.starts_with(|c: char| {
            " -?:,[]{}#&*!|>'\"%@`~".contains(c) || c.is_ascii_digit() || c == '.'
        })
        && !s.ends_with([' ', ':'])
        && !s.contains(": ")
        && !s.contains(" #")
        && !s.contains(|c: char| c.is_control())
        && !matches!(
            s.to_lowercase().as_str(),
            "true" | "false" | "null" | "yes" | "no" | "on" | "off" | "y" | "n"
        );
    if plain {
        s.to_string()
    } else {
        serde_json::to_string(s).unwrap()
    }
}
