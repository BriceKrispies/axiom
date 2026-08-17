//! **The caption over each body**: which station's shader it is wearing, in
//! world space, welded out of [`crate::glyphs`] into one mesh per label.
//!
//! ## Why a body needs a caption at all
//!
//! Twelve bodies stand in two rows and the page carries a legend under the
//! canvas that names ten stations. A reader has to count along a row and map
//! twelve objects onto ten entries — and the mapping is not one to one, because
//! station 5 stands two bodies up, station 6 stands three, and station 8 stands
//! two. The legend is a key to a diagram whose parts are unlabelled, which is
//! the one thing a demonstration must not be.
//!
//! So each caption is numbered by **body**, `1..=12`, left to right and front
//! row first — an unambiguous pointer at the thing you are looking at — and its
//! second line carries the station number the legend uses, so the two can be put
//! side by side.
//!
//! ## The captions are read off the station table
//!
//! [`LINES`] names its own short text (a station's `proves` sentence is a
//! paragraph, and a paragraph at this size is a smear), but [`STATION_OF_SLOT`]
//! is the one place the body → station mapping is written down, and
//! `tests::every_caption_names_a_real_station` checks each caption's `S<n>`
//! prefix against [`crate::stations::STATIONS`]. A station renumbered in the
//! table and not here is a failing test rather than a wrong label.
//!
//! ## World space, and the orbit camera
//!
//! A caption is ordinary scene geometry at an ordinary world transform: it
//! stands above its body, it moves when the body moves, and there is no viewpoint
//! baked into it. Facing it at the camera is not done here — it is done once per
//! frame in [`crate::frame::packet_of`], which is the app's own per-frame hook
//! and the only place that has the frame's camera. See
//! [`crate::frame::billboard_labels`].

use axiom::prelude::*;

use crate::glyphs::{text_columns, text_runs, GLYPH_H};
use crate::layout::{ch, slot_position, SLOT_COUNT};

/// How many captions the stand carries — one per body.
pub const COUNT: usize = SLOT_COUNT;

/// The two lines of each body's caption, in slot order.
///
/// Line 1 is the body's own number and what it is; line 2 is the station it
/// belongs to and the one thing that station proves. Both are kept to
/// [`MAX_CHARS`] because the width of a row is fixed and a caption that does not
/// fit its slot is a caption that overlaps its neighbour.
pub const LINES: [[&str; 2]; COUNT] = [
    ["1 . LAYERED", "S1 METAL+PAINT"],
    ["2 . LIVE", "S2 GRAPH TO WGSL"],
    ["3 . BAKED", "S3 THE SAME GRAPH"],
    ["4 . RETUNE", "S4 9 TUNES 1 PRG"],
    ["5 . WIND", "S5 VERTEX FIELD"],
    ["6 . RIPPLE", "S5 VERTEX FIELD"],
    ["7 . UNLIT", "S6 LIGHTING"],
    ["8 . LAMBERT", "S6 LIGHTING"],
    ["9 . LAMBERT S", "S6 LIGHTING"],
    ["10 . METABALLS", "S7 SCALARFIELD"],
    ["11 . MARBLE", "S8 SIN AND POW"],
    ["12 . WOOD", "S8 SIN AND POW"],
];

/// Which station each body belongs to, in slot order — the mapping the page's
/// legend needs and the only place it is written down.
pub const STATION_OF_SLOT: [u8; COUNT] = [1, 2, 3, 4, 5, 5, 6, 6, 6, 7, 8, 8];

/// The longest caption line this layout is designed for. A longer one still
/// renders — it is scaled down to fit — but it renders *smaller*, and past this
/// it stops being legible on the back row.
pub const MAX_CHARS: usize = 19;

/// The cap height of line 1 in the caption's own local units. Line 2 and the
/// gap are expressed as fractions of it, so the whole block scales as one.
const LINE1_CAP: f32 = 1.0;
/// Line 2's cap height, as a fraction of line 1's.
const LINE2_RATIO: f32 = 0.66;
/// The gap between the two lines, as a fraction of line 1's cap height.
const LINE_GAP: f32 = 0.34;

