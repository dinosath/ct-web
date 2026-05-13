// comptime_orm/src/query.rs – type-safe query builder backed by sqlx.

use std::marker::PhantomData;

use crate::entity::DbEntity;
use crate::mapper::ParamValue;

// ──────────────────────────────────────────────────────────────────────
// FilterOp
// ──────────────────────────────────────────────────────────────────────

/// Comparison operator for a WHERE clause predicate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterOp {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    Like,
    ILike,
    IsNull,
    IsNotNull,
}

impl FilterOp {
    const fn as_sql(self) -> &'static str {
        match self {
            FilterOp::Eq         => "=",
            FilterOp::Ne         => "!=",
            FilterOp::Lt         => "<",
            FilterOp::Le         => "<=",
            FilterOp::Gt         => ">",
            FilterOp::Ge         => ">=",
            FilterOp::Like       => "LIKE",
            FilterOp::ILike      => "ILIKE",
            FilterOp::IsNull     => "IS NULL",
            FilterOp::IsNotNull  => "IS NOT NULL",
        }
    }

    /// Whether this operator consumes a bound parameter value.
    const fn needs_value(self) -> bool {
        !matches!(self, FilterOp::IsNull | FilterOp::IsNotNull)
    }
}

// ──────────────────────────────────────────────────────────────────────
// Internal filter clause
// ──────────────────────────────────────────────────────────────────────

struct FilterClause {
    col_name: &'static str,
    op:       FilterOp,
    value:    Option<ParamValue>,
}

// ──────────────────────────────────────────────────────────────────────
// Query<E> – the builder
// ──────────────────────────────────────────────────────────────────────

/// A lazily-built SELECT query for entity `E`.
pub struct Query<E: DbEntity> {
    filters:  Vec<FilterClause>,
    order_by: Option<(&'static str, bool)>,
    limit:    Option<usize>,
    offset:   Option<usize>,
    _entity:  PhantomData<fn() -> E>,
}

impl<E: DbEntity> Query<E> {
    pub fn new() -> Self {
        Self {
            filters: Vec::new(),
            order_by: None,
            limit: None,
            offset: None,
            _entity: PhantomData,
        }
    }

    #[inline]
    pub fn filter(mut self, col: usize, op: FilterOp, value: ParamValue) -> Self {
        debug_assert!(col < E::entity_meta().columns.len(), "column index out of bounds");
        let col_name = E::entity_meta().columns[col].name;
        self.filters.push(FilterClause {
            col_name,
            op,
            value: if op.needs_value() { Some(value) } else { None },
        });
        self
    }

    #[inline]
    pub fn filter_null(mut self, col: usize, op: FilterOp) -> Self {
        debug_assert!(!op.needs_value(), "use filter() for operators with a value");
        let col_name = E::entity_meta().columns[col].name;
        self.filters.push(FilterClause { col_name, op, value: None });
        self
    }

    #[inline]
    pub fn order_by(mut self, col: usize, ascending: bool) -> Self {
        self.order_by = Some((E::entity_meta().columns[col].name, ascending));
        self
    }

    #[inline]
    pub fn limit(mut self, n: usize) -> Self { self.limit = Some(n); self }

    #[inline]
    pub fn offset(mut self, n: usize) -> Self { self.offset = Some(n); self }

    // ── sqlx execution ───────────────────────────────────────────────

    pub async fn fetch_all(
        &self,
        pool: &sqlx::PgPool,
    ) -> sqlx::Result<Vec<E>>
    where
        E: for<'r> sqlx::FromRow<'r, sqlx::postgres::PgRow> + Send + Unpin,
    {
        if self.is_passthrough() {
            return sqlx::query_as::<sqlx::Postgres, E>(E::entity_meta().find_all_sql)
                .persistent(true)
                .fetch_all(pool)
                .await;
        }
        let (sql, args) = self.build_sql_and_args();
        sqlx::query_as_with::<sqlx::Postgres, E, _>(&sql, args)
            .persistent(true)
            .fetch_all(pool)
            .await
    }

