// lib.rs – comptime_web framework root.
//
// Feature flags:
//   nightly  – enables `std::mem::type_info` compile-time reflection
//              (requires nightly Rust toolchain, enabled by default via
//              rust-toolchain.toml).
//
// Architecture summary (Agents.md §20):
// ┌────────────────────────────────────────────────────────────────────┐
// │  COMPILE TIME                                                       │
// │    proc-macros scan #[get]/#[post]/… attributes                    │
// │    → emit inventory::submit! route registrations                   │
// │    → #[derive(JsonSchema)] generates schema impls via type_info     │
// │                                                                     │
// │  STARTUP  (O(routes), happens once)                                 │
// │    inventory::iter collects all RouteRegistration records           │
// │    → build_router() constructs static &'static RouterNode trie      │
// │    → build_openapi() constructs static OPENAPI_SPEC string          │
// │                                                                     │
// │  RUNTIME  (O(path depth) per request, zero allocations on path)    │
// │    dispatch() traverses trie → calls HandlerFn                     │
// └────────────────────────────────────────────────────────────────────┘
//
// Migration note (Agents.md §16):
//   • inventory → #[comptime] once that feature lands in nightly
//   • proc-macros → comptime reflection once crate-level scanning is available
//   • box-leak trie → true static initialiser once const-alloc stabilises
//   Public developer API (#[get], #[post], Server::run, …) stays identical.

// ── nightly feature gates (nightly ≥ 1.94 required) ────────────────
// std::mem::type_info is not yet stable; tracked at rust-lang/rust#146922.
#![feature(type_info)]
#![feature(impl_trait_in_assoc_type)]

pub mod error;
pub mod handler;
pub mod middleware;
pub mod openapi;
pub mod params;
pub mod registry;
pub mod request;
pub mod response;
pub mod router;
pub mod schema;
pub mod server;

// ── ORM crate re-exported as `db` for backwards compatibility ────────
pub use comptime_orm as db;

// ── DI crate re-exported as `di` ────────────────────────────────────
pub use comptime_di as di;

// ── convenient top-level re-exports ──────────────────────────────────

pub use error::FrameworkError;
pub use handler::{BoxFuture, Handler, HandlerFn};
pub use middleware::Middleware;
pub use params::{extract_param, FromParam, FromRequest, Json, ParamInfo, ParamSource};
pub use registry::{all_routes, RouteRegistration};
pub use request::{HttpMethod, Request};
pub use response::{Response, StatusCode};
pub use router::{dispatch, HybridRouter, MethodHandler, PathSegment};
pub use schema::{JsonSchema, SchemaNode};
pub use server::{router, Server};

// ── re-export macros from the companion crate ─────────────────────────
pub use comptime_web_macros::{
    delete, get, patch, post, put,
    controller, middleware as middleware_attr,
    JsonSchema as DeriveJsonSchema,
};

// ── re-export Entity derive from comptime_orm ─────────────────────────
pub use comptime_orm::Entity as DeriveEntity;

// ── re-export inventory for use in generated code ────────────────────
pub use inventory;

// ── type_info helpers (unconditional – nightly ≥ 1.94) ─────────────
pub use schema::{type_info_kind, type_info_size, struct_field_names_runtime};

// ── database pool helpers ────────────────────────────────────────────
pub use db::pool::{init_pool, pool, try_pool, warm_all_statements, PoolConfig};
// ── schema migration ─────────────────────────────────────────────────
pub use db::migration::{run_migrations, SchemaStrategy, ACTIVE_STRATEGY};
