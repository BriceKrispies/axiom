//! The `wasm32` browser entry for the **Web Worker pool** streaming variant.
//!
//! Same deterministic brain as [`crate::web`] ([`AssetsApi`]), different host
//! edge: instead of `fetch`ing on the main thread, this spins up a pool of N
//! background [`web_sys::Worker`]s (`?workers=N`, default 3). The main thread owns
//! only scheduling — each animation frame it drains the completions the workers
//! reported, calls [`AssetsApi::advance`], pushes the returned loads onto a shared
//! pending queue, and hands queued jobs to whichever workers are idle. Each worker
//! `fetch`es one asset and runs a CPU-bound placeholder "decode" (the stand-in for
//! future wasm decode) entirely off the main thread, then reports the outcome
//! back.
//!
//! This is the "skeleton loads first, assets stream on workers" architecture: the
//! page is interactive immediately, and the heavy per-asset work runs in parallel
//! across the pool without ever blocking the frame loop.
//!
//! Determinism is unchanged. Workers finish in arbitrary order, but their outcomes
//! are only ever *applied* at a frame boundary, in the order the queue drains them
//! — so a given completion sequence produces the same schedule the single-threaded
//! variant would. The pool changes *where work runs*, never *what the brain does*.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

use axiom_assets::AssetsApi;
use axiom_kernel::AssetId;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::spawn_local;
use web_sys::{Document, MessageEvent, Worker};

use crate::web::{
    fetch_bytes, render_table, request_animation_frame, set_boot_error, update_header, update_row,
    MANIFEST_URL,
};

/// Default pool size when `?workers=N` is absent, and the inclusive clamp bounds.
const DEFAULT_WORKERS: usize = 3;
const MIN_WORKERS: usize = 1;
const MAX_WORKERS: usize = 8;

/// The scheduler's own in-flight budget. Set generously so the *pool* (not the
/// scheduler) is the real concurrency limiter — `advance` releases every eligible
/// asset and they queue up for the workers, which is the behavior this demo shows.
const SCHEDULER_BUDGET: usize = 64;

/// The classic worker script (served from the app's `web/` dir, resolved relative
/// to the page). Each instance runs one `fetch`+decode job at a time.
const WORKER_URL: &str = "worker.js";

/// All mutable streaming state, shared between the per-worker `onmessage` handlers
/// (which record completions and free the worker) and the animation-frame loop
/// (which schedules, dispatches, and renders). A single `RefCell` is sound because
/// the two never run concurrently in the browser's single-threaded event loop.
struct PoolState {
    api: AssetsApi,
    /// Completions reported by workers since the last frame (drained into `advance`).
    completed_ok: Vec<AssetId>,
    completed_failed: Vec<AssetId>,
    /// Loads the scheduler released that have no free worker yet — the shared queue.
    pending: VecDeque<(u64, String)>,
    /// Indices of workers ready for a job.
    idle: Vec<usize>,
    /// Workers currently running a job, and the peak ever reached (parallelism proof).
    busy: usize,
    peak_busy: usize,
    /// The order ids became ready — proves dependents land after their deps.
    ready_order: Vec<u64>,
    /// Per-worker current asset id (`None` = idle), for the live worker panel.
    worker_job: Vec<Option<u64>>,
}

/// Browser entry for the worker-pool variant: boot-fast, then stream via the pool.
#[wasm_bindgen]
pub fn start_pool() {
    console_error_panic_hook::set_once();

    let document = web_sys::window()
        .expect("a browser window")
        .document()
        .expect("a document");

    if let Some(boot) = document.get_element_by_id("boot") {
        boot.set_text_content(Some("engine ready — streaming assets on a worker pool…"));
    }

    spawn_local(run_pool());
}

/// Fetch the manifest on the main thread, build the catalog + worker pool, and
/// start the scheduling loop. A manifest failure is surfaced in the boot line.
async fn run_pool() {
    let document = web_sys::window()
        .expect("a browser window")
        .document()
        .expect("a document");

    match fetch_bytes(MANIFEST_URL).await {
        Ok(bytes) => match AssetsApi::from_manifest_bytes(&bytes, SCHEDULER_BUDGET) {
            Ok(api) => {
                render_table(&document, &api);
                drive_pool(api, document);
            }
            Err(_) => set_boot_error(&document, "manifest parse failed"),
        },
        Err(err) => set_boot_error(&document, &format!("manifest fetch failed: {err:?}")),
    }
}