/// **The widest a caption may be, in world units.**
///
/// This is the one number the whole caption layout is pinned to, and it is not a
/// taste setting: [`crate::layout`] stands the bodies `2.55` units apart, so a
/// caption wider than that *is* its neighbour's caption. The margin is what
/// leaves a visible gap between two adjacent captions that are both at the
/// limit. The first version of this file let the back row be `3.05` wide on the
/// theory that the space above it was empty — it is, vertically, and the row
/// still collided with itself horizontally in the very first capture.
const MAX_WIDTH: f32 = 2.28;

/// The tallest a caption's line 1 may be — the cap that stops the whole row
/// being scaled up to whatever the *shortest* caption could afford.
const MAX_CAP: f32 = 0.27;

/// How far above its slot's centre a caption's baseline stands.
const ROW_LIFT: [f32; 2] = [1.34, 1.52];

/// **How much larger the back row's captions are set, per row.**
///
/// The back row stands 5.6 units further from the authored eye than the front
/// one, so a caption of the same physical size reads at about 60% there — which
/// is the complaint this whole file answers, since the back row is exactly the
/// half whose detail is hardest to judge. Setting it larger puts the two rows at
/// roughly the same *apparent* size.
///
/// It is affordable only because of [`ROW_STAGGER`]. Without the stagger the
/// budget is one body spacing and a 1.45x caption does not fit in it.
const ROW_ENLARGE: [f32; 2] = [1.0, 1.45];

/// **How far every other caption of a row is raised above its neighbours.**
///
/// A caption's horizontal budget is the gap between two bodies — `2.55` units.
/// Lifting alternate captions clear of each other doubles it to the gap between
/// two bodies *two apart*, which is what pays for [`ROW_ENLARGE`]. Zero on the
/// front row: there is empty sky above the back row and nothing to occlude, but
/// a raised front-row caption would cross the back row's bodies.
const ROW_STAGGER: [f32; 2] = [0.0, 0.74];

/// The caption's colour. Emissive as well as lit, so a caption is legible
/// against a dark ground and against a bright body alike, and so it never reads
/// as one more shaded subject beside the eleven that are.
fn caption_material(app: &mut RunningApp) -> Handle<Material> {
    app.add_material(
        Material::lit(Color::linear_rgb(ch(0.72), ch(0.86), ch(1.0)))
            .with_emissive(Color::linear_rgb(ch(0.62), ch(0.80), ch(0.96))),
    )
}

/// How wide `lines` is in the caption's own local units, at [`LINE1_CAP`].
fn local_width(lines: [&str; 2]) -> f32 {
    let cell1 = LINE1_CAP / GLYPH_H as f32;
    let cell2 = LINE1_CAP * LINE2_RATIO / GLYPH_H as f32;
    let width1 = text_columns(lines[0]) as f32 * cell1;
    let width2 = text_columns(lines[1]) as f32 * cell2;
    width1.max(width2).max(f32::EPSILON)
}

/// **The one scale every caption is drawn at.**
///
/// Deliberately shared rather than fitted per caption. Fitting each one
/// separately makes the *shortest* caption the biggest — the first capture had
/// `5 . WIND` set half again as large as `9 . LAMBERT S` right beside it — which
/// reads as emphasis the app does not mean: a viewer takes a bigger caption to
/// be a more important station. One size across the stand says the twelve
/// bodies are twelve peers, and it is the longest caption that decides how big
/// that size can be.
fn caption_scale() -> f32 {
    LINES
        .iter()
        .map(|lines| MAX_WIDTH / local_width(*lines))
        .fold(MAX_CAP / LINE1_CAP, f32::min)
}

