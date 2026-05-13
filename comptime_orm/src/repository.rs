// comptime_orm/src/repository.rs – Jakarta Data-style repository traits and runtime impl.
//
// Hierarchy mirrors jakarta.data:
//
//   DataRepository<E, ID>           (marker)
//       └── CrudRepository<E, ID>   (CRUD + count + exists)
//               └── PageableRepository<E, ID>  (paginated finds)
//
// All SQL is generated at *runtime* by inspecting `EntityInfo::of::<E>()`
// (which uses `std::mem::type_info`).  Generated SQL strings are cached
// in a per-type `OnceLock` so each entity pays the generation cost once.
//
// Migration note (Agents.md §16):
//   When #[comptime] stabilises, `EntityInfo::of::<E>()` will be called at
//   compile time and all SQL strings will become `&'static str` in .rodata.
//   The trait signatures and `DynRepository` impl stay identical.

use std::sync::OnceLock;

use sqlx::postgres::PgArguments;
use sqlx::PgPool;

use crate::entity_info::RuntimeEntity;
use crate::mapper::ParamValue;
use crate::page::{Page, Pageable};

// ──────────────────────────────────────────────────────────────────────
// Marker / base trait
// ──────────────────────────────────────────────────────────────────────

/// Root repository marker – mirrors `jakarta.data.repository.DataRepository`.
pub trait DataRepository<E, ID>: Send + Sync {}

// ──────────────────────────────────────────────────────────────────────
// CrudRepository
// ──────────────────────────────────────────────────────────────────────

/// Full CRUD operations for an entity type `E` with primary key type `ID`.
///
/// Mirrors `jakarta.data.repository.CrudRepository`.
#[allow(async_fn_in_trait)]
pub trait CrudRepository<E, ID>: DataRepository<E, ID>
where
    E: RuntimeEntity
        + for<'r> sqlx::FromRow<'r, sqlx::postgres::PgRow>
        + Send
        + Unpin,
    ID: for<'q> sqlx::Encode<'q, sqlx::Postgres>
        + sqlx::Type<sqlx::Postgres>
        + Clone
        + Send
        + 'static,
{
    // ── abstract ──────────────────────────────────────────────────────

    /// Return the connection pool used by this repository.
    fn pool(&self) -> &PgPool;

    // ── provided ──────────────────────────────────────────────────────

    async fn find_by_id(&self, id: ID) -> sqlx::Result<Option<E>> {
        let sql = E::entity_info().find_by_id_sql();
        sqlx::query_as::<sqlx::Postgres, E>(&sql)
            .bind(id)
            .fetch_optional(self.pool())
            .await
    }

    async fn find_all(&self) -> sqlx::Result<Vec<E>> {
        let sql = E::entity_info().select_all_sql();
        sqlx::query_as::<sqlx::Postgres, E>(&sql)
            .fetch_all(self.pool())
            .await
    }

    /// Persist a new entity.  Field values are supplied as an ordered
    /// `Vec<ParamValue>` matching the entity's column order.
    ///
    /// Returns the new primary-key value as `i64`.
    async fn save(&self, values: Vec<ParamValue>) -> sqlx::Result<i64> {
        let info = E::entity_info();
        let sql  = info.insert_sql();
        let mut q = sqlx::query_scalar::<sqlx::Postgres, i64>(&sql);
        for v in &values {
            q = bind_param_value(q, v);
        }
        q.fetch_one(self.pool()).await
    }

    /// Update an entity by PK.
    /// `values` must contain non-PK columns in declaration order,
    /// followed by the PK value last.
    async fn update(&self, values: Vec<ParamValue>) -> sqlx::Result<u64> {
        let info = E::entity_info();
        let sql  = info.update_sql();
        let mut q = sqlx::query(&sql);
        for v in &values {
            q = bind_query_param(q, v);
        }
        let res = q.execute(self.pool()).await?;
        Ok(res.rows_affected())
    }

    async fn delete_by_id(&self, id: ID) -> sqlx::Result<u64> {
        let sql = E::entity_info().delete_sql();
        let res = sqlx::query(&sql)
            .bind(id)
            .execute(self.pool())
            .await?;
        Ok(res.rows_affected())
    }

    async fn delete_all(&self) -> sqlx::Result<u64> {
        let sql = E::entity_info().delete_all_sql();
        let res = sqlx::query(&sql).execute(self.pool()).await?;
        Ok(res.rows_affected())
    }

    async fn count(&self) -> sqlx::Result<i64> {
        let sql = E::entity_info().count_sql();
        sqlx::query_scalar::<sqlx::Postgres, i64>(&sql)
            .fetch_one(self.pool())
            .await
    }

    async fn exists_by_id(&self, id: ID) -> sqlx::Result<bool> {
        let info = E::entity_info();
        let sql  = format!(
            "SELECT EXISTS(SELECT 1 FROM {} WHERE {} = $1)",
            info.table,
            info.pk().col_name
        );
        sqlx::query_scalar::<sqlx::Postgres, bool>(&sql)
            .bind(id)
            .fetch_one(self.pool())
            .await
    }
}

