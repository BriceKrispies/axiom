//! The ported player subsystem, pinned against the JavaScript it came from.
//!
//! Golden values were captured by running the original
//! `C:/dev/Claude-of-Duty/src/player/{springs,tuning,mantle,camera}.js` under
//! Node (v24) — see `tests/player_port.rs`'s own history for the capture
//! scripts (deleted after use, per the port recipe: the committed goldens
//! below are the artifact, not the script that produced them).
//!
//! ## What is pinned, and how tightly
//!
//! * **Exactly** — everything reachable by `+ - * /` and comparisons only:
//!   the [`springs`] helpers, [`mantle::LedgeProbe::probe`]'s decision logic
//!   and numeric outputs (no `sin`/`cos`/`sqrt` anywhere in that function),
//!   [`mantle::MantleMotion`]'s position outputs (built from
//!   `smoothstep`/`smootherstep`, both polynomial), and the camera impulse
//!   channels' pure-arithmetic fields (`stepVelocity` from a plain
//!   `impulse()` addition).
//! * **Within `1e-12`** — anything a transcendental touches: [`springs::
//!   hash_noise`], [`springs::Spring::step`]/[`springs::RecoilAxis::step`]
//!   (`exp`), and [`mantle::MantleMotion`]'s camera-garnish fields (`sin`).
//!
//! `movement.rs`/`camera.rs`'s full per-frame integration (`Movement::step`,
//! `CameraRig::update`) is exercised natively against the physics/input seam
//! (see `crate::player`'s module doc comment) rather than pinned against the
//! JavaScript — driving the real `physics.createCharacter`-shaped controller
//! from Node would require a JS collision mock at least as large as this
//! test file. What's tested there is state-machine *behaviour*: the
//! documented transitions (crouch->slide, sprint->tacsprint, jump->fall->land)
//! fire in the right order given a scripted controller.

use std::cell::Cell;

use axiom_shmup::config::FIXED_DT;
use axiom_shmup::engine::Time;
use axiom_shmup::player::camera::{CameraRig, HealthView};
use axiom_shmup::player::mantle::{
    self, CapsuleHit, LedgeCharacter, LedgeKind, LedgeProbe, LedgeResult, MantleMotion, ProbeMask, RayHit, WorldProbe,
};
use axiom_shmup::player::movement::{CharacterController, InputAction, Movement, PlayerInput};
use axiom_shmup::player::springs::{
    angle_delta, approach, clamp, clamp01, ease_in_out_sine, ease_out_cubic, hash_noise, lerp, move_toward,
    smootherstep, smoothstep, RecoilAxis, Spring, DEG, TAU,
};
use axiom_shmup::player::tuning::{self, Stance, MOVE};
use axiom_shmup::player::Vec3;
use axiom_shmup::world::palette::Surface;

/// `sin`/`cos`/`exp` are not bit-guaranteed across libm implementations, so
/// anything a transcendental touches is compared within this absolute
/// tolerance rather than exactly — the same figure `tests/core_port.rs` and
/// `tests/weapons_mathx_port.rs` establish for the same reason.
const TOL: f64 = 1e-12;

fn assert_close(actual: f64, expected: f64, tol: f64, what: &str) {
    assert!(
        (actual - expected).abs() < tol,
        "{what}: expected {expected:.17}, got {actual:.17} (tol {tol})"
    );
}

// =========================================================================
// springs.js
// =========================================================================

#[test]
fn constants_match_the_javascript() {
    assert_eq!(TAU, 6.283185307179586);
    assert_eq!(DEG, 0.017453292519943295);
}

#[test]
fn clamp_helpers_match_the_javascript_exactly() {
    assert_eq!(clamp(5.0, 0.0, 10.0), 5.0);
    assert_eq!(clamp(-5.0, 0.0, 10.0), 0.0);
    assert_eq!(clamp(15.0, 0.0, 10.0), 10.0);
    assert_eq!(clamp01(-0.5), 0.0);
    assert_eq!(clamp01(0.5), 0.5);
    assert_eq!(clamp01(1.5), 1.0);
    assert_eq!(lerp(0.0, 10.0, 0.25), 2.5);
    assert_eq!(lerp(-5.0, 5.0, 0.5), 0.0);
}

#[test]
fn smoothstep_family_matches_the_javascript_exactly() {
    assert_eq!(smoothstep(-0.2), 0.0);
    assert_eq!(smoothstep(0.3), 0.216);
    assert_eq!(smoothstep(0.7), 0.7839999999999999);
    assert_eq!(smoothstep(1.3), 1.0);

    assert_eq!(smootherstep(-0.2), 0.0);
    assert_eq!(smootherstep(0.3), 0.16308000000000003);
    assert_eq!(smootherstep(0.7), 0.8369199999999999);
    assert_eq!(smootherstep(1.3), 1.0);

    assert_eq!(ease_out_cubic(0.0), 0.0);
    assert_eq!(ease_out_cubic(0.3), 0.657);
    assert_eq!(ease_out_cubic(0.7), 0.973);
    assert_eq!(ease_out_cubic(1.0), 1.0);
}

