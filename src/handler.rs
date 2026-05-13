// src/handler.rs – handler type alias and trait.
//
// `HandlerFn` is the single erased function type stored in the static router.
// It avoids vtable overhead by using a plain function pointer; the future is
// heap-allocated once per request (necessary because async fn → opaque type).

use std::future::Future;
use std::pin::Pin;

use crate::{request::Request, response::Response};

/// A heap-allocated, pinned future returned by every handler wrapper.
/// This is the only allocation on the hot path.
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// The erased, type-safe handler function pointer stored in `MethodHandler`.
///
/// The proc-macro generates a concrete `fn(&Request) -> BoxFuture<Response>`
/// for every annotated function and registers it in the static router.
pub type HandlerFn = for<'a> fn(&'a Request) -> BoxFuture<'a, Response>;

// ──────────────────────────────────────────────────────────────────────
// Handler trait (optional higher-level interface)
// ──────────────────────────────────────────────────────────────────────

/// Trait implemented by typed handler structs (e.g., controllers).
/// Not required for proc-macro-annotated functions.
pub trait Handler: Send + Sync + 'static {
    fn call<'a>(&'a self, req: &'a Request) -> BoxFuture<'a, Response>;
}
