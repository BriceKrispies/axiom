//! Backend-neutral **volumetric light** for a frame: screen-space "crepuscular
//! rays" (god-rays) expressed as neutral frame data and realized by a single pure
//! post-process that operates on a finished RGBA framebuffer.
//!
//! This lives in `host` — not in any one backend — precisely because the engine's
//! contract is *neutral frame in, pixels out, through **any** renderer*. A
//! [`FramePacket`] carries an optional [`FrameVolumetrics`]; every backend
//! (Canvas 2D software raster, WebGPU/WebGL via wgpu, …) calls
//! [`apply_frame_volumetrics`] on its output, so the shafts appear identically no
//! matter which renderer produced the frame. The effect is a screen-space gather —
//! for each pixel, march toward the sun's projected screen position accumulating a
//! luminance *snapshot* with exponential decay, then add the sun-tinted result.
//! Deterministic, no feedback, no browser types.

use crate::frame_packet::FramePacket;

/// Tuning for the volumetric light-scatter pass, carried as neutral frame data.
/// Presence of a `FrameVolumetrics` on a [`FramePacket`] *is* the enable.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FrameVolumetrics {
    samples: u32,
    density: f32,
    decay: f32,
    weight: f32,
    exposure: f32,
    threshold: f32,
    color: [f32; 3],
}

impl FrameVolumetrics {
    /// Assemble volumetric-light parameters. `samples` steps are marched toward the
    /// sun; `density` scales the step; `decay`/`weight` shape the per-sample
    /// contribution; `exposure` scales the added shaft; only luminance above
    /// `threshold` "leaks"; `color` is the linear-RGB sun tint added. Crate-internal:
    /// the public constructor is [`FrameVolumetrics::low_poly`] (a preset), so no
    /// naked tuning scalar crosses the module facade.
    pub(crate) const fn new(
        samples: u32,
        density: f32,
        decay: f32,
        weight: f32,
        exposure: f32,
        threshold: f32,
        color: [f32; 3],
    ) -> Self {
        FrameVolumetrics { samples, density, decay, weight, exposure, threshold, color }
    }

    /// The public constructor: a warm low-poly god-ray preset. Presets keep the raw
    /// tuning scalars off the public surface.
    pub const fn low_poly() -> Self {
        // Visible warm sunbeams: a lower threshold lets more of the bright backlit sky
        // leak, at a higher weight/exposure, so shafts stream through the trunks the way
        // the reference's misty backlight does — without fully blowing the sky gaps.
        FrameVolumetrics::new(48, 0.9, 0.94, 0.09, 0.8, 0.62, [1.0, 0.9, 0.68])
    }
}

/// Relative luminance (Rec. 709) of an RGBA8 pixel, in `0..=1`.
fn luminance(p: &[u8]) -> f32 {
    (0.2126 * f32::from(p[0]) + 0.7152 * f32::from(p[1]) + 0.0722 * f32::from(p[2])) / 255.0
}

/// Clamp a (possibly off-screen / non-finite) axis coordinate into `0..dim`.
fn clamp_axis(value: f32, dim: u32) -> u32 {
    (value as i64).clamp(0, dim as i64 - 1) as u32
}

/// The luminance-snapshot value at a clamped sample position.
fn sample_lum(lum: &[f32], w: u32, h: u32, fx: f32, fy: f32) -> f32 {
    let x = clamp_axis(fx, w);
    let y = clamp_axis(fy, h);
    lum[y as usize * w as usize + x as usize]
}

/// Accumulated shaft brightness added at pixel `(x, y)`.
fn shaft_at(lum: &[f32], w: u32, h: u32, x: u32, y: u32, sun: [f32; 2], v: &FrameVolumetrics) -> f32 {
    let px = x as f32 + 0.5;
    let py = y as f32 + 0.5;
    let steps = v.samples.max(1) as f32;
    let dx = (sun[0] - px) / steps * v.density;
    let dy = (sun[1] - py) / steps * v.density;
    let (sum, _) = (1..=v.samples).fold((0.0f32, 1.0f32), |(sum, decay), i| {
        let l = sample_lum(lum, w, h, px + dx * i as f32, py + dy * i as f32);
        let contrib = (l - v.threshold).max(0.0) * decay * v.weight;
        (sum + contrib, decay * v.decay)
    });
    sum * v.exposure
}

