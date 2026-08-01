//! Building one chunk of road geometry from the sampled centreline.
//!
//! A chunk is a fixed span of the course ([`CHUNK_LENGTH`] metres) turned into
//! four meshes, one per material, so a visible chunk costs four draw calls
//! rather than one per lane marking:
//!
//! | Mesh | Contents |
//! |---|---|
//! | `surface` | tarmac and both shoulders |
//! | `paint` | lane dashes, edge lines, rumble blocks |
//! | `rail` | guardrails, and the tunnel's walls and roof |
//! | `verge` | the ground strip either side, out to the scenery line |
//!
//! ## Why chunk boundaries cannot crack
//!
//! Chunk `n` spans samples `[n·k, (n+1)·k]` **inclusive at both ends** — the
//! last row of chunk `n` is generated from the *same* [`TrackSample`] as the
//! first row of chunk `n + 1`. Not an equivalent sample, not a recomputed one:
//! the same entry of the same immutable table. Two chunks therefore share their
//! boundary vertices exactly, and no floating-point difference can open a seam
//! between them. The test suite asserts this by comparing the actual generated
//! positions across every boundary on the course.

use axiom::prelude::{MeshData, Vec2, Vec3};

use crate::track::{Track, TrackSample};
use crate::tuning::CourseTuning;

use super::asphalt_texture::TILE_METRES;
use super::surface_builder::SurfaceBuilder;

/// The span of course one rendered chunk covers (m).
pub const CHUNK_LENGTH: f32 = 100.0;

/// How far past the barrier the ground strip extends (m). Enough to frame the
/// road and hide the horizon gap; nowhere near an open world.
pub const VERGE_REACH: f32 = 46.0;

/// Height of the guardrail above the road (m).
pub const RAIL_HEIGHT: f32 = 0.85;
/// Depth of the guardrail's face (m).
pub const RAIL_DEPTH: f32 = 0.16;
/// Height of a tunnel's ceiling above the road (m).
pub const TUNNEL_HEIGHT: f32 = 7.0;

/// The four material-separated meshes one chunk resolves to.
#[derive(Debug, Clone)]
pub struct ChunkMeshes {
    pub surface: MeshData,
    pub paint: MeshData,
    pub rail: MeshData,
    pub verge: MeshData,
}

/// How many chunks cover `track`.
pub fn chunk_count(track: &Track) -> usize {
    ((track.length() / CHUNK_LENGTH).ceil() as usize).max(1)
}

/// The inclusive sample index range chunk `index` is built from.
///
/// Both ends are inclusive, and consecutive chunks *share* their boundary index.
/// That sharing is the whole crack-free guarantee — see the module docs.
pub fn chunk_sample_range(track: &Track, index: usize) -> (usize, usize) {
    let per_chunk = (CHUNK_LENGTH / track.spacing()).round().max(1.0) as usize;
    let last = track.samples().len().saturating_sub(1);
    let start = (index * per_chunk).min(last);
    let end = ((index + 1) * per_chunk).min(last);
    (start, end)
}

/// Build chunk `index` of `track`.
pub fn build_chunk(track: &Track, index: usize, tuning: &CourseTuning) -> ChunkMeshes {
    let (start, end) = chunk_sample_range(track, index);
    let samples = &track.samples()[start..=end];
    let rows = samples.len();

    let mut surface = SurfaceBuilder::with_quad_capacity(rows * 3);
    let mut paint = SurfaceBuilder::with_quad_capacity(rows * 3);
    let mut rail = SurfaceBuilder::with_quad_capacity(rows * 4);
    let mut verge = SurfaceBuilder::with_quad_capacity(rows * 2);

    for pair in samples.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        strip_surface(&mut surface, track, &a, &b);
        strip_verge(&mut verge, track, &a, &b);
        strip_paint(&mut paint, track, &a, &b, tuning);
        strip_rail(&mut rail, track, &a, &b);
    }

    ChunkMeshes {
        surface: surface.build(),
        paint: paint.build(),
        rail: rail.build(),
        verge: verge.build(),
    }
}

