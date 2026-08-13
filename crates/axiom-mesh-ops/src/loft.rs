//! Skinning a surface through an ordered series of placed cross-sections.
//!
//! Where a [`crate::sweep`] carries *one* profile along a path, a loft
//! interpolates *between* profiles: the caller places each cross-section
//! explicitly and the operator stretches a skin through them in order. It is the
//! operator behind hulls, wings, bottles, ducts, chair legs and anything whose
//! silhouette is authored as a stack of outlines rather than as a single
//! extruded shape.
//!
//! ## Correspondence is by index. Always. Only.
//!
//! Section `k`'s point `j` connects to section `k + 1`'s point `j`, and nothing
//! else is attempted. There is **no** arc-length rematching, no nearest-point
//! search, no automatic seam rotation, and no resampling of one section to fit
//! another. That is a deliberate contract, not a missing feature:
//!
//! - it is the only correspondence rule that is *stateless and reproducible* —
//!   the mesh is a pure function of the point order the caller supplied;
//!   heuristic rematching would make a loft's topology depend on the geometry it
//!   is given, so a small edit to one section could re-thread the whole surface;
//! - it puts the seam where the author put it, which is what an author wants;
//! - it makes the failure mode loud instead of quiet: mismatched sections are
//!   rejected as [`MeshErrorCode::IncompatibleProfiles`] rather than silently
//!   producing a twisted skin.
//!
//! The consequence a caller must respect: every section needs the **same point
//! count, the same open/closed policy, the same winding, and the same starting
//! point**. Point count and closedness are checked; winding is *normalised*
//! (see below) because that one can be fixed without guessing at correspondence.
//!
//! ## Normals
//!
//! Unlike a sweep, a loft has no frame to read a normal off — consecutive
//! sections may differ in scale, orientation and shape, so the surface genuinely
//! tilts between them. Normals are therefore the true discrete surface normals,
//! `cross(d/d_column, d/d_row)` by central difference over the lattice, which is
//! exact for a cylinder and correct for a taper where a section-plane normal
//! would be wrong. A caller who wants faceted shading runs
//! [`axiom_mesh::generate_flat_normals`] over the result.

use axiom_math::{Transform, Vec2, Vec3};
use axiom_mesh::{combine, Mesh, MeshError, MeshErrorCode, MeshResult};

use crate::cap_policy::CapPolicy;
use crate::polygon_triangulation::triangulate_profile;
use crate::profile::{Profile, ProfileWinding};
use crate::sweep::{cap_mesh, column_arc, column_points, stitch_rings};

/// One cross-section of a loft: an outline, and where it sits in space.
///
/// The profile is authored in its own XY plane and mapped to `(x, y, 0)` before
/// `placement` is applied, so a section's local `+Z` is the direction the loft
/// runs in and its caps face along.
#[derive(Debug, Clone, PartialEq)]
pub struct LoftSection {
    /// The outline, in the section's own XY plane.
    pub profile: Profile,
    /// Where and how that outline sits in world space.
    pub placement: Transform,
}

/// How a series of sections is skinned.
/// The default caps both ends of an open series: a loft of closed outlines is a
/// solid unless the caller says otherwise.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct LoftOptions {
    /// Which ends are closed off. Ignored when `closed_loop` is set (a loop has
    /// no ends) and when the sections are open polylines (nothing to cap).
    pub caps: CapPolicy,
    /// Whether the last section joins back to the first. No section is
    /// duplicated to do it; the wrap span reuses section `0` directly.
    pub closed_loop: bool,
}

/// Skin a surface through `sections`, in the order given.
///
/// # Errors
///
/// - [`MeshErrorCode::InvalidParameter`] with fewer than two sections: one
///   outline is not a surface.
/// - [`MeshErrorCode::IncompatibleProfiles`] when the sections disagree on point
///   count or on open/closed policy, so index correspondence would be a lie.
/// - [`MeshErrorCode::InvalidProfile`] from cap triangulation.
pub fn loft(sections: &[LoftSection], options: LoftOptions) -> MeshResult<Mesh> {
    (sections.len() >= 2)
        .then_some(())
        .ok_or_else(|| {
            MeshError::new(
                MeshErrorCode::InvalidParameter,
                "a loft needs at least two sections",
            )
        })
        .and_then(|()| corresponding(sections))
        .and_then(|()| build_loft(sections, options))
}

