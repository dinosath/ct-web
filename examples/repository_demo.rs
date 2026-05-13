// examples/repository_demo.rs – Jakarta Data-style repository example.
//
// Run with:
//   DATABASE_URL=postgres://... cargo run --example repository_demo
//
// This demonstrates:
//   1. Defining an entity with #[derive(RuntimeEntity)]
//   2. Using GenericRepository for zero-boilerplate CRUD
//   3. Creating a typed newtype repository with custom methods
//   4. Pagination with Pageable / Page
//   5. OneToMany / ManyToOne relation loaders

use comptime_orm::{
    CrudRepository, GenericRepository, Page, PageableRepository, Pageable,
    ParamValue, RuntimeEntity, Sort,
};
use comptime_orm::pool::{init_pool, pool, PoolConfig};
use sqlx::FromRow;

// ──────────────────────────────────────────────────────────────────────
// Entity definitions
// ──────────────────────────────────────────────────────────────────────

/// A user entity – table name derived as "users" by RuntimeEntity.
///
/// `#[one_to_many]` on `orders` tells `#[derive(RuntimeEntity)]` to generate:
///   `async fn load_orders(&self, pool) -> sqlx::Result<Vec<Order>>`
#[derive(Debug, Clone, Default, FromRow, RuntimeEntity)]
pub struct User {
    pub id:        i64,
    pub name:      String,
    pub email:     String,
    pub is_active: bool,
    /// @OneToMany – not a DB column; drives relation code generation.
    #[one_to_many]
    pub orders: Vec<Order>,
}

/// An order entity – table "orders".
///
/// `#[many_to_one]` on `user` tells `#[derive(RuntimeEntity)]` to generate:
///   `async fn load_user(&self, pool) -> sqlx::Result<Option<User>>`
/// The FK column on this table is inferred as `user_id`.
#[derive(Debug, Clone, Default, FromRow, RuntimeEntity)]
pub struct Order {
    pub id:      i64,
    pub user_id: i64,   // FK stored in DB
    pub total:   f64,
    /// @ManyToOne – not a DB column; drives relation code generation.
    #[many_to_one]
    pub user: User,
}


// ──────────────────────────────────────────────────────────────────────
// Typed repository (newtype pattern)
// ──────────────────────────────────────────────────────────────────────

/// A strongly-typed UserRepository that delegates to GenericRepository.
pub struct UserRepository(GenericRepository<User, i64>);

impl UserRepository {
    pub fn new(pool: sqlx::PgPool) -> Self {
        UserRepository(GenericRepository::new(pool))
    }

    /// Find active users only.
    pub async fn find_active(&self) -> sqlx::Result<Vec<User>> {
        self.0
            .find_by_column("is_active", ParamValue::Bool(true))
            .await
    }

    /// Find active users with pagination.
    pub async fn find_active_paged(&self, page: usize, size: usize) -> sqlx::Result<Page<User>> {
        let pageable = Pageable::of(page, size)
            .sort(Sort::asc("name"));
        self.0
            .find_by_column_paged("is_active", ParamValue::Bool(true), pageable)
            .await
    }

    /// Fetch all orders for a given user via the generated OneToMany accessor.
    pub async fn orders_for(&self, user: &User) -> sqlx::Result<Vec<Order>> {
        user.load_orders(self.0.pool()).await
    }
}

// Delegate CrudRepository to inner GenericRepository.
impl comptime_orm::DataRepository<User, i64> for UserRepository {}

impl comptime_orm::CrudRepository<User, i64> for UserRepository {
    fn pool(&self) -> &sqlx::PgPool { self.0.pool() }
}

impl comptime_orm::PageableRepository<User, i64> for UserRepository {}

// ──────────────────────────────────────────────────────────────────────
// main
// ──────────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://postgres:postgres@localhost/comptime_demo".into());

    init_pool(&url, PoolConfig::default()).await?;
    let pg = pool().clone();

    // ── Print reflected metadata ────────────────────────────────────
    let user_info  = User::entity_info();
    let order_info = Order::entity_info();

    println!("=== Entity metadata (from std::mem::type_info) ===");
    println!("User  → table={}, pk={}", user_info.table, user_info.pk().col_name);
    println!("  columns:");
    for f in user_info.column_fields() {
        println!("    {} {} (nullable={})", f.col_name, f.sql_type, f.nullable);
    }

    println!("Order → table={}, pk={}", order_info.table, order_info.pk().col_name);
    println!("  columns:");
    for f in order_info.column_fields() {
        println!("    {} {} ({:?})", f.col_name, f.sql_type, f.kind);
    }

    println!("\n=== Generated SQL ===");
    println!("insert_sql  : {}", user_info.insert_sql());
    println!("update_sql  : {}", user_info.update_sql());
    println!("delete_sql  : {}", user_info.delete_sql());
    println!("find_by_id  : {}", user_info.find_by_id_sql());
    println!("select_all  : {}", user_info.select_all_sql());
    println!("count_sql   : {}", user_info.count_sql());

    // ── Generic repository usage ────────────────────────────────────
    let user_repo = UserRepository::new(pg.clone());
    let order_repo = GenericRepository::<Order, i64>::new(pg.clone());

    // Count
    let user_count  = user_repo.count().await?;
    let order_count = order_repo.count().await?;
    println!("\n=== Counts ===");
    println!("users={user_count}  orders={order_count}");

    // Paginated fetch
    let page = user_repo.find_all_paged(Pageable::of(0, 10).sort(Sort::desc("id"))).await?;
    println!("\n=== Page 0 of users (10 per page) ===");
    println!("total_elements={} total_pages={}", page.total_elements, page.total_pages());
    for u in &page.content {
        println!("  {:?}", u);
    }

    // OneToMany: load orders for the first user via the generated accessor.
    if let Some(first_user) = page.content.first() {
        let orders = user_repo.orders_for(first_user).await?;
        println!("\n=== Orders for user {} ===", first_user.id);
        for o in &orders {
            // ManyToOne: load parent user via the generated accessor.
            let owner = o.load_user(&pg).await?;
            println!("  order={:?}  owner_name={:?}", o, owner.map(|u| u.name));
        }
    }

    Ok(())
}
