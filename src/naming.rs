//! Name transformations matching openapi-generator's `StringUtils`.

/// `underscore`: "Ti365ConfigResponse" -> "ti365_config_response".
pub fn underscore(word: &str) -> String {
    let c: Vec<char> = word.replace('.', "/").replace('$', "_").chars().collect();
    // ([A-Z]+)([A-Z][a-z][a-z]+) -> $1_$2
    let mut step1: Vec<char> = Vec::with_capacity(c.len() + 8);
    for i in 0..c.len() {
        if i > 0
            && c[i].is_ascii_uppercase()
            && c[i - 1].is_ascii_uppercase()
            && c.get(i + 1).is_some_and(|x| x.is_ascii_lowercase())
            && c.get(i + 2).is_some_and(|x| x.is_ascii_lowercase())
        {
            step1.push('_');
        }
        step1.push(c[i]);
    }
    // ([a-z\d])([A-Z]) -> $1_$2
    let mut out = String::with_capacity(step1.len() + 8);
    for i in 0..step1.len() {
        if i > 0
            && step1[i].is_ascii_uppercase()
            && (step1[i - 1].is_ascii_lowercase() || step1[i - 1].is_ascii_digit())
        {
            out.push('_');
        }
        out.push(step1[i]);
    }
    out.replace('-', "_").to_lowercase()
}

pub fn kebab(word: &str) -> String {
    underscore(word).replace('_', "-")
}

/// `camelize`: "patient_event" -> "PatientEvent" (or "patientEvent" with `lower_first`).
pub fn camelize(word: &str, lower_first: bool) -> String {
    let mut out = String::with_capacity(word.len());
    let mut upper_next = !lower_first;
    for (i, ch) in word.chars().enumerate() {
        if ch == '_' || ch == '-' || ch == ' ' {
            upper_next = true;
            continue;
        }
        if i == 0 && lower_first {
            out.extend(ch.to_lowercase());
            upper_next = false;
        } else if upper_next {
            out.extend(ch.to_uppercase());
            upper_next = false;
        } else {
            out.push(ch);
        }
    }
    out
}

/// TypeScript enum member name with `enumPropertyNaming=PascalCase`.
pub fn enum_var_name(value: &str) -> String {
    let sanitized: String = value
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let trimmed = sanitized.trim_start_matches('_').trim_end_matches('_');
    let name = camelize(&underscore(trimmed), false);
    if name.starts_with(|c: char| c.is_ascii_digit()) {
        format!("_{name}")
    } else {
        name
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_names() {
        assert_eq!(kebab("Ti365ConfigResponse"), "ti365-config-response");
        assert_eq!(kebab("AbsenceDurationData"), "absence-duration-data");
        assert_eq!(kebab("Vk"), "vk");
    }

    #[test]
    fn enum_names() {
        assert_eq!(enum_var_name("PATIENT_EVENT"), "PatientEvent");
        assert_eq!(enum_var_name("MUSTER_13"), "Muster13");
        assert_eq!(enum_var_name("BB"), "Bb");
    }

    #[test]
    fn var_names() {
        assert_eq!(camelize("catalog_section", true), "catalogSection");
        assert_eq!(camelize("firstName", true), "firstName");
    }
}
