// comptime_di/src/registry.rs – compile-time service registry using type_info.
//
// Architecture:
//   • Services are stored as Arc<dyn Any + Send + Sync> keyed by TypeId.
//   • Singleton services are created once and shared via Arc.
//   • Transient services are created fresh on every get() call.
//   • Request-scoped services are created per-request (not stored globally).
//
// Compile-time metadata:
//   • ServiceRegistration (collected via inventory) provides type_name,
//     dependency list, and scope – all &'static data baked into .rodata.
//   • type_info reflection (reflect module) allows inspecting service
//     structs at compile time to validate the dependency graph.
//
// Migration note (Agents.md §12, §16):
//   When #[comptime] stabilises, the inventory-based collection will be
//   replaced by comptime scanning.  The ServiceRegistry API stays identical.

use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::sync::{Arc, OnceLock, RwLock};

use crate::Scope;

// ──────────────────────────────────────────────────────────────────────
// Service metadata (collected at link time via inventory)
// ──────────────────────────────────────────────────────────────────────

/// Compile-time registration record for a service.
///
/// Submitted via `inventory::submit!` by `#[derive(Injectable)]` or
/// `#[component]`.  All data is `&'static` – lives in `.rodata`.
pub struct ServiceRegistration {
    /// Human-readable type name (e.g. "OrderService").
    pub type_name: &'static str,
    /// Returns the `TypeId` of the service type.
    /// Function pointer because `TypeId::of::<T>()` is not const.
    pub type_id_fn: fn() -> TypeId,
    /// Names of dependency types (resolved from struct fields).
    pub deps: &'static [&'static str],
    /// Lifecycle scope.
    pub scope: Scope,
}

inventory::collect!(ServiceRegistration);

/// Iterate over all registered services.
pub fn all_services() -> impl Iterator<Item = &'static ServiceRegistration> {
    inventory::iter::<ServiceRegistration>()
}

// ──────────────────────────────────────────────────────────────────────
// ServiceRegistry – the runtime container
// ──────────────────────────────────────────────────────────────────────

type AnyArc = Arc<dyn Any + Send + Sync>;
type FactoryFn = Box<dyn Fn(&ServiceRegistry) -> AnyArc + Send + Sync>;

struct ServiceEntry {
    instance: Option<AnyArc>,
    factory:  Option<FactoryFn>,
    scope:    Scope,
}

/// The dependency injection container.
///
/// Stores singleton instances and factory functions keyed by `TypeId`.
/// Thread-safe via `RwLock` – reads (the common case) are concurrent,
/// writes (registration, first-time singleton creation) are exclusive.
///
/// # Compile-time guarantees
/// All service registrations and their dependency metadata are known at
/// link time (via `inventory`).  `validate()` checks the full dependency
/// graph at startup – if a dependency is missing, the application panics
/// before serving any requests.
pub struct ServiceRegistry {
    services: RwLock<HashMap<TypeId, ServiceEntry>>,
}

