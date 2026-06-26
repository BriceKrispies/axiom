//! The Canvas depth-cue **post-passes**: cheap, deterministic per-pixel and
//! per-object compositing applied to the finished z-buffer image — depth fog,
//! the camera-relative vertical colour grade, contact-shadow blobs, and
//! depth-weighted object outlines.
//!
//! Each is one pass over the framebuffer (fog/grade) or per-overlay
//! (shadows/outlines), with no per-pixel allocation. The per-*triangle* cues
//! (lighting, height tint, falloff) are baked into each triangle's flat colour
//! during conversion; these are the cues that operate on the composited frame.
//! Pure Rust — no browser types.

use crate::canvas_depth_cue::{fog_mix, mix, to_byte, vertical_grade_mix};
use crate::canvas_depth_cue_profile::CanvasDepthCueProfile;
use crate::depth_buffer::DepthBuffer;
use crate::frame_packet_raster::DrawOverlay;
use crate::software_framebuffer::SoftwareFramebuffer;

/// Clamp a (possibly off-screen or non-finite) axis coordinate to `0..dim`.
pub(crate) fn clamp_axis(value: f32, dim: u32) -> u32 {
    (value as i64).clamp(0, dim as i64 - 1) as u32
}

/// Blend one byte channel toward a linear `target` by `t` (0 = keep, 1 = target),
/// clamped + re-quantized. Alpha is never passed here, so it is preserved.
fn blend_byte(cur: u8, target: f32, t: f32) -> u8 {
    to_byte((cur as f32 / 255.0) * (1.0 - t) + target * t)
}

/// Blend an RGB pixel toward a linear `target` colour by `t`; returns `1`
/// (pixels touched). Out-of-range offsets are ignored (returns `0`).
fn blend_pixel(rgba: &mut [u8], w: u32, x: u32, y: u32, target: f32, t: f32) -> u64 {
    let off = (y as usize * w as usize + x as usize) * 4;
    rgba.get_mut(off..off + 3)
        .map(|p| {
            p[0] = blend_byte(p[0], target, t);
            p[1] = blend_byte(p[1], target, t);
            p[2] = blend_byte(p[2], target, t);
            1
        })
        .unwrap_or(0)
}

/// Depth-fog post-pass: mix every pixel toward the profile's fog colour by its
/// final depth (`fog_mix` includes `fog.strength` + a safe range). Returns the
/// count of pixels the fog actually touched.
pub(crate) fn apply_fog(
    fb: &mut SoftwareFramebuffer,
    depth: &DepthBuffer,
    cues: &CanvasDepthCueProfile,
) -> u64 {
    let (w, h) = (fb.width(), fb.height());
    let fog = cues.fog.color;
    let rgba = fb.rgba_mut();
    (0..h).fold(0_u64, |acc, y| {
        (0..w).fold(acc, |acc, x| {
            let f = fog_mix(depth.depth_at(x, y), cues);
            let off = (y as usize * w as usize + x as usize) * 4;
            rgba[off] = blend_byte(rgba[off], fog[0], f);
            rgba[off + 1] = blend_byte(rgba[off + 1], fog[1], f);
            rgba[off + 2] = blend_byte(rgba[off + 2], fog[2], f);
            acc + u64::from(f > 0.0)
        })
    })
}

/// Vertical colour-grade post-pass: darken each pixel toward black by its screen
/// `y` (`vertical_grade_mix` = `(y/h)·strength`) — a faint lower-screen anchor.
/// Returns the count of pixels touched (every row below the top).
pub(crate) fn apply_vertical_grade(
    fb: &mut SoftwareFramebuffer,
    cues: &CanvasDepthCueProfile,
) -> u64 {
    let (w, h) = (fb.width(), fb.height());
    let rgba = fb.rgba_mut();
    (0..h).fold(0_u64, |acc, y| {
        let t = vertical_grade_mix(y, h, cues);
        (0..w).fold(acc, |acc, x| {
            let off = (y as usize * w as usize + x as usize) * 4;
            rgba[off] = blend_byte(rgba[off], 0.0, t);
            rgba[off + 1] = blend_byte(rgba[off + 1], 0.0, t);
            rgba[off + 2] = blend_byte(rgba[off + 2], 0.0, t);
            acc + u64::from(t > 0.0)
        })
    })
}

