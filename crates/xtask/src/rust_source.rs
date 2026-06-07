//! Lightweight, dependency-free scanning of Rust source *as text*.
//!
//! These helpers never compile or fully parse Rust; they perform the strongest
//! practical structural approximation the Layer Law needs:
//! - find cross-layer path references (`prefix::...`) and judge whether they
//!   reach a public root item or a private module,
//! - detect whether a symbol is publicly exported,
//! - follow a `pub use` re-export to its module path.
//!
//! Heuristics are documented at each function so future agents can tune them.

use std::path::{Path, PathBuf};

/// A single `prefix::...` reference to another layer found in source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CrossRef {
    /// The import prefix that matched (e.g. `axiom_kernel`).
    pub prefix: String,
    /// 1-based line where the reference occurs.
    pub line: usize,
    /// `true` if the path reaches through a private module
    /// (`prefix::some_module::Item`) rather than a public root item.
    pub private: bool,
}

fn is_ident_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

fn line_of(text: &str, byte_index: usize) -> usize {
    // 1-based line number of `byte_index`.
    text[..byte_index].bytes().filter(|&b| b == b'\n').count() + 1
}

/// Find every cross-layer reference in `text` for any of the given import
/// `prefixes`.
///
/// A reference is `prefix::<tail>`. Classification of `private`:
/// - `prefix::Item`, `prefix::Item::assoc`, `prefix::{ ... }`, `prefix::*`,
///   `prefix::free_fn` → public root access (allowed).
/// - `prefix::module::...` where the first tail segment is a lowercase
///   (module-like) identifier followed by `::` → private module access.
///
/// This case-based rule lets `axiom_kernel::KernelApi::new()` (associated call
/// on a public type) pass while flagging `axiom_kernel::facade::KernelApi`
/// (reaching through the private `facade` module).
pub fn find_cross_refs(text: &str, prefixes: &[String]) -> Vec<CrossRef> {
    let bytes = text.as_bytes();
    let mut refs = Vec::new();

    for prefix in prefixes {
        let needle = format!("{prefix}::");
        let needle_bytes = needle.as_bytes();
        let mut search_from = 0;

        while let Some(rel) = text[search_from..].find(&needle) {
            let start = search_from + rel;
            search_from = start + needle_bytes.len();

            // Left boundary: the prefix must not be the tail of a longer
            // identifier or path segment (e.g. `axiom_kernel` must not match
            // inside `my_axiom_kernel` or `crate::axiom_kernel`).
            if start > 0 {
                let prev = bytes[start - 1];
                if is_ident_char(prev) || prev == b':' {
                    continue;
                }
            }

            let tail = &bytes[search_from..];
            let private = classify_tail(tail);
            refs.push(CrossRef {
                prefix: prefix.clone(),
                line: line_of(text, start),
                private,
            });
        }
    }

    refs.sort_by(|a, b| a.line.cmp(&b.line).then(a.prefix.cmp(&b.prefix)));
    refs
}

/// Given the bytes immediately following `prefix::`, decide if the access is
/// private (reaches through a lowercase module and continues deeper).
fn classify_tail(tail: &[u8]) -> bool {
    match tail.first() {
        // Group import or glob at the root: public.
        Some(b'{') | Some(b'*') => false,
        Some(&first) if first.is_ascii_alphabetic() || first == b'_' => {
            // Read the first identifier segment.
            let mut i = 0;
            while i < tail.len() && is_ident_char(tail[i]) {
                i += 1;
            }
            let module_like = first.is_ascii_lowercase() || first == b'_';
            let continues = tail.get(i) == Some(&b':') && tail.get(i + 1) == Some(&b':');
            // Private only when a module-like segment is followed by more path.
            module_like && continues
        }
        // Anything else (`;`, whitespace, malformed): not a meaningful ref.
        _ => false,
    }
}

