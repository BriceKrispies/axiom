//! The field paint **layout**: which marking quads exist this frame, given the
//! camera and the two live gameplay lines.
//!
//! This is a pure function into a caller-owned buffer. It allocates nothing, it
//! reads no engine state, and the same inputs always produce the same quads —
//! so the whole system is testable natively and replays exactly.
//!
//! Every marking it emits is a **world-space rectangle**, never a line: a
//! centre plus two half-extents in yards, projected by the renderer as an
//! ordinary four-corner filled polygon like any other piece of the field. There
//! is no stroke anywhere in the field, and nothing here is narrower than
//! [`PaintConfig::hash_width`], so no marking can decay into sub-pixel
//! geometry at the distances it is kept for.

use axiom::prelude::{Transform, Vec3};
use axiom_math::Quat;

use super::coordinates::{FIELD_HALF_LENGTH, FIELD_HALF_WIDTH, GOAL_LINE_Z, HASH_X};
use super::paint::{classify, Lod, PaintCamera, PaintCategory, PaintConfig, PAINT_Y};

/// One marking: a flat, axis-aligned world-space rectangle on the field plane.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PaintQuad {
    pub center: Vec3,
    /// Half the rectangle's extent along `X`, yards.
    pub half_x: f32,
    /// Half the rectangle's extent along `Z`, yards.
    pub half_z: f32,
    pub category: PaintCategory,
}

impl PaintQuad {
    /// The scene transform that draws this rectangle as a unit plane.
    pub fn transform(&self) -> Transform {
        Transform::new(
            self.center,
            Quat::IDENTITY,
            Vec3::new(self.half_x * 2.0, 1.0, self.half_z * 2.0),
        )
    }
}

/// The two markings that answer "what is happening right now", as opposed to
/// "what is this field". They are drawn at every detail tier — a player must
/// never lose the line of scrimmage or the line to gain to level of detail.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct GameplayLines {
    /// World `Z` of the line the current attempt snapped from.
    pub scrimmage_z: Option<f32>,
    /// World `Z` of the line to gain.
    pub line_to_gain_z: Option<f32>,
}

/// The largest number of hash rows the near window can ever ask for, so a
/// degenerate spacing cannot spin the emission loop.
const MAX_HASH_ROWS: i32 = 256;

fn quad(center: Vec3, half_x: f32, half_z: f32, category: PaintCategory) -> PaintQuad {
    PaintQuad {
        center,
        half_x,
        half_z,
        category,
    }
}

/// A full-width line across the field at `z`.
fn cross_line(z: f32, width: f32, y: f32, category: PaintCategory) -> PaintQuad {
    quad(
        Vec3::new(0.0, y, z),
        FIELD_HALF_WIDTH,
        width * 0.5,
        category,
    )
}

/// The field's own identity paint: two sidelines, two goal lines, two end
/// lines. Six quads, always present — they are what makes the surface read as
/// a football field at all, and each is long enough that no camera-relative
/// tier could sensibly drop it.
fn push_boundary(config: &PaintConfig, out: &mut Vec<PaintQuad>) {
    let half = config.boundary_width * 0.5;
    for side in [-1.0f32, 1.0] {
        out.push(quad(
            Vec3::new(side * (FIELD_HALF_WIDTH - half), PAINT_Y, 0.0),
            half,
            FIELD_HALF_LENGTH,
            PaintCategory::Boundary,
        ));
        out.push(cross_line(
            side * (FIELD_HALF_LENGTH - half),
            config.boundary_width,
            PAINT_Y,
            PaintCategory::Boundary,
        ));
        out.push(cross_line(
            side * GOAL_LINE_Z,
            config.boundary_width,
            PAINT_Y,
            PaintCategory::Boundary,
        ));
    }
}

/// Is `z` a major division line?
pub fn is_major_division(z: f32, config: &PaintConfig) -> bool {
    let spacing = config.major_yards;
    let inside = z.abs() < GOAL_LINE_Z;
    inside && spacing > 0.0 && (z / spacing).round().mul_add(-spacing, z).abs() < 1.0e-3
}