/// Tarmac plus both shoulders, as three quads per sample pair.
///
/// ## The paving is UV-mapped in **metres**, not per quad
///
/// A paved quad spans the full road width by one sample spacing — on the opening
/// straight, **18 m × 2 m**. Stretching the aggregate grain once across that (the
/// builder's default) does two visible things, and both of them are in the
/// champion render: the 32-texel grain lands at 0.56 m × 0.06 m per texel, so it
/// reads as metre-scale camouflage blotches smeared 9:1 across the road instead
/// of as aggregate; and because *every* quad gets exactly one copy, the identical
/// pattern repeats in lock-step every 2 m, banding the road transversely all the
/// way to the horizon.
///
/// Deriving each corner's UV from its own world position fixes both at the root.
/// `u` is the lateral offset in metres and `v` the absolute course distance, each
/// divided by [`TILE_METRES`] — so the grain is square, at a real physical scale,
/// and continuous across quad and chunk boundaries (adjacent chunks share their
/// boundary sample, so they share its `distance` exactly and the mapping cannot
/// crack). `Repeat` addressing plus the texture's toroidal lattice means the
/// resulting non-integer tile counts leave no seam.
fn strip_surface(out: &mut SurfaceBuilder, track: &Track, a: &TrackSample, b: &TrackSample) {
    let shoulder = track.shoulder();
    // Tarmac.
    out.ground_quad_with_uvs(
        a.at_lateral(-a.half_width),
        b.at_lateral(-b.half_width),
        b.at_lateral(b.half_width),
        a.at_lateral(a.half_width),
        paving_uvs([
            (-a.half_width, a.distance),
            (-b.half_width, b.distance),
            (b.half_width, b.distance),
            (a.half_width, a.distance),
        ]),
    );
    // Shoulders, a hair below the tarmac so the join reads as a lip rather than
    // z-fighting with it. Same mapping, so the grain runs across the join
    // unbroken rather than restarting at the tarmac edge.
    for side in [-1.0f32, 1.0] {
        out.ground_quad_with_uvs(
            a.at_lateral(side * a.half_width).add(Vec3::new(0.0, -SHOULDER_DROP, 0.0)),
            b.at_lateral(side * b.half_width).add(Vec3::new(0.0, -SHOULDER_DROP, 0.0)),
            b.at_lateral(side * (b.half_width + shoulder))
                .add(Vec3::new(0.0, -SHOULDER_DROP, 0.0)),
            a.at_lateral(side * (a.half_width + shoulder))
                .add(Vec3::new(0.0, -SHOULDER_DROP, 0.0)),
            paving_uvs([
                (side * a.half_width, a.distance),
                (side * b.half_width, b.distance),
                (side * (b.half_width + shoulder), b.distance),
                (side * (a.half_width + shoulder), a.distance),
            ]),
        );
    }
}

/// Corner UVs for a paved quad, from each corner's `(lateral, along)` position on
/// the course in metres.
fn paving_uvs(corners: [(f32, f32); 4]) -> [Vec2; 4] {
    corners.map(|(lateral, along)| Vec2::new(lateral / TILE_METRES, along / TILE_METRES))
}

/// How far below the tarmac the shoulder sits (m). Deliberately not a hair's
/// breadth: two nearly-coplanar surfaces a few centimetres apart z-fight into
/// shimmering bands once they are a few hundred metres away.
const SHOULDER_DROP: f32 = 0.09;

/// The ground either side, from the **shoulder edge** out to the scenery line.
///
/// It starts where the paved surface stops, not at the barrier. That is not a
/// detail: between the shoulder and the barrier is the dirt verge the car
/// actually drives on when it runs wide, and starting the ground at the barrier
/// leaves that strip with no geometry at all - a hole either side of the road
/// that the player sees straight through, worst exactly when they are off-line
/// and looking at it.
fn strip_verge(out: &mut SurfaceBuilder, track: &Track, a: &TrackSample, b: &TrackSample) {
    let inner_a = a.half_width + track.shoulder();
    let inner_b = b.half_width + track.shoulder();
    let outer_a = track.barrier_offset(a) + VERGE_REACH;
    let outer_b = track.barrier_offset(b) + VERGE_REACH;
    for side in [-1.0f32, 1.0] {
        out.ground_quad(
            a.at_lateral(side * inner_a).add(Vec3::new(0.0, -VERGE_DROP, 0.0)),
            b.at_lateral(side * inner_b).add(Vec3::new(0.0, -VERGE_DROP, 0.0)),
            b.at_lateral(side * outer_b).add(Vec3::new(0.0, -VERGE_FALL, 0.0)),
            a.at_lateral(side * outer_a).add(Vec3::new(0.0, -VERGE_FALL, 0.0)),
        );
    }
}

