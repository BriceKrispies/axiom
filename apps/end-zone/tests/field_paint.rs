//! Field paint proofs: the pure geometry and level-of-detail logic behind the
//! camera-driven field markings.
//!
//! Every assertion here is on data, not pixels — the paint layout is a pure
//! function of (camera, gameplay lines, config), so what the renderer will be
//! asked to draw is fully decidable natively. What the result *looks like* is
//! the job of the six `end-zone-field-*` inspection slices
//! (`axiom_end_zone::FieldView`).

use axiom::prelude::Vec3;
use axiom_end_zone::field::{
    classify, field_paint, is_major_division, paint_pool_capacity, GameplayLines, Lod, PaintCamera,
    PaintCategory, PaintQuad, FIELD_HALF_LENGTH, FIELD_HALF_WIDTH, GOAL_LINE_Z, PAINT, PAINT_Y,
};

const EPS: f32 = 1.0e-4;

/// A camera behind the play at `z`, looking downfield toward `+Z`.
fn downfield_camera(z: f32) -> PaintCamera {
    PaintCamera::looking(Vec3::new(0.0, 7.0, z), Vec3::new(0.0, 3.0, z + 30.0))
        .expect("a downfield look-at is a valid camera")
}

/// A camera lying in the field plane, looking straight down `+Z`. Distance and
/// forward depth to a marking are then both just its `Z`, which is what makes
/// the tier thresholds readable as plain yardage.
fn level_camera() -> PaintCamera {
    PaintCamera::looking(Vec3::new(0.0, PAINT_Y, 0.0), Vec3::new(0.0, PAINT_Y, 30.0))
        .expect("a level look-at is a valid camera")
}

fn paint_at(camera: PaintCamera, lines: GameplayLines) -> Vec<PaintQuad> {
    let mut out = Vec::new();
    field_paint(Some(camera), lines, &PAINT, &mut out);
    out
}

fn count(quads: &[PaintQuad], category: PaintCategory) -> usize {
    quads.iter().filter(|q| q.category == category).count()
}

// --- level of detail ---------------------------------------------------------

#[test]
fn depth_tiers_run_near_then_mid_then_far_then_culled() {
    let camera = level_camera();
    let ahead = |yards: f32| Vec3::new(0.0, PAINT_Y, yards);

    // The four tiers, sampled just inside each threshold.
    assert_eq!(classify(&camera, ahead(1.0), &PAINT), Lod::Near);
    assert_eq!(
        classify(&camera, ahead(PAINT.near_yards - 1.0), &PAINT),
        Lod::Near
    );
    assert_eq!(
        classify(&camera, ahead(PAINT.near_yards + 1.0), &PAINT),
        Lod::Mid
    );
    assert_eq!(
        classify(&camera, ahead(PAINT.mid_yards + 1.0), &PAINT),
        Lod::Far
    );
    assert_eq!(
        classify(&camera, ahead(PAINT.cull_yards + 1.0), &PAINT),
        Lod::Culled
    );
}

#[test]
fn geometry_behind_or_astride_the_near_plane_is_culled_before_projection() {
    let camera = level_camera();

    // Directly behind the camera.
    assert_eq!(
        classify(&camera, Vec3::new(0.0, PAINT_Y, -10.0), &PAINT),
        Lod::Culled
    );
    // Square beside the camera — well within range, but with no forward depth
    // at all. This is the case aggressive yaw sweeps through every frame.
    assert_eq!(
        classify(&camera, Vec3::new(8.0, PAINT_Y, 0.0), &PAINT),
        Lod::Culled
    );
    // Straddling the near plane: just inside it survives, just outside does not.
    let forward = camera.forward;
    let inside = camera
        .eye
        .add(forward.mul_scalar(PAINT.min_depth_yards + 0.05));
    let outside = camera
        .eye
        .add(forward.mul_scalar(PAINT.min_depth_yards - 0.05));
    assert_eq!(classify(&camera, inside, &PAINT), Lod::Near);
    assert_eq!(classify(&camera, outside, &PAINT), Lod::Culled);
}

