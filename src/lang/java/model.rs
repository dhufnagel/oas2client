//! CodegenModel / CodegenProperty contexts for Java models.

use super::naming::{
    capitalize_accessor, enum_value, enum_var_name, escape_text, snake_upper, var_name,
};
use super::{JType, Java, qualified};
use crate::core::{EnumDef, Model, ModelKind, Prop, Spec, Ty, TyKind};
use crate::lang::common_enum_prefix;
use crate::naming::camelize;
use serde_json::{Value, json};
use std::collections::{BTreeSet, HashMap};

/// `allowableValues` of every enum model, for properties referring to one.
pub(crate) struct Enums(pub HashMap<String, Value>);

impl Enums {
    pub fn new(spec: &Spec) -> Self {
        let mut map = HashMap::new();
        for name in spec.model_names() {
            if let ModelKind::Enum(e) = spec.model(name).kind {
                let jt = JType::of(&e.ty);
                map.insert(name.to_string(), allowable_values(&e, &jt.datatype));
            }
        }
        Enums(map)
    }
}

/// Imports every gson model class needs.
const GSON_POJO_IMPORTS: &[&str] = &[
    "Arrays",
    "IOException",
    "JsonAdapter",
    "JsonReader",
    "JsonWriter",
    "SerializedName",
    "TypeAdapter",
];

pub fn models(java: &Java, spec: &Spec, enums: &Enums) -> Result<Vec<Value>, String> {
    let mut out = Vec::new();
    // Schemas only used as form bodies get no model (skipFormModel).
    for name in spec
        .model_names()
        .into_iter()
        .filter(|n| !spec.form_only.contains(*n))
    {
        let m = spec.model(name);
        let (model, imports) = model(java, &m, enums);
        let imports: BTreeSet<String> = imports
            .iter()
            .map(|i| qualified(i, &java.model_package))
            .collect();
        out.push(json!({
            "package": java.model_package,
            "classname": m.name,
            "imports": imports.iter().map(|i| json!({ "import": i })).collect::<Vec<_>>(),
            "models": [{ "model": model, "importPath": format!("{}.{}", java.model_package, m.name) }],
        }));
    }
    Ok(out)
}

