// examples/fullstack.rs – full-stack example combining DI, ORM, and Web.
//
// Demonstrates:
//   • #[derive(Entity)]    – compile-time SQL generation for User and Order
//   • #[derive(Injectable)]– compile-time DI wiring for service layer
//   • #[get] / #[post]     – zero-runtime route registration
//   • #[inject]            – handler-level dependency injection
//   • ServiceRegistry      – global DI container with validation
//   • Query<E> builder     – type-safe query with filters/pagination
//   • Server::run()        – startup: router trie + OpenAPI + bind socket
//
// Run:
//   DATABASE_URL=postgres://postgres:postgres@localhost:5432/mydb \
//     cargo run --example fullstack

#![feature(type_info)]

use std::sync::Arc;

use comptime_di::{Injectable, Scope};
use comptime_orm::{DbEntity, Entity};
use comptime_web::*;
use comptime_web_macros::{get, post, delete, JsonSchema};
use serde::{Deserialize, Serialize};

// ═══════════════════════════════════════════════════════════════════════
// 1. Domain entities  (ORM layer)
// ═══════════════════════════════════════════════════════════════════════

/// A user in the system.
///
/// #[derive(Entity)] generates at compile time:
///   • static USER_ENTITY_META with all SQL strings in .rodata
///   • impl DbEntity – find_by_pk / find_all / insert / update / delete
///   • pub const COL_ID / COL_NAME / COL_EMAIL – column index constants
///   • inventory::submit! – registers for migration runner
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Entity, sqlx::FromRow)]
pub struct User {
    #[pk]
    pub id:    i64,
    pub name:  String,
    pub email: String,
}

/// An order placed by a user.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Entity, sqlx::FromRow)]
pub struct Order {
    #[pk]
    pub id:         i64,
    pub user_id:    i64,
    pub product:    String,
    pub quantity:   i32,
}

/// DTO for creating a new user (no id field – assigned by the database).
#[derive(Debug, Deserialize, JsonSchema)]
pub struct CreateUser {
    pub name:  String,
    pub email: String,
}

/// DTO for creating a new order.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct CreateOrder {
    pub user_id:  i64,
    pub product:  String,
    pub quantity: i32,
}

// ═══════════════════════════════════════════════════════════════════════
// 2. Repository layer  (thin wrappers around ORM + Query builder)
// ═══════════════════════════════════════════════════════════════════════

/// Encapsulates all User database operations.
///
/// Registered as a singleton service in the DI container.
/// Holds no state – the PgPool is accessed via the global `comptime_orm::pool()`.
pub struct UserRepo;

impl UserRepo {
    pub async fn find_all(&self) -> sqlx::Result<Vec<User>> {
        User::find_all(comptime_orm::pool()).await
    }

    pub async fn find_by_id(&self, id: i64) -> sqlx::Result<Option<User>> {
        User::find_by_pk(id, comptime_orm::pool()).await
    }

    pub async fn find_by_name(&self, name: &str) -> sqlx::Result<Vec<User>> {
        User::query()
            .filter(User::COL_NAME, comptime_orm::FilterOp::Eq, comptime_orm::ParamValue::Text(name.to_string()))
            .fetch_all(comptime_orm::pool())
            .await
    }

    pub async fn create(&self, user: &User) -> sqlx::Result<()> {
        user.insert(comptime_orm::pool()).await?;
        Ok(())
    }

    pub async fn delete(&self, user: &User) -> sqlx::Result<()> {
        user.delete(comptime_orm::pool()).await?;
        Ok(())
    }
}

/// Encapsulates all Order database operations.
pub struct OrderRepo;

impl OrderRepo {
    pub async fn find_all(&self) -> sqlx::Result<Vec<Order>> {
        Order::find_all(comptime_orm::pool()).await
    }

    pub async fn find_by_user(&self, user_id: i64) -> sqlx::Result<Vec<Order>> {
        Order::query()
            .filter(Order::COL_USER_ID, comptime_orm::FilterOp::Eq, comptime_orm::ParamValue::I64(user_id))
            .fetch_all(comptime_orm::pool())
            .await
    }

