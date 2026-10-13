# Proof Of Concept Status

## Verified

- The workspace points to the actual local Axum and SeaORM checkouts.
- The `ct-rustc` fork reflects tool attributes on types, fields, and local function items.
- `crate_functions_with_attr` discovers attributed, non-generic functions in the crate being compiled.
- Function metadata includes the source name, parameter types, signature output type, declared output when available, and asyncness.
- `ct-web::router` provides static route-tree types, allocation-free traversal, and a generic Tower `Service` adapter whose concrete dispatch future is supplied by a closure.
- `StaticService::into_make_service()` uses Tower's `make::Shared` to clone the service per connection for `axum::serve`.
- Applications invoke `core::mem::type_info::crate_functions_with_attr` directly in compile-time contexts; ct-web defines no route-discovery macros. `route_metadata_array` and `middleware_metadata_array` turn those reflected function slices into fixed-size const arrays in definition order, without runtime collection or heap allocation.
- The compiler UI tests `function_items.rs` and `attributes_di_graph.rs` pass together with stage 1.
- The new `ct-di` crate reflects component, singleton, and config types, checks that every `#[app::inject]` field points to a registered type, and rejects cycles among component/singleton fields. The simple example runs with a typed explicit bootstrap and Axum state; its database, repository, and service dependencies are validated at compile time.
- `examples/simple` was smoke-tested with in-memory SQLite across create (201), list/get/update (200), delete (204), and missing-record (404) requests.

## Not Yet Implemented

- Function parameter names and binding patterns are not reflected.
- `FnDef.output` remains the lowered output type; `FnDef.declared_output` exposes `Future::Output` when rustc can recover it.
- Generic-item and dependency-wide discovery are unavailable.
- The compiler has no supported mechanism for emitting new typed items from reflection results.
- The route tree carries route IDs, not callable handlers. The Axum request adapter and serve integration exist, but route-to-handler dispatch and middleware composition are not generated from reflected functions.
- Axum's generic `Handler` adapter boxes futures; the zero-box path must use generated/direct async dispatch rather than routing through `Handler`.
- The const arrays contain optional metadata and reflected `TypeId`s, not callable handler references. They are not yet folded into a generated route trie; applications still need to author the static tree and dispatch closure.
- `ct-di` validates dependency edges but does not yet generate constructors, initialization order, or an application state type. The simple example therefore wires the providers explicitly at startup.
- The target `#[app::ListCrudRepository]` entity annotation is demonstrated in `examples/simple`, but its repository generation is not implemented. The example currently has a handwritten SeaORM repository with `find_all`, `find_by_id`, `insert`, `update`, and `delete_by_id` operations.
- DTO-to-entity mapping is currently handwritten as `From<NewUser> for ActiveModel`. Add compiler-owned comptime generation for this conversion and validate field compatibility before passing the mapped value to `UserService`.
- Compiler reflection can discover entity attributes, but typed repository methods, constructor/bootstrap generation, and static route dispatch remain unimplemented. Keep the working explicit bootstrap as an executable reference until generated items are supported.

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

Run the DI reflection tests and the executable example checks with the custom toolchain:

```sh
CARGO_TARGET_DIR=/tmp/ct-web-di-target cargo +ct-rustc test -p ct-di --tests
CARGO_TARGET_DIR=/tmp/ct-web-simple-target cargo +ct-rustc check -p simple-example --bin simple-example
```

The simple example is runnable, but provider construction, repository generation, DTO mapping, and static route dispatch remain explicit or handwritten until compiler-owned item generation exists. Do not replace the target design with linker registries or runtime type maps.