#[test]
fn ease_in_out_sine_matches_the_javascript_within_tolerance() {
    // `cos` is involved, so this one is not exact-equality-eligible.
    assert_close(ease_in_out_sine(0.0), 0.0, TOL, "t=0");
    assert_close(ease_in_out_sine(0.25), 0.1464466094067262, TOL, "t=0.25");
    assert_close(ease_in_out_sine(0.5), 0.49999999999999994, TOL, "t=0.5");
    assert_close(ease_in_out_sine(0.75), 0.8535533905932737, TOL, "t=0.75");
    assert_close(ease_in_out_sine(1.0), 1.0, TOL, "t=1");
}

#[test]
fn approach_and_move_toward_match_the_javascript_exactly() {
    assert_eq!(approach(3.0, 3.0, 0.0, 0.016), 3.0); // tau <= 1e-6 -> target
    assert_eq!(move_toward(0.0, 10.0, 5.0, 0.016), 0.08);
    assert_eq!(move_toward(0.0, 1.0, 5.0, 0.5), 1.0); // overshoots -> clamps
    assert_eq!(move_toward(0.0, -10.0, 5.0, 0.016), -0.08);
}

#[test]
fn approach_with_exp_matches_the_javascript_within_tolerance() {
    assert_close(approach(0.0, 10.0, 0.1, 0.016), 1.4785621103378865, TOL, "approach a");
    assert_close(approach(5.0, -5.0, 0.05, 0.033), 0.16851334491699177, TOL, "approach b");
}

#[test]
fn angle_delta_matches_the_javascript_exactly() {
    assert_eq!(angle_delta(0.0, std::f64::consts::PI * 0.5), 1.5707963267948966);
    assert_eq!(angle_delta(0.0, std::f64::consts::PI * 1.9), -0.3141592653589793);
    assert_eq!(
        angle_delta(std::f64::consts::PI * 0.9, -std::f64::consts::PI * 0.9),
        0.6283185307179586
    );
}

#[test]
fn hash_noise_matches_the_javascript_within_tolerance() {
    assert_close(hash_noise(0.0, 0), -1.0, TOL, "hash(0,0)");
    assert_close(hash_noise(0.5, 0), -0.0734201658051461, TOL, "hash(0.5,0)");
    assert_close(hash_noise(1.25, 7), 0.5067405132722342, TOL, "hash(1.25,7)");
    assert_close(hash_noise(-3.6, 42), -0.24118574757874006, TOL, "hash(-3.6,42)");
}

#[test]
fn spring_step_matches_the_javascript_trace_within_tolerance() {
    let mut s = Spring::new(8.0, 0.7, 0.0);
    s.target = 1.0;
    let expected = [
        0.280291693848817,
        0.6513198141179379,
        0.8948602740975061,
        1.0039984891162745,
        1.0319537235043708,
        1.0264356728527761,
        1.01410285254697,
        1.0049775925725213,
        1.0004627377200084,
        0.9990487882815686,
    ];
    for (i, &want) in expected.iter().enumerate() {
        let got = s.step(1.0 / 60.0);
        assert_close(got, want, TOL, &format!("spring step {i}"));
    }
}

#[test]
fn spring_step_caps_substeps_at_24_and_drops_the_remainder_on_a_big_hitch() {
    // Source quirk, ported and pinned as-is (port recipe rule 7) — see
    // `Spring::step`'s doc comment. A 0.2 s hitch only gets 24 * (1/360) s
    // ~= 66.7 ms of actual integration; the rest of the hitch is dropped
    // rather than carried forward, so the guard-capped result overshoots the
    // value a fully-substepped integration reaches for the same total dt.
    let mut hitch = Spring::new(9.5, 0.52, 0.0);
    hitch.target = 1.0;
    let capped = hitch.step(0.2);
    assert_close(capped, 1.1164936901593498, TOL, "guard-capped hitch");

    let mut fine = Spring::new(9.5, 0.52, 0.0);
    fine.target = 1.0;
    let mut acc = 0.0_f64;
    while acc < 0.2 - 1e-9 {
        fine.step(1.0 / 360.0);
        acc += 1.0 / 360.0;
    }
    assert_close(fine.value, 1.0010470596193315, TOL, "fully substepped");
    assert!(
        (capped - fine.value).abs() > 0.1,
        "the guard cap should visibly under-integrate a big hitch"
    );
}

