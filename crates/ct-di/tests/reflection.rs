#![feature(register_tool)]
#![register_tool(app)]
#![allow(dead_code)]

use core::any::TypeId;

#[app::component]
struct Repository;

#[app::config]
struct DatabaseConfig;

#[app::singleton]
struct UserService {
    #[app::inject]
    repository: Repository,
    #[app::inject]
    config: DatabaseConfig,
}

struct Unregistered;

#[app::singleton]
struct BrokenService {
    #[app::inject]
    missing: Unregistered,
}

#[app::component]
struct FirstCycleNode {
    #[app::inject]
    next: &'static SecondCycleNode,
}

#[app::component]
struct SecondCycleNode {
    #[app::inject]
    next: &'static FirstCycleNode,
}

const VALID_GRAPH: bool = ct_di::validate_injections(
    &[TypeId::of::<Repository>()],
    &[TypeId::of::<UserService>()],
    &[TypeId::of::<DatabaseConfig>()],
);

const INVALID_GRAPH: bool = ct_di::validate_injections(&[], &[TypeId::of::<BrokenService>()], &[]);
const CYCLIC_GRAPH: bool = ct_di::validate_injections(
    &[
        TypeId::of::<FirstCycleNode>(),
        TypeId::of::<SecondCycleNode>(),
    ],
    &[],
    &[],
);

#[test]
fn accepts_registered_injection_fields() {
    assert!(VALID_GRAPH);
}

#[test]
fn rejects_unregistered_injection_fields() {
    assert!(!INVALID_GRAPH);
}

#[test]
fn rejects_dependency_cycles() {
    assert!(!CYCLIC_GRAPH);
}