/// Add `add` (linear) to one byte channel, clamped + re-quantized.
fn add_byte(cur: u8, add: f32) -> u8 {
    ((f32::from(cur) / 255.0 + add).clamp(0.0, 1.0) * 255.0 + 0.5) as u8
}

/// Project a directional light's **to-light** direction to a screen-space sun
/// position through the column-major `view_proj`. `Some([x, y])` in device pixels
/// when the sun is in front of the camera (clip `w > 0`); `None` when behind.
/// Crate-internal (raw matrix/vector), reached only through `apply_frame_volumetrics`.
pub(crate) fn project_sun_screen(
    view_proj: &[f32; 16],
    to_light: [f32; 3],
    w: u32,
    h: u32,
) -> Option<[f32; 2]> {
    let m = view_proj;
    let (dx, dy, dz) = (to_light[0], to_light[1], to_light[2]);
    let cx = m[0] * dx + m[4] * dy + m[8] * dz;
    let cy = m[1] * dx + m[5] * dy + m[9] * dz;
    let cw = m[3] * dx + m[7] * dy + m[11] * dz;
    (cw > 0.0).then(|| {
        [(cx / cw * 0.5 + 0.5) * w as f32, (1.0 - (cy / cw * 0.5 + 0.5)) * h as f32]
    })
}

/// The frame's directional-sun screen position: project the first directional
/// light's to-light vector through the camera. `None` without a directional light,
/// a camera, or when the sun is behind the camera.
fn sun_screen(packet: &FramePacket, w: u32, h: u32) -> Option<[f32; 2]> {
    packet
        .lights()
        .iter()
        .find(|l| l.kind() == 0)
        .map(|l| l.vec())
        .zip(packet.camera().map(|c| c.view_proj()))
        .and_then(|(dir, vp)| project_sun_screen(&vp, dir, w, h))
}