/// Whether `name` is publicly exported by this source text.
///
/// Matches `pub <kw> name ...` for item keywords and `pub use ... name;` /
/// `pub use ... as name;` re-exports. Deliberately excludes `pub(crate)` and
/// other restricted visibilities, which are not visible to other layers.
/// Returns the 1-based line of the first match.
pub fn find_public_export(text: &str, name: &str) -> Option<usize> {
    const ITEM_KEYWORDS: &[&str] = &[
        "fn", "struct", "enum", "trait", "type", "const", "static", "union", "mod",
    ];

    for (idx, raw_line) in text.lines().enumerate() {
        let line = raw_line.trim_start();
        let Some(rest) = line.strip_prefix("pub ") else {
            continue;
        };
        let rest = rest.trim_start();

        // `pub use ... name;` or `pub use ... as name;`
        if let Some(use_rest) = rest.strip_prefix("use ") {
            if reexports_name(use_rest, name) {
                return Some(idx + 1);
            }
            continue;
        }

        // `pub <keyword> name`
        for kw in ITEM_KEYWORDS {
            if let Some(after_kw) = rest.strip_prefix(kw) {
                // Require a separator after the keyword so `structure` != `struct`.
                let after_kw = match after_kw.chars().next() {
                    Some(c) if c.is_whitespace() => after_kw.trim_start(),
                    _ => continue,
                };
                if first_ident(after_kw) == Some(name) {
                    return Some(idx + 1);
                }
            }
        }
    }
    None
}

/// Whether a `use` body (text after `pub use `) re-exports `name`.
fn reexports_name(use_body: &str, name: &str) -> bool {
    // Strip a trailing `;` and surrounding whitespace.
    let body = use_body.trim().trim_end_matches(';').trim();

    // `... as alias`
    if let Some((_, alias)) = body.rsplit_once(" as ") {
        return alias.trim() == name;
    }
    // `a::b::name`
    let last = body.rsplit("::").next().unwrap_or(body).trim();
    last == name
}

/// The leading identifier of a string (stops at the first non-identifier char).
fn first_ident(s: &str) -> Option<&str> {
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() && is_ident_char(bytes[i]) {
        i += 1;
    }
    if i == 0 {
        None
    } else {
        Some(&s[..i])
    }
}

/// Whether `symbol` appears anywhere in `text` as a standalone identifier.
pub fn references_symbol(text: &str, symbol: &str) -> bool {
    let bytes = text.as_bytes();
    let sym = symbol.as_bytes();
    if sym.is_empty() {
        return false;
    }
    let mut from = 0;
    while let Some(rel) = text[from..].find(symbol) {
        let start = from + rel;
        let end = start + sym.len();
        from = start + 1;
        let left_ok = start == 0 || !is_ident_char(bytes[start - 1]);
        let right_ok = end >= bytes.len() || !is_ident_char(bytes[end]);
        if left_ok && right_ok {
            return true;
        }
    }
    false
}

/// If `name` is re-exported via `pub use a::b::name;`, return the module path
/// segments before the final symbol (`["a", "b"]`). Used to follow a facade
/// re-export to the module that actually references lower-layer symbols.
pub fn reexport_module_path(text: &str, name: &str) -> Option<Vec<String>> {
    for raw_line in text.lines() {
        let line = raw_line.trim_start();
        let Some(rest) = line.strip_prefix("pub ") else {
            continue;
        };
        let Some(use_body) = rest.trim_start().strip_prefix("use ") else {
            continue;
        };
        let body = use_body.trim().trim_end_matches(';').trim();

        let (path, exported) = match body.rsplit_once(" as ") {
            Some((path, alias)) => (path.trim(), alias.trim()),
            None => {
                let exported = body.rsplit("::").next().unwrap_or(body).trim();
                (body, exported)
            }
        };
        if exported != name {
            continue;
        }
        let mut segments: Vec<String> = path.split("::").map(|s| s.trim().to_string()).collect();
        // Drop the final symbol segment, keeping only the module path.
        segments.pop();
        // Ignore leading `crate`/`self`/`super` qualifiers; they are not modules
        // on disk we can resolve from `src/`.
        while matches!(
            segments.first().map(String::as_str),
            Some("crate") | Some("self") | Some("super")
        ) {
            segments.remove(0);
        }
        if segments.is_empty() {
            return None;
        }
        return Some(segments);
    }
    None
}

/// Remove `//` line comments, preserving line structure (one output line per
/// input line) so reported line numbers stay accurate.
///
/// This prevents a stray comment that merely *mentions* a symbol or path from
/// masking a real violation (a false negative) or inventing a false one. Block
/// comments and string literals are left intact — a documented limitation.
pub fn strip_line_comments(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for line in text.lines() {
        match line.find("//") {
            Some(idx) => out.push_str(&line[..idx]),
            None => out.push_str(line),
        }
        out.push('\n');
    }
    out
}

