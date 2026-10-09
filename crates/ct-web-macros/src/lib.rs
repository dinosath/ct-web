use std::collections::BTreeMap;

use proc_macro::TokenStream;
use proc_macro2::{Span, TokenStream as TokenStream2};
use quote::{format_ident, quote};
use syn::spanned::Spanned;
use syn::{
    Attribute, Error, ImplItem, Item, ItemFn, ItemImpl, ItemMod, LitStr, Result, Token,
    parse_macro_input,
};

#[proc_macro_attribute]
pub fn routes(_args: TokenStream, input: TokenStream) -> TokenStream {
    let module = parse_macro_input!(input as ItemMod);
    expand_routes(module).unwrap_or_else(Error::into_compile_error).into()
}

#[proc_macro_attribute]
pub fn route(args: TokenStream, input: TokenStream) -> TokenStream {
    let args = parse_macro_input!(args as RouteArgs);
    let function = parse_macro_input!(input as ItemFn);
    expand_route(args, function).unwrap_or_else(Error::into_compile_error).into()
}

#[proc_macro_attribute]
pub fn main(args: TokenStream, input: TokenStream) -> TokenStream {
    let mut function = parse_macro_input!(input as ItemFn);

    if !args.is_empty() {
        return Error::new(
            Span::call_site(),
            "ct_web::main takes no arguments; annotate handlers with `#[ct_web::route(...)]`",
        )
        .into_compile_error()
        .into();
    }

    if function.sig.ident != "main" {
        return Error::new(function.sig.ident.span(), "ct_web::main can only be used on `main`")
            .into_compile_error()
            .into();
    }
    if !function.sig.inputs.is_empty() {
        return Error::new(function.sig.inputs.span(), "main must not take parameters")
            .into_compile_error()
            .into();
    }
    if !function.block.stmts.is_empty() {
        return Error::new(
            function.block.span(),
            "the ct-web main body is generated; configure startup with environment variables",
        )
        .into_compile_error()
        .into();
    }

    function.sig.asyncness = None;
    function.sig.output = syn::parse_quote!(
        -> ::std::result::Result<(), Box<dyn ::std::error::Error>>
    );
    function.block = Box::new(syn::parse_quote!({
        let runtime = ::ct_web::tokio::runtime::Runtime::new()?;
        runtime.block_on(::ct_web::serve(
            &crate::routes::ROUTER,
            crate::routes::dispatch,
        ))?;
        Ok(())
    }));

    quote! {
        mod routes;
        #function
    }
    .into()
}

struct RouteArgs {
    method: LitStr,
    path: LitStr,
}

impl syn::parse::Parse for RouteArgs {
    fn parse(input: syn::parse::ParseStream<'_>) -> Result<Self> {
        let mut method = None;
        let mut path = None;
        while !input.is_empty() {
            let name: syn::Ident = input.parse()?;
            input.parse::<Token![=]>()?;
            let value: LitStr = input.parse()?;
            match name.to_string().as_str() {
                "method" if method.is_none() => method = Some(value),
                "path" if path.is_none() => path = Some(value),
                "method" | "path" => return Err(Error::new(name.span(), "duplicate route option")),
                _ => return Err(Error::new(name.span(), "expected `method` or `path`")),
            }
            if input.is_empty() {
                break;
            }
            input.parse::<Token![,]>()?;
        }
        Ok(Self {
            method: method.ok_or_else(|| input.error("missing route `method`"))?,
            path: path.ok_or_else(|| input.error("missing route `path`"))?,
        })
    }
}

