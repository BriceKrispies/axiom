//! Field proofs: dimensions, the one coordinate system, reversible
//! conversions, offense-relative mirroring, and finite generated geometry.

use axiom::prelude::Vec3;
use axiom_end_zone::field::{
    coordinates::{FIELD_LENGTH, FIELD_WIDTH},
    generate_field, normalized_to_world, world_to_yard_line, yard_line_to_z, DriveDirection,
    FieldMaterial, FieldMesh, OffenseFrame, OffensePoint, FIELD_HALF_LENGTH, FIELD_HALF_WIDTH,
    GOAL_LINE_Z, PAINT,
};

const EPS: f32 = 1.0e-4;

#[test]
fn field_dimensions_are_correct() {
    assert!(
        (FIELD_LENGTH - 120.0).abs() < EPS,
        "120 yards end line to end line"
    );
    assert!((FIELD_WIDTH - 160.0 / 3.0).abs() < EPS, "53 1/3 yards wide");
    assert!((FIELD_HALF_LENGTH - 60.0).abs() < EPS);
    assert!((FIELD_HALF_WIDTH - 80.0 / 3.0).abs() < EPS);
    assert!((GOAL_LINE_Z - 50.0).abs() < EPS, "10-yard end zones");
}

#[test]
fn midfield_is_z_zero() {
    // The 50-yard line is world Z = 0 from either drive direction.
    assert!(yard_line_to_z(50.0, DriveDirection::PlusZ).abs() < EPS);
    assert!(yard_line_to_z(50.0, DriveDirection::MinusZ).abs() < EPS);
    assert!((world_to_yard_line(Vec3::ZERO) - 50.0).abs() < EPS);
    assert!((normalized_to_world(0.5, 0.5).z).abs() < EPS);
}

#[test]
fn end_zone_boundaries_are_correct() {
    // Goal lines at Z = ±50, end lines at Z = ±60.
    for direction in [DriveDirection::PlusZ, DriveDirection::MinusZ] {
        let goal = yard_line_to_z(100.0, direction);
        assert!((goal.abs() - GOAL_LINE_Z).abs() < EPS, "opponent goal line");
        let own_goal = yard_line_to_z(0.0, direction);
        assert!((own_goal.abs() - GOAL_LINE_Z).abs() < EPS, "own goal line");
        assert!(goal * own_goal < 0.0, "goal lines on opposite sides");
    }
    // Yard-line numbers hit zero at the goal lines and -10 at the end lines.
    assert!((world_to_yard_line(Vec3::new(0.0, 0.0, 50.0))).abs() < EPS);
    assert!((world_to_yard_line(Vec3::new(0.0, 0.0, -50.0))).abs() < EPS);
    assert!((world_to_yard_line(Vec3::new(0.0, 0.0, 60.0)) + 10.0).abs() < EPS);
}

#[test]
fn yard_to_world_conversion_is_reversible() {
    for direction in [DriveDirection::PlusZ, DriveDirection::MinusZ] {
        for yards in [0.0f32, 12.5, 25.0, 50.0, 63.0, 88.5, 100.0] {
            let z = yard_line_to_z(yards, direction);
            // Invert: yards from own goal = z * sign + 50.
            let back = z * direction.sign() + GOAL_LINE_Z;
            assert!(
                (back - yards).abs() < EPS,
                "{yards} yd ({direction:?}) -> z {z} -> {back}"
            );
            // And the broadcast number matches the mirrored distance.
            let broadcast = world_to_yard_line(Vec3::new(0.0, 0.0, z));
            let expected = 50.0 - (yards - 50.0).abs();
            assert!((broadcast - expected).abs() < EPS);
        }
    }
}

