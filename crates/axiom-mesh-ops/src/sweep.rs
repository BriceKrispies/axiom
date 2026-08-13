//! Sweeping a 2D profile along a 3D curve.
//!
//! A sweep is the operator behind pipes, cables, rails, tunnels, extruded
//! mouldings, ribbons and roads: take a cross-section, carry it along a path,
//! and skin the result. The whole difficulty is in *how* the cross-section is
//! carried, which is why the framing policy is its own module
//! ([`crate::sweep_frames`]) and this one is only placement and topology.
//!
//! ## What this module also owns
//!
//! A sweep, a loft and a revolution are all the same shape of construction: an
//! ordered sequence of *rings* (one placed copy of a profile each), stitched
//! into quads span by span, optionally wrapped end-to-end, and optionally capped
//! at the two ends. Those mechanics —
//! [`oriented_ccw`], [`column_points`], [`column_arc`], [`column_normals`],
//! [`stitch_rings`] and [`cap_mesh`] — live here, crate-visible, because a sweep
//! is the canonical ring-lattice operator. `loft` and `revolve` call them rather
//! than re-deriving the winding and the seam rule and risking disagreeing with
//! the sweep about either.
//!
//! ## Conventions this operator commits to
//!
//! - **The profile is normalised to counter-clockwise** before anything else, so
//!   a caller who authored their cross-section the other way round still gets
//!   outward-facing triangles rather than a mesh that is inside out.
//! - **A closed profile's seam vertex is duplicated**, so `u` reaches both `0`
//!   and `1` and a texture wraps around the cross-section without a shear.
//! - **`u` is normalised cumulative perimeter** around the profile — not the
//!   point index — so an unevenly-tessellated cross-section does not smear its
//!   texture.
//! - **`v` is normalised cumulative *arc length* along the path**, taken from
//!   [`axiom_math::CurveSample::distance`] rather than the sample index. On a
//!   curve those two differ wherever the parameterisation is not unit-speed, and
//!   using the index is what makes a swept texture stretch through tight corners.
//! - **Normals come from the frame basis**, not from the emitted triangles: the
//!   cross-section's own outward normal, rotated by the twist and mapped through
//!   the frame. Shading is therefore independent of how densely the path was
//!   sampled, which a triangle-averaged normal would not be. It is exact for an
//!   untwisted, untapered sweep; under a strong taper the true surface normal
//!   tilts toward the path and a caller who needs that can run
//!   [`axiom_mesh::generate_normals`] over the result.

use axiom_kernel::{Radians, Ratio};
use axiom_math::{Curve, Vec2, Vec3};
use axiom_mesh::{combine, Mesh, MeshError, MeshErrorCode, MeshResult, MeshStreams};

use crate::cap_policy::CapPolicy;
use crate::polygon_triangulation::triangulate_profile;
use crate::profile::{Profile, ProfileWinding};
use crate::sweep_frames::{parallel_transport_frames, SweepFrame};
use crate::tessellation::Samples;

/// Below this a measured length is treated as zero and the quantity it would
/// have normalised is left un-normalised instead of dividing by ~0.
const LENGTH_EPSILON: f32 = 1.0e-9;

/// How a profile is carried along its path.
///
/// Every field is a deliberate, explicit choice; there is no "auto" anywhere,
/// because a generator that guesses is a generator whose output cannot be
/// reproduced from its inputs.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SweepOptions {
    /// Which ends of an open sweep are closed off. **Ignored** when
    /// `closed_path` is set (a loop has no ends) and when the profile is open
    /// (a polyline encloses no area to cap).
    pub caps: CapPolicy,
    /// Total rotation of the cross-section about the path, applied
    /// proportionally to normalised arc length: `0` at the start, the full angle
    /// at the end.
    pub twist: Radians,
    /// Uniform cross-section scale at the start of the path.
    pub start_scale: Ratio,
    /// Uniform cross-section scale at the end of the path, interpolated
    /// linearly from `start_scale` by normalised arc length.
    pub end_scale: Ratio,
    /// Whether the last ring joins back to the first. The path itself is
    /// **not** closed by this flag — supply a path whose end approaches its
    /// start without repeating it, and the final span bridges the two. No ring
    /// is duplicated, so a closed sweep has exactly as many vertices as an open
    /// one with the same sample count. The cost of that is a `v` discontinuity
    /// across the wrap span (`v` runs back from `1` to `0`), which is harmless
    /// for a tiling texture and visible for a clamped one.
    pub closed_path: bool,
    /// The direction the cross-section's local `+X` prefers to point at the
    /// start of the path. `Vec3::ZERO` asks for the deterministic fallback; see
    /// [`parallel_transport_frames`].
    pub initial_reference: Vec3,
}

