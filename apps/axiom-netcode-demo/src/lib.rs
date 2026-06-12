//! # Axiom Netcode Demo — the lockstep session driving the *real* engine
//!
//! The `axiom-netcode` module proved its session logic against a mock sim. This
//! app closes the loop: it drives two independent **real** engine `App`s through
//! the deterministic-lockstep session and shows they stay byte-identical at
//! every confirmed tick — the per-tick state hash is taken from the actual
//! engine [`FrameOutcome`], not a stand-in.
//!
//! It is an app because composing two modules is an app's job: it names both the
//! `axiom` umbrella (the engine `App`) and `axiom-netcode` (the session), and
//! owns the one piece of glue between them — turning a `FrameOutcome` into the
//! neutral bytes netcode hashes. The proof is in-process and native: no sockets,
//! no browser. (Inputs are carried through the timeline but do not yet drive the
//! simulation — sim-affecting input is the next slice, once the engine grows a
//! per-tick input channel.)

use axiom::prelude::*;
use axiom_netcode::NetcodeApi;

const PEER_A: u64 = 1;
const PEER_B: u64 = 2;

/// A finite linear colour channel from an authored literal.
fn ch(value: f32) -> Ratio {
    Ratio::new(value).expect("authored colour channel is finite")
}

/// Build a spinning-cube engine `App` with `cubes` renderable cubes, a camera,
/// and a light. Two peers given the same `cubes` run an identical world; a
/// different count is a divergent world (used to prove desync detection).
fn build_app(cubes: usize) -> RunningApp {
    App::new()
        .window(Window::new(800, 600).with_clear_color(Color::linear_rgb(
            ch(0.05),
            ch(0.06),
            ch(0.08),
        )))
        .add_plugins(DefaultPlugins)
        .setup(move |scene, meshes, materials| {
            let cube = meshes.add(Mesh::cube());
            for i in 0..cubes {
                let offset = i as f32 * 2.6 - 2.6;
                let material = materials.add(Material::lit(Color::linear_rgb(
                    ch(0.85),
                    ch(0.25),
                    ch(0.25),
                )));
                scene
                    .spawn(Transform::from_translation(Vec3::new(offset, 0.0, 0.0)))
                    .with_child((
                        Renderable {
                            mesh: cube,
                            material,
                        },
                        Spin::around(Vec3::UNIT_Y).period(360),
                    ));
            }
            scene.spawn((
                Transform::from_translation(Vec3::new(0.0, 0.0, 8.0)),
                Camera::perspective(PerspectiveProjection {
                    fov_y: Angle::degrees(60.0),
                    near: Meters::new(0.1).expect("near plane is finite"),
                    far: Meters::new(100.0).expect("far plane is finite"),
                }),
            ));
            scene.spawn((
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

/// Translate a frame outcome into the neutral bytes netcode fingerprints: the
/// packed per-object instance floats (MVP + colour), the clear colour, and the
/// command count. This is the app-owned bridge between the two modules.
fn frame_state_bytes(outcome: &FrameOutcome) -> Vec<u8> {
    let mut bytes = Vec::new();
    for f in outcome.instance_floats() {
        bytes.extend_from_slice(&f.to_le_bytes());
    }
    for c in outcome.clear_color() {
        bytes.extend_from_slice(&c.to_le_bytes());
    }
    bytes.extend_from_slice(&(outcome.command_count() as u64).to_le_bytes());
    bytes
}

/// The per-tick state-hash logs both peers recorded, plus the first tick (if
/// any) at which reconciliation reported a desync.
#[derive(Debug, Clone)]
pub struct LockstepReport {
    /// Peer A's state hash at each confirmed tick.
    pub peer_a_hashes: Vec<[u8; 32]>,
    /// Peer B's state hash at each confirmed tick.
    pub peer_b_hashes: Vec<[u8; 32]>,
    /// The first tick at which `reconcile` reported a desync, if any.
    pub desync_tick: Option<u64>,
}

/// Confirm the ready tick, step the real engine at it, hash the frame outcome,
/// record the hash in `log`, and return the beacon bytes to broadcast.
fn confirm_and_step(
    net: &mut NetcodeApi,
    app: &mut RunningApp,
    log: &mut Vec<[u8; 32]>,
) -> Vec<u8> {
    let tick = net
        .ready_tick()
        .expect("both peers submitted this tick, so it is ready");
    // Inputs are carried through the session but do not yet drive the sim.
    let _inputs = net.confirm_tick(tick);
    let outcome = app.tick(tick);
    let bytes = frame_state_bytes(&outcome);
    log.push(net.digest(&bytes));
    net.record_local_hash(tick, &bytes)
}

/// Drive two real engine `App`s through the lockstep session for `ticks` frames
/// over a clean in-process channel. Peer A runs a fixed three-cube world; peer B
/// runs `peer_b_cubes` cubes (pass `3` for an identical world, any other count
/// for a divergent one). Because the per-tick hash comes from the real engine's
/// `FrameOutcome`, equal hashes mean the two engines agree byte-for-byte.
pub fn run_two_peer_lockstep(ticks: u64, peer_b_cubes: usize) -> LockstepReport {
    let mut app_a = build_app(3);
    let mut app_b = build_app(peer_b_cubes);
    let peers = [PEER_A, PEER_B];
    let mut net_a = NetcodeApi::new(PEER_A, &peers);
    let mut net_b = NetcodeApi::new(PEER_B, &peers);

    let mut report = LockstepReport {
        peer_a_hashes: Vec::new(),
        peer_b_hashes: Vec::new(),
        desync_tick: None,
    };

    for tick in 0..ticks {
        // 1. Both peers submit their input for this tick and exchange them.
        let in_a = net_a.submit_local(0, &[tick as u8]);
        let in_b = net_b.submit_local(0, &[tick as u8]);
        net_a.ingest(&in_b).expect("well-formed input frame");
        net_b.ingest(&in_a).expect("well-formed input frame");

        // 2. Both confirm the now-ready tick and step their real engine.
        let beacon_a = confirm_and_step(&mut net_a, &mut app_a, &mut report.peer_a_hashes);
        let beacon_b = confirm_and_step(&mut net_b, &mut app_b, &mut report.peer_b_hashes);

        // 3. Exchange the state-hash beacons.
        net_b.ingest(&beacon_a).expect("well-formed beacon");
        net_a.ingest(&beacon_b).expect("well-formed beacon");

        // 4. Reconcile peer A's view of this tick; record the first desync.
        if net_a.reconcile(tick) == Some(false) && report.desync_tick.is_none() {
            report.desync_tick = Some(tick);
        }
    }

    report
}
