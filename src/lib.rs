#![recursion_limit = "256"]
//! oas2client: OpenAPI client generator in three layers.
//!
//! - [`core`]: loads and validates the spec and resolves schemas into a
//!   language-neutral model (types, models, operations).
//! - [`lang`]: one module per target language; turns the core model into template
//!   contexts (type mapping, naming, imports), shaped like openapi-generator's so its
//!   stock templates work unchanged.
//! - [`template`]: template sets of Mustache and/or Jinja templates plus a manifest
//!   saying which files to render.

pub mod core;
pub mod generate;
pub mod lang;
pub mod naming;
pub mod template;