/// Apply the frame's volumetric light-scatter to a finished RGBA8 framebuffer, in
/// place. A no-op (returns `0`) when the packet carries no [`FrameVolumetrics`], or
/// when the sun is not on-screen. Returns the count of pixels the shafts brightened.
///
/// **Every backend calls this on its output**, so god-rays render identically on
/// Canvas 2D, WebGPU, and WebGL — the effect is neutral frame data, not a
/// backend-specific feature.
pub fn apply_frame_volumetrics(rgba: &mut [u8], w: u32, h: u32, packet: &FramePacket) -> u64 {
    packet
        .volumetrics()
        .copied()
        .zip(sun_screen(packet, w, h))
        .map(|(v, sun)| {
            let lum: Vec<f32> = rgba.chunks_exact(4).map(luminance).collect();
            (0..h).fold(0_u64, |acc, y| {
                (0..w).fold(acc, |acc, x| {
                    let add = shaft_at(&lum, w, h, x, y, sun, &v);
                    let off = (y as usize * w as usize + x as usize) * 4;
                    rgba[off] = add_byte(rgba[off], v.color[0] * add);
                    rgba[off + 1] = add_byte(rgba[off + 1], v.color[1] * add);
                    rgba[off + 2] = add_byte(rgba[off + 2], v.color[2] * add);
                    acc + u64::from(add > 0.0)
                })
            })
        })
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame_packet::{FrameCamera, FrameLight, FrameViewport, FrameFeatureSet};

    /// `m[11] = 1` toy view_proj: a `+z` to-light projects in front at screen centre.
    const FRONT_VP: [f32; 16] =
        [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 0.0, 0.0, 0.0, 1.0];

    fn packet(vol: Option<FrameVolumetrics>, lights: Vec<FrameLight>, cam: Option<FrameCamera>) -> FramePacket {
        let mut p = FramePacket::new(
            0,
            0,
            FrameViewport::new(16, 16),
            [0.0, 0.0, 0.0, 1.0],
            cam,
            Vec::new(),
            lights,
            [0.0; 16],
            FrameFeatureSet::new(false, false, 0, 0),
        );
        p = vol.map(|v| p.clone().with_volumetrics(v)).unwrap_or(p);
        p
    }

    fn dir_light() -> Vec<FrameLight> {
        vec![FrameLight::new(0, [0.0, 0.0, 1.0], [1.0, 1.0, 1.0, 1.0])]
    }

    #[test]
    fn params_and_luminance() {
        let v = FrameVolumetrics::low_poly();
        assert_eq!(v.color, [1.0, 0.9, 0.68]);
        assert_eq!(FrameVolumetrics::new(1, 0.0, 0.0, 0.0, 0.0, 1.0, [0.0; 3]).samples, 1);
        assert!(luminance(&[0, 0, 0, 255]).abs() < 1e-6);
        assert!((luminance(&[255, 255, 255, 255]) - 1.0).abs() < 1e-4);
    }

    #[test]
    fn project_front_center_and_behind_none() {
        let s = project_sun_screen(&FRONT_VP, [0.0, 0.0, 1.0], 100, 80).unwrap();
        assert!((s[0] - 50.0).abs() < 1e-3 && (s[1] - 40.0).abs() < 1e-3);
        assert!(project_sun_screen(&FRONT_VP, [0.0, 0.0, -1.0], 100, 80).is_none());
    }

    #[test]
    fn no_volumetrics_or_no_sun_is_a_no_op() {
        let cam = Some(FrameCamera::new([0.0; 16], [0.0; 16], FRONT_VP));
        let mut rgba = vec![0u8; 16 * 16 * 4];
        // No volumetrics on the packet → 0.
        assert_eq!(apply_frame_volumetrics(&mut rgba, 16, 16, &packet(None, dir_light(), cam)), 0);
        // Volumetrics but no directional light → 0.
        let v = Some(FrameVolumetrics::low_poly());
        assert_eq!(apply_frame_volumetrics(&mut rgba, 16, 16, &packet(v, Vec::new(), cam)), 0);
        // Volumetrics but no camera → 0.
        assert_eq!(apply_frame_volumetrics(&mut rgba, 16, 16, &packet(v, dir_light(), None)), 0);
        // Volumetrics + light but sun behind the camera → 0.
        let behind = vec![FrameLight::new(0, [0.0, 0.0, -1.0], [1.0; 4])];
        assert_eq!(apply_frame_volumetrics(&mut rgba, 16, 16, &packet(v, behind, cam)), 0);
    }

    #[test]
    fn shafts_brighten_toward_an_on_screen_sun() {
        // Sun projects to screen centre (8,8); a bright pixel between a corner pixel
        // and the sun makes that corner pixel's ray brighten.
        let cam = Some(FrameCamera::new([0.0; 16], [0.0; 16], FRONT_VP));
        let mut rgba = vec![0u8; 16 * 16 * 4];
        rgba[3] = 255; // pixel (0,0) alpha
        // bright pixel at (6,6): index (6*16+6)*4
        let bi = (6 * 16 + 6) * 4;
        rgba[bi] = 255;
        rgba[bi + 1] = 255;
        rgba[bi + 2] = 255;
        let before = rgba.clone();
        let touched =
            apply_frame_volumetrics(&mut rgba, 16, 16, &packet(Some(FrameVolumetrics::low_poly()), dir_light(), cam));
        assert!(touched > 0);
        // Pixel (3,3) is on the far side of the bright spot (6,6) from the sun (8,8),
        // so its ray to the sun passes through the bright spot and it brightens.
        let pi = (3 * 16 + 3) * 4;
        assert!(rgba[pi] > before[pi]);
    }

    #[test]
    fn threshold_and_zero_samples_produce_nothing() {
        let cam = Some(FrameCamera::new([0.0; 16], [0.0; 16], FRONT_VP));
        let bi = (6 * 16 + 6) * 4;
        // Mid-grey source below a high threshold → no leak.
        let mut rgba = vec![0u8; 16 * 16 * 4];
        rgba[bi] = 128;
        rgba[bi + 1] = 128;
        rgba[bi + 2] = 128;
        let high = FrameVolumetrics::new(48, 0.9, 0.94, 0.055, 0.75, 0.9, [1.0, 1.0, 1.0]);
        assert_eq!(apply_frame_volumetrics(&mut rgba, 16, 16, &packet(Some(high), dir_light(), cam)), 0);
        // Zero samples → no march (also exercises off-screen clamp via the corner sun).
        let zero = FrameVolumetrics::new(0, 0.9, 0.94, 0.055, 0.75, 0.0, [1.0, 1.0, 1.0]);
        let mut full = vec![200u8; 16 * 16 * 4];
        assert_eq!(apply_frame_volumetrics(&mut full, 16, 16, &packet(Some(zero), dir_light(), cam)), 0);
    }
}
