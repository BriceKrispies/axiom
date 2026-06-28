//! The `#[wasm_bindgen]` boundary the TypeScript SDK binds — `wasm32`-only.
//!
//! This is deliberately thin: the deterministic work lives in [`GameRuntime`],
//! tested natively. Here we only expose a JS-constructable [`WasmGame`] that wraps
//! a runtime and a per-frame [`WasmGame::advance`] the host's
//! `requestAnimationFrame` loop calls with the elapsed nanoseconds it measured.
//! `advance` hands back the integer [`StepReport`] so the JS presentation layer
//! computes its own interpolation fraction (`remainder_nanos / fixed_step_nanos`)
//! — no wall-clock value crosses into a fixed tick.

use wasm_bindgen::prelude::*;

use crate::{demo_app, GameRuntime};

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
#[wasm_bindgen]
pub struct WasmGame {
    runtime: GameRuntime,
}

#[wasm_bindgen]
impl WasmGame {
    /// Build the deterministic demo game and wrap it in a fixed-step runtime.
    /// Installs the panic hook so a Rust panic surfaces as a readable JS error.
    #[wasm_bindgen(constructor)]
    pub fn new(fixed_step_nanos: u64, max_steps: u32) -> WasmGame {
        console_error_panic_hook::set_once();
        WasmGame {
            runtime: GameRuntime::new(demo_app().build(), fixed_step_nanos, max_steps),
        }
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