/// The model context and the simple names it imports.
fn model(java: &Java, m: &Model, enums: &Enums) -> (Value, BTreeSet<String>) {
    let description = m.description.map(escape_text);
    let mut imports = BTreeSet::new();
    let base = json!({
        "name": m.name,
        "schemaName": m.name,
        "classname": m.name,
        "classVarName": var_name(&m.name),
        "classFilename": m.name,
        "description": description,
        "unescapedDescription": m.description,
        "vendorExtensions": { "x-implements": [] },
        "oneOf": [],
        "anyOf": [],
        "allOf": [],
    });
    let (props, discriminator) = match &m.kind {
        ModelKind::Enum(e) => {
            imports.insert("SerializedName".to_string());
            let jt = JType::of(&e.ty);
            let mut v = base;
            let flags = type_flags(&jt, enums);
            for k in [
                "isString",
                "isInteger",
                "isLong",
                "isNumber",
                "isFloat",
                "isDouble",
                "isUri",
            ] {
                v[k] = flags[k].clone();
            }
            merge(
                &mut v,
                json!({
                    "isEnum": true,
                    "dataType": jt.datatype,
                    "allowableValues": allowable_values(e, &jt.datatype),
                    "hasVars": false,
                    "vars": [],
                    "allVars": [],
                }),
            );
            return (v, imports);
        }
        ModelKind::Object {
            props,
            discriminator,
        } => (props, discriminator),
    };

    imports.extend(GSON_POJO_IMPORTS.iter().map(|s| s.to_string()));
    let mut vars = Vec::new();
    for p in props {
        let jt = JType::of(&p.ty);
        let mut names = Vec::new();
        jt.import_names(&mut names);
        match &p.ty.kind {
            TyKind::Array(_) if p.ty.unique => names.push("LinkedHashSet".into()),
            TyKind::Array(_) => names.push("ArrayList".into()),
            TyKind::Map(_) => names.push("HashMap".into()),
            _ => {}
        }
        imports.extend(names.into_iter().filter(|n| n != &m.name));
        let mut v = property(java, p, &jt, enums);
        if discriminator
            .as_ref()
            .is_some_and(|d| d.property == p.base_name)
        {
            v["isDiscriminator"] = json!(true);
        }
        vars.push(v);
    }
    let required: Vec<Value> = vars
        .iter()
        .filter(|v| v["required"] == true)
        .cloned()
        .collect();
    let optional: Vec<Value> = vars
        .iter()
        .filter(|v| v["required"] != true)
        .cloned()
        .collect();
    let read_only: Vec<Value> = vars
        .iter()
        .filter(|v| v["isReadOnly"] == true)
        .cloned()
        .collect();
    let has_enums = vars
        .iter()
        .any(|v| v["isEnum"] == true || v["isEnumRef"] == true);
    // openApiNullable: optional nullable properties need JsonNullable helpers.
    let nullable_helpers = vars
        .iter()
        .any(|v| v["required"] == false && v["isNullable"] == true);
    if nullable_helpers {
        imports.insert("JsonNullable".to_string());
    }

    let mut v = base;
    merge(
        &mut v,
        json!({
            "isEnum": false,
            "isModel": true,
            "dataType": "Object",
            "hasVars": !vars.is_empty(),
            "hasEnums": has_enums,
            "hasRequired": !required.is_empty(),
            "hasOptional": !optional.is_empty(),
            "hasReadOnly": !read_only.is_empty(),
            "requiredVars": required,
            "optionalVars": optional,
            "readOnlyVars": read_only,
            "readWriteVars": vars.iter().filter(|v| v["isReadOnly"] != true).cloned().collect::<Vec<_>>(),
            "allVars": vars.clone(),
            "vars": vars,
            "discriminatorName": discriminator.as_ref().map(|d| var_name(&d.property)),
            "discriminator": discriminator.as_ref().map(|d| json!({
                "propertyName": var_name(&d.property),
                "propertyBaseName": d.property,
                "propertyGetter": format!("get{}", capitalize_accessor(&var_name(&d.property))),
                "propertyType": props.iter().find(|p| p.base_name == d.property).map(|p| JType::of(&p.ty).datatype),
                // Only subtypes that extend this model via allOf are mapped; inheritance is not supported.
                "mappedModels": [],
            })),
            "hasChildren": false,
            "vendorExtensions": { "x-implements": [], "x-jackson-optional-nullable-helpers": nullable_helpers },
            "parent": null,
            "isAdditionalPropertiesTrue": m.schema.get("additionalProperties") == Some(&Value::Bool(true)),
        }),
    );
    (v, imports)
}

fn property(_java: &Java, p: &Prop, jt: &JType, enums: &Enums) -> Value {
    let name = var_name(&p.base_name);
    let accessor = capitalize_accessor(&name);
    let mut v = json!({
        "baseName": p.base_name,
        "name": name,
        "getter": format!("get{accessor}"),
        "setter": format!("set{accessor}"),
        "nameInCamelCase": camelize(&name, true),
        "nameInPascalCase": camelize(&name, false),
        "nameInSnakeCase": snake_upper(&name),
        "description": p.description.map(escape_text),
        "unescapedDescription": p.description,
        "required": p.required,
        "isNullable": p.ty.nullable,
        "notRequiredOrIsNullable": !p.required || p.ty.nullable,
        "isReadOnly": p.read_only,
        "deprecated": p.deprecated,
        "isDeprecated": p.deprecated,
        "vendorExtensions": {},
    });
    merge(&mut v, type_flags(jt, enums));
    for key in ["minimum", "maximum"] {
        v[key] = json!(p.schema.get(key).map(Value::to_string));
    }
    if let Some(e) = &p.inline_enum {
        let enum_name = format!("{}Enum", camelize(&name, false));
        let base = match &p.ty.kind {
            TyKind::Ref(_) => "String".to_string(),
            _ => jt.datatype.clone(),
        };
        merge(
            &mut v,
            json!({
                "isEnum": true,
                "isInnerEnum": true,
                "enumName": enum_name,
                "datatypeWithEnum": enum_name,
                "isString": base == "String",
                "allowableValues": allowable_values(e, &base),
                "_enum": e.values,
            }),
        );
    }
    v
}

