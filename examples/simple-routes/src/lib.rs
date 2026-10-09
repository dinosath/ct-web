#[ct_web::routes]
pub mod routes {
    #[ct_web::route(method = "GET", path = "/dependency/users")]
    pub fn list_dependency_users() -> &'static str {
        "dependency users"
    }

    #[ct_web::route(method = "GET", path = "/dependency/users/{id}")]
    pub async fn get_dependency_user() -> &'static str {
        "dependency user"
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use ct_web::router::{HttpMethod, RouteId, RouteResolution};

        #[test]
        fn generated_router_matches_dependency_routes() {
            assert_eq!(
                ROUTER.resolve(HttpMethod::Get, "/dependency/users"),
                RouteResolution::Matched(RouteId(0))
            );
            assert_eq!(
                ROUTER.resolve(HttpMethod::Get, "/dependency/users/42"),
                RouteResolution::Matched(RouteId(1))
            );
        }
    }
}