/// One caption's geometry, in its own local space: `x` centred on zero, `y`
/// rising from zero at the bottom of line 2, `z` flat at zero with the lit face
/// toward `+z`.
///
/// The block is scaled here rather than by the spawn transform because the two
/// lines have different cap heights and a non-uniform node scale would shear
/// them differently. One mesh, already the size it will be drawn at.
pub fn caption_mesh(lines: [&str; 2], scale: f32) -> MeshData {
    let cap1 = LINE1_CAP * scale;
    let cap2 = cap1 * LINE2_RATIO;
    let baselines = [cap2 + LINE_GAP * cap1, 0.0];
    let caps = [cap1, cap2];

    let mut positions: Vec<Vec3> = Vec::new();
    let mut normals: Vec<Vec3> = Vec::new();
    let mut uvs: Vec<Vec2> = Vec::new();
    let mut indices: Vec<u32> = Vec::new();

    for (line, text) in lines.iter().enumerate() {
        let cell = caps[line] / GLYPH_H as f32;
        let left = -(text_columns(text) as f32 * cell) * 0.5;
        for run in text_runs(text) {
            let x0 = left + run.col as f32 * cell;
            let x1 = x0 + run.len as f32 * cell;
            // Row 0 is the cap line; the baseline is the bottom of row 6.
            let y1 = baselines[line] + caps[line] - run.row as f32 * cell;
            let y0 = y1 - cell;
            let base = positions.len() as u32;
            positions.extend([
                Vec3::new(x0, y0, 0.0),
                Vec3::new(x1, y0, 0.0),
                Vec3::new(x1, y1, 0.0),
                Vec3::new(x0, y1, 0.0),
            ]);
            normals.extend([Vec3::new(0.0, 0.0, 1.0); 4]);
            // Real corner UVs rather than four zeroes: a degenerate UV quad has
            // no derivative, and a zero-area UV footprint is the shape that
            // produces NaN tangents on the mobile GPU path.
            uvs.extend([
                Vec2::new(0.0, 1.0),
                Vec2::new(1.0, 1.0),
                Vec2::new(1.0, 0.0),
                Vec2::new(0.0, 0.0),
            ]);
            // Counter-clockwise seen from `+z`, which is the face the billboard
            // turns toward the camera.
            indices.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
        }
    }
    MeshData::new(positions, normals, uvs, indices)
}

/// Where the caption for `slot` stands: above its body, on the body's own
/// column and depth, and raised by [`ROW_STAGGER`] on every other slot.
pub fn caption_position(slot: usize) -> Vec3 {
    let row = row_of(slot);
    let lift = ROW_LIFT[row] + ROW_STAGGER[row] * (slot % 2) as f32;
    let body = slot_position(slot);
    Vec3::new(body.x, body.y + lift, body.z)
}

/// The scale the captions of `row` are set at — the one shared
/// [`caption_scale`], enlarged by that row's [`ROW_ENLARGE`].
fn row_scale(row: usize) -> f32 {
    caption_scale() * ROW_ENLARGE[row]
}

/// Which row `slot` stands in — `0` front, `1` back. The same halving
/// [`crate::layout::slot_position`] does, and the only thing the per-row caption
/// constants are indexed by.
fn row_of(slot: usize) -> usize {
    (slot / (SLOT_COUNT / 2)).min(1)
}

