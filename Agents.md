# Compile‑Time Rust Web Framework – Implementation Guide (Future `type_info` + `comptime`)

This document guides an autonomous coding agent on how to evolve a compile‑time web framework that initially uses macros and registries into a reflection‑based system using Rust's upcoming:

* `std::mem::type_info`
* `#[comptime]` compile‑time functions
* crate reflection APIs (future)

The goal is to eliminate:

* procedural macros
* linker registries
* runtime router construction

And replace them with **pure compile‑time code generation**.

---

# 1. Final Goal

The framework must provide:

* zero runtime router building
* compile‑time route discovery
* compile‑time middleware composition
* compile‑time OpenAPI generation
* static routing tables

Runtime startup should only:

1. start async runtime
2. bind socket
3. dispatch requests

---

# 2. Target Developer API

Developers should write:

```rust
#[get("/users")]
async fn list_users() {}

#[get("/users/:id")]
async fn get_user(id: u64) {}

#[post("/orders")]
async fn create_order(order: CreateOrder) {}
```

Controllers optional:

```rust
#[controller("/users")]
impl UsersController {

    #[get]
    async fn list() {}

    #[post]
    async fn create() {}
}
```

The framework generates a static router automatically.

---

# 3. High Level Compilation Pipeline

Compilation pipeline should look like:

```
crate compilation
    ↓
comptime reflection scan
    ↓
route metadata extraction
    ↓
router tree generation
    ↓
code generation
    ↓
static ROUTER object
```

No runtime registration must exist.

---

# 4. Reflection Phase

At compile time, run a reflection pass:

```rust
#[comptime]
fn scan_routes() -> Vec<RouteMeta>
```

Agent must iterate through all crate functions.

Pseudo API (future):

```rust
for func in reflection::crate_functions() {

    if func.has_attribute("get") {
        register_route(func, Method::GET)
    }

    if func.has_attribute("post") {
        register_route(func, Method::POST)
    }

}
```

Extract:

* function name
* attributes
* path string
* parameter list
* return type

Store in `RouteMeta`.

---

# 5. Route Metadata Structure

Agent must create:

```rust
struct RouteMeta {
    method: HttpMethod,
    path: &'static str,
    handler: FunctionInfo,
    parameters: Vec<ParamInfo>,
}
```

Parameter info:

```rust
struct ParamInfo {
    name: &'static str,
    ty: TypeInfo,
    source: ParamSource
}
```

ParamSource:

```
Path
Query
Body
Header
```

---

# 6. Path Parsing

Routes must be parsed at compile time.

Example:

```
/users/:id
```

Convert to:

```
["users", Param("id")]
```

Structure:

```rust
struct PathSegment {
    Static(&'static str),
    Param(&'static str)
}
```

---

# 7. Router Tree Generation

Agent must build a trie router:

```
/
 └ users
     ├ GET
     └ :id
```

Structure:

```rust
struct RouterNode {

    segment: PathSegment,

    static_children: &'static [RouterNode],

    param_child: Option<&'static RouterNode>,

    method_handlers: &'static [MethodHandler]
}
```

Method handler:

```rust
struct MethodHandler {

    method: HttpMethod,

    handler: HandlerFn

}
```

All nodes must be static.

---

# 8. Code Generation

Agent must generate Rust code for router.

Example output:

```rust
static ROUTER: RouterNode = RouterNode {
    segment: Root,
    static_children: &[
        RouterNode {
            segment: Static("users"),
            static_children: &[],
            param_child: Some(&USER_ID_NODE),
            method_handlers: &[
                MethodHandler {
                    method: GET,
                    handler: list_users
                }
            ]
        }
    ]
};
```

Code generation must occur during compilation.

---

# 9. Dispatch Function

Runtime dispatcher must be simple.

Example:

```rust
fn dispatch(req: Request) -> Response {

    let segments = req.path_segments();

    router_lookup(&ROUTER, segments, req.method)

}
```

Router traversal must avoid allocations.

---

# 10. Parameter Extraction

Agent must generate parameter parsing code.

Example:

```
/users/:id
```

Generated code:

```rust
let id: u64 = segment.parse()?;
```

Generated handler wrapper:

```rust
async fn get_user_wrapper(req: Request) {

    let id = parse_segment::<u64>(req.segment(1));

    get_user(id).await

}
```

---

# 11. Middleware Compilation

Middleware attributes must generate wrapper functions.

Example:

```rust
#[middleware(Auth)]
#[get("/users")]
async fn list_users() {}
```

Generated code:

```rust
async fn list_users_wrapped(req: Request) {

    auth_middleware(req, list_users).await

}
```

Middleware chain must be compiled statically.

---

# 12. Dependency Injection

Agent must detect handler parameters that are services.

Example:

```rust
async fn create_order(service: OrderService)
```

At compile time:

* detect service type
* register it

Generate service container:

```rust
static SERVICES: ServiceRegistry
```

Runtime lookup must be constant time.

---

# 13. OpenAPI Generation

Agent must generate OpenAPI during compilation.

Input:

* route metadata
* parameter types
* response types

Output:

```
openapi.json
```

Stored as:

```rust
static OPENAPI_SPEC: &str
```

---

# 14. Type Reflection Usage

Agent must use `type_info` to inspect types.

Example:

```rust
let info = type_of::<CreateOrder>();
```

If struct:

```
inspect fields
extract names
extract field types
```

Used for:

* request body parsing
* schema generation

---

# 15. Performance Requirements

Router must guarantee:

* zero runtime allocations
* O(depth) routing
* no hashmap lookup

Expected startup:

```
1‑5 ms
```

---

# 16. Migration Strategy

Use rust nightly features where possible. For example https://doc.rust-lang.org/nightly/std/mem/type_info/index.html

Follow https://github.com/rust-lang/rust-project-goals/issues/406. When #[comptime] land in nightly use it to replace build.rs, inventory 

When reflection stabilizes replace:

```
macros → reflection scanning
inventory → compile‑time collection
build.rs → comptime functions
```

Public API must remain identical.

---

# 17. Future Enhancements

Possible extensions:

* GraphQL auto generation
* admin dashboard
* automatic database migrations
* compile‑time validation

All based on type reflection.

---

# 18. Implementation Order

Agent should implement in this order:

1. static router types
2. route metadata structures
3. reflection scanner
4. router tree builder
5. code generator
6. dispatch function
7. parameter extraction
8. middleware wrappers
9. OpenAPI generator

---

# 19. Safety Requirements

The system must guarantee:

* type safe handler invocation
* compile‑time validation of routes
* no dynamic reflection

Errors must be reported during compilation.

---

# 20. End State

Final architecture:

```
compile time
-------------
route discovery
router generation
schema generation
middleware compilation

runtime
-------
start runtime
bind socket
dispatch requests
```

The server must perform **almost no initialization work at runtime**.