impl Default for SweepOptions {
    fn default() -> Self {
        SweepOptions {
            caps: CapPolicy::default(),
            twist: Radians::finite_or_zero(0.0),
            start_scale: Ratio::finite_or_zero(1.0),
            end_scale: Ratio::finite_or_zero(1.0),
            closed_path: false,
            initial_reference: Vec3::UNIT_Y,
        }
    }
}

/// Sweep `profile` along `path`, sampled at `samples` arc-length-uniform
/// stations.
///
/// # Errors
///
/// - [`MeshErrorCode::InvalidPath`] when the path cannot be sampled into a
///   frameable set of stations — a curve whose derivative vanishes at a station
///   has no tangent there, so no cross-section can be placed.
/// - [`MeshErrorCode::DegenerateAxis`] when `options.initial_reference` is not
///   finite.
/// - [`MeshErrorCode::InvalidProfile`] from cap triangulation, and any mesh
///   validation failure from the assembled result.
pub fn sweep(
    profile: &Profile,
    path: &Curve,
    samples: Samples,
    options: SweepOptions,
) -> MeshResult<Mesh> {
    path.sample_uniform(samples.get())
        .ok()
        .ok_or_else(|| {
            MeshError::new(
                MeshErrorCode::InvalidPath,
                "a sweep path must sample into stations that each have a defined tangent",
            )
        })
        .and_then(|stations| {
            parallel_transport_frames(&stations, options.initial_reference)
                .map(|frames| (stations, frames))
        })
        .and_then(|(stations, frames)| {
            let total = stations.last().map_or(0.0, |s| s.distance().get());
            let denom = [1.0, total][usize::from(total > LENGTH_EPSILON)];
            let progress: Vec<f32> = stations
                .iter()
                .map(|s| s.distance().get() / denom)
                .collect();
            build_sweep(profile, &frames, &progress, options)
        })
}

/// Place every ring, skin them, and add whatever caps the policy asks for.
fn build_sweep(
    profile: &Profile,
    frames: &[SweepFrame],
    progress: &[f32],
    options: SweepOptions,
) -> MeshResult<Mesh> {
    let oriented = oriented_ccw(profile);
    let columns = column_points(&oriented);
    let across = column_arc(&columns);
    let outward = column_normals(&oriented);
    let rings: Vec<Vec<Vec3>> = frames
        .iter()
        .zip(progress.iter())
        .map(|(frame, t)| place_ring(frame, &columns, *t, options))
        .collect();
    let normals: Vec<Vec<Vec3>> = frames
        .iter()
        .zip(progress.iter())
        .map(|(frame, t)| place_normals(frame, &outward, *t, options))
        .collect();
    let uvs: Vec<Vec<Vec2>> = progress
        .iter()
        .map(|v| across.iter().map(|u| Vec2::new(*u, *v)).collect())
        .collect();
    Mesh::from_streams(stitch_rings(&rings, &normals, &uvs, options.closed_path))
        .and_then(|side| {
            sweep_caps(&oriented, &rings, frames, options).map(|caps| (side, caps))
        })
        .and_then(|(side, caps)| {
            combine(&core::iter::once(side).chain(caps).collect::<Vec<Mesh>>())
        })
}

/// Scale and twist interpolated to `t`, then mapped into the frame basis.
fn place_ring(frame: &SweepFrame, columns: &[Vec2], t: f32, options: SweepOptions) -> Vec<Vec3> {
    let start = options.start_scale.get();
    let scale = start + (options.end_scale.get() - start) * t;
    let (sine, cosine) = twist_at(options.twist, t);
    columns
        .iter()
        .map(|p| {
            let x = (p.x * cosine - p.y * sine) * scale;
            let y = (p.x * sine + p.y * cosine) * scale;
            frame
                .position()
                .add(frame.normal().mul_scalar(x))
                .add(frame.binormal().mul_scalar(y))
        })
        .collect()
}

