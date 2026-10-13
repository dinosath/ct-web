#![feature(type_info, rustc_attrs, const_cmp, const_trait_impl)]
#![allow(internal_features)]

use core::any::TypeId;
use core::mem::type_info::TypeKind;

/// Validates that every reflected injection field resolves to a declared
/// component, singleton, or configuration type in the application crate.
#[rustc_comptime]
pub fn validate_injections(
    components: &[TypeId],
    singletons: &[TypeId],
    configs: &[TypeId],
) -> bool {
    validate_group(components, components, singletons, configs)
        && validate_group(singletons, components, singletons, configs)
        && !has_cycles(components, singletons)
}

#[rustc_comptime]
fn validate_group(
    providers: &[TypeId],
    components: &[TypeId],
    singletons: &[TypeId],
    configs: &[TypeId],
) -> bool {
    let mut provider_index = 0;
    while provider_index < providers.len() {
        let provider = providers[provider_index];
        let mut field_index = 0;
        while field_index < provider.fields(0) {
            let field = provider.field(0, field_index);
            if field.has_attr("app::inject") {
                let dependency = injected_type(field.type_id());
                if !contains(components, dependency)
                    && !contains(singletons, dependency)
                    && !contains(configs, dependency)
                {
                    return false;
                }
            }
            field_index += 1;
        }
        provider_index += 1;
    }
    true
}

#[rustc_comptime]
fn contains(types: &[TypeId], needle: TypeId) -> bool {
    let mut index = 0;
    while index < types.len() {
        if types[index] == needle {
            return true;
        }
        index += 1;
    }
    false
}

#[rustc_comptime]
fn injected_type(field_type: TypeId) -> TypeId {
    if let TypeKind::Reference = field_type.info().kind {
        field_type.points_to().unwrap()
    } else {
        field_type
    }
}

#[rustc_comptime]
fn has_cycles(components: &[TypeId], singletons: &[TypeId]) -> bool {
    let max_depth = components.len() + singletons.len();
    let mut index = 0;
    while index < components.len() {
        let provider = components[index];
        if reaches(provider, provider, components, singletons, max_depth) {
            return true;
        }
        index += 1;
    }
    index = 0;
    while index < singletons.len() {
        let provider = singletons[index];
        if reaches(provider, provider, components, singletons, max_depth) {
            return true;
        }
        index += 1;
    }
    false
}

#[rustc_comptime]
fn reaches(
    origin: TypeId,
    current: TypeId,
    components: &[TypeId],
    singletons: &[TypeId],
    remaining_depth: usize,
) -> bool {
    if remaining_depth == 0 {
        return false;
    }
    let mut field_index = 0;
    while field_index < current.fields(0) {
        let field = current.field(0, field_index);
        if field.has_attr("app::inject") {
            let dependency = injected_type(field.type_id());
            if dependency == origin {
                return true;
            }
            if (contains(components, dependency) || contains(singletons, dependency))
                && reaches(
                    origin,
                    dependency,
                    components,
                    singletons,
                    remaining_depth - 1,
                )
            {
                return true;
            }
        }
        field_index += 1;
    }
    false
}
