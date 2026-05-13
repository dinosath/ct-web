// build.rs – compile-time route + entity scanning → phf perfect-hash map generation.
//
// Runs before rustc processes any source file.  Scans every `*.rs` file under
// `src/` for:
//
//   • `#[get("…")]` / `#[post("…")]` / … → PHF route maps (existing)
//   • `#[derive(Entity)]` + optional `#[table("…")]` → PHF entity-table set (new,
//     discussion.md §6 migration runner support)
//
// Generated statics (included by `src/router/mod.rs`):
//
//   PHF_GET_ROUTES:         phf::Map<&str, usize>   – path → handler index
//   … (one per HTTP method)
//   PHF_ALL_STATIC_PATHS:   phf::Set<&str>           – 405 detection
//   PHF_ENTITY_TABLE_NAMES: phf::Set<&str>           – all derived entity table names
//
// Migration note (Agents.md §16):
//   When `#[comptime]` lands this file is replaced by a comptime function.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::Path;

const HTTP_METHODS: &[(&str, &str)] = &[
    ("get",     "GET"),
    ("post",    "POST"),
    ("put",     "PUT"),
    ("delete",  "DELETE"),
    ("patch",   "PATCH"),
    ("head",    "HEAD"),
    ("options", "OPTIONS"),
];

fn main() {
    // Rerun this script whenever any Rust source file changes.
    println!("cargo:rerun-if-changed=src");

    // ── Route scanning ────────────────────────────────────────────────
    let mut by_method: BTreeMap<&str, BTreeSet<String>> = HTTP_METHODS
        .iter()
        .map(|&(_, upper)| (upper, BTreeSet::new()))
        .collect();
    let mut entity_tables: BTreeSet<String> = BTreeSet::new();

    scan_dir(Path::new("src"), &mut by_method, &mut entity_tables);

    let all_paths: BTreeSet<String> = by_method.values().flatten().cloned().collect();

    // ── Code generation ───────────────────────────────────────────────
    let out_dir  = std::env::var("OUT_DIR").expect("OUT_DIR not set");
    let out_path = Path::new(&out_dir).join("phf_routes.rs");
    let mut code = String::new();

    // Per-method PHF maps (path → usize index into handler array).
    for &(_, upper) in HTTP_METHODS {
        let paths: Vec<&String> = by_method[upper].iter().collect();
        let map_name = format!("PHF_{}_ROUTES", upper);
        let len_name = format!("PHF_{}_ROUTES_LEN", upper);

        writeln!(code, "#[allow(dead_code)]").unwrap();
        writeln!(code, "pub(crate) const {}: usize = {};", len_name, paths.len()).unwrap();

        let idx_strings: Vec<String> = (0..paths.len()).map(|i| i.to_string()).collect();
        let mut map = phf_codegen::Map::new();
        for (idx, path) in paths.iter().enumerate() {
            map.entry(path.as_str(), &idx_strings[idx]);
        }
        writeln!(
            code,
            "#[allow(dead_code)]\npub(crate) static {}: ::phf::Map<&'static str, usize> = {};\n",
            map_name,
            map.build(),
        )
        .unwrap();
    }

    // Single set of every static path (any method) – O(1) 405 check.
    let mut path_set = phf_codegen::Set::new();
    for p in &all_paths {
        path_set.entry(p.as_str());
    }
    writeln!(
        code,
        "#[allow(dead_code)]\npub(crate) static PHF_ALL_STATIC_PATHS: ::phf::Set<&'static str> = {};\n",
        path_set.build(),
    )
    .unwrap();

    // PHF set of entity table names – used by the migration runner to validate
    // that every entity has a matching CREATE TABLE migration at startup.
    let mut entity_set = phf_codegen::Set::new();
    for t in &entity_tables {
        entity_set.entry(t.as_str());
    }
    writeln!(
        code,
        "#[allow(dead_code)]\npub(crate) static PHF_ENTITY_TABLE_NAMES: ::phf::Set<&'static str> = {};\n",
        entity_set.build(),
    )
    .unwrap();

    std::fs::write(&out_path, code).expect("Could not write phf_routes.rs");
}

// ──────────────────────────────────────────────────────────────────────
// Source scanning
// ──────────────────────────────────────────────────────────────────────