/// Create the worker pool and run the per-frame scheduling loop until every asset
/// is ready or failed.
fn drive_pool(api: AssetsApi, document: Document) {
    let count = worker_count();

    let state = Rc::new(RefCell::new(PoolState {
        api,
        completed_ok: Vec::new(),
        completed_failed: Vec::new(),
        pending: VecDeque::new(),
        idle: (0..count).collect(),
        busy: 0,
        peak_busy: 0,
        ready_order: Vec::new(),
        worker_job: vec![None; count],
    }));

    // Each worker's onmessage records its job's outcome and returns it to the idle set.
    let workers: Vec<Worker> = (0..count)
        .map(|i| {
            let worker = Worker::new(WORKER_URL).expect("spawn worker.js");
            let state_for_msg = state.clone();
            let on_message = Closure::<dyn FnMut(MessageEvent)>::new(move |e: MessageEvent| {
                let data = e.data();
                let id = reflect_f64(&data, "id") as u64;
                let ok = reflect_bool(&data, "ok");
                let mut s = state_for_msg.borrow_mut();
                match ok {
                    true => s.completed_ok.push(AssetId::from_raw(id)),
                    false => s.completed_failed.push(AssetId::from_raw(id)),
                }
                s.busy = s.busy.saturating_sub(1);
                s.idle.push(i);
                s.worker_job[i] = None;
            });
            worker.set_onmessage(Some(on_message.as_ref().unchecked_ref()));
            // The handler lives for the page's lifetime, like the rAF closure below.
            on_message.forget();
            worker
        })
        .collect();
    let workers = Rc::new(workers);

    let callback: Rc<RefCell<Option<Closure<dyn FnMut()>>>> = Rc::new(RefCell::new(None));
    let scheduler = callback.clone();

    *callback.borrow_mut() = Some(Closure::<dyn FnMut()>::new(move || {
        let mut s = state.borrow_mut();

        let ok: Vec<AssetId> = s.completed_ok.drain(..).collect();
        let failed: Vec<AssetId> = s.completed_failed.drain(..).collect();

        let requests = s.api.advance(&ok, &failed);
        for (id, locator) in requests {
            s.pending.push_back((id.raw(), locator));
        }

        while !s.idle.is_empty() && !s.pending.is_empty() {
            let wi = s.idle.pop().expect("idle worker");
            let (id, locator) = s.pending.pop_front().expect("pending job");
            let _ = workers[wi].post_message(&job_message(id, &locator));
            s.busy += 1;
            s.peak_busy = s.peak_busy.max(s.busy);
            s.worker_job[wi] = Some(id);
        }

        for id in s.api.take_ready() {
            s.ready_order.push(id.raw());
        }

        for id in s.api.asset_ids() {
            update_row(&document, id.raw(), s.api.state_code(id));
        }
        let total = s.api.total_count();
        let ready = s.api.ready_count();
        let failed_count = s.api.failed_count();
        let in_flight = s.api.in_flight_count();
        let done = ready + failed_count == total;
        update_header(&document, ready, total, failed_count, in_flight, done);
        update_worker_panel(
            &document,
            &s.worker_job,
            s.busy,
            s.pending.len(),
            s.peak_busy,
        );
        update_window_state_pool(&s, count, ready, total, failed_count, in_flight, done);

        if !done {
            request_animation_frame(scheduler.borrow().as_ref().expect("raf closure set"));
        }
    }));

    request_animation_frame(callback.borrow().as_ref().expect("raf closure set"));
}

/// Build the `{id, locator}` job object posted to a worker.
fn job_message(id: u64, locator: &str) -> JsValue {
    let obj = js_sys::Object::new();
    let _ = js_sys::Reflect::set(
        &obj,
        &JsValue::from_str("id"),
        &JsValue::from_f64(id as f64),
    );
    let _ = js_sys::Reflect::set(
        &obj,
        &JsValue::from_str("locator"),
        &JsValue::from_str(locator),
    );
    obj.into()
}

/// Read a numeric field off a worker's reply object (0.0 if absent/malformed).
fn reflect_f64(obj: &JsValue, key: &str) -> f64 {
    js_sys::Reflect::get(obj, &JsValue::from_str(key))
        .ok()
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0)
}

/// Read a boolean field off a worker's reply object (false if absent/malformed).
fn reflect_bool(obj: &JsValue, key: &str) -> bool {
    js_sys::Reflect::get(obj, &JsValue::from_str(key))
        .ok()
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

/// Pool size from `?workers=N`, defaulted and clamped to a sane range.
fn worker_count() -> usize {
    web_sys::window()
        .and_then(|w| w.location().search().ok())
        .map(|search| parse_workers(&search))
        .unwrap_or(DEFAULT_WORKERS)
        .clamp(MIN_WORKERS, MAX_WORKERS)
}

/// Parse `workers=N` out of a `?a=b&workers=4` query string; default if absent.
fn parse_workers(search: &str) -> usize {
    search
        .trim_start_matches('?')
        .split('&')
        .find_map(|pair| pair.strip_prefix("workers="))
        .and_then(|n| n.parse::<usize>().ok())
        .unwrap_or(DEFAULT_WORKERS)
}

/// Render the live worker panel: one row per worker (idle / decoding asset N),
/// plus a stats line with the queue depth and peak concurrent workers.
fn update_worker_panel(
    document: &Document,
    worker_job: &[Option<u64>],
    busy: usize,
    queued: usize,
    peak: usize,
) {
    let rows: String = worker_job
        .iter()
        .enumerate()
        .map(|(i, job)| {
            let (busy_attr, label) = match job {
                Some(id) => ("1", format!("decoding asset {id}")),
                None => ("0", "idle".to_string()),
            };
            format!("<li data-worker=\"{i}\" data-busy=\"{busy_attr}\">worker {i}: {label}</li>")
        })
        .collect();
    if let Some(list) = document.get_element_by_id("workers") {
        list.set_inner_html(&rows);
    }
    if let Some(stats) = document.get_element_by_id("pool-stats") {
        stats.set_text_content(Some(&format!(
            "pool: {} workers · {busy} busy now · peak {peak} concurrent · {queued} queued",
            worker_job.len()
        )));
    }
}

/// Mirror progress onto `window.__assetDemo` for the e2e test — the same shape the
/// main-thread variant writes, plus `workerCount` and `peakBusy` so the test can
/// prove genuine pool parallelism (peak > 1).
fn update_window_state_pool(
    state: &PoolState,
    worker_count: usize,
    ready: usize,
    total: usize,
    failed: usize,
    in_flight: usize,
    done: bool,
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
    set("workerCount", JsValue::from_f64(worker_count as f64));
    set("peakBusy", JsValue::from_f64(state.peak_busy as f64));
    let order = js_sys::Array::new();
    for raw in &state.ready_order {
        order.push(&JsValue::from_f64(*raw as f64));
    }
    set("readyOrder", order.into());
    let _ = js_sys::Reflect::set(&window, &JsValue::from_str("__assetDemo"), &object);
}
