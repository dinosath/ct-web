// src/openapi.rs – compile-time OpenAPI 3.0 document generation.
//
// Agents.md §13: generate OpenAPI during compilation.
//   Input:  route metadata + parameter types + response types
//   Output: OPENAPI_SPEC static string / /openapi.json endpoint
//
// The document is built once at startup from inventory-collected route
// metadata and their associated `JsonSchema` implementations.
// With `#[comptime]` this will move entirely to compile time.

use std::collections::HashMap;

use serde::Serialize;

use crate::{
    params::ParamSource,
    registry::RouteRegistration,
    request::HttpMethod,
    schema::{JsonSchema, SchemaNode},
};

// ──────────────────────────────────────────────────────────────────────
// OpenAPI document structures
// ──────────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct OpenApiDoc {
    pub openapi: &'static str,
    pub info:    OpenApiInfo,
    pub paths:   HashMap<String, PathItem>,
}

#[derive(Debug, Serialize)]
pub struct OpenApiInfo {
    pub title:   String,
    pub version: String,
}

#[derive(Debug, Default, Serialize)]
pub struct PathItem {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub get:    Option<Operation>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub post:   Option<Operation>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub put:    Option<Operation>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delete: Option<Operation>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub patch:  Option<Operation>,
}

#[derive(Debug, Serialize)]
pub struct Operation {
    pub summary:    String,
    pub parameters: Vec<Parameter>,
    pub responses:  HashMap<String, ApiResponse>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_body: Option<RequestBody>,
}

#[derive(Debug, Serialize)]
pub struct Parameter {
    pub name:        String,
    #[serde(rename = "in")]
    pub location:    String,
    pub required:    bool,
    pub schema:      SchemaNode,
}

#[derive(Debug, Serialize)]
pub struct ApiResponse {
    pub description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content:     Option<HashMap<String, MediaType>>,
}

#[derive(Debug, Serialize)]
pub struct MediaType {
    pub schema: SchemaNode,
}

#[derive(Debug, Serialize)]
pub struct RequestBody {
    pub required: bool,
    pub content:  HashMap<String, MediaType>,
}

// ──────────────────────────────────────────────────────────────────────
// Static storage
// ──────────────────────────────────────────────────────────────────────

/// The serialised OpenAPI document stored as a static string once built.
///
/// Agents.md §13: `static OPENAPI_SPEC: &str`
static OPENAPI_SPEC: std::sync::OnceLock<String> = std::sync::OnceLock::new();

/// Access the cached OpenAPI spec (JSON string).
pub fn openapi_spec() -> &'static str {
    OPENAPI_SPEC.get().map(|s| s.as_str()).unwrap_or("{}")
}

// ──────────────────────────────────────────────────────────────────────
// Builder
// ──────────────────────────────────────────────────────────────────────

/// Build and cache the OpenAPI document from all registered routes.
///
/// Called once during `Server::run()`.
pub fn build_openapi(
    title:   impl Into<String>,
    version: impl Into<String>,
    routes:  impl IntoIterator<Item = &'static RouteRegistration>,
) {
    let doc = build_doc(title.into(), version.into(), routes);
    let json = serde_json::to_string_pretty(&doc).unwrap_or_default();
    let _ = OPENAPI_SPEC.set(json);
}

fn build_doc(
    title:   String,
    version: String,
    routes:  impl IntoIterator<Item = &'static RouteRegistration>,
) -> OpenApiDoc {
    let mut paths: HashMap<String, PathItem> = HashMap::new();

    for reg in routes {
        // Convert `:id` style params to `{id}` for OpenAPI.
        let openapi_path = convert_path(reg.path);
        let path_item = paths.entry(openapi_path).or_default();

        // Use compile-time type metadata when available; fall back to
        // path-pattern scanning for handlers that take `req: &Request` directly.
        let (parameters, request_body) = params_for_operation(reg.params, reg.path);

        let operation = Operation {
            summary:      format!("{} {}", reg.method.as_str(), reg.path),
            parameters,
            responses:    default_responses(),
            request_body,
        };

        match reg.method {
            HttpMethod::Get    => path_item.get    = Some(operation),
            HttpMethod::Post   => path_item.post   = Some(operation),
            HttpMethod::Put    => path_item.put    = Some(operation),
            HttpMethod::Delete => path_item.delete = Some(operation),
            HttpMethod::Patch  => path_item.patch  = Some(operation),
            _ => {}
        }
    }

    OpenApiDoc {
        openapi: "3.0.3",
        info: OpenApiInfo { title, version },
        paths,
    }
}

