//! Spec access and resolution into models and operations.

use super::types::{Ty, TyKind};
use serde_json::{Map, Value};
use std::collections::BTreeSet;

pub struct Spec<'a> {
    pub root: &'a Value,
    pub schemas: &'a Map<String, Value>,
    /// Schemas only used as multipart/form bodies (openapi-generator's `skipFormModel` candidates).
    pub form_only: BTreeSet<String>,
}

pub struct Model<'a> {
    pub name: String,
    pub schema: &'a Value,
    pub description: Option<&'a str>,
    pub kind: ModelKind<'a>,
}

pub enum ModelKind<'a> {
    Enum(EnumDef<'a>),
    Object {
        props: Vec<Prop<'a>>,
        discriminator: Option<Discriminator>,
    },
}

pub struct EnumDef<'a> {
    pub values: &'a [Value],
    /// Type of the values (string, integer, ...).
    pub ty: Ty,
}

pub struct Prop<'a> {
    pub base_name: String,
    pub schema: &'a Value,
    pub ty: Ty,
    pub required: bool,
    pub description: Option<&'a str>,
    /// `enum` next to the type (even next to a `$ref`): an inline enum of this property.
    pub inline_enum: Option<EnumDef<'a>>,
    pub read_only: bool,
    pub deprecated: bool,
}

pub struct Discriminator {
    pub property: String,
    /// (mapping name, model name), sorted by mapping name.
    pub mapping: Vec<(String, String)>,
}

pub struct Operation<'a> {
    pub id: String,
    /// First tag, or "default".
    pub tag: String,
    pub method: &'static str,
    pub path: String,
    pub op: &'a Value,
    pub summary: Option<&'a str>,
    pub description: Option<&'a str>,
    pub deprecated: bool,
    /// Path-item parameters, operation parameters, then the body or form fields.
    pub params: Vec<Param<'a>>,
    /// The request body's media types.
    pub consumes: Vec<String>,
    /// The request body schema when it is a form (multipart or urlencoded).
    pub form_schema: Option<&'a Value>,
    pub responses: Vec<Response<'a>>,
    pub security: Vec<Security<'a>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParamIn {
    Path,
    Query,
    Header,
    Cookie,
    Form,
    Body,
}

pub struct Param<'a> {
    /// Name in the spec; for a body: the referenced schema name, or "body".
    pub name: String,
    pub location: ParamIn,
    pub ty: Ty,
    pub required: bool,
    pub explode: bool,
    pub description: Option<&'a str>,
    pub schema: &'a Value,
    /// The parameter object itself (null for body and form fields).
    pub raw: &'a Value,
}

pub struct Response<'a> {
    pub code: String,
    pub description: Option<&'a str>,
    /// First media type of `content` and its schema's type.
    pub content: Option<(String, Ty, &'a Value)>,
    pub raw: &'a Value,
}

pub struct Security<'a> {
    pub name: String,
    pub scheme: &'a Value,
    pub scopes: &'a Value,
}

pub type Body<'a> = Param<'a>;

const METHODS: &[&str] = &[
    "get", "put", "post", "delete", "options", "head", "patch", "trace",
];
const FORM_MEDIA: &[&str] = &["multipart/form-data", "application/x-www-form-urlencoded"];

static EMPTY_MAP: std::sync::OnceLock<Map<String, Value>> = std::sync::OnceLock::new();
static NULL: Value = Value::Null;

impl<'a> Spec<'a> {
    pub fn new(root: &'a Value) -> Result<Self, String> {
        let schemas = match root.pointer("/components/schemas") {
            Some(Value::Object(m)) => m,
            Some(_) => return Err("components.schemas is not an object".into()),
            None => EMPTY_MAP.get_or_init(Map::new),
        };
        let mut missing = BTreeSet::new();
        collect_refs(root, &mut |r| {
            if !r.starts_with("#/components/schemas/") || !schemas.contains_key(ref_name(r)) {
                missing.insert(r.to_string());
            }
        });
        if !missing.is_empty() {
            return Err(format!(
                "unresolvable $ref: {}",
                missing.into_iter().collect::<Vec<_>>().join(", ")
            ));
        }
        Ok(Spec {
            root,
            schemas,
            form_only: form_only_schemas(root),
        })
    }

