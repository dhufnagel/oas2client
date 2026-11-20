//! CodegenOperation / CodegenParameter contexts for Java API classes.

use super::model::{Enums, type_flags};
use super::naming::{escape_text, snake_upper, var_name};
use super::{JType, Java, qualified};
use crate::core::{Operation, Param, ParamIn, Spec, TyKind};
use crate::lang::typescript::sanitize_tag;
use crate::naming::{camelize, underscore};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

/// One context per tag, sorted by API class name.
pub fn apis(java: &Java, spec: &Spec, enums: &Enums) -> Result<Vec<Value>, String> {
    let auth = auth_methods(spec);
    let mut by_tag: BTreeMap<String, Vec<(String, Value)>> = BTreeMap::new();
    for op in spec.operations()? {
        let ctx = operation(spec, &op, enums, &auth)?;
        by_tag
            .entry(sanitize_tag(&op.tag))
            .or_default()
            .push((op.id.clone(), ctx));
    }
    let mut out = Vec::new();
    for (tag, mut ops) in by_tag {
        ops.sort_by(|a, b| a.0.cmp(&b.0));
        let classname = format!("{tag}Api");
        let mut imports = BTreeSet::new();
        for (_, op) in &ops {
            for i in op["imports"].as_array().unwrap() {
                imports.insert(i.as_str().unwrap().to_string());
            }
        }
        let imports: Vec<Value> = imports
            .iter()
            .map(|c| json!({ "import": qualified(c, &java.model_package), "classname": c }))
            .collect();
        out.push(json!({
            "package": java.api_package,
            "classname": classname,
            "classVarName": camelize(&tag, true),
            "classFilename": classname,
            "baseName": tag,
            "importPath": format!("{}.{tag}", java.api_package),
            "hasImport": !imports.is_empty(),
            "hasModel": !imports.is_empty(),
            "imports": imports,
            "operationTagDescription": "",
            "operations": {
                "classname": classname,
                "pathPrefix": tag.to_lowercase(),
                "operation": ops.into_iter().map(|(_, o)| o).collect::<Vec<_>>(),
            },
        }));
    }
    Ok(out)
}

