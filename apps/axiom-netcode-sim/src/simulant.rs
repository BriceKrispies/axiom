//! The two interchangeable client backends each peer can run.

use core::fmt;

use axiom::prelude::*;

use crate::config::Backend;

/// The `submit_local` kind tag for a move payload (app-defined; opaque to netcode).
pub(crate) const MOVE_KIND: u32 = 0;

/// Units a player moves per unit of input.
pub(crate) const MOVE_SPEED: f32 = 0.05;

/// One peer's local simulation. Every peer in a run uses the same backend and
/// applies the same confirmed inputs in the same order, so they converge.
pub(crate) trait Simulant: fmt::Debug {
    /// Apply `tick`'s confirmed `(peer, kind, payload)` inputs (already in peer
    /// order) and return the bytes to fingerprint this peer's state.
    fn step(&mut self, tick: u64, inputs: &[(u64, u32, Vec<u8>)]) -> Vec<u8>;
}

/// Construct the backend for a run.
pub(crate) fn build(backend: Backend, peers: usize) -> Box<dyn Simulant> {
    let engine: fn(usize) -> Box<dyn Simulant> = |p| Box::new(EngineSimulant::new(p));
    let mock: fn(usize) -> Box<dyn Simulant> = |_| Box::<MockSimulant>::default();
    [mock, engine][(backend == Backend::Engine) as usize](peers)
}

/// Encode a move delta as the netcode payload: x and y as two little-endian f32s.
pub(crate) fn encode_delta(delta: Vec3) -> Vec<u8> {
    let mut out = Vec::with_capacity(8);
    out.extend_from_slice(&delta.x.to_le_bytes());
    out.extend_from_slice(&delta.y.to_le_bytes());
    out
}

/// Decode a move payload; a short/garbled payload is no movement.
fn decode_delta(payload: &[u8]) -> Vec3 {
    payload
        .get(0..8)
        .map(|p| {
            let x = f32::from_le_bytes([p[0], p[1], p[2], p[3]]);
            let y = f32::from_le_bytes([p[4], p[5], p[6], p[7]]);
            Vec3::new(x, y, 0.0)
        })
        .unwrap_or(Vec3::ZERO)
}

/// A finite linear colour channel from an authored literal.
fn ch(value: f32) -> Ratio {
    Ratio::new(value).expect("authored colour channel is finite")
}

/// The full-fidelity backend: a real engine `App` with one controllable cube per
/// peer. Inputs actually move the cubes, and the fingerprint is the real packed
/// frame, so two peers agree iff their engines agree byte-for-byte.
struct EngineSimulant {
    app: RunningApp,
}

impl EngineSimulant {
    fn new(peers: usize) -> Self {
        EngineSimulant {
            app: build_app(peers),
        }
    }
}

impl fmt::Debug for EngineSimulant {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EngineSimulant").finish_non_exhaustive()
    }
}

impl Simulant for EngineSimulant {
    fn step(&mut self, tick: u64, inputs: &[(u64, u32, Vec<u8>)]) -> Vec<u8> {
        let player_inputs: Vec<PlayerInput> = inputs
            .iter()
            .map(|(peer, _kind, payload)| {
                PlayerInput::new((peer.saturating_sub(1)) as u32, decode_delta(payload))
            })
            .collect();
        let outcome = self.app.tick_with(tick, &player_inputs);
        outcome
            .instance_floats()
            .iter()
            .flat_map(|f| f.to_le_bytes())
            .collect()
    }
}

/// Build an `App` with `peers` player cubes spread along x, a pulled-back camera,
/// and a light. No spin: the cubes move only in response to player input.
fn build_app(peers: usize) -> RunningApp {
    App::new()
        .window(Window::new(800, 600).with_clear_color(Color::linear_rgb(
            ch(0.04),
            ch(0.05),
            ch(0.08),
        )))
        .add_plugins(DefaultPlugins)
        .setup(move |world, meshes, materials| {
            let cube = meshes.add(Mesh::cube());
            let span = peers.max(1) as f32;
            (0..peers).for_each(|i| {
                let x = (i as f32) - (span - 1.0) * 0.5;
                let material = materials.add(Material::lit(Color::linear_rgb(
                    ch(0.30 + 0.5 * ((i % 2) as f32)),
                    ch(0.35),
                    ch(0.85 - 0.4 * ((i % 2) as f32)),
                )));
                world.spawn((
                    Transform::from_translation(Vec3::new(x * 1.4, 0.0, 0.0)),
                    Renderable {
                        mesh: cube,
                        material,
                    },
                    Player::new(i as u32),
                ));
            });
            world.spawn((
                Transform::from_translation(Vec3::new(0.0, 0.0, 6.0 + span)),
                Camera::perspective(PerspectiveProjection {
                    fov_y: Angle::degrees(60.0),
                    near: Meters::new(0.1).expect("near plane is finite"),
                    far: Meters::new(1000.0).expect("far plane is finite"),
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
        .build()
}

/// The scale backend: a cheap deterministic fold over the confirmed inputs
/// (mirrors the convergence proof's `mix`). Identical inputs ⇒ identical state.
#[derive(Debug, Default)]
struct MockSimulant {
    state: u64,
}

impl Simulant for MockSimulant {
    fn step(&mut self, tick: u64, inputs: &[(u64, u32, Vec<u8>)]) -> Vec<u8> {
        let seed = self.state ^ tick.wrapping_mul(0x9E37_79B9_7F4A_7C15);
        let s = inputs.iter().fold(seed, |acc, (peer, kind, payload)| {
            let acc = acc.rotate_left(7) ^ peer.wrapping_mul(0xD1B5_4A32_D192_ED03);
            let acc = acc.wrapping_add(*kind as u64);
            payload
                .iter()
                .fold(acc, |a, &b| (a ^ b as u64).wrapping_mul(0x0000_0100_0000_01B3))
        });
        self.state = s;
        s.to_le_bytes().to_vec()
    }
}
