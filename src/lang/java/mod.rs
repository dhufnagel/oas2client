//! Java with the okhttp-gson library: type mapping, naming, imports and contexts
//! in the shape of openapi-generator's `JavaClientCodegen`.
//!
//! Options (all optional): `invokerPackage`, `modelPackage`, `apiPackage`, `groupId`,
//! `artifactId`, `artifactVersion`, `sourceFolder`.

mod api;
mod model;
mod naming;

use super::{Contexts, Language, opt_str};
use crate::core::{Spec, Ty, TyKind};
use serde_json::{Map, Value, json};

pub struct Java {
    options: Map<String, Value>,
    invoker_package: String,
    model_package: String,
    api_package: String,
}

impl Java {
    pub fn new(options: Map<String, Value>) -> Result<Self, String> {
        let invoker_package = opt_str(&options, "invokerPackage", "org.openapitools.client");
        let model_package = opt_str(
            &options,
            "modelPackage",
            &format!("{invoker_package}.model"),
        );
        let api_package = opt_str(&options, "apiPackage", &format!("{invoker_package}.api"));
        Ok(Java {
            options,
            invoker_package,
            model_package,
            api_package,
        })
    }

    fn opt(&self, key: &str, default: &str) -> String {
        opt_str(&self.options, key, default)
    }
}

/// A core type mapped to Java.
#[derive(Debug, Clone)]
pub(crate) struct JType {
    /// Declared type: `List<UUID>`, `Map<String, Pet>`, `BigDecimal`.
    pub datatype: String,
    /// Simple name to import for the outer type (`List`, `BigDecimal`, a model name).
    pub base: String,
    pub ty: Ty,
    pub items: Option<Box<JType>>,
}

/// Simple type names that are always in scope (java.lang) or primitive.
const NO_IMPORT: &[&str] = &[
    "String", "Integer", "Long", "Float", "Double", "Boolean", "Object", "byte[]",
];

impl JType {
    pub fn of(t: &Ty) -> JType {
        let simple = |s: &str| (s.to_string(), s.to_string(), None);
        let (datatype, base, items) = match &t.kind {
            TyKind::String => simple(match t.format() {
                "date" => "LocalDate",
                "date-time" => "OffsetDateTime",
                "uuid" => "UUID",
                "uri" => "URI",
                "binary" => "File",
                "byte" => "byte[]",
                _ => "String",
            }),
            TyKind::Integer => simple(if t.format() == "int64" {
                "Long"
            } else {
                "Integer"
            }),
            TyKind::Number => simple(match t.format() {
                "float" => "Float",
                "double" => "Double",
                _ => "BigDecimal",
            }),
            TyKind::Boolean => simple("Boolean"),
            TyKind::Array(inner) => {
                let i = JType::of(inner);
                let outer = if t.unique { "Set" } else { "List" };
                (
                    format!("{outer}<{}>", i.datatype),
                    outer.to_string(),
                    Some(Box::new(i)),
                )
            }
            TyKind::Map(inner) => {
                let i = JType::of(inner);
                (
                    format!("Map<String, {}>", i.datatype),
                    "Map".to_string(),
                    Some(Box::new(i)),
                )
            }
            TyKind::Ref(name) => simple(name),
            TyKind::Object | TyKind::Any => simple("Object"),
        };
        JType {
            datatype,
            base,
            ty: t.clone(),
            items,
        }
    }

    /// The innermost element type (through nested arrays and maps).
    pub fn most_inner(&self) -> &JType {
        match &self.items {
            Some(i) => i.most_inner(),
            None => self,
        }
    }

    /// `isPrimitiveType` of openapi-generator's Java generator: language primitives and
    /// containers of them.
    pub fn is_primitive(&self) -> bool {
        match &self.items {
            Some(i) => i.items.is_none() && i.is_primitive(),
            None => NO_IMPORT.contains(&self.datatype.as_str()),
        }
    }

