use core::cmp::Ordering;
use core::future::Future;
use core::task::{Context, Poll};
use std::convert::Infallible;

use tower_service::Service;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum HttpMethod {
    Connect,
    Delete,
    Get,
    Head,
    Options,
    Patch,
    Post,
    Put,
    Trace,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PathSegment {
    Root,
    Static(&'static str),
    Param(&'static str),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RouteId(pub u32);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MethodHandler {
    pub method: HttpMethod,
    pub route: RouteId,
}

pub struct RouterNode {
    pub segment: PathSegment,
    /// Static children must be sorted by segment name.
    pub static_children: &'static [RouterNode],
    pub param_child: Option<&'static RouterNode>,
    /// Method handlers must be sorted by method.
    pub method_handlers: &'static [MethodHandler],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RouteMatch<'path> {
    pub route: RouteId,
    pub path: &'path str,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RouteResolution {
    Matched(RouteId),
    MethodNotAllowed,
    NotFound,
}

enum LookupResult<'path> {
    NoPath,
    MethodNotAllowed,
    Matched(RouteMatch<'path>),
}

impl RouterNode {
    pub fn resolve(&self, method: HttpMethod, path: &str) -> RouteResolution {
        let Some(segments) = path.strip_prefix('/') else {
            return RouteResolution::NotFound;
        };
        let segments = match segments.strip_suffix('/') {
            Some(segments) if segments.ends_with('/') => return RouteResolution::NotFound,
            Some(segments) => segments,
            None => segments,
        };

        match self.lookup_from(method, segments, path) {
            LookupResult::Matched(found) => RouteResolution::Matched(found.route),
            LookupResult::MethodNotAllowed => RouteResolution::MethodNotAllowed,
            LookupResult::NoPath => RouteResolution::NotFound,
        }
    }

    pub fn lookup<'path>(&self, method: HttpMethod, path: &'path str) -> Option<RouteMatch<'path>> {
        let segments = path.strip_prefix('/')?;
        let segments = match segments.strip_suffix('/') {
            Some(segments) if segments.ends_with('/') => return None,
            Some(segments) => segments,
            None => segments,
        };

        match self.lookup_from(method, segments, path) {
            LookupResult::Matched(found) => Some(found),
            LookupResult::NoPath | LookupResult::MethodNotAllowed => None,
        }
    }

    fn lookup_from<'path>(
        &self,
        method: HttpMethod,
        remaining: &'path str,
        full_path: &'path str,
    ) -> LookupResult<'path> {
        if remaining.is_empty() {
            let handler = self
                .method_handlers
                .binary_search_by_key(&method, |handler| handler.method)
                .ok()
                .map(|index| &self.method_handlers[index]);
            return match handler.map(|handler| RouteMatch {
                route: handler.route,
                path: full_path,
            }) {
                Some(found) => LookupResult::Matched(found),
                None => LookupResult::MethodNotAllowed,
            };
        }

        let segment_end = remaining.find('/').unwrap_or(remaining.len());
        if segment_end == 0 {
            return LookupResult::NoPath;
        }

        let segment = &remaining[..segment_end];
        let rest = if segment_end == remaining.len() {
            ""
        } else {
            &remaining[segment_end + 1..]
        };

        if let Ok(index) = self.static_children.binary_search_by(|child| {
            let PathSegment::Static(child_segment) = child.segment else {
                return Ordering::Less;
            };
            child_segment.cmp(segment)
        }) {
            match self.static_children[index].lookup_from(method, rest, full_path) {
                LookupResult::NoPath => {}
                result => return result,
            }
        }

        match self.param_child {
            Some(child) => child.lookup_from(method, rest, full_path),
            None => LookupResult::NoPath,
        }
    }
}

pub trait RouteRequest {
    fn route_method(&self) -> Option<HttpMethod>;
    fn route_path(&self) -> &str;
}

/// Routes requests through a static tree without constructing an Axum `Router`.
/// The service does not allocate or box its dispatch future.
pub struct StaticService<D> {
    routes: &'static RouterNode,
    dispatch: D,
}

impl<D> StaticService<D> {
    pub const fn new(routes: &'static RouterNode, dispatch: D) -> Self {
        Self { routes, dispatch }
    }
}

impl<D: Clone> Clone for StaticService<D> {
    fn clone(&self) -> Self {
        Self {
            routes: self.routes,
            dispatch: self.dispatch.clone(),
        }
    }
}