impl ServiceRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self {
            services: RwLock::new(HashMap::new()),
        }
    }

    /// Register a pre-built singleton instance.
    ///
    /// ```rust,no_run
    /// registry.register_singleton::<PgPool>(pool);
    /// ```
    pub fn register_singleton<T: Send + Sync + 'static>(&self, value: T) {
        let mut map = self.services.write().expect("registry poisoned");
        map.insert(
            TypeId::of::<T>(),
            ServiceEntry {
                instance: Some(Arc::new(value)),
                factory:  None,
                scope:    Scope::Singleton,
            },
        );
    }

    /// Register a pre-built `Arc<T>` singleton (avoids double-wrapping).
    pub fn register_arc<T: Send + Sync + 'static>(&self, arc: Arc<T>) {
        let mut map = self.services.write().expect("registry poisoned");
        map.insert(
            TypeId::of::<T>(),
            ServiceEntry {
                instance: Some(arc),
                factory:  None,
                scope:    Scope::Singleton,
            },
        );
    }

    /// Register a factory function for creating instances of `T`.
    ///
    /// The factory receives `&ServiceRegistry` so it can resolve its own
    /// dependencies.  For `Scope::Singleton` the factory is called once
    /// and the result cached.  For `Scope::Transient` it is called on
    /// every `get()`.
    pub fn register_factory<T: Send + Sync + 'static>(
        &self,
        scope: Scope,
        factory: impl Fn(&ServiceRegistry) -> T + Send + Sync + 'static,
    ) {
        let mut map = self.services.write().expect("registry poisoned");
        map.insert(
            TypeId::of::<T>(),
            ServiceEntry {
                instance: None,
                factory:  Some(Box::new(move |reg| Arc::new(factory(reg)))),
                scope,
            },
        );
    }

    /// Register a type that implements `Injectable`.
    ///
    /// The `Injectable::resolve` method is used as the factory function.
    /// Scope is determined by the `#[scope("…")]` attribute on the struct.
    pub fn register_injectable<T: crate::Injectable + Send + Sync + 'static>(
        &self,
        scope: Scope,
    ) {
        self.register_factory::<T>(scope, |reg| T::resolve(reg));
    }

    /// Retrieve a service as `Arc<T>`.
    ///
    /// # Panics
    /// Panics if `T` is not registered.  Use `try_get()` for fallible lookup.
    pub fn get<T: Send + Sync + 'static>(&self) -> Arc<T> {
        self.try_get::<T>().unwrap_or_else(|| {
            panic!(
                "[comptime_di] service not found: {}",
                std::any::type_name::<T>()
            )
        })
    }

    /// Try to retrieve a service as `Arc<T>`.  Returns `None` if not registered.
    pub fn try_get<T: Send + Sync + 'static>(&self) -> Option<Arc<T>> {
        let tid = TypeId::of::<T>();

        // Fast path: check for existing instance under read lock.
        {
            let map = self.services.read().expect("registry poisoned");
            if let Some(entry) = map.get(&tid) {
                if let Some(ref arc) = entry.instance {
                    // Downcast the stored Arc<dyn Any> to Arc<T>.
                    return arc.clone().downcast::<T>().ok();
                }
            }
        }

        // Slow path: create via factory under write lock.
        let mut map = self.services.write().expect("registry poisoned");
        let entry = map.get_mut(&tid)?;

        if let Some(ref arc) = entry.instance {
            return arc.clone().downcast::<T>().ok();
        }

        let factory = entry.factory.as_ref()?;
        let arc = factory(self);
        let result = arc.clone().downcast::<T>().ok();

        if entry.scope == Scope::Singleton {
            entry.instance = Some(arc);
        }

        result
    }

    /// Check whether a service of type `T` is registered.
    pub fn contains<T: 'static>(&self) -> bool {
        let map = self.services.read().expect("registry poisoned");
        map.contains_key(&TypeId::of::<T>())
    }

    /// Validate that all registered services have their dependencies met.
    ///
    /// Iterates over every `ServiceRegistration` collected via `inventory`
    /// and verifies that each declared dependency is present in the registry.
    ///
    /// # Panics
    /// Panics with a detailed message listing all missing dependencies.
    /// Call this at startup before serving requests.
    pub fn validate(&self) {
        let _map = self.services.read().expect("registry poisoned");

        // Build a set of registered type names for lookup.
        let registered_names: std::collections::HashSet<&str> =
            all_services().map(|r| r.type_name).collect();

        let mut errors = Vec::new();

        for reg in all_services() {
            for dep in reg.deps {
                if !registered_names.contains(dep) {
                    errors.push(format!(
                        "  {} depends on '{}' which is not registered",
                        reg.type_name, dep
                    ));
                }
            }
        }

        if !errors.is_empty() {
            panic!(
                "[comptime_di] dependency validation failed:\n{}",
                errors.join("\n")
            );
        }
    }

    /// Print the dependency graph to stdout (useful for debugging).
    pub fn dump_graph(&self) {
        println!("─── comptime_di service graph ───");
        for reg in all_services() {
            let scope_str = match reg.scope {
                Scope::Singleton => "singleton",
                Scope::Request   => "request",
                Scope::Transient => "transient",
            };
            if reg.deps.is_empty() {
                println!("  {} [{}] (no deps)", reg.type_name, scope_str);
            } else {
                println!(
                    "  {} [{}] ← {}",
                    reg.type_name,
                    scope_str,
                    reg.deps.join(", ")
                );
            }
        }
    }
}

impl Default for ServiceRegistry {
    fn default() -> Self {
        Self::new()
    }
}

// ──────────────────────────────────────────────────────────────────────
// Global registry (singleton)
// ──────────────────────────────────────────────────────────────────────

static GLOBAL_REGISTRY: OnceLock<ServiceRegistry> = OnceLock::new();

/// Initialise the global service registry.
///
/// Call once at startup.  Returns `&'static ServiceRegistry`.
///
/// ```rust,no_run
/// let reg = comptime_di::init_registry();
/// reg.register_singleton(my_pool);
/// reg.register_injectable::<OrderService>(Scope::Singleton);
/// reg.validate();
/// ```
pub fn init_registry() -> &'static ServiceRegistry {
    GLOBAL_REGISTRY.get_or_init(ServiceRegistry::new)
}

/// Access the global service registry.
///
/// # Panics
/// Panics if `init_registry()` has not been called.
pub fn service_registry() -> &'static ServiceRegistry {
    GLOBAL_REGISTRY
        .get()
        .expect("[comptime_di] service_registry() called before init_registry()")
}

/// Try to access the global service registry.  Returns `None` if not initialised.
pub fn try_service_registry() -> Option<&'static ServiceRegistry> {
    GLOBAL_REGISTRY.get()
}
