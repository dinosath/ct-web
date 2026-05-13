// comptime_orm/src/entity_info.rs – Runtime entity introspection via std::mem::type_info.
//
// Jakarta Data uses annotations on Java classes; we use:
//   1. A `RuntimeEntity` derive-trait so entities declare their metadata.
//   2. `std::mem::type_info` to enumerate struct fields at runtime and
//      infer column names, PK, relations, and SQL types.
//
// Migration note (Agents.md §16):
//   When #[comptime] + field-level type_info stabilise, `RuntimeEntity`
//   will be implemented automatically for every `#[derive(Entity)]` struct
//   and all `entity_info()` calls will be resolved at compile time to
//   `&'static EntityInfo`.  The public API is intentionally identical.

use std::mem::type_info::{Type, TypeKind};

// ──────────────────────────────────────────────────────────────────────
// FieldKind – mirrors ParamSource for relation classification
// ──────────────────────────────────────────────────────────────────────

/// How a struct field maps to storage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FieldKind {
    /// Scalar column (maps 1-to-1 to a DB column).
    Column,
    /// Primary-key scalar column.
    Id,
    /// `ManyToOne` FK column (the FK lives on this entity's table).
    ManyToOne { target_table: &'static str },
    /// `OneToMany` – no column on this table; fetched separately.
    OneToMany { target_table: &'static str, mapped_by: &'static str },
    /// `ManyToMany` via join table.
    ManyToMany { join_table: &'static str },
}

// ──────────────────────────────────────────────────────────────────────
// FieldInfo
// ──────────────────────────────────────────────────────────────────────

/// Runtime description of a single entity field.
#[derive(Debug, Clone)]
pub struct FieldInfo {
    /// Rust field name.
    pub name:      &'static str,
    /// Inferred SQL column name (snake_case, same as field name).
    pub col_name:  &'static str,
    /// Inferred SQL type string (e.g. "BIGINT", "TEXT").
    pub sql_type:  &'static str,
    /// Whether the column is nullable.
    pub nullable:  bool,
    /// Field classification.
    pub kind:      FieldKind,
}

// ──────────────────────────────────────────────────────────────────────
// EntityInfo – runtime metadata produced by type_info inspection
// ──────────────────────────────────────────────────────────────────────

/// Runtime metadata for a database entity type.
///
/// Produced once per type by `EntityInfo::of::<T>()` and cached in a
/// `OnceLock` inside `RuntimeEntity::entity_info()`.
#[derive(Debug, Clone)]
pub struct EntityInfo {
    /// SQL table name (plural snake_case of the type name).
    pub table:  String,
    /// Ordered field list (same order as struct definition).
    pub fields: Vec<FieldInfo>,
    /// Index into `fields` of the primary-key field.
    pub pk_idx: usize,
}

impl EntityInfo {
    /// Introspect `T` using `std::mem::type_info` and build runtime metadata.
    ///
    /// Field classification heuristics (will be replaced by attribute
    /// annotations once `#[comptime]` lands):
    ///
    ///  * field named `"id"` or ending with `"_id"` when it is the first
    ///    field → primary key
    ///  * field whose name ends with `"_id"` (not first) → ManyToOne FK
    ///  * field whose name ends with `"s"` or `"_list"` or `"_ids"` → OneToMany
    ///  * other fields → scalar column
    pub fn of<T: ?Sized>() -> Self {
        let type_inf = Type::of::<T>();

        // ── table name ────────────────────────────────────────────────
        // `Type` does not expose a type name in the current nightly API,
        // so we fall back to the `std::any::type_name::<T>()` which yields
        // a fully-qualified path like `my_crate::models::User`.  We use
        // only the last segment.
        let full_name = std::any::type_name::<T>();
        let short_name = full_name.rsplit(':').next().unwrap_or(full_name);
        let table = to_snake_plural(short_name);

        // ── fields ────────────────────────────────────────────────────
        // Field::ty is a TypeId; Field::offset is the byte offset within
        // the struct.  We use offset differences as a proxy for field size.
        let raw_fields: Vec<(&'static str, usize /*offset*/)> =
            if let TypeKind::Struct(s) = type_inf.kind {
                s.fields.iter().map(|f| (f.name, f.offset)).collect()
            } else {
                vec![]
            };

        // Compute per-field size from offset differences.
        let total_size = type_inf.size.unwrap_or(0);
        let n = raw_fields.len();
        let raw_fields_with_size: Vec<(&'static str, Option<usize>)> = raw_fields
            .iter()
            .enumerate()
            .map(|(i, (name, offset))| {
                let next_offset = if i + 1 < n {
                    raw_fields[i + 1].1
                } else {
                    total_size
                };
                let sz = if next_offset > *offset { Some(next_offset - offset) } else { None };
                (*name, sz)
            })
            .collect();

        let mut pk_idx = 0usize;
        let mut found_pk = false;

        let fields: Vec<FieldInfo> = raw_fields_with_size
            .into_iter()
            .enumerate()
            .map(|(i, (name, size))| {
                let kind = classify_field(name, i, &mut pk_idx, &mut found_pk);
                let sql_type = infer_sql_type(name, size, &kind);
                let nullable = matches!(kind, FieldKind::OneToMany { .. } | FieldKind::ManyToMany { .. });

                FieldInfo {
                    name,
                    col_name: name,
                    sql_type,
                    nullable,
                    kind,
                }
            })
            .collect();

        EntityInfo { table, fields, pk_idx }
    }

    // ── convenience accessors ─────────────────────────────────────────

    /// Return the primary-key `FieldInfo`.
    pub fn pk(&self) -> &FieldInfo {
        &self.fields[self.pk_idx]
    }

    /// Columns that map directly to SQL columns (scalar + FK, not OneToMany/ManyToMany).
    pub fn column_fields(&self) -> impl Iterator<Item = &FieldInfo> {
        self.fields.iter().filter(|f| {
            matches!(f.kind, FieldKind::Column | FieldKind::Id | FieldKind::ManyToOne { .. })
        })
    }

    // ── SQL generation ────────────────────────────────────────────────

    /// `SELECT col1, col2, … FROM table`
    pub fn select_all_sql(&self) -> String {
        let cols = self.col_list();
        format!("SELECT {} FROM {}", cols, self.table)
    }

    /// `SELECT … FROM table WHERE pk = $1`
    pub fn find_by_id_sql(&self) -> String {
        format!(
            "SELECT {} FROM {} WHERE {} = $1",
            self.col_list(),
            self.table,
            self.pk().col_name
        )
    }

    /// `SELECT COUNT(*) FROM table`
    pub fn count_sql(&self) -> String {
        format!("SELECT COUNT(*) FROM {}", self.table)
    }

    /// `INSERT INTO table (cols…) VALUES ($1, …) RETURNING pk`
    pub fn insert_sql(&self) -> String {
        let cols: Vec<&str> = self.column_fields().map(|f| f.col_name).collect();
        let placeholders: Vec<String> =
            (1..=cols.len()).map(|i| format!("${i}")).collect();
        format!(
            "INSERT INTO {} ({}) VALUES ({}) RETURNING {}",
            self.table,
            cols.join(", "),
            placeholders.join(", "),
            self.pk().col_name
        )
    }

    /// `UPDATE table SET col1=$1 … WHERE pk=$N`
    pub fn update_sql(&self) -> String {
        let non_pk: Vec<&FieldInfo> = self
            .column_fields()
            .filter(|f| !matches!(f.kind, FieldKind::Id))
            .collect();
        let set: Vec<String> = non_pk
            .iter()
            .enumerate()
            .map(|(i, f)| format!("{} = ${}", f.col_name, i + 1))
            .collect();
        format!(
            "UPDATE {} SET {} WHERE {} = ${}",
            self.table,
            set.join(", "),
            self.pk().col_name,
            non_pk.len() + 1
        )
    }

    /// `DELETE FROM table WHERE pk = $1`
    pub fn delete_sql(&self) -> String {
        format!("DELETE FROM {} WHERE {} = $1", self.table, self.pk().col_name)
    }

    /// `DELETE FROM table`
    pub fn delete_all_sql(&self) -> String {
        format!("DELETE FROM {}", self.table)
    }

    // ── helpers ───────────────────────────────────────────────────────

    fn col_list(&self) -> String {
        self.column_fields()
            .map(|f| f.col_name)
            .collect::<Vec<_>>()
            .join(", ")
    }
}

// ──────────────────────────────────────────────────────────────────────
// RuntimeEntity trait
// ──────────────────────────────────────────────────────────────────────

/// Implemented by entity structs (via `#[derive(Repository)]` or manually).
///
/// Provides cached runtime metadata built from `std::mem::type_info`.
/// When `#[comptime]` lands, the `entity_info()` body will be replaced
/// with a const-evaluated `&'static EntityInfo`.
pub trait RuntimeEntity: Sized + Send + Sync + 'static {
    /// Return the runtime `EntityInfo` for this type (lazily built once).
    fn entity_info() -> &'static EntityInfo;
}

// ──────────────────────────────────────────────────────────────────────
// Private helpers
// ──────────────────────────────────────────────────────────────────────

fn classify_field(
    name: &'static str,
    idx: usize,
    pk_idx: &mut usize,
    found_pk: &mut bool,
) -> FieldKind {
    // First field named "id" or any field ending "_id" that is the first field.
    if !*found_pk && (name == "id" || (idx == 0 && name.ends_with("_id"))) {
        *pk_idx   = idx;
        *found_pk = true;
        return FieldKind::Id;
    }

    // Non-first `*_id` field → foreign key (ManyToOne).
    // We derive the target table name from the prefix before `_id`.
    if name.ends_with("_id") && name.len() > 3 {
        let target = &name[..name.len() - 3];
        // We need a &'static str; we leak a small allocation once per
        // field (acceptable until comptime replaces this path).
        let target_table: &'static str = Box::leak(to_snake_plural(target).into_boxed_str());
        return FieldKind::ManyToOne { target_table };
    }

    // `*_ids`, `*s`, `*_list` heuristic → OneToMany.
    if name.ends_with("_ids") || name.ends_with("_list") {
        // Strip suffix to get target table.
        let base = if name.ends_with("_ids") {
            &name[..name.len() - 4]
        } else {
            &name[..name.len() - 5]
        };
        let target_table: &'static str = Box::leak(to_snake_plural(base).into_boxed_str());
        // "mapped_by" uses a convention: snake of caller entity + "_id".
        // Without comptime we use an empty string; callers should override.
        return FieldKind::OneToMany { target_table, mapped_by: "" };
    }

    FieldKind::Column
}

fn infer_sql_type(name: &'static str, size: Option<usize>, kind: &FieldKind) -> &'static str {
    match kind {
        FieldKind::Id | FieldKind::ManyToOne { .. } => "BIGINT",
        FieldKind::OneToMany { .. } | FieldKind::ManyToMany { .. } => "JSONB", // not a real column
        FieldKind::Column => {
            if name.starts_with("is_") || name.starts_with("has_") || name == "active" || name == "deleted" {
                return "BOOLEAN";
            }
            if name.ends_with("_at") || name.ends_with("_date") || name.ends_with("_time") {
                return "TIMESTAMP";
            }
            if name == "price" || name == "amount" || name == "total" || name == "balance" {
                return "NUMERIC";
            }
            // Use byte size as a proxy for integer width.
            match size {
                Some(1) | Some(2) => "SMALLINT",
                Some(4)           => "INTEGER",
                Some(8)           => "BIGINT",
                _                 => "TEXT",
            }
        }
    }
}

/// Convert `CamelCase` or `snake_case` to `snake_case_plural`.
fn to_snake_plural(s: &str) -> String {
    let snake = camel_to_snake(s);
    // Naïve English pluralisation sufficient for table naming.
    if snake.ends_with('s') || snake.ends_with('x') || snake.ends_with('z') {
        format!("{}es", snake)
    } else {
        format!("{}s", snake)
    }
}

fn camel_to_snake(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 4);
    for (i, ch) in s.char_indices() {
        if ch.is_uppercase() && i > 0 {
            out.push('_');
        }
        out.extend(ch.to_lowercase());
    }
    out
}
