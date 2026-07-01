//! The `wasm32` browser entry: boot fast, then stream assets in the background
//! driven by the `axiom-assets` scheduler.
//!
//! This is the demo's whole job and its nondeterministic edge. It owns no
//! scheduling logic — it constructs [`AssetsApi`], renders a live table, and each
//! animation frame feeds the just-completed fetches in and dispatches the new
//! ones the scheduler asks for. The async, parallel `fetch`es are the host
//! boundary; their outcomes are queued and drained at a frame boundary so the
//! streaming session stays replayable.

use std::cell::RefCell;
use std::rc::Rc;

use axiom_assets::AssetsApi;
use axiom_kernel::AssetId;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::{spawn_local, JsFuture};
use web_sys::Document;

/// The streaming concurrency budget: at most this many loads are in flight at
/// once. The scheduler dispatches by priority + dependency order within it.
const MAX_IN_FLIGHT: usize = 4;

/// The manifest the demo fetches first (served from the app's `web/` dir,
/// produced by `tools/axiom-asset-pack`).
pub(crate) const MANIFEST_URL: &str = "manifest.bin";

/// Browser entry: render the boot status immediately (proving the page is
/// interactive before any asset loads), seed `window.__assetDemo`, then kick off
/// the async manifest fetch + streaming loop.
#[wasm_bindgen]
pub fn start() {
    console_error_panic_hook::set_once();

    let document = web_sys::window()
        .expect("a browser window")
        .document()
        .expect("a document");

    if let Some(boot) = document.get_element_by_id("boot") {
        boot.set_text_content(Some("engine ready — streaming assets…"));
    }
    update_window_state(0, 0, 0, 0, false, &[]);

    spawn_local(run());
}

/// Fetch the manifest, build the streaming catalog, render the asset table, and
/// start the per-frame streaming loop. A fetch/parse failure is surfaced in the
/// boot line rather than panicking the page.
async fn run() {
    let document = web_sys::window()
        .expect("a browser window")
        .document()
        .expect("a document");

    match fetch_bytes(MANIFEST_URL).await {
        Ok(bytes) => match AssetsApi::from_manifest_bytes(&bytes, MAX_IN_FLIGHT) {
            Ok(api) => {
                render_table(&document, &api);
                drive(api, document);
            }
            Err(_) => set_boot_error(&document, "manifest parse failed"),
        },
        Err(err) => set_boot_error(&document, &format!("manifest fetch failed: {err:?}")),
    }
}

/// Own the catalog for the page's lifetime and stream until everything is
/// ready/failed. Each frame: drain completed fetches → `advance` → dispatch the
/// returned loads as parallel `spawn_local` fetches → drain `take_ready` →
/// reflect state into the DOM + `window.__assetDemo`. When nothing remains, stop
/// scheduling and mark the run done.
fn drive(api: AssetsApi, document: Document) {
    // Shared host-boundary queues: the spawned fetch futures push their outcomes
    // here; the frame callback drains them and feeds them to `advance`.
    let completed_ok: Rc<RefCell<Vec<AssetId>>> = Rc::new(RefCell::new(Vec::new()));
    let completed_failed: Rc<RefCell<Vec<AssetId>>> = Rc::new(RefCell::new(Vec::new()));

    let callback: Rc<RefCell<Option<Closure<dyn FnMut()>>>> = Rc::new(RefCell::new(None));
    let scheduler = callback.clone();

    let mut api = api;
    // The order ids became ready — proof that dependents land after their deps.
    let mut ready_order: Vec<u64> = Vec::new();

    *callback.borrow_mut() = Some(Closure::<dyn FnMut()>::new(move || {
        let ok: Vec<AssetId> = completed_ok.borrow_mut().drain(..).collect();
        let failed: Vec<AssetId> = completed_failed.borrow_mut().drain(..).collect();

        let requests = api.advance(&ok, &failed);

        for (id, locator) in requests {
            let ok_push = completed_ok.clone();
            let fail_push = completed_failed.clone();
            spawn_local(async move {
                match fetch_bytes(&locator).await {
                    Ok(_bytes) => ok_push.borrow_mut().push(id),
                    Err(_) => fail_push.borrow_mut().push(id),
                }
            });
        }

        for id in api.take_ready() {
            ready_order.push(id.raw());
        }

        for id in api.asset_ids() {
            update_row(&document, id.raw(), api.state_code(id));
        }
        let total = api.total_count();
        let ready = api.ready_count();
        let failed_count = api.failed_count();
        let in_flight = api.in_flight_count();
        let done = ready + failed_count == total;
        update_header(&document, ready, total, failed_count, in_flight, done);
        update_window_state(ready, total, failed_count, in_flight, done, &ready_order);

        if !done {
            request_animation_frame(scheduler.borrow().as_ref().expect("raf closure set"));
        }
    }));

    request_animation_frame(callback.borrow().as_ref().expect("raf closure set"));
}

