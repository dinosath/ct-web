// comptime_orm/src/pool.rs – global Postgres connection pool, binary-protocol config,
// and prepared-statement warmup.

use std::sync::OnceLock;
use std::time::Duration;

use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use sqlx::PgPool;

use crate::entity::all_entities;

static POOL: OnceLock<PgPool> = OnceLock::new();

// ──────────────────────────────────────────────────────────────────────
// Pool configuration
// ──────────────────────────────────────────────────────────────────────

/// Connection pool and binary-protocol tuning parameters.
pub struct PoolConfig {
    pub min_connections: u32,
    pub max_connections: u32,
    pub acquire_timeout: Option<Duration>,
    pub idle_timeout: Option<Duration>,
    pub max_lifetime: Option<Duration>,
    pub statement_cache_capacity: usize,
}

impl Default for PoolConfig {
    fn default() -> Self {
        Self {
            min_connections:          1,
            max_connections:          32,
            acquire_timeout:          Some(Duration::from_secs(5)),
            idle_timeout:             Some(Duration::from_secs(600)),
            max_lifetime:             Some(Duration::from_secs(1800)),
            statement_cache_capacity: 256,
        }
    }
}

// ──────────────────────────────────────────────────────────────────────
// Initialisation
// ──────────────────────────────────────────────────────────────────────

/// Initialise the global connection pool from a Postgres URL.
pub async fn init_pool(url: &str, cfg: PoolConfig) -> sqlx::Result<()> {
    let mut opts: PgConnectOptions = url.parse()?;
    opts = opts.statement_cache_capacity(cfg.statement_cache_capacity);

    let mut pool_opts = PgPoolOptions::new()
        .min_connections(cfg.min_connections)
        .max_connections(cfg.max_connections);
    if let Some(d) = cfg.acquire_timeout {
        pool_opts = pool_opts.acquire_timeout(d);
    }
    if let Some(d) = cfg.idle_timeout {
        pool_opts = pool_opts.idle_timeout(d);
    }
    if let Some(d) = cfg.max_lifetime {
        pool_opts = pool_opts.max_lifetime(d);
    }

    let pool = pool_opts.connect_with(opts).await?;

    let _ = POOL.set(pool);
    Ok(())
}

// ──────────────────────────────────────────────────────────────────────
// Accessors
// ──────────────────────────────────────────────────────────────────────

/// Return the global pool.
///
/// # Panics
/// Panics if `init_pool` has not been called.
#[inline]
pub fn pool() -> &'static PgPool {
    POOL.get().expect("comptime_orm::pool() called before init_pool()")
}

/// Return `Some(&pool)` if available, `None` if `init_pool` was not called.
#[inline]
pub fn try_pool() -> Option<&'static PgPool> {
    POOL.get()
}

// ──────────────────────────────────────────────────────────────────────
// Prepared-statement warmup
// ──────────────────────────────────────────────────────────────────────

/// Pre-warm the prepared-statement cache for every registered entity.
pub async fn warm_all_statements() -> sqlx::Result<()> {
    let p = pool();

    for reg in all_entities() {
        let meta = reg.meta;

        let _ = sqlx::query(meta.find_all_sql)
            .persistent(true)
            .fetch_all(p)
            .await?;

        let _ = sqlx::query(meta.find_sql)
            .bind(0_i64)
            .persistent(true)
            .fetch_optional(p)
            .await?;
    }

    let mut tx = p.begin().await?;
    for reg in all_entities() {
        let meta = reg.meta;
        for sql in &[
            meta.insert_sql,
            meta.insert_returning_sql,
            meta.update_sql,
            meta.delete_sql,
        ] {
            let param_count = count_placeholders(sql);
            let mut q = sqlx::query(sql);
            for _ in 0..param_count {
                q = q.bind(0_i64);
            }
            let _ = q.persistent(true).execute(&mut *tx).await;
        }
    }
    tx.rollback().await?;

    Ok(())
}

// ──────────────────────────────────────────────────────────────────────
// Internal helpers
// ──────────────────────────────────────────────────────────────────────

fn count_placeholders(sql: &str) -> usize {
    let mut max = 0usize;
    let bytes = sql.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'$' {
            i += 1;
            let start = i;
            while i < bytes.len() && bytes[i].is_ascii_digit() {
                i += 1;
            }
            if i > start {
                if let Ok(n) = sql[start..i].parse::<usize>() {
                    if n > max { max = n; }
                }
            }
        } else {
            i += 1;
        }
    }
    max
}