/// Blank out the body of every `#[cfg(test)]` / `#[test]` item, preserving line
/// structure (newlines kept; other bytes replaced with spaces) so reported line
/// numbers stay accurate.
///
/// This makes the cross-layer import scan see only **non-test** code, so a layer
/// that imports another layer purely from its tests is not treated as depending
/// on it — matching the `engine_genuine_dependency` dylint, which also ignores
/// test code. `depends_on` is a layer's non-test architecture in both tools.
///
/// Run this on already comment-stripped text. It matches the literal attributes
/// `#[cfg(test)]` and `#[test]`; exotic forms (`#[cfg(all(test, …))]`) and braces
/// inside string literals are not handled — the same documented limitation as
/// the rest of this text scanner.
pub fn strip_test_code(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out: Vec<u8> = bytes.to_vec();
    for marker in ["#[cfg(test)]", "#[test]"] {
        let mut from = 0;
        while let Some(rel) = text[from..].find(marker) {
            let attr_start = from + rel;
            from = attr_start + marker.len();
            let end = item_end(bytes, attr_start);
            for b in out.iter_mut().take(end).skip(attr_start) {
                if *b != b'\n' {
                    *b = b' ';
                }
            }
        }
    }
    String::from_utf8(out).unwrap_or_else(|_| text.to_string())
}

/// The end (exclusive) of the item an attribute at `attr_start` applies to:
/// either the matching `}` of its first brace block, or the first `;` if that
/// comes first (e.g. `#[cfg(test)] use foo::Bar;`).
fn item_end(bytes: &[u8], attr_start: usize) -> usize {
    let mut i = attr_start;
    while i < bytes.len() && bytes[i] != b'{' && bytes[i] != b';' {
        i += 1;
    }
    if i >= bytes.len() {
        return bytes.len();
    }
    if bytes[i] == b';' {
        return i + 1;
    }
    let mut depth = 0u32;
    while i < bytes.len() {
        match bytes[i] {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return i + 1;
                }
            }
            _ => {}
        }
        i += 1;
    }
    bytes.len()
}

/// The names of file-modules declared `#[cfg(test)] mod NAME;` (the `;` form, a
/// separate file — not an inline `mod NAME { … }`, which `strip_test_code`
/// already handles). Those files are compiled only in test builds, so the
/// import scan must skip them entirely — matching the dylint's HIR-level
/// `is_in_test`, which sees the gate that lives in the *declaring* file.
pub fn find_cfg_test_modules(text: &str) -> Vec<String> {
    let marker = "#[cfg(test)]";
    let mut names = Vec::new();
    let mut from = 0;
    while let Some(rel) = text[from..].find(marker) {
        let after = from + rel + marker.len();
        from = after;
        let rest = strip_leading_pub(text[after..].trim_start());
        let Some(rest) = rest.strip_prefix("mod ") else {
            continue;
        };
        let rest = rest.trim_start();
        let Some(name) = first_ident(rest) else {
            continue;
        };
        // Only the `mod NAME;` declaration form (a separate file).
        if rest[name.len()..].trim_start().starts_with(';') {
            names.push(name.to_string());
        }
    }
    names
}

/// Strip a leading `pub` / `pub(crate)` / `pub(in …)` visibility, returning the
/// rest trimmed.
fn strip_leading_pub(s: &str) -> &str {
    let Some(after_pub) = s.strip_prefix("pub") else {
        return s;
    };
    let rest = after_pub.trim_start();
    if let Some(open) = rest.strip_prefix('(') {
        if let Some(close) = open.find(')') {
            return open[close + 1..].trim_start();
        }
    }
    rest
}

/// Recursively collect every `.rs` file under `dir`, sorted for determinism.
pub fn collect_rs_files(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    collect_into(dir, &mut files);
    files.sort();
    files
}