#[test]
fn normalized_coordinates_span_the_field() {
    let a = normalized_to_world(0.0, 0.0);
    let b = normalized_to_world(1.0, 1.0);
    assert!((a.x + FIELD_HALF_WIDTH).abs() < EPS && (a.z + FIELD_HALF_LENGTH).abs() < EPS);
    assert!((b.x - FIELD_HALF_WIDTH).abs() < EPS && (b.z - FIELD_HALF_LENGTH).abs() < EPS);
    assert_eq!(a.y, 0.0, "the surface is Y = 0");
}

#[test]
fn offense_relative_coordinates_work_in_both_drive_directions() {
    let point = OffensePoint::new(5.0, 10.0);
    for direction in [DriveDirection::PlusZ, DriveDirection::MinusZ] {
        let frame = OffenseFrame::at_yard_line(35.0, direction);
        let world = frame.to_world(point);
        // Downfield is toward the opponent end zone.
        let downfield = (world.z - frame.line_of_scrimmage_z) * direction.sign();
        assert!((downfield - 10.0).abs() < EPS, "{direction:?}");
        // Round trip.
        let back = frame.from_world(world);
        assert!((back.lateral - point.lateral).abs() < EPS);
        assert!((back.downfield - point.downfield).abs() < EPS);
    }
    // The same authored point mirrors across midfield between directions.
    let plus = OffenseFrame::at_yard_line(35.0, DriveDirection::PlusZ).to_world(point);
    let minus = OffenseFrame::at_yard_line(35.0, DriveDirection::MinusZ).to_world(point);
    assert!((plus.x + minus.x).abs() < EPS, "lateral mirrors");
    assert!((plus.z + minus.z).abs() < EPS, "downfield mirrors");
}

#[test]
fn generated_field_geometry_is_finite_and_nonempty() {
    let field = generate_field();
    assert!(
        field.pieces.len() > 25,
        "turf bands, zones, apron, goalposts"
    );
    // The static field is surface only: paint is camera-driven and lives in
    // `field::paint_layout`, so nothing here may be a marking.
    assert!(
        !field
            .pieces
            .iter()
            .any(|p| p.transform.translation.y > 0.0 && p.mesh == FieldMesh::Plane),
        "no static paint above the turf plane"
    );
    for piece in &field.pieces {
        let t = piece.transform;
        for v in [
            t.translation.x,
            t.translation.y,
            t.translation.z,
            t.rotation.x,
            t.rotation.y,
            t.rotation.z,
            t.rotation.w,
            t.scale.x,
            t.scale.y,
            t.scale.z,
        ] {
            assert!(v.is_finite(), "piece transform is finite");
        }
    }
}

#[test]
fn the_playable_length_is_divided_into_alternating_turf_bands() {
    let field = generate_field();
    let mut bands: Vec<_> = field
        .pieces
        .iter()
        .filter(|p| {
            matches!(
                p.material,
                FieldMaterial::TurfLight | FieldMaterial::TurfDark
            )
        })
        .collect();
    bands.sort_by(|a, b| {
        a.transform
            .translation
            .z
            .total_cmp(&b.transform.translation.z)
    });

    let expected = (GOAL_LINE_Z * 2.0 / PAINT.band_yards).round() as usize;
    assert_eq!(bands.len(), expected, "one band per five-yard section");

    for (index, band) in bands.iter().enumerate() {
        // Bands tile the field between the goal lines with no gap or overlap.
        let z0 = -GOAL_LINE_Z + index as f32 * PAINT.band_yards;
        let center = z0 + PAINT.band_yards * 0.5;
        assert!(
            (band.transform.translation.z - center).abs() < EPS,
            "band {index} tiles"
        );
        assert!((band.transform.scale.z - PAINT.band_yards).abs() < EPS);
        assert!(
            (band.transform.scale.x - FIELD_HALF_WIDTH * 2.0).abs() < EPS,
            "full width"
        );
        // And they alternate, so parity alone classifies a band.
        let expected_material = [FieldMaterial::TurfLight, FieldMaterial::TurfDark][index % 2];
        assert_eq!(band.material, expected_material, "band {index} alternates");
    }
}