// ──────────────────────────────────────────────────────────────────────
// PageableRepository
// ──────────────────────────────────────────────────────────────────────

/// Extends `CrudRepository` with paginated and filtered retrieval.
///
/// Mirrors `jakarta.data.repository.PageableRepository`.
#[allow(async_fn_in_trait)]
pub trait PageableRepository<E, ID>: CrudRepository<E, ID>
where
    E: RuntimeEntity
        + for<'r> sqlx::FromRow<'r, sqlx::postgres::PgRow>
        + Send
        + Unpin,
    ID: for<'q> sqlx::Encode<'q, sqlx::Postgres>
        + sqlx::Type<sqlx::Postgres>
        + Clone
        + Send
        + 'static,
{
    /// Fetch a page of all rows.
    async fn find_all_paged(&self, pageable: Pageable) -> sqlx::Result<Page<E>> {
        let info  = E::entity_info();
        let base  = info.select_all_sql();
        let total = self.count().await?;

        let (suffix, _) = pageable.sql_suffix(1);
        let sql = format!("{}{}", base, suffix);

        let content = sqlx::query_as::<sqlx::Postgres, E>(&sql)
            .bind(pageable.size as i64)
            .bind(pageable.offset() as i64)
            .fetch_all(self.pool())
            .await?;

        Ok(Page::new(content, pageable, total as u64))
    }

    /// Find by a single column value (dynamically built WHERE clause).
    async fn find_by_column(
        &self,
        col_name: &str,
        value: ParamValue,
    ) -> sqlx::Result<Vec<E>> {
        let info = E::entity_info();
        let sql = format!(
            "SELECT {} FROM {} WHERE {} = $1",
            info.column_fields().map(|f| f.col_name).collect::<Vec<_>>().join(", "),
            info.table,
            col_name
        );
        let mut q = sqlx::query_as::<sqlx::Postgres, E>(&sql);
        q = bind_as_param(q, &value);
        q.fetch_all(self.pool()).await
    }

    /// Find by column with pagination.
    async fn find_by_column_paged(
        &self,
        col_name: &str,
        value: ParamValue,
        pageable: Pageable,
    ) -> sqlx::Result<Page<E>> {
        let info = E::entity_info();

        // Count matching rows.
        let count_sql = format!(
            "SELECT COUNT(*) FROM {} WHERE {} = $1",
            info.table, col_name
        );
        let total: i64 = {
            let mut cq = sqlx::query_scalar::<sqlx::Postgres, i64>(&count_sql);
            cq = bind_scalar_param(cq, &value);
            cq.fetch_one(self.pool()).await?
        };

        // Fetch page.
        let col_list = info
            .column_fields()
            .map(|f| f.col_name)
            .collect::<Vec<_>>()
            .join(", ");
        let (suffix, _) = pageable.sql_suffix(2);
        let sql = format!(
            "SELECT {} FROM {} WHERE {} = $1{}",
            col_list, info.table, col_name, suffix
        );
        let mut q = sqlx::query_as::<sqlx::Postgres, E>(&sql);
        q = bind_as_param(q, &value);
        q = q
            .bind(pageable.size as i64)
            .bind(pageable.offset() as i64);
        let content = q.fetch_all(self.pool()).await?;

        Ok(Page::new(content, pageable, total as u64))
    }
}

// ──────────────────────────────────────────────────────────────────────
// GenericRepository – a concrete zero-boilerplate impl
// ──────────────────────────────────────────────────────────────────────

/// Ready-to-use repository backed by a `PgPool`.
///
/// Developers can either:
///   (a) use `GenericRepository::<MyEntity, i64>::new(pool)` directly, or
///   (b) create a newtype and delegate through the trait impls.
///
/// # Example
/// ```rust
/// let repo = GenericRepository::<User, i64>::new(pool.clone());
/// let user = repo.find_by_id(1).await?;
/// ```
pub struct GenericRepository<E, ID> {
    pool: PgPool,
    _e:   std::marker::PhantomData<fn() -> (E, ID)>,
}

