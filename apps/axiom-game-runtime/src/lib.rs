//! # Axiom — Game Runtime (wasm-bindgen boundary)
//!
//! The native boundary the TypeScript authoring SDK (`@axiom/game`) projects
//! through. It owns [`GameRuntime`] — the deterministic driver that banks a real,
//! variable-rate host elapsed interval into whole fixed simulation steps and runs
//! exactly that many deterministic ticks on a [`RunningApp`](axiom::prelude::RunningApp).
//!
//! The split is the determinism boundary made physical:
//! - the **accumulator** (in the `frame` layer, re-exported through `axiom`)
//!   decides *how many* fixed steps a frame runs — pure integer arithmetic;
//! - this **runtime** drives exactly that many `tick`s — deterministic;
//! - the **TS SDK** owns the clock and the render and computes the `0..1`
//!   interpolation fraction from the returned [`StepBudget`](axiom::prelude::StepBudget).
//!
//! The deterministic core ([`runtime`]) is the rlib part, proven by the native
//! slice tests below and in `runtime.rs`; the `#[wasm_bindgen]` `start`/`WasmGame`
//! entry (in [`wasm`], compiled only for `wasm32`) is the thin JS-facing boundary
//! the SDK binds. Nothing here reads a wall clock — elapsed time enters as data.

use axiom::prelude::*;

mod runtime;
pub use runtime::GameRuntime;

/// The `#[wasm_bindgen]` boundary the TS SDK binds — compiled only for `wasm32`,
/// so native `cargo test` never touches the browser glue.
#[cfg(target_arch = "wasm32")]
mod wasm;

/// A linear colour channel / intensity from a known-finite authored literal.
// Reached at runtime only from the `wasm32` `WasmGame` entry; on native it is
// used solely from tests, so the non-wasm library build sees it as dead.
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
fn ch(value: f32) -> Ratio {
    Ratio::new(value).expect("authored colour channel is finite")
}

/// The trivial deterministic demo game the runtime drives at M0: one cube that
/// spins about Y, a pulled-back camera, and a single directional light. The
/// spinning cube mutates its stored transform every tick, so the per-tick
/// simulation snapshot genuinely evolves — exactly the state a determinism /
/// replay proof needs. Authored against the engine's `App` API; browser-free, so
/// the native slice tests build and tick it without a surface.
// Used at runtime only by the `wasm32` boundary; on native it is reached solely
// from the slice tests, so the non-wasm library build sees it as dead.
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
pub(crate) fn demo_app() -> App {
    App::new()
        .window(Window::new(320, 240))
        .add_plugins(DefaultPlugins)
        .setup(|world, meshes, materials| {
            let cube = meshes.add(Mesh::cube());
            let material =
                materials.add(Material::lit(Color::linear_rgb(ch(0.85), ch(0.30), ch(0.30))));
            world
                .spawn(Transform::IDENTITY)
                .with_child((
                    Renderable {
                        mesh: cube,
                        material,
                    },
                    Spin::around(Vec3::UNIT_Y).period(120),
                ));
            world.spawn((
                Transform::from_translation(Vec3::new(0.0, 0.0, 6.0)),
                Camera::perspective(PerspectiveProjection {
                    fov_y: Angle::degrees(60.0),
                    near: Meters::new(0.1).expect("authored near plane is finite"),
                    far: Meters::new(100.0).expect("authored far plane is finite"),
                }),
            ));
            world.spawn((
                Transform::IDENTITY,
                DirectionalLight {
                    direction: Vec3::new(0.3, -1.0, 0.4),
                    color: Color::WHITE,
                    intensity: ch(1.0),
                },
            ));
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 60 Hz fixed step in nanoseconds — the cadence a real host frame budgets.
    const STEP_60HZ: u64 = 16_666_667;

    #[test]
    fn the_demo_game_runs_end_to_end_and_replays_deterministically() {
        // Drive a fixed wall-clock budget of ~half a second at a steady 60 Hz
        // host cadence through the runtime, then prove a second runtime fed the
        // identical elapsed sequence ends byte-identical: the whole boundary
        // (accumulator → tick loop → simulation snapshot) is deterministic.
        let drive = || -> (u64, Vec<u8>) {
            let mut rt = GameRuntime::new(demo_app().build(), STEP_60HZ, 8);
            (0..30u32).for_each(|_frame| {
                rt.advance(STEP_60HZ);
            });
            (rt.tick(), rt.snapshot_sim())
        };
        let (ticks, snapshot) = drive();
        // A steady one-step-per-frame budget runs exactly one tick per frame.
        assert_eq!(ticks, 30);
        assert_eq!(drive(), (ticks, snapshot));
    }
}
