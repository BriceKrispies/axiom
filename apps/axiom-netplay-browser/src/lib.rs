//! # Axiom Netplay (browser) — live deterministic-lockstep multiplayer
//!
//! Two browsers each run the engine and control their own cube with the arrow
//! keys. Only inputs cross the wire (through `axiom-netcode-relay`); both clients
//! simulate, so the screens stay identical by determinism — no state is ever
//! sent. This app owns the nondeterministic edge (the WebSocket + the keyboard)
//! and the glue between the engine `App`, the lockstep session, and the live
//! windowing loop. The deterministic core (the scene, the input codec, the
//! lockstep stepping) is native-testable; the live arm is `wasm32`-only.

use axiom::prelude::*;

/// The presentation canvas element id (must match `web/index.html`).
pub const CANVAS_ID: &str = "axiom-netplay-canvas";

/// Units a player cube moves per tick while an arrow key is held.
pub const MOVE_SPEED: f32 = 0.06;

/// The `submit_local` kind tag for a move payload (app-defined; netcode treats
/// the payload as opaque bytes).
pub const MOVE_KIND: u32 = 0;

/// A linear colour channel from an authored literal.
fn ch(value: f32) -> Ratio {
    Ratio::new(value).expect("authored colour channel is finite")
}

/// The arrow-key state, polled each frame into a move delta.
#[derive(Debug, Default, Clone, Copy)]
pub struct Keys {
    left: bool,
    right: bool,
    up: bool,
    down: bool,
}

impl Keys {
    /// The per-tick translation delta these held keys produce (right/up are
    /// positive x/y).
    pub fn delta(self) -> Vec3 {
        let x = (self.right as i32 - self.left as i32) as f32;
        let y = (self.up as i32 - self.down as i32) as f32;
        Vec3::new(x * MOVE_SPEED, y * MOVE_SPEED, 0.0)
    }
}

/// Encode a move delta as the netcode command payload: the x and y translation
/// as two little-endian `f32`s.
pub fn encode_delta(delta: Vec3) -> Vec<u8> {
    let mut out = Vec::with_capacity(8);
    out.extend_from_slice(&delta.x.to_le_bytes());
    out.extend_from_slice(&delta.y.to_le_bytes());
    out
}

/// Decode a move payload. A short/garbled payload decodes to no movement.
pub fn decode_delta(payload: &[u8]) -> Vec3 {
    if payload.len() < 8 {
        return Vec3::ZERO;
    }
    let x = f32::from_le_bytes([payload[0], payload[1], payload[2], payload[3]]);
    let y = f32::from_le_bytes([payload[4], payload[5], payload[6], payload[7]]);
    Vec3::new(x, y, 0.0)
}