#[test]
fn spring_impulse_and_denormal_kill_match_the_javascript() {
    let mut s = Spring::new(12.0, 0.62, 0.0);
    s.impulse(3.0);
    let expected = [
        0.013186998797905513,
        0.014697407437995788,
        0.01103073295471869,
        0.006331299849873831,
        0.0025812066594311433,
        0.0003081878962076966,
    ];
    for &want in &expected {
        let got = s.step(1.0 / 120.0);
        assert_close(got, want, TOL, "impulse trace");
    }

    let mut settle = Spring::new(20.0, 1.0, 0.0);
    settle.target = 0.5;
    for _ in 0..200 {
        settle.step(1.0 / 60.0);
    }
    assert_eq!(settle.value, 0.5);
    assert_eq!(settle.velocity, 0.0);
}

#[test]
fn recoil_axis_trace_matches_the_javascript_within_tolerance() {
    let mut r = RecoilAxis::new(9.5, 0.52, 0.3, 0.34);
    r.kick(0.05);
    let expected = [
        0.044805120945423355,
        0.035930346532151866,
        0.02668550356863543,
        0.019024011638837914,
        0.013733746555410589,
        0.010781925672474128,
        0.009677244414792223,
        0.009768244074327182,
        0.010444942581810442,
        0.011244689322223887,
        0.011882239755762402,
        0.012230772824512147,
    ];
    for (i, &want) in expected.iter().enumerate() {
        let got = r.step(1.0 / 120.0);
        assert_close(got, want, TOL, &format!("recoil trace {i}"));
    }
}

#[test]
fn recoil_axis_double_kick_matches_the_javascript_within_tolerance() {
    let mut r = RecoilAxis::default_tuned();
    r.kick(0.1);
    r.step(0.05);
    r.kick(-0.03);
    let expected = [
        -0.0020217197706367436,
        0.01107497166114504,
        0.017992390245539832,
        0.018305410010630768,
        0.01583517669350966,
        0.01349123673190805,
        0.01215733804167391,
        0.01153923595857316,
    ];
    for (i, &want) in expected.iter().enumerate() {
        let got = r.step(1.0 / 60.0);
        assert_close(got, want, TOL, &format!("double-kick trace {i}"));
    }
}

// =========================================================================
// tuning.js
// =========================================================================

#[test]
fn jump_speed_matches_the_javascript_within_the_transcendental_tolerance() {
    // `1e-12`, not the `1e-6` this used to need. The wider bound existed only
    // because `config::UNITS.gravity` was stored as `f32` and `GRAVITY`
    // inherited the round-trip; `UNITS` is `f64` now, so `sqrt` is the sole
    // remaining source of imprecision. Do NOT widen this back — a tolerance
    // that swallows an `f32` round trip is a tolerance that hides the exact
    // storage-width bug this pinned (it broke three assertions in
    // `tests/player_system_port.rs`).
    assert_close(*tuning::JUMP_SPEED, 4.972041834095928, TOL, "JUMP_SPEED");
}

#[test]
fn lean_roll_is_an_exact_multiply_of_deg() {
    // `13 * DEG` is pure multiplication on the same `PI` both languages
    // share bit-for-bit — no tolerance needed.
    assert_eq!(MOVE.lean.roll, 0.22689280275926285);
}

#[test]
fn stance_table_matches_the_javascript() {
    // Exact. These used to need an `f32::EPSILON` tolerance because
    // `config::UNITS` narrowed them; it is `f64` now, so the stance table is
    // the source's own arithmetic bit-for-bit. `1.78 - 0.12` and `1.12 - 0.1`
    // are written as the `f64` results Node prints, not as the decimals they
    // look like.
    assert_eq!(Stance::Stand.def().height, 1.78);
    assert_eq!(Stance::Stand.def().eye, 1.6600000000000001);
    assert_eq!(Stance::Crouch.def().height, 1.12);
    assert_eq!(Stance::Crouch.def().eye, 1.02);
    // `prone` is the source's own literal, not derived from `UNITS` — exact.
    assert_eq!(Stance::Prone.def().height, 0.7);
    assert_eq!(Stance::Prone.def().eye, 0.4);
}

// =========================================================================
// mantle.js — MantleMotion (pure curve evaluation)
// =========================================================================

fn mantle_fast_ledge() -> LedgeResult {
    LedgeResult {
        kind: LedgeKind::Mantle,
        fast: true,
        obstacle_height: 0.5,
        top_y: 0.5,
        lip_x: 0.0,
        lip_z: 0.0,
        land_x: 0.9,
        land_y: 0.5,
        land_z: 0.9,
        distance: 0.0,
        surface: Surface::Concrete,
    }
}

struct StubLedgeChar {
    pos: Vec3,
}
impl LedgeCharacter for StubLedgeChar {
    fn position(&self) -> Vec3 {
        self.pos
    }
    fn radius(&self) -> f64 {
        0.32
    }
    fn check_capsule(&self, _x: f64, _y: f64, _z: f64, _height: f64) -> bool {
        true
    }
}