    pub fn resolve(&self, s: &'a Value) -> &'a Value {
        match s.get("$ref").and_then(Value::as_str) {
            Some(r) => &self.schemas[ref_name(r)],
            None => s,
        }
    }

    /// Schemas that become their own model (enums and objects with properties).
    /// Primitive schemas such as `LocalDate` or `UUID` and bare `{"type": "object"}`
    /// are inlined at their use sites.
    pub fn is_model(&self, name: &str) -> bool {
        let s = &self.schemas[name];
        s.get("enum").is_some() || s.get("properties").is_some()
    }

    /// Names of all model schemas, sorted.
    pub fn model_names(&self) -> Vec<&'a str> {
        let mut names: Vec<&str> = self
            .schemas
            .keys()
            .map(String::as_str)
            .filter(|n| self.is_model(n))
            .collect();
        names.sort();
        names
    }

    pub fn ty(&self, s: &Value) -> Ty {
        if let Some(r) = s.get("$ref").and_then(Value::as_str) {
            let name = ref_name(r);
            if !self.is_model(name) {
                let mut t = self.ty(&self.schemas[name]);
                t.nullable = false;
                return t;
            }
            let mut t = Ty::new(TyKind::Ref(name.to_string()));
            t.enum_ref = self.schemas[name].get("enum").is_some();
            return t;
        }
        // OAS 3.0 style: `allOf: [{$ref}]` wrapping a reference to add `nullable` etc.
        if let Some([single]) = s.get("allOf").and_then(Value::as_array).map(Vec::as_slice)
            && s.get("properties").is_none()
        {
            let mut t = self.ty(single);
            t.nullable = nullable_flag(s);
            return t;
        }
        if let Some(any_of) = s.get("anyOf").and_then(Value::as_array) {
            let non_null: Vec<&Value> = any_of
                .iter()
                .filter(|m| type_of(m) != Some("null"))
                .collect();
            if non_null.len() == 1 {
                let mut t = self.ty(non_null[0]);
                t.nullable = non_null.len() < any_of.len() && t.is_ref();
                return t;
            }
        }
        let (ty, null_in_type) = match s.get("type") {
            Some(Value::String(t)) => (Some(t.as_str()), false),
            Some(Value::Array(ts)) => {
                let base = ts.iter().filter_map(Value::as_str).find(|t| *t != "null");
                (base, ts.iter().any(|t| t == "null"))
            }
            _ => (None, false),
        };
        let format = s.get("format").and_then(Value::as_str).map(str::to_string);
        let mut t = match ty {
            Some("string") => Ty::new(TyKind::String),
            Some("integer") => Ty::new(TyKind::Integer),
            Some("number") => Ty::new(TyKind::Number),
            Some("boolean") => Ty::new(TyKind::Boolean),
            Some("array") => {
                let mut t = Ty::new(TyKind::Array(Box::new(
                    self.ty(s.get("items").unwrap_or(&NULL)),
                )));
                t.unique = s
                    .get("uniqueItems")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                t
            }
            Some("object") if s.get("additionalProperties").is_some_and(Value::is_object) => {
                Ty::new(TyKind::Map(Box::new(self.ty(&s["additionalProperties"]))))
            }
            Some("object") => Ty::new(TyKind::Object),
            _ => {
                let mut t = Ty::new(TyKind::Any);
                t.nullable = true;
                t
            }
        };
        if matches!(t.kind, TyKind::String | TyKind::Integer | TyKind::Number) {
            t.format = format;
        }
        // OAS 3.1 `type: [x, "null"]` makes everything but arrays nullable;
        // OAS 3.0 `nullable: true` applies to arrays as well.
        if (null_in_type && !t.is_array()) || nullable_flag(s) {
            t.nullable = true;
        }
        t
    }

