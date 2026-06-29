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

/// The deterministic RNG seam (SPEC-01) the TS `Rng` projection drives, routed
/// over the real `axiom-entropy` facade. Native-testable; the `wasm32` boundary
/// marshals to it through [`GameBridge`].
mod rng;
pub use rng::RngHub;

/// The deterministic native core the `wasm32` boundary marshals to — the
/// fixed-step loop, the seeded RNG hub, and the terminal-outcome latch composed
/// into one [`GameBridge`]. The rlib heart proven by the slice tests in
/// `bridge.rs`; the thin JS-marshalling shell lives in [`wasm`].
mod bridge;
pub use bridge::GameBridge;

/// The retained-world component vocabulary (SPEC-02) and the branchless
/// kind→codec dispatch the bridge routes `worldSet`/`worldGet` through. Defines
/// the closed game-component `Reflect` types (`Transform`/`Velocity`/`Sprite`/…)
/// whose schema names are the TS `Component.kind` keys, and documents the
/// `(kind, bytes)` marshalling convention every later subsystem reuses.
mod world;

/// Physics (SPEC-10): a deterministic rigid-body world over `axiom-physics`
/// composed into [`GameBridge`], stepped inside the fixed-step loop and written
/// back to each bodied entity's `Transform`. Native-testable; the `wasm32`
/// boundary marshals to it through `GameBridge`. See [`physics`] for the boundary
/// convention and the physics-facade gaps it works within.
mod physics;

/// Input (SPEC-05): the deterministic per-tick intent snapshot over
/// `axiom-input` composed into [`GameBridge`] — a live device-state accumulator
/// the browser feeds raw key/pointer events into, sampled once per fixed tick
/// inside the loop, and projected to the TS `NativeBridge` input reads.
/// Native-testable; the `wasm32` boundary marshals to it through `GameBridge`.
mod input;

/// Timers + state machines (SPEC-07) and tick-sampled tweens (SPEC-09): the
/// deterministic `axiom-tick` timer wheel + state machines and the `axiom-tween`
/// eased-curve table composed into [`GameBridge`], pumped once per fixed tick
/// inside the loop and projected to the TS `NativeBridge` timer / machine / tween
/// reads. Native-testable; the `wasm32` boundary marshals to it through `GameBridge`.
mod time;

/// Grid / pathfinding (SPEC-06): the deterministic `axiom-grid` BFS / wavefront
/// core composed into [`GameBridge`], projected to the TS `HostBridge` grid
/// surface. Pure functions of a passability mask + endpoint cells; native-tested
/// against a known board, the `wasm32` boundary marshals to it through `GameBridge`.
mod grid;

/// 3D / scalar math (SPEC-03 / SPEC-11): the `v3` / `mat4` / `quat` ops projected
/// to the TS `HostBridge` math surface, every one forwarding to `axiom-math` (the
/// single deterministic source of truth — no math re-implementation). Native-tested
/// against `MathApi` directly; the `wasm32` boundary marshals to it through `GameBridge`.
mod mathbridge;

/// Audio (SPEC-08): the neutral `axiom-audio` mixer core composed into
/// [`GameBridge`] (handle allocation, tone/playlist/scheduling bookkeeping), with
/// the live Web Audio output realized in the `#[cfg(target_arch = "wasm32")]` arm.
/// Native-tested for deterministic handle allocation; playback is browser-proven.
mod audio;

/// The embed seam (SPEC-12): decode the inbound session config, latch the single
/// outbound outcome. Pure, native-testable core; the browser channel that carries
/// it lives in [`wasm`]. Reached at runtime only from the `wasm32` boundary, so on
/// a non-wasm build it is exercised solely by the slice tests below.
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
mod embed;

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

    #[test]
    fn the_embed_seam_reads_a_seed_and_reports_one_outcome() {
        use crate::embed::{decode_session_config, OutcomeLatch};

        // Inbound: a host-supplied query decodes to a session config carrying a
        // seed and opaque params — resolved before tick 0 (SPEC-12 §6).
        let config = decode_session_config("?seed=5&mode=ranked");
        assert_eq!(config.seed(), 5);
        assert_eq!(
            config.params().get("mode"),
            Some(&HostParamValue::Text(String::from("ranked")))
        );

        // Advance the authored game BY the seed: run exactly `seed` deterministic
        // fixed ticks of the demo game.
        let mut rt = GameRuntime::new(demo_app().build(), STEP_60HZ, u32::MAX);
        (0..config.seed()).for_each(|_step| {
            rt.advance(STEP_60HZ);
        });
        assert_eq!(rt.tick(), 5);

        // Outbound: mint exactly one terminal outcome derived from the final
        // deterministic state and latch it. A second report is a no-op, so the
        // game cannot report two terminal states.
        let host = HostApi::new();
        let outcome = host.outcome(rt.tick() > 0, Score::new(rt.tick() as f64));
        let mut latch = OutcomeLatch::new();
        assert!(latch.report(outcome.clone()));
        assert!(!latch.report(host.outcome(false, Score::new(0.0))));
        assert_eq!(latch.reported(), Some(&outcome));
        assert_eq!(latch.reported().map(HostOutcome::score), Some(Score::new(5.0)));
    }
}