#[test]
fn mantle_motion_fast_trace_matches_the_javascript() {
    let c1 = StubLedgeChar { pos: [0.0, 0.0, 0.0] };
    let mut m = MantleMotion::new();
    m.begin(&mantle_fast_ledge(), &c1, 0.7071067811865476, 0.7071067811865475, 1.0, 3.0);
    assert_eq!(m.duration, 0.3808000000000001);
    assert_eq!(m.exit_speed, 2.16);

    // sample 0 (after 1 step of 1/120s)
    m.step(1.0 / 120.0);
    assert_eq!(m.t, 0.008333333333333333);
    assert_eq!(m.px, 0.0005912280298928256);
    assert_eq!(m.py, 0.0002333976348188902);
    assert_close(m.cam_y, -0.015062272053452532, TOL, "camY[0]");
    assert_close(m.cam_pitch, -0.018226779318028602, TOL, "camPitch[0]");

    // fast-forward to sample "22" (23 total steps at 1/120s)
    for _ in 0..22 {
        m.step(1.0 / 120.0);
    }
    assert_eq!(m.t, 0.19166666666666665);
    assert_eq!(m.px, 0.1643872000036092);
    assert_eq!(m.py, 0.5324221777831742);
    assert_close(m.cam_forward, 0.04999726997511192, TOL, "camForward[22]");

    // fast-forward through the end of the motion (sample "45": step() returns
    // false once t >= duration).
    let mut alive = true;
    for _ in 0..23 {
        alive = m.step(1.0 / 120.0);
    }
    assert!(!alive);
    assert_eq!(m.px, 0.9);
    assert_eq!(m.py, 0.5);
    assert_eq!(m.pz, 0.9);
    assert_close(m.cam_pitch, 0.03665191429188092, TOL, "camPitch at rest");

    // Continuing to step an ended-but-not-`end()`-ed motion holds steady —
    // `progress()` clamps at 1.
    for _ in 0..43 {
        m.step(1.0 / 120.0);
    }
    assert_eq!(m.px, 0.9);
    assert_eq!(m.py, 0.5);
}

#[test]
fn mantle_motion_tall_and_vault_traces_match_the_javascript() {
    let c1 = StubLedgeChar { pos: [0.0, 0.0, 0.0] };

    // A tall (non-"fast") mantle.
    let tall = LedgeResult {
        kind: LedgeKind::Mantle,
        fast: false,
        obstacle_height: 1.6,
        top_y: 1.6,
        land_x: 0.6,
        land_y: 1.6,
        land_z: 0.2,
        ..mantle_fast_ledge()
    };
    let mut m = MantleMotion::new();
    m.begin(&tall, &c1, 1.0, 0.0, -1.0, 1.5);
    assert_eq!(m.duration, 0.7757522123893805);
    assert_eq!(m.exit_speed, 1.35);
    for _ in 0..109 {
        m.step(1.0 / 120.0);
    }
    assert_eq!(m.px, 0.6);
    assert_eq!(m.py, 1.6);
    assert_eq!(m.pz, 0.2);

    // A fast vault.
    let vault_fast = LedgeResult {
        kind: LedgeKind::Vault,
        fast: true,
        obstacle_height: 0.4,
        top_y: 0.4,
        land_x: 1.1,
        land_y: 0.0,
        land_z: 0.0,
        ..mantle_fast_ledge()
    };
    let mut v = MantleMotion::new();
    v.begin(&vault_fast, &c1, 1.0, 0.0, 1.0, 6.0);
    assert_eq!(v.duration, 0.34);
    assert_eq!(v.exit_speed, 5.28);
    v.step(1.0 / 120.0);
    assert_eq!(v.px, 0.0019500182810532902);
    assert_close(v.cam_y, -0.003461568938750872, TOL, "vault camY[0]");
    for _ in 0..47 {
        v.step(1.0 / 120.0);
    }
    assert_eq!(v.px, 1.1);
    assert_eq!(v.py, 0.0);

    // A slow ("not fast") vault.
    let vault_slow = LedgeResult {
        kind: LedgeKind::Vault,
        fast: false,
        obstacle_height: 0.65,
        top_y: 0.65,
        land_x: 0.0,
        land_y: 0.65,
        land_z: 1.0,
        ..mantle_fast_ledge()
    };
    let mut sv = MantleMotion::new();
    sv.begin(&vault_slow, &c1, 0.0, 1.0, -1.0, 1.0);
    assert_eq!(sv.duration, 0.493);
    assert_eq!(sv.exit_speed, 2.6);
    for _ in 0..77 {
        sv.step(1.0 / 120.0);
    }
    assert_eq!(sv.py, 0.65);
    assert_eq!(sv.pz, 1.0);
}

