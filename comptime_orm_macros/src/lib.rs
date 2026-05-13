// comptime_orm_macros – procedural macros for the compile-time ORM crate.
//
// Provides `#[derive(Entity)]` which generates at compile time:
//   • static <NAME>_ENTITY_META: EntityMeta  – SQL strings in .rodata
//   • Column index constants (User::COL_ID, …)
//   • impl DbEntity  – SQL string accessors
//   • find_by_pk / find_all / insert / update / delete methods
//   • inventory::submit! registration
//
// Migration note (Agents.md §16):
//   When #[comptime] stabilises, this proc-macro will be replaced by a
//   comptime fn that uses std::mem::type_info reflection.

use proc_macro::TokenStream;
use proc_macro2::Span;
use quote::quote;
use syn::{parse_macro_input, LitStr};

// ──────────────────────────────────────────────────────────────────────
// #[derive(Entity)] – compile-time ORM entity
// ──────────────────────────────────────────────────────────────────────

#[proc_macro_derive(Entity, attributes(table, pk))]
pub fn derive_entity(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as syn::DeriveInput);
    match derive_entity_inner(input) {
        Ok(ts) => ts,
        Err(e) => e.to_compile_error().into(),
    }
}

fn derive_entity_inner(input: syn::DeriveInput) -> syn::Result<TokenStream> {
    let name     = &input.ident;
    let name_str = name.to_string();

    // ── table name ───────────────────────────────────────────────────
    let table_name = extract_table_attr(&input.attrs)
        .unwrap_or_else(|| to_snake_case_plural(&name_str));

    // ── parse fields ────────────────────────────────────────────────
    let fields = match &input.data {
        syn::Data::Struct(s) => parse_entity_fields(&s.fields)?,
        _ => return Err(syn::Error::new_spanned(&input.ident, "#[derive(Entity)] only supports structs")),
    };

    if fields.is_empty() {
        return Err(syn::Error::new_spanned(&input.ident, "#[derive(Entity)] requires at least one field"));
    }

    let pk_fields:     Vec<_> = fields.iter().filter(|f| f.is_pk).collect();
    let non_pk_fields: Vec<_> = fields.iter().filter(|f| !f.is_pk).collect();
    let pk = pk_fields.first().copied().unwrap_or(&fields[0]);

    // ── build SQL strings at macro-expand time ──────────────────────
    let col_list = fields.iter().map(|f| f.col_name.as_str()).collect::<Vec<_>>().join(",");

    let find_sql = format!(
        "SELECT {} FROM {} WHERE {}=$1",
        col_list, table_name, pk.col_name
    );
    let find_all_sql = format!("SELECT {} FROM {}", col_list, table_name);

    let insert_placeholders = (1..=fields.len())
        .map(|i| format!("${i}"))
        .collect::<Vec<_>>()
        .join(",");
    let insert_sql = format!(
        "INSERT INTO {} ({}) VALUES ({})",
        table_name, col_list, insert_placeholders
    );
    let insert_returning_sql = format!(
        "INSERT INTO {} ({}) VALUES ({}) RETURNING {}",
        table_name, col_list, insert_placeholders, pk.col_name
    );

    let (update_sql, delete_sql) = if non_pk_fields.is_empty() {
        (
            format!("UPDATE {} SET {}={} WHERE {}=$1", table_name, pk.col_name, pk.col_name, pk.col_name),
            format!("DELETE FROM {} WHERE {}=$1", table_name, pk.col_name),
        )
    } else {
        let set_clauses = non_pk_fields
            .iter()
            .enumerate()
            .map(|(i, f)| format!("{}=${}", f.col_name, i + 1))
            .collect::<Vec<_>>()
            .join(",");
        (
            format!(
                "UPDATE {} SET {} WHERE {}=${}",
                table_name, set_clauses, pk.col_name, non_pk_fields.len() + 1
            ),
            format!("DELETE FROM {} WHERE {}=$1", table_name, pk.col_name),
        )
    };

    let col_defs = fields.iter().map(|f| {
        let null_str = if f.nullable { "" } else { " NOT NULL" };
        let pk_str   = if f.is_pk    { " PRIMARY KEY" } else { "" };
        format!("{} {}{}{}", f.col_name, f.sql_type_str, null_str, pk_str)
    }).collect::<Vec<_>>().join(", ");
    let migration_sql = format!("CREATE TABLE IF NOT EXISTS {} ({})", table_name, col_defs);
    let drop_sql      = format!("DROP TABLE IF EXISTS {}", table_name);

    // ── column index constants ───────────────────────────────────────
    let col_const_idents: Vec<syn::Ident> = fields.iter()
        .map(|f| syn::Ident::new(&format!("COL_{}", f.col_name.to_uppercase()), Span::call_site()))
        .collect();
    let col_indices: Vec<usize> = (0..fields.len()).collect();

    // ── ColumnMeta array entries ─────────────────────────────────────
    let col_meta_entries: Vec<proc_macro2::TokenStream> = fields.iter().map(|f| {
        let col_name = &f.col_name;
        let sql_type = &f.sql_type_variant;
        let nullable = f.nullable;
        let is_pk    = f.is_pk;
        quote! {
            ::comptime_orm::ColumnMeta {
                name:     #col_name,
                sql_type: #sql_type,
                nullable: #nullable,
                is_pk:    #is_pk,
            }
        }
    }).collect();

    // ── static ident ─────────────────────────────────────────────────
    let meta_ident = syn::Ident::new(
        &format!("{}_ENTITY_META", name_str.to_uppercase()),
        Span::call_site(),
    );

    // ── field idents for bind chains ────────────────────────────────
    let all_field_idents: Vec<syn::Ident> = fields.iter()
        .map(|f| syn::Ident::new(&f.name, Span::call_site()))
        .collect();
    let non_pk_field_idents: Vec<syn::Ident> = non_pk_fields.iter()
        .map(|f| syn::Ident::new(&f.name, Span::call_site()))
        .collect();
    let pk_ident = syn::Ident::new(&pk.name, Span::call_site());

    let expanded = quote! {
        // ── column index constants ───────────────────────────────────
        impl #name {
            #( pub const #col_const_idents: usize = #col_indices; )*
        }

        // ── compile-time entity metadata ─────────────────────────────
        #[allow(non_upper_case_globals)]
        static #meta_ident: ::comptime_orm::EntityMeta = ::comptime_orm::EntityMeta {
            table:         #table_name,
            columns:       &[ #( #col_meta_entries ),* ],
            find_sql:             #find_sql,
            find_all_sql:         #find_all_sql,
            insert_sql:           #insert_sql,
            insert_returning_sql: #insert_returning_sql,
            update_sql:           #update_sql,
            delete_sql:           #delete_sql,
            migration_sql:        #migration_sql,
            drop_sql:             #drop_sql,
        };

        // ── DbEntity impl ─────────────────────────────────────────────
        impl ::comptime_orm::DbEntity for #name {
            fn entity_meta() -> &'static ::comptime_orm::EntityMeta {
                &#meta_ident
            }
        }

        // ── CRUD methods (static SQL + typed sqlx bindings) ───────────
        impl #name {
            /// SELECT by primary key.
            pub async fn find_by_pk<'c, X>(
                pk: impl ::sqlx::Encode<'c, ::sqlx::Postgres>
                   + ::sqlx::Type<::sqlx::Postgres>
                   + Send
                   + 'c,
                executor: X,
            ) -> ::sqlx::Result<Option<Self>>
            where
                X: ::sqlx::Executor<'c, Database = ::sqlx::Postgres>,
                Self: for<'r> ::sqlx::FromRow<'r, ::sqlx::postgres::PgRow> + Send + Unpin,
            {
                ::sqlx::query_as::<::sqlx::Postgres, Self>(#find_sql)
                    .bind(pk)
                    .persistent(true)
                    .fetch_optional(executor)
                    .await
            }

            /// SELECT all rows.
            pub async fn find_all<'c, X>(executor: X) -> ::sqlx::Result<Vec<Self>>
            where
                X: ::sqlx::Executor<'c, Database = ::sqlx::Postgres>,
                Self: for<'r> ::sqlx::FromRow<'r, ::sqlx::postgres::PgRow> + Send + Unpin,
            {
                ::sqlx::query_as::<::sqlx::Postgres, Self>(#find_all_sql)
                    .persistent(true)
                    .fetch_all(executor)
                    .await
            }

            /// INSERT using pre-generated static SQL.
            pub async fn insert<'c, X>(&self, executor: X)
                -> ::sqlx::Result<::sqlx::postgres::PgQueryResult>
            where
                X: ::sqlx::Executor<'c, Database = ::sqlx::Postgres>,
            {
                ::sqlx::query(#insert_sql)
                    #( .bind(&self.#all_field_idents) )*
                    .persistent(true)
                    .execute(executor)
                    .await
            }

            /// INSERT and return the server-assigned primary key in one round-trip.
            pub async fn insert_returning<'c, X>(&self, executor: X)
                -> ::sqlx::Result<i64>
            where
                X: ::sqlx::Executor<'c, Database = ::sqlx::Postgres>,
            {
                ::sqlx::query_scalar(#insert_returning_sql)
                    #( .bind(&self.#all_field_idents) )*
                    .persistent(true)
                    .fetch_one(executor)
                    .await
            }

            /// UPDATE non-PK fields WHERE pk matches.
            pub async fn update<'c, X>(&self, executor: X)
                -> ::sqlx::Result<::sqlx::postgres::PgQueryResult>
            where
                X: ::sqlx::Executor<'c, Database = ::sqlx::Postgres>,
            {
                ::sqlx::query(#update_sql)
                    #( .bind(&self.#non_pk_field_idents) )*
                    .bind(&self.#pk_ident)
                    .persistent(true)
                    .execute(executor)
                    .await
            }

            /// DELETE WHERE pk matches.
            pub async fn delete<'c, X>(&self, executor: X)
                -> ::sqlx::Result<::sqlx::postgres::PgQueryResult>
            where
                X: ::sqlx::Executor<'c, Database = ::sqlx::Postgres>,
            {
                ::sqlx::query(#delete_sql)
                    .bind(&self.#pk_ident)
                    .persistent(true)
                    .execute(executor)
                    .await
            }
        }

        // ── inventory registration ────────────────────────────────────
        ::inventory::submit! {
            ::comptime_orm::EntityRegistration { meta: &#meta_ident }
        }
    };

    Ok(TokenStream::from(expanded))
}

// ──────────────────────────────────────────────────────────────────────
// Entity derive helpers
// ──────────────────────────────────────────────────────────────────────

struct EntityFieldInfo {
    name:             String,
    col_name:         String,
    #[allow(dead_code)]
    ty:               syn::Type,
    sql_type_str:     String,
    sql_type_variant: proc_macro2::TokenStream,
    is_pk:            bool,
    nullable:         bool,
}

fn parse_entity_fields(fields: &syn::Fields) -> syn::Result<Vec<EntityFieldInfo>> {
    let named = match fields {
        syn::Fields::Named(n) => &n.named,
        _ => return Err(syn::Error::new(Span::call_site(), "#[derive(Entity)] requires named fields")),
    };

    named.iter().map(|f| {
        let name     = f.ident.as_ref().unwrap().to_string();
        let col_name = name.clone();
        let is_pk    = f.attrs.iter().any(|a| a.path().is_ident("pk"))
                    || name == "id";
        let ty = f.ty.clone();
        let nullable = is_option_type(&ty);
        let inner_ty = unwrap_option(&ty).unwrap_or(&ty);
        let (sql_type_str, sql_type_variant) = rust_type_to_sql(inner_ty);

        Ok(EntityFieldInfo {
            name, col_name, ty, sql_type_str, sql_type_variant, is_pk, nullable,
        })
    }).collect()
}

fn extract_table_attr(attrs: &[syn::Attribute]) -> Option<String> {
    for attr in attrs {
        if attr.path().is_ident("table") {
            if let Ok(lit) = attr.parse_args::<LitStr>() {
                return Some(lit.value());
            }
        }
    }
    None
}

/// "UserProfile" → "user_profiles"
fn to_snake_case_plural(s: &str) -> String {
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if c.is_uppercase() && i > 0 { out.push('_'); }
        out.push(c.to_ascii_lowercase());
    }
    if !out.ends_with('s') { out.push('s'); }
    out
}

fn is_option_type(ty: &syn::Type) -> bool {
    if let syn::Type::Path(p) = ty {
        if let Some(seg) = p.path.segments.last() {
            return seg.ident == "Option";
        }
    }
    false
}

fn unwrap_option(ty: &syn::Type) -> Option<&syn::Type> {
    if let syn::Type::Path(p) = ty {
        if let Some(seg) = p.path.segments.last() {
            if seg.ident == "Option" {
                if let syn::PathArguments::AngleBracketed(ab) = &seg.arguments {
                    if let Some(syn::GenericArgument::Type(inner)) = ab.args.first() {
                        return Some(inner);
                    }
                }
            }
        }
    }
    None
}

fn unwrap_vec(ty: &syn::Type) -> Option<&syn::Type> {
    if let syn::Type::Path(p) = ty {
        if let Some(seg) = p.path.segments.last() {
            if seg.ident == "Vec" {
                if let syn::PathArguments::AngleBracketed(ab) = &seg.arguments {
                    if let Some(syn::GenericArgument::Type(inner)) = ab.args.first() {
                        return Some(inner);
                    }
                }
            }
        }
    }
    None
}

// ──────────────────────────────────────────────────────────────────────
// #[derive(RuntimeEntity)] – Jakarta Data-style repository support
// ──────────────────────────────────────────────────────────────────────

/// Derive macro that implements `comptime_orm::RuntimeEntity` for a struct
/// and generates typed relation accessor methods from field attributes.
///
/// # Relation attributes
///
/// Add these to fields whose type is another entity struct:
///
/// - `#[many_to_one]`  – FK lives on *this* table; loads one parent row.
///   The FK column is inferred as `{field_name}_id`.
///
/// - `#[one_to_many]`  – FK lives on the *child* table; loads Vec of children.
///   The FK column on the child table is inferred as `{this_type_snake}_id`.
///
/// - `#[many_to_many(join_table = "orders_tags")]` – loads via a bridge table.
///
/// # Example
/// ```rust
/// #[derive(Debug, Clone, sqlx::FromRow, RuntimeEntity)]
/// pub struct Order {
///     pub id:    i64,
///     pub total: f64,
///     #[many_to_one]
///     pub user:  User,          // generates Order::load_user(&self, pool)
/// }
/// ```
#[proc_macro_derive(RuntimeEntity, attributes(many_to_one, one_to_many, many_to_many))]
pub fn derive_runtime_entity(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as syn::DeriveInput);
    match derive_runtime_entity_inner(input) {
        Ok(ts) => ts,
        Err(e) => e.to_compile_error().into(),
    }
}

fn derive_runtime_entity_inner(input: syn::DeriveInput) -> syn::Result<TokenStream> {
    let name     = &input.ident;
    let name_str = name.to_string();

    // ── OnceLock static ───────────────────────────────────────────────
    let lock_ident = syn::Ident::new(
        &format!("{}_RUNTIME_ENTITY_INFO", name_str.to_uppercase()),
        Span::call_site(),
    );

    // ── Scan fields for relation attributes ───────────────────────────
    let named_fields = match &input.data {
        syn::Data::Struct(s) => match &s.fields {
            syn::Fields::Named(n) => &n.named,
            _ => return Err(syn::Error::new_spanned(name, "#[derive(RuntimeEntity)] requires named fields")),
        },
        _ => return Err(syn::Error::new_spanned(name, "#[derive(RuntimeEntity)] only supports structs")),
    };

    let this_table = to_snake_case_plural(&name_str);
    // default FK that child tables use to point back to us: "user_id", "order_id", …
    let this_fk = format!("{}_id", camel_to_snake_macro(&name_str));

    let mut relation_methods: Vec<proc_macro2::TokenStream> = vec![];

    for field in named_fields {
        let field_name = field.ident.as_ref().unwrap();
        let field_name_str = field_name.to_string();

        // Resolve the inner type (unwrap Option<T> → T, Vec<T> → T).
        let field_ty = &field.ty;
        let unwrapped = unwrap_option(field_ty).unwrap_or(field_ty);
        let inner_ty  = unwrap_vec(unwrapped).unwrap_or(unwrapped);

        // Derive the target table name from the field's type name.
        let target_type_str = quote!(#inner_ty).to_string().replace(" ", "");
        // Strip any path prefix (e.g. "crate::User" → "User").
        let target_short = target_type_str.rsplit("::").next().unwrap_or(&target_type_str);
        let target_table  = to_snake_case_plural(target_short);

        // ── #[many_to_one] ────────────────────────────────────────────
        if field.attrs.iter().any(|a| a.path().is_ident("many_to_one")) {
            // FK column on THIS table: "{field_name}_id"
            let fk_col     = format!("{}_id", field_name_str);
            let fk_field   = syn::Ident::new(&fk_col, Span::call_site());
            let loader_name = syn::Ident::new(
                &format!("load_{}", field_name_str),
                Span::call_site(),
            );
            let rel_const_name = syn::Ident::new(
                &format!("{}_RELATION", field_name_str.to_uppercase()),
                Span::call_site(),
            );
            let target_table_lit  = &target_table;
            let this_table_lit    = &this_table;
            let fk_col_lit        = &fk_col;

            relation_methods.push(quote! {
                /// Auto-generated `@ManyToOne` relation accessor.
                /// Loads the parent `#inner_ty` for this entity by FK `#fk_col_lit`.
                pub async fn #loader_name(
                    &self,
                    pool: &::sqlx::PgPool,
                ) -> ::sqlx::Result<Option<#inner_ty>>
                where
                    #inner_ty: for<'r> ::sqlx::FromRow<'r, ::sqlx::postgres::PgRow> + Send + Unpin,
                {
                    const REL: ::comptime_orm::ManyToOne = ::comptime_orm::ManyToOne {
                        owner_table:  #this_table_lit,
                        fk_column:    #fk_col_lit,
                        parent_table: #target_table_lit,
                        parent_pk:    "id",
                    };
                    let sql = REL.fetch_sql();
                    ::sqlx::query_as::<::sqlx::Postgres, #inner_ty>(&sql)
                        .bind(self.#fk_field)
                        .fetch_optional(pool)
                        .await
                }
            });

            // Also expose a typed const descriptor on the struct.
            relation_methods.push(quote! {
                pub const #rel_const_name: ::comptime_orm::ManyToOne = ::comptime_orm::ManyToOne {
                    owner_table:  #this_table_lit,
                    fk_column:    #fk_col_lit,
                    parent_table: #target_table_lit,
                    parent_pk:    "id",
                };
            });
        }

        // ── #[one_to_many] ────────────────────────────────────────────
        if field.attrs.iter().any(|a| a.path().is_ident("one_to_many")) {
            let loader_name = syn::Ident::new(
                &format!("load_{}", field_name_str),
                Span::call_site(),
            );
            let rel_const_name = syn::Ident::new(
                &format!("{}_RELATION", field_name_str.to_uppercase()),
                Span::call_site(),
            );
            let this_table_lit   = &this_table;
            let this_fk_lit      = &this_fk;
            let target_table_lit = &target_table;

            relation_methods.push(quote! {
                /// Auto-generated `@OneToMany` relation accessor.
                /// Loads all `#inner_ty` rows where `#this_fk_lit` matches `self.id`.
                pub async fn #loader_name(
                    &self,
                    pool: &::sqlx::PgPool,
                ) -> ::sqlx::Result<Vec<#inner_ty>>
                where
                    #inner_ty: for<'r> ::sqlx::FromRow<'r, ::sqlx::postgres::PgRow> + Send + Unpin,
                {
                    const REL: ::comptime_orm::OneToMany = ::comptime_orm::OneToMany {
                        owner_table: #this_table_lit,
                        fk_column:   #this_fk_lit,
                        child_table: #target_table_lit,
                        select_cols: &[],
                    };
                    REL.load::<#inner_ty>(self.id, pool).await
                }
            });

            relation_methods.push(quote! {
                pub const #rel_const_name: ::comptime_orm::OneToMany = ::comptime_orm::OneToMany {
                    owner_table: #this_table_lit,
                    fk_column:   #this_fk_lit,
                    child_table: #target_table_lit,
                    select_cols: &[],
                };
            });
        }

        // ── #[many_to_many(join_table = "…")] ─────────────────────────
        for attr in &field.attrs {
            if attr.path().is_ident("many_to_many") {
                // Parse join_table = "name" from the attribute args.
                let join_table: String = attr
                    .parse_args::<syn::MetaNameValue>()
                    .ok()
                    .and_then(|nv| if nv.path.is_ident("join_table") {
                        if let syn::Expr::Lit(syn::ExprLit { lit: syn::Lit::Str(s), .. }) = nv.value {
                            Some(s.value())
                        } else { None }
                    } else { None })
                    .unwrap_or_else(|| format!("{}_{}", this_table, target_table));

                let loader_name = syn::Ident::new(
                    &format!("load_{}", field_name_str),
                    Span::call_site(),
                );
                let rel_const_name = syn::Ident::new(
                    &format!("{}_RELATION", field_name_str.to_uppercase()),
                    Span::call_site(),
                );
                // owner FK = "{this_type}_id", target FK = "{target_type}_id"
                let owner_fk  = format!("{}_id", camel_to_snake_macro(&name_str));
                let target_fk = format!("{}_id", camel_to_snake_macro(target_short));
                let target_table_lit = &target_table;

                relation_methods.push(quote! {
                    /// Auto-generated `@ManyToMany` relation accessor.
                    pub async fn #loader_name(
                        &self,
                        pool: &::sqlx::PgPool,
                    ) -> ::sqlx::Result<Vec<#inner_ty>>
                    where
                        #inner_ty: for<'r> ::sqlx::FromRow<'r, ::sqlx::postgres::PgRow> + Send + Unpin,
                    {
                        const REL: ::comptime_orm::ManyToMany = ::comptime_orm::ManyToMany {
                            join_table:   #join_table,
                            owner_fk:     #owner_fk,
                            target_fk:    #target_fk,
                            target_table: #target_table_lit,
                        };
                        REL.load::<#inner_ty>(self.id, pool).await
                    }
                });

                relation_methods.push(quote! {
                    pub const #rel_const_name: ::comptime_orm::ManyToMany = ::comptime_orm::ManyToMany {
                        join_table:   #join_table,
                        owner_fk:     #owner_fk,
                        target_fk:    #target_fk,
                        target_table: #target_table_lit,
                    };
                });
            }
        }
    }

    let expanded = quote! {
        static #lock_ident: ::std::sync::OnceLock<::comptime_orm::EntityInfo> =
            ::std::sync::OnceLock::new();

        impl ::comptime_orm::RuntimeEntity for #name {
            fn entity_info() -> &'static ::comptime_orm::EntityInfo {
                #lock_ident.get_or_init(|| ::comptime_orm::EntityInfo::of::<#name>())
            }
        }

        impl #name {
            #( #relation_methods )*
        }
    };

    Ok(TokenStream::from(expanded))
}