/// Build parameter and request-body metadata for one route.
///
/// Uses the compile-time `ParamInfo` slice (populated by the route proc-macro
/// via `std::mem::type_info`).  Falls back to path-string scanning when the
/// handler takes `req: &Request` directly and no `ParamInfo` was generated.
fn params_for_operation(
    params: &'static [crate::params::ParamInfo],
    path:   &str,
) -> (Vec<Parameter>, Option<RequestBody>) {
    if params.is_empty() {
        // Fallback: extract path params from the URL pattern (no type info).
        return (extract_path_params(path), None);
    }

    let mut parameters = Vec::new();
    let mut body_schema: Option<SchemaNode> = None;

    for p in params {
        match p.source {
            ParamSource::Path => parameters.push(Parameter {
                name:     p.name.to_string(),
                location: "path".to_string(),
                required: true,
                schema:   (p.schema_fn)(),  // compile-time type via JsonSchema
            }),
            ParamSource::Query => parameters.push(Parameter {
                name:     p.name.to_string(),
                location: "query".to_string(),
                required: false,
                schema:   (p.schema_fn)(),
            }),
            ParamSource::Header => parameters.push(Parameter {
                name:     p.name.to_string(),
                location: "header".to_string(),
                required: false,
                schema:   (p.schema_fn)(),
            }),
            ParamSource::Body => {
                // First body param wins; its JsonSchema drives the request body.
                if body_schema.is_none() {
                    body_schema = Some((p.schema_fn)());
                }
            }
        }
    }

    let request_body = body_schema.map(|schema| {
        let mut content = HashMap::new();
        content.insert("application/json".to_string(), MediaType { schema });
        RequestBody { required: true, content }
    });

    (parameters, request_body)
}

/// Convert URL path from `:id` style to `{id}` (OpenAPI convention).
fn convert_path(path: &str) -> String {
    path.split('/')
        .map(|seg| {
            if let Some(name) = seg.strip_prefix(':') {
                format!("{{{}}}", name)
            } else {
                seg.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("/")
}

/// Extract path parameters from a route pattern like `/users/:id`.
fn extract_path_params(path: &str) -> Vec<Parameter> {
    path.split('/')
        .filter_map(|seg| seg.strip_prefix(':'))
        .map(|name| Parameter {
            name:     name.to_string(),
            location: "path".to_string(),
            required: true,
            schema:   SchemaNode::String,
        })
        .collect()
}

fn default_responses() -> HashMap<String, ApiResponse> {
    let mut map = HashMap::new();
    map.insert(
        "200".to_string(),
        ApiResponse {
            description: "Success".to_string(),
            content:     None,
        },
    );
    map.insert(
        "404".to_string(),
        ApiResponse {
            description: "Not found".to_string(),
            content:     None,
        },
    );
    map
}

// ──────────────────────────────────────────────────────────────────────
// OpenAPI endpoint handler
// ──────────────────────────────────────────────────────────────────────

/// Built-in handler for `GET /openapi.json`.
pub fn openapi_handler(
    _req: &crate::request::Request,
) -> crate::handler::BoxFuture<'_, crate::response::Response> {
    Box::pin(async {
        let body = openapi_spec().as_bytes().to_vec();
        let mut resp = crate::response::Response::ok(body);
        resp.headers.insert(
            "Content-Type".to_string(),
            "application/json".to_string(),
        );
        resp
    })
}