#[test]
fn mantle_motion_is_framerate_independent() {
    let c1 = StubLedgeChar { pos: [0.0, 0.0, 0.0] };
    let ledge = mantle_fast_ledge();

    let mut a = MantleMotion::new();
    a.begin(&ledge, &c1, 0.7071067811865476, 0.7071067811865475, 1.0, 3.0);
    a.step(1.0 / 30.0);

    let mut b = MantleMotion::new();
    b.begin(&ledge, &c1, 0.7071067811865476, 0.7071067811865475, 1.0, 3.0);
    b.step(1.0 / 60.0);
    b.step(1.0 / 60.0);

    assert_eq!(a.px, b.px);
    assert_eq!(a.py, b.py);
    assert_eq!(a.pz, b.pz);
    assert_close(a.cam_y, b.cam_y, TOL, "rate-independent camY");
}

// =========================================================================
// mantle.js — LedgeProbe (scripted physics/character)
// =========================================================================

struct ScriptedWorld {
    capsule_cast: Option<CapsuleHit>,
    raycasts: Vec<Option<RayHit>>,
    raycast_idx: Cell<usize>,
}

impl WorldProbe for ScriptedWorld {
    fn raycast(&self, _origin: Vec3, _dir: Vec3, _max_dist: f64, _mask: ProbeMask) -> Option<RayHit> {
        let i = self.raycast_idx.get();
        self.raycast_idx.set(i + 1);
        self.raycasts.get(i).copied().flatten()
    }

    fn capsule_cast(
        &self,
        _p0: Vec3,
        _p1: Vec3,
        _radius: f64,
        _dir: Vec3,
        _max_dist: f64,
        _mask: ProbeMask,
    ) -> Option<CapsuleHit> {
        self.capsule_cast
    }

    fn check_capsule_segment(&self, _p0: Vec3, _p1: Vec3, _radius: f64, _mask: ProbeMask) -> bool {
        false
    }
}

struct ScriptedChar {
    pos: Vec3,
    radius: f64,
    fits: Vec<bool>,
    fit_idx: Cell<usize>,
}

impl LedgeCharacter for ScriptedChar {
    fn position(&self) -> Vec3 {
        self.pos
    }
    fn radius(&self) -> f64 {
        self.radius
    }
    fn check_capsule(&self, _x: f64, _y: f64, _z: f64, _height: f64) -> bool {
        let i = self.fit_idx.get();
        self.fit_idx.set(i + 1);
        self.fits.get(i).copied().unwrap_or(false)
    }
}

#[test]
fn ledge_probe_returns_none_with_no_wall_in_front() {
    let world = ScriptedWorld {
        capsule_cast: None,
        raycasts: vec![],
        raycast_idx: Cell::new(0),
    };
    let c = ScriptedChar {
        pos: [0.0, 0.0, 0.0],
        radius: 0.32,
        fits: vec![],
        fit_idx: Cell::new(0),
    };
    let mut probe = LedgeProbe::new();
    let kind = probe.probe(&world, &c, 0.0, 1.0, 1.78);
    assert_eq!(kind, LedgeKind::None);
}

#[test]
fn ledge_probe_returns_none_when_the_top_is_not_walkable() {
    let world = ScriptedWorld {
        capsule_cast: Some(CapsuleHit {
            normal: [0.0, 0.0, -1.0],
            distance: 0.4,
            surface: Surface::Concrete,
        }),
        raycasts: vec![Some(RayHit {
            point: [0.0, 0.9, 0.68],
            normal: [0.9, 0.1, 0.0],
            surface: Surface::Concrete,
        })],
        raycast_idx: Cell::new(0),
    };
    let c = ScriptedChar {
        pos: [0.0, 0.0, 0.0],
        radius: 0.32,
        fits: vec![],
        fit_idx: Cell::new(0),
    };
    let mut probe = LedgeProbe::new();
    assert_eq!(probe.probe(&world, &c, 0.0, 1.0, 1.78), LedgeKind::None);
}

#[test]
fn ledge_probe_mantles_a_wide_deep_ledge() {
    let world = ScriptedWorld {
        capsule_cast: Some(CapsuleHit {
            normal: [0.0, 0.0, -1.0],
            distance: 0.4,
            surface: Surface::Concrete,
        }),
        raycasts: vec![
            Some(RayHit {
                point: [0.0, 0.9, 0.7352000000000001],
                normal: [0.0, 1.0, 0.0],
                surface: Surface::Concrete,
            }),
            Some(RayHit {
                point: [0.0, 0.9, 1.1952],
                normal: [0.0, 1.0, 0.0],
                surface: Surface::Concrete,
            }),
        ],
        raycast_idx: Cell::new(0),
    };
    let c = ScriptedChar {
        pos: [0.0, 0.0, 0.0],
        radius: 0.32,
        fits: vec![true, true],
        fit_idx: Cell::new(0),
    };
    let mut probe = LedgeProbe::new();
    let kind = probe.probe(&world, &c, 0.0, 1.0, 1.78);
    assert_eq!(kind, LedgeKind::Mantle);
    let r = probe.result;
    assert_eq!(r.obstacle_height, 0.9);
    assert_eq!(r.top_y, 0.9);
    assert_eq!(r.lip_z, 0.7352000000000001);
    assert_eq!(r.land_x, 0.0);
    assert_eq!(r.land_y, 0.9);
    assert_eq!(r.land_z, 1.1952);
    assert_eq!(r.distance, 0.4);
    assert!(!r.fast);
    assert_eq!(r.surface, Surface::Concrete);
}