/// Drop at the inner edge of the verge (m). Large enough that the verge and the
/// shoulder are never within depth-buffer precision of each other at distance.
const VERGE_DROP: f32 = 0.16;
/// Drop at the outer edge of the verge (m) — a gentle fall away from the road.
const VERGE_FALL: f32 = 1.6;

/// Lane dashes, edge lines and rumble blocks.
///
/// Dashes are placed by **absolute course distance**, not by sample index, so
/// their spacing is constant in metres no matter how the samples fall — which is
/// the entire point of the arc-length table. That constant spacing is what makes
/// them a usable speed reference: at 90 m/s a 12 m period is seven and a half
/// dashes a second, and the eye reads that rate directly as speed.
fn strip_paint(
    out: &mut SurfaceBuilder,
    track: &Track,
    a: &TrackSample,
    b: &TrackSample,
    tuning: &CourseTuning,
) {
    // Solid edge lines, both sides, continuous.
    for side in [-1.0f32, 1.0] {
        let inner = side * (a.half_width - EDGE_LINE_INSET);
        let inner_b = side * (b.half_width - EDGE_LINE_INSET);
        let outer = side * (a.half_width - EDGE_LINE_INSET - side.signum() * side * EDGE_LINE_WIDTH);
        let outer_b =
            side * (b.half_width - EDGE_LINE_INSET - side.signum() * side * EDGE_LINE_WIDTH);
        out.ground_quad(
            a.at_lateral(inner).add(PAINT_LIFT),
            b.at_lateral(inner_b).add(PAINT_LIFT),
            b.at_lateral(outer_b).add(PAINT_LIFT),
            a.at_lateral(outer).add(PAINT_LIFT),
        );
    }

    // Lane dashes: one quad per sample pair that falls inside a painted dash.
    let phase = a.distance.rem_euclid(tuning.dash_period);
    if phase < tuning.dash_length {
        // The dividers are painted between the *track's* lanes, the same ones
        // the traffic holds — see `Track::lane_count`.
        let lanes = track.lane_count(a) as i32;
        for lane in 1..lanes {
            let t = lane as f32 / lanes as f32;
            let offset_a = -a.half_width + t * a.half_width * 2.0;
            let offset_b = -b.half_width + t * b.half_width * 2.0;
            out.ground_quad(
                a.at_lateral(offset_a - DASH_HALF_WIDTH).add(PAINT_LIFT),
                b.at_lateral(offset_b - DASH_HALF_WIDTH).add(PAINT_LIFT),
                b.at_lateral(offset_b + DASH_HALF_WIDTH).add(PAINT_LIFT),
                a.at_lateral(offset_a + DASH_HALF_WIDTH).add(PAINT_LIFT),
            );
        }
    }

    // Rumble blocks on the shoulder, alternating so they strobe past.
    if a.distance.rem_euclid(RUMBLE_PERIOD) < RUMBLE_PERIOD * 0.5 {
        let shoulder = track.shoulder();
        for side in [-1.0f32, 1.0] {
            let inner = side * (a.half_width + shoulder * 0.15);
            let inner_b = side * (b.half_width + shoulder * 0.15);
            let outer = side * (a.half_width + shoulder * 0.9);
            let outer_b = side * (b.half_width + shoulder * 0.9);
            out.ground_quad(
                a.at_lateral(inner).add(PAINT_LIFT),
                b.at_lateral(inner_b).add(PAINT_LIFT),
                b.at_lateral(outer_b).add(PAINT_LIFT),
                a.at_lateral(outer).add(PAINT_LIFT),
            );
        }
    }
}

