//! Shared helpers for the `axiom-shot` backend-parity proofs (SPEC-04 §7 2D
//! alpha-blend parity, SPEC-11 §7 GPU↔canvas2d 3D parity).
//!
//! The backend glue (host presentation request, GPU/canvas2d render, neutral
//! `FramePacket` reconstruction) now lives ONCE in `axiom_shot::capture`; these
//! helpers re-export it so a test feeds the SAME neutral frame data both
//! backends receive from the binary. Only the pixel-comparison *metrics* are
//! test-local.

// The re-exports and `FrameOutcome` are consumed only by the `offscreen`-gated
// GPU parity tests; in the default build they are legitimately unused.
#![allow(dead_code, unused_imports)]

pub use axiom_shot::capture::{present_request, render_canvas2d};

use axiom::prelude::FrameOutcome;

/// Render a ticked frame through the native off-screen GPU path (the same
/// `scene_renderer` the browser arm runs) at `w`×`h`. `offscreen` feature only.
#[cfg(feature = "offscreen")]
pub fn render_gpu(
    meshes: &[(u64, Vec<f32>, Vec<u32>)],
    materials: &[(u64, u32, u32, Vec<u8>)],
    outcome: &FrameOutcome,
    w: u32,
    h: u32,
) -> (Vec<u8>, u32, u32) {
    axiom_shot::capture::render_gpu(meshes, materials, outcome, w, h, None)
}

/// The maximum per-channel absolute byte difference between two equal-length RGBA8
/// buffers — the tight 2D parity metric.
pub fn max_channel_diff(a: &[u8], b: &[u8]) -> u8 {
    a.iter()
        .zip(b.iter())
        .map(|(x, y)| x.abs_diff(*y))
        .max()
        .unwrap_or(0)
}

/// The mean per-channel absolute byte difference (a secondary 2D parity metric).
pub fn mean_channel_diff(a: &[u8], b: &[u8]) -> f64 {
    let sum: u64 = a
        .iter()
        .zip(b.iter())
        .map(|(x, y)| u64::from(x.abs_diff(*y)))
        .sum();
    sum as f64 / a.len().max(1) as f64
}

/// Coarse "where are the rendered objects" stats for one RGBA8 image: the
/// normalised `(centroid_x, centroid_y)` in `[0,1]` and the coverage fraction of
/// pixels that differ from the background (top-left corner) by more than
/// `threshold` on any channel. The resolution-independent 3D parity metric.
pub fn region_stats(px: &[u8], w: u32, h: u32, threshold: u8) -> (f64, f64, f64) {
    let bg = [px[0], px[1], px[2]];
    let mut count = 0u64;
    let mut sx = 0f64;
    let mut sy = 0f64;
    (0..h).for_each(|y| {
        (0..w).for_each(|x| {
            let i = ((y * w + x) * 4) as usize;
            let d = px[i]
                .abs_diff(bg[0])
                .max(px[i + 1].abs_diff(bg[1]))
                .max(px[i + 2].abs_diff(bg[2]));
            (d > threshold).then(|| {
                count += 1;
                sx += f64::from(x);
                sy += f64::from(y);
            });
        })
    });
    let denom = count.max(1) as f64;
    (
        sx / denom / f64::from(w),
        sy / denom / f64::from(h),
        count as f64 / f64::from(w * h),
    )
}

/// The coverage fraction of object pixels in the left and right halves of an
/// image. Used to assert both backends place an object in each half.
pub fn half_coverage(px: &[u8], w: u32, h: u32, threshold: u8) -> (f64, f64) {
    let bg = [px[0], px[1], px[2]];
    let mid = w / 2;
    let mut left = 0u64;
    let mut right = 0u64;
    for y in 0..h {
        for x in 0..w {
            let i = ((y * w + x) * 4) as usize;
            let d = px[i]
                .abs_diff(bg[0])
                .max(px[i + 1].abs_diff(bg[1]))
                .max(px[i + 2].abs_diff(bg[2]));
            if d > threshold {
                if x < mid {
                    left += 1;
                } else {
                    right += 1;
                }
            }
        }
    }
    let half = f64::from((w / 2) * h);
    (left as f64 / half, right as f64 / half)
}