#[test]
fn non_finite_geometry_is_culled_rather_than_projected() {
    let camera = downfield_camera(0.0);
    for bad in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        assert_eq!(
            classify(&camera, Vec3::new(bad, PAINT_Y, 10.0), &PAINT),
            Lod::Culled,
            "{bad} must never reach a projection"
        );
        assert_eq!(
            classify(&camera, Vec3::new(0.0, PAINT_Y, bad), &PAINT),
            Lod::Culled
        );
    }
}

#[test]
fn an_invalid_camera_paints_nothing_at_all() {
    // A degenerate look-at (eye == target) has no forward direction, and a
    // non-finite one has no valid projection. Both must yield no geometry
    // rather than geometry that cannot be projected.
    assert!(PaintCamera::looking(Vec3::ZERO, Vec3::ZERO).is_none());
    assert!(PaintCamera::looking(Vec3::new(f32::NAN, 0.0, 0.0), Vec3::ZERO).is_none());
    assert!(PaintCamera::looking(Vec3::ZERO, Vec3::new(0.0, f32::INFINITY, 0.0)).is_none());

    let mut out = vec![PaintQuad {
        center: Vec3::ZERO,
        half_x: 1.0,
        half_z: 1.0,
        category: PaintCategory::Hash,
    }];
    field_paint(None, GameplayLines::default(), &PAINT, &mut out);
    assert!(out.is_empty(), "an invalid camera clears the buffer");
}

// --- what each tier retains --------------------------------------------------

#[test]
fn near_mid_and_far_retain_progressively_less() {
    let lines = GameplayLines {
        scrimmage_z: Some(-20.0),
        line_to_gain_z: Some(-10.0),
    };
    // Near: the camera sits on the field, so hashes and near majors exist.
    let near = paint_at(downfield_camera(-25.0), lines);
    assert!(count(&near, PaintCategory::Hash) > 0, "near keeps hashes");
    assert!(count(&near, PaintCategory::MajorNear) > 0);

    // Mid: pulled back past the hash window, majors survive, hashes do not.
    let mid = paint_at(
        PaintCamera::looking(
            Vec3::new(0.0, 20.0, -GOAL_LINE_Z - 8.0),
            Vec3::new(0.0, 0.0, -20.0),
        )
        .expect("valid camera"),
        lines,
    );
    assert_eq!(count(&mid, PaintCategory::Hash), 0, "mid drops hashes");
    assert!(count(&mid, PaintCategory::MajorMid) > 0, "mid keeps majors");

    // Far: looking the length of the field from beyond the far end line, the
    // divisions down the other end are past the cull distance entirely.
    let far = paint_at(
        PaintCamera::looking(
            Vec3::new(0.0, 30.0, -FIELD_HALF_LENGTH - 30.0),
            Vec3::new(0.0, 0.0, GOAL_LINE_Z),
        )
        .expect("valid camera"),
        lines,
    );
    assert_eq!(count(&far, PaintCategory::Hash), 0);
    let far_majors = count(&far, PaintCategory::MajorNear) + count(&far, PaintCategory::MajorMid);
    let near_majors =
        count(&near, PaintCategory::MajorNear) + count(&near, PaintCategory::MajorMid);
    assert!(
        far_majors < near_majors,
        "distance culls divisions: {far_majors} < {near_majors}"
    );

    // The field's own identity survives every tier.
    for (label, quads) in [("near", &near), ("mid", &mid), ("far", &far)] {
        assert_eq!(
            count(quads, PaintCategory::Boundary),
            6,
            "{label}: two sidelines, two goal lines, two end lines"
        );
    }
}

