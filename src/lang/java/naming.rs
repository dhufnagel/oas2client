//! Java identifiers as `AbstractJavaCodegen` derives them.

use crate::naming::{camelize, underscore};

const RESERVED: &[&str] = &[
    "object",
    "list",
    "file",
    "localvarpath",
    "localvarqueryparams",
    "localvarcollectionqueryparams",
    "localvarheaderparams",
    "localvarcookieparams",
    "localvarformparams",
    "localvarpostbody",
    "localvaraccepts",
    "localvaraccept",
    "localvarcontenttypes",
    "localvarcontenttype",
    "localvarauthnames",
    "localreturntype",
    "apiclient",
    "apiexception",
    "apiresponse",
    "configuration",
    "stringutil",
    "abstract",
    "continue",
    "for",
    "new",
    "switch",
    "assert",
    "default",
    "if",
    "package",
    "synchronized",
    "boolean",
    "do",
    "goto",
    "private",
    "this",
    "break",
    "double",
    "implements",
    "protected",
    "throw",
    "byte",
    "else",
    "import",
    "public",
    "throws",
    "case",
    "enum",
    "instanceof",
    "return",
    "transient",
    "catch",
    "extends",
    "int",
    "short",
    "try",
    "char",
    "final",
    "interface",
    "static",
    "void",
    "class",
    "finally",
    "long",
    "strictfp",
    "volatile",
    "const",
    "float",
    "native",
    "super",
    "while",
    "null",
    "offsetdatetime",
    "localdate",
    "localtime",
];

pub fn is_reserved(word: &str) -> bool {
    RESERVED.contains(&word.to_lowercase().as_str())
}

/// `toVarName` / `toParamName`.
pub fn var_name(name: &str) -> String {
    let mut name: String = name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '_' || c == '$' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if name.to_lowercase().trim_start_matches('_') == "class" {
        return "propertyClass".into();
    }
    if name == "_" {
        name = "_u".into();
    }
    // all upper case: keep as is
    if name
        .chars()
        .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
        && !name.is_empty()
    {
        return name;
    }
    let mut chars = name.chars();
    if let (Some(a), Some(b)) = (chars.next(), chars.next())
        && a.is_uppercase()
        && b.is_uppercase()
    {
        name = format!("{}{}", name[..2].to_lowercase(), &name[2..]);
    }
    let name = camelize(&name, true);
    if is_reserved(&name) || name.starts_with(|c: char| c.is_ascii_digit()) {
        format!("_{name}")
    } else {
        name
    }
}

/// `getterAndSetterCapitalize`: "petId" -> "PetId", but "eTag" stays "eTag".
pub fn capitalize_accessor(name: &str) -> String {
    let mut chars = name.chars();
    match (chars.next(), chars.next()) {
        (Some(_), Some(b)) if b.is_uppercase() => name.to_string(),
        _ => camelize(name, false),
    }
}

/// `toEnumVarName` for string and number enums.
pub fn enum_var_name(value: &str, datatype: &str) -> String {
    if value.is_empty() {
        return "EMPTY".into();
    }
    if matches!(
        datatype,
        "Integer" | "Long" | "Float" | "Double" | "BigDecimal"
    ) {
        return format!("NUMBER_{value}")
            .replace('-', "MINUS_")
            .replace('+', "PLUS_")
            .replace('.', "_DOT_");
    }
    let mut var = String::new();
    let mut in_sep = false;
    for c in value.chars() {
        if c.is_alphanumeric() || c == '_' {
            var.extend(c.to_uppercase());
            in_sep = false;
        } else if !in_sep {
            var.push('_');
            in_sep = true;
        }
    }
    if var.starts_with(|c: char| c.is_ascii_digit()) {
        format!("_{var}")
    } else {
        var
    }
}

/// `toEnumValue`.
pub fn enum_value(value: &serde_json::Value, datatype: &str) -> String {
    match datatype {
        "Integer" | "Double" => value.to_string(),
        "Long" => format!("{value}l"),
        "Float" => format!("{value}f"),
        "BigDecimal" => format!("new BigDecimal(\"{value}\")"),
        _ => format!("\"{}\"", escape_text(value.as_str().unwrap_or_default())),
    }
}

/// `escapeText`: escapes quotes and backslashes and drops line breaks and tabs.
pub fn escape_text(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace(['\n', '\r'], " ")
        .replace('\t', " ")
}

pub fn snake_upper(name: &str) -> String {
    underscore(name).to_uppercase()
}
