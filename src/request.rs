// src/request.rs – HTTP request representation.

use std::collections::HashMap;

// ──────────────────────────────────────────────────────────────────────
// HttpMethod
// ──────────────────────────────────────────────────────────────────────

/// HTTP verb used in route matching and `MethodHandler`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HttpMethod {
    Get,
    Post,
    Put,
    Delete,
    Patch,
    Head,
    Options,
}

impl HttpMethod {
    pub const fn as_str(self) -> &'static str {
        match self {
            HttpMethod::Get     => "GET",
            HttpMethod::Post    => "POST",
            HttpMethod::Put     => "PUT",
            HttpMethod::Delete  => "DELETE",
            HttpMethod::Patch   => "PATCH",
            HttpMethod::Head    => "HEAD",
            HttpMethod::Options => "OPTIONS",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        // Byte comparison avoids `.to_uppercase()` which allocates a String.
        // Methods are almost always uppercase already; this handles both cases.
        match s.len() {
            3 => match [s.as_bytes()[0] | 32, s.as_bytes()[1] | 32, s.as_bytes()[2] | 32] {
                [b'g', b'e', b't'] => Some(HttpMethod::Get),
                [b'p', b'u', b't'] => Some(HttpMethod::Put),
                _ => None,
            },
            4 => match [s.as_bytes()[0] | 32, s.as_bytes()[1] | 32,
                        s.as_bytes()[2] | 32, s.as_bytes()[3] | 32] {
                [b'p', b'o', b's', b't'] => Some(HttpMethod::Post),
                [b'h', b'e', b'a', b'd'] => Some(HttpMethod::Head),
                _ => None,
            },
            5 => match [s.as_bytes()[0] | 32, s.as_bytes()[1] | 32, s.as_bytes()[2] | 32,
                        s.as_bytes()[3] | 32, s.as_bytes()[4] | 32] {
                [b'p', b'a', b't', b'c', b'h'] => Some(HttpMethod::Patch),
                _ => None,
            },
            6 => match [s.as_bytes()[0] | 32, s.as_bytes()[1] | 32, s.as_bytes()[2] | 32,
                        s.as_bytes()[3] | 32, s.as_bytes()[4] | 32, s.as_bytes()[5] | 32] {
                [b'd', b'e', b'l', b'e', b't', b'e'] => Some(HttpMethod::Delete),
                _ => None,
            },
            7 => match [s.as_bytes()[0] | 32, s.as_bytes()[1] | 32, s.as_bytes()[2] | 32,
                        s.as_bytes()[3] | 32, s.as_bytes()[4] | 32, s.as_bytes()[5] | 32,
                        s.as_bytes()[6] | 32] {
                [b'o', b'p', b't', b'i', b'o', b'n', b's'] => Some(HttpMethod::Options),
                _ => None,
            },
            _ => None,
        }
    }
}

// ──────────────────────────────────────────────────────────────────────
// Request
// ──────────────────────────────────────────────────────────────────────

/// An incoming HTTP request.
///
/// All allocations happen once when the request is parsed; the router and
/// handler chain then operate on shared references into this struct.
#[derive(Debug)]
pub struct Request {
    pub method:       HttpMethod,
    pub path:         String,
    pub headers:      HashMap<String, String>,
    pub query_params: HashMap<String, String>,
    /// Path parameter values extracted during routing (name → value).
    pub path_params:  HashMap<String, String>,
    pub body:         Vec<u8>,
}

impl Request {
    /// Split the URL path into segments (no allocations on hot path in the
    /// router – the trie Walk operates on the raw `&str`).
    pub fn path_segments(&self) -> impl Iterator<Item = &str> {
        self.path
            .trim_matches('/')
            .split('/')
            .filter(|s| !s.is_empty())
    }

    /// Look up a path parameter by name.
    pub fn param(&self, name: &str) -> Option<&str> {
        self.path_params.get(name).map(String::as_str)
    }

    /// Look up a query parameter by name.
    pub fn query(&self, name: &str) -> Option<&str> {
        self.query_params.get(name).map(String::as_str)
    }

    /// Get the raw request body as a `&str`.
    pub fn body_str(&self) -> &str {
        std::str::from_utf8(&self.body).unwrap_or("")
    }
}

impl Default for Request {
    fn default() -> Self {
        Request {
            method:       HttpMethod::Get,
            path:         String::new(),
            headers:      HashMap::new(),
            query_params: HashMap::new(),
            path_params:  HashMap::new(),
            body:         Vec::new(),
        }
    }
}