/// Every section must present the same indices to correspond on.
fn corresponding(sections: &[LoftSection]) -> MeshResult<()> {
    let first = &sections[0].profile;
    sections
        .iter()
        .all(|s| {
            (s.profile.point_count() == first.point_count())
                & (s.profile.is_closed() == first.is_closed())
        })
        .then_some(())
        .ok_or_else(|| {
            MeshError::new(
                MeshErrorCode::IncompatibleProfiles,
                "every loft section must have the same point count and the same open/closed policy",
            )
        })
}

/// Place every section, skin the lattice, and cap the ends.
fn build_loft(sections: &[LoftSection], options: LoftOptions) -> MeshResult<Mesh> {
    let profiles = normalised_winding(sections);
    let columns: Vec<Vec<Vec2>> = profiles.iter().map(column_points).collect();
    let rings: Vec<Vec<Vec3>> = columns
        .iter()
        .zip(sections.iter())
        .map(|(cols, section)| {
            cols.iter()
                .map(|p| section.placement.transform_point(Vec3::new(p.x, p.y, 0.0)))
                .collect()
        })
        .collect();
    let normals = lattice_normals(&rings, options.closed_loop, profiles[0].is_closed());
    let rows = sections.len();
    let denominator = [(rows - 1) as f32, rows as f32][usize::from(options.closed_loop)];
    let uvs: Vec<Vec<Vec2>> = columns
        .iter()
        .enumerate()
        .map(|(row, cols)| {
            let v = row as f32 / denominator;
            column_arc(cols).iter().map(|u| Vec2::new(*u, v)).collect()
        })
        .collect();
    Mesh::from_streams(stitch_rings(&rings, &normals, &uvs, options.closed_loop))
        .and_then(|side| {
            loft_caps(&profiles, sections, &rings, options).map(|caps| (side, caps))
        })
        .and_then(|(side, caps)| {
            combine(&core::iter::once(side).chain(caps).collect::<Vec<Mesh>>())
        })
}

/// Every section reversed, or none of them.
///
/// Reversing one section and not another would silently re-thread the
/// correspondence, so winding is normalised for the whole series on the evidence
/// of the first section — the one whose point order the rest are matched
/// against anyway.
fn normalised_winding(sections: &[LoftSection]) -> Vec<Profile> {
    let reverse = !matches!(
        sections[0].profile.winding(),
        ProfileWinding::CounterClockwise
    );
    sections
        .iter()
        .map(|s| {
            reverse
                .then(|| s.profile.reversed())
                .unwrap_or_else(|| s.profile.clone())
        })
        .collect()
}

/// True discrete surface normals over the placed lattice.
///
/// `cross(around, along)` matches the winding [`stitch_rings`] emits, so a
/// normal always agrees with the triangle it belongs to. Differences are central
/// where they can be and one-sided at an open boundary.
fn lattice_normals(
    rings: &[Vec<Vec3>],
    closed_loop: bool,
    closed_profile: bool,
) -> Vec<Vec<Vec3>> {
    let rows = rings.len();
    let columns = rings[0].len();
    let distinct = columns - usize::from(closed_profile);
    (0..rows)
        .map(|row| {
            let back = [row.saturating_sub(1), (row + rows - 1) % rows][usize::from(closed_loop)];
            let ahead = [(row + 1).min(rows - 1), (row + 1) % rows][usize::from(closed_loop)];
            (0..columns)
                .map(|column| {
                    let here = column % distinct;
                    let left = [here.saturating_sub(1), (here + distinct - 1) % distinct]
                        [usize::from(closed_profile)];
                    let right = [(here + 1).min(distinct - 1), (here + 1) % distinct]
                        [usize::from(closed_profile)];
                    let around = rings[row][right].subtract(rings[row][left]);
                    let along = rings[ahead][here].subtract(rings[back][here]);
                    around.cross(along).normalize().unwrap_or(Vec3::UNIT_Y)
                })
                .collect()
        })
        .collect()
}