#[test]
fn ledge_probe_mantles_at_crouch_clearance_when_full_stand_fails() {
    let world = ScriptedWorld {
        capsule_cast: Some(CapsuleHit {
            normal: [0.0, 0.0, -1.0],
            distance: 0.4,
            surface: Surface::Concrete,
        }),
        raycasts: vec![
            Some(RayHit {
                point: [0.0, 0.9, 0.7352000000000001],
                normal: [0.0, 1.0, 0.0],
                surface: Surface::Concrete,
            }),
            Some(RayHit {
                point: [0.0, 0.9, 1.1952],
                normal: [0.0, 1.0, 0.0],
                surface: Surface::Concrete,
            }),
        ],
        raycast_idx: Cell::new(0),
    };
    let c = ScriptedChar {
        pos: [0.0, 0.0, 0.0],
        radius: 0.32,
        // full-stand check fails, crouch-height near-lip check passes, head
        // clearance passes.
        fits: vec![false, true, true],
        fit_idx: Cell::new(0),
    };
    let mut probe = LedgeProbe::new();
    let kind = probe.probe(&world, &c, 0.0, 1.0, 1.78);
    assert_eq!(kind, LedgeKind::Mantle);
    let r = probe.result;
    assert_eq!(r.land_x, 0.0);
    assert_eq!(r.land_y, 0.9);
    assert_eq!(r.land_z, 1.0352000000000001);
}

#[test]
fn ledge_probe_vaults_a_thin_rail() {
    let world = ScriptedWorld {
        capsule_cast: Some(CapsuleHit {
            normal: [0.0, 0.0, -1.0],
            distance: 0.35,
            surface: Surface::Metal,
        }),
        raycasts: vec![
            // top: a thin rail lip, 0.5m above the feet
            Some(RayHit {
                point: [0.0, 0.5, 0.6852],
                normal: [0.0, 1.0, 0.0],
                surface: Surface::Metal,
            }),
            // deep: well below topY - 0.14 -> not deepSupported
            Some(RayHit {
                point: [0.0, -1.0, 1.1452],
                normal: [0.0, 1.0, 0.0],
                surface: Surface::Concrete,
            }),
            // floor beyond the rail
            Some(RayHit {
                point: [0.0, 0.0, 1.4252],
                normal: [0.0, 1.0, 0.0],
                surface: Surface::Concrete,
            }),
        ],
        raycast_idx: Cell::new(0),
    };
    let c = ScriptedChar {
        pos: [0.0, 0.0, 0.0],
        radius: 0.32,
        fits: vec![true, true],
        fit_idx: Cell::new(0),
    };
    let mut probe = LedgeProbe::new();
    let kind = probe.probe(&world, &c, 0.0, 1.0, 1.78);
    assert_eq!(kind, LedgeKind::Vault);
    let r = probe.result;
    assert!(r.fast); // 0.5 <= autoVaultMax (0.72)
    assert_eq!(r.obstacle_height, 0.5);
    assert_eq!(r.top_y, 0.5);
    assert_eq!(r.land_x, 0.0);
    assert_eq!(r.land_y, 0.0);
    assert_eq!(r.land_z, 1.4252);
    assert_eq!(r.distance, 0.35);
    assert_eq!(r.surface, Surface::Metal);
    assert_eq!(mantle::ledge_kind_name(kind), "vault");
}

// =========================================================================
// camera.js — impulse channels
// =========================================================================

#[test]
fn on_land_below_min_speed_does_nothing() {
    let mut cam = CameraRig::new(80.0);
    assert_eq!(cam.on_land(0.0), 0.0);
    assert_eq!(cam.on_land(1.0), 0.0);
    assert_eq!(cam.on_land(2.2), 0.0); // exactly at CAMERA.land.minSpeed
    assert_eq!(cam.trauma, 0.0);
}

#[test]
fn on_land_matches_the_javascript_within_tolerance() {
    for (speed, mag, dip_velocity, trauma) in [
        (5.0_f64, 0.3914812630047285_f64, -0.9199809680611121_f64, 0.052107576956484325_f64),
        (8.0, 0.661340856620475, -1.5541510130581164, 0.14870638773607128),
        (20.0, 1.0, -2.35, 0.34),
    ] {
        let mut cam = CameraRig::new(80.0);
        let got_mag = cam.on_land(speed);
        assert_close(got_mag, mag, TOL, "onLand mag");
        assert_close(cam.trauma, trauma, TOL, "onLand trauma");
        assert_close(cam.dip.velocity, dip_velocity, TOL, "onLand dip velocity");
    }
}

