#![feature(register_tool)]
#![register_tool(app)]
#![allow(dead_code)]

pub struct HealthController;

impl HealthController {
    #[app::route(method = "GET", path = "/health")]
    pub fn health() -> &'static str {
        "ok"
    }
}