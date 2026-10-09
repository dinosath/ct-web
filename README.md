# ct-web

An experimental Rust web framework POC exploring native compiler reflection and compile-time route generation. The workspace uses the local `ct-rustc` compiler fork, the real Axum and SeaORM checkouts, and a `simple` example crate.

The framework now includes a static route tree, a Tower `Service` adapter with a concrete dispatch future, and compile-time reflection readers that materialize route and middleware metadata as fixed-size const arrays. Applications call the compiler reflection API directly; ct-web does not define route-discovery macros. The service can be passed to `axum::serve` through `into_make_service()`. The compiler does not yet emit handler wrappers or a router tree from those descriptors, so route-to-handler dispatch and middleware composition remain manual.

See [POC.md](POC.md) for verified capabilities and [ARCHITECTURE.md](ARCHITECTURE.md) for the intended compilation and runtime boundaries.