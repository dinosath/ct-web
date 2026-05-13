// comptime_di/src/reflect.rs – compile-time service introspection via std::mem::type_info.
//
// Uses the nightly `type_info` API to inspect service types at compile time.
// This enables:
//   • Compile-time dependency graph validation (struct field → service type)
//   • Automatic detection of injectable fields without macro parsing
//   • Service metadata extraction for diagnostics and documentation
//
// When #[comptime] stabilises, these functions will replace the proc-macro
// approach entirely – the DI container will wire services by inspecting
// struct fields via type_info reflection.

use std::mem::type_info::{Type, TypeKind};

/// Compile-time metadata about a service type.
pub struct ServiceMeta {
    /// The Rust type name from type_info.
    pub type_name: &'static str,
    /// Size in bytes (None for unsized types).
    pub size: Option<usize>,
    /// Kind classification (struct, enum, etc.).
    pub kind: &'static str,
    /// Field names (empty for non-struct types).
    pub fields: Vec<&'static str>,
}

/// Inspect a type via `std::mem::type_info` and return service metadata.
///
/// This is the primary entry point for compile-time service introspection.
/// Used by the DI framework to understand service structure without macros.
///
/// # Example
/// ```rust
/// let meta = comptime_di::reflect::service_meta::<OrderService>();
/// assert_eq!(meta.kind, "object");
/// assert!(meta.fields.contains(&"user_repo"));
/// ```
pub fn service_meta<T: ?Sized>() -> ServiceMeta {
    let info = Type::of::<T>();

    let kind = match info.kind {
        TypeKind::Struct(_) => "object",
        TypeKind::Enum(_)   => "enum",
        TypeKind::Bool(_)   => "boolean",
        TypeKind::Int(_)    => "integer",
        TypeKind::Float(_)  => "number",
        TypeKind::Str(_)    => "string",
        _                   => "unknown",
    };

    let fields = if let TypeKind::Struct(s) = info.kind {
        s.fields.iter().map(|f| f.name).collect()
    } else {
        vec![]
    };

    ServiceMeta {
        type_name: std::any::type_name::<T>(),
        size: info.size,
        kind,
        fields,
    }
}

/// Classify a service type at compile time.
///
/// Returns "object" for structs (services), "enum" for enums, etc.
pub const fn service_kind<T: ?Sized>() -> &'static str {
    let info = Type::of::<T>();
    match info.kind {
        TypeKind::Struct(_) => "object",
        TypeKind::Enum(_)   => "enum",
        _                   => "other",
    }
}

/// Get the compile-time type name.
pub const fn service_type_name<T: ?Sized>() -> &'static str {
    std::any::type_name::<T>()
}

/// Get the number of fields in a service struct.
///
/// Returns 0 for non-struct types.  Used to compute dependency counts
/// at compile time.
pub const fn service_field_count<T: ?Sized>() -> usize {
    let info = Type::of::<T>();
    if let TypeKind::Struct(s) = info.kind {
        s.fields.len()
    } else {
        0
    }
}

/// Extract field names from a service struct using type_info.
///
/// Each field name corresponds to a dependency that should be resolved
/// from the service registry.  Fields whose type is `Arc<T>` indicate
/// a dependency on service `T`.
pub fn service_field_names<T: ?Sized>() -> Vec<&'static str> {
    let info = Type::of::<T>();
    if let TypeKind::Struct(s) = info.kind {
        s.fields.iter().map(|f| f.name).collect()
    } else {
        vec![]
    }
}

/// Check if a type is a struct (and thus potentially injectable).
pub const fn is_injectable_type<T: ?Sized>() -> bool {
    matches!(Type::of::<T>().kind, TypeKind::Struct(_))
}

/// Compile-time validation: assert that a type is a struct.
///
/// Include this in generated code to produce a compile error if someone
/// tries to `#[derive(Injectable)]` on a non-struct type.
pub const fn assert_struct<T: ?Sized>() {
    if !matches!(Type::of::<T>().kind, TypeKind::Struct(_)) {
        panic!("Injectable types must be structs");
    }
}