    /// Simple names this type needs imported (outer type, then element types).
    pub fn import_names(&self, out: &mut Vec<String>) {
        if !NO_IMPORT.contains(&self.base.as_str()) {
            out.push(self.base.clone());
        }
        if let Some(i) = &self.items {
            i.import_names(out);
        }
    }

    pub fn default_value(&self) -> Option<String> {
        match &self.ty.kind {
            // nullable containers start as null
            TyKind::Array(_) | TyKind::Map(_) if self.ty.nullable => None,
            TyKind::Array(_) if self.ty.unique => Some("new LinkedHashSet<>()".into()),
            TyKind::Array(_) => Some("new ArrayList<>()".into()),
            TyKind::Map(_) => Some("new HashMap<>()".into()),
            TyKind::Any => Some("null".into()),
            _ => None,
        }
    }
}

/// Fully qualified import of a simple type name; model names map into the model package.
pub(crate) fn qualified(name: &str, model_package: &str) -> String {
    let fixed = match name {
        "BigDecimal" => "java.math.BigDecimal",
        "LocalDate" => "java.time.LocalDate",
        "OffsetDateTime" => "java.time.OffsetDateTime",
        "UUID" => "java.util.UUID",
        "URI" => "java.net.URI",
        "File" => "java.io.File",
        "List" => "java.util.List",
        "ArrayList" => "java.util.ArrayList",
        "Arrays" => "java.util.Arrays",
        "Map" => "java.util.Map",
        "HashMap" => "java.util.HashMap",
        "Set" => "java.util.Set",
        "LinkedHashSet" => "java.util.LinkedHashSet",
        "IOException" => "java.io.IOException",
        "TypeAdapter" => "com.google.gson.TypeAdapter",
        "JsonAdapter" => "com.google.gson.annotations.JsonAdapter",
        "SerializedName" => "com.google.gson.annotations.SerializedName",
        "JsonReader" => "com.google.gson.stream.JsonReader",
        "JsonWriter" => "com.google.gson.stream.JsonWriter",
        "JsonElement" => "com.google.gson.JsonElement",
        "JsonNullable" => "org.openapitools.jackson.nullable.JsonNullable",
        // openapi-generator's default importMapping, applied even to a model of that name
        "JsonValue" => "com.fasterxml.jackson.annotation.JsonValue",
        other if other.contains('.') => other,
        other => return format!("{model_package}.{other}"),
    };
    fixed.to_string()
}

impl Language for Java {
    fn contexts(&self, spec: &Spec) -> Result<Contexts, String> {
        let global = self.global(spec);
        let enums = model::Enums::new(spec);
        let models = model::models(self, spec, &enums)?;
        let apis = api::apis(self, spec, &enums)?;
        let once = json!({});
        Ok(Contexts {
            global,
            models,
            apis,
            once,
        })
    }
}