#[test]
fn the_two_gameplay_lines_ignore_level_of_detail_entirely() {
    let lines = GameplayLines {
        scrimmage_z: Some(40.0),
        line_to_gain_z: Some(48.0),
    };
    // A camera at the opposite end of the field: both lines are far past the
    // cull distance that would erase an ordinary marking...
    let camera = downfield_camera(-FIELD_HALF_LENGTH);
    assert_eq!(
        classify(&camera, Vec3::new(0.0, PAINT_Y, 40.0), &PAINT),
        Lod::Culled,
        "an ordinary marking there would be culled"
    );
    // ...and both are still painted.
    let quads = paint_at(camera, lines);
    assert_eq!(count(&quads, PaintCategory::Scrimmage), 1);
    assert_eq!(count(&quads, PaintCategory::LineToGain), 1);

    // Absent lines paint nothing; an off-field line is rejected rather than
    // clamped into a lie.
    let none = paint_at(camera, GameplayLines::default());
    assert_eq!(count(&none, PaintCategory::Scrimmage), 0);
    assert_eq!(count(&none, PaintCategory::LineToGain), 0);
    let off = paint_at(
        camera,
        GameplayLines {
            scrimmage_z: Some(FIELD_HALF_LENGTH + 5.0),
            line_to_gain_z: Some(f32::NAN),
        },
    );
    assert_eq!(count(&off, PaintCategory::Scrimmage), 0);
    assert_eq!(count(&off, PaintCategory::LineToGain), 0);
}

// --- the markings themselves -------------------------------------------------

#[test]
fn ten_yard_divisions_are_identified_exactly() {
    for z in [-40.0f32, -30.0, -20.0, -10.0, 0.0, 10.0, 20.0, 30.0, 40.0] {
        assert!(is_major_division(z, &PAINT), "{z} is a ten-yard division");
    }
    // Five-yard band seams are NOT divisions — that is the whole point of
    // replacing them with turf bands.
    for z in [-45.0f32, -35.0, -5.0, 5.0, 15.0, 25.0] {
        assert!(
            !is_major_division(z, &PAINT),
            "{z} is a band seam, not paint"
        );
    }
    // Neither are the goal lines (the boundary paints those) or anything past
    // them, and neither is an off-grid yard.
    for z in [-GOAL_LINE_Z, GOAL_LINE_Z, -FIELD_HALF_LENGTH, 3.0, 12.5] {
        assert!(!is_major_division(z, &PAINT), "{z} is not a division");
    }
}

#[test]
fn every_marking_is_a_world_space_rectangle_with_real_width() {
    let quads = paint_at(
        downfield_camera(-25.0),
        GameplayLines {
            scrimmage_z: Some(-20.0),
            line_to_gain_z: Some(-10.0),
        },
    );
    assert!(!quads.is_empty());

    // The narrowest dimension any marking may have. Nothing thinner than a
    // hash's thickness exists, so no marking can decay into a hairline.
    let floor = PAINT
        .hash_width
        .min(PAINT.major_width)
        .min(PAINT.gameplay_width)
        .min(PAINT.boundary_width)
        * 0.5;

    for quad in &quads {
        assert!(
            quad.half_x >= floor && quad.half_z >= floor,
            "{:?} is thinner than the paint floor",
            quad
        );
        for v in [
            quad.center.x,
            quad.center.y,
            quad.center.z,
            quad.half_x,
            quad.half_z,
        ] {
            assert!(v.is_finite(), "no marking carries a non-finite coordinate");
        }
        // Four real corners, all on the field surface, all inside the world.
        assert!(quad.center.y >= PAINT_Y - EPS, "paint sits above the turf");
        assert!(quad.center.x.abs() <= FIELD_HALF_WIDTH + EPS);
        assert!(quad.center.z.abs() <= FIELD_HALF_LENGTH + EPS);

        // And the transform the scene draws it with is the same rectangle.
        let transform = quad.transform();
        assert!((transform.scale.x - quad.half_x * 2.0).abs() < EPS);
        assert!((transform.scale.z - quad.half_z * 2.0).abs() < EPS);
        assert_eq!(transform.translation, quad.center);
    }
}

