//! Behavioural proofs for the [`PerceptionApi`] facade, driven only through its
//! public surface. Covers the ray-fan geometry, the view-cone cull, the fact
//! encodings, subject tracking, and determinism.

use axiom_kernel::{Meters, Radians};
use axiom_math::Vec3;
use axiom_perception::PerceptionApi;

/// A world-unit value as the micro-units the facts encode.
fn micro(v: f32) -> i64 {
    (f64::from(v) * 1_000_000.0) as i64
}

const FORWARD: Vec3 = Vec3::new(0.0, 0.0, -1.0); // first-person -Z forward
fn rad(v: f32) -> Radians {
    Radians::new(v).unwrap()
}
fn m(v: f32) -> Meters {
    Meters::new(v).unwrap()
}

#[test]
fn fact_kind_codes_are_distinct() {
    let kinds = [
        PerceptionApi::FACT_OBSTACLE,
        PerceptionApi::FACT_VISIBLE,
        PerceptionApi::FACT_TRACKED,
    ];
    let mut sorted = kinds.to_vec();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(sorted.len(), kinds.len(), "fact kinds must be distinct");
}

#[test]
fn ray_fan_of_one_points_dead_ahead() {
    let fan = PerceptionApi::ray_fan(FORWARD, rad(1.0), 1);
    assert_eq!(fan.len(), 1);
    assert!((fan[0].x).abs() < 1.0e-6, "no horizontal deflection");
    assert!((fan[0].z + 1.0).abs() < 1.0e-6, "still facing -Z");
}

#[test]
fn ray_fan_is_symmetric_and_spans_the_fov() {
    // An even count straddles forward: the two halves mirror across -Z (x flips).
    let fan = PerceptionApi::ray_fan(FORWARD, rad(std::f32::consts::FRAC_PI_2), 2);
    assert_eq!(fan.len(), 2);
    assert!(
        fan[0].x * fan[1].x < 0.0,
        "the two rays deflect opposite ways"
    );
    assert!(
        (fan[0].x + fan[1].x).abs() < 1.0e-6,
        "symmetric about forward"
    );
    assert!(fan[0].x.abs() > 1.0e-3, "the fan actually spreads");
    // Zero rays requested -> empty fan.
    assert!(PerceptionApi::ray_fan(FORWARD, rad(1.0), 0).is_empty());
}

#[test]
fn in_view_keeps_what_is_ahead_and_drops_the_rest() {
    let eye = Vec3::ZERO;
    let fov = rad(std::f32::consts::FRAC_PI_2); // 90° total -> 45° half-angle
    let range = m(100.0);
    let candidates = [
        (10u32, Vec3::new(0.0, 0.0, -5.0)),   // dead ahead, close
        (11u32, Vec3::new(0.0, 0.0, 5.0)),    // directly behind -> out of cone
        (12u32, Vec3::new(0.0, 0.0, -500.0)), // ahead but out of range
        (13u32, Vec3::new(-2.0, 0.0, -2.0)),  // within the 45° cone
    ];
    let seen = PerceptionApi::in_view(eye, FORWARD, fov, range, &candidates);
    let ids: Vec<u32> = seen.iter().map(|(id, _)| *id).collect();
    assert!(ids.contains(&10), "dead-ahead is visible");
    assert!(ids.contains(&13), "within-cone is visible");
    assert!(!ids.contains(&11), "behind is culled");
    assert!(!ids.contains(&12), "out-of-range is culled");
}

#[test]
fn in_view_with_degenerate_forward_sees_nothing() {
    let seen = PerceptionApi::in_view(
        Vec3::ZERO,
        Vec3::ZERO, // no facing
        rad(1.0),
        m(100.0),
        &[(1, Vec3::new(0.0, 0.0, -1.0))],
    );
    assert!(seen.is_empty());
}

#[test]
fn obstacle_fact_encodes_probe_point_and_distance() {
    let fact = PerceptionApi::obstacle_fact(3, Vec3::new(0.0, 0.0, -2.0), m(2.0));
    assert_eq!(fact.0, PerceptionApi::FACT_OBSTACLE);
    assert_eq!(fact.1, 3, "subject is the probe index");
    assert_eq!((fact.2, fact.3, fact.4), (0, 0, micro(-2.0)), "hit point");
    assert_eq!(fact.5, micro(2.0), "value is the distance");
}

#[test]
fn visible_fact_encodes_id_position_and_kind() {
    let fact = PerceptionApi::visible_fact(42, Vec3::new(1.0, 0.0, -3.0), 7);
    assert_eq!(fact.0, PerceptionApi::FACT_VISIBLE);
    assert_eq!(fact.1, 42, "subject is the entity id");
    assert_eq!((fact.2, fact.3, fact.4), (micro(1.0), 0, micro(-3.0)));
    assert_eq!(fact.5, 7, "value is the coarse kind code");
}

#[test]
fn decode_obstacle_inverts_the_obstacle_fact() {
    let fact = PerceptionApi::obstacle_fact(3, Vec3::new(0.0, 1.0, -2.0), m(2.5));
    let (probe, hit, distance) = PerceptionApi::decode_obstacle(fact).expect("an obstacle decodes");
    assert_eq!(probe, 3, "subject is the probe index");
    assert!(hit.x.abs() < 1.0e-6 && (hit.y - 1.0).abs() < 1.0e-6 && (hit.z + 2.0).abs() < 1.0e-6);
    assert!(
        (distance.get() - 2.5).abs() < 1.0e-6,
        "value decodes to the distance"
    );
    // A fact of another kind is not an obstacle.
    let visible = PerceptionApi::visible_fact(1, Vec3::ZERO, 0);
    assert!(PerceptionApi::decode_obstacle(visible).is_none());
}

