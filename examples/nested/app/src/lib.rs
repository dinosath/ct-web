#![feature(register_tool)]
#![register_tool(app)]

pub mod entity;
pub mod routes;

pub use nested_controller::HealthController;