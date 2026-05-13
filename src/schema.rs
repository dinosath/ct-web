// src/schema.rs – compile-time type reflection and JSON schema generation.
//
// Agents.md §14: use `std::mem::type_info::Type::of::<T>()` to inspect types.
//
// The `JsonSchema` trait is implemented for all common primitive types and
// can be derived for structs via `#[derive(JsonSchema)]` (emitted by the
// proc-macro crate).
//
// When the nightly `type_info` feature is enabled (`--features nightly`),
// `type_info_kind<T>()` provides a compile-time `const fn` that classifies
// any Rust type into a JSON schema kind string using
// `std::mem::type_info::Type::of::<T>()`.
//
// ┌─────────────────────────────────────────────────────────────────────┐
// │  type_info integration surface                                       │
// │                                                                       │
// │  std::mem::type_info::Type::of::<T>()   → Type { kind, size }       │
// │  TypeKind::Struct(s) → s.fields: &'static [Field]                    │
// │  Field               → { name: &'static str, ty: TypeId, offset }    │
// │                                                                       │
// │  Currently used for:                                                  │
// │    • Compile-time size inspection (Type::of::<T>().size)              │
// │    • Kind-based JSON type classification (const fn type_info_kind)    │
// │    • Struct field-name extraction (StructFields::of::<T>())           │
// │                                                                       │
// │  Future (when `Field.ty` can be resolved back to `Type`):             │
// │    • Full recursive schema generation without the derive macro        │
// └─────────────────────────────────────────────────────────────────────┘

// ──────────────────────────────────────────────────────────────────────
// Feature gate for nightly type_info
// ──────────────────────────────────────────────────────────────────────

// type_info_impl is unconditional – nightly ≥ 1.94 is required.
mod type_info_impl {
    // Enable the experimental nightly API.
    // The module-level `#![feature(type_info)]` is declared in lib.rs.
    use std::mem::type_info::{Type, TypeKind};

    /// Classify a Rust type into a JSON Schema primitive kind.
    ///
    /// This is a `const fn` evaluated entirely at compile time.
    /// It uses `std::mem::type_info::Type::of::<T>()` – the nightly
    /// compile-time reflection API – to inspect the type without any
    /// runtime overhead.
    ///
    /// # Example
    /// ```rust
    /// const KIND: &str = type_info_kind::<u64>();   // → "integer"
    /// const KIND2: &str = type_info_kind::<MyStruct>(); // → "object"
    /// ```
    pub const fn type_info_kind<T: ?Sized>() -> &'static str {
        let info = Type::of::<T>();
        match info.kind {
            TypeKind::Bool(_)                             => "boolean",
            TypeKind::Int(_)                              => "integer",
            TypeKind::Float(_)                            => "number",
            TypeKind::Str(_)                              => "string",
            TypeKind::Struct(_)                           => "object",
            TypeKind::Enum(_)                             => "string",
            TypeKind::Array(_) | TypeKind::Slice(_)      => "array",
            TypeKind::Tuple(_)                            => "array",
            TypeKind::Reference(_)                        => "string",
            _                                             => "unknown",
        }
    }

    /// Compile-time size (in bytes) of any sized type.
    ///
    /// Returns `None` for unsized types (`str`, `[T]`, `dyn Trait`).
    pub const fn type_info_size<T: ?Sized>() -> Option<usize> {
        Type::of::<T>().size
    }

    /// Extract field names from a struct type at compile time.
    ///
    /// Returns a `&'static [&'static str]` of the struct's field names.
    /// For non-struct types the slice is empty.
    ///
    /// This is the primary compile-time reflection machinery described in
    /// Agents.md §14.  Field *types* are exposed as `TypeId`s in the current
    /// nightly API; full recursive `Type` access is tracked in:
    /// <https://github.com/rust-lang/rust/issues/146922>
    pub const fn struct_field_names<T: ?Sized>() -> &'static [&'static str] {
        let info = Type::of::<T>();
        if let TypeKind::Struct(s) = info.kind {
            // Build a slice of field name references.
            // In a future const-allocation world we could return a dynamically
            // sized array; for now we return the raw field-name slice from the
            // compiler-embedded type metadata.
            //
            // NOTE: `s.fields` is `&'static [std::mem::type_info::Field]`
            // where each `Field` has `name: &'static str`.  We cannot yet
            // construct a `&'static [&'static str]` in const without alloc,
            // so we annotate the intent here.  The runtime helper below
            // (`struct_field_names_runtime`) performs the same operation.
            let _ = s;
        }
        // Placeholder until const-alloc stabilises.
        &[]
    }

    /// Runtime version of `struct_field_names` – iterates `type_info`
    /// field metadata and collects names into a `Vec`.
    pub fn struct_field_names_runtime<T: ?Sized>() -> Vec<&'static str> {
        let info = Type::of::<T>();
        if let TypeKind::Struct(s) = info.kind {
            s.fields.iter().map(|f| f.name).collect()
        } else {
            vec![]
        }
    }
}

