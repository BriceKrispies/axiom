//! Measure the cost of the streaming terrain regeneration, to diagnose the
//! edge-of-zone FPS stutter. The browser regenerates the whole terrain window
//! synchronously inside one frame when the player crosses the recenter
//! threshold; this times that exact work (the per-vertex `sample_height_m`
//! field evaluation) natively so the spike is observable as milliseconds.
//!
//! Run: cargo run -p axiom-growth --example bench_stream --release

use std::time::Instant;

use axiom_growth::gameworld::sample_height_m;
use axiom_growth::model_world::GameWorldLocalMap;
use axiom_growth::presets::PlanetPreset;
use axiom_growth::Growth;

/// Time sampling an `sx × sz` grid of terrain heights at 1 m spacing, centred
/// at (cx,cz). Returns (best_ms_over_runs, sample_count).
fn time_grid(
    g: &Growth,
    lm: &GameWorldLocalMap,
    seed: u64,
    center: (f32, f32),
    sx: usize,
    sz: usize,
    runs: usize,
) -> (f64, usize) {
    let (cx, cz) = center;
    let halfx = (sx as f32 - 1.0) * 0.5;
    let halfz = (sz as f32 - 1.0) * 0.5;
    let mut best = f64::MAX;
    for r in 0..runs {
        let off = r as f32 * 16.0; // shift so we don't hit an identical cache pattern
        let t0 = Instant::now();
        let mut acc = 0.0f32;
        for gz in 0..sz {
            for gx in 0..sx {
                let x = cx + off - halfx + gx as f32;
                let z = cz - halfz + gz as f32;
                acc += sample_height_m(&g.atlas, lm, seed, x, z);
            }
        }
        let ms = t0.elapsed().as_secs_f64() * 1000.0;
        std::hint::black_box(acc);
        best = best.min(ms);
    }
    (best, sx * sz)
}

fn report(name: &str, ms: f64, n: usize) {
    let frames_at_60 = ms / 16.67;
    println!(
        "{name:<42} {ms:>8.2} ms   {n:>7} samples   {:>6.0} ns/sample   ~{frames_at_60:>5.1} dropped frames @60fps",
        ms * 1e6 / n as f64
    );
}

fn main() {
    // The browser viewer's default world + the anchored local frame.
    let g = Growth::generate("growth-browser-viewer", PlanetPreset::Earthlike, 16384);
    let lm = GameWorldLocalMap::anchored(&g.atlas);
    let seed = g.seed.value;

    // warm caches / branch predictor
    let _ = time_grid(&g, &lm, seed, (0.0, 0.0), 64, 64, 1);

    println!("\n=== Streaming terrain regeneration cost (sample_height_m field) ===");
    println!("(AREA_HALF_M=160 → a 321×321 vertex window; RECENTER_THRESHOLD≈80 m)\n");

    // The CURRENT behaviour: regenerate the entire window in one frame.
    let (ms_full, n_full) = time_grid(&g, &lm, seed, (0.0, 0.0), 321, 321, 5);
    report(
        "FULL window regen (current, per edge cross)",
        ms_full,
        n_full,
    );

    // What an incremental/chunked approach would do instead: only the new
    // leading strip exposed by sliding the window one 16 m chunk.
    let (ms_strip, n_strip) = time_grid(&g, &lm, seed, (0.0, 0.0), 17, 321, 5);
    report(
        "leading STRIP only (1 chunk, incremental)",
        ms_strip,
        n_strip,
    );

    // A single chunk's worth (what a per-chunk streamer regenerates per step).
    let (ms_chunk, n_chunk) = time_grid(&g, &lm, seed, (0.0, 0.0), 17, 17, 8);
    report("one 16 m CHUNK (per-chunk streamer)", ms_chunk, n_chunk);

    println!("\nnote: build_terrain also computes per-vertex normals (≈ same number");
    println!("of extra height reads) and then the GPU buffers are reallocated, so the");
    println!("real frame cost is roughly 1.5–2× the FULL number above plus upload.\n");
    println!(
        "ratio full/strip = {:.1}× more work than an incremental slide",
        ms_full / ms_strip.max(1e-6)
    );
}
