# Proof Of Concept Status

## Verified

- The workspace points to the actual local Axum and SeaORM checkouts.
- The `ct-rustc` fork reflects tool attributes on types, fields, and local function items.
- `crate_functions_with_attr` discovers attributed, non-generic functions in the crate being compiled.
- Function metadata currently includes the source name, parameter types, signature output type, and asyncness.
- `ct-web::router` provides static route-tree types, allocation-free traversal, and a generic Tower `Service` adapter whose concrete dispatch future is supplied by a closure.
- `StaticService::into_make_service()` uses Tower's `make::Shared` to clone the service per connection for `axum::serve`.
- Applications invoke `core::mem::type_info::crate_functions_with_attr` directly in compile-time contexts; ct-web defines no route-discovery macros. `route_metadata_array` and `middleware_metadata_array` turn those reflected function slices into fixed-size const arrays in definition order, without runtime collection or heap allocation.
- The compiler UI tests `function_items.rs` and `attributes_di_graph.rs` pass together with stage 1.

## Not Yet Implemented

- Function parameter names and binding patterns are not reflected.
- Async function output currently describes the opaque future type.
- Generic-item and dependency-wide discovery are unavailable.
- The compiler has no supported mechanism for emitting new typed items from reflection results.
- The route tree carries route IDs, not callable handlers. The Axum request adapter and serve integration exist, but route-to-handler dispatch and middleware composition are not generated from reflected functions.
- Axum's generic `Handler` adapter boxes futures; the zero-box path must use generated/direct async dispatch rather than routing through `Handler`.
- The const arrays contain optional metadata and reflected `TypeId`s, not callable handler references. They are not yet folded into a generated route trie; applications still need to author the static tree and dispatch closure.
- `examples/simple` has no `main.rs` yet because no generated dispatch function exists to call.
- The target `#[app::ListCrudRepository]` entity annotation is demonstrated in `examples/simple`. It is intended to generate an entity-scoped `Repository` using SeaORM's `Entity`, `Model`, and `ActiveModel` types, with `find_all`, `find_by_id`, `insert`, `update`, and `delete_by_id` operations. The repository should be a compile-time DI component backed by the unique `DatabaseConnection` provider.
- DTO-to-entity mapping is currently handwritten in the controller. Add a compiler-native comptime generator (or an explicitly approved macro alternative) that generates a typed conversion from a request DTO to the corresponding SeaORM entity input (`ActiveModel` where required), validates compatible fields at compile time, and lets the controller pass the mapped value to `UserService`.
- Compiler reflection can discover entity attributes, but typed repository methods, provider graph validation, and generated bootstrap items are not implemented. The simple example therefore documents the target API and is not expected to compile until compiler-owned item generation is available.

The next compiler milestone is a small, typed item-generation vertical slice that turns these descriptors into handler wrappers and a static router. It must use compiler-native reflection and comptime, without declarative macros, procedural macros, or `build.rs`, before adding the requested example entry point.

## Validation

Run the compiler reflection tests from `~/Projects/rust`:

```sh
./x.py test tests/ui/reflection/function_items.rs tests/ui/reflection/attributes_di_graph.rs --stage 1
```

The direct framework checks are:

```sh
TOWER_SERVICE_SRC=$(find "$HOME/.cargo/registry/src" -type f -path '*/tower-service-*/src/lib.rs' -print -quit)
rustc +ct-rustc --edition=2018 --crate-type=lib --crate-name=tower_service "$TOWER_SERVICE_SRC" -o /tmp/libtower_service.rlib
rustc +ct-rustc --edition=2024 --test crates/ct-web/src/router.rs --extern tower_service=/tmp/libtower_service.rlib -o /tmp/ct-web-router-tests
/tmp/ct-web-router-tests
rustc +ct-rustc --edition=2024 --test crates/ct-web/tests/reflection.rs -o /tmp/ct-web-reflection-tests
/tmp/ct-web-reflection-tests
```

The simple example is an aspirational target-API sample. Keep the missing-code-generation limitation explicit; do not replace it with procedural macros, linker registries, or runtime router construction.