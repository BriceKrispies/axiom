//! The ported weapon data + ballistics, pinned against the JavaScript.
//!
//! The recoil-pattern and falloff-curve `expected` values below were captured
//! by running the original `C:/dev/Claude-of-Duty/src/weapons/{defs,ballistics}.js`
//! under Node (v24): `buildRecoilPattern(def, Rng)` for every weapon in
//! `WEAPON_DEFS`, and the falloff formula over a spread of `dropoff`/`range01`
//! samples. They are golden values, not recomputations: if a future edit to
//! `defs.rs`/`ballistics.rs` changes one of them, the port has silently
//! stopped matching the source, and every recoil pattern and damage curve
//! downstream has moved.

use std::cell::RefCell;

use axiom_claude_of_duty::weapons::ballistics::{
    falloff, range01, FireBulletRequest, ProjectileSim, RaycastHit, RaycastWorld, SpawnParams,
    Vec3, GRAVITY, MAX_LIVE,
};
use axiom_claude_of_duty::weapons::defs::{
    build_recoil_pattern, by_id, Stance, DEG2RAD, PISTOL, RIFLE, SMG, WEAPON_DEFS,
};

// ---------------------------------------------------------------------------
// defs.js — weapon data tables
// ---------------------------------------------------------------------------

#[test]
fn weapon_defs_match_the_source_field_for_field() {
    assert_eq!(RIFLE.id, "rifle");
    assert_eq!(RIFLE.label, "M4A1");
    assert_eq!(RIFLE.rpm, 800.0);
    assert_eq!(RIFLE.modes, &["auto", "burst", "semi"]);
    assert_eq!(RIFLE.muzzle_velocity, 880.0);
    assert_eq!(RIFLE.dropoff, 0.62);
    assert_eq!(RIFLE.recoil.pattern_seed, 0x4d34a1);
    assert_eq!(RIFLE.recoil.climb_shape, &[1.45, 1.3, 1.15, 1.05, 1.0]);

    assert_eq!(SMG.muzzle_velocity, 400.0);
    assert_eq!(SMG.caliber, "9x19");
    assert_eq!(SMG.recoil.pattern_seed, 0x9ac31f);

    assert_eq!(PISTOL.muzzle_velocity, 360.0);
    assert_eq!(PISTOL.recoil.climb_shape, &[1.0]);
    assert_eq!(PISTOL.recoil.pattern_seed, 0x1f77bc);

    // WEAPON_DEFS is every weapon, in the source's declaration order.
    assert_eq!(
        WEAPON_DEFS.map(|d| d.id),
        ["rifle", "smg", "pistol"]
    );
    assert_eq!(by_id("smg").unwrap().label, "MPX-9");
    assert!(by_id("nope").is_none());
}

#[test]
fn deg2rad_is_pi_over_180() {
    assert!((DEG2RAD - std::f64::consts::PI / 180.0).abs() < 1e-15);
    // 180 degrees is pi radians, to within f64 rounding.
    assert!((180.0 * DEG2RAD - std::f64::consts::PI).abs() < 1e-12);
}

#[test]
fn spread_mods_match_the_source_table() {
    assert_eq!(Stance::Crouch.spread_mod(), 0.78);
    assert_eq!(Stance::Prone.spread_mod(), 0.6);
    assert_eq!(Stance::Still.spread_mod(), 0.82);
    assert_eq!(Stance::Walking.spread_mod(), 1.15);
    assert_eq!(Stance::Sprinting.spread_mod(), 2.2);
    assert_eq!(Stance::Airborne.spread_mod(), 2.0);
    assert_eq!(Stance::Hipfire.spread_mod(), 1.0);
}

// ---------------------------------------------------------------------------
// defs.js — buildRecoilPattern (golden capture)
// ---------------------------------------------------------------------------

/// The vertical (`pitch`) component of every pattern entry is built only from
/// `+ - *` over values already narrowed through `float()`/`signed()`, so it
/// matches the JS `Float32Array` output exactly once both sides narrow to
/// `f32` the same way (a deterministic, round-to-nearest-even conversion on
/// both V8 and Rust).
fn assert_pitch_exact(actual: f32, expected: f32) {
    assert_eq!(actual, expected);
}