/// The cross-section's outward normals, twisted the same way the points were and
/// mapped into the frame basis. Uniform scale does not change a normal, so the
/// scale factor deliberately does not appear here.
fn place_normals(
    frame: &SweepFrame,
    outward: &[Vec2],
    t: f32,
    options: SweepOptions,
) -> Vec<Vec3> {
    let (sine, cosine) = twist_at(options.twist, t);
    outward
        .iter()
        .map(|n| {
            let x = n.x * cosine - n.y * sine;
            let y = n.x * sine + n.y * cosine;
            frame
                .normal()
                .mul_scalar(x)
                .add(frame.binormal().mul_scalar(y))
        })
        .collect()
}

fn twist_at(twist: Radians, t: f32) -> (f32, f32) {
    let angle = twist.get() * t;
    (angle.sin(), angle.cos())
}

/// The zero, one or two end caps this sweep wants.
fn sweep_caps(
    oriented: &Profile,
    rings: &[Vec<Vec3>],
    frames: &[SweepFrame],
    options: SweepOptions,
) -> MeshResult<Vec<Mesh>> {
    let eligible = !options.closed_path & oriented.is_closed();
    let wanted = [
        eligible & options.caps.caps_start(),
        eligible & options.caps.caps_end(),
    ];
    (wanted[0] | wanted[1])
        .then(|| triangulate_profile(oriented))
        .transpose()
        .map(Option::unwrap_or_default)
        .and_then(|triangles| {
            let ends = [0usize, rings.len() - 1];
            (0..2)
                .filter(|end| wanted[*end])
                .map(|end| {
                    let at = ends[end];
                    // The start cap looks back down the path; the end cap along it.
                    let leading = end == 0;
                    let facing = frames[at]
                        .tangent()
                        .mul_scalar([1.0, -1.0][usize::from(leading)]);
                    cap_mesh(
                        &rings[at][..oriented.point_count()],
                        oriented.points(),
                        &triangles,
                        facing,
                        leading,
                    )
                })
                .collect()
        })
}

/// `profile`, or its reverse when it winds clockwise.
///
/// Every ring-lattice operator works exclusively in counter-clockwise profile
/// space so the emitted triangle winding can be derived once, here, instead of
/// carrying a sign through every quad.
pub(crate) fn oriented_ccw(profile: &Profile) -> Profile {
    matches!(profile.winding(), ProfileWinding::CounterClockwise)
        .then(|| profile.clone())
        .unwrap_or_else(|| profile.reversed())
}

/// The profile's points as lattice columns: one per point, plus a duplicate of
/// the first when the profile is closed, so the wrap seam carries `u = 1`.
pub(crate) fn column_points(profile: &Profile) -> Vec<Vec2> {
    let count = profile.point_count();
    (0..count + usize::from(profile.is_closed()))
        .map(|j| profile.points()[j % count])
        .collect()
}

/// Normalised cumulative perimeter across the columns: `0` at the first,
/// `1` at the last.
pub(crate) fn column_arc(columns: &[Vec2]) -> Vec<f32> {
    let running: Vec<f32> = columns
        .windows(2)
        .scan(0.0f32, |acc, w| {
            *acc += w[0].distance(w[1]);
            Some(*acc)
        })
        .collect();
    let total = running.last().copied().unwrap_or(0.0);
    let denom = [1.0, total][usize::from(total > LENGTH_EPSILON)];
    core::iter::once(0.0)
        .chain(running.iter().map(|d| d / denom))
        .collect()
}

/// The 2D outward normal at every column: the normalised average of the edge
/// normals meeting there, so a swept circle shades smoothly and a swept polygon
/// shades as the smooth surface it is a discretisation of.
pub(crate) fn column_normals(profile: &Profile) -> Vec<Vec2> {
    let points = profile.points();
    let count = points.len();
    let closed = profile.is_closed();
    (0..count + usize::from(closed))
        .map(|j| {
            let here = j % count;
            let before = [here.saturating_sub(1), (here + count - 1) % count][usize::from(closed)];
            let after = [(here + 1).min(count - 1), (here + 1) % count][usize::from(closed)];
            let incoming = edge_normal(points[before], points[here]);
            let outgoing = edge_normal(points[here], points[after]);
            incoming
                .add(outgoing)
                .normalize()
                .unwrap_or(outgoing)
        })
        .collect()
}