#[test]
fn hash_blocks_are_squat_paired_and_never_double_a_division() {
    let camera = downfield_camera(-25.0);
    let quads = paint_at(camera, GameplayLines::default());
    let hashes: Vec<_> = quads
        .iter()
        .filter(|q| q.category == PaintCategory::Hash)
        .collect();
    assert!(!hashes.is_empty(), "the near window has hashes");

    for hash in &hashes {
        assert!(
            hash.half_x > hash.half_z,
            "a hash is a squat block, wider than it is thick"
        );
        assert!(
            !is_major_division(hash.center.z, &PAINT),
            "a hash never sits on a ten-yard line the majors already paint"
        );
        assert!(
            hash.center.z.abs() < GOAL_LINE_Z + EPS,
            "hashes stay between the goal lines"
        );
    }

    // They come in pairs: one column each side of the field centre line.
    let left = hashes.iter().filter(|h| h.center.x < 0.0).count();
    let right = hashes.iter().filter(|h| h.center.x > 0.0).count();
    assert_eq!(left, right, "paired hash columns");
    assert!(left > 0);

    // And only near the camera: nothing a long way downfield of it.
    let furthest = hashes
        .iter()
        .map(|h| (h.center.z - camera.eye.z).abs())
        .fold(0.0f32, f32::max);
    assert!(
        furthest <= PAINT.near_yards + EPS,
        "hashes stay inside the near window ({furthest} yd)"
    );
}

// --- bounds ------------------------------------------------------------------

#[test]
fn emission_never_exceeds_the_pool_any_camera_can_fill() {
    // Sweep cameras across the field and around the compass. No camera may ask
    // for more of a category than the scene pool holds — the pool is the hard
    // bound the scene is built to, so overflowing it would silently drop paint.
    let mut worst = [0usize; 6];
    let mut buffer = Vec::new();
    for step in 0..24 {
        let angle = step as f32 * core::f32::consts::TAU / 24.0;
        for z in [-55.0f32, -30.0, -5.0, 0.0, 15.0, 45.0, 58.0] {
            for height in [0.5f32, 3.0, 9.0, 25.0] {
                let eye = Vec3::new(FIELD_HALF_WIDTH * 0.5, height, z);
                let target = eye.add(Vec3::new(angle.sin() * 20.0, -1.0, angle.cos() * 20.0));
                let camera = PaintCamera::looking(eye, target).expect("valid camera");
                field_paint(
                    Some(camera),
                    GameplayLines {
                        scrimmage_z: Some(z.clamp(-GOAL_LINE_Z, GOAL_LINE_Z)),
                        line_to_gain_z: Some((z + 10.0).clamp(-GOAL_LINE_Z, GOAL_LINE_Z)),
                    },
                    &PAINT,
                    &mut buffer,
                );
                assert!(buffer.len() <= paint_pool_capacity());
                for category in PaintCategory::ALL {
                    let n = count(&buffer, category);
                    worst[category.index()] = worst[category.index()].max(n);
                    assert!(
                        n <= category.pool_size(),
                        "{category:?}: {n} quads > pool of {}",
                        category.pool_size()
                    );
                }
            }
        }
    }
    // Every category is genuinely reachable, so no pool slot is dead weight.
    for category in PaintCategory::ALL {
        assert!(
            worst[category.index()] > 0,
            "{category:?} is never emitted — its pool is dead"
        );
    }
}

#[test]
fn the_same_camera_always_paints_the_same_field() {
    // Stability under rotation is the whole requirement: the paint must be a
    // function of the camera, never of frame history.
    let lines = GameplayLines {
        scrimmage_z: Some(-20.0),
        line_to_gain_z: Some(-10.0),
    };
    let camera = downfield_camera(-25.0);
    let first = paint_at(camera, lines);
    let elsewhere = paint_at(downfield_camera(20.0), lines);
    let again = paint_at(camera, lines);
    assert_eq!(first, again, "paint is a pure function of the camera");
    assert_ne!(first, elsewhere, "and it genuinely tracks the camera");
}