/// The type-dependent fields shared by properties, parameters and items.
pub(crate) fn type_flags(jt: &JType, enums: &Enums) -> Value {
    let t = &jt.ty;
    let is_ref = matches!(t.kind, TyKind::Ref(_));
    let complex = match &jt.items {
        Some(i) if i.items.is_some() => Some(i.base.clone()),
        Some(i) => i.complex_type(),
        None => jt.complex_type(),
    };
    let mut v = json!({
        "dataType": jt.datatype,
        "datatypeWithEnum": jt.datatype,
        "baseType": jt.base_type(),
        "complexType": complex,
        "openApiType": open_api_type(t),
        "isContainer": t.is_array() || t.is_map(),
        "isArray": t.is_array(),
        "isMap": t.is_map(),
        "uniqueItems": t.unique,
        "isModel": t.is_model(),
        "isEnumRef": t.enum_ref,
        "isEnumOrRef": t.enum_ref,
        "isPrimitiveType": jt.is_primitive(),
        "isString": t.is_string() && !t.is_date() && !t.is_date_time() && !t.is_binary() && t.format() != "byte",
        "isUuid": t.is_string() && t.format() == "uuid",
        "isUri": t.is_string() && t.format() == "uri",
        "isDate": t.is_date(),
        "isDateTime": t.is_date_time(),
        "isBinary": t.is_binary(),
        "isFile": t.is_binary(),
        "isByteArray": t.is_string() && t.format() == "byte",
        "isInteger": t.kind == TyKind::Integer && t.format() != "int64",
        "isLong": t.kind == TyKind::Integer && t.format() == "int64",
        "isNumber": t.kind == TyKind::Number && !matches!(t.format(), "float" | "double"),
        "isFloat": t.kind == TyKind::Number && t.format() == "float",
        "isDouble": t.kind == TyKind::Number && t.format() == "double",
        "isNumeric": matches!(t.kind, TyKind::Integer | TyKind::Number),
        "isBoolean": t.kind == TyKind::Boolean,
        "isFreeFormObject": matches!(t.kind, TyKind::Object | TyKind::Map(_)),
        "isAnyType": t.kind == TyKind::Any,
        "isNullable": t.nullable,
        "isEnum": false,
        "defaultValue": jt.default_value(),
        "allowableValues": match &t.kind {
            TyKind::Ref(n) if t.enum_ref => enums.0.get(n).cloned(),
            _ => None,
        },
        "items": jt.items.as_ref().map(|i| type_flags(i, enums)),
        "mostInnerItems": jt.items.as_ref().map(|_| type_flags(jt.most_inner(), enums)),
    });
    if is_ref && t.enum_ref {
        v["isEnumOrRef"] = json!(true);
    }
    v
}

fn open_api_type(t: &Ty) -> String {
    match &t.kind {
        TyKind::String => "string".into(),
        TyKind::Integer => "integer".into(),
        TyKind::Number => "number".into(),
        TyKind::Boolean => "boolean".into(),
        TyKind::Array(_) => "array".into(),
        TyKind::Map(_) | TyKind::Object => "object".into(),
        TyKind::Ref(n) => n.clone(),
        TyKind::Any => "Object".into(),
    }
}

impl JType {
    /// `complexType`: the referenced class for models, the Java type for non-primitive scalars.
    fn complex_type(&self) -> Option<String> {
        if self.items.is_some() || self.is_primitive() {
            None
        } else {
            Some(self.datatype.clone())
        }
    }

    fn base_type(&self) -> String {
        self.base.clone()
    }
}

pub(crate) fn allowable_values(e: &EnumDef, datatype: &str) -> Value {
    let strs: Vec<&str> = e.values.iter().filter_map(Value::as_str).collect();
    let prefix_len = if strs.len() == e.values.len() {
        common_enum_prefix(&strs).len()
    } else {
        0
    };
    let vars: Vec<Value> = e
        .values
        .iter()
        .map(|v| {
            let raw = v.as_str().map(str::to_string).unwrap_or_else(|| v.to_string());
            let short = if raw.len() > prefix_len { &raw[prefix_len..] } else { &raw };
            json!({ "name": enum_var_name(short, datatype), "isString": datatype == "String", "value": enum_value(v, datatype) })
        })
        .collect();
    json!({ "values": e.values, "enumVars": vars })
}

fn merge(target: &mut Value, extra: Value) {
    if let Value::Object(m) = extra {
        for (k, v) in m {
            target[k] = v;
        }
    }
}