impl<D, Req, Fut, Res> Service<Req> for StaticService<D>
where
    Req: RouteRequest,
    D: FnMut(RouteResolution, Req) -> Fut,
    Fut: Future<Output = Result<Res, Infallible>>,
{
    type Response = Res;
    type Error = Infallible;
    type Future = Fut;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, request: Req) -> Self::Future {
        let resolution = match request.route_method() {
            Some(method) => self.routes.resolve(method, request.route_path()),
            None => RouteResolution::NotFound,
        };
        (self.dispatch)(resolution, request)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::pin::Pin;
    use core::task::Waker;
    use std::future::ready;

    static USER_ID_HANDLERS: [MethodHandler; 2] = [
        MethodHandler {
            method: HttpMethod::Get,
            route: RouteId(1),
        },
        MethodHandler {
            method: HttpMethod::Put,
            route: RouteId(2),
        },
    ];
    static DETAILS_HANDLERS: [MethodHandler; 1] = [MethodHandler {
        method: HttpMethod::Get,
        route: RouteId(4),
    }];
    const DETAILS_NODE: RouterNode = RouterNode {
        segment: PathSegment::Static("details"),
        static_children: &[],
        param_child: None,
        method_handlers: &DETAILS_HANDLERS,
    };
    static USER_ID_CHILDREN: [RouterNode; 1] = [DETAILS_NODE];
    static USER_ID_NODE: RouterNode = RouterNode {
        segment: PathSegment::Param("id"),
        static_children: &USER_ID_CHILDREN,
        param_child: None,
        method_handlers: &USER_ID_HANDLERS,
    };
    static SEARCH_HANDLERS: [MethodHandler; 1] = [MethodHandler {
        method: HttpMethod::Get,
        route: RouteId(3),
    }];
    const SEARCH_NODE: RouterNode = RouterNode {
        segment: PathSegment::Static("search"),
        static_children: &[],
        param_child: None,
        method_handlers: &SEARCH_HANDLERS,
    };
    static USERS_HANDLERS: [MethodHandler; 1] = [MethodHandler {
        method: HttpMethod::Get,
        route: RouteId(0),
    }];
    static USERS_CHILDREN: [RouterNode; 1] = [SEARCH_NODE];
    const USERS_NODE: RouterNode = RouterNode {
        segment: PathSegment::Static("users"),
        static_children: &USERS_CHILDREN,
        param_child: Some(&USER_ID_NODE),
        method_handlers: &USERS_HANDLERS,
    };
    static ROOT_CHILDREN: [RouterNode; 1] = [USERS_NODE];
    static ROOT: RouterNode = RouterNode {
        segment: PathSegment::Root,
        static_children: &ROOT_CHILDREN,
        param_child: None,
        method_handlers: &[],
    };

    struct TestRequest {
        method: Option<HttpMethod>,
        path: &'static str,
    }

    impl RouteRequest for TestRequest {
        fn route_method(&self) -> Option<HttpMethod> {
            self.method
        }

        fn route_path(&self) -> &str {
            self.path
        }
    }

    fn poll_ready<F: Future + Unpin>(mut future: F) -> F::Output {
        let mut context = Context::from_waker(Waker::noop());
        match Pin::new(&mut future).poll(&mut context) {
            Poll::Ready(output) => output,
            Poll::Pending => panic!("immediate test future unexpectedly pending"),
        }
    }

    #[test]
    fn matches_static_and_parameter_routes_by_method() {
        assert_eq!(
            ROOT.lookup(HttpMethod::Get, "/users").unwrap().route,
            RouteId(0)
        );
        assert_eq!(
            ROOT.lookup(HttpMethod::Get, "/users/42").unwrap().route,
            RouteId(1)
        );
        assert_eq!(
            ROOT.lookup(HttpMethod::Get, "/users/42").unwrap().path,
            "/users/42"
        );
        assert_eq!(
            ROOT.lookup(HttpMethod::Put, "/users/42").unwrap().route,
            RouteId(2)
        );
    }

    #[test]
    fn static_child_takes_precedence_over_parameter_child() {
        assert_eq!(
            ROOT.lookup(HttpMethod::Get, "/users/search").unwrap().route,
            RouteId(3)
        );
        assert!(ROOT.lookup(HttpMethod::Put, "/users/search").is_none());
        assert_eq!(
            ROOT.lookup(HttpMethod::Get, "/users/search/details")
                .unwrap()
                .route,
            RouteId(4)
        );
    }

    #[test]
    fn rejects_unknown_methods_and_malformed_paths() {
        assert!(ROOT.lookup(HttpMethod::Post, "/users/42").is_none());
        assert!(ROOT.lookup(HttpMethod::Get, "/users//42").is_none());
        assert!(ROOT.lookup(HttpMethod::Get, "/users/42//").is_none());
        assert!(ROOT.lookup(HttpMethod::Get, "users/42").is_none());
    }

    #[test]
    fn tower_service_resolves_then_dispatches_without_boxing() {
        let mut service = StaticService::new(&ROOT, |resolution, _request: TestRequest| {
            ready::<Result<RouteResolution, Infallible>>(Ok(resolution))
        });

        assert_eq!(
            poll_ready(service.call(TestRequest {
                method: Some(HttpMethod::Get),
                path: "/users/42",
            })),
            Ok(RouteResolution::Matched(RouteId(1)))
        );
        assert_eq!(
            poll_ready(service.call(TestRequest {
                method: Some(HttpMethod::Post),
                path: "/users/42",
            })),
            Ok(RouteResolution::MethodNotAllowed)
        );
        assert_eq!(
            poll_ready(service.call(TestRequest {
                method: None,
                path: "/missing",
            })),
            Ok(RouteResolution::NotFound)
        );
    }
}