fn expand_route(args: RouteArgs, function: ItemFn) -> Result<TokenStream2> {
    validate_handler(&function.sig, function.sig.ident.span())?;
    Method::parse(&args.method.value(), args.method.span())?;
    validate_route_path(&args.path.value(), args.path.span())?;
    Ok(quote!(#function))
}

fn validate_route_path(path: &str, span: Span) -> Result<()> {
    if !path.starts_with('/') {
        return Err(Error::new(span, "route paths must start with `/`"));
    }
    if path != "/" && path.ends_with('/') {
        return Err(Error::new(span, "route paths cannot end with `/`"));
    }
    if path == "/" {
        return Ok(());
    }
    for segment in path[1..].split('/') {
        if segment.is_empty() {
            return Err(Error::new(span, "route paths cannot contain empty segments"));
        }
        if let Some(name) = segment.strip_prefix('{').and_then(|value| value.strip_suffix('}')) {
            if name.is_empty() {
                return Err(Error::new(span, "route parameter names cannot be empty"));
            }
        } else if let Some(name) = segment.strip_prefix(':') {
            if name.is_empty() {
                return Err(Error::new(span, "route parameter names cannot be empty"));
            }
        } else if segment.contains(['{', '}', ':']) {
            return Err(Error::new(span, "invalid route path segment"));
        }
    }
    Ok(())
}

fn expand_routes(mut module: ItemMod) -> Result<TokenStream2> {
    let Some((_, items)) = module.content.as_mut() else {
        return Err(Error::new(
            module.span(),
            "ct_web::routes requires an inline module",
        ));
    };

    let routes = collect_routes(items)?;
    let mut root = TrieNode::root();
    for (index, route) in routes.iter().enumerate() {
        root.insert(route, index as u32)?;
    }

    let router = router_node_tokens(&root);
    let dispatch_arms = routes.iter().enumerate().map(|(index, route)| {
        let route_id = index as u32;
        let call = &route.call;
        let call = if route.is_async {
            quote!((#call).await)
        } else {
            quote!(#call)
        };
        quote! {
            ::ct_web::router::RouteResolution::Matched(::ct_web::router::RouteId(#route_id)) => {
                let response = #call;
                ::core::result::Result::Ok(
                    ::ct_web::axum::response::IntoResponse::into_response(response)
                )
            }
        }
    });

    let generated = quote! {
        #[doc(hidden)]
        pub static ROUTER: ::ct_web::router::RouterNode = #router;

        #[doc(hidden)]
        pub async fn dispatch(
            resolution: ::ct_web::router::RouteResolution,
            _request: ::ct_web::axum::http::Request<::ct_web::axum::body::Body>,
        ) -> ::core::result::Result<
            ::ct_web::axum::http::Response<::ct_web::axum::body::Body>,
            ::core::convert::Infallible,
        > {
            match resolution {
                #(#dispatch_arms,)*
                ::ct_web::router::RouteResolution::MethodNotAllowed => {
                    ::core::result::Result::Ok(
                        ::ct_web::axum::response::IntoResponse::into_response((
                            ::ct_web::axum::http::StatusCode::METHOD_NOT_ALLOWED,
                            "method not allowed",
                        ))
                    )
                }
                ::ct_web::router::RouteResolution::Matched(_)
                | ::ct_web::router::RouteResolution::NotFound => {
                    ::core::result::Result::Ok(
                        ::ct_web::axum::response::IntoResponse::into_response((
                            ::ct_web::axum::http::StatusCode::NOT_FOUND,
                            "not found",
                        ))
                    )
                }
            }
        }

        pub fn application() -> impl ::core::future::Future<
            Output = ::core::result::Result<(), ::std::io::Error>,
        > {
            ::ct_web::serve(&ROUTER, dispatch)
        }
    };
    items.extend(syn::parse2::<syn::File>(generated)?.items);

    Ok(quote!(#module))
}

fn collect_routes(items: &[Item]) -> Result<Vec<Route>> {
    let mut routes = Vec::new();
    for item in items {
        match item {
            Item::Fn(function) => {
                if let Some(route) = route_attribute(&function.attrs)? {
                    validate_handler(&function.sig, route.span)?;
                    let method = Method::parse(&route.method, route.span)?;
                    let name = &function.sig.ident;
                    routes.push(Route {
                        method,
                        path: route.path,
                        call: quote!(#name()),
                        is_async: function.sig.asyncness.is_some(),
                        span: route.span,
                    });
                }
            }
            Item::Impl(implementation) => {
                collect_impl_routes(implementation, &mut routes)?;
            }
            _ => {}
        }
    }
    Ok(routes)
}

fn collect_impl_routes(implementation: &ItemImpl, routes: &mut Vec<Route>) -> Result<()> {
    for item in &implementation.items {
        let ImplItem::Fn(function) = item else {
            continue;
        };
        let Some(route) = route_attribute(&function.attrs)? else {
            continue;
        };
        if implementation.trait_.is_some() {
            return Err(Error::new(
                route.span,
                "routes on trait implementation methods are not supported yet",
            ));
        }
        validate_handler(&function.sig, route.span)?;
        let method = Method::parse(&route.method, route.span)?;
        let self_ty = &implementation.self_ty;
        let name = &function.sig.ident;
        routes.push(Route {
            method,
            path: route.path,
            call: quote!(<#self_ty>::#name()),
            is_async: function.sig.asyncness.is_some(),
            span: route.span,
        });
    }
    Ok(())
}

fn validate_handler(signature: &syn::Signature, span: Span) -> Result<()> {
    if !signature.inputs.is_empty() {
        return Err(Error::new(
            span,
            "route handlers must take no parameters; request extraction is not generated yet",
        ));
    }
    if !signature.generics.params.is_empty() {
        return Err(Error::new(span, "generic route handlers are not supported"));
    }
    if signature.unsafety.is_some() {
        return Err(Error::new(span, "unsafe route handlers are not supported"));
    }
    Ok(())
}

struct RouteAttribute {
    method: String,
    path: String,
    span: Span,
}

fn route_attribute(attributes: &[Attribute]) -> Result<Option<RouteAttribute>> {
    let mut found = None;
    for attribute in attributes {
        let segments = &attribute.path().segments;
        if segments.len() != 2
            || !matches!(segments[0].ident.to_string().as_str(), "app" | "ct_web")
            || segments[1].ident != "route"
        {
            continue;
        }

        if found.is_some() {
            return Err(Error::new(attribute.span(), "duplicate route attribute"));
        }
        let args = attribute.parse_args::<RouteArgs>()?;
        found = Some(RouteAttribute {
            method: args.method.value(),
            path: args.path.value(),
            span: attribute.span(),
        });
    }
    Ok(found)
}

#[derive(Clone, Copy, Eq, Ord, PartialEq, PartialOrd)]
enum Method {
    Connect,
    Delete,
    Get,
    Head,
    Options,
    Patch,
    Post,
    Put,
    Trace,
}

impl Method {
    fn parse(method: &str, span: Span) -> Result<Self> {
        match method {
            "CONNECT" => Ok(Self::Connect),
            "DELETE" => Ok(Self::Delete),
            "GET" => Ok(Self::Get),
            "HEAD" => Ok(Self::Head),
            "OPTIONS" => Ok(Self::Options),
            "PATCH" => Ok(Self::Patch),
            "POST" => Ok(Self::Post),
            "PUT" => Ok(Self::Put),
            "TRACE" => Ok(Self::Trace),
            _ => Err(Error::new(span, "unsupported HTTP method")),
        }
    }

    fn ident(self) -> syn::Ident {
        let name = match self {
            Self::Connect => "Connect",
            Self::Delete => "Delete",
            Self::Get => "Get",
            Self::Head => "Head",
            Self::Options => "Options",
            Self::Patch => "Patch",
            Self::Post => "Post",
            Self::Put => "Put",
            Self::Trace => "Trace",
        };
        format_ident!("{name}")
    }
}

struct Route {
    method: Method,
    path: String,
    call: TokenStream2,
    is_async: bool,
    span: Span,
}

enum Segment {
    Root,
    Static(String),
    Param(String),
}

struct TrieNode {
    segment: Segment,
    static_children: BTreeMap<String, TrieNode>,
    param_child: Option<Box<TrieNode>>,
    handlers: Vec<(Method, u32)>,
}

impl TrieNode {
    fn root() -> Self {
        Self::new(Segment::Root)
    }

    fn new(segment: Segment) -> Self {
        Self {
            segment,
            static_children: BTreeMap::new(),
            param_child: None,
            handlers: Vec::new(),
        }
    }

    fn insert(&mut self, route: &Route, route_id: u32) -> Result<()> {
        if !route.path.starts_with('/') {
            return Err(Error::new(route.span, "route paths must start with `/`"));
        }

        let parts = if route.path == "/" {
            Vec::new()
        } else {
            route.path[1..].split('/').collect()
        };
        let mut node = self;
        for part in parts {
            if part.is_empty() {
                return Err(Error::new(route.span, "route paths cannot contain empty segments"));
            }
            if let Some(name) = part.strip_prefix('{').and_then(|part| part.strip_suffix('}')) {
                if name.is_empty() || name.chars().any(|ch| matches!(ch, '{' | '}' | '/')) {
                    return Err(Error::new(route.span, "invalid route parameter segment"));
                }
                node = node
                    .param_child
                    .get_or_insert_with(|| Box::new(Self::new(Segment::Param(name.to_owned()))));
            } else if let Some(name) = part.strip_prefix(':') {
                if name.is_empty() || name.chars().any(|ch| matches!(ch, '{' | '}' | '/' | ':')) {
                    return Err(Error::new(route.span, "invalid route parameter segment"));
                }
                node = node
                    .param_child
                    .get_or_insert_with(|| Box::new(Self::new(Segment::Param(name.to_owned()))));
            } else {
                if part.chars().any(|ch| matches!(ch, '{' | '}')) {
                    return Err(Error::new(route.span, "invalid route parameter segment"));
                }
                node = node
                    .static_children
                    .entry(part.to_owned())
                    .or_insert_with(|| Self::new(Segment::Static(part.to_owned())));
            }
        }

        if node.handlers.iter().any(|(method, _)| *method == route.method) {
            return Err(Error::new(
                route.span,
                "duplicate route for this HTTP method and path",
            ));
        }
        node.handlers.push((route.method, route_id));
        node.handlers.sort_by_key(|(method, _)| *method);
        Ok(())
    }
}

fn router_node_tokens(node: &TrieNode) -> TokenStream2 {
    let segment = match &node.segment {
        Segment::Root => quote!(::ct_web::router::PathSegment::Root),
        Segment::Static(name) => quote!(::ct_web::router::PathSegment::Static(#name)),
        Segment::Param(name) => quote!(::ct_web::router::PathSegment::Param(#name)),
    };
    let static_children = node.static_children.values().map(router_node_tokens);
    let param_child = node
        .param_child
        .as_deref()
        .map(|child| {
            let child = router_node_tokens(child);
            quote!(Some(&#child))
        })
        .unwrap_or_else(|| quote!(None));
    let handlers = node.handlers.iter().map(|(method, route)| {
        let method = method.ident();
        quote! {
            ::ct_web::router::MethodHandler {
                method: ::ct_web::router::HttpMethod::#method,
                route: ::ct_web::router::RouteId(#route),
            }
        }
    });

    quote! {
        ::ct_web::router::RouterNode {
            segment: #segment,
            static_children: &[#(#static_children),*],
            param_child: #param_child,
            method_handlers: &[#(#handlers),*],
        }
    }
}
