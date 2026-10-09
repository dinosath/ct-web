#![feature(type_info, register_tool, rustc_attrs)]
#![register_tool(app)]
#![allow(internal_features)]

use ct_web::reflection::{RouteMetadata, route_functions, route_metadata_array};
use nested_app::{HealthController, routes::UserController};

const ROUTE_FUNCTIONS: &[core::any::TypeId] = route_functions();
const ROUTES: [Option<RouteMetadata>; ROUTE_FUNCTIONS.len()] =
    route_metadata_array(ROUTE_FUNCTIONS).unwrap();

#[test]
fn collects_routes_from_the_app_and_its_dependency() {
    let _controllers = (HealthController, UserController);
    let mut found_health = false;
    let mut found_user_list = false;
    let mut found_user = false;

    for route in ROUTES {
        let route = route.expect("every reflected route should have metadata");
        match (route.method, route.path) {
            ("GET", "/health") => found_health = true,
            ("GET", "/users") => found_user_list = true,
            ("GET", "/users/{id}") => found_user = true,
            _ => panic!("unexpected route: {} {}", route.method, route.path),
        }
    }

    assert_eq!(ROUTES.len(), 3);
    assert!(found_health, "route from nested-controller was not collected");
    assert!(found_user_list, "local collection route was not found");
    assert!(found_user, "local item route was not found");
}