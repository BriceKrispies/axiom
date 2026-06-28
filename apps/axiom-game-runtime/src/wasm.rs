//! The `#[wasm_bindgen]` boundary the TypeScript SDK binds — `wasm32`-only.
//!
//! This is deliberately thin: the deterministic work lives in [`GameRuntime`],
//! tested natively. Here we only expose a JS-constructable [`WasmGame`] that wraps
//! a runtime and a per-frame [`WasmGame::advance`] the host's
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

use axiom::prelude::{HostApi, HostOutcome, Score};

use crate::embed::{decode_session_config, OutcomeLatch};
use crate::{demo_app, GameRuntime};

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
pub struct WasmGame {
    runtime: GameRuntime,
    seed: u64,
    outcome: OutcomeLatch,
}

#[wasm_bindgen]
impl WasmGame {
    /// Build the deterministic demo game and wrap it in a fixed-step runtime.
    /// Installs the panic hook so a Rust panic surfaces as a readable JS error,
    /// and decodes the inbound session config (seed + params) before tick 0.
    #[wasm_bindgen(constructor)]
    pub fn new(fixed_step_nanos: u64, max_steps: u32) -> WasmGame {
        console_error_panic_hook::set_once();
        let config = decode_session_config(&host_query());
        WasmGame {
            runtime: GameRuntime::new(demo_app().build(), fixed_step_nanos, max_steps),
            seed: config.seed(),
            outcome: OutcomeLatch::new(),
        }
    }

    /// The host-supplied session seed, fixed before tick 0 (the determinism
    /// input the sim's `Rng` is seeded from). Constant for the whole session.
    #[wasm_bindgen(getter)]
    pub fn seed(&self) -> u64 {
        self.seed
    }

    /// Report the terminal outcome (`reportOutcome`, SPEC-12 §4.2). The first
    /// call latches and forwards exactly one [`HostOutcome`] to the parent frame;
    /// any later call is a no-op. Returns whether this call was the one accepted.
    pub fn report_outcome(&mut self, won: bool, score: f64) -> bool {
        let outcome = HostApi::new().outcome(won, Score::new(score));
        let accepted = self.outcome.report(outcome);
        if accepted {
            if let Some(latched) = self.outcome.reported() {
                post_outcome_to_parent(latched);
            }
        }
        accepted
    }

    /// Bank `elapsed_nanos` of real host time, run the resulting whole fixed
    /// ticks, and report the integer budget for the SDK to interpolate with.
    pub fn advance(&mut self, elapsed_nanos: u64) -> StepReport {
        let budget = self.runtime.advance(elapsed_nanos);
        StepReport {
            steps: budget.steps(),
            remainder_nanos: budget.remainder_nanos(),
            fixed_step_nanos: budget.fixed_step_nanos(),
        }
    }

    /// The monotonic count of fixed ticks driven so far.
    #[wasm_bindgen(getter)]
    pub fn current_tick(&self) -> u64 {
        self.runtime.tick()
    }

    /// The durable simulation state as opaque bytes — the host stores or compares
    /// these to checkpoint or verify determinism.
    pub fn snapshot(&self) -> Vec<u8> {
        self.runtime.snapshot_sim()
    }
}

/// Page entry: install the panic hook. The page then constructs a [`WasmGame`]
/// and drives it from its own `requestAnimationFrame` loop (in the TS SDK).
#[wasm_bindgen]
pub fn start() {
    console_error_panic_hook::set_once();
}