/// The horizontal (`yaw`) component runs through `sin()`, which is not
/// bit-guaranteed across libm implementations. Both sides narrow the result to
/// `f32` (precision ~1.2e-7 relative), so a tolerance a few `f32` ulps wide is
/// enough to absorb any `f64`-level libm disagreement while still catching a
/// genuine drift in the maths.
fn assert_yaw_close(actual: f32, expected: f32) {
    assert!(
        (actual - expected).abs() < 1e-6,
        "expected {expected}, got {actual}"
    );
}

fn assert_pattern(actual: &[[f32; 2]], expected: &[[f32; 2]]) {
    assert_eq!(actual.len(), expected.len());
    for (a, e) in actual.iter().zip(expected) {
        assert_pitch_exact(a[0], e[0]);
        assert_yaw_close(a[1], e[1]);
    }
}

#[test]
fn rifle_recoil_pattern_matches_the_javascript() {
    let expected: [[f32; 2]; 30] = [
        [0.01226347591727972, -0.00026643756427802145],
        [0.010333409532904625, -0.0002620226296130568],
        [0.009265465661883354, -0.0008892822661437094],
        [0.009839070029556751, -0.0010304737370461226],
        [0.008977381512522697, -0.0006278986693359911],
        [0.0077683893032372, -0.0008322768844664097],
        [0.008905057795345783, -0.00179456383921206],
        [0.007838677614927292, -0.002263498492538929],
        [0.009410439059138298, -0.002759468974545598],
        [0.009026355110108852, -0.0033104403410106897],
        [0.009030723012983799, -0.0042498172260820866],
        [0.008974089287221432, -0.003918734844774008],
        [0.008089190348982811, -0.0034550840500742197],
        [0.00907664280384779, -0.002396139781922102],
        [0.009204740636050701, -0.0015727919526398182],
        [0.007859072647988796, 0.000731156615074724],
        [0.008262877352535725, 0.00229999260045588],
        [0.008490574546158314, 0.00307643786072731],
        [0.008800322189927101, 0.0036447837483137846],
        [0.009369023144245148, 0.0037501202896237373],
        [0.008130707778036594, 0.0027179967146366835],
        [0.00911168847233057, 0.002001002663746476],
        [0.0076063163578510284, 0.0004306161717977375],
        [0.008679751306772232, -0.0008261893526650965],
        [0.008935502730309963, -0.0008405123371630907],
        [0.007953963242471218, -0.0012085132766515017],
        [0.007568730972707272, -0.0009591443231329322],
        [0.008189622312784195, -0.0010676662204787135],
        [0.009474269114434719, -0.0009873469825834036],
        [0.008196702226996422, -0.0018854321679100394],
    ];
    assert_pattern(&build_recoil_pattern(&RIFLE.recoil), &expected);
}

#[test]
fn smg_recoil_pattern_matches_the_javascript() {
    let expected: [[f32; 2]; 32] = [
        [0.0084307212382555, -0.00026852547307498753],
        [0.007093770895153284, -0.0018246539402753115],
        [0.006092763040214777, -0.0024823236744850874],
        [0.0058523500338196754, -0.0031472037080675364],
        [0.006122939754277468, -0.003547026077285409],
        [0.005560397170484066, -0.0019716594833880663],
        [0.0057107824832201, -0.0020824589300900698],
        [0.005736430175602436, -0.0026925276033580303],
        [0.006195548456162214, -0.002070677699521184],
        [0.006238609552383423, -0.0018379176035523415],
        [0.006130733992904425, -0.002146422630175948],
        [0.005357672460377216, -0.002424295525997877],
        [0.0058694458566606045, -0.0025469434913247824],
        [0.005232243333011866, -0.0012036970583721995],
        [0.006424646358937025, 0.00012325997522566468],
        [0.006196942180395126, 0.0025535004679113626],
        [0.006432824768126011, 0.004499952774494886],
        [0.006007837597280741, 0.006768481805920601],
        [0.005504175554960966, 0.007906158454716206],
        [0.005827189423143864, 0.007672736421227455],
        [0.006190768908709288, 0.006966509856283665],
        [0.006442810874432325, 0.005453203339129686],
        [0.005277031101286411, 0.003374096006155014],
        [0.006159552372992039, 0.0007556358468718827],
        [0.005768308416008949, -0.0005847052088938653],
        [0.006376536563038826, -0.0022723760921508074],
        [0.005419537890702486, -0.0030654503498226404],
        [0.006386900320649147, -0.00362405925989151],
        [0.005998680368065834, -0.002983309328556061],
        [0.005152610130608082, -0.00283081759698689],
        [0.005406165029853582, -0.0026117742527276278],
        [0.006028126925230026, -0.002285093069076538],
    ];
    assert_pattern(&build_recoil_pattern(&SMG.recoil), &expected);
}

