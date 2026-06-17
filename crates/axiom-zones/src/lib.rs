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
use quote::{format_ident, quote, ToTokens};
use syn::{Item, LitStr};

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
    let marker = format_ident!("__engine_escape_hatch_reason");
    let injected = quote! {
        #[allow(dead_code, non_upper_case_globals)]
        const #marker: &str = #reason;
    };
    inject(item, injected)
}

/// Parse `reason = "…"` from the attribute tokens, defaulting to `""` (which the
/// reason lint then rejects) when absent or malformed.
fn parse_escape_hatch_reason(attr: TokenStream) -> LitStr {
    // Expect `reason = "<text>"`. An empty/malformed attr fails to parse (or
    // fails a filter) and falls through to the `""` default, which the reason
    // lint then rejects — behavior-identical to the prior explicit branches.
    // Extract the `LitStr` without destructuring the `syn::Expr`/`Lit` enums:
    // re-parse `nv.value`'s tokens directly as a `LitStr`. A non-string-literal
    // value (e.g. `reason = 1` or `reason = foo`) yields tokens that fail to
    // parse as `LitStr`, so it falls through to the `""` default — exactly as the
    // old `Expr::Lit(.. Lit::Str ..) => Some / _ => None` branches did. `Result`/
    // `Option` combinators only, no enum `match`.
    syn::parse::<syn::MetaNameValue>(attr)
        .ok()
        .filter(|nv| nv.path.is_ident("reason"))
        .and_then(|nv| syn::parse2::<LitStr>(nv.value.into_token_stream()).ok())
        .unwrap_or_else(|| LitStr::new("", proc_macro2::Span::call_site()))
}

/// Inject `const __engine_zone_<zone>: () = ();` into `item`.
fn inject_zone(item: TokenStream, zone: &str) -> TokenStream {
    let marker = format_ident!("__engine_zone_{}", zone);
    let injected = quote! {
        #[allow(dead_code, non_upper_case_globals)]
        const #marker: () = ();
    };
    inject(item, injected)
}

/// Prepend `injected` to a function body, an `impl`-method body, or a module's
/// items, re-emitting the item. Any other item kind is a compile error: zones
/// live on functions, methods, and inline modules.
fn inject(item: TokenStream, injected: proc_macro2::TokenStream) -> TokenStream {
    // Try each concrete item kind directly, chained with `Result` combinators —
    // no `Item` enum and no variant `match`. A free-standing `fn`, then an inline
    // `mod`, then an associated function (a method in an `impl` block, which
    // parses as `ImplItemFn`, not `Item` — so zones can mark the engine's
    // `step`/`advance` etc.). The final fallback emits the unsupported-item error.
    let injected_for_mod = injected.clone();
    let injected_for_method = injected.clone();
    syn::parse::<syn::ItemFn>(item.clone())
        .map(|mut f| {
            f.block.stmts.insert(0, syn::parse_quote! { #injected });
            quote! { #f }
        })
        .or_else(|_| {
            syn::parse::<syn::ItemMod>(item.clone()).map(|mut m| {
                // `m.content` is `Option<(Brace, Vec<Item>)>`: an inline body
                // injects the marker; a bodyless `mod name;` produces the
                // module-needs-a-body error. `Option` combinators, no `match`.
                let with_body = m.content.as_mut().map(|(_brace, items)| {
                    items.insert(0, syn::parse_quote! { #injected_for_mod });
                });
                with_body
                    .map(|()| quote! { #m })
                    .unwrap_or_else(|| {
                        syn::Error::new_spanned(
                            &m,
                            "a zone marker on a module requires an inline `mod name { ... }` body",
                        )
                        .to_compile_error()
                    })
            })
        })
        .or_else(|_| {
            syn::parse::<syn::ImplItemFn>(item.clone()).map(|mut method| {
                method
                    .block
                    .stmts
                    .insert(0, syn::parse_quote! { #injected_for_method });
                quote! { #method }
            })
        })
        // Every concrete kind failed. Reproduce the original two-tier error
        // without matching the `Item` enum: if the tokens *do* parse as some
        // `Item` (necessarily neither `fn` nor `mod`, since those were tried
        // above), emit the "apply only to" error spanned on that item — exactly
        // the old `Ok(other) =>` arm. Otherwise the tokens aren't a valid item
        // at all, so surface that parse error verbatim — the old `ImplItemFn`
        // `Err(err)` arm. `Result::or_else`/`map_err`, no `match`.
        .unwrap_or_else(|impl_err| {
            syn::parse::<Item>(item)
                .map(|other| {
                    syn::Error::new_spanned(
                        &other,
                        "zone markers apply only to functions, methods, and inline modules",
                    )
                    .to_compile_error()
                })
                .unwrap_or_else(|_| impl_err.to_compile_error())
        })
        .into()
}