/// "UserProfile" → "user_profile" (snake_case, no pluralisation).
fn camel_to_snake_macro(s: &str) -> String {
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if c.is_uppercase() && i > 0 { out.push('_'); }
        out.push(c.to_ascii_lowercase());
    }
    out
}

/// Map a Rust type to `(DDL_TYPE_STR, SqlType::Variant tokens)`.
fn rust_type_to_sql(ty: &syn::Type) -> (String, proc_macro2::TokenStream) {
    let ts = quote!(#ty).to_string().replace(" ", "");
    match ts.as_str() {
        "i64" | "u64"
            => ("BIGINT".into(),          quote! { ::comptime_orm::SqlType::BigInt }),
        "i32" | "u32"
            => ("INTEGER".into(),         quote! { ::comptime_orm::SqlType::Integer }),
        "i16" | "u16" | "i8" | "u8"
            => ("SMALLINT".into(),        quote! { ::comptime_orm::SqlType::SmallInt }),
        "f32"
            => ("REAL".into(),            quote! { ::comptime_orm::SqlType::Real }),
        "f64"
            => ("DOUBLE PRECISION".into(),quote! { ::comptime_orm::SqlType::DoublePrecision }),
        "bool"
            => ("BOOLEAN".into(),         quote! { ::comptime_orm::SqlType::Boolean }),
        "Vec<u8>"
            => ("BYTEA".into(),           quote! { ::comptime_orm::SqlType::Bytea }),
        "serde_json::Value"
            => ("JSON".into(),            quote! { ::comptime_orm::SqlType::Json }),
        _ => ("TEXT".into(),              quote! { ::comptime_orm::SqlType::Text }),
    }
}
