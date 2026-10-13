#![feature(type_info, register_tool)]
#![register_tool(app)]

mod config;
mod controller;
mod database;
mod entity;
mod repository;
mod service;
mod user_service;

const _: () = assert!(ct_di::validate_injections(
    core::mem::type_info::crate_types_with_attr("app::component"),
    core::mem::type_info::crate_types_with_attr("app::singleton"),
    core::mem::type_info::crate_types_with_attr("app::config"),
));

fn main() {
    ct_web::launch(application).expect("server failed");
}

async fn application() -> Result<(), std::io::Error> {
    let config = config::DatabaseConfig::from_env();
    let database = database::Database::connect(config)
        .await
        .map_err(std::io::Error::other)?;
    let repository = repository::Repository::__ct_di_new(database);
    let users = user_service::UserService::__ct_di_new(repository);
    let app = controller::router(users);
    let address = std::env::var("CT_WEB_ADDR").unwrap_or_else(|_| "127.0.0.1:3000".to_owned());
    let listener = ct_web::tokio::net::TcpListener::bind(&address).await?;
    println!("ct-web listening on {}", listener.local_addr()?);
    axum::serve(listener, app).await
}
