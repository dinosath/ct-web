use axum::http::Request;

use crate::router::{HttpMethod, RouteRequest, StaticService};

impl<B> RouteRequest for Request<B> {
    fn route_method(&self) -> Option<HttpMethod> {
        match self.method().as_str() {
            "CONNECT" => Some(HttpMethod::Connect),
            "DELETE" => Some(HttpMethod::Delete),
            "GET" => Some(HttpMethod::Get),
            "HEAD" => Some(HttpMethod::Head),
            "OPTIONS" => Some(HttpMethod::Options),
            "PATCH" => Some(HttpMethod::Patch),
            "POST" => Some(HttpMethod::Post),
            "PUT" => Some(HttpMethod::Put),
            "TRACE" => Some(HttpMethod::Trace),
            _ => None,
        }
    }

    fn route_path(&self) -> &str {
        self.uri().path()
    }
}

impl<D: Clone> StaticService<D> {
    /// Adapts this service for `axum::serve` by cloning it per connection.
    pub fn into_make_service(self) -> tower::make::Shared<Self> {
        tower::make::Shared::new(self)
    }
}
