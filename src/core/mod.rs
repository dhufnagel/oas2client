//! Core layer: loads and validates an OpenAPI 3.0/3.1 spec and resolves it into a
//! language-neutral model. Nothing here knows about a target language.

mod dupcheck;
mod spec;
mod types;
pub mod yaml;

pub use spec::{
    Body, EnumDef, Model, ModelKind, Operation, Param, ParamIn, Prop, Response, Security, Spec,
};
pub use types::{Ty, TyKind};

/// Reads, parses and validates a spec the way openapi-generator would reject it.
pub fn load(raw: &str) -> Result<serde_json::Value, String> {
    dupcheck::check(raw).map_err(|e| format!("invalid spec: {e}"))?;
    serde_json::from_str(raw).map_err(|e| format!("invalid spec: {e}"))
}