#[test]
fn on_footstep_impulse_matches_the_javascript_exactly() {
    // `impulse()` is a pure addition on `step.velocity` — exact-equality
    // eligible, and `CameraRig::step` is `pub` (matching the source: these
    // spring channels were never hidden — see camera.rs's field doc comment).
    let cases: [(Stance, bool, f64); 6] = [
        (Stance::Stand, false, -0.085),
        (Stance::Stand, true, -0.14450000000000002),
        (Stance::Crouch, false, -0.04675000000000001),
        (Stance::Crouch, true, -0.07947500000000002),
        (Stance::Prone, false, -0.025500000000000002),
        (Stance::Prone, true, -0.04335000000000001),
    ];
    for (stance, running, expected_velocity) in cases {
        let mut cam = CameraRig::new(80.0);
        cam.on_footstep(running, stance);
        assert_eq!(cam.step.velocity, expected_velocity);
    }
}

// =========================================================================
// movement.rs — native state-machine behaviour (no JS pin: physics/input
// seam, per the module doc comment).
// =========================================================================

struct FakeChar {
    pos: Vec3,
    vel: Vec3,
    radius: f64,
    height: f64,
    step_height: f64,
    grounded: bool,
}

impl FakeChar {
    fn new() -> Self {
        FakeChar {
            pos: [0.0, 0.0, 0.0],
            vel: [0.0, 0.0, 0.0],
            radius: 0.32,
            height: tuning::STAND.height,
            step_height: tuning::STAND.step_height,
            grounded: true,
        }
    }
}

impl LedgeCharacter for FakeChar {
    fn position(&self) -> Vec3 {
        self.pos
    }
    fn radius(&self) -> f64 {
        self.radius
    }
    fn check_capsule(&self, _x: f64, _y: f64, _z: f64, _height: f64) -> bool {
        true
    }
}

impl CharacterController for FakeChar {
    fn height(&self) -> f64 {
        self.height
    }
    fn set_height(&mut self, h: f64) {
        self.height = h;
    }
    fn step_height(&self) -> f64 {
        self.step_height
    }
    fn set_step_height(&mut self, h: f64) {
        self.step_height = h;
    }
    fn grounded(&self) -> bool {
        self.grounded
    }
    fn set_grounded(&mut self, g: bool) {
        self.grounded = g;
    }
    fn velocity(&self) -> Vec3 {
        self.vel
    }
    fn set_velocity(&mut self, v: Vec3) {
        self.vel = v;
    }
    fn can_fit(&self, _height: f64) -> bool {
        true
    }
    fn last_move_blocked(&self) -> bool {
        false
    }
    fn touching_ceiling(&self) -> bool {
        false
    }
    fn ground_normal(&self) -> Vec3 {
        [0.0, 1.0, 0.0]
    }
    fn ground_friction(&self) -> f64 {
        0.55
    }
    fn ground_surface(&self) -> Surface {
        Surface::Concrete
    }
    fn landing_speed(&self) -> f64 {
        0.0
    }
    fn move_by(&mut self, dx: f64, dy: f64, dz: f64) -> f64 {
        self.pos[0] += dx;
        self.pos[1] += dy;
        self.pos[2] += dz;
        if self.pos[1] <= 0.0 {
            self.pos[1] = 0.0;
            self.vel[1] = 0.0;
            self.grounded = true;
        } else {
            self.grounded = false;
        }
        (dx * dx + dy * dy + dz * dz).sqrt()
    }
    fn teleport_to(&mut self, x: f64, y: f64, z: f64) {
        self.pos = [x, y, z];
    }
    fn set_position(&mut self, x: f64, y: f64, z: f64) {
        self.pos = [x, y, z];
    }
    fn depenetrate(&mut self, _iterations: u32) {}
    fn probe_ground(&mut self) {
        self.grounded = self.pos[1] <= 1e-9;
    }
}

#[derive(Default, Clone, Copy)]
struct FakeInput {
    move_x: f64,
    move_y: f64,
    jump: bool,
    crouch: bool,
    sprint: bool,
}

impl PlayerInput for FakeInput {
    fn move_vector(&self) -> (f64, f64) {
        (self.move_x, self.move_y)
    }
    fn action(&self, action: InputAction) -> bool {
        match action {
            InputAction::Jump => self.jump,
            InputAction::Crouch => self.crouch,
            InputAction::Sprint => self.sprint,
            InputAction::Prone | InputAction::LeanLeft | InputAction::LeanRight => false,
        }
    }
    fn stick_move_y(&self) -> f64 {
        0.0
    }
    fn ads(&self) -> bool {
        false
    }
}

fn tick(m: &mut Movement, input: &FakeInput, frame: u64) -> Time {
    let time = Time {
        elapsed: frame as f64 * FIXED_DT,
        raw: frame as f64 * FIXED_DT,
        dt: FIXED_DT,
        fixed: FIXED_DT,
        alpha: 1.0,
        scale: 1.0,
        frame,
    };
    m.latch_input(&time, input);
    m.step(&time, None);
    time
}