/// Stand a caption over every body.
///
/// One mesh per caption: they differ in their glyphs, so there is nothing to
/// share, and twelve small static meshes registered once at startup cost one
/// upload each and nothing per frame.
pub fn stand_captions(app: &mut RunningApp) {
    let material = caption_material(app);
    (0..COUNT).for_each(|slot| {
        let mesh = caption_mesh(LINES[slot], row_scale(row_of(slot)));
        app.add_mesh_data(mesh)
            .ok()
            .into_iter()
            .for_each(|handle| {
                app.spawn(Spawn::new(
                    Transform::from_translation(caption_position(slot)),
                    handle,
                    material,
                ));
            });
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::glyphs::{has_glyph, GLYPH_GAP, GLYPH_W};
    use crate::stations::STATIONS;

    /// **Every caption is drawable by the font this app ships.** A caption with
    /// a character the table has no glyph for renders a silent gap, which is
    /// exactly the kind of defect a screenshot review misses.
    #[test]
    fn every_caption_character_has_a_glyph() {
        LINES.iter().flatten().for_each(|line| {
            line.chars().for_each(|character| {
                assert!(has_glyph(character), "no glyph for {character:?} in {line:?}");
            });
        });
    }

    /// **Every caption fits the row it stands in.** The layout scales a caption
    /// down to fit rather than letting it overlap, so "fits" is not in question
    /// — what this pins is that no caption is so long that fitting it makes it
    /// smaller than the design allows.
    #[test]
    fn no_caption_is_longer_than_the_layout_is_designed_for() {
        LINES.iter().flatten().for_each(|line| {
            assert!(
                line.chars().count() <= MAX_CHARS,
                "{line:?} is {} characters, past {MAX_CHARS}",
                line.chars().count()
            );
        });
    }

    /// **Each caption's `S<n>` names a station that exists**, and the slot →
    /// station mapping agrees with the order [`crate::stand::populate`] stands
    /// the bodies up in. This is what stops the caption table and the station
    /// table drifting apart.
    #[test]
    fn every_caption_names_a_real_station() {
        LINES.iter().enumerate().for_each(|(slot, lines)| {
            let station = STATION_OF_SLOT[slot];
            assert!(
                STATIONS.iter().any(|s| s.number == station),
                "slot {slot} names station {station}, which does not exist"
            );
            assert!(
                lines[1].starts_with(&format!("S{station} ")),
                "slot {slot}'s second line {:?} does not name station {station}",
                lines[1]
            );
        });
    }

    /// The body numbers run `1..=12` in slot order, so a viewer counting along
    /// the front row and then the back one reads them in order.
    #[test]
    fn the_body_numbers_run_one_to_twelve_in_slot_order() {
        LINES.iter().enumerate().for_each(|(slot, lines)| {
            assert!(
                lines[0].starts_with(&format!("{} . ", slot + 1)),
                "slot {slot} is numbered {:?}",
                lines[0]
            );
        });
    }

    /// **Every station that stands a body up is captioned.** Stations 9 and 10
    /// are reports about the others and author no body, so they are the only two
    /// absent — a station that quietly stopped being represented would otherwise
    /// go unnoticed.
    #[test]
    fn every_station_with_a_body_is_captioned() {
        let captioned: std::collections::BTreeSet<u8> =
            STATION_OF_SLOT.iter().copied().collect();
        assert_eq!(captioned, (1_u8..=8).collect());
    }

    /// A caption is real geometry: four vertices and six indices per lit run,
    /// and every index inside the vertex array.
    #[test]
    fn a_caption_is_a_welded_quad_per_lit_run() {
        let mesh = caption_mesh(LINES[0], row_scale(0));
        assert!(!mesh.positions().is_empty());
        assert_eq!(mesh.positions().len() % 4, 0);
        assert_eq!(mesh.indices().len(), mesh.positions().len() / 4 * 6);
        assert_eq!(mesh.normals().len(), mesh.positions().len());
        assert_eq!(mesh.uvs().len(), mesh.positions().len());
        let vertices = mesh.positions().len() as u32;
        assert!(mesh.indices().iter().all(|index| *index < vertices));
    }

    /// **A caption fits the width it was given, and is centred on its own
    /// origin.** An off-centre caption drifts away from the body it names as it
    /// gets longer, which is the failure mode of laying text out from a left
    /// edge.
    ///
    /// Centred is asserted on the *advance* box within one glyph, not on the lit
    /// extent to the float: a `1` lights only the middle of its five cells and a
    /// `.` only one of them, so the ink of a caption beginning `10 . ` genuinely
    /// starts a cell in from its own left edge. Pinning the ink exactly would be
    /// pinning the shapes of the glyphs, which is [`crate::glyphs`]'s job.
    #[test]
    fn a_caption_fits_its_width_and_is_centred() {
        LINES.iter().enumerate().for_each(|(slot, lines)| {
            let row = row_of(slot);
            let mesh = caption_mesh(*lines, row_scale(row));
            let xs: Vec<f32> = mesh.positions().iter().map(|p| p.x).collect();
            let min = xs.iter().copied().fold(f32::MAX, f32::min);
            let max = xs.iter().copied().fold(f32::MIN, f32::max);
            let glyph = MAX_CAP * ROW_ENLARGE[row] * GLYPH_W as f32 / GLYPH_H as f32;
            assert!(
                max - min <= MAX_WIDTH * ROW_ENLARGE[row] + 1.0e-4,
                "slot {slot} is too wide"
            );
            assert!(
                (min + max).abs() < glyph,
                "slot {slot} is off centre by {}",
                (min + max).abs()
            );
        });
    }

    /// **No caption reaches the caption beside it.**
    ///
    /// The failure this pins is the one the first two captures actually showed:
    /// a caption wider than its share of [`crate::layout`]'s spacing overwrites
    /// its neighbour and the row becomes an unreadable smear. The budget is one
    /// body spacing on an unstaggered row and two on a staggered one, because a
    /// staggered caption's *nearest caption at the same height* is two slots
    /// away. Both numbers are derived from the real slot positions, so
    /// re-spacing the stand cannot silently reintroduce the collision.
    #[test]
    fn no_caption_reaches_the_caption_beside_it() {
        let spacing = slot_position(1).x - slot_position(0).x;
        [0_usize, 1].iter().for_each(|row| {
            let staggered = ROW_STAGGER[*row] > 0.0;
            let budget = spacing * [1.0_f32, 2.0][usize::from(staggered)];
            let widest = LINES
                .iter()
                .enumerate()
                .filter(|(slot, _)| row_of(*slot) == *row)
                .map(|(_, lines)| local_width(*lines) * row_scale(*row))
                .fold(0.0_f32, f32::max);
            assert!(
                widest < budget,
                "row {row}'s widest caption is {widest} against a budget of {budget}"
            );
            assert!(
                widest > budget * 0.55,
                "row {row}'s captions are smaller than the stand can afford"
            );
        });
    }

    /// **A staggered caption clears the one beside it vertically.** The stagger
    /// is what buys the back row its size; if the offset were smaller than a
    /// caption is tall, the two would still overlap and the extra size would be
    /// bought with a collision.
    #[test]
    fn the_stagger_clears_a_caption_height() {
        let mesh = caption_mesh(LINES[6], row_scale(1));
        let ys: Vec<f32> = mesh.positions().iter().map(|p| p.y).collect();
        let height = ys.iter().copied().fold(f32::MIN, f32::max)
            - ys.iter().copied().fold(f32::MAX, f32::min);
        assert!(
            ROW_STAGGER[1] > height,
            "the back row staggers by {} against a caption {height} tall",
            ROW_STAGGER[1]
        );
        assert_eq!(ROW_STAGGER[0], 0.0, "the front row must not cross the back row");
    }

    /// **Every caption in a row is drawn at the same size.** One scale per row,
    /// bounded by the longest caption and by the cap — so a short caption is not
    /// promoted into a banner and a long one is not shrunk alone.
    #[test]
    fn every_caption_in_a_row_is_set_at_that_rows_one_scale() {
        assert!(caption_scale() > 0.0 && caption_scale() <= MAX_CAP / LINE1_CAP);
        [0_usize, 1].iter().for_each(|row| {
            let heights: Vec<f32> = LINES
                .iter()
                .enumerate()
                .filter(|(slot, _)| row_of(*slot) == *row)
                .map(|(_, lines)| {
                    let mesh = caption_mesh(*lines, row_scale(*row));
                    mesh.positions()
                        .iter()
                        .map(|p| p.y)
                        .fold(f32::MIN, f32::max)
                })
                .collect();
            heights.windows(2).for_each(|pair| {
                assert!((pair[0] - pair[1]).abs() < 1.0e-5, "row {row} differs in size");
            });
        });
        // The back row is set larger, which is the whole reason it staggers.
        assert!(row_scale(1) > row_scale(0));
    }

    /// Every caption stands above its body and above the ground, and the back
    /// row's captions stand higher than the front row's — so a front caption
    /// never crosses a back body.
    #[test]
    fn every_caption_stands_above_its_body() {
        (0..COUNT).for_each(|slot| {
            let caption = caption_position(slot);
            let body = slot_position(slot);
            assert_eq!((caption.x, caption.z), (body.x, body.z));
            assert!(caption.y > body.y);
        });
        assert!(caption_position(6).y > caption_position(0).y);
    }

    /// The row split is the same halving the layout does, and it is total.
    #[test]
    fn the_row_of_a_slot_is_the_layouts_own_halving() {
        assert_eq!(row_of(0), 0);
        assert_eq!(row_of(5), 0);
        assert_eq!(row_of(6), 1);
        assert_eq!(row_of(11), 1);
        assert_eq!(row_of(999), 1, "the row index is clamped, never out of range");
    }

    /// The glyph cell constants the layout divides by are the font's own.
    #[test]
    fn the_layout_measures_in_the_fonts_own_cells() {
        assert_eq!((GLYPH_W, GLYPH_H, GLYPH_GAP), (5, 7, 1));
    }
}
