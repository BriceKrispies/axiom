//! The `#[wasm_bindgen]` boundary the TypeScript SDK binds — `wasm32`-only.
//!
//! This is deliberately thin: the deterministic work lives in [`GameBridge`],
//! tested natively. Here we only expose a JS-constructable [`WasmGame`] that wraps
//! that bridge and a per-frame [`WasmGame::advance`] the host's
//! `requestAnimationFrame` loop calls with the elapsed nanoseconds it measured.
//! `advance` hands back the integer [`StepReport`] so the JS presentation layer
//! computes its own interpolation fraction (`remainder_nanos / fixed_step_nanos`)
//! — no wall-clock value crosses into a fixed tick.
//!
//! This boundary also owns the embed seam's **host channel** (SPEC-12): on
//! construction it decodes the inbound [`HostSessionConfig`] from
//! `window.location.search` (before tick 0), and [`WasmGame::report_outcome`]
//! drains the engine's single [`HostOutcome`] back out to the parent frame via
//! `window.parent.postMessage` exactly once (latched). The pure decode/latch
//! logic is in [`crate::embed`]; only the browser calls live here.

use wasm_bindgen::prelude::*;

use axiom::prelude::HostOutcome;

use crate::embed::decode_session_config;
use crate::{demo_app, GameBridge};

/// Read the inbound host query string (`window.location.search`). Returns an
/// empty string if there is no window/location, so the decode falls back to the
/// default session config (seed `0`, no params).
fn host_query() -> String {
    web_sys::window()
        .and_then(|window| window.location().search().ok())
        .unwrap_or_default()
}

/// Forward `outcome` to the parent frame as a JSON `"complete"` message — the one
/// universal word every hosted game speaks. Best-effort: if there is no parent
/// window (top-level, not embedded) the post is simply skipped.
fn post_outcome_to_parent(outcome: &HostOutcome) {
    let won = outcome.won();
    let score = outcome.score().get();
    let payload = format!("{{\"type\":\"complete\",\"won\":{won},\"score\":{score}}}");
    let parent = web_sys::window().and_then(|window| window.parent().ok().flatten());
    if let Some(parent) = parent {
        let _ = parent.post_message(&JsValue::from_str(&payload), "*");
    }
}

/// The integer step budget one `advance` produced, marshalled to JS. The SDK's
/// platform-edge bridge reads these and computes the `0..1` interpolation
/// fraction itself (float math is unconstrained at the presentation boundary).
#[wasm_bindgen]
#[derive(Debug)]
pub struct StepReport {
    steps: u32,
    remainder_nanos: u64,
    fixed_step_nanos: u64,
}

#[wasm_bindgen]
impl StepReport {
    /// How many fixed simulation steps `advance` ran this frame.
    #[wasm_bindgen(getter)]
    pub fn steps(&self) -> u32 {
        self.steps
    }

    /// Sub-step time left banked after this frame, in `[0, fixed_step_nanos)`.
    #[wasm_bindgen(getter)]
    pub fn remainder_nanos(&self) -> u64 {
        self.remainder_nanos
    }

    /// The fixed step size, so the SDK can compute `remainder_nanos / fixed_step_nanos`.
    #[wasm_bindgen(getter)]
    pub fn fixed_step_nanos(&self) -> u64 {
        self.fixed_step_nanos
    }
}

/// The JS-facing game object. Construct it with the fixed step (nanoseconds) and
/// the per-frame step ceiling, then call [`Self::advance`] once per host frame.
///
/// On construction it resolves the inbound [`HostSessionConfig`] from the host
/// query string (the embed seam's `getSessionConfig`, SPEC-12 §4.2); the seed is
/// fixed for the whole session and read via [`Self::seed`]. The single terminal
/// outcome is emitted once through [`Self::report_outcome`].
#[wasm_bindgen]
#[derive(Debug)]
pub struct WasmGame {
    bridge: GameBridge,
}

#[wasm_bindgen]
impl WasmGame {
    /// Build the deterministic demo game and wrap it in the bridge core. Installs
    /// the panic hook so a Rust panic surfaces as a readable JS error, and decodes
    /// the inbound session config (seed + params) before tick 0 — the seed keys
    /// the bridge's RNG hub for the whole session.
    #[wasm_bindgen(constructor)]
    pub fn new(fixed_step_nanos: u64, max_steps: u32) -> WasmGame {
        console_error_panic_hook::set_once();
        let config = decode_session_config(&host_query());
        WasmGame {
            bridge: GameBridge::new(
                demo_app().build(),
                config.seed(),
                fixed_step_nanos,
                max_steps,
            ),
        }
    }

