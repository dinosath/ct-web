// src/server.rs – HTTP server binding and main dispatch loop.
//
// Agents.md §20 – runtime does only:
//   1. start async runtime     ← tokio::main in main.rs
//   2. bind socket             ← Server::run
//   3. dispatch requests       ← dispatch() in router/mod.rs
//
// This module provides a simple TCP-level server for demonstration.
// In production you would plug the dispatch function into hyper or axum's
// lower-level `Service` interface.

use std::sync::OnceLock;

use crate::{
    openapi,
    registry,
    router::{build::build_router, dispatch, HybridRouter},
};

// ──────────────────────────────────────────────────────────────────────
// Global static router  (Agents.md §8 "static ROUTER object")
// ──────────────────────────────────────────────────────────────────────

/// Lazily-initialised static trie router.
///
/// Built once from inventory-collected routes before the first request.
/// `OnceLock` guarantees this happens exactly once, without a mutex on
/// the hot dispatch path.
///
/// Migration note: when `#[comptime]` replaces `inventory`, this becomes
/// a genuine `static ROUTER: HybridRouter` initialised at compile time.
static ROUTER: OnceLock<&'static HybridRouter> = OnceLock::new();

/// Obtain a reference to the global router, building it on first access.
pub fn router() -> &'static HybridRouter {
    ROUTER.get_or_init(|| {
        build_router(registry::all_routes())
    })
}

// ──────────────────────────────────────────────────────────────────────
// Server configuration
// ──────────────────────────────────────────────────────────────────────

pub struct ServerConfig {
    pub title:   String,
    pub version: String,
}

impl Default for ServerConfig {
    fn default() -> Self {
        ServerConfig {
            title:   "comptime_web".to_string(),
            version: "0.1.0".to_string(),
        }
    }
}

// ──────────────────────────────────────────────────────────────────────
// Server
// ──────────────────────────────────────────────────────────────────────

pub struct Server {
    config: ServerConfig,
}

impl Server {
    pub fn new() -> Self {
        Server { config: ServerConfig::default() }
    }

    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.config.title = title.into();
        self
    }

    pub fn version(mut self, version: impl Into<String>) -> Self {
        self.config.version = version.into();
        self
    }

    /// Initialise the framework and run the server.
    ///
    /// This is the **only** place that performs startup work:
    ///   1. Build the static router trie from inventory routes.
    ///   2. Build the OpenAPI document.
    ///   3. Bind the TCP socket and serve requests.
    pub async fn run(self, addr: &str) -> std::io::Result<()> {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::TcpListener;

        // Step 1 – build router (O(routes) startup cost, then O(depth) per request).
        let router_ref = router();

        // Step 2 – build OpenAPI spec.
        openapi::build_openapi(
            &self.config.title,
            &self.config.version,
            registry::all_routes(),
        );

        println!("comptime_web listening on http://{}", addr);
        println!("OpenAPI spec at http://{}/openapi.json", addr);
        println!("Routes registered: {}", registry::all_routes().count());

        // Step 3 – serve.
        let listener = TcpListener::bind(addr).await?;

        loop {
            let (mut stream, peer) = listener.accept().await?;

            tokio::spawn(async move {
                // Stack-allocated read buffer — no heap allocation per connection.
                let mut buf = [0u8; 8192];
                let n = match stream.read(&mut buf).await {
                    Ok(n) if n > 0 => n,
                    _ => return,
                };

                let raw = std::str::from_utf8(&buf[..n]).unwrap_or("");
                let req = parse_http_request(raw);

                let resp = dispatch(router_ref, req).await;

                // Build headers and body into one contiguous buffer so a single
                // write_all syscall sends both — halves per-response syscall count.
                let body = &resp.body;
                let status = resp.status.as_u16();
                let mut out = Vec::with_capacity(128 + body.len());
                use std::io::Write as _;
                let _ = write!(
                    out,
                    "HTTP/1.1 {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    status, body.len(),
                );
                out.extend_from_slice(body);
                let _ = stream.write_all(&out).await;
            });
        }
    }
}

// ──────────────────────────────────────────────────────────────────────
// Minimal HTTP/1.1 request parser
// ──────────────────────────────────────────────────────────────────────

fn parse_http_request(raw: &str) -> crate::request::Request {
    use std::collections::HashMap;
    use crate::request::{HttpMethod, Request};

    let mut req = Request::default();

    let mut lines = raw.lines();
    if let Some(start) = lines.next() {
        let mut parts = start.splitn(3, ' ');
        if let Some(method_str) = parts.next() {
            req.method = HttpMethod::from_str(method_str).unwrap_or(HttpMethod::Get);
        }
        if let Some(path_and_query) = parts.next() {
            if let Some((path, query)) = path_and_query.split_once('?') {
                req.path = path.to_string();
                for kv in query.split('&') {
                    if let Some((k, v)) = kv.split_once('=') {
                        req.query_params.insert(k.to_string(), v.to_string());
                    }
                }
            } else {
                req.path = path_and_query.to_string();
            }
        }
    }

    // Parse headers until blank line.
    for line in lines.by_ref() {
        if line.is_empty() { break; }
        if let Some((key, val)) = line.split_once(": ") {
            req.headers.insert(key.to_lowercase(), val.to_string());
        }
    }

    req
}