#[test]
fn sprint_then_crouch_press_starts_a_slide() {
    let mut m = Movement::new();
    m.init(Box::new(FakeChar::new()), None);

    let mut input = FakeInput {
        move_y: 1.0,
        sprint: true,
        ..FakeInput::default()
    };
    let mut frame = 0_u64;
    // Run sprinting forward until above the slide's minimum entry speed.
    for _ in 0..200 {
        tick(&mut m, &input, frame);
        frame += 1;
    }
    assert!(m.sprinting, "should be sprinting after holding forward+sprint");
    assert!(
        m.velocity[0].hypot(m.velocity[2]) >= MOVE.slide.min_speed_to_start,
        "should have reached slide-eligible speed"
    );

    input.crouch = true;
    tick(&mut m, &input, frame);
    frame += 1;
    assert!(m.sliding, "crouch press while sprinting fast should start a slide");
    assert!(m.slide_started);
    assert_eq!(m.stance, Stance::Crouch);

    // Release crouch so the edge-triggered press doesn't relatch every tick.
    input.crouch = false;
    // Run out the slide's duration; it should end back in a crouch.
    for _ in 0..200 {
        tick(&mut m, &input, frame);
        frame += 1;
        if !m.sliding {
            break;
        }
    }
    assert!(!m.sliding, "slide should end within its duration cap");
    assert!(m.slide_ended);
}

#[test]
fn double_tap_sprint_enters_tactical_sprint() {
    let mut m = Movement::new();
    m.init(Box::new(FakeChar::new()), None);

    let mut input = FakeInput {
        move_y: 1.0,
        sprint: false,
        ..FakeInput::default()
    };
    let mut frame = 0_u64;

    // First tap.
    input.sprint = true;
    tick(&mut m, &input, frame);
    frame += 1;
    input.sprint = false;
    tick(&mut m, &input, frame);
    frame += 1;

    // Second tap, inside the tap window (tacSprintTapWindow = 0.32s).
    input.sprint = true;
    tick(&mut m, &input, frame);
    frame += 1;

    // Hold sprint long enough to actually engage (sprintStartDelay = 0.05s).
    for _ in 0..20 {
        tick(&mut m, &input, frame);
        frame += 1;
    }

    assert!(m.sprinting);
    assert!(m.tactical_sprint, "a double-tap within the window should enter tactical sprint");
}

#[test]
fn jump_transitions_through_jump_fall_and_lands() {
    use axiom_shmup::player::movement::MovementState;

    let mut m = Movement::new();
    m.init(Box::new(FakeChar::new()), None);

    let mut input = FakeInput::default();
    let mut frame = 0_u64;

    input.jump = true;
    tick(&mut m, &input, frame);
    frame += 1;
    assert_eq!(m.state, MovementState::Jump);
    assert!(m.jumped);
    assert!(m.velocity[1] > 0.0);

    input.jump = false;
    // Run until velocity turns downward -> state should read `Fall`.
    let mut saw_fall = false;
    let mut saw_land = false;
    for _ in 0..300 {
        tick(&mut m, &input, frame);
        frame += 1;
        if m.state == MovementState::Fall {
            saw_fall = true;
        }
        if m.land_event.pending {
            saw_land = true;
            break;
        }
    }
    assert!(saw_fall, "should pass through Fall on the way down");
    assert!(saw_land, "should emit a land event on touching down");
    assert!(m.grounded);
}

#[test]
fn camera_rig_responds_to_a_recoil_impulse() {
    let mut cam = CameraRig::new(80.0);
    cam.add_recoil(0.05, 0.0, 0.0, 0.0);

    let mut m = Movement::new();
    m.init(Box::new(FakeChar::new()), None);
    let time = Time {
        elapsed: 0.0,
        raw: 0.0,
        dt: FIXED_DT,
        fixed: FIXED_DT,
        alpha: 1.0,
        scale: 1.0,
        frame: 0,
    };
    let config = axiom_shmup::config::Config::default();
    let health = HealthView::default();

    let pitch_before = cam.rotation.pitch;
    cam.update(1.0 / 120.0, &mut m, health, &config, &time);
    assert!(
        cam.rotation.pitch != pitch_before,
        "a recoil kick should move the composed pitch away from its rest value"
    );

    // Stepping many more frames with no further impulse should let it decay
    // back toward (not necessarily all the way to, since breathing sway is
    // still live) its rest neighbourhood.
    let peak = cam.rotation.pitch.abs();
    for _ in 0..600 {
        cam.update(1.0 / 120.0, &mut m, health, &config, &time);
    }
    assert!(
        cam.rotation.pitch.abs() < peak,
        "the recoil channel should have decayed well below its initial peak"
    );
}