    pub fn model(&self, name: &str) -> Model<'a> {
        let s = &self.schemas[name];
        let description = s.get("description").and_then(Value::as_str);
        let model_name = name.to_string();
        if let Some(values) = s.get("enum").and_then(Value::as_array) {
            let mut ty = self.ty(s);
            ty.nullable = false;
            return Model {
                name: model_name,
                schema: s,
                description,
                kind: ModelKind::Enum(EnumDef { values, ty }),
            };
        }
        let required: BTreeSet<&str> = s
            .get("required")
            .and_then(Value::as_array)
            .map(|r| r.iter().filter_map(Value::as_str).collect())
            .unwrap_or_default();
        let props = s
            .get("properties")
            .and_then(Value::as_object)
            .into_iter()
            .flatten()
            .map(|(base, ps)| self.prop(base, ps, required.contains(base.as_str())))
            .collect();
        let discriminator = s.get("discriminator").map(|d| {
            let mut mapping: Vec<(String, String)> = d
                .get("mapping")
                .and_then(Value::as_object)
                .into_iter()
                .flatten()
                .map(|(k, v)| {
                    (
                        k.clone(),
                        ref_name(v.as_str().unwrap_or_default()).to_string(),
                    )
                })
                .collect();
            mapping.sort();
            Discriminator {
                property: d["propertyName"].as_str().unwrap_or_default().to_string(),
                mapping,
            }
        });
        Model {
            name: model_name,
            schema: s,
            description,
            kind: ModelKind::Object {
                props,
                discriminator,
            },
        }
    }

    fn prop(&self, base: &str, schema: &'a Value, required: bool) -> Prop<'a> {
        let ty = self.ty(schema);
        let inline_enum = schema
            .get("enum")
            .and_then(Value::as_array)
            .map(|values| EnumDef {
                values,
                ty: ty.clone(),
            });
        Prop {
            base_name: base.to_string(),
            schema,
            ty,
            required,
            description: schema.get("description").and_then(Value::as_str),
            inline_enum,
            read_only: schema
                .get("readOnly")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            deprecated: schema
                .get("deprecated")
                .and_then(Value::as_bool)
                .unwrap_or(false),
        }
    }

    /// All operations in spec order, validated like openapi-generator does.
    pub fn operations(&self) -> Result<Vec<Operation<'a>>, String> {
        let schemes = self
            .root
            .pointer("/components/securitySchemes")
            .and_then(Value::as_object);
        let global_security = self.root.get("security");
        let paths = self.root["paths"].as_object().ok_or("spec has no paths")?;
        let mut seen_ids = BTreeSet::new();
        let mut out = Vec::new();
        for (path, item) in paths {
            for method in METHODS {
                let Some(op) = item.get(*method) else {
                    continue;
                };
                let id = op["operationId"]
                    .as_str()
                    .ok_or_else(|| format!("{method} {path}: no operationId"))?;
                if !seen_ids.insert(id) {
                    return Err(format!("operationId '{id}' is repeated ({method} {path})"));
                }
                let security = op.get("security").or(global_security);
                out.push(self.operation(path, method, id, op, item, security, schemes)?);
            }
        }
        Ok(out)
    }

    #[allow(clippy::too_many_arguments)]
    fn operation(
        &self,
        path: &str,
        method: &'static str,
        id: &str,
        op: &'a Value,
        path_item: &'a Value,
        security: Option<&'a Value>,
        schemes: Option<&'a Map<String, Value>>,
    ) -> Result<Operation<'a>, String> {
        let mut params = Vec::new();
        let declared = path_item
            .get("parameters")
            .and_then(Value::as_array)
            .into_iter()
            .flatten();
        for p in declared.chain(
            op.get("parameters")
                .and_then(Value::as_array)
                .into_iter()
                .flatten(),
        ) {
            let schema = p.get("schema").unwrap_or(&NULL);
            let location = match p["in"].as_str().unwrap_or_default() {
                "path" => ParamIn::Path,
                "query" => ParamIn::Query,
                "header" => ParamIn::Header,
                "cookie" => ParamIn::Cookie,
                other => return Err(format!("{id}: unknown parameter location '{other}'")),
            };
            params.push(Param {
                name: p["name"].as_str().unwrap_or_default().to_string(),
                location,
                ty: self.ty(schema),
                required: p
                    .get("required")
                    .and_then(Value::as_bool)
                    .unwrap_or(location == ParamIn::Path),
                explode: p.get("explode").and_then(Value::as_bool).unwrap_or(true),
                description: p.get("description").and_then(Value::as_str),
                schema,
                raw: p,
            });
        }

        let mut consumes = Vec::new();
        let mut form_schema = None;
        if let Some(rb) = op.get("requestBody") {
            let required = rb.get("required").and_then(Value::as_bool).unwrap_or(false);
            let content = rb["content"]
                .as_object()
                .ok_or_else(|| format!("{id}: requestBody without content"))?;
            consumes = content.keys().cloned().collect();
            let (media, mt) = content
                .iter()
                .next()
                .ok_or_else(|| format!("{id}: empty requestBody"))?;
            let schema = mt.get("schema").unwrap_or(&NULL);
            if FORM_MEDIA.contains(&media.as_str()) {
                let schema = self.resolve(schema);
                form_schema = Some(schema);
                let form_required: BTreeSet<&str> = schema
                    .get("required")
                    .and_then(Value::as_array)
                    .map(|r| r.iter().filter_map(Value::as_str).collect())
                    .unwrap_or_default();
                for (name, ps) in schema
                    .get("properties")
                    .and_then(Value::as_object)
                    .into_iter()
                    .flatten()
                {
                    params.push(Param {
                        name: name.clone(),
                        location: ParamIn::Form,
                        ty: self.ty(ps),
                        required: form_required.contains(name.as_str()),
                        explode: true,
                        description: ps.get("description").and_then(Value::as_str),
                        schema: ps,
                        raw: &NULL,
                    });
                }
            } else {
                let name = match schema.get("$ref").and_then(Value::as_str) {
                    Some(r) => ref_name(r).to_string(),
                    None => "body".to_string(),
                };
                params.push(Param {
                    name,
                    location: ParamIn::Body,
                    ty: self.ty(schema),
                    required,
                    explode: true,
                    description: rb.get("description").and_then(Value::as_str),
                    schema,
                    raw: &NULL,
                });
            }
        }
        for placeholder in path.split('{').skip(1).filter_map(|s| s.split('}').next()) {
            if !params
                .iter()
                .any(|p| p.location == ParamIn::Path && p.name == placeholder)
            {
                return Err(format!(
                    "{path}: path parameter '{placeholder}' is not declared ({id})"
                ));
            }
        }

        let responses = op
            .get("responses")
            .and_then(Value::as_object)
            .into_iter()
            .flatten()
            .map(|(code, r)| Response {
                code: code.clone(),
                description: r.get("description").and_then(Value::as_str),
                content: r
                    .get("content")
                    .and_then(Value::as_object)
                    .and_then(|c| c.iter().next())
                    .map(|(media, m)| {
                        let schema = m.get("schema").unwrap_or(&NULL);
                        (media.clone(), self.ty(schema), schema)
                    }),
                raw: r,
            })
            .collect();

        let mut sec = Vec::new();
        for requirement in security.and_then(Value::as_array).into_iter().flatten() {
            for (name, scopes) in requirement.as_object().into_iter().flatten() {
                let scheme = schemes
                    .and_then(|s| s.get(name))
                    .ok_or_else(|| format!("unknown security scheme {name}"))?;
                sec.push(Security {
                    name: name.clone(),
                    scheme,
                    scopes,
                });
            }
        }

        Ok(Operation {
            id: id.to_string(),
            tag: op
                .pointer("/tags/0")
                .and_then(Value::as_str)
                .unwrap_or("default")
                .to_string(),
            method,
            path: path.to_string(),
            op,
            summary: op.get("summary").and_then(Value::as_str),
            description: op.get("description").and_then(Value::as_str),
            deprecated: op
                .get("deprecated")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            params,
            consumes,
            form_schema,
            responses,
            security: sec,
        })
    }
}

