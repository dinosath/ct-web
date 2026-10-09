#![feature(register_tool)]
#![register_tool(app)]

mod config;
mod controller;
mod entity;
mod service;
mod user_service;

#[app::main]
fn main() {}