    pub async fn create(&self, order: &Order) -> sqlx::Result<()> {
        order.insert(comptime_orm::pool()).await?;
        Ok(())
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 3. Service layer  (business logic with injected repositories)
// ═══════════════════════════════════════════════════════════════════════

/// Application service that orchestrates user operations.
///
/// `#[derive(Injectable)]` generates:
///   impl Injectable for UserService {
///       fn resolve(reg) -> Self {
///           Self { repo: reg.get::<UserRepo>() }
///       }
///       fn dependencies() -> &["UserRepo"]
///   }
///
/// `#[scope("singleton")]` – one instance shared for the app lifetime.
#[derive(Injectable)]
#[scope("singleton")]
pub struct UserService {
    pub repo: Arc<UserRepo>,
}

impl UserService {
    pub async fn list_users(&self) -> Result<Vec<User>, String> {
        self.repo.find_all().await.map_err(|e| e.to_string())
    }

    pub async fn get_user(&self, id: i64) -> Result<Option<User>, String> {
        self.repo.find_by_id(id).await.map_err(|e| e.to_string())
    }

    pub async fn create_user(&self, dto: CreateUser) -> Result<User, String> {
        let user = User { id: 0, name: dto.name, email: dto.email };
        self.repo.create(&user).await.map_err(|e| e.to_string())?;
        Ok(user)
    }

    pub async fn delete_user(&self, id: i64) -> Result<(), String> {
        let user = self.repo.find_by_id(id).await.map_err(|e| e.to_string())?
            .ok_or_else(|| "user not found".to_string())?;
        self.repo.delete(&user).await.map_err(|e| e.to_string())
    }
}

/// Application service for order operations.
///
/// Depends on both OrderRepo and UserService (to validate user existence).
#[derive(Injectable)]
#[scope("singleton")]
pub struct OrderService {
    pub order_repo:   Arc<OrderRepo>,
    pub user_service: Arc<UserService>,
}

impl OrderService {
    pub async fn list_orders(&self) -> Result<Vec<Order>, String> {
        self.order_repo.find_all().await.map_err(|e| e.to_string())
    }

    pub async fn orders_for_user(&self, user_id: i64) -> Result<Vec<Order>, String> {
        // Validate user exists before querying orders.
        self.user_service.get_user(user_id).await?
            .ok_or_else(|| format!("user {} not found", user_id))?;
        self.order_repo.find_by_user(user_id).await.map_err(|e| e.to_string())
    }

    pub async fn place_order(&self, dto: CreateOrder) -> Result<Order, String> {
        // Validate user exists.
        self.user_service.get_user(dto.user_id).await?
            .ok_or_else(|| format!("user {} not found", dto.user_id))?;

        let order = Order {
            id:       0,
            user_id:  dto.user_id,
            product:  dto.product,
            quantity: dto.quantity,
        };
        self.order_repo.create(&order).await.map_err(|e| e.to_string())?;
        Ok(order)
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 4. HTTP handlers  (Web layer – uses DI to access services)
// ═══════════════════════════════════════════════════════════════════════

/// GET /users – list all users.
///
/// Resolves `UserService` from the global DI registry at request time.
#[get("/users")]
async fn handle_list_users(_req: &Request) -> Response {
    let svc = comptime_di::service_registry().get::<UserService>();
    match svc.list_users().await {
        Ok(users) => Response::json(StatusCode::OK, &users),
        Err(e)    => Response::json(StatusCode::INTERNAL_SERVER_ERR,
                         &serde_json::json!({ "error": e })),
    }
}

/// GET /users/:id – get a single user.
#[get("/users/:id")]
async fn handle_get_user(id: i64) -> Response {
    let svc = comptime_di::service_registry().get::<UserService>();
    match svc.get_user(id).await {
        Ok(Some(u)) => Response::json(StatusCode::OK, &u),
        Ok(None)    => Response::json(StatusCode::NOT_FOUND,
                           &serde_json::json!({ "error": "not found" })),
        Err(e)      => Response::json(StatusCode::INTERNAL_SERVER_ERR,
                           &serde_json::json!({ "error": e })),
    }
}

/// POST /users – create a new user.
#[post("/users")]
async fn handle_create_user(req: &Request) -> Response {
    let dto = match serde_json::from_slice::<CreateUser>(&req.body) {
        Ok(d)  => d,
        Err(e) => return Response::json(StatusCode::BAD_REQUEST,
                      &serde_json::json!({ "error": e.to_string() })),
    };
    let svc = comptime_di::service_registry().get::<UserService>();
    match svc.create_user(dto).await {
        Ok(user) => Response::json(StatusCode::CREATED, &user),
        Err(e)   => Response::json(StatusCode::INTERNAL_SERVER_ERR,
                        &serde_json::json!({ "error": e })),
    }
}

/// DELETE /users/:id – delete a user.
#[delete("/users/:id")]
async fn handle_delete_user(id: i64) -> Response {
    let svc = comptime_di::service_registry().get::<UserService>();
    match svc.delete_user(id).await {
        Ok(())   => Response::json(StatusCode::OK,
                        &serde_json::json!({ "deleted": id })),
        Err(e)   => Response::json(StatusCode::NOT_FOUND,
                        &serde_json::json!({ "error": e })),
    }
}

/// GET /orders – list all orders.
#[get("/orders")]
async fn handle_list_orders(_req: &Request) -> Response {
    let svc = comptime_di::service_registry().get::<OrderService>();
    match svc.list_orders().await {
        Ok(orders) => Response::json(StatusCode::OK, &orders),
        Err(e)     => Response::json(StatusCode::INTERNAL_SERVER_ERR,
                          &serde_json::json!({ "error": e })),
    }
}

/// GET /users/:id/orders – list orders for a specific user.
#[get("/users/:id/orders")]
async fn handle_user_orders(id: i64) -> Response {
    let svc = comptime_di::service_registry().get::<OrderService>();
    match svc.orders_for_user(id).await {
        Ok(orders) => Response::json(StatusCode::OK, &orders),
        Err(e)     => Response::json(StatusCode::NOT_FOUND,
                          &serde_json::json!({ "error": e })),
    }
}

/// POST /orders – place a new order.
#[post("/orders")]
async fn handle_create_order(req: &Request) -> Response {
    let dto = match serde_json::from_slice::<CreateOrder>(&req.body) {
        Ok(d)  => d,
        Err(e) => return Response::json(StatusCode::BAD_REQUEST,
                      &serde_json::json!({ "error": e.to_string() })),
    };
    let svc = comptime_di::service_registry().get::<OrderService>();
    match svc.place_order(dto).await {
        Ok(order) => Response::json(StatusCode::CREATED, &order),
        Err(e)    => Response::json(StatusCode::INTERNAL_SERVER_ERR,
                         &serde_json::json!({ "error": e })),
    }
}

/// GET /health – health check.
#[get("/health")]
async fn handle_health(_req: &Request) -> Response {
    Response::json(StatusCode::OK, &serde_json::json!({ "status": "ok" }))
}

/// GET /openapi.json – auto-generated OpenAPI spec.
#[get("/openapi.json")]
async fn handle_openapi(req: &Request) -> Response {
    comptime_web::openapi::openapi_handler(req).await
}

// ═══════════════════════════════════════════════════════════════════════
// 5. Bootstrap  (wire everything together)
// ═══════════════════════════════════════════════════════════════════════

/// Initialise the DI container with all repositories and services.
///
/// Repositories are leaf services (no deps) registered as singletons.
/// Services use `register_injectable` which calls their `Injectable::resolve()`
/// to auto-wire `Arc<T>` fields from the registry.
fn setup_di() {
    let reg = comptime_di::init_registry();

    // ── leaf services (no dependencies) ───────────────────────────
    reg.register_singleton(UserRepo);
    reg.register_singleton(OrderRepo);

    // ── injectable services (deps auto-resolved) ──────────────────
    reg.register_injectable::<UserService>(Scope::Singleton);
    reg.register_injectable::<OrderService>(Scope::Singleton);

    // ── validate dependency graph (panics if anything is missing) ──
    reg.validate();

    // ── print the dependency graph for debugging ──────────────────
    reg.dump_graph();
}

/// Print compile-time entity metadata generated by #[derive(Entity)].
fn show_entity_meta() {
    println!("─── compile-time entity metadata ───");
    for reg in comptime_orm::all_entities() {
        let meta = reg.meta;
        println!("  table: {}", meta.table);
        println!("    find_all : {}", meta.find_all_sql);
        println!("    insert   : {}", meta.insert_sql);
        println!("    migration: {}", meta.migration_sql);
        for col in meta.columns {
            println!("    col {:12} {:16} pk={}", col.name, col.sql_type.as_sql_str(), col.is_pk);
        }
        println!();
    }
}

/// Print type_info reflection data for entity structs.
fn show_type_info() {
    use comptime_di::reflect;

    println!("─── type_info service introspection ───");
    println!("  UserService  kind={}, fields={:?}",
        reflect::service_kind::<UserService>(),
        reflect::service_field_names::<UserService>());
    println!("  OrderService kind={}, fields={:?}",
        reflect::service_kind::<OrderService>(),
        reflect::service_field_names::<OrderService>());
    println!("  UserRepo     injectable={}",
        reflect::is_injectable_type::<UserRepo>());
    println!();
}

/// Demonstrate the DI container resolving services.
fn show_di_resolution() {
    let reg = comptime_di::service_registry();

    println!("─── DI resolution ───");
    let user_svc: Arc<UserService> = reg.get::<UserService>();
    let order_svc: Arc<OrderService> = reg.get::<OrderService>();

    // Both services are singletons – same Arc instance on repeated get().
    let user_svc2: Arc<UserService> = reg.get::<UserService>();
    println!("  UserService  singleton: {}", Arc::ptr_eq(&user_svc, &user_svc2));

    // OrderService holds a reference to UserService – same instance.
    println!("  OrderService → UserService is same singleton: {}",
        Arc::ptr_eq(&order_svc.user_service, &user_svc));
    println!();
}

// ═══════════════════════════════════════════════════════════════════════
// 6. Entry point
// ═══════════════════════════════════════════════════════════════════════

#[tokio::main]
async fn main() {
    println!("comptime_web fullstack example");
    println!("══════════════════════════════");
    println!();

    // ── compile-time metadata demos ───────────────────────────────
    show_entity_meta();
    show_type_info();

    // ── database ──────────────────────────────────────────────────
    let db_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://postgres:postgres@localhost:5432/mydb".to_string());

    println!("Connecting to database …");
    comptime_orm::init_pool(&db_url, Default::default()).await
        .expect("failed to connect to database");
    println!("  pool ready");

    println!("Running migrations …");
    comptime_orm::run_migrations(comptime_orm::pool()).await
        .expect("migration failed");
    println!("  migrations done");

    comptime_orm::warm_all_statements().await
        .expect("failed to warm statements");
    println!("  statements warmed");
    println!();

    // ── dependency injection ──────────────────────────────────────
    setup_di();
    show_di_resolution();

    // ── start server ──────────────────────────────────────────────
    println!("Starting server on 127.0.0.1:8080 …");
    println!();
    println!("  Routes:");
    println!("    GET    /health");
    println!("    GET    /users");
    println!("    GET    /users/:id");
    println!("    POST   /users");
    println!("    DELETE /users/:id");
    println!("    GET    /orders");
    println!("    GET    /users/:id/orders");
    println!("    POST   /orders");
    println!("    GET    /openapi.json");
    println!();

    Server::new()
        .title("comptime_web fullstack example")
        .version("0.1.0")
        .run("127.0.0.1:8080")
        .await
        .expect("server error");
}
