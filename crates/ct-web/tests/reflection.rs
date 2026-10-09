#![feature(type_info, register_tool, rustc_attrs)]
#![allow(internal_features)]
#![register_tool(app)]
#![allow(dead_code)]

#[path = "../src/reflection.rs"]
mod reflection;

#[app::route(method = "GET", path = "/users")]
fn list_users() {}

#[app::route(method = "POST", path = "/orders")]
fn create_order() {}

#[app::middleware(order = "before_routes")]
fn authenticate() {}

#[app::middleware(order = "after_routes")]
fn audit() {}

const ROUTE_FUNCTIONS: &[core::any::TypeId] =
    core::mem::type_info::crate_functions_with_attr("app::route");
const MIDDLEWARE_FUNCTIONS: &[core::any::TypeId] =
    core::mem::type_info::crate_functions_with_attr("app::middleware");
const ROUTES: [Option<reflection::RouteMetadata>; ROUTE_FUNCTIONS.len()] =
    reflection::route_metadata_array(ROUTE_FUNCTIONS).unwrap();
const MIDDLEWARES: [Option<reflection::MiddlewareMetadata>; MIDDLEWARE_FUNCTIONS.len()] =
    reflection::middleware_metadata_array(MIDDLEWARE_FUNCTIONS).unwrap();

#[test]
fn discovers_routes_and_middleware_at_compile_time() {
    let route = ROUTES[0].unwrap();
    assert_eq!(route.name, "list_users");
    assert_eq!(route.method, "GET");
    assert_eq!(route.path, "/users");
    let create_order = ROUTES[1].unwrap();
    assert_eq!(create_order.name, "create_order");
    assert_eq!(create_order.method, "POST");
    assert_eq!(create_order.path, "/orders");

    let middleware = MIDDLEWARES[0].unwrap();
    assert_eq!(middleware.name, "authenticate");
    assert_eq!(middleware.order, Some("before_routes"));
    let audit = MIDDLEWARES[1].unwrap();
    assert_eq!(audit.name, "audit");
    assert_eq!(audit.order, Some("after_routes"));
}
