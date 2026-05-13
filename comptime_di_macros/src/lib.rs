// comptime_di_macros – procedural macros for compile-time dependency injection.
//
// Provides:
//   #[derive(Injectable)]  – auto-implement Injectable for structs whose fields
//                            are themselves Injectable or registered services.
//   #[inject]              – mark a function/method to have its parameters
//                            resolved from the ServiceRegistry automatically.
//   #[component]           – register a struct as a singleton service component.
//
// Migration note (Agents.md §16):
//   When #[comptime] + full field-level type_info resolution stabilises,
//   these macros will be replaced by comptime functions that inspect struct
//   fields via std::mem::type_info and wire dependencies automatically.

use proc_macro::TokenStream;
use proc_macro2::Span;
use quote::quote;
use syn::{parse_macro_input, ItemFn, LitStr};

// ──────────────────────────────────────────────────────────────────────
// #[derive(Injectable)] – auto-wire struct dependencies
// ──────────────────────────────────────────────────────────────────────
//
// For a struct like:
//
//   #[derive(Injectable)]
//   struct OrderService {
//       user_repo: Arc<UserRepository>,
//       db_pool:   Arc<PgPool>,
//   }
//
// Generates:
//
//   impl Injectable for OrderService {
//       fn resolve(registry: &ServiceRegistry) -> Self {
//           Self {
//               user_repo: registry.get::<UserRepository>(),
//               db_pool:   registry.get::<PgPool>(),
//           }
//       }
//   }
//
//   inventory::submit! {
//       ServiceRegistration {
//           type_name: "OrderService",
//           type_id_fn: || std::any::TypeId::of::<OrderService>(),
//           deps: &["UserRepository", "PgPool"],
//           scope: Scope::Singleton,
//       }
//   }

#[proc_macro_derive(Injectable, attributes(scope, inject_skip))]
pub fn derive_injectable(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as syn::DeriveInput);
    match derive_injectable_inner(input) {
        Ok(ts) => ts,
        Err(e) => e.to_compile_error().into(),
    }
}

