// src/middleware.rs – static middleware chain composition.
//
// Agents.md §11: middleware attributes must generate wrapper functions.
// The chain is compiled statically – each layer is a type that implements
// `Middleware`, and the chain is composed at compile time with zero-cost
// abstractions.
//
// Example:
//   #[middleware(Auth)]
//   #[get("/users")]
//   async fn list_users() {}
//
// Generates:
//   async fn list_users_wrapped(req: Request) {
//       Auth::call(req, list_users).await
//   }

use std::future::Future;
use std::pin::Pin;

use crate::{request::Request, response::Response};

/// Type alias for a boxed next-handler closure passed to middleware.
pub type Next<'a> = Box<dyn Fn(Request) -> Pin<Box<dyn Future<Output = Response> + Send>> + Send + 'a>;

/// A middleware layer.  Implement this trait for each middleware type.
///
/// ```rust
/// pub struct Auth;
/// impl Middleware for Auth {
///     async fn call(req: Request, next: Next<'_>) -> Response {
///         if req.headers.get("Authorization").is_none() {
///             return Response::new(StatusCode::UNAUTHORIZED, b"");
///         }
///         next(req).await
///     }
/// }
/// ```
pub trait Middleware: Send + Sync + 'static {
    fn call(
        req:  Request,
        next: Next<'static>,
    ) -> impl Future<Output = Response> + Send;
}

// ──────────────────────────────────────────────────────────────────────
// Built-in middleware
// ──────────────────────────────────────────────────────────────────────

/// Pass-through middleware – useful as a test double.
pub struct Passthrough;

impl Middleware for Passthrough {
    async fn call(req: Request, next: Next<'static>) -> Response {
        next(req).await
    }
}

/// Logging middleware – emits timing information to stdout.
pub struct Logging;

impl Middleware for Logging {
    async fn call(req: Request, next: Next<'static>) -> Response {
        let start = std::time::Instant::now();
        let path  = req.path.clone();
        let method = req.method.as_str();
        let resp  = next(req).await;
        println!(
            "{} {} → {} ({:?})",
            method,
            path,
            resp.status,
            start.elapsed(),
        );
        resp
    }
}