#[test]
fn pistol_recoil_pattern_matches_the_javascript() {
    let expected: [[f32; 2]; 17] = [
        [0.01326390728354454, 0.005803301464766264],
        [0.013485963456332684, 0.007180796470493078],
        [0.011946299113333225, 0.010118996724486351],
        [0.013416135683655739, 0.00834329891949892],
        [0.0118520837277174, 0.0026330447290092707],
        [0.0122376075014472, -0.0067578391171991825],
        [0.011686463840305805, -0.011954864487051964],
        [0.013504397124052048, -0.011200744658708572],
        [0.01138289738446474, -0.0067349569872021675],
        [0.011926892213523388, 0.0002180562587454915],
        [0.013741507194936275, 0.005279645323753357],
        [0.012515905313193798, 0.006411512847989798],
        [0.011777895502746105, 0.00510748103260994],
        [0.012350442819297314, 0.006497546099126339],
        [0.011631971225142479, 0.008142534643411636],
        [0.011383913457393646, 0.008844040334224701],
        [0.01327796746045351, 0.004024378955364227],
    ];
    assert_pattern(&build_recoil_pattern(&PISTOL.recoil), &expected);
}

#[test]
fn a_shot_past_the_climb_shape_holds_the_last_multiplier() {
    // Pistol's climbShape is a single entry: every one of its 17 shots reads
    // `climbShape[min(shot, 0)]`, i.e. the same 1.0 multiplier throughout.
    // Regenerating with a climb shape trimmed to just the first entry must
    // therefore reproduce the pitch column exactly.
    let mut trimmed = PISTOL.recoil;
    trimmed.climb_shape = &PISTOL.recoil.climb_shape[..1];
    let full = build_recoil_pattern(&PISTOL.recoil);
    let short = build_recoil_pattern(&trimmed);
    for (f, s) in full.iter().zip(short.iter()) {
        assert_eq!(f[0], s[0]);
    }
}

// ---------------------------------------------------------------------------
// ballistics.js — falloff / range01 (golden capture)
// ---------------------------------------------------------------------------

#[test]
fn falloff_curve_matches_the_javascript() {
    // (dropoff, range01, expected) — `1 - (1 - dropoff) * range01^2`, pure
    // `+ - *`, so exact `f64` equality against the captured JS doubles.
    let expected: [(f64, f64, f64); 35] = [
        (0.42, 0.0, 1.0),
        (0.42, 0.1, 0.9942),
        (0.42, 0.25, 0.96375),
        (0.42, 0.5, 0.855),
        (0.42, 0.75, 0.67375),
        (0.42, 0.9, 0.5301999999999999),
        (0.42, 1.0, 0.41999999999999993),
        (0.48, 0.0, 1.0),
        (0.48, 0.1, 0.9948),
        (0.48, 0.25, 0.9675),
        (0.48, 0.5, 0.87),
        (0.48, 0.75, 0.7075),
        (0.48, 0.9, 0.5788),
        (0.48, 1.0, 0.48),
        (0.62, 0.0, 1.0),
        (0.62, 0.1, 0.9962),
        (0.62, 0.25, 0.97625),
        (0.62, 0.5, 0.905),
        (0.62, 0.75, 0.78625),
        (0.62, 0.9, 0.6921999999999999),
        (0.62, 1.0, 0.62),
        (0.0, 0.0, 1.0),
        (0.0, 0.1, 0.99),
        (0.0, 0.25, 0.9375),
        (0.0, 0.5, 0.75),
        (0.0, 0.75, 0.4375),
        (0.0, 0.9, 0.18999999999999995),
        (0.0, 1.0, 0.0),
        (1.0, 0.0, 1.0),
        (1.0, 0.1, 1.0),
        (1.0, 0.25, 1.0),
        (1.0, 0.5, 1.0),
        (1.0, 0.75, 1.0),
        (1.0, 0.9, 1.0),
        (1.0, 1.0, 1.0),
    ];
    for (dropoff, r01, want) in expected {
        assert_eq!(falloff(dropoff, r01), want);
    }
}

