use core::any::TypeId;
use core::mem::type_info::{FnDef, TypeKind};

pub const ROUTE_ATTRIBUTE: &str = "app::route";
pub const MIDDLEWARE_ATTRIBUTE: &str = "app::middleware";

#[rustc_comptime]
pub fn route_functions() -> &'static [TypeId] {
    core::mem::type_info::all_functions_with_attr(ROUTE_ATTRIBUTE)
}

#[rustc_comptime]
pub fn middleware_functions() -> &'static [TypeId] {
    core::mem::type_info::all_functions_with_attr(MIDDLEWARE_ATTRIBUTE)
}

#[derive(Clone, Copy, Debug)]
pub struct RouteMetadata {
    pub handler: TypeId,
    pub name: &'static str,
    pub method: &'static str,
    pub path: &'static str,
}
#[derive(Clone, Copy, Debug)]
pub struct MiddlewareMetadata {
    pub handler: TypeId,
    pub name: &'static str,
    pub order: Option<&'static str>,
}

#[rustc_comptime]
pub fn route_metadata(handler: TypeId) -> Option<RouteMetadata> {
    if !handler.has_attr(ROUTE_ATTRIBUTE) {
        return None;
    }

    let method = match handler.attr_value(ROUTE_ATTRIBUTE, "method") {
        Some(value) => value,
        None => return None,
    };
    let path = match handler.attr_value(ROUTE_ATTRIBUTE, "path") {
        Some(value) => value,
        None => return None,
    };
    let TypeKind::FnDef(FnDef { name, .. }) = handler.info().kind else {
        return None;
    };

    Some(RouteMetadata {
        handler,
        name,
        method,
        path,
    })
}

#[rustc_comptime]
pub fn route_metadata_array<const N: usize>(
    handlers: &[TypeId],
) -> Option<[Option<RouteMetadata>; N]> {
    if handlers.len() != N {
        return None;
    }

    let mut metadata = [None; N];
    let mut index = 0;
    while index < handlers.len() {
        metadata[index] = route_metadata(handlers[index]);
        index += 1;
    }
    Some(metadata)
}

#[rustc_comptime]
pub fn middleware_metadata(handler: TypeId) -> Option<MiddlewareMetadata> {
    if !handler.has_attr(MIDDLEWARE_ATTRIBUTE) {
        return None;
    }

    let TypeKind::FnDef(FnDef { name, .. }) = handler.info().kind else {
        return None;
    };

    Some(MiddlewareMetadata {
        handler,
        name,
        order: handler.attr_value(MIDDLEWARE_ATTRIBUTE, "order"),
    })
}

#[rustc_comptime]
pub fn middleware_metadata_array<const N: usize>(
    handlers: &[TypeId],
) -> Option<[Option<MiddlewareMetadata>; N]> {
    if handlers.len() != N {
        return None;
    }

    let mut metadata = [None; N];
    let mut index = 0;
    while index < handlers.len() {
        metadata[index] = middleware_metadata(handlers[index]);
        index += 1;
    }
    Some(metadata)
}
