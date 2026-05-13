// comptime_web_macros – procedural macros for the compile-time web framework.
//
// Migration note (Agents.md §16):
//   Once `#[comptime]` stabilises on nightly these proc-macros will be
//   replaced by comptime functions that scan the crate via the reflection API.
//   The public developer API (#[get], #[post], …) will remain identical.

use proc_macro::TokenStream;
use proc_macro2::Span;
use zyn::quote::quote;
use syn::{
    parse_macro_input, parse_quote,
    punctuated::Punctuated,
    token::Comma,
    FnArg, ItemFn, ItemImpl, LitStr, Pat, PatType, Type,
};

// ──────────────────────────────────────────────────────────────────────
// HTTP-method route macros
// ──────────────────────────────────────────────────────────────────────

/// `#[get("/path")]` – register an async handler for HTTP GET.
#[proc_macro_attribute]
pub fn get(attr: TokenStream, item: TokenStream) -> TokenStream {
    route_macro("GET", attr, item)
}

/// `#[post("/path")]` – register an async handler for HTTP POST.
#[proc_macro_attribute]
pub fn post(attr: TokenStream, item: TokenStream) -> TokenStream {
    route_macro("POST", attr, item)
}

/// `#[put("/path")]` – register an async handler for HTTP PUT.
#[proc_macro_attribute]
pub fn put(attr: TokenStream, item: TokenStream) -> TokenStream {
    route_macro("PUT", attr, item)
}

/// `#[delete("/path")]` – register an async handler for HTTP DELETE.
#[proc_macro_attribute]
pub fn delete(attr: TokenStream, item: TokenStream) -> TokenStream {
    route_macro("DELETE", attr, item)
}

/// `#[patch("/path")]` – register an async handler for HTTP PATCH.
#[proc_macro_attribute]
pub fn patch(attr: TokenStream, item: TokenStream) -> TokenStream {
    route_macro("PATCH", attr, item)
}

// ──────────────────────────────────────────────────────────────────────
// Core route-macro implementation
// ──────────────────────────────────────────────────────────────────────

fn route_macro(method: &str, attr: TokenStream, item: TokenStream) -> TokenStream {
    // Parse the path literal from the attribute, e.g. `"/users/:id"`.
    let path_lit = parse_macro_input!(attr as LitStr);
    let path_str = path_lit.value();

    // Parse the annotated async function.
    let func = parse_macro_input!(item as ItemFn);
    let fn_name = &func.sig.ident;
    let fn_vis  = &func.vis;

    // Build `<fn_name>_comptime_handler` – the erased async wrapper that the
    // router calls at runtime.  Parameters that look like plain types (not
    // `req: &Request`) are extracted from the request automatically.
    let (param_extraction, call_args, param_infos) = generate_param_extraction(&func, &path_str);

    // Unique identifier for the inventory registration symbol.
    let _registration_ident = syn::Ident::new(
        &format!("__ROUTE_{}_{}", method, sanitize_path(&path_str)),
        Span::call_site(),
    );
    // Static slice holding compile-time param metadata for this route.
    let params_ident = syn::Ident::new(
        &format!("__PARAMS_{}_{}", method, sanitize_path(&path_str)),
        Span::call_site(),
    );
    let wrapper_ident = syn::Ident::new(
        &format!("{}_comptime_handler", fn_name),
        Span::call_site(),
    );

    let method_variant = method_to_variant(method);

    let expanded = quote! {
        // ── original handler function unchanged ──────────────────────────
        #func

        // ── erased wrapper: fn(&Request) -> BoxFuture<Response> ──────────
        #[doc(hidden)]
        #fn_vis fn #wrapper_ident(
            req: &::comptime_web::request::Request,
        ) -> ::comptime_web::handler::BoxFuture<'_, ::comptime_web::response::Response> {
            Box::pin(async move {
                #param_extraction
                #fn_name(#call_args).await
            })
        }

        // ── param metadata static (replaced by #[comptime] in future) ─────
        #[doc(hidden)]
        #[allow(non_upper_case_globals)]
        static #params_ident: &[::comptime_web::params::ParamInfo] = &[
            #(#param_infos),*
        ];

        // ── inventory registration (replaced by #[comptime] in future) ────
        #[allow(non_upper_case_globals)]
        ::inventory::submit! {
            ::comptime_web::registry::RouteRegistration {
                method:  ::comptime_web::request::HttpMethod::#method_variant,
                path:    #path_lit,
                handler: #wrapper_ident,
                params:  #params_ident,
            }
        }
    };

    TokenStream::from(expanded)
}

