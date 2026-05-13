// src/response.rs – HTTP response representation.

use std::collections::HashMap;

// ──────────────────────────────────────────────────────────────────────
// StatusCode
// ──────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StatusCode(pub u16);

impl StatusCode {
    pub const OK:                  StatusCode = StatusCode(200);
    pub const CREATED:             StatusCode = StatusCode(201);
    pub const NO_CONTENT:          StatusCode = StatusCode(204);
    pub const BAD_REQUEST:         StatusCode = StatusCode(400);
    pub const UNAUTHORIZED:        StatusCode = StatusCode(401);
    pub const FORBIDDEN:           StatusCode = StatusCode(403);
    pub const NOT_FOUND:           StatusCode = StatusCode(404);
    pub const METHOD_NOT_ALLOWED:  StatusCode = StatusCode(405);
    pub const UNPROCESSABLE:       StatusCode = StatusCode(422);
    pub const INTERNAL_SERVER_ERR:   StatusCode = StatusCode(500);
    pub const INTERNAL_SERVER_ERROR: StatusCode = StatusCode(500);

    pub const fn as_u16(self) -> u16 { self.0 }
}

impl std::fmt::Display for StatusCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

// ──────────────────────────────────────────────────────────────────────
// Response
// ──────────────────────────────────────────────────────────────────────

#[derive(Debug)]
pub struct Response {
    pub status:  StatusCode,
    pub headers: HashMap<String, String>,
    pub body:    Vec<u8>,
}

impl Response {
    pub fn new(status: StatusCode, body: impl Into<Vec<u8>>) -> Self {
        let mut headers = HashMap::new();
        headers.insert("Content-Type".to_string(), "application/json".to_string());
        Response { status, headers, body: body.into() }
    }

    pub fn ok(body: impl Into<Vec<u8>>) -> Self {
        Self::new(StatusCode::OK, body)
    }

    pub fn json(status: StatusCode, value: &impl serde::Serialize) -> Self {
        let body = serde_json::to_vec(value).unwrap_or_default();
        Self::new(status, body)
    }

    pub fn not_found() -> Self {
        Self::new(StatusCode::NOT_FOUND, b"{\"error\":\"not found\"}".as_ref())
    }

    pub fn method_not_allowed() -> Self {
        Self::new(
            StatusCode::METHOD_NOT_ALLOWED,
            b"{\"error\":\"method not allowed\"}".as_ref(),
        )
    }

    pub fn internal_error() -> Self {
        Self::new(
            StatusCode::INTERNAL_SERVER_ERR,
            b"{\"error\":\"internal server error\"}".as_ref(),
        )
    }
}
