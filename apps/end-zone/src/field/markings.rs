//! Field markings as procedural quad geometry: sidelines, end lines, goal
//! lines, five-yard lines, one-yard ticks, hash marks, the midfield mark, and
//! block-style field numbers built from a seven-segment table. Everything is
//! flat quads slightly above the turf — no textures, no fonts.

use axiom::prelude::{Vec2, Vec3};

use super::coordinates::{FIELD_HALF_LENGTH, FIELD_HALF_WIDTH, GOAL_LINE_Z, HASH_X};

/// Marking quads float this far above the turf to avoid z-fighting.
pub const MARKING_Y: f32 = 0.02;

/// A batch of flat quads (double-sided so either winding convention renders).
#[derive(Debug, Default, Clone)]
pub struct QuadBatch {
    positions: Vec<Vec3>,
    normals: Vec<Vec3>,
    indices: Vec<u32>,
}

impl QuadBatch {
    pub fn positions(&self) -> &[Vec3] {
        &self.positions
    }

    pub fn normals(&self) -> &[Vec3] {
        &self.normals
    }

    pub fn indices(&self) -> &[u32] {
        &self.indices
    }

    /// Consume into the raw streams (positions, normals, uvs, indices).
    pub fn into_streams(self) -> (Vec<Vec3>, Vec<Vec3>, Vec<Vec2>, Vec<u32>) {
        (self.positions, self.normals, Vec::new(), self.indices)
    }

    /// Push one flat rectangle: `center ± right*half_r ± up*half_u`, both
    /// faces emitted so backface culling can never hide a marking.
    pub fn push_rect(&mut self, center: Vec3, right: Vec3, up: Vec3, half_r: f32, half_u: f32) {
        let r = right.mul_scalar(half_r);
        let u = up.mul_scalar(half_u);
        let base = self.positions.len() as u32;
        self.positions.push(center.subtract(r).subtract(u));
        self.positions.push(center.add(r).subtract(u));
        self.positions.push(center.add(r).add(u));
        self.positions.push(center.subtract(r).add(u));
        for _ in 0..4 {
            self.normals.push(Vec3::UNIT_Y);
        }
        for tri in [[0, 1, 2], [0, 2, 3], [0, 2, 1], [0, 3, 2]] {
            for corner in tri {
                self.indices.push(base + corner);
            }
        }
    }

    /// Push an axis-aligned rectangle on the field plane at [`MARKING_Y`].
    fn push_xz(&mut self, x: f32, z: f32, half_x: f32, half_z: f32) {
        self.push_rect(
            Vec3::new(x, MARKING_Y, z),
            Vec3::UNIT_X,
            Vec3::UNIT_Z,
            half_x,
            half_z,
        );
    }
}

/// Line widths (yards).
const LINE_W: f32 = 0.17;
const BORDER_W: f32 = 0.33;
const TICK_LEN: f32 = 0.32;
const HASH_LEN: f32 = 0.32;

/// All white line work: boundary, goal lines, five-yard lines, ticks, hashes,
/// and the midfield diamond.
pub fn build_markings() -> QuadBatch {
    let mut batch = QuadBatch::default();

    // Sidelines (full length) and end lines (full width).
    for side in [-1.0f32, 1.0] {
        batch.push_xz(
            side * (FIELD_HALF_WIDTH - BORDER_W),
            0.0,
            BORDER_W,
            FIELD_HALF_LENGTH,
        );
        batch.push_xz(
            0.0,
            side * (FIELD_HALF_LENGTH - BORDER_W),
            FIELD_HALF_WIDTH,
            BORDER_W,
        );
        // Goal lines.
        batch.push_xz(0.0, side * GOAL_LINE_Z, FIELD_HALF_WIDTH, LINE_W);
    }

    // Five-yard lines between the goal lines (skipping the goal lines).
    let mut line = -45i32;
    while line <= 45 {
        batch.push_xz(0.0, line as f32, FIELD_HALF_WIDTH, LINE_W);
        line += 5;
    }

    // One-yard ticks near each sideline and hash marks at the inset columns.
    let mut yard = -49i32;
    while yard <= 49 {
        if yard % 5 != 0 {
            let z = yard as f32;
            for side in [-1.0f32, 1.0] {
                batch.push_xz(side * (FIELD_HALF_WIDTH - 1.4), z, TICK_LEN, 0.08);
                batch.push_xz(side * HASH_X, z, HASH_LEN, 0.08);
            }
        }
        yard += 1;
    }

    // Midfield mark: an original hollow diamond (no league branding).
    let d = core::f32::consts::FRAC_1_SQRT_2;
    let diag_a = Vec3::new(d, 0.0, d);
    let diag_b = Vec3::new(-d, 0.0, d);
    for (right, up) in [(diag_a, diag_b), (diag_b, diag_a)] {
        for sign in [-1.0f32, 1.0] {
            batch.push_rect(
                Vec3::new(0.0, MARKING_Y, 0.0).add(up.mul_scalar(sign * 1.5)),
                right,
                up,
                1.7,
                0.14,
            );
        }
    }

    batch
}

