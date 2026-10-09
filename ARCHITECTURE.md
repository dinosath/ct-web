# Architecture

## Current Boundary

`ct-rustc` owns compile-time reflection. The `ct-web` crate is intended to own framework metadata and adapters, while the example crate owns application handlers, entities, services, and configuration. Axum and SeaORM remain upstream dependencies rather than local substitutes.

Today, applications can call the compiler reflection API directly from compile-time contexts, and comptime helpers can materialize reflected route and middleware metadata as fixed-size const arrays. The project must not use declarative or procedural macros for route discovery or generation. `ct-web::router` has static route-tree primitives and allocation-free traversal; `StaticService` implements Tower `Service` and `into_make_service()` adapts it for `axum::serve`. The route tree identifies routes by numeric IDs, and an application-supplied dispatch closure maps those IDs to responses. The descriptors contain `TypeId` values, not callable handler references. The compiler cannot emit handler wrappers or a router tree from them yet, so the existing sample attributes are not wired to generated dispatch.

## Target Pipeline

```text
application functions and tool attributes
                 |
                 v
ct-rustc compile-time reflection
                 |
                 v
typed route and handler metadata
                 |
                 v
compiler-owned item generation
                 |
                 v
generated static dispatch integrated with Axum
```

Compiler-owned code generation must create typed references to application handlers, handler-specific request extraction, and middleware composition without a runtime route registry, boxed handler futures, declarative macros, or procedural macros. The framework deliberately serves a Tower `Service` rather than building `axum::Router`: Axum's router is assembled through runtime APIs and stores dynamic routing state, while `axum::serve` accepts a custom service.

## Runtime Goal

Runtime startup should be limited to application initialization, such as loading configuration, connecting SeaORM, and binding the server. Route discovery and route-to-handler dispatch structure should be generated during compilation. A minimal `examples/simple/src/main.rs` will be added only after this generated-router API exists and compiles against the real Axum dependency.