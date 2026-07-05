//! Stored golden capture for the retro FPS **render boundary**.
//!
//! `retro_fps_golden_state.rs` pins the deterministic game *state* bytes across
//! commits; this file pins the deterministic *render* bytes — the per-frame
//! `FrameOutcome` the live GPU / Canvas2D backends consume (the mesh-batch
//! draws, camera view-projection, and resolved lights). Before Stream B gave the
//! game a real render path, the replay-determinism test recorded the render
//! artifact as EMPTY (`Vec::new()`); now the render boundary is a first-class,
//! byte-pinned golden captured natively and headlessly — no browser needed,
//! since `RunningApp::tick_with_controls` returns the deterministic `FrameOutcome`
//! directly.
//!
//! The scenario is the exact one the replay-determinism + state golden tests use,
//! so the render golden lines up frame-for-frame with them. A *missing* golden is
//! captured on the next run; an *existing* golden must match. To re-capture after
//! an intended render change, delete the golden (or `AXIOM_REGOLD=1`), review the
//! diff, AND repin its SHA-256 in `games/retro-fps/slice.toml`.

use std::path::PathBuf;

use axiom::prelude::FrameOutcome;
use axiom_game_retro_fps::level::LevelDoc;
use axiom_game_retro_fps::{apply_lifecycle, build_retro_fps_app, Intent, RetroFpsGame};

/// The same fixed scenario the replay-determinism and state-golden tests use:
/// one held-input intent per tick. Fixing these fixes the whole run.
fn scenario() -> Vec<Intent> {
    let forward = Intent {
        forward: true,
        ..Default::default()
    };
    let turn = Intent {
        turn_left: true,
        ..Default::default()
    };
    let fire = Intent {
        fire: true,
        ..Default::default()
    };
    let strafe_fire = Intent {
        strafe_right: true,
        fire: true,
        ..Default::default()
    };
    vec![
        Intent::default(),
        forward,
        forward,
        turn,
        fire,
        forward,
        strafe_fire,
        turn,
    ]
}

/// Canonical render bytes for one frame: the deterministic scene→render boundary
/// fields, little-endian, length-prefixed collections. Backend-state flags
/// (`presented`/`recorded`) are excluded — they are not part of the render
/// command boundary the goldens pin.
fn encode_frame(f: &FrameOutcome) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&f.tick().to_le_bytes());
    out.extend_from_slice(&(f.command_count() as u32).to_le_bytes());
    f.clear_color().iter().for_each(|v| out.extend_from_slice(&v.to_le_bytes()));
    f.camera_view_proj().iter().for_each(|v| out.extend_from_slice(&v.to_le_bytes()));
    f.light_view_proj().iter().for_each(|v| out.extend_from_slice(&v.to_le_bytes()));
    out.extend_from_slice(&(f.draws().len() as u32).to_le_bytes());
    f.draws().iter().for_each(|d| {
        d.mvp().iter().for_each(|v| out.extend_from_slice(&v.to_le_bytes()));
        d.world().iter().for_each(|v| out.extend_from_slice(&v.to_le_bytes()));
        d.color().iter().for_each(|v| out.extend_from_slice(&v.to_le_bytes()));
        out.extend_from_slice(&d.mesh_id().to_le_bytes());
        out.extend_from_slice(&d.material_id().to_le_bytes());
        out.push(u8::from(d.casts_contact_shadow()));
    });
    out.extend_from_slice(&(f.lights().len() as u32).to_le_bytes());
    f.lights().iter().for_each(|l| {
        out.extend_from_slice(&l.kind().to_le_bytes());
        l.vec().iter().for_each(|v| out.extend_from_slice(&v.to_le_bytes()));
        l.color().iter().for_each(|v| out.extend_from_slice(&v.to_le_bytes()));
        out.extend_from_slice(&l.intensity().to_le_bytes());
    });
    out
}

fn golden_path(name: &str) -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.push("tests");
    p.push("retro_fps/golden");
    p.push(format!("{name}.bin"));
    p
}

fn assert_golden(name: &str, actual: &[u8]) {
    let path = golden_path(name);
    let force = std::env::var_os("AXIOM_REGOLD").is_some();
    match std::fs::read(&path).ok() {
        Some(expected) if !force => assert_eq!(
            actual,
            expected.as_slice(),
            "golden mismatch for `{name}` ({} vs {} bytes): retro FPS render output drifted. \
             If intended, re-capture (delete the golden or set AXIOM_REGOLD=1) and repin its \
             SHA-256 in games/retro-fps/slice.toml.",
            actual.len(),
            expected.len(),
        ),
        _ => {
            std::fs::create_dir_all(path.parent().unwrap()).expect("create golden dir");
            std::fs::write(&path, actual).expect("write golden");
        }
    }
}

/// Drive the fixed scenario, concatenating each frame's render bytes (length-
/// prefixed so frame boundaries are recoverable). `force_first_forward` perturbs
/// the run by force-moving forward on frame 0 only — a genuine render-affecting
/// change (the camera and every draw's MVP move), used to prove the golden is
/// sensitive to a real input change rather than a constant. (An extra *fire*
/// would only touch the HUD/ammo, which the render boundary does not carry.)
fn render_sequence(force_first_forward: bool) -> Vec<u8> {
    let doc = LevelDoc::default();
    let mut game = RetroFpsGame::from_level(&doc);
    let (mut app, assets) = build_retro_fps_app(&doc);
    game.bind_entities(&app);
    let mut bytes = Vec::new();
    scenario().into_iter().enumerate().for_each(|(i, intent)| {
        let mut intent = intent;
        intent.forward = intent.forward || (force_first_forward && i == 0);
        let commands = game.step(intent, &app);
        apply_lifecycle(&mut game, &mut app, &assets, &commands);
        let frame = app.tick_with_controls(i as u64, &commands.enemies, &[commands.control]);
        let encoded = encode_frame(&frame);
        bytes.extend_from_slice(&(encoded.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&encoded);
    });
    bytes
}

#[test]
fn golden_retro_fps_render_sequence() {
    assert_golden("retro_fps_render_sequence", &render_sequence(false));
}

#[test]
fn render_capture_is_stable() {
    // The render capture is a pure function of the fixed scenario.
    assert_eq!(render_sequence(false), render_sequence(false));
}

#[test]
fn a_perturbed_scenario_yields_different_render_bytes() {
    // NEGATIVE: force-move forward on frame 0 only — the player (and camera) move,
    // so every draw's MVP and the camera view-projection shift, proving the golden
    // is sensitive to a genuine input change, not a constant.
    assert_ne!(render_sequence(false), render_sequence(true));
}
