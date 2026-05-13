// comptime_orm – compile-time ORM for Rust using nightly type_info reflection.
//
// This crate provides:
//   • Entity metadata structures (EntityMeta, ColumnMeta, DbEntity trait)
//   • Compile-time SQL generation via #[derive(Entity)]
//   • Type-safe query builder (Query<E>)
//   • Connection pool management (init_pool, pool)
//   • Schema migration runner (run_migrations)
//   • type_info-based reflection utilities (reflect module)
//
// Requires nightly Rust (≥ 1.94) for std::mem::type_info.
//
// Migration note (Agents.md §16):
//   When #[comptime] stabilises, the proc-macro Entity derive will be
//   replaced by comptime functions using the reflect module's type_info
//   scanning.  The public API stays identical.

// ── nightly feature gates ──────────────────────────────────────────
#![feature(type_info)]

pub mod entity;
pub mod entity_info;
pub mod mapper;
pub mod migration;
pub mod page;
pub mod pool;
pub mod query;
pub mod reflect;
pub mod relation;
pub mod repository;

// ── Convenient re-exports ──────────────────────────────────────────

pub use entity::{
    all_entities, ColumnMeta, DbEntity, EntityMeta, EntityRegistration, SqlType,
};
pub use entity_info::{EntityInfo, FieldInfo, FieldKind, RuntimeEntity};
pub use mapper::ParamValue;
pub use migration::{run_migrations, SchemaStrategy, ACTIVE_STRATEGY};
pub use page::{Direction, Page, Pageable, Sort};
pub use pool::{init_pool, pool, try_pool, warm_all_statements, PoolConfig};
pub use query::{FilterOp, Query};
pub use relation::{ManyToMany, ManyToOne, OneToMany};
pub use repository::{CrudRepository, DataRepository, GenericRepository, PageableRepository};

/// Postgres connection pool type alias (re-exported from sqlx).
pub type PgPool = sqlx::PgPool;

// ── re-export the Entity + RuntimeEntity derive macros ────────────────
pub use comptime_orm_macros::Entity;
pub use comptime_orm_macros::RuntimeEntity;

// ── re-export inventory for generated code ───────────────────────────
pub use inventory;