fn operation(spec: &Spec, op: &Operation, enums: &Enums, auth: &[Value]) -> Result<Value, String> {
    let mut imports = BTreeSet::new();
    let mut all = Vec::new();
    for p in &op.params {
        let v = param(spec, p, enums);
        if let Some(names) = v["_imports"].as_array() {
            imports.extend(names.iter().filter_map(Value::as_str).map(String::from));
        }
        all.push(v);
    }
    for v in &mut all {
        v.as_object_mut().unwrap().remove("_imports");
    }
    let of =
        |flag: &str| -> Vec<Value> { all.iter().filter(|p| p[flag] == true).cloned().collect() };
    let (path, query, header, cookie, form) = (
        of("isPathParam"),
        of("isQueryParam"),
        of("isHeaderParam"),
        of("isCookieParam"),
        of("isFormParam"),
    );
    let body = all.iter().find(|p| p["isBodyParam"] == true).cloned();
    // allParams: required first, otherwise declaration order (sortParamsByRequiredFlag).
    let mut sorted = all.clone();
    sorted.sort_by_key(|p| p["required"] != true);
    let required: Vec<Value> = sorted
        .iter()
        .filter(|p| p["required"] == true)
        .cloned()
        .collect();
    let optional: Vec<Value> = sorted
        .iter()
        .filter(|p| p["required"] != true)
        .cloned()
        .collect();
    let not_nullable: Vec<Value> = sorted
        .iter()
        .filter(|p| p["isNullable"] != true)
        .cloned()
        .collect();
    let required_not_nullable: Vec<Value> = required
        .iter()
        .filter(|p| p["isNullable"] != true)
        .cloned()
        .collect();

    // The method's return type comes from the lowest 2xx response.
    let method_response = op
        .responses
        .iter()
        .filter(|r| r.code.starts_with('2'))
        .min_by_key(|r| r.code.clone());
    let ret = method_response
        .and_then(|r| r.content.as_ref())
        .map(|(_, t, _)| JType::of(t));
    if let Some(r) = &ret {
        let mut names = Vec::new();
        r.import_names(&mut names);
        imports.extend(
            names
                .into_iter()
                .filter(|n| !matches!(n.as_str(), "List" | "Set" | "Map")),
        );
    }
    let responses: Vec<Value> = op
        .responses
        .iter()
        .map(|r| {
            let jt = r.content.as_ref().map(|(_, t, _)| JType::of(t));
            let mut v = match &jt {
                Some(t) => type_flags(t, enums),
                None => json!({ "isPrimitiveType": true }),
            };
            let fields = json!({
                "code": r.code,
                "message": r.description.map(escape_text),
                "dataType": jt.as_ref().map(|t| t.datatype.clone()),
                "baseType": jt.as_ref().map(|t| t.most_inner().datatype.clone()),
                "is2xx": r.code.starts_with('2'),
                "isDefault": method_response.is_some_and(|m| m.code == r.code),
                "headers": [],
            });
            for (k, x) in fields.as_object().unwrap() {
                v[k] = x.clone();
            }
            if let Some(t) = &jt {
                v["isPrimitiveType"] = json!(is_java_lang(t));
            }
            v
        })
        .collect();

    let mut produces = Vec::new();
    for r in &op.responses {
        for media in r
            .raw
            .get("content")
            .and_then(Value::as_object)
            .into_iter()
            .flat_map(|c| c.keys())
        {
            let m = media_type(media);
            if !produces.contains(&m) {
                produces.push(m);
            }
        }
    }
    let consumes: Vec<Value> = op.consumes.iter().map(|m| media_type(m)).collect();

    let auth_methods: Vec<Value> = op
        .security
        .iter()
        .filter_map(|s| auth.iter().find(|a| a["name"] == s.name.as_str()).cloned())
        .collect();

    let ret_type = ret.as_ref().map(|t| t.datatype.clone());
    let return_container = ret.as_ref().and_then(|t| match t.ty.kind {
        TyKind::Array(_) => Some("array"),
        TyKind::Map(_) => Some("map"),
        _ => None,
    });
    Ok(json!({
        "operationId": op.id,
        "operationIdOriginal": op.id,
        "operationIdLowerCase": op.id.to_lowercase(),
        "operationIdCamelCase": camelize(&op.id, false),
        "operationIdSnakeCase": underscore(&op.id),
        "nickname": camelize(&op.id, true),
        "httpMethod": op.method.to_uppercase(),
        "path": op.path,
        "summary": op.summary.map(escape_text),
        "unescapedNotes": op.description,
        "notes": op.description.map(escape_text),
        "isDeprecated": op.deprecated,
        "tags": [{ "name": op.tag }],
        "baseName": sanitize_tag(&op.tag),
        "allParams": sorted,
        "requiredParams": required,
        "optionalParams": optional,
        "notNullableParams": not_nullable,
        "requiredAndNotNullableParams": required_not_nullable,
        "pathParams": path,
        "queryParams": query,
        "headerParams": header,
        "cookieParams": cookie,
        "formParams": form,
        "bodyParam": body,
        "bodyParams": body.iter().cloned().collect::<Vec<_>>(),
        "hasParams": !all.is_empty(),
        "hasRequiredParams": !required.is_empty(),
        "hasOptionalParams": !optional.is_empty(),
        "hasPathParams": !path.is_empty(),
        "hasQueryParams": !query.is_empty(),
        "hasHeaderParams": !header.is_empty(),
        "hasCookieParams": !cookie.is_empty(),
        "hasFormParams": !form.is_empty(),
        "hasBodyParam": body.is_some(),
        "hasConsumes": !consumes.is_empty(),
        "hasProduces": !produces.is_empty(),
        "consumes": if consumes.is_empty() { Value::Null } else { json!(consumes) },
        "produces": if produces.is_empty() { Value::Null } else { json!(produces) },
        "returnType": ret_type,
        "returnBaseType": ret.as_ref().map(|t| t.most_inner().datatype.clone()),
        "returnContainer": return_container,
        "returnSimpleType": ret.is_some() && return_container.is_none(),
        "returnTypeIsPrimitive": ret.as_ref().is_some_and(|t| is_java_lang(t.most_inner())),
        "isArray": ret.as_ref().is_some_and(|t| t.ty.is_array()),
        "isMap": ret.as_ref().is_some_and(|t| t.ty.is_map()),
        "isResponseFile": ret.as_ref().is_some_and(|t| t.datatype == "File"),
        "responses": responses,
        "authMethods": if auth_methods.is_empty() { Value::Null } else { json!(auth_methods) },
        "hasAuthMethods": !auth_methods.is_empty(),
        "vendorExtensions": {},
        "servers": [],
        "imports": imports,
    }))
}

/// Types of java.lang that need no import and count as primitive return types.
fn is_java_lang(t: &JType) -> bool {
    matches!(
        t.datatype.as_str(),
        "String" | "Integer" | "Long" | "Float" | "Double" | "Boolean" | "Object"
    )
}

fn media_type(media: &str) -> Value {
    let m = media.replace("*/", "");
    let is_json = media
        .split(';')
        .next()
        .is_some_and(|base| base == "application/json" || base.ends_with("+json"));
    if is_json {
        json!({ "mediaType": m, "isJson": "true" })
    } else {
        json!({ "mediaType": m })
    }
}