fn derive_injectable_inner(input: syn::DeriveInput) -> syn::Result<TokenStream> {
    let name = &input.ident;
    let name_str = name.to_string();

    // Parse scope attribute: #[scope("singleton")] or #[scope("request")]
    let scope = extract_scope_attr(&input.attrs);
    let scope_variant = match scope.as_str() {
        "request"  => quote! { ::comptime_di::Scope::Request },
        "transient" => quote! { ::comptime_di::Scope::Transient },
        _          => quote! { ::comptime_di::Scope::Singleton },
    };

    let fields = match &input.data {
        syn::Data::Struct(s) => parse_injectable_fields(&s.fields)?,
        _ => return Err(syn::Error::new_spanned(
            &input.ident,
            "#[derive(Injectable)] only supports structs",
        )),
    };

    // Generate field resolution expressions.
    let field_resolutions: Vec<proc_macro2::TokenStream> = fields
        .iter()
        .map(|f| {
            let field_ident = &f.ident;
            let inner_ty = &f.inner_ty;
            if f.skip {
                // Fields marked #[inject_skip] use Default.
                quote! { #field_ident: Default::default() }
            } else {
                quote! { #field_ident: registry.get::<#inner_ty>() }
            }
        })
        .collect();

    // Collect dependency type names for registration metadata.
    let dep_names: Vec<String> = fields
        .iter()
        .filter(|f| !f.skip)
        .map(|f| f.inner_ty_name.clone())
        .collect();
    let dep_name_literals: Vec<&str> = dep_names.iter().map(|s| s.as_str()).collect();
    let dep_count = dep_name_literals.len();

    // Registration ident for inventory.
    let reg_ident = syn::Ident::new(
        &format!("__DI_REG_{}", name_str.to_uppercase()),
        Span::call_site(),
    );

    let expanded = quote! {
        impl ::comptime_di::Injectable for #name {
            fn resolve(registry: &::comptime_di::ServiceRegistry) -> Self {
                Self {
                    #( #field_resolutions ),*
                }
            }

            fn dependencies() -> &'static [&'static str] {
                static DEPS: [&str; #dep_count] = [ #( #dep_name_literals ),* ];
                &DEPS
            }
        }

        // ── inventory registration ────────────────────────────────────
        #[allow(non_upper_case_globals)]
        static #reg_ident: [&str; #dep_count] = [ #( #dep_name_literals ),* ];

        ::inventory::submit! {
            ::comptime_di::ServiceRegistration {
                type_name:  #name_str,
                type_id_fn: || ::std::any::TypeId::of::<#name>(),
                deps:       &#reg_ident,
                scope:      #scope_variant,
            }
        }
    };

    Ok(TokenStream::from(expanded))
}

// ──────────────────────────────────────────────────────────────────────
// #[component] – register a factory function as a service provider
// ──────────────────────────────────────────────────────────────────────
//
// Usage:
//   #[component]
//   fn create_pool() -> PgPool { ... }
//
// Registers the return type as an available service.  The function body
// becomes the factory invoked once (singleton) during registry initialisation.

#[proc_macro_attribute]
pub fn component(attr: TokenStream, item: TokenStream) -> TokenStream {
    let scope_str = if attr.is_empty() {
        "singleton".to_string()
    } else {
        let lit = parse_macro_input!(attr as LitStr);
        lit.value()
    };

    let func = parse_macro_input!(item as ItemFn);
    let fn_name = &func.sig.ident;
    let fn_vis = &func.vis;

    // Extract return type.
    let ret_ty = match &func.sig.output {
        syn::ReturnType::Type(_, ty) => ty.clone(),
        syn::ReturnType::Default => {
            return syn::Error::new_spanned(
                &func.sig.ident,
                "#[component] functions must have a return type",
            )
            .to_compile_error()
            .into()
        }
    };

    let ret_ty_str = quote!(#ret_ty).to_string().replace(' ', "");
    let scope_variant = match scope_str.as_str() {
        "request"   => quote! { ::comptime_di::Scope::Request },
        "transient" => quote! { ::comptime_di::Scope::Transient },
        _           => quote! { ::comptime_di::Scope::Singleton },
    };

    let reg_ident = syn::Ident::new(
        &format!("__COMPONENT_REG_{}", fn_name.to_string().to_uppercase()),
        Span::call_site(),
    );

    let expanded = quote! {
        #func

        #[allow(non_upper_case_globals)]
        #fn_vis static #reg_ident: [&str; 0] = [];

        ::inventory::submit! {
            ::comptime_di::ServiceRegistration {
                type_name:  #ret_ty_str,
                type_id_fn: || ::std::any::TypeId::of::<#ret_ty>(),
                deps:       &#reg_ident,
                scope:      #scope_variant,
            }
        }
    };

    TokenStream::from(expanded)
}

// ──────────────────────────────────────────────────────────────────────
// #[inject] – resolve function parameters from the registry
// ──────────────────────────────────────────────────────────────────────
//
// Usage:
//   #[inject]
//   async fn handle_order(svc: Arc<OrderService>, req: &Request) -> Response { … }
//
// Generates a wrapper that fetches each Arc<T> parameter from the global
// service registry before calling the original function.

#[proc_macro_attribute]
pub fn inject(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let func = parse_macro_input!(item as ItemFn);
    let fn_name = &func.sig.ident;
    let fn_vis = &func.vis;
    let fn_attrs = &func.attrs;
    let fn_sig = &func.sig;
    let fn_block = &func.block;

    let wrapper_ident = syn::Ident::new(
        &format!("{}_injected", fn_name),
        Span::call_site(),
    );

    // Parse parameters: Arc<T> params are resolved, others pass through.
    let mut wrapper_params = Vec::new();
    let mut resolve_stmts = Vec::new();
    let mut call_args = Vec::new();

    for input in &fn_sig.inputs {
        match input {
            syn::FnArg::Typed(pat_type) => {
                let pat = &pat_type.pat;
                let ty = &pat_type.ty;

                if let Some(inner) = extract_arc_inner(ty) {
                    // This is an Arc<T> – resolve from registry.
                    resolve_stmts.push(quote! {
                        let #pat: ::std::sync::Arc<#inner> =
                            ::comptime_di::service_registry().get::<#inner>();
                    });
                    call_args.push(quote! { #pat });
                } else {
                    // Non-injected parameter – keep in wrapper signature.
                    wrapper_params.push(quote! { #pat: #ty });
                    call_args.push(quote! { #pat });
                }
            }
            syn::FnArg::Receiver(_) => {
                wrapper_params.push(quote! { self });
                call_args.push(quote! { self });
            }
        }
    }

    let ret = &fn_sig.output;
    let is_async = fn_sig.asyncness.is_some();

    let inner_call = if is_async {
        quote! { #fn_name(#( #call_args ),*).await }
    } else {
        quote! { #fn_name(#( #call_args ),*) }
    };

    let async_kw = if is_async {
        quote! { async }
    } else {
        quote! {}
    };

    let expanded = quote! {
        // Keep the original function.
        #( #fn_attrs )*
        #fn_vis #fn_sig #fn_block

        // Injected wrapper that resolves Arc<T> deps from the registry.
        #[doc(hidden)]
        #fn_vis #async_kw fn #wrapper_ident(#( #wrapper_params ),*) #ret {
            #( #resolve_stmts )*
            #inner_call
        }
    };

    TokenStream::from(expanded)
}

// ──────────────────────────────────────────────────────────────────────
// Helpers
// ──────────────────────────────────────────────────────────────────────

struct InjectableField {
    ident:         syn::Ident,
    inner_ty:      syn::Type,     // the T inside Arc<T>
    inner_ty_name: String,
    skip:          bool,
}

fn parse_injectable_fields(fields: &syn::Fields) -> syn::Result<Vec<InjectableField>> {
    let named = match fields {
        syn::Fields::Named(n) => &n.named,
        _ => {
            return Err(syn::Error::new(
                Span::call_site(),
                "#[derive(Injectable)] requires named fields",
            ))
        }
    };

    named
        .iter()
        .map(|f| {
            let ident = f.ident.clone().unwrap();
            let skip = f.attrs.iter().any(|a| a.path().is_ident("inject_skip"));
            let ty = &f.ty;

            // Try to unwrap Arc<T> → T; if not Arc, use the type directly.
            let (inner_ty, inner_ty_name) = if let Some(inner) = extract_arc_inner(ty) {
                let name = quote!(#inner).to_string().replace(' ', "");
                (inner, name)
            } else {
                let name = quote!(#ty).to_string().replace(' ', "");
                (ty.clone(), name)
            };

            Ok(InjectableField {
                ident,
                inner_ty,
                inner_ty_name,
                skip,
            })
        })
        .collect()
}

fn extract_scope_attr(attrs: &[syn::Attribute]) -> String {
    for attr in attrs {
        if attr.path().is_ident("scope") {
            if let Ok(lit) = attr.parse_args::<LitStr>() {
                return lit.value();
            }
        }
    }
    "singleton".to_string()
}

/// Extract the inner type T from `Arc<T>` or `std::sync::Arc<T>`.
fn extract_arc_inner(ty: &syn::Type) -> Option<syn::Type> {
    if let syn::Type::Path(p) = ty {
        let seg = p.path.segments.last()?;
        if seg.ident == "Arc" {
            if let syn::PathArguments::AngleBracketed(ab) = &seg.arguments {
                if let Some(syn::GenericArgument::Type(inner)) = ab.args.first() {
                    return Some(inner.clone());
                }
            }
        }
    }
    None
}