/// The retained ten-yard divisions, at whichever fade tier each one falls in.
/// A division is a full-width line, so its distance is measured at the point on
/// it nearest the camera rather than at its centre — otherwise a camera parked
/// on a sideline would tier every line as if it were at midfield.
fn push_majors(camera: &PaintCamera, config: &PaintConfig, out: &mut Vec<PaintQuad>) {
    let steps = (GOAL_LINE_Z / config.major_yards).floor() as i32;
    for index in -steps..=steps {
        let z = index as f32 * config.major_yards;
        if !is_major_division(z, config) {
            continue;
        }
        let nearest = Vec3::new(
            camera.eye.x.clamp(-FIELD_HALF_WIDTH, FIELD_HALF_WIDTH),
            PAINT_Y,
            z,
        );
        let category = match classify(camera, nearest, config) {
            Lod::Near => PaintCategory::MajorNear,
            Lod::Mid => PaintCategory::MajorMid,
            Lod::Far | Lod::Culled => continue,
        };
        out.push(cross_line(z, config.major_width, PAINT_Y, category));
    }
}

/// Paired hash blocks in the near window only.
///
/// The window is derived arithmetically from the camera's own `Z` before a
/// single quad is built, so the ~90 % of the field that could never qualify
/// costs nothing at all — the depth cull happens on an integer range, not on a
/// list of markings.
fn push_hashes(camera: &PaintCamera, config: &PaintConfig, out: &mut Vec<PaintQuad>) {
    let step = config.hash_spacing_yards.max(0.25);
    let first = (camera.eye.z - config.near_yards).max(-GOAL_LINE_Z);
    let last = (camera.eye.z + config.near_yards).min(GOAL_LINE_Z);
    let low = (first / step).ceil() as i32;
    let high = (last / step).floor() as i32;
    let high = high.min(low.saturating_add(MAX_HASH_ROWS));
    for index in low..=high {
        let z = index as f32 * step;
        // A hash never doubles a ten-yard division: that line is already there,
        // full width, and two coplanar quads would fight over the same pixels.
        if is_major_division(z, config) {
            continue;
        }
        for side in [-1.0f32, 1.0] {
            let center = Vec3::new(side * HASH_X, PAINT_Y, z);
            if classify(camera, center, config) != Lod::Near {
                continue;
            }
            out.push(quad(
                center,
                config.hash_length * 0.5,
                config.hash_width * 0.5,
                PaintCategory::Hash,
            ));
        }
    }
}

/// The line of scrimmage and the line to gain — the only paint that is exempt
/// from level of detail entirely.
fn push_gameplay_lines(lines: GameplayLines, config: &PaintConfig, out: &mut Vec<PaintQuad>) {
    let y = PAINT_Y + config.gameplay_lift;
    let on_field = |z: f32| z.is_finite() && z.abs() <= FIELD_HALF_LENGTH;
    for (z, category) in [
        (lines.scrimmage_z, PaintCategory::Scrimmage),
        (lines.line_to_gain_z, PaintCategory::LineToGain),
    ] {
        if let Some(z) = z.filter(|z| on_field(*z)) {
            out.push(cross_line(z, config.gameplay_width, y, category));
        }
    }
}

/// Build this frame's field paint into `out` (cleared first).
///
/// A `None` camera — a degenerate or non-finite look-at, which is the only way
/// an invalid projection could ever be reached — paints nothing rather than
/// emitting geometry that cannot be projected.
pub fn field_paint(
    camera: Option<PaintCamera>,
    lines: GameplayLines,
    config: &PaintConfig,
    out: &mut Vec<PaintQuad>,
) {
    out.clear();
    let Some(camera) = camera else {
        return;
    };
    push_boundary(config, out);
    push_majors(&camera, config, out);
    push_hashes(&camera, config, out);
    push_gameplay_lines(lines, config, out);
}

/// The total pool the emission can ever fill.
pub fn paint_pool_capacity() -> usize {
    PaintCategory::ALL
        .iter()
        .map(|category| category.pool_size())
        .sum()
}
