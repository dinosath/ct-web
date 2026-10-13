# Architecture

## Current Boundary

`ct-rustc` owns compile-time reflection. `ct-di` validates reflected injection fields against application component, singleton, and config types. `ct-web` owns HTTP adapters, while the example owns constructors, entities, services, and configuration. Axum and SeaORM remain upstream dependencies rather than local substitutes.

Today, `ct-di` uses `ct-rustc` comptime reflection to reject unresolved `#[app::inject]` fields. The simple example explicitly constructs `DatabaseConfig -> Database -> Repository -> UserService` and runs CRUD handlers through Axum state. Its SeaORM repository and DTO conversion are handwritten; the `ListCrudRepository` and DTO attributes are target metadata, not generators yet. `ct-web::router` has static route-tree primitives and allocation-free traversal; `StaticService` implements Tower `Service` and adapts to `axum::serve`, but reflected routes are not yet connected to callable handlers.

The project target remains macro-free route/DI discovery and generation. Compiler-owned item generation is required to replace the example's explicit constructors, Axum runtime route assembly, repository implementation, and DTO conversion with generated typed code.

## Target Pipeline

```text
application types/functions and tool attributes
                 |
                 v
ct-rustc compile-time reflection
                 |
                 v
typed DI and route metadata validation
                 |
                 v
compiler-owned item generation
                 |
                 v
generated providers, repository, DTO mapping, and static dispatch
```

Compiler-owned code generation must create typed references to application handlers, handler-specific request extraction, and middleware composition without a runtime route registry, boxed handler futures, declarative macros, or procedural macros. The framework deliberately serves a Tower `Service` rather than building `axum::Router`: Axum's router is assembled through runtime APIs and stores dynamic routing state, while `axum::serve` accepts a custom service.

## Runtime Goal

Runtime startup should be limited to application initialization, such as loading configuration, connecting SeaORM, and binding the server. Route discovery and route-to-handler dispatch structure should be generated during compilation. `examples/simple/src/main.rs` currently demonstrates the explicit bootstrap needed until compiler-owned provider and handler generation is available.