/// How far the paint sits above the tarmac (m) — enough to beat depth precision
/// at a kilometre, small enough to be invisible.
const PAINT_LIFT: Vec3 = Vec3::new(0.0, 0.075, 0.0);
/// How far inside the tarmac edge the solid edge line starts (m).
const EDGE_LINE_INSET: f32 = 0.25;
/// Width of the solid edge line (m).
const EDGE_LINE_WIDTH: f32 = 0.22;
/// Half-width of a lane dash (m).
const DASH_HALF_WIDTH: f32 = 0.12;
/// Period of the alternating rumble blocks (m).
const RUMBLE_PERIOD: f32 = 3.0;

/// Guardrails, and a tunnel's walls and roof.
fn strip_rail(out: &mut SurfaceBuilder, track: &Track, a: &TrackSample, b: &TrackSample) {
    let walled = a.section.walled();
    // Open road gets a guardrail only where the corner is sharp enough to need
    // one; a rail down every straight would be visual noise and 9 km of geometry.
    let needs_rail = walled || a.curvature.abs() > RAIL_CURVATURE;
    if !needs_rail {
        return;
    }
    let offset_a = track.barrier_offset(a);
    let offset_b = track.barrier_offset(b);
    for side in [-1.0f32, 1.0] {
        let low_a = a.at_lateral(side * offset_a);
        let low_b = b.at_lateral(side * offset_b);
        let high_a = low_a.add(a.up.mul_scalar(RAIL_HEIGHT));
        let high_b = low_b.add(b.up.mul_scalar(RAIL_HEIGHT));
        let inward = a.right.mul_scalar(-side);
        // The rail face, pointing back at the road.
        out.quad(
            low_a.add(a.up.mul_scalar(RAIL_HEIGHT - RAIL_DEPTH)),
            low_b.add(b.up.mul_scalar(RAIL_HEIGHT - RAIL_DEPTH)),
            high_b,
            high_a,
            inward,
        );
        if walled {
            // A tunnel: the rail becomes a full wall, and there is a roof.
            let wall_a = low_a.add(a.up.mul_scalar(TUNNEL_HEIGHT));
            let wall_b = low_b.add(b.up.mul_scalar(TUNNEL_HEIGHT));
            out.quad(high_a, high_b, wall_b, wall_a, inward);
        }
    }
    if walled {
        let roof_a = a
            .position
            .add(a.up.mul_scalar(TUNNEL_HEIGHT));
        let roof_b = b.position.add(b.up.mul_scalar(TUNNEL_HEIGHT));
        let half_a = offset_a;
        let half_b = offset_b;
        out.quad(
            roof_a.add(a.right.mul_scalar(-half_a)),
            roof_b.add(b.right.mul_scalar(-half_b)),
            roof_b.add(b.right.mul_scalar(half_b)),
            roof_a.add(a.right.mul_scalar(half_a)),
            a.up.mul_scalar(-1.0),
        );
    }
}

/// Curvature above which open road gets a guardrail (rad/m).
const RAIL_CURVATURE: f32 = 0.0012;

#[cfg(test)]
mod tests {
    use super::*;

    fn track() -> Track {
        Track::generate(crate::DEFAULT_SEED, &CourseTuning::DEFAULT)
    }

    #[test]
    fn the_course_divides_into_a_bounded_number_of_chunks() {
        let track = track();
        let count = chunk_count(&track);
        assert!(count > 70 && count < 130, "a 9 km course at 100 m: {count}");
        // The last chunk reaches the end of the course.
        let (_, end) = chunk_sample_range(&track, count - 1);
        assert_eq!(end, track.samples().len() - 1);
    }

    /// The crack-free guarantee, asserted on the real course: chunk `n`'s last
    /// row and chunk `n+1`'s first row are the *same* sample, so their generated
    /// vertices are bit-identical.
    #[test]
    fn adjacent_chunks_share_their_boundary_samples_exactly() {
        let track = track();
        for index in 0..chunk_count(&track) - 1 {
            let (_, end) = chunk_sample_range(&track, index);
            let (next_start, _) = chunk_sample_range(&track, index + 1);
            assert_eq!(
                end, next_start,
                "chunk {index} ends where chunk {} begins",
                index + 1
            );
            assert_eq!(
                track.samples()[end],
                track.samples()[next_start],
                "and it is the same sample, not an equivalent one"
            );
        }
    }