#[test]
fn range01_clamps_to_one_and_matches_the_javascript() {
    assert_eq!(range01(0.0, 400.0), 0.0);
    assert_eq!(range01(200.0, 400.0), 0.5);
    assert_eq!(range01(400.0, 400.0), 1.0);
    // Past max range: `Math.min(1, travelled / maxRange)` clamps rather than
    // producing a value > 1.
    assert_eq!(range01(800.0, 400.0), 1.0);
}

// ---------------------------------------------------------------------------
// ballistics.js — ProjectileSim (integration, pooling, the physics seam)
// ---------------------------------------------------------------------------

/// A minimal [`RaycastWorld`] for tests: every raycast in `hits` fires once
/// (by call order), everything else misses. Every `fire_bullet` request is
/// recorded for inspection.
#[derive(Default)]
struct MockWorld {
    hits: RefCell<Vec<Option<RaycastHit>>>,
    fired: RefCell<Vec<FireBulletRequest>>,
}

impl RaycastWorld for MockWorld {
    fn raycast(&self, _origin: Vec3, _dir: Vec3, _max_dist: f64) -> Option<RaycastHit> {
        let mut hits = self.hits.borrow_mut();
        if hits.is_empty() {
            return None;
        }
        hits.remove(0)
    }

    fn fire_bullet(&mut self, request: FireBulletRequest) {
        self.fired.borrow_mut().push(request);
    }
}

#[test]
fn spawn_sets_every_field_from_the_source_defaults() {
    let mut sim = ProjectileSim::new();
    let idx = sim
        .spawn(
            SpawnParams {
                origin: Vec3::new(1.0, 2.0, 3.0),
                dir: Vec3::new(0.0, 0.0, 5.0), // not unit-length: spawn normalizes
                ..SpawnParams::default()
            },
            None,
            None,
        )
        .unwrap();
    let p = sim.live_at(idx).unwrap();
    assert!(p.alive);
    assert_eq!(p.pos, Vec3::new(1.0, 2.0, 3.0));
    assert_eq!(p.dir, Vec3::new(0.0, 0.0, 1.0));
    assert_eq!(p.vel, Vec3::new(0.0, 0.0, 800.0)); // default speed
    assert_eq!(p.damage, 30.0);
    assert_eq!(p.penetration, 1.0);
    assert_eq!(p.drag_k, 0.3);
    assert_eq!(p.dropoff, 0.5);
    assert_eq!(p.max_range, 400.0);
    assert_eq!(p.travelled, 0.0);
    assert_eq!(p.age, 0.0);
    assert_eq!(sim.stats.fired, 1);
    assert_eq!(sim.live_count(), 1);
}

#[test]
fn the_oldest_round_yields_its_slot_once_the_pool_is_exhausted() {
    let mut sim = ProjectileSim::new();
    for _ in 0..MAX_LIVE {
        sim.spawn(SpawnParams::default(), None, None).unwrap();
    }
    assert_eq!(sim.live_count(), MAX_LIVE);
    assert_eq!(sim.stats.fired, MAX_LIVE as u32);

    // One more spawn does not grow the live list past the pool size — the
    // oldest round is retired and its slot reused, exactly as the source's
    // "oldest round yields its slot rather than dropping the shot" comment
    // describes.
    sim.spawn(SpawnParams::default(), None, None).unwrap();
    assert_eq!(sim.live_count(), MAX_LIVE);
    assert_eq!(sim.stats.fired, MAX_LIVE as u32 + 1);
}

#[test]
fn fixed_update_integrates_gravity_and_drag_with_no_world_bound() {
    let mut sim = ProjectileSim::new();
    sim.spawn(
        SpawnParams {
            origin: Vec3::ZERO,
            dir: Vec3::new(0.0, 0.0, 1.0),
            speed: 800.0,
            drag_k: 0.3,
            ..SpawnParams::default()
        },
        None,
        None,
    );

    let h = 1.0 / 120.0;
    sim.fixed_update(h, None);

    let p = sim.live_at(0).unwrap();
    // vel.y after one step: 0 + GRAVITY * h, then scaled by the drag decay.
    let decay = (1.0 - 0.3 * h).max(0.0);
    let expected_vy = GRAVITY * h * decay;
    let expected_vz = 800.0 * decay;
    assert_eq!(p.vel.y, expected_vy);
    assert_eq!(p.vel.z, expected_vz);
    assert_eq!(p.pos.y, expected_vy * h);
    assert_eq!(p.pos.z, expected_vz * h);
    assert_eq!(p.age, h);
    assert_eq!(sim.stats.live, 1);
}

