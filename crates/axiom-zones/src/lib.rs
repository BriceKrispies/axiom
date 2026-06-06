//! Zone marker attributes for Axiom engine code.
//!
//! These attributes label a function or module as belonging to a structural
//! *zone* so the dylint rulebook can enforce zone-specific rules (e.g. "no
//! wall-clock time in a `#[sim]` zone", "no allocation in a `#[hot_path]`").
//!
//! Custom attributes do not exist on stable Rust, so each attribute here is a
//! proc-macro that re-emits the annotated item with a **greppable zero-sized
//! marker** prepended:
//!
//! ```ignore
//! #[axiom_zones::sim]
//! fn step() { /* ... */ }
//! // expands to:
//! fn step() {
//!     const __engine_zone_sim: () = ();   // <- the lints detect this
//!     /* ... */
//! }
//! ```
//!
//! The raw `#[sim]` attribute is consumed at expansion (like `#[test]`), but the
//! injected `const __engine_zone_sim` survives into HIR, where a lint finds it by
//! name. `#[escape_hatch(reason = "…")]` additionally injects the reason string
//! so a lint can require it to be non-empty.

use proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::{Item, LitStr, parse_macro_input};

/// `#[sim]` — deterministic simulation zone.
#[proc_macro_attribute]
pub fn sim(_attr: TokenStream, item: TokenStream) -> TokenStream {
    inject_zone(item, "sim")
}

/// `#[hot_path]` — per-frame / per-tick hot path.
#[proc_macro_attribute]
pub fn hot_path(_attr: TokenStream, item: TokenStream) -> TokenStream {
    inject_zone(item, "hot_path")
}

/// `#[strict]` — branchless / primitive zone with the tightest rules.
#[proc_macro_attribute]
pub fn strict(_attr: TokenStream, item: TokenStream) -> TokenStream {
    inject_zone(item, "strict")
}

/// `#[supervisor]` — a supervisor loop where an unbounded `loop` is permitted.
#[proc_macro_attribute]
pub fn supervisor(_attr: TokenStream, item: TokenStream) -> TokenStream {
    inject_zone(item, "supervisor")
}

/// `#[escape_hatch(reason = "…")]` — a documented, deliberate exception. Injects
/// the reason so `engine_require_escape_hatch_reason` can demand it be non-empty.
#[proc_macro_attribute]
pub fn escape_hatch(attr: TokenStream, item: TokenStream) -> TokenStream {
    let reason = parse_escape_hatch_reason(attr);
    let item = parse_macro_input!(item as Item);
    let marker = format_ident!("__engine_escape_hatch_reason");
    let injected = quote! {
        #[allow(dead_code, non_upper_case_globals)]
        const #marker: &str = #reason;
    };
    inject_into_item(item, injected)
}

/// Parse `reason = "…"` from the attribute tokens, defaulting to `""` (which the
/// reason lint then rejects) when absent or malformed.
fn parse_escape_hatch_reason(attr: TokenStream) -> LitStr {
    let default = LitStr::new("", proc_macro2::Span::call_site());
    if attr.is_empty() {
        return default;
    }
    // Expect `reason = "<text>"`.
    match syn::parse::<syn::MetaNameValue>(attr) {
        Ok(nv) if nv.path.is_ident("reason") => match nv.value {
            syn::Expr::Lit(syn::ExprLit {
                lit: syn::Lit::Str(s),
                ..
            }) => s,
            _ => default,
        },
        _ => default,
    }
}

/// Inject `const __engine_zone_<zone>: () = ();` into `item`.
fn inject_zone(item: TokenStream, zone: &str) -> TokenStream {
    let item = parse_macro_input!(item as Item);
    let marker = format_ident!("__engine_zone_{}", zone);
    let injected = quote! {
        #[allow(dead_code, non_upper_case_globals)]
        const #marker: () = ();
    };
    inject_into_item(item, injected)
}

/// Prepend `injected` to a function body or a module's items, re-emitting the
/// item. Any other item kind is a compile error: zones live on fns and mods.
fn inject_into_item(item: Item, injected: proc_macro2::TokenStream) -> TokenStream {
    match item {
        Item::Fn(mut f) => {
            let stmt: syn::Stmt = syn::parse_quote! { #injected };
            f.block.stmts.insert(0, stmt);
            quote! { #f }.into()
        }
        Item::Mod(mut m) => match &mut m.content {
            Some((_brace, items)) => {
                let nested: Item = syn::parse_quote! { #injected };
                items.insert(0, nested);
                quote! { #m }.into()
            }
            None => syn::Error::new_spanned(
                &m,
                "a zone marker on a module requires an inline `mod name { ... }` body",
            )
            .to_compile_error()
            .into(),
        },
        other => syn::Error::new_spanned(
            &other,
            "zone markers apply only to functions and inline modules",
        )
        .to_compile_error()
        .into(),
    }
}