impl Java {
    fn global(&self, spec: &Spec) -> Value {
        let info = &spec.root["info"];
        let description = info.get("description").and_then(Value::as_str).unwrap_or(
            "No description provided (generated by Openapi Generator https://github.com/openapitools/openapi-generator)",
        );
        let version = info["version"].as_str().unwrap_or("1.0.0");
        let servers: Vec<Value> = spec
            .root
            .get("servers")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .map(|s| json!({ "url": s["url"], "description": s.get("description"), "variables": [], "vendorExtensions": {} }))
            .collect();
        let base_path = servers
            .first()
            .and_then(|s| s["url"].as_str())
            .unwrap_or("http://localhost")
            .to_string();
        let auth_methods = api::auth_methods(spec);
        let has_bearer = auth_methods.iter().any(|a| a["isBasicBearer"] == true);
        let folder = |p: &str| p.replace('.', "/");
        let source_folder = self.opt("sourceFolder", "src/main/java");
        json!({
            "appName": info["title"],
            "appDescription": description,
            "appDescriptionWithNewLines": description,
            "unescapedAppDescription": description,
            "appVersion": version,
            "version": version,
            "infoEmail": info.pointer("/contact/email"),
            "infoUrl": info.pointer("/contact/url"),
            "licenseInfo": info.pointer("/license/name"),
            "generatorClass": "org.openapitools.codegen.languages.JavaClientCodegen",
            "generatorVersion": "7.8.0",
            "hideGenerationTimestamp": true,
            "invokerPackage": self.invoker_package,
            "modelPackage": self.model_package,
            "apiPackage": self.api_package,
            "invokerFolder": format!("{source_folder}/{}", folder(&self.invoker_package)),
            "modelFolder": format!("{source_folder}/{}", folder(&self.model_package)),
            "apiFolder": folder(&self.api_package),
            "apiSourceFolder": format!("{source_folder}/{}", folder(&self.api_package)),
            "modelTestFolder": format!("src/test/java/{}", folder(&self.model_package)),
            "apiTestFolder": format!("src/test/java/{}", folder(&self.api_package)),
            "groupId": self.opt("groupId", "org.openapitools"),
            "artifactId": self.opt("artifactId", "openapi-java-client"),
            "artifactVersion": self.opt("artifactVersion", version),
            "artifactUrl": "https://github.com/openapitools/openapi-generator",
            "artifactDescription": "OpenAPI Java",
            "developerName": "OpenAPI-Generator Contributors",
            "developerEmail": "team@openapitools.org",
            "developerOrganization": "OpenAPITools.org",
            "developerOrganizationUrl": "http://openapitools.org",
            "scmConnection": "scm:git:git@github.com:openapitools/openapi-generator.git",
            "scmDeveloperConnection": "scm:git:git@github.com:openapitools/openapi-generator.git",
            "scmUrl": "https://github.com/openapitools/openapi-generator",
            "gitHost": "github.com",
            "gitUserId": "GIT_USER_ID",
            "gitRepoId": "GIT_REPO_ID",
            "releaseNote": "Minor update",
            "library": "okhttp-gson",
            "gson": "true",
            "java8": "true",
            "jsr310": "true",
            "doNotUseRx": true,
            "javaxPackage": "javax",
            "rootJavaEEPackage": "javax",
            "annotationLibrary": "none",
            "documentationProvider": "source",
            "sourceDocumentationProvider": true,
            "withoutAnnotationLibrary": true,
            "modelDocPath": "docs/",
            "apiDocPath": "docs/",
            "servers": servers,
            "hasServers": !servers.is_empty(),
            "basePath": base_path,
            "authMethods": auth_methods,
            "hasAuthMethods": !auth_methods.is_empty(),
            "httpBearerMethods": auth_methods.iter().filter(|a| a["isBasicBearer"] == true).cloned().collect::<Vec<_>>(),
            "hasHttpBearerMethods": has_bearer,
            "hasFormParamsInSpec": api::has_form_params(spec),
            "licenseName": info.pointer("/license/name").and_then(Value::as_str).unwrap_or("Unlicense"),
            "licenseUrl": info.pointer("/license/url").and_then(Value::as_str).unwrap_or("http://unlicense.org"),
            "sourceFolder": source_folder,
            "testFolder": "src/test/java",
            "openApiNullable": true,
            "serializableModel": false,
            "serializationLibrary": "gson",
            "booleanGetterPrefix": "get",
            "dateLibrary": "java8",
            "discriminatorCaseSensitive": true,
            "useOneOfDiscriminatorLookup": false,
            "useBeanValidation": false,
            "generateBuilders": false,
            "jackson": false,
            "scheme": base_path.split("://").next(),
            "contextPath": "",
            "basePathWithoutHost": "",
            "hasOperationServers": false,
            // openapi-generator re-serializes its normalized spec here (with generated
            // examples); oas2client writes the input spec as YAML.
            "openapi-yaml": crate::core::yaml::to_yaml(spec.root),
            "generateApiTests": true,
            "generateModelTests": true,
            "generateApiDocs": true,
            "generateModelDocs": true,
        })
    }
}
