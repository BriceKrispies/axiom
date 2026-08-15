//! The address bar, which is this app's **only** persistent page state.
//!
//! The engine uploads geometry once at bind (`NOTES.md` §8) and configures its
//! surface once (§7), so two things the page can do — move the detail dial,
//! rotate the device — are answered by a reload rather than by a re-author. A
//! reload is only non-destructive if the URL carries everything the session had
//! chosen, which is why every control that changes a *choice* writes it here.
//!
//! The URL carries two kinds of parameter, and this module is what keeps them
//! from evicting each other:
//!
//! * the **dials**, owned by [`SceneConfig`] and written as a block by
//!   [`remember`];
//! * the **non-dial choices** — the debug `view` and the [`Stage`] — each
//!   written on its own by [`remember_param`].
//!
//! Writing one used to mean rebuilding the query string from that one source,
//! which silently dropped the other: moving any slider on a `?view=normals` page
//! sent the reload back to the shaded view. Both writers now merge against
//! what is already in the bar, so a page can hold a stage, a view and fifteen
//! dials at once and survive a reload with all of them.
//!
//! Compiled only for `wasm32`: it is the browser edge, and nothing but plumbing.
//! What the values *mean* lives in `config.rs` and `stage.rs`, natively tested.

use wasm_bindgen::prelude::*;

use crate::config::{Dial, SceneConfig};

/// The page URL's query string, leading `?` and all, or the empty string.
pub fn query() -> String {
    web_sys::window()
        .and_then(|window| window.location().search().ok())
        .unwrap_or_default()
}

/// One parameter's value from the page URL, or the empty string.
pub fn param(key: &str) -> String {
    pairs(&query())
        .into_iter()
        .find(|(name, _)| name == key)
        .map(|(_, value)| value)
        .unwrap_or_default()
}

/// Put the whole configuration in the address bar without navigating, keeping
/// every non-dial parameter already there.
pub fn remember(config: &SceneConfig) {
    replace(merge(kept(None), config.to_query()));
}

/// Set one non-dial parameter, keeping the dials already there. An empty value
/// removes the parameter rather than writing `key=`.
pub fn remember_param(key: &str, value: &str) {
    let dials = SceneConfig::from_query(&query()).to_query();
    let mut extras = kept(Some(key));
    (!value.is_empty()).then(|| extras.push(format!("{key}={value}")));
    replace(merge(extras, dials));
}

/// Re-run the app against the configuration now in the address bar.
pub fn reload() {
    let _ = web_sys::window().map(|window| window.location().reload());
}

/// Every `key=value` pair in a query string, in the order it carries them.
fn pairs(query: &str) -> Vec<(String, String)> {
    query
        .trim_start_matches('?')
        .split('&')
        .filter_map(|pair| pair.split_once('='))
        .map(|(key, value)| (key.to_string(), value.to_string()))
        .collect()
}

/// The non-dial parameters currently in the bar, minus `replaced`. Dial
/// parameters are dropped because the caller is about to write the whole dial
/// block from the configuration itself.
fn kept(replaced: Option<&str>) -> Vec<String> {
    pairs(&query())
        .into_iter()
        .filter(|(key, _)| !key.is_empty() & Dial::from_key(key).is_none())
        .filter(|(key, _)| replaced.map(|name| name != key).unwrap_or(true))
        .map(|(key, value)| format!("{key}={value}"))
        .collect()
}

/// The non-dial parameters and the dial block as one query string.
fn merge(mut extras: Vec<String>, dials: String) -> String {
    (!dials.is_empty()).then(|| extras.push(dials));
    extras.join("&")
}

/// Write `query` into the address bar without navigating.
fn replace(query: String) {
    let target = format!("?{query}");
    let url = [".", target.as_str()][usize::from(!query.is_empty())];
    let _ = web_sys::window()
        .and_then(|window| window.history().ok())
        .map(|history| history.replace_state_with_url(&JsValue::NULL, "", Some(url)));
}