/// The outward normal of a counter-clockwise edge `a -> b`. A zero-length edge
/// (an open profile's clamped end) contributes nothing to the average.
fn edge_normal(a: Vec2, b: Vec2) -> Vec2 {
    let d = b.subtract(a);
    Vec2::new(d.y, -d.x).normalize().unwrap_or(Vec2::ZERO)
}

/// Skin an ordered lattice of equal-length rings into a triangle mesh.
///
/// Ring `i`, column `j` is vertex `i * columns + j`. Each cell emits the quad
/// `(i, j) (i, j+1) (i+1, j+1) (i+1, j)` as two triangles, which is
/// counter-clockwise-front for counter-clockwise profile columns advancing along
/// the ring order. `wrap` adds one more span joining the last ring back to the
/// first **without** appending a duplicate ring.
///
/// Preconditions, guaranteed by every caller in this layer: at least two rings,
/// at least two columns, and every ring the same length as the first.
pub(crate) fn stitch_rings(
    positions: &[Vec<Vec3>],
    normals: &[Vec<Vec3>],
    uvs: &[Vec<Vec2>],
    wrap: bool,
) -> MeshStreams {
    let rows = positions.len();
    let columns = positions[0].len();
    let spans = rows - 1 + usize::from(wrap);
    let indices = (0..spans)
        .flat_map(|i| {
            let near = i * columns;
            let far = ((i + 1) % rows) * columns;
            (0..columns - 1).flat_map(move |j| {
                let (a, b) = ((near + j) as u32, (near + j + 1) as u32);
                let (c, d) = ((far + j + 1) as u32, (far + j) as u32);
                [a, b, c, a, c, d]
            })
        })
        .collect();
    MeshStreams {
        normals: normals.iter().flatten().copied().collect(),
        uvs: uvs.iter().flatten().copied().collect(),
        ..MeshStreams::new(
            positions.iter().flatten().copied().collect(),
            indices,
        )
    }
}

/// One flat end cap: the already-placed ring, triangulated in profile space.
///
/// `triangles` index `profile_points` (and therefore `ring`) directly.
/// `reverse` flips the winding, which is what a start cap needs: the same
/// polygon that faces `+tangent` at the end of a sweep faces `-tangent` at its
/// start.
pub(crate) fn cap_mesh(
    ring: &[Vec3],
    profile_points: &[Vec2],
    triangles: &[[u32; 3]],
    facing: Vec3,
    reverse: bool,
) -> MeshResult<Mesh> {
    let indices: Vec<u32> = triangles
        .iter()
        .flat_map(|t| [[t[0], t[1], t[2]], [t[0], t[2], t[1]]][usize::from(reverse)])
        .collect();
    Mesh::from_streams(MeshStreams {
        normals: vec![facing; ring.len()],
        uvs: cap_uvs(profile_points),
        ..MeshStreams::new(ring.to_vec(), indices)
    })
}

