//! Language-neutral types of schemas.

#[derive(Debug, Clone, PartialEq)]
pub enum TyKind {
    String,
    Integer,
    Number,
    Boolean,
    Array(Box<Ty>),
    /// `type: object` with `additionalProperties: <schema>`.
    Map(Box<Ty>),
    /// A named model or enum.
    Ref(String),
    /// `type: object` without properties (free-form object).
    Object,
    /// No type at all.
    Any,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Ty {
    pub kind: TyKind,
    /// `format` of string/number types (`date`, `date-time`, `binary`, `int64`, ...).
    pub format: Option<String>,
    pub nullable: bool,
    /// `uniqueItems` of arrays.
    pub unique: bool,
    /// For `Ref`: the target is an enum (not an object model).
    pub enum_ref: bool,
}

impl Ty {
    pub fn new(kind: TyKind) -> Self {
        Ty {
            kind,
            format: None,
            nullable: false,
            unique: false,
            enum_ref: false,
        }
    }

    pub fn format(&self) -> &str {
        self.format.as_deref().unwrap_or("")
    }

    /// Element type of arrays and value type of maps.
    pub fn inner(&self) -> Option<&Ty> {
        match &self.kind {
            TyKind::Array(t) | TyKind::Map(t) => Some(t),
            _ => None,
        }
    }

    /// The model or enum this type refers to, directly or through an array or map.
    pub fn class_ref(&self) -> Option<&str> {
        match &self.kind {
            TyKind::Ref(n) => Some(n),
            TyKind::Array(t) | TyKind::Map(t) => t.class_ref(),
            _ => None,
        }
    }

    pub fn is_ref(&self) -> bool {
        matches!(self.kind, TyKind::Ref(_))
    }
    pub fn is_model(&self) -> bool {
        self.is_ref() && !self.enum_ref
    }
    pub fn is_array(&self) -> bool {
        matches!(self.kind, TyKind::Array(_))
    }
    pub fn is_map(&self) -> bool {
        matches!(self.kind, TyKind::Map(_))
    }
    pub fn is_string(&self) -> bool {
        self.kind == TyKind::String
    }
    pub fn is_date(&self) -> bool {
        self.is_string() && self.format() == "date"
    }
    pub fn is_date_time(&self) -> bool {
        self.is_string() && self.format() == "date-time"
    }
    pub fn is_binary(&self) -> bool {
        self.is_string() && self.format() == "binary"
    }
    pub fn is_free_form(&self) -> bool {
        matches!(self.kind, TyKind::Object | TyKind::Any)
    }
}