#[test]
fn decode_visible_inverts_the_visible_fact_and_returns_the_raw_kind() {
    let fact = PerceptionApi::visible_fact(42, Vec3::new(1.0, 0.0, -3.0), 7);
    let (subject, pos, value) =
        PerceptionApi::decode_visible(fact).expect("a visible fact decodes");
    assert_eq!(subject, 42, "subject is the entity id");
    assert!((pos.x - 1.0).abs() < 1.0e-6 && (pos.z + 3.0).abs() < 1.0e-6);
    assert_eq!(value, 7, "the coarse kind is returned uninterpreted");
    // An obstacle fact is not a visible fact.
    let obstacle = PerceptionApi::obstacle_fact(0, Vec3::ZERO, m(1.0));
    assert!(PerceptionApi::decode_visible(obstacle).is_none());
}

#[test]
fn sense_with_probe_fans_the_rays_culls_the_landmarks_and_assembles_facts() {
    let eye = Vec3::ZERO;
    let fov = rad(std::f32::consts::FRAC_PI_2); // 90° total
    let range = m(1000.0);
    // A test probe that only strikes on the near-straight-ahead ray (a small |x|
    // deflection) — so the two outer rays of a 3-ray fan miss (the discard path).
    let probe = |dir: Vec3| {
        (dir.x.abs() < 0.1).then(|| (m(12.0), Vec3::new(dir.x * 12.0, 0.0, dir.z * 12.0)))
    };
    // One landmark straight ahead and very HIGH (proving the altitude flattening
    // keeps a towering summit in the cone), one directly behind (culled).
    let landmarks = [
        (100u32, Vec3::new(0.0, 900.0, -50.0), 5u32),
        (200u32, Vec3::new(0.0, 0.0, 50.0), 6u32),
    ];
    let facts = PerceptionApi::sense_with_probe(eye, FORWARD, fov, range, 3, probe, &landmarks);

    let obstacles: Vec<_> = facts
        .iter()
        .filter_map(|&f| PerceptionApi::decode_obstacle(f))
        .collect();
    assert_eq!(obstacles.len(), 1, "only the centre ray struck");
    assert_eq!(
        obstacles[0].0, 1,
        "the centre probe of a 3-ray fan is index 1"
    );
    assert!(
        (obstacles[0].2.get() - 12.0).abs() < 1.0e-6,
        "the probe's distance"
    );

    let visible: Vec<_> = facts
        .iter()
        .filter_map(|&f| PerceptionApi::decode_visible(f))
        .collect();
    assert_eq!(visible.len(), 1, "the behind landmark is culled");
    assert_eq!(visible[0].0, 100, "the ahead summit is seen");
    assert_eq!(visible[0].2, 5, "its kind passes through untouched");
    assert!(
        (visible[0].1.y - 900.0).abs() < 1.0e-3,
        "emitted with the landmark's TRUE altitude, not the flattened one"
    );
}

#[test]
fn tracking_yields_per_tick_velocity_and_a_fact() {
    // A subject moving +1 on X per tick.
    let prior = Vec3::new(2.0, 0.0, -3.0);
    let current = Vec3::new(3.0, 0.0, -3.0);
    let velocity = PerceptionApi::relative_motion(prior, current);
    assert!((velocity.x - 1.0).abs() < 1.0e-6);
    assert!(velocity.z.abs() < 1.0e-6);

    let fact = PerceptionApi::tracked_fact(42, velocity);
    assert_eq!(fact.0, PerceptionApi::FACT_TRACKED);
    assert_eq!(fact.1, 42);
    assert_eq!((fact.2, fact.3, fact.4), (micro(1.0), 0, 0));
    assert_eq!(fact.5, 0);
}

#[test]
fn nearest_obstacle_picks_the_closest_and_handles_empty() {
    let probes = [(0u32, m(8.0)), (1u32, m(2.5)), (2u32, m(5.0))];
    let nearest = PerceptionApi::nearest_obstacle(&probes).unwrap();
    assert_eq!(nearest.0, 1, "probe 1 is closest");
    assert!((nearest.1.get() - 2.5).abs() < 1.0e-6);
    assert!(PerceptionApi::nearest_obstacle(&[]).is_none());
}

#[test]
fn perception_is_deterministic() {
    let fan_a = PerceptionApi::ray_fan(FORWARD, rad(1.2), 5);
    let fan_b = PerceptionApi::ray_fan(FORWARD, rad(1.2), 5);
    assert_eq!(fan_a, fan_b);
    let f1 = PerceptionApi::obstacle_fact(1, Vec3::new(1.0, 2.0, 3.0), m(4.0));
    let f2 = PerceptionApi::obstacle_fact(1, Vec3::new(1.0, 2.0, 3.0), m(4.0));
    assert_eq!(f1, f2);
    // The facade is a unit struct; exercise its derive.
    assert!(format!("{:?}", PerceptionApi).contains("PerceptionApi"));
}