/// Cap UVs: the profile's own bounding box remapped to the unit square, so a cap
/// texture covers the cross-section regardless of its world size.
fn cap_uvs(points: &[Vec2]) -> Vec<Vec2> {
    let (low, high) = points.iter().fold(
        (
            Vec2::new(f32::INFINITY, f32::INFINITY),
            Vec2::new(f32::NEG_INFINITY, f32::NEG_INFINITY),
        ),
        |(low, high), p| {
            (
                Vec2::new(low.x.min(p.x), low.y.min(p.y)),
                Vec2::new(high.x.max(p.x), high.y.max(p.y)),
            )
        },
    );
    let extent = high.subtract(low);
    let width = [1.0, extent.x][usize::from(extent.x > LENGTH_EPSILON)];
    let height = [1.0, extent.y][usize::from(extent.y > LENGTH_EPSILON)];
    points
        .iter()
        .map(|p| Vec2::new((p.x - low.x) / width, (p.y - low.y) / height))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_kernel::Meters;
    use crate::tessellation::Segments;

    fn meters(v: f32) -> Meters {
        Meters::new(v).unwrap()
    }

    fn ratio(v: f32) -> Ratio {
        Ratio::new(v).unwrap()
    }

    fn circle(radius: f32, segments: u32) -> Profile {
        Profile::circle(meters(radius), Segments::new(segments).unwrap()).unwrap()
    }

    fn straight_z(length: f32) -> Curve {
        Curve::polyline(vec![Vec3::ZERO, Vec3::new(0.0, 0.0, length)]).unwrap()
    }

    fn samples(n: u32) -> Samples {
        Samples::new(n).unwrap()
    }

    fn open_options() -> SweepOptions {
        SweepOptions {
            caps: CapPolicy::None,
            ..SweepOptions::default()
        }
    }

    /// Signed volume test that every triangle faces away from the given interior
    /// point.
    fn faces_outward(mesh: &Mesh, interior: Vec3) -> bool {
        mesh.indices().chunks(3).all(|t| {
            let p = mesh.positions();
            let (a, b, c) = (
                p[t[0] as usize],
                p[t[1] as usize],
                p[t[2] as usize],
            );
            let out = a
                .add(b)
                .add(c)
                .mul_scalar(1.0 / 3.0)
                .subtract(interior);
            let geometric = b.subtract(a).cross(c.subtract(a));
            // Degenerate triangles carry no orientation and are not judged.
            geometric.length() < 1.0e-9 || geometric.dot(out) > 0.0
        })
    }

    #[test]
    fn sweeping_a_circle_along_a_line_is_a_cylinder() {
        let mesh = sweep(&circle(1.0, 12), &straight_z(4.0), samples(5), open_options())
            .unwrap();
        assert_eq!(mesh.vertex_count(), 5 * 13);
        assert_eq!(mesh.triangle_count(), 4 * 12 * 2);
        for p in mesh.positions() {
            let radial = (p.x * p.x + p.y * p.y).sqrt();
            assert!((radial - 1.0).abs() < 1.0e-4, "radius {radial}");
            assert!(p.z >= -1.0e-4 && p.z <= 4.0 + 1.0e-4);
        }
        assert!(faces_outward(&mesh, Vec3::new(0.0, 0.0, 2.0)));
        assert!(mesh.has_normals());
        assert!(mesh.has_uvs());
        for (p, n) in mesh.positions().iter().zip(mesh.normals()) {
            let radial = Vec3::new(p.x, p.y, 0.0).normalize().unwrap();
            assert!(n.subtract(radial).length() < 1.0e-4);
        }
    }

    #[test]
    fn u_runs_the_full_zero_to_one_across_the_duplicated_seam() {
        let mesh = sweep(&circle(1.0, 8), &straight_z(2.0), samples(3), open_options())
            .unwrap();
        let first_ring = &mesh.uvs()[..9];
        assert!(first_ring[0].x.abs() < 1.0e-6);
        assert!((first_ring[8].x - 1.0).abs() < 1.0e-5);
        // Evenly-spaced circle points give evenly-spaced u.
        for (i, uv) in first_ring.iter().enumerate() {
            assert!((uv.x - i as f32 / 8.0).abs() < 1.0e-5);
        }
        // The seam duplicate is the same position with a different u.
        assert!(mesh.positions()[0].subtract(mesh.positions()[8]).length() < 1.0e-6);
    }

    #[test]
    fn v_tracks_arc_length_not_sample_index() {
        // A Catmull-Rom whose parameterisation is far from unit speed: a
        // parameter-uniform v would drift badly from the measured geometry.
        let curve = Curve::catmull_rom(vec![
            Vec3::new(-1.0, 0.0, 0.0),
            Vec3::ZERO,
            Vec3::new(6.0, 0.0, 0.0),
            Vec3::new(7.0, 4.0, 0.0),
            Vec3::new(7.5, 9.0, 0.0),
        ])
        .unwrap();
        let count = 24;
        let mesh = sweep(&circle(0.2, 6), &curve, samples(count), open_options()).unwrap();
        let columns = 7;
        // Ring centres, and the cumulative chord length through them.
        let centres: Vec<Vec3> = (0..count as usize)
            .map(|i| {
                mesh.positions()[i * columns..i * columns + 6]
                    .iter()
                    .fold(Vec3::ZERO, |acc, p| acc.add(*p))
                    .mul_scalar(1.0 / 6.0)
            })
            .collect();
        let mut cumulative = vec![0.0f32];
        for w in centres.windows(2) {
            let last = *cumulative.last().unwrap();
            cumulative.push(last + w[0].distance(w[1]));
        }
        let total = *cumulative.last().unwrap();
        for i in 0..count as usize {
            let v = mesh.uvs()[i * columns].y;
            let fraction = cumulative[i] / total;
            assert!((v - fraction).abs() < 5.0e-3, "ring {i}: v = {v}, arc = {fraction}");
        }
        assert!(mesh.uvs()[0].y.abs() < 1.0e-6);
        assert!((mesh.uvs().last().unwrap().y - 1.0).abs() < 1.0e-5);
    }

    #[test]
    fn caps_close_a_closed_profile_on_an_open_path() {
        let bare = sweep(&circle(1.0, 8), &straight_z(2.0), samples(3), open_options()).unwrap();
        let both = sweep(
            &circle(1.0, 8),
            &straight_z(2.0),
            samples(3),
            SweepOptions {
                caps: CapPolicy::Both,
                ..SweepOptions::default()
            },
        )
        .unwrap();
        assert_eq!(bare.vertex_count() + 16, both.vertex_count());
        assert_eq!(bare.triangle_count() + 12, both.triangle_count());
        assert!(faces_outward(&both, Vec3::new(0.0, 0.0, 1.0)));
    }

    #[test]
    fn each_single_ended_cap_policy_adds_exactly_one_cap() {
        let count = |caps| {
            sweep(
                &circle(1.0, 8),
                &straight_z(2.0),
                samples(3),
                SweepOptions {
                    caps,
                    ..SweepOptions::default()
                },
            )
            .unwrap()
            .triangle_count()
        };
        let bare = count(CapPolicy::None);
        assert_eq!(count(CapPolicy::Start), bare + 6);
        assert_eq!(count(CapPolicy::End), bare + 6);
        assert_eq!(count(CapPolicy::Both), bare + 12);
    }

    #[test]
    fn a_start_cap_faces_backwards_along_the_path() {
        let mesh = sweep(
            &circle(1.0, 6),
            &straight_z(2.0),
            samples(2),
            SweepOptions {
                caps: CapPolicy::Both,
                ..SweepOptions::default()
            },
        )
        .unwrap();
        let side = 2 * 7;
        assert!(mesh.normals()[side].subtract(Vec3::new(0.0, 0.0, -1.0)).length() < 1.0e-5);
        assert!(mesh.normals()[side + 6].subtract(Vec3::UNIT_Z).length() < 1.0e-5);
        assert!(faces_outward(&mesh, Vec3::new(0.0, 0.0, 1.0)));
        // Cap UVs remap the circle's bounding box onto the unit square.
        for uv in &mesh.uvs()[side..] {
            assert!(uv.x >= -1.0e-6 && uv.x <= 1.0 + 1.0e-6);
            assert!(uv.y >= -1.0e-6 && uv.y <= 1.0 + 1.0e-6);
        }
    }

    #[test]
    fn an_open_profile_ignores_the_cap_policy() {
        let ribbon = Profile::open(vec![
            Vec2::new(-1.0, 0.0),
            Vec2::new(0.0, 0.4),
            Vec2::new(1.0, 0.0),
        ])
        .unwrap();
        let capped = sweep(
            &ribbon,
            &straight_z(2.0),
            samples(3),
            SweepOptions {
                caps: CapPolicy::Both,
                ..SweepOptions::default()
            },
        )
        .unwrap();
        // 3 columns (no seam duplicate), 3 rings, 2 spans x 2 quads x 2 tris.
        assert_eq!(capped.vertex_count(), 9);
        assert_eq!(capped.triangle_count(), 8);
        assert!(capped.normals().iter().all(|n| (n.length() - 1.0).abs() < 1.0e-4));
    }

    #[test]
    fn a_closed_path_wraps_without_duplicating_the_final_ring() {
        // A square loop in XZ whose last point does not repeat the first.
        let loop_path = Curve::polyline(vec![
            Vec3::new(2.0, 0.0, 0.0),
            Vec3::new(0.0, 0.0, 2.0),
            Vec3::new(-2.0, 0.0, 0.0),
            Vec3::new(0.0, 0.0, -2.0),
        ])
        .unwrap();
        let options = SweepOptions {
            caps: CapPolicy::Both,
            closed_path: true,
            ..SweepOptions::default()
        };
        let mesh = sweep(&circle(0.25, 6), &loop_path, samples(12), options).unwrap();
        // 12 rings x 7 columns; caps are ignored, so no cap vertices at all.
        assert_eq!(mesh.vertex_count(), 12 * 7);
        // 12 spans (one more than the open case), not 11.
        assert_eq!(mesh.triangle_count(), 12 * 6 * 2);
        let open = sweep(
            &circle(0.25, 6),
            &loop_path,
            samples(12),
            SweepOptions {
                closed_path: false,
                caps: CapPolicy::None,
                ..SweepOptions::default()
            },
        )
        .unwrap();
        assert_eq!(open.vertex_count(), mesh.vertex_count());
        assert_eq!(open.triangle_count() + 6 * 2, mesh.triangle_count());
        // The final ring is a genuinely different ring, not a copy of the first.
        assert!(mesh.positions()[0].distance(mesh.positions()[11 * 7]) > 0.5);
    }

    #[test]
    fn a_taper_scales_the_last_ring() {
        let mesh = sweep(
            &circle(1.0, 8),
            &straight_z(4.0),
            samples(5),
            SweepOptions {
                caps: CapPolicy::None,
                start_scale: ratio(1.0),
                end_scale: ratio(0.25),
                ..SweepOptions::default()
            },
        )
        .unwrap();
        let radius_of = |ring: usize| {
            let p = mesh.positions()[ring * 9];
            (p.x * p.x + p.y * p.y).sqrt()
        };
        assert!((radius_of(0) - 1.0).abs() < 1.0e-4);
        assert!((radius_of(2) - 0.625).abs() < 1.0e-4);
        assert!((radius_of(4) - 0.25).abs() < 1.0e-4);
    }

    #[test]
    fn a_twist_rotates_the_cross_section_along_the_path() {
        let quarter = Radians::new(core::f32::consts::FRAC_PI_2).unwrap();
        let mesh = sweep(
            &Profile::rectangle(meters(1.0), meters(0.2)).unwrap(),
            &straight_z(4.0),
            samples(3),
            SweepOptions {
                caps: CapPolicy::None,
                twist: quarter,
                ..SweepOptions::default()
            },
        )
        .unwrap();
        // Frame at a +Z tangent with a +Y reference: normal = +Y, binormal = -X.
        // Profile point 1 is (1, -0.2); at the end it has turned a quarter turn.
        // Measure each ring's offset from its own centre on the path.
        let offset = |ring: usize| {
            mesh.positions()[ring * 5 + 1].subtract(Vec3::new(0.0, 0.0, ring as f32 * 2.0))
        };
        let (start, middle, end) = (offset(0), offset(1), offset(2));
        assert!(start.subtract(Vec3::new(0.2, 1.0, 0.0)).length() < 1.0e-5);
        assert!(end.subtract(Vec3::new(-1.0, 0.2, 0.0)).length() < 1.0e-5);
        // A rotation preserves the cross-section's radius at every station.
        assert!((start.length() - end.length()).abs() < 1.0e-4);
        assert!((middle.length() - start.length()).abs() < 1.0e-4);
        assert!(start.subtract(end).length() > 0.5);
    }

    #[test]
    fn a_clockwise_profile_is_normalised_and_still_faces_outward() {
        let clockwise = circle(1.0, 10).reversed();
        assert_eq!(clockwise.winding(), ProfileWinding::Clockwise);
        let mesh = sweep(&clockwise, &straight_z(2.0), samples(3), open_options()).unwrap();
        assert!(faces_outward(&mesh, Vec3::new(0.0, 0.0, 1.0)));
        for (p, n) in mesh.positions().iter().zip(mesh.normals()) {
            let radial = Vec3::new(p.x, p.y, 0.0).normalize().unwrap();
            assert!(n.subtract(radial).length() < 1.0e-4);
        }
    }

    #[test]
    fn a_path_through_vertical_sweeps_without_a_seam_snap() {
        let climb = Curve::catmull_rom(vec![
            Vec3::new(-4.0, -1.0, 0.0),
            Vec3::new(-3.0, 0.0, 0.0),
            Vec3::ZERO,
            Vec3::new(0.0, 3.0, 0.0),
            Vec3::new(0.0, 3.0, 3.0),
            Vec3::new(0.0, 3.0, 4.0),
        ])
        .unwrap();
        let mesh = sweep(&circle(0.5, 8), &climb, samples(40), open_options()).unwrap();
        let columns = 9;
        // Corresponding vertices of consecutive rings stay close: a frame flip
        // would teleport them across the tube, a full diameter (1.0) away.
        for i in 0..39usize {
            for j in 0..columns {
                let a = mesh.positions()[i * columns + j];
                let b = mesh.positions()[(i + 1) * columns + j];
                assert!(a.distance(b) < 0.6, "ring {i} column {j} jumped");
            }
        }
    }

    #[test]
    fn an_unsampleable_path_is_an_invalid_path() {
        // A Catmull-Rom whose first and third control points coincide has a zero
        // derivative at the start of its span: no tangent, so no frame.
        let cusp = Curve::catmull_rom(vec![
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::ZERO,
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(2.0, 1.0, 0.0),
        ])
        .unwrap();
        assert_eq!(
            sweep(&circle(1.0, 6), &cusp, samples(8), open_options())
                .unwrap_err()
                .code(),
            MeshErrorCode::InvalidPath
        );
    }

    #[test]
    fn a_non_finite_reference_is_reported_by_the_sweep_too() {
        assert_eq!(
            sweep(
                &circle(1.0, 6),
                &straight_z(2.0),
                samples(3),
                SweepOptions {
                    initial_reference: Vec3::new(f32::NAN, 0.0, 0.0),
                    ..SweepOptions::default()
                }
            )
            .unwrap_err()
            .code(),
            MeshErrorCode::DegenerateAxis
        );
    }

    #[test]
    fn the_default_options_are_an_untwisted_unscaled_capped_open_sweep() {
        let d = SweepOptions::default();
        assert_eq!(d.caps, CapPolicy::Both);
        assert_eq!(d.twist.get(), 0.0);
        assert_eq!(d.start_scale.get(), 1.0);
        assert_eq!(d.end_scale.get(), 1.0);
        assert!(!d.closed_path);
        assert_eq!(d.initial_reference, Vec3::UNIT_Y);
    }

    #[test]
    fn column_arc_is_perimeter_proportional_not_index_proportional() {
        // A rectangle: the long edges must consume more u than the short ones.
        let rect = Profile::rectangle(meters(4.0), meters(1.0)).unwrap();
        let columns = column_points(&rect);
        assert_eq!(columns.len(), 5);
        let u = column_arc(&columns);
        assert_eq!(u.len(), 5);
        assert!(u[0].abs() < 1.0e-6);
        // Perimeter 2*(8 + 2) = 20; the bottom edge is 8 of it.
        assert!((u[1] - 0.4).abs() < 1.0e-5);
        assert!((u[2] - 0.5).abs() < 1.0e-5);
        assert!((u[4] - 1.0).abs() < 1.0e-5);
    }

    #[test]
    fn column_arc_of_a_zero_length_run_does_not_divide_by_zero() {
        let u = column_arc(&[Vec2::ZERO, Vec2::ZERO, Vec2::ZERO]);
        assert_eq!(u, vec![0.0, 0.0, 0.0]);
    }

    #[test]
    fn an_open_profiles_end_columns_take_their_single_edge_normal() {
        let ribbon = Profile::open(vec![
            Vec2::new(0.0, 0.0),
            Vec2::new(1.0, 0.0),
            Vec2::new(2.0, 0.0),
        ])
        .unwrap();
        let normals = column_normals(&ribbon);
        assert_eq!(normals.len(), 3);
        for n in &normals {
            assert!(n.subtract(Vec2::new(0.0, -1.0)).length() < 1.0e-5);
        }
    }

    #[test]
    fn cap_uvs_of_a_flat_profile_do_not_divide_by_zero() {
        let uvs = cap_uvs(&[Vec2::ZERO, Vec2::new(1.0, 0.0), Vec2::new(2.0, 0.0)]);
        assert_eq!(uvs[0], Vec2::ZERO);
        assert!((uvs[2].x - 1.0).abs() < 1.0e-6);
        assert_eq!(uvs[2].y, 0.0);
    }
}