impl<E, ID> GenericRepository<E, ID> {
    pub fn new(pool: PgPool) -> Self {
        GenericRepository { pool, _e: std::marker::PhantomData }
    }
}

impl<E, ID> DataRepository<E, ID> for GenericRepository<E, ID>
where
    E: RuntimeEntity
        + for<'r> sqlx::FromRow<'r, sqlx::postgres::PgRow>
        + Send
        + Unpin,
    ID: for<'q> sqlx::Encode<'q, sqlx::Postgres>
        + sqlx::Type<sqlx::Postgres>
        + Clone
        + Send
        + 'static,
{}

impl<E, ID> CrudRepository<E, ID> for GenericRepository<E, ID>
where
    E: RuntimeEntity
        + for<'r> sqlx::FromRow<'r, sqlx::postgres::PgRow>
        + Send
        + Unpin,
    ID: for<'q> sqlx::Encode<'q, sqlx::Postgres>
        + sqlx::Type<sqlx::Postgres>
        + Clone
        + Send
        + 'static,
{
    fn pool(&self) -> &PgPool { &self.pool }
}

impl<E, ID> PageableRepository<E, ID> for GenericRepository<E, ID>
where
    E: RuntimeEntity
        + for<'r> sqlx::FromRow<'r, sqlx::postgres::PgRow>
        + Send
        + Unpin,
    ID: for<'q> sqlx::Encode<'q, sqlx::Postgres>
        + sqlx::Type<sqlx::Postgres>
        + Clone
        + Send
        + 'static,
{}

// ──────────────────────────────────────────────────────────────────────
// Private bind helpers
//
// We use a small helper set to bind `ParamValue` to the various sqlx
// query builder types without repeating match arms everywhere.
// ──────────────────────────────────────────────────────────────────────

fn bind_param_value<'q, O>(
    q: sqlx::query::QueryScalar<'q, sqlx::Postgres, O, PgArguments>,
    v: &ParamValue,
) -> sqlx::query::QueryScalar<'q, sqlx::Postgres, O, PgArguments>
where
    O: Send + Unpin,
{
    match v {
        ParamValue::I64(n)  => q.bind(*n),
        ParamValue::F64(n)  => q.bind(*n),
        ParamValue::Bool(b) => q.bind(*b),
        ParamValue::Text(s) => q.bind(s.clone()),
        ParamValue::Null    => q.bind(Option::<i64>::None),
    }
}

fn bind_query_param<'q>(
    q: sqlx::query::Query<'q, sqlx::Postgres, PgArguments>,
    v: &ParamValue,
) -> sqlx::query::Query<'q, sqlx::Postgres, PgArguments> {
    match v {
        ParamValue::I64(n)  => q.bind(*n),
        ParamValue::F64(n)  => q.bind(*n),
        ParamValue::Bool(b) => q.bind(*b),
        ParamValue::Text(s) => q.bind(s.clone()),
        ParamValue::Null    => q.bind(Option::<i64>::None),
    }
}

fn bind_as_param<'q, E>(
    q: sqlx::query::QueryAs<'q, sqlx::Postgres, E, PgArguments>,
    v: &ParamValue,
) -> sqlx::query::QueryAs<'q, sqlx::Postgres, E, PgArguments>
where
    E: for<'r> sqlx::FromRow<'r, sqlx::postgres::PgRow>,
{
    match v {
        ParamValue::I64(n)  => q.bind(*n),
        ParamValue::F64(n)  => q.bind(*n),
        ParamValue::Bool(b) => q.bind(*b),
        ParamValue::Text(s) => q.bind(s.clone()),
        ParamValue::Null    => q.bind(Option::<i64>::None),
    }
}

fn bind_scalar_param<'q, O>(
    q: sqlx::query::QueryScalar<'q, sqlx::Postgres, O, PgArguments>,
    v: &ParamValue,
) -> sqlx::query::QueryScalar<'q, sqlx::Postgres, O, PgArguments>
where
    O: Send + Unpin,
{
    match v {
        ParamValue::I64(n)  => q.bind(*n),
        ParamValue::F64(n)  => q.bind(*n),
        ParamValue::Bool(b) => q.bind(*b),
        ParamValue::Text(s) => q.bind(s.clone()),
        ParamValue::Null    => q.bind(Option::<i64>::None),
    }
}
