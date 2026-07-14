//! Boundary-determinism goldens for the stress-cubes **App-core render boundary**.
//!
//! The path this demo actually renders through is `RunningApp::tick →
//! FrameOutcome` (the mesh-batch / camera-view-proj / light data the live GPU and
//! Canvas2D backends consume). This file pins THAT boundary — the real-pixel
//! render command boundary — as committed golden bytes for the
//! `stress_cubes_core(N)` App-core, carried over unchanged from the merged
//! gallery crate's `gallery_render_determinism.rs` in the gallery de-merge (the
//! golden `.bin` is byte-identical).
//!
//! The slice carries the full golden discipline: a committed golden `.bin`, a
//! positive replay-equal assertion (build twice → byte-equal), and a NEGATIVE
//! assertion (a later animated tick MUST differ), so the golden is not a vacuous
//! `assert_eq!(x, x)`. The golden is SHA-256-pinned in
//! `apps/axiom-stress-cubes/slice.toml` and enforced by `cargo xtask
//! check-slices`.
//!
//! ## Regenerating (the only sanctioned update path)
//!
//! A *missing* golden is captured on the next run (written, test passes); an
//! *existing* golden must match byte-for-byte. To re-capture after an intended
//! render change, delete the affected golden(s) or force a rewrite, then review
//! the diff AND repin the SHA-256 in `slice.toml`:
//!
//! ```text
//! AXIOM_REGOLD=1 cargo test -p axiom-stress-cubes --test render_determinism
//! ```

use std::path::PathBuf;

use axiom::prelude::FrameOutcome;
use axiom_stress_cubes::stress_cubes_core;

// --- canonical FrameOutcome encoder ----------------------------------------
//
// Appends a fixed sequence of little-endian primitives, so the same outcome
// always yields the same bytes. Collections are length-prefixed (a u32 count)
// so a structural change (an extra draw / light) shifts the bytes detectably.
// Only the deterministic scene→render fields are encoded; the backend-state
// flags (`presented`/`recorded`) are not part of the render command boundary.

fn push_f32(out: &mut Vec<u8>, v: f32) {
    out.extend_from_slice(&v.to_le_bytes());
}

fn push_u32(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_le_bytes());
}

fn push_u64(out: &mut Vec<u8>, v: u64) {
    out.extend_from_slice(&v.to_le_bytes());
}

fn push_f32s(out: &mut Vec<u8>, vs: &[f32]) {
    vs.iter().for_each(|&v| push_f32(out, v));
}

fn encode_frame_outcome(f: &FrameOutcome) -> Vec<u8> {
    let mut out = Vec::new();
    push_u64(&mut out, f.tick());
    push_u32(&mut out, f.command_count() as u32);
    push_f32s(&mut out, &f.clear_color());
    push_f32s(&mut out, &f.camera_view_proj());
    push_f32s(&mut out, &f.light_view_proj());
    // Draws, in submission order (deterministic scene order).
    push_u32(&mut out, f.draws().len() as u32);
    f.draws().iter().for_each(|d| {
        push_f32s(&mut out, &d.mvp());
        push_f32s(&mut out, &d.world());
        push_f32s(&mut out, &d.color());
        push_u64(&mut out, d.mesh_id());
        push_u64(&mut out, d.material_id());
        out.push(u8::from(d.casts_contact_shadow()));
    });
    // Lights, in scene order.
    push_u32(&mut out, f.lights().len() as u32);
    f.lights().iter().for_each(|l| {
        push_u32(&mut out, l.kind());
        push_f32s(&mut out, &l.vec());
        push_f32s(&mut out, &l.color());
        push_f32(&mut out, l.intensity());
    });
    out
}

// --- golden machinery (mirrors the rotating-cube demo's golden_artifacts.rs) -

fn golden_path(name: &str) -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.push("tests");
    p.push("golden");
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
            "golden mismatch for `{name}` ({} bytes actual vs {} bytes golden): the stress-cubes \
             render boundary drifted. If intended, re-capture (delete this golden or set \
             AXIOM_REGOLD=1) and repin its SHA-256 in apps/axiom-stress-cubes/slice.toml.",
            actual.len(),
            expected.len(),
        ),
        _ => {
            std::fs::create_dir_all(path.parent().unwrap()).expect("create golden dir");
            std::fs::write(&path, actual).expect("write golden");
        }
    }
}

// --- stress-cubes App-core render boundary ----------------------------------

/// A small fixed cube count keeps the golden compact while still exercising the
/// per-cube batching + animation the stress scene is built around.
const STRESS_CUBES: u32 = 16;

/// Drive `stress_cubes_core(STRESS_CUBES)` from tick 0 through `last` (the tick
/// sequence must be monotonic) and capture the render boundary of the final
/// frame.
fn stress_cubes_render(last: u64) -> Vec<u8> {
    let mut app = stress_cubes_core(STRESS_CUBES);
    let mut frame = app.tick(0);
    (1..=last).for_each(|t| frame = app.tick(t));
    encode_frame_outcome(&frame)
}

#[test]
fn golden_stress_cubes_render_tick0() {
    assert_golden("stress_cubes_render_tick0", &stress_cubes_render(0));
}

#[test]
fn stress_cubes_render_replays_byte_equal() {
    assert_eq!(stress_cubes_render(0), stress_cubes_render(0));
}

#[test]
fn stress_cubes_render_differs_across_animated_ticks() {
    // NEGATIVE: each cube spins on its own period, so a later tick must differ.
    assert_ne!(stress_cubes_render(0), stress_cubes_render(90));
}