/// Fetch `url` and return its bytes, erroring on a network failure or non-2xx
/// status so the caller can route it into the completed-FAIL queue.
pub(crate) async fn fetch_bytes(url: &str) -> Result<Vec<u8>, JsValue> {
    let window = web_sys::window().ok_or_else(|| JsValue::from_str("no browser window"))?;
    let response_value = JsFuture::from(window.fetch_with_str(url)).await?;
    let response: web_sys::Response = response_value.dyn_into()?;
    if !response.ok() {
        return Err(JsValue::from_str(&format!(
            "HTTP {} for {url}",
            response.status()
        )));
    }
    let buffer = JsFuture::from(response.array_buffer()?).await?;
    Ok(js_sys::Uint8Array::new(&buffer).to_vec())
}

/// Render the static asset table once: one row per asset with its id, locator,
/// kind, dependencies, and a status cell the frame loop keeps live.
pub(crate) fn render_table(document: &Document, api: &AssetsApi) {
    let rows: String = api
        .asset_ids()
        .into_iter()
        .map(|id| {
            let raw = id.raw();
            let locator = api.locator(id).unwrap_or_default();
            let kind = api
                .kind(id)
                .map_or_else(|| "?".to_string(), |k| k.to_string());
            let deps = api
                .dependencies_of(id)
                .into_iter()
                .map(|d| d.raw().to_string())
                .collect::<Vec<_>>()
                .join(", ");
            format!(
                "<tr id=\"asset-{raw}\" data-state=\"0\">\
                   <td>{raw}</td><td>{locator}</td><td>{kind}</td>\
                   <td>{deps}</td>\
                   <td id=\"asset-{raw}-state\" data-state=\"0\">unrequested</td>\
                 </tr>"
            )
        })
        .collect();
    if let Some(tbody) = document.get_element_by_id("assets") {
        tbody.set_inner_html(&rows);
    }
}

/// Update one asset row's status cell (text + `data-state`) and the row's own
/// `data-state` attribute, for both human reading and test assertions.
pub(crate) fn update_row(document: &Document, raw: u64, code: u8) {
    if let Some(cell) = document.get_element_by_id(&format!("asset-{raw}-state")) {
        let _ = cell.set_attribute("data-state", &code.to_string());
        cell.set_text_content(Some(state_label(code)));
    }
    if let Some(row) = document.get_element_by_id(&format!("asset-{raw}")) {
        let _ = row.set_attribute("data-state", &code.to_string());
    }
}

/// The headline progress line.
pub(crate) fn update_header(
    document: &Document,
    ready: usize,
    total: usize,
    failed: usize,
    in_flight: usize,
    done: bool,
) {
    if let Some(status) = document.get_element_by_id("status") {
        let suffix = if done { " · done" } else { "" };
        status.set_text_content(Some(&format!(
            "Streaming: {ready}/{total} ready · {in_flight} in flight · {failed} failed{suffix}"
        )));
    }
}

/// Mirror progress onto `window.__assetDemo` for deterministic test assertions,
/// including `readyOrder` (the order ids became ready, which proves dependents
/// land after their dependencies).
fn update_window_state(
    ready: usize,
    total: usize,
    failed: usize,
    in_flight: usize,
    done: bool,
    ready_order: &[u64],
) {
    let window = web_sys::window().expect("a browser window");
    let object = js_sys::Object::new();
    let set = |key: &str, value: JsValue| {
        let _ = js_sys::Reflect::set(&object, &JsValue::from_str(key), &value);
    };
    set("ready", JsValue::from_f64(ready as f64));
    set("total", JsValue::from_f64(total as f64));
    set("failed", JsValue::from_f64(failed as f64));
    set("inFlight", JsValue::from_f64(in_flight as f64));
    set("done", JsValue::from_bool(done));
    let order = js_sys::Array::new();
    for raw in ready_order {
        order.push(&JsValue::from_f64(*raw as f64));
    }
    set("readyOrder", order.into());
    let _ = js_sys::Reflect::set(&window, &JsValue::from_str("__assetDemo"), &object);
}

/// Surface a fatal startup error in the boot line (red), matching the other
/// Axiom browser apps' "Startup failed:" convention the e2e smoke test keys on.
pub(crate) fn set_boot_error(document: &Document, message: &str) {
    if let Some(boot) = document.get_element_by_id("boot") {
        boot.set_text_content(Some(&format!("Startup failed: {message}")));
        let _ = boot.set_attribute("style", "color:#e07a7a");
    }
}

/// Map a load `state_code` to a human label.
pub(crate) fn state_label(code: u8) -> &'static str {
    match code {
        1 => "in-flight",
        2 => "ready",
        3 => "failed",
        _ => "unrequested",
    }
}

pub(crate) fn request_animation_frame(closure: &Closure<dyn FnMut()>) {
    let _ = web_sys::window()
        .expect("a browser window")
        .request_animation_frame(closure.as_ref().unchecked_ref());
}
