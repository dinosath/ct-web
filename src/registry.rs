// src/registry.rs – inventory-based route registration.
//
// Agents.md §16 (migration strategy):
//   Macros register routes via `inventory::submit!` at program start.
//   Once `#[comptime]` lands, this is replaced by a compile-time scan.
//
// Every `#[get]` / `#[post]` / … annotation generates:
//
//   inventory::submit! {
//       RouteRegistration {
//           method:  HttpMethod::Get,
//           path:    "/users",
//           handler: list_users_comptime_handler,
//       }
//   }
//
// At startup, `inventory::iter::<RouteRegistration>()` yields all routes
// collected from every linked compilation unit (including transitive deps).

use crate::{handler::HandlerFn, request::HttpMethod};

// ──────────────────────────────────────────────────────────────────────
// Route registration record
// ──────────────────────────────────────────────────────────────────────

/// A single route registered at program startup.
///
/// This struct is `inventory::Collect`-able so that `inventory::submit!`
/// can accumulate all registrations across the entire crate graph.
pub struct RouteRegistration {
    pub method:  HttpMethod,
    pub path:    &'static str,
    pub handler: HandlerFn,
    /// Parameter metadata generated at compile time by the route proc-macro.
    /// Each entry carries the parameter name, source, JSON kind (from
    /// `std::mem::type_info`), and a function pointer to
    /// `<T as JsonSchema>::schema` for OpenAPI generation.
    pub params:  &'static [crate::params::ParamInfo],
}

inventory::collect!(RouteRegistration);

// ──────────────────────────────────────────────────────────────────────
// Accessors
// ──────────────────────────────────────────────────────────────────────

/// Iterate over all routes registered via `inventory::submit!`.
///
/// Called once during `Server::run()` to build the static trie and the
/// OpenAPI document.
pub fn all_routes() -> impl Iterator<Item = &'static RouteRegistration> {
    inventory::iter::<RouteRegistration>()
}
