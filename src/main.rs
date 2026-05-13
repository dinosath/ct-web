// main.rs – example application using the comptime_web framework.
//
// Developer experience (Agents.md §2):
//   • Annotate async functions with #[get], #[post], etc.
//   • Optionally group them in #[controller] impl blocks.
//   • Call Server::run() – framework does the rest at startup.
//
// Zero runtime router building: the route registrations are linked into
// the binary by `inventory`; the trie is built once before serving starts.

// Required because #[derive(JsonSchema)] expands `::std::mem::type_info::Type::of`
// directly into this crate's code.
#![feature(type_info)]

use std::sync::Arc;

use comptime_web::*;
use comptime_web_macros::{get, post, delete, JsonSchema};
use comptime_orm::Entity;
use comptime_di::{Injectable, Scope};
use serde::{Deserialize, Serialize};

// ──────────────────────────────────────────────────────────────────────
// Domain types
// ──────────────────────────────────────────────────────────────────────

/// Database entity + API type for a user.
///
/// `#[derive(Entity)]` generates at compile time (discussion.md §3):
///   static USER_ENTITY_META  – all SQL strings embedded in .rodata
///   pub const COL_ID / COL_NAME / COL_EMAIL  – column index constants
///   impl DbEntity  – find_by_pk / find_all / insert / update / delete
///   inventory::submit!  – registers for migration runner / OpenAPI
///
/// `sqlx::FromRow` maps DB rows to this struct by column name.
#[derive(Debug, Serialize, Deserialize, JsonSchema, Entity)]
#[derive(sqlx::FromRow)]
pub struct User {
    #[pk]
    pub id:    i64,   // BIGINT – Postgres has no unsigned int
    pub name:  String,
    pub email: String,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct CreateUser {
    pub name:  String,
    pub email: String,
}

/// Database entity + API type for an order.
#[derive(Debug, Serialize, Deserialize, JsonSchema, Entity)]
#[derive(sqlx::FromRow)]
pub struct Order {
    #[pk]
    pub id:         i64,
    pub user_id:    i64,
    pub product_id: i64,
    pub quantity:   i32,   // INTEGER
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct CreateOrder {
    pub user_id:    i64,
    pub product_id: i64,
    pub quantity:   i32,
}

// ──────────────────────────────────────────────────────────────────────
// Route handlers  (Agents.md §2)
// ──────────────────────────────────────────────────────────────────────

/// GET /hello – list all users.
#[get("/hello")]
async fn hello(_req: &Request) -> Response {
    return Response { status: StatusCode::OK, headers: Default::default(), body: "Hello, world!".as_bytes().to_vec() };
}

/// GET /users – list all users.
#[get("/users")]
async fn list_users(_req: &Request) -> Response {
    match User::find_all(comptime_web::pool()).await {
        Ok(users) => Response::json(StatusCode::OK, &users),
        Err(e)    => Response::json(StatusCode::INTERNAL_SERVER_ERR,
                         &serde_json::json!({ "error": e.to_string() })),
    }
}

/// GET /users/:id – get a single user by ID.
#[get("/users/:id")]
async fn get_user(id: i64) -> Response {
    match User::find_by_pk(id, comptime_web::pool()).await {
        Ok(Some(u)) => Response::json(StatusCode::OK, &u),
        Ok(None)    => Response::json(StatusCode::NOT_FOUND,
                           &serde_json::json!({ "error": "not found" })),
        Err(e)      => Response::json(StatusCode::INTERNAL_SERVER_ERR,
                           &serde_json::json!({ "error": e.to_string() })),
    }
}

/// POST /users – create a new user.
#[post("/users")]
async fn create_user(req: &Request) -> Response {
    let payload = match serde_json::from_slice::<CreateUser>(&req.body) {
        Ok(p)  => p,
        Err(e) => return Response::json(StatusCode::BAD_REQUEST,
                      &serde_json::json!({ "error": e.to_string() })),
    };
    let user = User { id: 42, name: payload.name, email: payload.email };
    match user.insert(comptime_web::pool()).await {
        Ok(_)  => Response::json(StatusCode::CREATED, &user),
        Err(e) => Response::json(StatusCode::INTERNAL_SERVER_ERR,
                      &serde_json::json!({ "error": e.to_string() })),
    }
}

/// DELETE /users/:id – delete a user.
#[delete("/users/:id")]
async fn delete_user(id: i64) -> Response {
    let pool = comptime_web::pool();
    match User::find_by_pk(id, pool).await {
        Ok(Some(u)) => match u.delete(pool).await {
            Ok(_)  => Response::json(StatusCode::OK,
                          &serde_json::json!({ "deleted": id })),
            Err(e) => Response::json(StatusCode::INTERNAL_SERVER_ERR,
                          &serde_json::json!({ "error": e.to_string() })),
        },
        Ok(None) => Response::json(StatusCode::NOT_FOUND,
                        &serde_json::json!({ "error": "not found" })),
        Err(e)   => Response::json(StatusCode::INTERNAL_SERVER_ERR,
                        &serde_json::json!({ "error": e.to_string() })),
    }
}

/// GET /orders – list all orders.
#[get("/orders")]
async fn list_orders(_req: &Request) -> Response {
    match Order::find_all(comptime_web::pool()).await {
        Ok(orders) => Response::json(StatusCode::OK, &orders),
        Err(e)     => Response::json(StatusCode::INTERNAL_SERVER_ERR,
                          &serde_json::json!({ "error": e.to_string() })),
    }
}

/// POST /orders – create a new order.
#[post("/orders")]
async fn create_order(req: &Request) -> Response {
    let payload = match serde_json::from_slice::<CreateOrder>(&req.body) {
        Ok(p)  => p,
        Err(e) => return Response::json(StatusCode::BAD_REQUEST,
                      &serde_json::json!({ "error": e.to_string() })),
    };
    let order = Order {
        id:         1,
        user_id:    payload.user_id,
        product_id: payload.product_id,
        quantity:   payload.quantity,
    };
    match order.insert(comptime_web::pool()).await {
        Ok(_)  => Response::json(StatusCode::CREATED, &order),
        Err(e) => Response::json(StatusCode::INTERNAL_SERVER_ERR,
                      &serde_json::json!({ "error": e.to_string() })),
    }
}

/// GET /health – health check.
#[get("/health")]
async fn health(_req: &Request) -> Response {
    Response::json(StatusCode::OK, &serde_json::json!({ "status": "ok" }))
}

/// GET /openapi.json – serve the generated OpenAPI spec.
#[get("/openapi.json")]
async fn openapi_endpoint(req: &Request) -> Response {
    comptime_web::openapi::openapi_handler(req).await
}

// ──────────────────────────────────────────────────────────────────────
// Entity metadata demonstration  (discussion.md §3, §6)
// ──────────────────────────────────────────────────────────────────────

/// Print the compile-time generated SQL strings and column metadata.
/// All strings below are &'static str living in .rodata – zero runtime cost.
fn show_entity_meta() {
    use comptime_web::db::DbEntity;

    println!("─── compile-time entity metadata (User) ───");
    let meta = User::entity_meta();
    println!("  table        : {}", meta.table);
    println!("  find_sql     : {}", User::find_sql());
    println!("  find_all_sql : {}", User::find_all_sql());
    println!("  insert_sql   : {}", User::insert_sql());
    println!("  update_sql   : {}", User::update_sql());
    println!("  delete_sql   : {}", User::delete_sql());
    println!("  migration_sql: {}", User::migration_sql());
    println!("  columns:");
    for col in meta.columns {
        println!("    {:12} {:16} pk={} nullable={}",
            col.name, col.sql_type.as_sql_str(), col.is_pk, col.nullable);
    }

    println!();
    println!("─── compile-time entity metadata (Order) ───");
    let meta = Order::entity_meta();
    println!("  table        : {}", meta.table);
    println!("  find_all_sql : {}", Order::find_all_sql());
    println!("  insert_sql   : {}", Order::insert_sql());
    println!("  migration_sql: {}", Order::migration_sql());

    println!();
    println!("─── inventory-collected entities ───");
    for reg in comptime_web::db::all_entities() {
        println!("  table '{}' – {} columns", reg.meta.table, reg.meta.columns.len());
    }
}

fn show_type_info() {
    use comptime_web::{type_info_kind, type_info_size};

    // These are evaluated at compile time via std::mem::type_info::Type::of::<T>().
    const USER_KIND:      &str       = type_info_kind::<User>();
    const USER_SIZE:      Option<usize> = type_info_size::<User>();
    const ORDER_KIND:     &str       = type_info_kind::<Order>();
    const U64_KIND:       &str       = type_info_kind::<u64>();
    const STRING_KIND:    &str       = type_info_kind::<String>();
    const BOOL_KIND:      &str       = type_info_kind::<bool>();

    println!("─── std::mem::type_info compile-time reflection ───");
    println!("  User  → kind = {}, size = {:?}", USER_KIND, USER_SIZE);
    println!("  Order → kind = {}", ORDER_KIND);
    println!("  u64   → kind = {}", U64_KIND);
    println!("  String→ kind = {}", STRING_KIND);
    println!("  bool  → kind = {}", BOOL_KIND);

    // Struct field names extracted via type_info at runtime
    // (const-alloc equivalent planned for future nightly).
    let user_fields = comptime_web::struct_field_names_runtime::<User>();
    println!("  User fields (via type_info): {:?}", user_fields);
}

// ──────────────────────────────────────────────────────────────────────
// JsonSchema demonstration
// ──────────────────────────────────────────────────────────────────────

fn show_schemas() {
    use comptime_web::JsonSchema;

    println!("─── JsonSchema (derived, uses type_info for field names) ───");
    println!("  User  schema: {}", serde_json::to_string_pretty(&User::schema()).unwrap());
    println!("  Order schema: {}", serde_json::to_string_pretty(&Order::schema()).unwrap());
}

// ──────────────────────────────────────────────────────────────────────
// Dependency Injection demonstration  (Agents.md §12)
// ──────────────────────────────────────────────────────────────────────

/// Example repository service (leaf dependency – no further deps).
pub struct UserRepository;

impl UserRepository {
    pub fn find_all(&self) -> Vec<String> {
        vec!["alice".into(), "bob".into()]
    }
}

/// Example service with an injected dependency.
///
/// `#[derive(Injectable)]` generates:
///   impl Injectable for UserService { fn resolve(reg) -> Self { … } }
///   inventory::submit!(ServiceRegistration { … })
#[derive(Injectable)]
#[scope("singleton")]
pub struct UserService {
    pub user_repo: Arc<UserRepository>,
}

impl UserService {
    pub fn list_names(&self) -> Vec<String> {
        self.user_repo.find_all()
    }
}

/// Demonstrates the compile-time DI framework.
fn show_di() {
    println!("─── comptime_di – dependency injection ───");

    let reg = comptime_di::init_registry();

    // Register a leaf service (no deps).
    reg.register_singleton(UserRepository);

    // Register an injectable service (deps resolved automatically).
    reg.register_injectable::<UserService>(Scope::Singleton);

    // Validate the dependency graph at startup.
    reg.validate();

    // Dump the service graph for debugging.
    reg.dump_graph();

    // Resolve and use the service.
    let svc: Arc<UserService> = reg.get::<UserService>();
    println!("  UserService.list_names() = {:?}", svc.list_names());
}

// ──────────────────────────────────────────────────────────────────────
// Entry point
// ──────────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() {
    println!("comptime_web – compile-time web framework");
    println!("==========================================");

    show_entity_meta();
    show_type_info();
    show_schemas();
    show_di();

    // ── Database setup ────────────────────────────────────────
    // "postgres" hostname resolves inside Docker Compose; locally override with
    // DATABASE_URL=postgres://postgres:postgres@localhost:5432/<db>
    let db_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://postgres:postgres@localhost:5432".to_string());

    println!();
    println!("Connecting to database...");
    comptime_web::init_pool(&db_url, Default::default()).await
        .expect("failed to connect to database");
    println!("  pool ready  (binary protocol via extended query protocol)");

    println!("Running schema migrations (strategy: {:?}) …", comptime_web::ACTIVE_STRATEGY);
    comptime_web::run_migrations(comptime_web::pool()).await
        .expect("schema migration failed");
    println!("  migrations done");

    comptime_web::warm_all_statements().await
        .expect("failed to warm prepared statements");
    println!("  statements warmed (parse+plan cached, binary format active)");

    println!();
    println!("Starting server on 127.0.0.1:8080 ...");

    Server::new()
        .title("comptime_web example")
        .version("0.1.0")
        .run("127.0.0.1:8080")
        .await
        .expect("server error");
}

