// src/params.rs – compile-time parameter metadata and runtime extraction.
//
// The `ParamInfo` / `ParamSource` structs mirror the `RouteMeta` layout
// described in Agents.md §5.  They are populated by the proc-macros and
// stored alongside each registered route.
//
// `extract_param` is the runtime helper generated inside handler wrappers.

use crate::request::Request;

// ──────────────────────────────────────────────────────────────────────
// Compile-time metadata (stored in RouteMeta)
// ──────────────────────────────────────────────────────────────────────

/// Where a handler parameter originates from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParamSource {
    Path,
    Query,
    Body,
    Header,
}

/// Compile-time description of a single handler parameter.
///
/// Every field is `'static` so the struct can live in a `static` slice
/// inside `RouteRegistration` (emitted by the proc-macro into inventory).
#[derive(Debug, Clone, Copy)]
pub struct ParamInfo {
    pub name:   &'static str,
    pub source: ParamSource,
    /// The Rust type name as a string (for display / debugging).
    pub type_name: &'static str,
    /// JSON Schema kind: "integer" | "number" | "boolean" | "string" | "object" | "array".
    /// Derived at compile time via `std::mem::type_info::Type::of::<T>()`.
    pub type_kind: &'static str,
    /// Pointer to `<T as JsonSchema>::schema` – called at startup to produce
    /// the OpenAPI schema node for this parameter.
    pub schema_fn: fn() -> crate::schema::SchemaNode,
}

// ──────────────────────────────────────────────────────────────────────
// `FromParam` – trait for types extractable from a raw string segment
// ──────────────────────────────────────────────────────────────────────

/// Types that can be constructed from a URL segment or query value.
///
/// Implemented for all common primitives; derive via `#[derive(FromParam)]`
/// is a future extension once `#[comptime]` is available.
pub trait FromParam: Sized {
    fn from_param(raw: &str) -> Result<Self, String>;
}

macro_rules! impl_from_param_parse {
    ($($t:ty),+) => {
        $(
            impl FromParam for $t {
                fn from_param(raw: &str) -> Result<Self, String> {
                    raw.parse::<$t>().map_err(|e| e.to_string())
                }
            }
        )+
    };
}

impl_from_param_parse!(u8, u16, u32, u64, u128, usize, i8, i16, i32, i64, i128, isize, f32, f64, bool);

impl FromParam for String {
    fn from_param(raw: &str) -> Result<Self, String> {
        Ok(raw.to_string())
    }
}

impl FromParam for Box<str> {
    fn from_param(raw: &str) -> Result<Self, String> {
        Ok(raw.into())
    }
}

// ──────────────────────────────────────────────────────────────────────
// `FromRequest` – trait for types extractable from the full request
// ──────────────────────────────────────────────────────────────────────

/// Types that can be extracted from the full `Request` context.
///
/// Used for JSON body types, service injection, etc.
pub trait FromRequest: Sized + Send {
    fn from_request(req: &Request) -> impl Future<Output = Self> + Send + '_;
}

use std::future::Future;

/// JSON body extractor: `Json<T>` can be used as a handler parameter.
pub struct Json<T>(pub T);

impl<T: serde::de::DeserializeOwned + Send> FromRequest for Json<T> {
    fn from_request(req: &Request) -> impl Future<Output = Self> + Send + '_ {
        async move {
            let val: T = serde_json::from_slice(&req.body)
                .expect("failed to deserialise JSON body");
            Json(val)
        }
    }
}

// ──────────────────────────────────────────────────────────────────────
// extract_param – called from generated handler wrappers
// ──────────────────────────────────────────────────────────────────────

/// Extract a parameter from the request.
///
/// Resolution order: path params → query params → header → body.
/// Panics with a descriptive message on parse failure (converts to 400 in
/// production via the error-handling middleware layer).
pub async fn extract_param<T: FromParam>(req: &Request, name: &str) -> T {
    // 1. Path parameter
    if let Some(raw) = req.path_params.get(name) {
        return T::from_param(raw).unwrap_or_else(|e| {
            panic!("path param '{}': {}", name, e)
        });
    }
    // 2. Query parameter
    if let Some(raw) = req.query_params.get(name) {
        return T::from_param(raw).unwrap_or_else(|e| {
            panic!("query param '{}': {}", name, e)
        });
    }
    // 3. Header
    if let Some(raw) = req.headers.get(name) {
        return T::from_param(raw).unwrap_or_else(|e| {
            panic!("header '{}': {}", name, e)
        });
    }
    panic!("required parameter '{}' not found in request", name);
}
