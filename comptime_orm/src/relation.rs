// comptime_orm/src/relation.rs – Jakarta Data-style relation descriptors and lazy loading.
//
// Supports:
//   OneToMany  – fetch related rows via FK on the child table.
//   ManyToOne  – fetch the parent row via FK on this table.
//   ManyToMany – fetch via a join table (bridge table).
//
// All loaders return owned `Vec<T>` / `Option<T>` to avoid lifetime
// complexity at this stage.  Future: return `Stream<Item=T>` for large sets.
//
// Migration note (Agents.md §16):
//   Once #[comptime] lands the descriptor structs become compile-time
//   constants inlined directly into the router / handler metadata.

use sqlx::PgPool;

// ──────────────────────────────────────────────────────────────────────
// Relation descriptors
// ──────────────────────────────────────────────────────────────────────

/// A one-to-many relationship between two entity tables.
///
/// Example (Jakarta Data equivalent: `@OneToMany`):
/// ```
/// OneToMany { owner_table: "users", fk_column: "user_id", child_table: "orders" }
/// ```
#[derive(Debug, Clone)]
pub struct OneToMany {
    /// Table that owns the PK side.
    pub owner_table: &'static str,
    /// FK column on `child_table` that references owner PK.
    pub fk_column:   &'static str,
    /// Table whose rows are returned.
    pub child_table: &'static str,
    /// Columns to SELECT from `child_table` (empty = `*`).
    pub select_cols: &'static [&'static str],
}

impl OneToMany {
    /// Build: `SELECT {cols} FROM {child} WHERE {fk} = $1`
    pub fn fetch_sql(&self) -> String {
        let cols = if self.select_cols.is_empty() {
            "*".to_string()
        } else {
            self.select_cols.join(", ")
        };
        format!(
            "SELECT {} FROM {} WHERE {} = $1",
            cols, self.child_table, self.fk_column
        )
    }

    /// Execute the fetch against a live pool.
    pub async fn load<C>(
        &self,
        owner_pk: i64,
        pool: &PgPool,
    ) -> sqlx::Result<Vec<C>>
    where
        C: for<'r> sqlx::FromRow<'r, sqlx::postgres::PgRow> + Send + Unpin,
    {
        let sql = self.fetch_sql();
        sqlx::query_as::<sqlx::Postgres, C>(&sql)
            .bind(owner_pk)
            .fetch_all(pool)
            .await
    }
}

/// A many-to-one relationship (FK lives on the current entity's table).
///
/// Equivalent to Jakarta Data `@ManyToOne`.
#[derive(Debug, Clone)]
pub struct ManyToOne {
    /// Table holding the FK.
    pub owner_table:  &'static str,
    /// FK column on `owner_table`.
    pub fk_column:    &'static str,
    /// Referenced (parent) table.
    pub parent_table: &'static str,
    /// PK column on `parent_table`.
    pub parent_pk:    &'static str,
}

impl ManyToOne {
    /// Build: `SELECT * FROM {parent} WHERE {parent_pk} = $1`
    pub fn fetch_sql(&self) -> String {
        format!(
            "SELECT * FROM {} WHERE {} = $1",
            self.parent_table, self.parent_pk
        )
    }

    /// Execute the fetch – returns `None` when the FK is null / not found.
    pub async fn load<P>(
        &self,
        fk_value: i64,
        pool: &PgPool,
    ) -> sqlx::Result<Option<P>>
    where
        P: for<'r> sqlx::FromRow<'r, sqlx::postgres::PgRow> + Send + Unpin,
    {
        let sql = self.fetch_sql();
        sqlx::query_as::<sqlx::Postgres, P>(&sql)
            .bind(fk_value)
            .fetch_optional(pool)
            .await
    }
}

/// A many-to-many relationship via a join table.
///
/// Equivalent to Jakarta Data `@ManyToMany`.
#[derive(Debug, Clone)]
pub struct ManyToMany {
    /// Join / bridge table.
    pub join_table:      &'static str,
    /// FK column in `join_table` that points to the owner entity PK.
    pub owner_fk:        &'static str,
    /// FK column in `join_table` that points to the target entity PK.
    pub target_fk:       &'static str,
    /// Target entity table.
    pub target_table:    &'static str,
}

impl ManyToMany {
    /// `SELECT t.* FROM target JOIN join_table jt ON jt.target_fk = t.pk WHERE jt.owner_fk = $1`
    pub fn fetch_sql(&self) -> String {
        format!(
            "SELECT t.* FROM {target} t \
             INNER JOIN {join} jt ON jt.{tfk} = t.id \
             WHERE jt.{ofk} = $1",
            target = self.target_table,
            join   = self.join_table,
            tfk    = self.target_fk,
            ofk    = self.owner_fk,
        )
    }

    pub async fn load<T>(
        &self,
        owner_pk: i64,
        pool: &PgPool,
    ) -> sqlx::Result<Vec<T>>
    where
        T: for<'r> sqlx::FromRow<'r, sqlx::postgres::PgRow> + Send + Unpin,
    {
        let sql = self.fetch_sql();
        sqlx::query_as::<sqlx::Postgres, T>(&sql)
            .bind(owner_pk)
            .fetch_all(pool)
            .await
    }
}