    /// And the consequence, measured on the geometry itself: the last row of
    /// vertices in one chunk's surface matches the first row of the next.
    #[test]
    fn adjacent_chunk_surfaces_meet_without_a_gap() {
        let track = track();
        let t = CourseTuning::DEFAULT;
        for index in [0usize, 1, 17, 40] {
            let here = build_chunk(&track, index, &t);
            let next = build_chunk(&track, index + 1, &t);
            let (_, end) = chunk_sample_range(&track, index);
            let sample = track.samples()[end];
            let seam = sample.at_lateral(-sample.half_width);

            let touches = |data: &MeshData| {
                data.positions()
                    .iter()
                    .any(|p| p.distance(seam) < 1.0e-4)
            };
            assert!(touches(&here.surface), "chunk {index} reaches the seam");
            assert!(touches(&next.surface), "and chunk {} starts there", index + 1);
        }
    }

    #[test]
    fn every_generated_chunk_is_finite_and_non_degenerate() {
        let track = track();
        let t = CourseTuning::DEFAULT;
        for index in 0..chunk_count(&track) {
            let chunk = build_chunk(&track, index, &t);
            for data in [&chunk.surface, &chunk.paint, &chunk.rail, &chunk.verge] {
                for p in data.positions() {
                    assert!(
                        p.x.is_finite() && p.y.is_finite() && p.z.is_finite(),
                        "chunk {index} produced {p:?}"
                    );
                }
                assert_eq!(data.indices().len() % 3, 0);
                assert!(data
                    .indices()
                    .iter()
                    .all(|i| (*i as usize) < data.positions().len()));
            }
            assert!(
                !chunk.surface.positions().is_empty(),
                "chunk {index} has a road surface"
            );
        }
    }

    /// Every road triangle winds the same way — outward, matching the engine's
    /// builtin plane. A single flipped quad is invisible from above on the GPU
    /// and lit from underneath on Canvas2D.
    #[test]
    fn road_triangles_have_consistent_outward_winding() {
        let track = track();
        let t = CourseTuning::DEFAULT;
        for index in [0usize, 5, 33, 61] {
            let chunk = build_chunk(&track, index, &t);
            for (name, data) in [
                ("surface", &chunk.surface),
                ("paint", &chunk.paint),
                ("verge", &chunk.verge),
            ] {
                for tri in data.indices().chunks(3) {
                    let a = data.positions()[tri[0] as usize];
                    let b = data.positions()[tri[1] as usize];
                    let c = data.positions()[tri[2] as usize];
                    let normal = b.subtract(a).cross(c.subtract(a));
                    assert!(
                        normal.y >= -1.0e-6,
                        "chunk {index} {name}: a triangle faces down ({normal:?})"
                    );
                }
                // And the stored normals agree with the winding.
                for (i, tri) in data.indices().chunks(3).enumerate() {
                    let a = data.positions()[tri[0] as usize];
                    let b = data.positions()[tri[1] as usize];
                    let c = data.positions()[tri[2] as usize];
                    let wound = b.subtract(a).cross(c.subtract(a));
                    let stored = data.normals()[tri[0] as usize];
                    assert!(
                        wound.dot(stored) >= -1.0e-6,
                        "chunk {index} {name} triangle {i}: winding and normal disagree"
                    );
                }
            }
        }
    }

    #[test]
    fn the_road_surface_spans_the_full_width_of_the_road() {
        let track = track();
        let chunk = build_chunk(&track, 12, &CourseTuning::DEFAULT);
        let (start, end) = chunk_sample_range(&track, 12);
        let widest = track.samples()[start..=end]
            .iter()
            .map(|s| s.half_width + track.shoulder())
            .fold(0.0f32, f32::max);
        // Every surface vertex is within the road's own width of the centreline.
        for p in chunk.surface.positions() {
            let (_, lateral) = track.localise(*p, track.samples()[start].distance, 200.0);
            assert!(
                lateral.abs() <= widest + 0.5,
                "a surface vertex at {lateral} m is outside the road"
            );
        }
    }

