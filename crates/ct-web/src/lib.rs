#![feature(type_info, register_tool, rustc_attrs)]
#![allow(internal_features)]

pub mod axum_adapter;
pub mod reflection;
pub mod router;

pub use axum;
pub use ct_web_macros::{main, route, routes};
use sea_orm as _;
pub use tokio;

use core::future::Future;
use axum::{body::Body, http::{Request, Response}};
use std::convert::Infallible;
use router::{RouteResolution, RouterNode, StaticService};

pub async fn serve<D, F>(
    routes: &'static RouterNode,
    dispatch: D,
) -> Result<(), std::io::Error>
where
    D: FnMut(RouteResolution, Request<Body>) -> F + Clone + Send + 'static,
    F: Future<Output = Result<Response<Body>, Infallible>> + Send + 'static,
{
	let address = std::env::var("CT_WEB_ADDR").unwrap_or_else(|_| "127.0.0.1:3000".to_owned());
	let listener = tokio::net::TcpListener::bind(&address).await?;
	println!("ct-web listening on {}", listener.local_addr()?);
	axum::serve(listener, StaticService::new(routes, dispatch).into_make_service()).await
}

pub fn launch<F, Fut>(application: F) -> Result<(), Box<dyn std::error::Error>>
where
	F: FnOnce() -> Fut,
	Fut: Future<Output = Result<(), std::io::Error>>,
{
	let runtime = tokio::runtime::Runtime::new()?;
	runtime.block_on(application())?;
	Ok(())
}