pub fn ref_name(r: &str) -> &str {
    r.rsplit('/').next().unwrap()
}

fn nullable_flag(s: &Value) -> bool {
    s.get("nullable").and_then(Value::as_bool).unwrap_or(false)
}

fn type_of(s: &Value) -> Option<&str> {
    s.get("type").and_then(Value::as_str)
}

fn collect_refs(v: &Value, f: &mut impl FnMut(&str)) {
    match v {
        Value::Object(m) => {
            for (k, x) in m {
                match (k.as_str(), x) {
                    ("$ref", Value::String(r)) => f(r),
                    _ => collect_refs(x, f),
                }
            }
        }
        Value::Array(a) => a.iter().for_each(|x| collect_refs(x, f)),
        _ => {}
    }
}

/// Schemas referenced directly as a form request body and nowhere else.
fn form_only_schemas(root: &Value) -> BTreeSet<String> {
    let mut form_refs = BTreeSet::new();
    let mut paths_without_form_bodies = root.get("paths").cloned().unwrap_or(Value::Null);
    if let Some(paths) = paths_without_form_bodies.as_object_mut() {
        for item in paths.values_mut() {
            for op in item
                .as_object_mut()
                .into_iter()
                .flat_map(|m| m.values_mut())
            {
                let Some(content) = op
                    .pointer_mut("/requestBody/content")
                    .and_then(Value::as_object_mut)
                else {
                    continue;
                };
                for media in FORM_MEDIA {
                    if let Some(r) = content
                        .get(*media)
                        .and_then(|m| m.pointer("/schema/$ref"))
                        .and_then(Value::as_str)
                    {
                        form_refs.insert(ref_name(r).to_string());
                        content.remove(*media);
                    }
                }
            }
        }
    }
    let mut used_elsewhere = BTreeSet::new();
    let mut note = |r: &str| {
        used_elsewhere.insert(ref_name(r).to_string());
    };
    collect_refs(&paths_without_form_bodies, &mut note);
    if let Some(components) = root.get("components") {
        collect_refs(components, &mut note);
    }
    form_refs.retain(|n| !used_elsewhere.contains(n));
    form_refs
}
