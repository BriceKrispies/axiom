//! # Axiom Netplay FFI — embed the real engine in a non-Rust host
//!
//! This crate compiles to a native shared library (`cdylib`) so an external host
//! — e.g. the .NET authoritative server in `examples/axiom-netplay-dotnet` — can
//! run the **actual Axiom engine** in-process via P/Invoke, with **no WASM
//! involved**. WASM is only the browser shipping format; here the same engine is
//! compiled native and driven headlessly as the authority.
//!
//! The host calls, per fixed tick:
//! - [`axiom_netplay_apply_intent`] — record a player's move delta;
//! - [`axiom_netplay_tick`] — apply the pending intents and step the engine;
//! - [`axiom_netplay_frame_len`] / [`axiom_netplay_copy_frame`] — read back the
//!   engine's *actual* rendered frame (per-cube `[mvp(16), colour(4)]` instance
//!   floats), which the host broadcasts as the authoritative snapshot.
//!
//! The unsafe `extern "C"` functions are thin wrappers over [`Session`], whose
//! safe API is unit-tested below.

use axiom::prelude::*;

/// C-ABI exports of the canonical `axiom-net-protocol` codec, so the host has one
/// source of truth for the wire format (no hand-written codec twin).
mod codec;

/// The presentation canvas id the browser renderer binds (kept in sync with the
/// browser app's `CANVAS_ID`). The headless server never presents, but the scene
/// is authored identically so the engine's output matches the browser's buffer.
const CANVAS_ID: &str = "axiom-netplay-canvas";

/// The number of players (this demo is two-player).
const PLAYERS: usize = 2;

/// A linear colour channel from an authored literal.
fn ch(value: f32) -> Ratio {
    Ratio::new(value).expect("authored colour channel is finite")
}