/// Build the netplay scene: two player cubes (player 0 red on the left, player 1
/// blue on the right), a pulled-back camera, and a directional light. Rendering
/// is enabled. Two peers build identical apps and stay in sync via determinism.
pub fn build_netplay_app() -> RunningApp {
    App::new()
        .window(
            Window::new(800, 600)
                .with_surface_id(CANVAS_ID)
                .with_clear_color(Color::linear_rgb(ch(0.04), ch(0.05), ch(0.08))),
        )
        .add_plugins(DefaultPlugins)
        .setup(|world, meshes, materials| {
            let cube = meshes.add(Mesh::cube());
            let red = materials.add(Material::lit(Color::linear_rgb(
                ch(0.90),
                ch(0.27),
                ch(0.27),
            )));
            let blue = materials.add(Material::lit(Color::linear_rgb(
                ch(0.30),
                ch(0.45),
                ch(0.95),
            )));
            world.spawn((
                Transform::from_translation(Vec3::new(-1.5, 0.0, 0.0)),
                Renderable {
                    mesh: cube,
                    material: red,
                },
                Player::new(0),
            ));
            world.spawn((
                Transform::from_translation(Vec3::new(1.5, 0.0, 0.0)),
                Renderable {
                    mesh: cube,
                    material: blue,
                },
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

/// The neutral bytes a peer fingerprints each tick — the real frame's packed
/// instance floats — so two peers agree iff their engines agree.
pub fn frame_state_bytes(outcome: &FrameOutcome) -> Vec<u8> {
    outcome
        .instance_floats()
        .iter()
        .flat_map(|f| f.to_le_bytes())
        .collect()
}

/// Map a confirmed `(peer, kind, payload)` input into a [`PlayerInput`]: peer
/// `p` drives player index `p - 1`.
pub fn input_for(peer: u64, payload: &[u8]) -> PlayerInput {
    PlayerInput::new((peer.saturating_sub(1)) as u32, decode_delta(payload))
}

#[cfg(target_arch = "wasm32")]
mod web;

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_crypto::SigningKey;
    use axiom_netcode::NetcodeApi;

    /// Peer `id`'s deterministic signing key for the tests.
    fn key(id: u64) -> SigningKey {
        SigningKey::from_seed([id as u8; 32])
    }

    /// The two-peer roster both sessions share.
    fn roster() -> [(u64, axiom_crypto::VerifyingKey); 2] {
        [(1, key(1).verifying_key()), (2, key(2).verifying_key())]
    }

    /// Drive two peers in lockstep over a clean in-process channel. Peer 1
    /// (player 0) plays `m0`; peer 2 (player 1) plays `m1`. Each peer applies
    /// *both* players' confirmed inputs to its own engine. Returns each peer's
    /// per-tick state hash and whether reconcile ever flagged a desync.
    fn run(m0: &[Vec3], m1: &[Vec3]) -> (Vec<[u8; 32]>, Vec<[u8; 32]>, bool) {
        assert_eq!(m0.len(), m1.len());
        let mut app_a = build_netplay_app();
        let mut app_b = build_netplay_app();
        let mut net_a = NetcodeApi::new(1, key(1), &roster());
        let mut net_b = NetcodeApi::new(2, key(2), &roster());
        let (mut ha, mut hb, mut desync) = (Vec::new(), Vec::new(), false);

        for tick in 0..m0.len() {
            let in_a = net_a.submit_local(MOVE_KIND, &encode_delta(m0[tick]));
            let in_b = net_b.submit_local(MOVE_KIND, &encode_delta(m1[tick]));
            net_a.ingest(&in_b).unwrap();
            net_b.ingest(&in_a).unwrap();

            let beacon_a = step(&mut net_a, &mut app_a, &mut ha);
            let beacon_b = step(&mut net_b, &mut app_b, &mut hb);
            net_b.ingest(&beacon_a).unwrap();
            net_a.ingest(&beacon_b).unwrap();

            if net_a.reconcile(tick as u64) == Some(false) {
                desync = true;
            }
        }
        (ha, hb, desync)
    }

    fn step(net: &mut NetcodeApi, app: &mut RunningApp, log: &mut Vec<[u8; 32]>) -> Vec<u8> {
        let tick = net.ready_tick().expect("both peers submitted this tick");
        let inputs: Vec<PlayerInput> = net
            .confirm_tick(tick)
            .iter()
            .map(|(peer, _kind, payload)| input_for(*peer, payload))
            .collect();
        let bytes = frame_state_bytes(&app.tick_with(tick, &inputs));
        log.push(net.digest(&bytes));
        net.record_local_hash(tick, &bytes)
    }

    #[test]
    fn same_inputs_keep_both_browsers_byte_identical() {
        let m0 = [Vec3::new(MOVE_SPEED, 0.0, 0.0); 24];
        let m1 = [Vec3::new(0.0, MOVE_SPEED, 0.0); 24];
        let (ha, hb, desync) = run(&m0, &m1);
        assert_eq!(ha.len(), 24);
        assert_eq!(
            ha, hb,
            "both browsers' real engines agree every confirmed tick"
        );
        assert!(!desync, "matching deterministic engines never desync");
    }

    #[test]
    fn a_players_moves_change_the_frame() {
        // Player 0 moving right makes consecutive frames differ.
        let m0 = [Vec3::new(MOVE_SPEED, 0.0, 0.0); 12];
        let m1 = [Vec3::ZERO; 12];
        let (ha, _, _) = run(&m0, &m1);
        let changed = ha.windows(2).filter(|w| w[0] != w[1]).count();
        assert!(
            changed > 8,
            "moving the player must change the rendered frame"
        );
    }

    #[test]
    fn codec_round_trips_and_keys_map_to_deltas() {
        assert_eq!(
            decode_delta(&encode_delta(Vec3::new(0.5, -0.25, 0.0))),
            Vec3::new(0.5, -0.25, 0.0)
        );
        assert_eq!(decode_delta(&[1, 2, 3]), Vec3::ZERO); // too short
        assert_eq!(
            Keys {
                right: true,
                ..Keys::default()
            }
            .delta(),
            Vec3::new(MOVE_SPEED, 0.0, 0.0)
        );
        assert_eq!(
            Keys {
                up: true,
                left: true,
                ..Keys::default()
            }
            .delta(),
            Vec3::new(-MOVE_SPEED, MOVE_SPEED, 0.0)
        );
        assert_eq!(Keys::default().delta(), Vec3::ZERO);
    }

    #[test]
    fn input_for_maps_peer_to_player_index() {
        // peer 1 -> player 0, peer 2 -> player 1.
        assert_eq!(input_for(1, &encode_delta(Vec3::ZERO)).player, 0);
        assert_eq!(input_for(2, &encode_delta(Vec3::ZERO)).player, 1);
    }

    #[test]
    fn the_scene_draws_two_player_cubes() {
        let mut app = build_netplay_app();
        assert_eq!(app.tick(0).draws().len(), 2);
    }

    #[test]
    fn a_compromised_peer_cannot_forge_the_other_players_input() {
        // Two honest browsers. A third actor (a compromised client / malicious
        // relay) holds NO roster key; it floods peer A every tick with frames
        // claiming to be peer 2, signed by its own key. Peer A must stay
        // byte-identical to peer B — the forged frames never reach the sim.
        let mut app_a = build_netplay_app();
        let mut app_b = build_netplay_app();
        let mut net_a = NetcodeApi::new(1, key(1), &roster());
        let mut net_b = NetcodeApi::new(2, key(2), &roster());
        // The attacker thinks it is peer 2 but holds an off-roster key.
        let attacker_key = SigningKey::from_seed([200u8; 32]);
        let mut attacker = NetcodeApi::new(
            2,
            attacker_key.clone(),
            &[(2, attacker_key.verifying_key())],
        );
        let (mut ha, mut hb) = (Vec::new(), Vec::new());

        for _ in 0..16 {
            let in_a =
                net_a.submit_local(MOVE_KIND, &encode_delta(Vec3::new(MOVE_SPEED, 0.0, 0.0)));
            let in_b = net_b.submit_local(MOVE_KIND, &encode_delta(Vec3::ZERO));
            net_a.ingest(&in_b).unwrap();
            net_b.ingest(&in_a).unwrap();
            // Forged peer-2 input pushed at peer A: decodes fine, fails the
            // signature check against peer 2's real roster key, and is dropped.
            let forged =
                attacker.submit_local(MOVE_KIND, &encode_delta(Vec3::new(0.0, -MOVE_SPEED, 0.0)));
            net_a.ingest(&forged).unwrap();

            step(&mut net_a, &mut app_a, &mut ha);
            step(&mut net_b, &mut app_b, &mut hb);
        }
        assert_eq!(
            ha, hb,
            "forged frames never reached the sim; the two engines stay identical"
        );
    }
}