fn scan_dir(
    dir: &Path,
    routes: &mut BTreeMap<&str, BTreeSet<String>>,
    entity_tables: &mut BTreeSet<String>,
) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            scan_dir(&path, routes, entity_tables);
        } else if path.extension().map_or(false, |e| e == "rs") {
            let Ok(content) = std::fs::read_to_string(&path) else { continue };
            extract_static_routes(&content, routes);
            extract_entity_tables(&content, entity_tables);
        }
    }
}

/// Walk source text for `#[method("path")]` annotations.
/// Only static paths (no `:param`) are added to the PHF maps.
fn extract_static_routes(content: &str, routes: &mut BTreeMap<&str, BTreeSet<String>>) {
    for line in content.lines() {
        let trimmed = line.trim();
        for &(lower, upper) in HTTP_METHODS {
            let open = format!("#[{}(\"", lower);
            if let Some(rest) = trimmed.strip_prefix(open.as_str()) {
                if let Some(end) = rest.find('"') {
                    let raw_path = &rest[..end];
                    if !raw_path.split('/').any(|seg| seg.starts_with(':')) {
                        routes
                            .get_mut(upper)
                            .unwrap()
                            .insert(normalize_path(raw_path));
                    }
                }
            }
        }
    }
}

/// Walk source text looking for structs annotated with `#[derive(Entity)]`.
///
/// For each such struct, extract the table name from `#[table("…")]` (if
/// present) or derive it as pluralised snake_case from the struct name.
/// The result is added to `entity_tables` for inclusion in
/// `PHF_ENTITY_TABLE_NAMES`.
fn extract_entity_tables(content: &str, entity_tables: &mut BTreeSet<String>) {
    let lines: Vec<&str> = content.lines().collect();
    let mut in_entity_derive = false;
    let mut pending_table: Option<String> = None;

    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim();

        // Detect #[derive(… Entity …)]
        if trimmed.starts_with("#[derive(") && trimmed.contains("Entity") {
            in_entity_derive = true;
            pending_table = None;
        }

        // Detect #[table("…")] on a line near the struct
        if in_entity_derive {
            if let Some(rest) = trimmed.strip_prefix("#[table(\"") {
                if let Some(end) = rest.find('"') {
                    pending_table = Some(rest[..end].to_string());
                }
            }
        }

        // Detect `struct StructName` after we've seen Entity derive
        if in_entity_derive && (trimmed.starts_with("pub struct ") || trimmed.starts_with("struct ")) {
            let name = trimmed
                .trim_start_matches("pub ")
                .trim_start_matches("struct ")
                .split_whitespace()
                .next()
                .unwrap_or("")
                .trim_end_matches('{')
                .trim_end_matches(';');

            if !name.is_empty() {
                let table = pending_table
                    .take()
                    .unwrap_or_else(|| to_snake_case_plural(name));
                entity_tables.insert(table);
            }
            in_entity_derive = false;

        // Reset if we hit something that can't be a struct decl in this context
        } else if in_entity_derive && i > 0
            && !trimmed.starts_with('#')
            && !trimmed.starts_with("pub")
            && !trimmed.starts_with("struct")
            && !trimmed.is_empty()
        {
            in_entity_derive = false;
            pending_table = None;
        }
    }
}

// ──────────────────────────────────────────────────────────────────────
// Comptime.toml parser
// ──────────────────────────────────────────────────────────────────────

/// Read `schema-management.strategy` from `Comptime.toml`.
/// Returns `"none"` if the file is absent or the key is missing.
/// Mirror of `to_snake_case_plural` in the proc-macro crate.
fn to_snake_case_plural(s: &str) -> String {
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if c.is_uppercase() && i > 0 { out.push('_'); }
        out.push(c.to_ascii_lowercase());
    }
    if !out.ends_with('s') { out.push('s'); }
    out
}

/// Normalise to the canonical lookup form used at dispatch time:
/// leading `/`, no trailing `/`, e.g. `"users"` → `"/users"`.
fn normalize_path(path: &str) -> String {
    let s = path.trim_end_matches('/');
    if s.is_empty() {
        "/".to_string()
    } else if s.starts_with('/') {
        s.to_string()
    } else {
        format!("/{}", s)
    }
}