/// Seven-segment table for the digits the field needs (0–5). Segments:
/// `[top, top-right, bottom-right, bottom, bottom-left, top-left, middle]`.
const SEGMENTS: [[bool; 7]; 6] = [
    [true, true, true, true, true, true, false],     // 0
    [false, true, true, false, false, false, false], // 1
    [true, true, false, true, true, false, true],    // 2
    [true, true, true, true, false, false, true],    // 3
    [false, true, true, false, false, true, true],   // 4
    [true, false, true, true, true, true, true],     // 5
];

/// Digit metrics (yards).
const DIGIT_H: f32 = 2.1;
const DIGIT_W: f32 = 1.25;
const SEG_T: f32 = 0.15;

/// Push one seven-segment digit. `right`/`up` are the glyph's in-plane frame,
/// `center` is the glyph center on the field plane.
fn push_digit(batch: &mut QuadBatch, digit: usize, center: Vec3, right: Vec3, up: Vec3) {
    let hw = DIGIT_W / 2.0 - SEG_T;
    let hh = DIGIT_H / 4.0;
    let segs = SEGMENTS[digit];
    let rects: [(f32, f32, f32, f32); 7] = [
        (0.0, 2.0 * hh, hw, SEG_T),  // top
        (hw, hh, SEG_T, hh),         // top-right
        (hw, -hh, SEG_T, hh),        // bottom-right
        (0.0, -2.0 * hh, hw, SEG_T), // bottom
        (-hw, -hh, SEG_T, hh),       // bottom-left
        (-hw, hh, SEG_T, hh),        // top-left
        (0.0, 0.0, hw, SEG_T),       // middle
    ];
    for (on, (du, dv, half_r, half_u)) in segs.iter().zip(rects) {
        if *on {
            let c = center.add(right.mul_scalar(du)).add(up.mul_scalar(dv));
            batch.push_rect(c, right, up, half_r, half_u);
        }
    }
}

/// Block field numbers every ten yards on both sides, oriented so each column
/// reads from its near sideline (glyph top toward the field center).
pub fn build_numbers() -> QuadBatch {
    let mut batch = QuadBatch::default();
    let number_x = 17.0;
    for (column_x, right, up) in [
        (
            number_x,
            Vec3::new(0.0, 0.0, -1.0),
            Vec3::new(-1.0, 0.0, 0.0),
        ),
        (
            -number_x,
            Vec3::new(0.0, 0.0, 1.0),
            Vec3::new(1.0, 0.0, 0.0),
        ),
    ] {
        for line in [-40i32, -30, -20, -10, 0, 10, 20, 30, 40] {
            let tens = (GOAL_LINE_Z - (line as f32).abs()) as usize / 10;
            let z = line as f32;
            let spacing = (DIGIT_W + 0.45) / 2.0;
            let c0 = Vec3::new(column_x, MARKING_Y, z).subtract(right.mul_scalar(spacing));
            let c1 = Vec3::new(column_x, MARKING_Y, z).add(right.mul_scalar(spacing));
            push_digit(&mut batch, tens, c0, right, up);
            push_digit(&mut batch, 0, c1, right, up);
        }
    }
    batch
}
