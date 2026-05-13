// src/router/build.rs – build HybridRouter from inventory-collected routes.
//
// Two-phase construction:
//   Phase 1 – classify each route as "static" (no `:param` segments) or "param".
//   Phase 2 – freeze into HybridRouter: static routes → per-method HandlerFn slices
//             indexed by the compile-time PHF maps; param routes → Box<[ParamRoute]>.
//
// The final HybridRouter is leaked once so that all references are `&'static`.
//
// Migration note (Agents.md §16):
//   When `#[comptime]` lands this entire module is replaced by a comptime function
//   that generates phf::Maps for static routes and a static `match` expression for
//   param routes.

use super::{
    HybridRouter, ParamRoute, SegmentKind,
    PHF_GET_ROUTES,     PHF_GET_ROUTES_LEN,
    PHF_POST_ROUTES,    PHF_POST_ROUTES_LEN,
    PHF_PUT_ROUTES,     PHF_PUT_ROUTES_LEN,
    PHF_DELETE_ROUTES,  PHF_DELETE_ROUTES_LEN,
    PHF_PATCH_ROUTES,   PHF_PATCH_ROUTES_LEN,
    PHF_HEAD_ROUTES,    PHF_HEAD_ROUTES_LEN,
    PHF_OPTIONS_ROUTES, PHF_OPTIONS_ROUTES_LEN,
};
use crate::{
    handler::{BoxFuture, HandlerFn},
    registry::RouteRegistration,
    request::{HttpMethod, Request},
    response::Response,
};

// ──────────────────────────────────────────────────────────────────────
// Public builder
// ──────────────────────────────────────────────────────────────────────

/// Build a static `HybridRouter` from all inventory-registered routes.
///
/// Called exactly once during `Server::run()`.  All heap data is leaked to
/// produce `'static` references; subsequent lookups involve no allocation.
pub fn build_router(
    routes: impl IntoIterator<Item = &'static RouteRegistration>,
) -> &'static HybridRouter {
    // Pre-allocate per-method handler arrays sized to match the compile-time PHF
    // maps.  Slots are initialised to `placeholder_handler` so that any PHF hit
    // without a corresponding inventory entry returns 500 instead of undefined
    // behaviour.  In practice this should never occur.
    let p: HandlerFn = placeholder_handler;
    let mut get     = vec![p; PHF_GET_ROUTES_LEN];
    let mut post    = vec![p; PHF_POST_ROUTES_LEN];
    let mut put     = vec![p; PHF_PUT_ROUTES_LEN];
    let mut delete  = vec![p; PHF_DELETE_ROUTES_LEN];
    let mut patch   = vec![p; PHF_PATCH_ROUTES_LEN];
    let mut head    = vec![p; PHF_HEAD_ROUTES_LEN];
    let mut options = vec![p; PHF_OPTIONS_ROUTES_LEN];
    let mut param_routes = Vec::<ParamRoute>::new();

    for reg in routes {
        if is_static_path(reg.path) {
            let path = normalize_path(reg.path);
            let key: &str = &path;
            match reg.method {
                HttpMethod::Get     => { if let Some(&i) = PHF_GET_ROUTES.get(key)     { get[i]     = reg.handler; } }
                HttpMethod::Post    => { if let Some(&i) = PHF_POST_ROUTES.get(key)    { post[i]    = reg.handler; } }
                HttpMethod::Put     => { if let Some(&i) = PHF_PUT_ROUTES.get(key)     { put[i]     = reg.handler; } }
                HttpMethod::Delete  => { if let Some(&i) = PHF_DELETE_ROUTES.get(key)  { delete[i]  = reg.handler; } }
                HttpMethod::Patch   => { if let Some(&i) = PHF_PATCH_ROUTES.get(key)   { patch[i]   = reg.handler; } }
                HttpMethod::Head    => { if let Some(&i) = PHF_HEAD_ROUTES.get(key)    { head[i]    = reg.handler; } }
                HttpMethod::Options => { if let Some(&i) = PHF_OPTIONS_ROUTES.get(key) { options[i] = reg.handler; } }
            }
        } else {
            param_routes.push(make_param_route(reg.method, reg.path, reg.handler));
        }
    }

    Box::leak(Box::new(HybridRouter {
        get_handlers:     Box::leak(get    .into_boxed_slice()),
        post_handlers:    Box::leak(post   .into_boxed_slice()),
        put_handlers:     Box::leak(put    .into_boxed_slice()),
        delete_handlers:  Box::leak(delete .into_boxed_slice()),
        patch_handlers:   Box::leak(patch  .into_boxed_slice()),
        head_handlers:    Box::leak(head   .into_boxed_slice()),
        options_handlers: Box::leak(options.into_boxed_slice()),
        param_routes:     param_routes.into_boxed_slice(),
    }))
}

// ──────────────────────────────────────────────────────────────────────
// Helpers
// ──────────────────────────────────────────────────────────────────────

/// Fallback handler for PHF slots with no registered inventory entry.
/// Returns 500; should never be reached in a correctly-compiled binary.
fn placeholder_handler(_req: &Request) -> BoxFuture<'_, Response> {
    Box::pin(async { Response::internal_error() })
}

/// Returns `true` when the path contains no `:param` segments.
fn is_static_path(path: &str) -> bool {
    !path.split('/').any(|seg| seg.starts_with(':'))
}

/// Normalise a registered path to the canonical HashMap key form:
/// leading `/`, no trailing `/`, e.g. `"users"` → `"/users"`.
fn normalize_path(path: &str) -> Box<str> {
    let s = path.trim_end_matches('/');
    if s.is_empty() {
        "/".into()
    } else if s.starts_with('/') {
        s.into()
    } else {
        format!("/{}", s).into()
    }
}

/// Parse a parameterised path string into a `ParamRoute`.
///
/// Example: `"GET /users/:id"` → segments `[Static("users"), Param("id")]`,
/// `expected_slash_count = 2`.
fn make_param_route(method: HttpMethod, path: &str, handler: HandlerFn) -> ParamRoute {
    let expected_slash_count = path.as_bytes().iter().filter(|&&b| b == b'/').count();

    let segments: Vec<SegmentKind> = path
        .trim_matches('/')
        .split('/')
        .filter(|s| !s.is_empty())
        .map(|seg| {
            if let Some(name) = seg.strip_prefix(':') {
                SegmentKind::Param(name.into())
            } else {
                SegmentKind::Static(seg.into())
            }
        })
        .collect();

    ParamRoute {
        method,
        segments: segments.into_boxed_slice(),
        handler,
        expected_slash_count,
    }
}