fn collect_into(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        if path.is_dir() {
            collect_into(&path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
            out.push(path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn prefixes() -> Vec<String> {
        vec!["axiom_kernel".to_string(), "top".to_string()]
    }

    #[test]
    fn public_root_use_is_not_private() {
        let refs = find_cross_refs("use axiom_kernel::KernelApi;", &prefixes());
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].prefix, "axiom_kernel");
        assert!(!refs[0].private);
    }

    #[test]
    fn associated_call_on_public_type_is_not_private() {
        let refs = find_cross_refs("let x = axiom_kernel::KernelApi::new();", &prefixes());
        assert_eq!(refs.len(), 1);
        assert!(!refs[0].private, "Type::assoc access must stay public");
    }

    #[test]
    fn reaching_through_lowercase_module_is_private() {
        let refs = find_cross_refs("use axiom_kernel::facade::KernelApi;", &prefixes());
        assert_eq!(refs.len(), 1);
        assert!(refs[0].private);
    }

    #[test]
    fn group_and_glob_imports_are_public() {
        assert!(!find_cross_refs("use axiom_kernel::{A, B};", &prefixes())[0].private);
        assert!(!find_cross_refs("use axiom_kernel::*;", &prefixes())[0].private);
    }

    #[test]
    fn longer_identifier_does_not_match_prefix() {
        // `my_top::X` must not match prefix `top`.
        let refs = find_cross_refs("use my_top::X;", &prefixes());
        assert!(refs.iter().all(|r| r.prefix != "top"));
    }

    #[test]
    fn detects_public_items_and_reexports() {
        assert!(find_public_export("pub struct Runtime { x: u8 }", "Runtime").is_some());
        assert!(find_public_export("pub fn step() {}", "step").is_some());
        assert!(find_public_export("pub use facade::KernelApi;", "KernelApi").is_some());
        assert!(find_public_export("pub use inner::Thing as Runtime;", "Runtime").is_some());
        // Restricted visibility is not a public export.
        assert!(find_public_export("pub(crate) struct Hidden;", "Hidden").is_none());
        // Substring keyword guard.
        assert!(find_public_export("pub structure_field: u8", "structure_field").is_none());
    }

    #[test]
    fn reexport_module_path_strips_symbol_and_qualifiers() {
        assert_eq!(
            reexport_module_path("pub use facade::KernelApi;", "KernelApi"),
            Some(vec!["facade".to_string()])
        );
        assert_eq!(
            reexport_module_path("pub use crate::inner::deep::Thing as T;", "T"),
            Some(vec!["inner".to_string(), "deep".to_string()])
        );
    }

    #[test]
    fn references_symbol_respects_word_boundaries() {
        assert!(references_symbol(
            "let c: KernelClock = todo();",
            "KernelClock"
        ));
        assert!(!references_symbol("KernelClockTower", "KernelClock"));
    }

    #[test]
    fn test_code_is_stripped_but_lines_preserved() {
        let src = "use axiom_kernel::A;\n\
                   #[cfg(test)]\n\
                   mod tests {\n\
                       use axiom_host::B;\n\
                   }\n\
                   pub fn keep() {}\n";
        let stripped = strip_test_code(src);
        // Non-test import survives; the test-module import is gone.
        assert!(stripped.contains("axiom_kernel::A"));
        assert!(!stripped.contains("axiom_host::B"));
        // The non-test item after the test module is preserved.
        assert!(stripped.contains("pub fn keep"));
        // Line count is unchanged so reported line numbers stay accurate.
        assert_eq!(stripped.lines().count(), src.lines().count());
    }

    #[test]
    fn finds_cfg_test_file_modules_only() {
        let src = "#[cfg(test)]\nmod fixtures;\n\
                   #[cfg(test)] pub mod helpers;\n\
                   #[cfg(test)]\nmod inline { fn x() {} }\n\
                   mod real;\n";
        let mods = find_cfg_test_modules(src);
        assert!(mods.contains(&"fixtures".to_string()));
        assert!(mods.contains(&"helpers".to_string()));
        // The inline (`{ … }`) form is handled by strip_test_code, not here.
        assert!(!mods.contains(&"inline".to_string()));
        // A non-test module is never returned.
        assert!(!mods.contains(&"real".to_string()));
    }

    #[test]
    fn test_attribute_on_a_use_statement_is_stripped() {
        let src = "#[cfg(test)] use axiom_math::M;\nuse axiom_kernel::K;\n";
        let stripped = strip_test_code(src);
        assert!(!stripped.contains("axiom_math::M"));
        assert!(stripped.contains("axiom_kernel::K"));
    }

    #[test]
    fn line_comments_are_stripped_but_lines_preserved() {
        let src = "use kernel::Clock; // mentions other::Thing\nlet x = 1;\n";
        let stripped = strip_line_comments(src);
        // The comment text is gone...
        assert!(!stripped.contains("other::Thing"));
        // ...but the real code and its line position remain.
        assert!(stripped.contains("use kernel::Clock;"));
        assert_eq!(stripped.lines().count(), 2);
    }
}