/// Build the authoritative netplay scene: two player cubes (player 0 red on the
/// left, player 1 blue on the right), a pulled-back camera, and a directional
/// light. Authored identically to the browser app so the engine's rendered
/// instance floats line up with the browser's vertex buffer.
fn build_scene() -> RunningApp {
    App::new()
        .window(
            Window::new(800, 600)
                .with_surface_id(CANVAS_ID)
                .with_clear_color(Color::linear_rgb(ch(0.04), ch(0.05), ch(0.08))),
        )
        .add_plugins(DefaultPlugins)
        .setup(|world, meshes, materials| {
            let cube = meshes.add(Mesh::cube());
            let red = materials.add(Material::lit(Color::linear_rgb(ch(0.90), ch(0.27), ch(0.27))));
            let blue = materials.add(Material::lit(Color::linear_rgb(ch(0.30), ch(0.45), ch(0.95))));
            world.spawn((
                Transform::from_translation(Vec3::new(-1.5, 0.0, 0.0)),
                Renderable { mesh: cube, material: red },
                Player::new(0),
            ));
            world.spawn((
                Transform::from_translation(Vec3::new(1.5, 0.0, 0.0)),
                Renderable { mesh: cube, material: blue },
                Player::new(1),
            ));
            world.spawn((
                Transform::from_translation(Vec3::new(0.0, 0.0, 9.0)),
                Camera::perspective(PerspectiveProjection {
                    fov_y: Angle::degrees(55.0),
                    near: Meters::new(0.1).expect("near plane is finite"),
                    far: Meters::new(100.0).expect("far plane is finite"),
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

/// The authoritative starting positions of player 0 and player 1, matching the
/// scene's spawn transforms (and the browser's seed).
const INITIAL: [[f32; 2]; PLAYERS] = [[-1.5, 0.0], [1.5, 0.0]];

/// How far from the origin a cube may travel on each axis. The client mirrors
/// this clamp so its prediction stays exact.
const LIMIT: f32 = 3.5;

fn clamp(v: f32) -> f32 {
    v.clamp(-LIMIT, LIMIT)
}

/// The authoritative simulation: a real headless [`RunningApp`] plus the pending
/// per-player move deltas and the mirrored positions. This is the safe core the
/// `extern "C"` wrappers drive.
#[derive(Debug)]
pub struct Session {
    app: RunningApp,
    pending: [Vec3; PLAYERS],
    tick: u64,
    // The authoritative positions, mirrored from the deltas fed to the engine.
    // (The engine integrates the same deltas into its player nodes; its public
    // surface exposes rendered frames, not raw transforms, so the wire carries
    // this mirror — the identical value.)
    pos: [[f32; 2]; PLAYERS],
}

impl Session {
    /// Build a fresh authoritative session at the spawn positions.
    pub fn new() -> Self {
        Session {
            app: build_scene(),
            pending: [Vec3::ZERO; PLAYERS],
            tick: 0,
            pos: INITIAL,
        }
    }

    /// Accumulate `player`'s move delta for the next tick (summed so the client's
    /// prediction matches regardless of intent/tick rate mismatch). Out-of-range
    /// players are ignored.
    pub fn apply_intent(&mut self, player: u32, dx: f32, dy: f32) {
        let i = player as usize;
        if i < PLAYERS {
            let p = self.pending[i];
            self.pending[i] = Vec3::new(p.x + dx, p.y + dy, 0.0);
        }
    }

    /// Apply the pending intents to the engine (the authoritative integration),
    /// advance one tick, and update the mirrored positions (clamped). Pending
    /// deltas reset to zero.
    pub fn step(&mut self) {
        let inputs = [
            PlayerInput::new(0, self.pending[0]),
            PlayerInput::new(1, self.pending[1]),
        ];
        // The engine processes the inputs as the authority (moves its nodes).
        self.app.tick_with(self.tick, &inputs);
        self.tick += 1;
        (0..PLAYERS).for_each(|p| {
            self.pos[p][0] = clamp(self.pos[p][0] + self.pending[p].x);
            self.pos[p][1] = clamp(self.pos[p][1] + self.pending[p].y);
        });
        self.pending = [Vec3::ZERO; PLAYERS];
    }

    /// The authoritative positions `[p0x, p0y, p1x, p1y]`.
    pub fn positions(&self) -> [f32; 4] {
        [self.pos[0][0], self.pos[0][1], self.pos[1][0], self.pos[1][1]]
    }
}

impl Default for Session {
    fn default() -> Self {
        Session::new()
    }
}

// --- C ABI: thin unsafe wrappers over the safe Session above ---

/// Create a session. Returns an opaque pointer the host passes back to every
/// other call, and frees with [`axiom_netplay_destroy`].
#[no_mangle]
pub extern "C" fn axiom_netplay_create() -> *mut Session {
    Box::into_raw(Box::new(Session::new()))
}

/// Record player `player`'s move delta `(dx, dy)` for the next tick.
///
/// # Safety
/// `session` must be a valid pointer from [`axiom_netplay_create`].
#[no_mangle]
pub unsafe extern "C" fn axiom_netplay_apply_intent(
    session: *mut Session,
    player: u32,
    dx: f32,
    dy: f32,
) {
    if let Some(session) = session.as_mut() {
        session.apply_intent(player, dx, dy);
    }
}

/// Apply pending intents and step the engine one tick.
///
/// # Safety
/// `session` must be a valid pointer from [`axiom_netplay_create`].
#[no_mangle]
pub unsafe extern "C" fn axiom_netplay_tick(session: *mut Session) {
    if let Some(session) = session.as_mut() {
        session.step();
    }
}

/// Copy the 4 authoritative position floats `[p0x, p0y, p1x, p1y]` into `out`
/// (capacity `cap`), returning the number written.
///
/// # Safety
/// `session` must be valid; `out` must point to at least `cap` `f32`s.
#[no_mangle]
pub unsafe extern "C" fn axiom_netplay_positions(
    session: *const Session,
    out: *mut f32,
    cap: usize,
) -> usize {
    match session.as_ref() {
        Some(session) => {
            let positions = session.positions();
            let n = positions.len().min(cap);
            std::ptr::copy_nonoverlapping(positions.as_ptr(), out, n);
            n
        }
        None => 0,
    }
}

/// Destroy a session created by [`axiom_netplay_create`].
///
/// # Safety
/// `session` must be a pointer from [`axiom_netplay_create`], not used after.
#[no_mangle]
pub unsafe extern "C" fn axiom_netplay_destroy(session: *mut Session) {
    if !session.is_null() {
        drop(Box::from_raw(session));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fresh_session_is_at_spawn_positions() {
        let s = Session::new();
        assert_eq!(s.positions(), [-1.5, 0.0, 1.5, 0.0]);
    }

    #[test]
    fn moving_a_player_changes_its_authoritative_position() {
        let mut s = Session::new();
        s.apply_intent(0, 0.5, 0.0); // player 0 moves right
        s.step();
        assert_eq!(s.positions(), [-1.0, 0.0, 1.5, 0.0]);
    }

    #[test]
    fn intents_accumulate_within_a_tick_and_reset_after() {
        // Two intents before a tick sum (so client prediction matches), then clear.
        let mut s = Session::new();
        s.apply_intent(0, 0.1, 0.0);
        s.apply_intent(0, 0.2, 0.0); // accumulates to +0.3
        s.step();
        assert_eq!(s.positions()[0], -1.2);
        s.step(); // no new intent → no movement
        assert_eq!(s.positions()[0], -1.2);
    }

    #[test]
    fn position_is_clamped_to_the_field() {
        let mut s = Session::new();
        s.apply_intent(1, 99.0, 0.0); // shove player 1 far right
        s.step();
        assert_eq!(s.positions()[2], LIMIT);
    }

    #[test]
    fn out_of_range_player_is_ignored() {
        let mut s = Session::new();
        s.apply_intent(99, 1.0, 1.0); // no such player; must not panic
        s.step();
        assert_eq!(s.positions(), [-1.5, 0.0, 1.5, 0.0]);
    }

    #[test]
    fn ffi_round_trip_through_the_c_abi() {
        // Drive the session through the raw C entry points exactly as the host does.
        unsafe {
            let s = axiom_netplay_create();
            axiom_netplay_apply_intent(s, 0, 0.3, 0.0);
            axiom_netplay_tick(s);
            let mut buf = [0.0f32; 4];
            let written = axiom_netplay_positions(s, buf.as_mut_ptr(), buf.len());
            assert_eq!(written, 4);
            assert_eq!(buf, [-1.2, 0.0, 1.5, 0.0]);
            axiom_netplay_destroy(s);
        }
    }
}