    /// The ground must be continuous from the tarmac outward. A gap between the
    /// paved shoulder and the verge is a hole either side of the road - exactly
    /// where a player who has run wide is looking.
    #[test]
    fn the_ground_is_continuous_from_the_tarmac_to_the_scenery_line() {
        let track = track();
        let t = CourseTuning::DEFAULT;
        for index in [0usize, 9, 31, 55] {
            let chunk = build_chunk(&track, index, &t);
            let (start, _) = chunk_sample_range(&track, index);
            let sample = track.samples()[start];
            let paved_edge = sample.half_width + track.shoulder();
            let laterals: Vec<f32> = chunk
                .verge
                .positions()
                .iter()
                .map(|p| track.localise(*p, sample.distance, 260.0).1.abs())
                .collect();

            let verge_inner = laterals.iter().copied().fold(f32::INFINITY, f32::min);
            assert!(
                verge_inner <= paved_edge + 0.5,
                "chunk {index}: the ground starts at {verge_inner} m but the paving ends at {paved_edge} m"
            );
            let verge_outer = laterals.iter().copied().fold(0.0f32, f32::max);
            assert!(
                verge_outer > track.barrier_offset(&sample),
                "chunk {index}: the ground stops at {verge_outer} m, inside the barrier"
            );
        }
    }

    /// The near-coplanar road layers are separated by enough to survive the
    /// depth buffer at the far end of the drawn road.
    #[test]
    fn the_road_layers_are_separated_enough_to_beat_depth_precision() {
        assert!(PAINT_LIFT.y >= 0.05, "the paint sits proud of the tarmac");
        assert!(SHOULDER_DROP >= 0.05, "and the shoulder sits below it");
        assert!(VERGE_DROP > SHOULDER_DROP, "and the verge below that");
        assert!(PAINT_LIFT.y < 0.15 && VERGE_DROP < 0.3, "none of it is a visible step");
    }

    #[test]
    fn tunnels_get_walls_and_a_roof_and_open_road_does_not() {
        let track = track();
        let t = CourseTuning::DEFAULT;
        let tunnel_chunk = (0..chunk_count(&track))
            .find(|i| {
                let (start, _) = chunk_sample_range(&track, *i);
                track.samples()[start].section.walled()
            })
            .expect("the course has a tunnel");
        let tunnel = build_chunk(&track, tunnel_chunk, &t);
        let highest = tunnel
            .rail
            .positions()
            .iter()
            .map(|p| p.y)
            .fold(f32::NEG_INFINITY, f32::max);
        let (start, _) = chunk_sample_range(&track, tunnel_chunk);
        let road_y = track.samples()[start].position.y;
        assert!(
            highest > road_y + TUNNEL_HEIGHT * 0.8,
            "the tunnel has a roof: {highest} vs road {road_y}"
        );

        // The opening straight is dead straight and open, so no rail at all.
        let opening = build_chunk(&track, 0, &t);
        assert!(
            opening.rail.positions().is_empty(),
            "the start straight has no guardrail"
        );
    }

    #[test]
    fn lane_dashes_appear_and_are_spaced_by_distance_not_by_sample() {
        let track = track();
        let t = CourseTuning::DEFAULT;
        // Count the distinct along-course positions a dash was emitted at.
        let dashes: Vec<f32> = track
            .samples()
            .iter()
            .take(400)
            .filter(|s| s.distance.rem_euclid(t.dash_period) < t.dash_length)
            .map(|s| s.distance)
            .collect();
        assert!(!dashes.is_empty(), "there are dashes");
        // Their runs repeat at exactly the dash period.
        let starts: Vec<f32> = dashes
            .windows(2)
            .filter(|w| w[1] - w[0] > track.spacing() * 1.5)
            .map(|w| w[1])
            .collect();
        for w in starts.windows(2) {
            assert!(
                (w[1] - w[0] - t.dash_period).abs() < track.spacing() + 1.0e-3,
                "dash period drifted: {} -> {}",
                w[0],
                w[1]
            );
        }
        let painted = build_chunk(&track, 0, &t);
        assert!(!painted.paint.positions().is_empty(), "the paint mesh is built");
    }

