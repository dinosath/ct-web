// comptime_orm/src/reflect.rs – compile-time type reflection via std::mem::type_info.
//
// Uses the nightly `type_info` API (rust-lang/rust#146922) to inspect entity
// structs at compile time.  This enables:
//   • Field name extraction without proc-macro parsing
//   • Type classification for schema generation
//   • Size inspection for query planning
//
// When `#[comptime]` stabilises (rust-lang/rust-project-goals#406), this
// module will drive the full entity metadata generation, replacing the
// proc-macro approach entirely.

use std::mem::type_info::{Type, TypeKind};

/// Classify a Rust type into a SQL-like kind string at compile time.
///
/// Uses `std::mem::type_info::Type::of::<T>()` – the nightly compile-time
/// reflection API.
///
/// # Example
/// ```rust
/// const KIND: &str = comptime_orm::reflect::type_kind::<i64>();  // "integer"
/// const KIND2: &str = comptime_orm::reflect::type_kind::<String>(); // "string"
/// ```
pub const fn type_kind<T: ?Sized>() -> &'static str {
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
pub const fn type_size<T: ?Sized>() -> Option<usize> {
    Type::of::<T>().size
}

/// Extract field names from a struct type at runtime using `type_info`.
///
/// Returns a `Vec<&'static str>` of the struct's field names.
/// For non-struct types the vec is empty.
///
/// This is the primary reflection machinery described in Agents.md §14.
/// Field *types* are exposed as `TypeId`s in the current nightly API;
/// full recursive `Type` access is tracked in:
/// <https://github.com/rust-lang/rust/issues/146922>
pub fn struct_field_names<T: ?Sized>() -> Vec<&'static str> {
    let info = Type::of::<T>();
    if let TypeKind::Struct(s) = info.kind {
        s.fields.iter().map(|f| f.name).collect()
    } else {
        vec![]
    }
}

/// Classify a Rust type into a SQL type string using `type_info`.
///
/// Returns a SQL type string suitable for DDL generation.
/// For struct types returns "JSONB" (treat as serialised object).
pub const fn type_to_sql_kind<T: ?Sized>() -> &'static str {
    let info = Type::of::<T>();
    match info.kind {
        TypeKind::Bool(_)   => "BOOLEAN",
        TypeKind::Int(_)    => {
            // Use overall type size to distinguish int widths.
            match info.size {
                Some(1) | Some(2) => "SMALLINT",
                Some(3) | Some(4) => "INTEGER",
                _                 => "BIGINT",
            }
        }
        TypeKind::Float(_)  => {
            match info.size {
                Some(4) => "REAL",
                _       => "DOUBLE PRECISION",
            }
        }
        TypeKind::Str(_)    => "TEXT",
        TypeKind::Struct(_) => "JSONB",
        TypeKind::Enum(_)   => "TEXT",
        _                   => "TEXT",
    }
}

/// Inspect all fields of a struct entity type and return metadata tuples.
///
/// Returns `(field_name, sql_kind)` pairs for each struct field.
/// Non-struct types return an empty vec.
///
/// This function demonstrates the future direction where entity metadata
/// is derived entirely from `type_info` reflection instead of proc macros.
pub fn entity_field_info<T: ?Sized>() -> Vec<(&'static str, &'static str)> {
    let info = Type::of::<T>();
    if let TypeKind::Struct(s) = info.kind {
        s.fields.iter().map(|f| {
            // Currently we can't resolve field TypeId back to a full Type,
            // so we use a heuristic based on field size and name.
            // This will improve when type_info field-type resolution stabilises.
            let sql_kind = match f.name {
                n if n.ends_with("_id") || n == "id" => "BIGINT",
                n if n.starts_with("is_") || n.starts_with("has_") => "BOOLEAN",
                _ => "TEXT",
            };
            (f.name, sql_kind)
        }).collect()
    } else {
        vec![]
    }
}