    pub async fn fetch_one(
        &self,
        pool: &sqlx::PgPool,
    ) -> sqlx::Result<E>
    where
        E: for<'r> sqlx::FromRow<'r, sqlx::postgres::PgRow> + Send + Unpin,
    {
        if self.is_passthrough() {
            return sqlx::query_as::<sqlx::Postgres, E>(E::entity_meta().find_all_sql)
                .persistent(true)
                .fetch_one(pool)
                .await;
        }
        let (sql, args) = self.build_sql_and_args();
        sqlx::query_as_with::<sqlx::Postgres, E, _>(&sql, args)
            .persistent(true)
            .fetch_one(pool)
            .await
    }

    pub async fn fetch_optional(
        &self,
        pool: &sqlx::PgPool,
    ) -> sqlx::Result<Option<E>>
    where
        E: for<'r> sqlx::FromRow<'r, sqlx::postgres::PgRow> + Send + Unpin,
    {
        if self.is_passthrough() {
            return sqlx::query_as::<sqlx::Postgres, E>(E::entity_meta().find_all_sql)
                .persistent(true)
                .fetch_optional(pool)
                .await;
        }
        let (sql, args) = self.build_sql_and_args();
        sqlx::query_as_with::<sqlx::Postgres, E, _>(&sql, args)
            .persistent(true)
            .fetch_optional(pool)
            .await
    }

    // ── private helpers ──────────────────────────────────────────────

    fn is_passthrough(&self) -> bool {
        self.filters.is_empty() && self.order_by.is_none()
            && self.limit.is_none() && self.offset.is_none()
    }

    fn build_sql_and_args(&self) -> (String, sqlx::postgres::PgArguments) {
        use sqlx::Arguments as _;

        let base_len = E::entity_meta().find_all_sql.len();
        let extra = self.filters.len() * 32
            + if self.order_by.is_some() { 32 } else { 0 }
            + if self.limit.is_some()    { 16 } else { 0 }
            + if self.offset.is_some()   { 16 } else { 0 };
        let mut sql = String::with_capacity(base_len + extra);
        sql.push_str(E::entity_meta().find_all_sql);

        let mut args = sqlx::postgres::PgArguments::default();
        let mut param_idx = 1usize;

        if !self.filters.is_empty() {
            sql.push_str(" WHERE ");
            let mut first = true;
            for f in &self.filters {
                if !first { sql.push_str(" AND "); }
                first = false;

                if f.op.needs_value() {
                    sql.push_str(f.col_name);
                    sql.push(' ');
                    sql.push_str(f.op.as_sql());
                    sql.push_str(" $");
                    push_usize(&mut sql, param_idx);
                    param_idx += 1;
                    if let Some(v) = &f.value {
                        match v {
                            ParamValue::I64(v)  => args.add(*v).expect("bind i64"),
                            ParamValue::F64(v)  => args.add(*v).expect("bind f64"),
                            ParamValue::Bool(v) => args.add(*v).expect("bind bool"),
                            ParamValue::Text(v) => args.add(v.as_str()).expect("bind text"),
                            ParamValue::Null    => args.add(Option::<String>::None).expect("bind null"),
                        }
                    }
                } else {
                    sql.push_str(f.col_name);
                    sql.push(' ');
                    sql.push_str(f.op.as_sql());
                }
            }
        }

        if let Some((col, asc)) = self.order_by {
            sql.push_str(" ORDER BY ");
            sql.push_str(col);
            sql.push_str(if asc { " ASC" } else { " DESC" });
        }
        if let Some(n) = self.limit {
            sql.push_str(" LIMIT ");
            push_usize(&mut sql, n);
        }
        if let Some(n) = self.offset {
            sql.push_str(" OFFSET ");
            push_usize(&mut sql, n);
        }

        (sql, args)
    }
}

impl<E: DbEntity> Default for Query<E> {
    fn default() -> Self { Self::new() }
}

// ──────────────────────────────────────────────────────────────────────
// Helpers
// ──────────────────────────────────────────────────────────────────────

#[inline]
fn push_usize(s: &mut String, mut n: usize) {
    if n == 0 {
        s.push('0');
        return;
    }
    let mut buf = [0u8; 20];
    let mut len = 0;
    while n > 0 {
        buf[len] = b'0' + (n % 10) as u8;
        n /= 10;
        len += 1;
    }
    buf[..len].reverse();
    // SAFETY: every byte is ASCII digit 0x30..=0x39.
    s.push_str(unsafe { std::str::from_utf8_unchecked(&buf[..len]) });
}