/// Outline post-pass: stroke each important object's screen bounding box with a
/// depth-weighted dark border (near objects stronger than far). Bounds-based,
/// not image-wide edge detection. Returns `(objects outlined, pixels written)`.
pub(crate) fn apply_outlines(
    fb: &mut SoftwareFramebuffer,
    overlays: &[DrawOverlay],
    cues: &CanvasDepthCueProfile,
) -> (u32, u64) {
    let (w, h) = (fb.width(), fb.height());
    let rgba = fb.rgba_mut();
    overlays.iter().fold((0_u32, 0_u64), |(count, pixels), o| {
        let t = outline_alpha(o.mean_depth, cues);
        let [minx, miny, maxx, maxy] = o.bbox;
        let x0 = clamp_axis(minx, w);
        let x1 = clamp_axis(maxx, w);
        let y0 = clamp_axis(miny, h);
        let y1 = clamp_axis(maxy, h);
        let horiz = (x0..x1 + 1).fold(0_u64, |acc, px| {
            acc + blend_pixel(rgba, w, px, y0, 0.0, t) + blend_pixel(rgba, w, px, y1, 0.0, t)
        });
        let vert = (y0..y1 + 1).fold(0_u64, |acc, py| {
            acc + blend_pixel(rgba, w, x0, py, 0.0, t) + blend_pixel(rgba, w, x1, py, 0.0, t)
        });
        (count + 1, pixels + horiz + vert)
    })
}

/// Outline alpha by object depth: `near_outline_alpha` near (depth 0),
/// `far_outline_alpha` far (depth 1), clamped.
pub(crate) fn outline_alpha(depth: f32, cues: &CanvasDepthCueProfile) -> f32 {
    let t = depth.clamp(0.0, 1.0);
    mix(cues.near_outline_alpha, cues.far_outline_alpha, t).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::canvas_depth_cue_profile::CanvasDepthCueProfile;

    fn cues() -> CanvasDepthCueProfile {
        CanvasDepthCueProfile::low_poly_framebuffer()
    }

    fn px(bytes: &[u8], w: u32, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * w + x) * 4) as usize;
        [bytes[i], bytes[i + 1], bytes[i + 2], bytes[i + 3]]
    }

    #[test]
    fn clamp_axis_clamps_into_range() {
        assert_eq!(clamp_axis(-5.0, 10), 0);
        assert_eq!(clamp_axis(3.4, 10), 3);
        assert_eq!(clamp_axis(99.0, 10), 9);
        assert_eq!(clamp_axis(f32::INFINITY, 10), 9);
    }

    #[test]
    fn fog_leaves_near_unchanged_pushes_far_to_fog_and_counts() {
        let mut fb = SoftwareFramebuffer::new(2, 1);
        let mut depth = DepthBuffer::new(2, 1);
        fb.set_pixel(0, 0, [1.0, 0.0, 0.0, 1.0]);
        fb.set_pixel(1, 0, [1.0, 0.0, 0.0, 1.0]);
        depth.slice_mut()[0] = 0.0;
        depth.slice_mut()[1] = 1.0;
        let mut c = cues();
        c.fog.near = 0.0;
        c.fog.far = 1.0;
        c.fog.strength = 1.0;
        c.fog.color = [0.0, 0.0, 1.0, 1.0];
        let touched = apply_fog(&mut fb, &depth, &c);
        let bytes = fb.into_rgba_bytes();
        assert_eq!(px(&bytes, 2, 0, 0), [255, 0, 0, 255], "near unchanged");
        assert_eq!(px(&bytes, 2, 1, 0), [0, 0, 255, 255], "far fully fogged");
        assert_eq!(touched, 1);
    }

    #[test]
    fn vertical_grade_darkens_lower_rows() {
        let mut fb = SoftwareFramebuffer::new(1, 4);
        (0..4).for_each(|y| fb.set_pixel(0, y, [1.0, 1.0, 1.0, 1.0]));
        let touched = apply_vertical_grade(&mut fb, &cues());
        let bytes = fb.into_rgba_bytes();
        // Top (y=0) unchanged; bottom (y=3) darker.
        assert_eq!(px(&bytes, 1, 0, 0)[0], 255);
        assert!(px(&bytes, 1, 0, 3)[0] < 255);
        assert!(touched > 0);
    }

    #[test]
    fn outline_strokes_a_border_and_near_is_stronger_than_far() {
        assert!(outline_alpha(0.0, &cues()) > outline_alpha(1.0, &cues()));
        let mut fb = SoftwareFramebuffer::new(16, 16);
        fb.clear([1.0, 1.0, 1.0, 1.0]);
        let overlay = DrawOverlay {
            bbox: [3.0, 3.0, 11.0, 11.0],
            mean_depth: 0.0, // near → strong outline
            object_id: 7,
        };
        let (count, pixels) = apply_outlines(&mut fb, &[overlay], &cues());
        assert_eq!(count, 1);
        assert!(pixels > 0);
        // A far object (depth 1) → far_outline_alpha 0 → no darkening, but still
        // "outlined" (the object was processed); pixels are written at alpha 0.
        let mut fb2 = SoftwareFramebuffer::new(16, 16);
        fb2.clear([1.0, 1.0, 1.0, 1.0]);
        let far = DrawOverlay {
            bbox: [3.0, 3.0, 11.0, 11.0],
            mean_depth: 1.0,
            object_id: 8,
        };
        let (c2, _) = apply_outlines(&mut fb2, &[far], &cues());
        assert_eq!(c2, 1);
    }
}