    #[test]
    fn chunk_ranges_clamp_at_the_end_of_the_course() {
        let track = track();
        let beyond = chunk_count(&track) + 50;
        let (start, end) = chunk_sample_range(&track, beyond);
        assert_eq!(start, track.samples().len() - 1);
        assert_eq!(end, track.samples().len() - 1);
        // Building it yields empty geometry rather than panicking.
        let chunk = build_chunk(&track, beyond, &CourseTuning::DEFAULT);
        assert!(chunk.surface.positions().is_empty());
    }

    /// The grain is tiled in **metres**, not once per quad.
    ///
    /// The failure this pins is silent and was live: with the builder's default
    /// `0..1` mapping every paved quad carried exactly one copy of the texture,
    /// so a 32-texel grain authored as decimetre aggregate rendered as metre-wide
    /// blotches smeared across an 18 m × 2 m panel — and repeated identically
    /// every 2 m. Both symptoms are the same number, so both are asserted here:
    /// the tarmac's UV span in each axis is its world span over `TILE_METRES`.
    #[test]
    fn the_paving_grain_is_tiled_in_metres_rather_than_once_per_quad() {
        let track = track();
        let chunk = build_chunk(&track, 0, &CourseTuning::DEFAULT);
        let uvs = chunk.surface.uvs();
        let (u_lo, u_hi) = uvs
            .iter()
            .fold((f32::MAX, f32::MIN), |(lo, hi), uv| (lo.min(uv.x), hi.max(uv.x)));
        let (v_lo, v_hi) = uvs
            .iter()
            .fold((f32::MAX, f32::MIN), |(lo, hi), uv| (lo.min(uv.y), hi.max(uv.y)));

        // Across: the paved width (tarmac + both shoulders) in tiles.
        let (start, end) = chunk_sample_range(&track, 0);
        let paved = track.samples()[start..=end]
            .iter()
            .map(|s| (s.half_width + track.shoulder()) * 2.0)
            .fold(0.0f32, f32::max);
        assert!(
            ((u_hi - u_lo) - paved / TILE_METRES).abs() < 0.1,
            "the grain spans {:.1} tiles across a {paved:.1} m road; at {TILE_METRES} m \
             per tile it should span {:.1}",
            u_hi - u_lo,
            paved / TILE_METRES
        );
        // Along: a 100 m chunk is many tiles, not one.
        assert!(
            v_hi - v_lo > CHUNK_LENGTH / TILE_METRES - 2.0,
            "the grain repeats only {:.1} times over a {CHUNK_LENGTH} m chunk",
            v_hi - v_lo
        );
        // And it is anchored to absolute course distance, so chunk 30 does not
        // restart the pattern chunk 0 already drew.
        let later = build_chunk(&track, 30, &CourseTuning::DEFAULT);
        let later_v = later.surface.uvs().iter().map(|uv| uv.y).fold(f32::MIN, f32::max);
        assert!(later_v > v_hi, "the mapping restarts at every chunk: {later_v} vs {v_hi}");
    }

    /// Adjacent chunks share their boundary sample, so the grain cannot crack at
    /// a chunk join — the same guarantee the positions have, on the UVs.
    #[test]
    fn the_paving_uvs_meet_exactly_across_a_chunk_boundary() {
        let track = track();
        let t = CourseTuning::DEFAULT;
        let here = build_chunk(&track, 7, &t);
        let next = build_chunk(&track, 8, &t);
        let (_, end) = chunk_sample_range(&track, 7);
        let seam = track.samples()[end].distance / TILE_METRES;
        let touches = |data: &MeshData| data.uvs().iter().any(|uv| (uv.y - seam).abs() < 1.0e-3);
        assert!(touches(&here.surface), "chunk 7 reaches the seam's v");
        assert!(touches(&next.surface), "and chunk 8 starts at the same v");
    }

    #[test]
    fn chunk_generation_is_deterministic() {
        let track = track();
        let t = CourseTuning::DEFAULT;
        let a = build_chunk(&track, 22, &t);
        let b = build_chunk(&track, 22, &t);
        assert_eq!(a.surface.positions(), b.surface.positions());
        assert_eq!(a.paint.indices(), b.paint.indices());
        assert_eq!(a.rail.positions(), b.rail.positions());
        assert_eq!(a.verge.positions(), b.verge.positions());
    }
}