// ──────────────────────────────────────────────────────────────────────
// Parameter extraction codegen
// ──────────────────────────────────────────────────────────────────────

/// Returns `(extraction_stmts, call_arg_list, param_infos)` for an async handler function.
///
/// `param_infos` is a `Vec<TokenStream>` where each element is a
/// `::comptime_web::params::ParamInfo { … }` literal.  Each entry carries:
///   - `type_kind` – derived at compile time via `type_info_kind::<T>()`
///   - `schema_fn` – `<T as JsonSchema>::schema` function pointer
///
/// Supported parameter patterns:
///   - `req: &Request`  → passed through directly
///   - Anything else    → treated as a path/query/body parameter and parsed
///     from the request via `comptime_web::params::extract_param`.
fn generate_param_extraction(
    func: &ItemFn,
    path: &str,
) -> (proc_macro2::TokenStream, proc_macro2::TokenStream, Vec<proc_macro2::TokenStream>) {
    let mut stmts = proc_macro2::TokenStream::new();
    let mut args: Punctuated<proc_macro2::TokenStream, Comma> = Punctuated::new();
    let mut infos: Vec<proc_macro2::TokenStream> = Vec::new();

    for input in &func.sig.inputs {
        match input {
            FnArg::Receiver(_) => {
                // `self` – only relevant inside `#[controller]` impls; skip.
            }
            FnArg::Typed(PatType { pat, ty, .. }) => {
                // Detect `req: &Request` and forward directly.
                if is_request_ref(ty) {
                    args.push(quote! { req });
                } else if let Pat::Ident(ident) = pat.as_ref() {
                    let param_name = ident.ident.to_string();
                    let param_ident = &ident.ident;
                    // Emit: `let param_name: T = extract_param(req, "param_name").await;`
                    stmts.extend(quote! {
                        let #param_ident: #ty =
                            ::comptime_web::params::extract_param::<#ty>(req, #param_name).await;
                    });
                    args.push(quote! { #param_ident });

                    // Determine ParamSource: path if `:name` appears in the path template.
                    let is_path_param = path
                        .split('/')
                        .any(|seg| seg == format!(":{}", param_name));
                    let source = if is_path_param {
                        quote! { ::comptime_web::params::ParamSource::Path }
                    } else {
                        quote! { ::comptime_web::params::ParamSource::Body }
                    };

                    // ParamInfo literal.
                    // - `type_kind` is evaluated by the compiler at const-eval time
                    //   via `std::mem::type_info::Type::of::<T>()` inside `type_info_kind`.
                    // - `schema_fn` is a function-item-to-pointer coercion; requires
                    //   the type to implement `JsonSchema`.
                    infos.push(quote! {
                        ::comptime_web::params::ParamInfo {
                            name:      #param_name,
                            source:    #source,
                            type_name: stringify!(#ty),
                            type_kind: ::comptime_web::schema::type_info_kind::<#ty>(),
                            schema_fn: <#ty as ::comptime_web::schema::JsonSchema>::schema,
                        }
                    });
                }
            }
        }
    }

    (stmts, quote! { #args }, infos)
}

fn is_request_ref(ty: &Type) -> bool {
    if let Type::Reference(r) = ty {
        if let Type::Path(p) = r.elem.as_ref() {
            if let Some(seg) = p.path.segments.last() {
                return seg.ident == "Request";
            }
        }
    }
    false
}

// ──────────────────────────────────────────────────────────────────────
// #[controller("/prefix")] – annotate an impl block
// ──────────────────────────────────────────────────────────────────────

/// ```rust
/// #[controller("/users")]
/// impl UsersController {
///     #[get]
///     async fn list(&self) -> Response { … }
/// }
/// ```
///
/// Prepends the controller prefix to every inner `#[get]`/`#[post]`/… path.
#[proc_macro_attribute]
pub fn controller(attr: TokenStream, item: TokenStream) -> TokenStream {
    let prefix_lit = parse_macro_input!(attr as LitStr);
    let prefix = prefix_lit.value();

    let mut impl_block = parse_macro_input!(item as ItemImpl);

    for item in &mut impl_block.items {
        if let syn::ImplItem::Fn(method) = item {
            // Find HTTP-method attributes and prepend the controller prefix.
            for attr in &mut method.attrs {
                let last_seg = attr
                    .path()
                    .segments
                    .last()
                    .map(|s| s.ident.to_string());

                if matches!(
                    last_seg.as_deref(),
                    Some("get") | Some("post") | Some("put") | Some("delete") | Some("patch")
                ) {
                    // Rewrite the path literal inside the attribute.
                    let _meta = attr.meta.clone();
                    if let Ok(lit) = attr.parse_args::<LitStr>() {
                        let new_path = format!("{}{}", prefix, lit.value());
                        let new_lit = LitStr::new(&new_path, lit.span());
                        let path_tokens = attr.path().clone();
                        *attr = parse_quote!(#[#path_tokens(#new_lit)]);
                    }
                }
            }
        }
    }

    quote! { #impl_block }.into()
}

// ──────────────────────────────────────────────────────────────────────
// #[middleware(AuthMiddleware)] – wrap a handler in a middleware layer
// ──────────────────────────────────────────────────────────────────────

/// ```rust
/// #[middleware(Auth)]
/// #[get("/protected")]
/// async fn protected_endpoint(req: &Request) -> Response { … }
/// ```
///
/// Generates a middleware wrapper that calls `Auth::call(req, next).await`.
#[proc_macro_attribute]
pub fn middleware(attr: TokenStream, item: TokenStream) -> TokenStream {
    let mw_type = parse_macro_input!(attr as Type);
    let func    = parse_macro_input!(item as ItemFn);

    let fn_name   = &func.sig.ident;
    let wrapped   = syn::Ident::new(&format!("{}_mw_wrapped", fn_name), Span::call_site());
    let inner_fn  = func.clone();

    let expanded = quote! {
        #inner_fn

        // Replace the original name with the wrapped version for registration.
        #[allow(non_snake_case)]
        async fn #wrapped(req: ::comptime_web::request::Request) -> ::comptime_web::response::Response {
            <#mw_type as ::comptime_web::middleware::Middleware>::call(
                req,
                |r| Box::pin(async move { #fn_name(r).await }),
            ).await
        }
    };

    TokenStream::from(expanded)
}

// ──────────────────────────────────────────────────────────────────────
// #[derive(JsonSchema)] – compile-time schema generation
// ──────────────────────────────────────────────────────────────────────
//
// Generates a `JsonSchema` impl that:
//   1. Uses `std::mem::type_info::Type::of::<Self>()` (nightly) to obtain
//      compile-time size and kind information.
//   2. Iterates over annotated struct fields to build a JSON schema object.
//
// When `#[comptime]` lands, this derive can be replaced by a comptime fn
// that reads the same information from the reflection API without a macro.

/// Derive `JsonSchema` for a struct.
///
/// ```rust
/// #[derive(JsonSchema, serde::Deserialize)]
/// struct CreateOrder {
///     product_id: u64,
///     quantity:   u32,
/// }
/// ```
#[proc_macro_derive(JsonSchema, attributes(schema))]
pub fn derive_json_schema(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as syn::DeriveInput);

    let name     = &input.ident;
    let name_str = name.to_string();

    // Collect field metadata (name → type string) from the struct definition.
    // This mirrors what `std::mem::type_info::Struct::fields` exposes at
    // runtime/const-time, but the derive happens at compile time.
    let field_entries = match &input.data {
        syn::Data::Struct(s) => struct_schema_fields(&s.fields),
        syn::Data::Enum(_)   => enum_schema_variant(&name_str),
        syn::Data::Union(_)  => quote! {
            vec![("_union".to_string(), ::comptime_web::schema::SchemaNode::Any)]
        },
    };

    let expanded = quote! {
        impl ::comptime_web::schema::JsonSchema for #name {
            fn schema() -> ::comptime_web::schema::SchemaNode {
                // ── type_info integration ────────────────────────────────
                // Use the nightly `std::mem::type_info` API to obtain the
                // compile-time size of this type.  The `kind` field tells us
                // whether we are dealing with a struct, enum, etc., which
                // matches the `SchemaNode` we return below.
                //
                // Use the nightly `std::mem::type_info` API (always enabled;
                // nightly ≥ 1.94 is enforced by rust-toolchain.toml).
                // Evaluated as a const computation at compile time.
                const _TYPE_INFO: ::std::mem::type_info::Type =
                    ::std::mem::type_info::Type::of::<#name>();
                // (Future) When field-level type IDs can be resolved back
                // to `Type`, we will iterate `_TYPE_INFO.kind` to build the
                // schema without a derive macro at all.
                let _ = _TYPE_INFO;

                let fields: Vec<(String, ::comptime_web::schema::SchemaNode)> = #field_entries;
                ::comptime_web::schema::SchemaNode::Object {
                    name:   #name_str.to_string(),
                    fields,
                }
            }
        }
    };

    TokenStream::from(expanded)
}

/// Build the field list for a struct schema.
fn struct_schema_fields(fields: &syn::Fields) -> proc_macro2::TokenStream {
    let mut entries: Vec<proc_macro2::TokenStream> = Vec::new();

    let named = match fields {
        syn::Fields::Named(n)   => n.named.iter().collect::<Vec<_>>(),
        syn::Fields::Unnamed(u) => u.unnamed.iter().collect::<Vec<_>>(),
        syn::Fields::Unit       => vec![],
    };

    for (idx, f) in named.iter().enumerate() {
        let field_name = f
            .ident
            .as_ref()
            .map(|i| i.to_string())
            .unwrap_or_else(|| idx.to_string());
        let ty = &f.ty;
        entries.push(quote! {
            (#field_name.to_string(), <#ty as ::comptime_web::schema::JsonSchema>::schema())
        });
    }

    quote! {
        {
            let mut v: Vec<(String, ::comptime_web::schema::SchemaNode)> = Vec::new();
            #( v.push(#entries); )*
            v
        }
    }
}

fn enum_schema_variant(name: &str) -> proc_macro2::TokenStream {
    quote! {
        vec![(#name.to_string(), ::comptime_web::schema::SchemaNode::String)]
    }
}

// ──────────────────────────────────────────────────────────────────────
// Helpers
// ──────────────────────────────────────────────────────────────────────

fn method_to_variant(method: &str) -> proc_macro2::TokenStream {
    match method {
        "GET"    => quote! { Get },
        "POST"   => quote! { Post },
        "PUT"    => quote! { Put },
        "DELETE" => quote! { Delete },
        "PATCH"  => quote! { Patch },
        other    => {
            let v = syn::Ident::new(other, Span::call_site());
            quote! { #v }
        }
    }
}

/// Convert a URL path to a valid Rust identifier fragment.
fn sanitize_path(path: &str) -> String {
    path.chars()
        .map(|c| if c.is_alphanumeric() { c.to_ascii_uppercase() } else { '_' })
        .collect()
}