    /// The host-supplied session seed, fixed before tick 0 (the determinism input
    /// the bridge's `Rng` is seeded from). Constant for the whole session.
    #[wasm_bindgen(getter)]
    pub fn seed(&self) -> u64 {
        self.bridge.seed()
    }

    /// Report the terminal outcome (`reportOutcome`, SPEC-12 §4.2). The first
    /// call latches and forwards exactly one [`HostOutcome`] to the parent frame;
    /// any later call is a no-op. Returns whether this call was the one accepted.
    pub fn report_outcome(&mut self, won: bool, score: f64) -> bool {
        let accepted = self.bridge.report_outcome(won, score);
        if accepted {
            if let Some(latched) = self.bridge.reported_outcome() {
                post_outcome_to_parent(latched);
            }
        }
        accepted
    }

    /// Bank `elapsed_nanos` of real host time, run the resulting whole fixed
    /// ticks, and report the integer budget for the SDK to interpolate with.
    pub fn advance(&mut self, elapsed_nanos: u64) -> StepReport {
        let budget = self.bridge.advance(elapsed_nanos);
        StepReport {
            steps: budget.steps(),
            remainder_nanos: budget.remainder_nanos(),
            fixed_step_nanos: budget.fixed_step_nanos(),
        }
    }

    /// The monotonic count of fixed ticks driven so far.
    #[wasm_bindgen(getter)]
    pub fn current_tick(&self) -> u64 {
        self.bridge.tick()
    }

    /// The durable simulation state as opaque bytes — the host stores or compares
    /// these to checkpoint or verify determinism.
    pub fn snapshot(&self) -> Vec<u8> {
        self.bridge.snapshot_sim()
    }

    // --- Deterministic RNG seam (SPEC-01) ---
    //
    // The `NativeBridge` rng methods, marshalled to the bridge's seeded
    // [`crate::RngHub`]. The `js_name` is the camelCase identifier the TS
    // `bridgeFromWasm` adapter forwards verbatim (`game.rngUnit`, ...). Stream
    // ids are opaque JS numbers the hub owns; id `0` is the root.

    /// A uniform float in `[0, 1)` from `stream` (`Rng::unit`).
    #[wasm_bindgen(js_name = rngUnit)]
    pub fn rng_unit(&mut self, stream: u32) -> f64 {
        self.bridge.rng_unit(stream)
    }

    /// A uniform integer in `[0, max_exclusive)` from `stream` (`Rng::int`).
    #[wasm_bindgen(js_name = rngBelow)]
    pub fn rng_below(&mut self, stream: u32, max_exclusive: u32) -> u32 {
        self.bridge.rng_below(stream, u64::from(max_exclusive)) as u32
    }

    /// The index `weights` selects, drawn proportionally to the weights, from
    /// `stream` (`Rng::weighted`). JS weights are plain numbers; each is floored
    /// to a non-negative integer weight (the exact, cross-machine form the
    /// entropy facade selects over).
    #[wasm_bindgen(js_name = rngWeighted)]
    pub fn rng_weighted(&mut self, stream: u32, weights: &[f64]) -> u32 {
        let weights: Vec<u64> = weights.iter().map(|&w| w.max(0.0) as u64).collect();
        self.bridge.rng_weighted(stream, &weights)
    }

    /// A Fisher-Yates permutation of `[0, length)` the core drew from `stream`
    /// (`Rng::permutation`). Returned as a real JS `number[]` (not a typed array)
    /// so it matches the contract's `readonly number[]` and the projection can map
    /// the author's array through it.
    #[wasm_bindgen(js_name = rngPermutation)]
    pub fn rng_permutation(&mut self, stream: u32, length: u32) -> Vec<JsValue> {
        self.bridge
            .rng_permutation(stream, length)
            .into_iter()
            .map(|index| JsValue::from_f64(f64::from(index)))
            .collect()
    }

    /// Resolve the deterministic id of the named sub-stream of `parent`
    /// (`Rng::stream`). Idempotent: the same `(parent, name)` resolves to the same
    /// id.
    #[wasm_bindgen(js_name = rngStream)]
    pub fn rng_stream(&mut self, parent: u32, name: String) -> u32 {
        self.bridge.rng_stream(parent, &name)
    }
}

/// Page entry: install the panic hook. The page then constructs a [`WasmGame`]
/// and drives it from its own `requestAnimationFrame` loop (in the TS SDK).
#[wasm_bindgen]
pub fn start() {
    console_error_panic_hook::set_once();
}
