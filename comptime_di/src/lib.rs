// comptime_di – compile-time dependency injection framework for Rust.
//
// Uses nightly std::mem::type_info for compile-time service introspection.
//
// Architecture (Agents.md §12):
//   • Handler parameters that are service types are detected at compile time
//   • Services are registered in a static ServiceRegistry
//   • Runtime lookup is O(1) via TypeId → Arc<dyn Any> HashMap
//   • Dependency graph is validated at startup
//
// Macros:
//   #[derive(Injectable)] – auto-implement dependency resolution for structs
//   #[component]          – register a factory function as a service provider
//   #[inject]             – resolve function parameters from the registry
//
// Requires nightly Rust (≥ 1.94) for std::mem::type_info.
//
// Migration note (Agents.md §16):
//   When #[comptime] stabilises, inventory-based collection will be replaced
//   by comptime scanning.  The public API stays identical.

#![feature(type_info)]
#![feature(const_type_name)]

pub mod reflect;
pub mod registry;

// ── Scope enum ─────────────────────────────────────────────────────

/// Service lifecycle scope.
///
/// Determines how many instances of a service are created.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    /// One instance shared across the entire application.
    Singleton,
    /// One instance per request (created and dropped with the request).
    Request,
    /// New instance on every resolution.
    Transient,
}

// ── Injectable trait ───────────────────────────────────────────────

/// Trait for types that can be constructed by resolving their
/// dependencies from a `ServiceRegistry`.
///
/// Implemented automatically by `#[derive(Injectable)]`.
///
/// # Example
/// ```rust,no_run
/// use comptime_di::{Injectable, ServiceRegistry};
///
/// struct OrderService {
///     user_repo: Arc<UserRepository>,
/// }
///
/// impl Injectable for OrderService {
///     fn resolve(registry: &ServiceRegistry) -> Self {
///         Self {
///             user_repo: registry.get::<UserRepository>(),
///         }
///     }
/// }
/// ```
pub trait Injectable: Sized {
    /// Construct an instance by resolving dependencies from the registry.
    fn resolve(registry: &ServiceRegistry) -> Self;

    /// Return the names of this type's dependencies.
    ///
    /// Used for compile-time dependency graph validation.
    fn dependencies() -> &'static [&'static str] {
        &[]
    }
}

// ── Re-exports ─────────────────────────────────────────────────────

pub use registry::{
    all_services, init_registry, service_registry, try_service_registry,
    ServiceRegistration, ServiceRegistry,
};

// ── Re-export macros ───────────────────────────────────────────────
pub use comptime_di_macros::{Injectable, component, inject};

// ── Re-export inventory for generated code ─────────────────────────
pub use inventory;
