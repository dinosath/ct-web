// comptime_orm/src/migration.rs – compile-time schema management strategy + migration runner.
//
// The active strategy is resolved at compile time from `Comptime.toml`:
//
//   [database]
//   schema-management.strategy = "create"
//
// build.rs reads `Comptime.toml`, validates the strategy, and emits
// `$OUT_DIR/comptime_config.rs` containing a const u8.

include!(concat!(env!("OUT_DIR"), "/comptime_config.rs"));

use crate::entity::all_entities;

// ──────────────────────────────────────────────────────────────────────
// SchemaStrategy
// ──────────────────────────────────────────────────────────────────────

/// Schema management strategy compiled into this binary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SchemaStrategy {
    /// Do nothing – recommended for production deployments.
    None,
    /// Panic on startup if any entity table or column is absent from the DB.
    Validate,
    /// `CREATE TABLE IF NOT EXISTS` for every entity (safe default).
    Create,
    /// `DROP TABLE IF EXISTS` then `CREATE TABLE` for every entity.
    /// **Destructive** – dev / test only.
    DropAndCreate,
    /// Create missing tables; `ALTER TABLE … ADD COLUMN IF NOT EXISTS` for
    /// missing columns.  Never drops anything.
    Update,
}

/// The strategy compiled into this binary (resolved from `Comptime.toml`).
pub const ACTIVE_STRATEGY: SchemaStrategy = match SCHEMA_MANAGEMENT_STRATEGY {
    0 => SchemaStrategy::None,
    1 => SchemaStrategy::Validate,
    2 => SchemaStrategy::Create,
    3 => SchemaStrategy::DropAndCreate,
    _ => SchemaStrategy::Update, // 4
};

// ──────────────────────────────────────────────────────────────────────
// Public entry-point
// ──────────────────────────────────────────────────────────────────────

/// Execute schema migrations according to the compile-time [`ACTIVE_STRATEGY`].
pub async fn run_migrations(pool: &sqlx::PgPool) -> sqlx::Result<()> {
    match ACTIVE_STRATEGY {
        SchemaStrategy::None         => {}
        SchemaStrategy::Validate     => validate(pool).await?,
        SchemaStrategy::Create       => create(pool).await?,
        SchemaStrategy::DropAndCreate => drop_and_create(pool).await?,
        SchemaStrategy::Update       => update(pool).await?,
    }
    Ok(())
}

// ──────────────────────────────────────────────────────────────────────
// Strategy implementations
// ──────────────────────────────────────────────────────────────────────

async fn validate(pool: &sqlx::PgPool) -> sqlx::Result<()> {
    for reg in all_entities() {
        let meta = reg.meta;

        let table_exists: bool = sqlx::query_scalar(
            "SELECT EXISTS(\
               SELECT 1 FROM information_schema.tables \
               WHERE table_schema = current_schema() AND table_name = $1\
             )",
        )
        .bind(meta.table)
        .fetch_one(pool)
        .await?;

        if !table_exists {
            panic!(
                "[comptime_orm] schema validate: table '{}' does not exist in the database. \
                 Set `schema-management.strategy = \"create\"` in Comptime.toml to create it.",
                meta.table
            );
        }

        let existing_cols: Vec<String> = sqlx::query_scalar(
            "SELECT column_name FROM information_schema.columns \
             WHERE table_schema = current_schema() AND table_name = $1",
        )
        .bind(meta.table)
        .fetch_all(pool)
        .await?;

        for col in meta.columns {
            if !existing_cols.iter().any(|c| c == col.name) {
                panic!(
                    "[comptime_orm] schema validate: column '{}.{}' is missing from the database. \
                     Set `schema-management.strategy = \"update\"` in Comptime.toml to add it.",
                    meta.table, col.name
                );
            }
        }
    }
    Ok(())
}

async fn create(pool: &sqlx::PgPool) -> sqlx::Result<()> {
    for reg in all_entities() {
        sqlx::query(reg.meta.migration_sql)
            .execute(pool)
            .await?;
    }
    Ok(())
}

async fn drop_and_create(pool: &sqlx::PgPool) -> sqlx::Result<()> {
    let entities: Vec<_> = all_entities().collect();

    for reg in entities.iter().rev() {
        sqlx::query(reg.meta.drop_sql)
            .execute(pool)
            .await?;
    }
    for reg in entities.iter() {
        sqlx::query(reg.meta.migration_sql)
            .execute(pool)
            .await?;
    }
    Ok(())
}

async fn update(pool: &sqlx::PgPool) -> sqlx::Result<()> {
    for reg in all_entities() {
        let meta = reg.meta;

        sqlx::query(meta.migration_sql)
            .execute(pool)
            .await?;

        let existing_cols: Vec<String> = sqlx::query_scalar(
            "SELECT column_name FROM information_schema.columns \
             WHERE table_schema = current_schema() AND table_name = $1",
        )
        .bind(meta.table)
        .fetch_all(pool)
        .await?;

        for col in meta.columns {
            if !existing_cols.iter().any(|c| c == col.name) {
                let alter = format!(
                    "ALTER TABLE {} ADD COLUMN IF NOT EXISTS {} {}",
                    meta.table,
                    col.name,
                    col.sql_type.as_sql_str(),
                );
                sqlx::query(&alter).execute(pool).await?;
            }
        }
    }
    Ok(())
}