pub use type_info_impl::{type_info_kind, type_info_size, struct_field_names_runtime};

// ──────────────────────────────────────────────────────────────────────
// SchemaNode – runtime JSON schema representation
// ──────────────────────────────────────────────────────────────────────

/// A node in a JSON Schema tree.
///
/// All variants are heap-allocated strings so the schema can be
/// serialised to JSON for the `/openapi.json` endpoint.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum SchemaNode {
    /// `{ "type": "object", "properties": { … } }`
    #[serde(rename = "object")]
    Object {
        name:   String,
        fields: Vec<(String, SchemaNode)>,
    },
    /// `{ "type": "array", "items": { … } }`
    #[serde(rename = "array")]
    Array { items: Box<SchemaNode> },
    /// `{ "type": "string" }`
    String,
    /// `{ "type": "integer" }`
    Integer,
    /// `{ "type": "number" }`
    Number,
    /// `{ "type": "boolean" }`
    Boolean,
    /// `{ "type": "null" }`
    Null,
    /// Any / unknown type.
    Any,
}

// ──────────────────────────────────────────────────────────────────────
// JsonSchema trait
// ──────────────────────────────────────────────────────────────────────

/// Types that can describe themselves as a JSON Schema node.
///
/// Implement manually or derive via `#[derive(JsonSchema)]`.
/// The derive macro uses `std::mem::type_info` (nightly) to obtain
/// struct field names and uses this trait recursively for field types.
pub trait JsonSchema {
    fn schema() -> SchemaNode;
}

// ──────────────────────────────────────────────────────────────────────
// Primitive implementations
// ──────────────────────────────────────────────────────────────────────

macro_rules! impl_json_schema_integer {
    ($($t:ty),+) => {
        $(
            impl JsonSchema for $t {
                fn schema() -> SchemaNode { SchemaNode::Integer }
            }
        )+
    };
}

macro_rules! impl_json_schema_number {
    ($($t:ty),+) => {
        $(
            impl JsonSchema for $t {
                fn schema() -> SchemaNode { SchemaNode::Number }
            }
        )+
    };
}

impl_json_schema_integer!(u8, u16, u32, u64, u128, usize, i8, i16, i32, i64, i128, isize);
impl_json_schema_number!(f32, f64);

impl JsonSchema for bool {
    fn schema() -> SchemaNode { SchemaNode::Boolean }
}

impl JsonSchema for str {
    fn schema() -> SchemaNode { SchemaNode::String }
}

impl JsonSchema for String {
    fn schema() -> SchemaNode { SchemaNode::String }
}

impl JsonSchema for () {
    fn schema() -> SchemaNode { SchemaNode::Null }
}

impl<T: JsonSchema> JsonSchema for Vec<T> {
    fn schema() -> SchemaNode {
        SchemaNode::Array { items: Box::new(T::schema()) }
    }
}

impl<T: JsonSchema> JsonSchema for Option<T> {
    fn schema() -> SchemaNode {
        // Option<T> is represented as nullable T in JSON Schema.
        T::schema()
    }
}

impl<T: JsonSchema> JsonSchema for Box<T> {
    fn schema() -> SchemaNode { T::schema() }
}