#[test]
fn a_round_expires_past_its_max_range() {
    let mut sim = ProjectileSim::new();
    sim.spawn(
        SpawnParams {
            origin: Vec3::ZERO,
            dir: Vec3::new(1.0, 0.0, 0.0),
            speed: 800.0,
            max_range: 1.0, // one physics step overshoots this
            drag_k: 0.0,
            ..SpawnParams::default()
        },
        None,
        None,
    );
    sim.fixed_update(1.0 / 120.0, None);
    assert_eq!(sim.live_count(), 0);
}

#[test]
fn a_round_expires_past_the_altitude_floor() {
    let mut sim = ProjectileSim::new();
    sim.spawn(
        SpawnParams {
            origin: Vec3::new(0.0, -90.0, 0.0),
            dir: Vec3::new(0.0, -1.0, 0.0),
            speed: 1.0,
            max_range: 1_000_000.0,
            drag_k: 0.0,
            ..SpawnParams::default()
        },
        None,
        None,
    );
    sim.fixed_update(1.0 / 120.0, None);
    assert_eq!(sim.live_count(), 0);
}

#[test]
fn a_hit_hands_a_falloff_scaled_request_to_fire_bullet_and_retires_the_round() {
    let mut sim = ProjectileSim::new();
    sim.spawn(
        SpawnParams {
            origin: Vec3::ZERO,
            dir: Vec3::new(0.0, 0.0, 1.0),
            speed: 800.0,
            damage: 30.0,
            dropoff: 0.5,
            max_range: 400.0,
            penetration: 0.7,
            mask: Some(3),
            drag_k: 0.0,
            ..SpawnParams::default()
        },
        None,
        None,
    );

    let mut world = MockWorld::default();
    world.hits.borrow_mut().push(Some(RaycastHit { distance: 6.0 }));
    sim.fixed_update(1.0 / 120.0, Some(&mut world));

    assert_eq!(sim.live_count(), 0);
    assert_eq!(sim.stats.impacts, 1);
    let fired = world.fired.borrow();
    assert_eq!(fired.len(), 1);
    let req = &fired[0];
    assert_eq!(req.penetration, 0.7);
    assert_eq!(req.dropoff, 1.0); // the source always hands fireBullet dropoff: 1
    assert_eq!(req.mask, Some(3));
    // damage = base damage * falloff(dropoff, range01) — both computed from
    // this exact step's travelled distance, so recomputing that distance here
    // (same formula the sim uses internally) pins the relationship, not just
    // one hardcoded number. `drag_k = 0` still lets gravity nudge `vel.y`, so
    // the segment is not purely along +z; the full vector length is used.
    let h = 1.0 / 120.0;
    let vy_after_gravity = GRAVITY * h;
    let seg_y = vy_after_gravity * h;
    let seg_z = 800.0 * h;
    let travelled = (seg_y * seg_y + seg_z * seg_z).sqrt();
    let expected = 30.0 * falloff(0.5, range01(travelled, 400.0));
    assert!((req.damage - expected).abs() < 1e-9);
}

#[test]
fn clear_retires_every_live_round() {
    let mut sim = ProjectileSim::new();
    for _ in 0..5 {
        sim.spawn(SpawnParams::default(), None, None);
    }
    assert_eq!(sim.live_count(), 5);
    sim.clear();
    assert_eq!(sim.live_count(), 0);
}

#[test]
fn vec3_normalize_of_a_zero_vector_stays_zero() {
    // THREE.Vector3.normalize() divides by `length() || 1`, so a zero vector
    // does not become NaN.
    assert_eq!(Vec3::ZERO.normalize(), Vec3::ZERO);
    let n = Vec3::new(3.0, 0.0, 4.0).normalize();
    assert!((n.length() - 1.0).abs() < 1e-12);
}