/// The zero, one or two end caps this loft wants, each triangulated from its own
/// end section and facing along that section's local `+Z`.
fn loft_caps(
    profiles: &[Profile],
    sections: &[LoftSection],
    rings: &[Vec<Vec3>],
    options: LoftOptions,
) -> MeshResult<Vec<Mesh>> {
    let eligible = !options.closed_loop & profiles[0].is_closed();
    let wanted = [
        eligible & options.caps.caps_start(),
        eligible & options.caps.caps_end(),
    ];
    let ends = [0usize, profiles.len() - 1];
    (0..2)
        .filter(|end| wanted[*end])
        .map(|end| {
            let at = ends[end];
            let axis = sections[at]
                .placement
                .rotation
                .rotate(Vec3::UNIT_Z)
                .normalize()
                .unwrap_or(Vec3::UNIT_Z);
            let facing = axis.mul_scalar([1.0, -1.0][1 - end]);
            triangulate_profile(&profiles[at]).and_then(|triangles| {
                cap_mesh(
                    &rings[at][..profiles[at].point_count()],
                    profiles[at].points(),
                    &triangles,
                    facing,
                    end == 0,
                )
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_kernel::Meters;
    use axiom_math::Quat;

    use crate::tessellation::Segments;

    fn meters(v: f32) -> Meters {
        Meters::new(v).unwrap()
    }

    fn square() -> Profile {
        Profile::rectangle(meters(1.0), meters(1.0)).unwrap()
    }

    fn at_z(profile: Profile, z: f32) -> LoftSection {
        LoftSection {
            profile,
            placement: Transform::from_translation(Vec3::new(0.0, 0.0, z)),
        }
    }

    fn bounds(mesh: &Mesh) -> (Vec3, Vec3) {
        mesh.positions().iter().fold(
            (
                Vec3::new(f32::INFINITY, f32::INFINITY, f32::INFINITY),
                Vec3::new(f32::NEG_INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY),
            ),
            |(lo, hi), p| {
                (
                    Vec3::new(lo.x.min(p.x), lo.y.min(p.y), lo.z.min(p.z)),
                    Vec3::new(hi.x.max(p.x), hi.y.max(p.y), hi.z.max(p.z)),
                )
            },
        )
    }

    fn faces_outward(mesh: &Mesh, interior: Vec3) -> bool {
        mesh.indices().chunks(3).all(|t| {
            let p = mesh.positions();
            let (a, b, c) = (p[t[0] as usize], p[t[1] as usize], p[t[2] as usize]);
            let out = a.add(b).add(c).mul_scalar(1.0 / 3.0).subtract(interior);
            let geometric = b.subtract(a).cross(c.subtract(a));
            geometric.length() < 1.0e-9 || geometric.dot(out) > 0.0
        })
    }

    #[test]
    fn two_identical_squares_loft_into_a_box() {
        let mesh = loft(
            &[at_z(square(), 0.0), at_z(square(), 2.0)],
            LoftOptions::default(),
        )
        .unwrap();
        // 2 rings x 5 columns of side, plus 4 + 4 cap vertices.
        assert_eq!(mesh.vertex_count(), 18);
        // 1 span x 4 quads x 2, plus 2 + 2 cap triangles.
        assert_eq!(mesh.triangle_count(), 12);
        let (lo, hi) = bounds(&mesh);
        assert!(lo.subtract(Vec3::new(-1.0, -1.0, 0.0)).length() < 1.0e-6);
        assert!(hi.subtract(Vec3::new(1.0, 1.0, 2.0)).length() < 1.0e-6);
        assert!(faces_outward(&mesh, Vec3::new(0.0, 0.0, 1.0)));
        assert!(mesh.has_normals());
        assert!(mesh.has_uvs());
    }

    #[test]
    fn a_prisms_side_normals_are_horizontal_and_its_caps_axial() {
        let mesh = loft(
            &[at_z(square(), 0.0), at_z(square(), 2.0)],
            LoftOptions::default(),
        )
        .unwrap();
        for n in &mesh.normals()[..10] {
            assert!(n.z.abs() < 1.0e-5, "side normal leaned along the loft: {n:?}");
            assert!((n.length() - 1.0).abs() < 1.0e-4);
        }
        assert!(mesh.normals()[10].subtract(Vec3::new(0.0, 0.0, -1.0)).length() < 1.0e-5);
        assert!(mesh.normals()[14].subtract(Vec3::UNIT_Z).length() < 1.0e-5);
    }

    #[test]
    fn a_taper_tilts_the_side_normals_toward_the_narrow_end() {
        let wide = Profile::circle(meters(2.0), Segments::new(16).unwrap()).unwrap();
        let narrow = Profile::circle(meters(0.5), Segments::new(16).unwrap()).unwrap();
        let mesh = loft(
            &[at_z(wide, 0.0), at_z(narrow, 3.0)],
            LoftOptions {
                caps: CapPolicy::None,
                closed_loop: false,
            },
        )
        .unwrap();
        // A cone's normals lean away from the apex: +Z here, since the surface
        // narrows with increasing z.
        for n in mesh.normals() {
            assert!(n.z > 0.3, "taper normal did not tilt: {n:?}");
            assert!((n.length() - 1.0).abs() < 1.0e-4);
        }
        assert!(faces_outward(&mesh, Vec3::new(0.0, 0.0, 1.5)));
    }

    #[test]
    fn a_section_placement_is_applied_to_its_profile_points() {
        // A twisted, scaled top section. A twisted prism is not convex, so the
        // assertion is on placement, not on a convexity test: every ring vertex
        // must be exactly its profile point carried by its own transform.
        let quarter = Quat::from_axis_angle(Vec3::UNIT_Z, core::f32::consts::FRAC_PI_2).unwrap();
        let placement = Transform::new(
            Vec3::new(0.0, 0.0, 2.0),
            quarter,
            Vec3::new(2.0, 2.0, 1.0),
        );
        let mesh = loft(
            &[
                at_z(square(), 0.0),
                LoftSection {
                    profile: square(),
                    placement,
                },
            ],
            LoftOptions {
                caps: CapPolicy::None,
                closed_loop: false,
            },
        )
        .unwrap();
        for (j, p) in square().points().iter().enumerate() {
            let expected = placement.transform_point(Vec3::new(p.x, p.y, 0.0));
            assert!(
                mesh.positions()[5 + j].subtract(expected).length() < 1.0e-5,
                "column {j} was not placed by its section's transform"
            );
        }
        let (lo, hi) = bounds(&mesh);
        assert!((hi.x - 2.0).abs() < 1.0e-5);
        assert!((lo.y + 2.0).abs() < 1.0e-5);
        assert!((hi.z - 2.0).abs() < 1.0e-5);
    }

    #[test]
    fn a_scaled_section_lofts_into_a_convex_frustum() {
        let mesh = loft(
            &[
                at_z(square(), 0.0),
                LoftSection {
                    profile: square(),
                    placement: Transform::new(
                        Vec3::new(0.0, 0.0, 2.0),
                        Quat::IDENTITY,
                        Vec3::new(0.4, 0.4, 1.0),
                    ),
                },
            ],
            LoftOptions::default(),
        )
        .unwrap();
        let (lo, hi) = bounds(&mesh);
        assert!(hi.subtract(Vec3::new(1.0, 1.0, 2.0)).length() < 1.0e-5);
        assert!(lo.subtract(Vec3::new(-1.0, -1.0, 0.0)).length() < 1.0e-5);
        assert!(faces_outward(&mesh, Vec3::new(0.0, 0.0, 0.5)));
    }

    #[test]
    fn v_advances_one_step_per_section_and_u_wraps_the_perimeter() {
        let mesh = loft(
            &[
                at_z(square(), 0.0),
                at_z(square(), 1.0),
                at_z(square(), 2.0),
            ],
            LoftOptions {
                caps: CapPolicy::None,
                closed_loop: false,
            },
        )
        .unwrap();
        assert_eq!(mesh.vertex_count(), 15);
        for row in 0..3usize {
            let v = mesh.uvs()[row * 5].y;
            assert!((v - row as f32 / 2.0).abs() < 1.0e-6);
            assert!(mesh.uvs()[row * 5].x.abs() < 1.0e-6);
            assert!((mesh.uvs()[row * 5 + 4].x - 1.0).abs() < 1.0e-5);
            assert!((mesh.uvs()[row * 5 + 2].x - 0.5).abs() < 1.0e-5);
        }
    }

    #[test]
    fn a_closed_loop_wraps_the_last_section_to_the_first_and_takes_no_caps() {
        let corner = |x: f32, z: f32, turn: f32| LoftSection {
            profile: square(),
            placement: Transform::new(
                Vec3::new(x, 0.0, z),
                Quat::from_axis_angle(Vec3::UNIT_Y, turn).unwrap(),
                Vec3::ONE,
            ),
        };
        let ring = [
            corner(3.0, 0.0, 0.0),
            corner(0.0, 3.0, core::f32::consts::FRAC_PI_2),
            corner(-3.0, 0.0, core::f32::consts::PI),
            corner(0.0, -3.0, -core::f32::consts::FRAC_PI_2),
        ];
        let mesh = loft(
            &ring,
            LoftOptions {
                caps: CapPolicy::Both,
                closed_loop: true,
            },
        )
        .unwrap();
        // 4 rings x 5 columns, no caps at all despite CapPolicy::Both.
        assert_eq!(mesh.vertex_count(), 20);
        // 4 spans, one more than the open case.
        assert_eq!(mesh.triangle_count(), 4 * 4 * 2);
        let open = loft(
            &ring,
            LoftOptions {
                caps: CapPolicy::None,
                closed_loop: false,
            },
        )
        .unwrap();
        assert_eq!(open.vertex_count(), mesh.vertex_count());
        assert_eq!(open.triangle_count() + 8, mesh.triangle_count());
        assert!(mesh.uvs().iter().all(|uv| uv.y < 0.8));
    }

    #[test]
    fn open_sections_loft_into_a_ribbon_and_ignore_the_cap_policy() {
        let strip = || {
            Profile::open(vec![
                Vec2::new(-1.0, 0.0),
                Vec2::new(0.0, 0.5),
                Vec2::new(1.0, 0.0),
            ])
            .unwrap()
        };
        let mesh = loft(
            &[at_z(strip(), 0.0), at_z(strip(), 1.0), at_z(strip(), 2.0)],
            LoftOptions {
                caps: CapPolicy::Both,
                closed_loop: false,
            },
        )
        .unwrap();
        // 3 columns (no seam duplicate) x 3 rows, no caps.
        assert_eq!(mesh.vertex_count(), 9);
        assert_eq!(mesh.triangle_count(), 2 * 2 * 2);
        assert!(mesh.normals().iter().all(|n| (n.length() - 1.0).abs() < 1.0e-4));
    }

    #[test]
    fn a_clockwise_first_section_normalises_the_whole_series() {
        let clockwise = square().reversed();
        assert_eq!(clockwise.winding(), ProfileWinding::Clockwise);
        let mesh = loft(
            &[
                at_z(clockwise.clone(), 0.0),
                at_z(clockwise, 2.0),
            ],
            LoftOptions::default(),
        )
        .unwrap();
        assert!(faces_outward(&mesh, Vec3::new(0.0, 0.0, 1.0)));
    }

    #[test]
    fn each_cap_policy_adds_exactly_the_caps_it_names() {
        let count = |caps| {
            loft(
                &[at_z(square(), 0.0), at_z(square(), 2.0)],
                LoftOptions {
                    caps,
                    closed_loop: false,
                },
            )
            .unwrap()
            .triangle_count()
        };
        assert_eq!(count(CapPolicy::None), 8);
        assert_eq!(count(CapPolicy::Start), 10);
        assert_eq!(count(CapPolicy::End), 10);
        assert_eq!(count(CapPolicy::Both), 12);
    }

    #[test]
    fn fewer_than_two_sections_is_an_invalid_parameter() {
        assert_eq!(
            loft(&[], LoftOptions::default()).unwrap_err().code(),
            MeshErrorCode::InvalidParameter
        );
        assert_eq!(
            loft(&[at_z(square(), 0.0)], LoftOptions::default())
                .unwrap_err()
                .code(),
            MeshErrorCode::InvalidParameter
        );
    }

    #[test]
    fn mismatched_point_counts_are_incompatible_profiles() {
        let triangle = Profile::closed(vec![
            Vec2::new(-1.0, -1.0),
            Vec2::new(1.0, -1.0),
            Vec2::new(0.0, 1.0),
        ])
        .unwrap();
        assert_eq!(
            loft(
                &[at_z(square(), 0.0), at_z(triangle, 2.0)],
                LoftOptions::default()
            )
            .unwrap_err()
            .code(),
            MeshErrorCode::IncompatibleProfiles
        );
    }

    #[test]
    fn a_mixed_open_and_closed_series_is_incompatible() {
        let open_quad = Profile::open(vec![
            Vec2::new(-1.0, -1.0),
            Vec2::new(1.0, -1.0),
            Vec2::new(1.0, 1.0),
            Vec2::new(-1.0, 1.0),
        ])
        .unwrap();
        assert_eq!(
            loft(
                &[at_z(square(), 0.0), at_z(open_quad, 2.0)],
                LoftOptions::default()
            )
            .unwrap_err()
            .code(),
            MeshErrorCode::IncompatibleProfiles
        );
    }

    #[test]
    fn the_default_options_cap_both_ends_of_an_open_series() {
        let d = LoftOptions::default();
        assert_eq!(d.caps, CapPolicy::Both);
        assert!(!d.closed_loop);
    }

    #[test]
    fn a_loft_is_reproducible() {
        let build = || {
            loft(
                &[at_z(square(), 0.0), at_z(square(), 2.5)],
                LoftOptions::default(),
            )
            .unwrap()
        };
        assert_eq!(build(), build());
    }

    #[test]
    fn a_section_carries_its_own_outline_and_placement() {
        let s = at_z(square(), 1.5);
        assert_eq!(s.profile.point_count(), 4);
        assert_eq!(s.placement.translation, Vec3::new(0.0, 0.0, 1.5));
        assert_eq!(s.clone(), s);
    }
}