fn param(spec: &Spec, p: &Param, enums: &Enums) -> Value {
    let jt = JType::of(&p.ty);
    let base_name = p.name.clone();
    let name = var_name(&if p.location == ParamIn::Body {
        camelize(&p.name, true)
    } else {
        p.name.clone()
    });
    let mut v = type_flags(&jt, enums);
    let t = &p.ty;
    let scalar =
        jt.items.is_none() && !t.is_ref() && !matches!(t.kind, TyKind::Object | TyKind::Any);
    let mut imports = Vec::new();
    jt.import_names(&mut imports);
    imports.retain(|n| !matches!(n.as_str(), "List" | "Set" | "Map"));
    let default = p.schema.get("default").map(|d| match d {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    });
    let extra = json!({
        "baseName": base_name,
        "paramName": name,
        "nameInLowerCase": name.to_lowercase(),
        "nameInCamelCase": camelize(&name, true),
        "nameInPascalCase": camelize(&name, false),
        "nameInSnakeCase": snake_upper(&name),
        "description": p.description.map(escape_text),
        "unescapedDescription": p.description,
        "required": p.required,
        "isNullable": t.nullable,
        "isPrimitiveType": scalar && !t.enum_ref,
        "baseType": match (&jt.items, &t.kind) {
            (Some(i), _) => Some(i.datatype.clone()),
            // a model's name only for request bodies; other model parameters have none
            (None, TyKind::Ref(n)) if !t.enum_ref && p.location == ParamIn::Body => Some(n.clone()),
            (None, _) if jt.datatype == "File" => Some("File".to_string()),
            _ => None,
        },
        "defaultValue": default,
        "example": example(spec, &jt, &name),
        "collectionFormat": (p.location == ParamIn::Query && t.is_array()).then_some(if p.explode { "multi" } else { "csv" }),
        "isPathParam": p.location == ParamIn::Path,
        "isQueryParam": p.location == ParamIn::Query,
        "isHeaderParam": p.location == ParamIn::Header,
        "isCookieParam": p.location == ParamIn::Cookie,
        "isFormParam": p.location == ParamIn::Form,
        "isBodyParam": p.location == ParamIn::Body,
        "vendorExtensions": {},
        "_imports": imports,
    });
    for (k, x) in extra.as_object().unwrap() {
        v[k] = x.clone();
    }
    v
}

/// `setParameterExampleValue` of the Java generator.
fn example(spec: &Spec, jt: &JType, param_name: &str) -> String {
    match &jt.ty.kind {
        TyKind::Array(_) => "Arrays.asList()".into(),
        TyKind::Map(_) => "new HashMap()".into(),
        TyKind::Ref(name) if jt.ty.enum_ref => {
            let first = spec.schemas[name.as_str()]["enum"]
                .get(0)
                .cloned()
                .unwrap_or(Value::Null);
            match first {
                Value::String(s) => format!("{name}.fromValue(\"{s}\")"),
                other => format!("{name}.fromValue({other})"),
            }
        }
        TyKind::Ref(name) => format!("new {name}()"),
        _ => match jt.datatype.as_str() {
            "UUID" => "UUID.randomUUID()".into(),
            "LocalDate" => "LocalDate.now()".into(),
            "OffsetDateTime" => "OffsetDateTime.now()".into(),
            "File" => "new File(\"/path/to/file\")".into(),
            "Integer" => "56".into(),
            "Long" => "56L".into(),
            "Float" => "3.4F".into(),
            "Double" => "3.4D".into(),
            "BigDecimal" => "new BigDecimal(78)".into(),
            "Boolean" => "true".into(),
            "URI" => "new URI(\"http://example.com\")".into(),
            "Object" => "null".into(),
            _ => format!("\"{param_name}_example\""),
        },
    }
}

/// `authMethods` of the whole spec (CodegenSecurity).
pub fn auth_methods(spec: &Spec) -> Vec<Value> {
    let schemes = spec
        .root
        .pointer("/components/securitySchemes")
        .and_then(Value::as_object);
    schemes
        .into_iter()
        .flatten()
        .map(|(name, s)| {
            let ty = s["type"].as_str().unwrap_or_default();
            let scheme = s["scheme"].as_str().unwrap_or_default();
            json!({
                "name": name,
                "description": s.get("description"),
                "type": ty,
                "scheme": scheme,
                "bearerFormat": s.get("bearerFormat"),
                "isBasic": ty == "http",
                "isBasicBasic": ty == "http" && scheme == "basic",
                "isBasicBearer": ty == "http" && scheme == "bearer",
                "isApiKey": ty == "apiKey",
                "isOAuth": ty == "oauth2",
                "isOpenId": ty == "openIdConnect",
                "isHttpSignature": false,
                "vendorExtensions": {},
            })
        })
        .collect()
}

pub fn has_form_params(spec: &Spec) -> bool {
    spec.operations()
        .map(|ops| ops.iter().any(|o| o.form_schema.is_some()))
        .unwrap_or(false)
}
