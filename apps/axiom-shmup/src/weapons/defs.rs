//! Weapon data.
//!
//! Ported from `C:/dev/Claude-of-Duty/src/weapons/defs.js:1-320` — the whole
//! file.
//!
//! Ballistics are real: 5.56x45 leaves a 14.5" barrel at ~880 m/s, 9x19 from a
//! 4.5" barrel at ~360 m/s, and both drop under gravity on the way to the
//! target. Rates of fire, magazine capacities and ADS times are the real ones
//! too (an M4A1 is 800 rpm and reaches the optic in about 220 ms).
//!
//! Recoil is split in two, exactly as a modern shooter does it:
//!   - `pattern`  a DETERMINISTIC per-shot camera climb a player can memorise
//!                and counter. Generated once from a fixed seed
//!                ([`build_recoil_pattern`]).
//!   - `spread`   a random cone that grows with sustained fire and shrinks when
//!                aiming, crouched or still. This is the part you cannot learn
//!                ([`Stance::spread_mod`]).
//!
//! Everything below is data, ported field-for-field as `const`s rather than the
//! source's plain object literals — Rust has no object literal, and a `struct`
//! with named fields says the same thing with every field checked at compile
//! time.

use crate::rng::Rng;

/// The source imports this from `mathx.js` (`export const DEG2RAD = DEG` where
/// `DEG = Math.PI / 180`, `mathx.js:10`). `mathx.rs` is a sibling module in
/// this port, landing concurrently with this file from a different agent; to
/// avoid a cross-file dependency on code not yet settled, the constant is
/// restated here rather than imported. Once `mathx.rs` is stable this can
/// become `pub use crate::weapons::mathx::DEG as DEG2RAD;` — the value is
/// identical either way, it is exactly `PI / 180`.
pub const DEG2RAD: f64 = std::f64::consts::PI / 180.0;

/// One weapon's recoil tuning — `WEAPON_DEFS.<id>.recoil`, e.g.
/// `defs.js:48-61` for the rifle.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RecoilDef {
    /// Radians of camera climb per shot.
    pub pitch: f64,
    pub yaw: f64,
    /// Metres the viewmodel travels rearward.
    pub kick_back: f64,
    pub kick_up: f64,
    pub roll: f64,
    pub punch: f64,
    pub freq: f64,
    pub damping: f64,
    pub pattern_length: usize,
    pub pattern_seed: u32,
    /// First-shots multiplier. Index `min(shot, len - 1)` — the last entry
    /// holds for every shot past the array's length.
    pub climb_shape: &'static [f64],
    /// How much the pattern wanders horizontally.
    pub drift: f64,
}

/// One weapon's full data row — one `WEAPON_DEFS` value, e.g. `defs.js:19-144`
/// for the rifle.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WeaponDef {
    pub id: &'static str,
    pub label: &'static str,
    pub class: &'static str,
    pub caliber: &'static str,
    /* --- fire control --- */
    pub rpm: f64,
    pub modes: &'static [&'static str],
    pub burst_count: u32,
    pub burst_rpm: f64,
    pub burst_delay: f64,
    /* --- ammunition --- */
    pub mag_size: u32,
    pub reserve: u32,
    /* --- terminal ballistics --- */
    pub muzzle_velocity: f64,
    pub damage: f64,
    pub penetration: f64,
    pub dropoff: f64,
    pub max_range: f64,
    pub drag_k: f64,
    pub tracer_every: u32,
    /* --- accuracy (degrees) --- */
    pub spread_hip: f64,
    pub spread_ads: f64,
    pub spread_per_shot: f64,
    pub spread_max: f64,
    pub spread_decay: f64,
    /* --- recoil --- */
    pub recoil: RecoilDef,
    /* --- handling (seconds) --- */
    pub ads_time: f64,
    pub ads_fov: f64,
    pub view_fov: f64,
    pub reload_tac: f64,
    pub reload_empty: f64,
    pub inspect_time: f64,
    pub draw_time: f64,
    pub holster_time: f64,
    /* --- pose --- */
    pub hip_pos: [f64; 3],
    pub hip_rot: [f64; 3],
    pub ads_cant: [f64; 3],
    pub eye_relief: f64,
    pub sprint_pos: [f64; 3],
    pub sprint_rot: [f64; 3],
    pub low_ready_pos: [f64; 3],
    pub low_ready_rot: [f64; 3],
    pub sway_scale: f64,
    pub bob_scale: f64,
    pub mag_len: f64,
}

/// `WEAPON_DEFS.rifle` — the M4A1, `defs.js:19-144`.
///
/// Pose-derivation notes, ported verbatim from the source comment
/// (`defs.js:71-111`), because the numbers below only make sense with the
/// constraints that produced them:
///
/// Weapon-local origin is the web of the shooting hand (top of the grip). The
/// butt pad is at z=+0.245, the muzzle crown at z=-0.502, the optic ocular at
/// (0, 0.142, +0.006) and the mag floorplate ~150 mm below origin.
///
/// SOLVED FROM THE BORE AXIS, not from where the optic happens to land.
///
/// The previous pose (hipPos [0.081,-0.192,-0.215], hipRot [-0.026,0.076,
/// 0.055]) was derived by putting the OPTIC at a chosen screen position, and
/// that is the wrong constraint: it left the bore 1.5 deg nose-down with the
/// weapon only 215 mm from the eye, so the whole barrel forward of the
/// receiver ran off the top-left of the frame and the muzzle crown — where the
/// flash spawns — projected onto empty street. What reads as "the gun points
/// at the crosshair" is the MUZZLE being visible, up-left of the receiver, on
/// the way to the centre of the screen.
///
/// Constraints, in order:
///   1. bore axis 4.0 deg LEFT of view-forward (converging on the crosshair)
///      and 2.9 deg nose-down: rx = -0.050, ry = +0.070
///   2. rolled 7.7 deg so the LEFT flank of the receiver (the side that
///      carries the rollmark, the bolt catch and the port) faces the camera
///      and the rail deck turns edge-on instead of presenting its lit top
///      face: rz = -0.135
///   3. muzzle crown inside x 1050-1300, y 620-780 at 1920x1080
///   4. optic ocular below and right of screen centre
///   5. magazine + pistol grip in the lower-right frame
///
/// With the rotation above the muzzle offset is (-0.025, +0.049, -0.505) and
/// the ocular offset (+0.019, +0.141, -0.003), so at a 60 deg vertical view
/// FOV (half-height 0.5774|z|, half-width 1.0264|z|): muzzle -> (1064, 698)
/// ocular -> (1374, 677) magwell mouth -> (1268, 870), i.e. the muzzle is 300
/// px up-LEFT of the optic and heading for the middle of the frame, which is
/// the read that was missing.
///
/// z = -0.30 (was -0.215) is what makes the weapon small enough for the mag
/// and grip to enter the frame at all: the gun's vertical extent from optic to
/// floorplate is 291 mm, and at 215 mm from the eye that is 93% of the frame
/// height. It is also the limit — the support hand is then 620 mm downrange of
/// a shoulder 200 mm off the eye, and a 572 mm arm has nothing left. The butt
/// pad ends up 60 mm in FRONT of the eye but 140 mm off axis, so it is outside
/// the frustum rather than clipped by the near plane.
///
/// `eyeRelief` (eye to the rear lens) is MEASURED FROM THE ADS FRAME, not
/// chosen for realism: housing size (the 31 mm tube's outer rim subtends
/// rOuter/relief — 0.115 puts it at 168 px of radius, 31% of frame height,
/// where a modern shooter frames a tube sight) and sight picture (stopped by
/// the objective bore at relief+len, so a longer relief improves
/// relief/(relief+len) — 0.53 to 0.69) both wanted the same direction of
/// change, so both moved together.
pub const RIFLE: WeaponDef = WeaponDef {
    id: "rifle",
    label: "M4A1",
    class: "carbine",
    caliber: "5.56x45",
    rpm: 800.0,
    modes: &["auto", "burst", "semi"],
    burst_count: 3,
    burst_rpm: 950.0,
    burst_delay: 0.16,
    mag_size: 30,
    reserve: 210,
    muzzle_velocity: 880.0,
    damage: 33.0,
    penetration: 1.0,
    dropoff: 0.62,
    max_range: 420.0,
    drag_k: 0.28,
    tracer_every: 3,
    spread_hip: 2.05,
    spread_ads: 0.24,
    spread_per_shot: 0.3,
    spread_max: 3.4,
    spread_decay: 3.6,
    recoil: RecoilDef {
        pitch: 0.0085,
        yaw: 0.0022,
        kick_back: 0.019,
        kick_up: 0.0072,
        roll: 0.032,
        punch: 0.35,
        freq: 8.5,
        damping: 0.42,
        pattern_length: 30,
        pattern_seed: 0x4d34a1,
        climb_shape: &[1.45, 1.3, 1.15, 1.05, 1.0],
        drift: 0.55,
    },
    ads_time: 0.22,
    ads_fov: 0.74,
    view_fov: 0.86,
    reload_tac: 2.1,
    reload_empty: 2.9,
    inspect_time: 3.2,
    draw_time: 0.62,
    holster_time: 0.4,
    hip_pos: [0.118, -0.185, -0.3],
    hip_rot: [-0.05, 0.081, -0.135],
    ads_cant: [0.0, 0.0, 0.004],
    eye_relief: 0.115,
    sprint_pos: [0.09, -0.262, -0.275],
    sprint_rot: [-0.4, 0.6, 0.2],
    low_ready_pos: [0.112, -0.28, -0.289],
    low_ready_rot: [-0.46, 0.125, -0.09],
    sway_scale: 1.0,
    bob_scale: 1.0,
    mag_len: 0.212,
};

/// `WEAPON_DEFS.smg` — the MPX-9, `defs.js:146-209`.
///
/// Pose solved from the bore axis exactly as [`RIFLE`]'s is (see there): 4.1
/// deg of convergence, 2.9 deg nose-down, 7.5 deg of outboard roll, and far
/// enough out that the muzzle of a 210 mm barrel is on screen up-left of the
/// optic. `eyeRelief` follows the same aperture-budget derivation: the 27.6 mm
/// tube's outer rim wants to land near 165 px of radius and the 44 mm bore
/// wants the eye far enough back that the objective is not the stop.
pub const SMG: WeaponDef = WeaponDef {
    id: "smg",
    label: "MPX-9",
    class: "smg",
    caliber: "9x19",
    rpm: 950.0,
    modes: &["auto", "semi"],
    burst_count: 2,
    burst_rpm: 1100.0,
    burst_delay: 0.14,
    mag_size: 32,
    reserve: 224,
    muzzle_velocity: 400.0,
    damage: 24.0,
    penetration: 0.45,
    dropoff: 0.48,
    max_range: 240.0,
    drag_k: 0.42,
    tracer_every: 4,
    spread_hip: 2.5,
    spread_ads: 0.4,
    spread_per_shot: 0.26,
    spread_max: 3.9,
    spread_decay: 4.4,
    recoil: RecoilDef {
        pitch: 0.0058,
        yaw: 0.0026,
        kick_back: 0.0135,
        kick_up: 0.0052,
        roll: 0.026,
        punch: 0.24,
        freq: 10.5,
        damping: 0.4,
        pattern_length: 32,
        pattern_seed: 0x9ac31f,
        climb_shape: &[1.3, 1.18, 1.08, 1.0],
        drift: 0.8,
    },
    ads_time: 0.185,
    ads_fov: 0.78,
    view_fov: 0.88,
    reload_tac: 1.85,
    reload_empty: 2.5,
    inspect_time: 2.9,
    draw_time: 0.52,
    holster_time: 0.34,
    hip_pos: [0.111, -0.163, -0.288],
    hip_rot: [-0.05, 0.072, -0.131],
    ads_cant: [0.0, 0.0, 0.005],
    eye_relief: 0.104,
    sprint_pos: [0.088, -0.24, -0.262],
    sprint_rot: [-0.38, 0.58, 0.19],
    low_ready_pos: [0.108, -0.252, -0.276],
    low_ready_rot: [-0.44, 0.125, -0.085],
    sway_scale: 0.92,
    bob_scale: 0.95,
    mag_len: 0.192,
};

/// `WEAPON_DEFS.pistol` — the P-19, `defs.js:211-272`.
///
/// A pistol is held out on the arms rather than braced on the shoulder, so the
/// hip pose is FURTHER from the eye than a carbine's and the ADS eye relief is
/// most of an arm's length. 0.34 m keeps both elbows visibly bent; past ~0.40 m
/// the two-bone solve hits full extension and they lock.
pub const PISTOL: WeaponDef = WeaponDef {
    id: "pistol",
    label: "P-19",
    class: "pistol",
    caliber: "9x19",
    rpm: 460.0,
    modes: &["semi"],
    burst_count: 1,
    burst_rpm: 460.0,
    burst_delay: 0.1,
    mag_size: 17,
    reserve: 68,
    muzzle_velocity: 360.0,
    damage: 28.0,
    penetration: 0.35,
    dropoff: 0.42,
    max_range: 180.0,
    drag_k: 0.46,
    tracer_every: 5,
    spread_hip: 3.1,
    spread_ads: 0.5,
    spread_per_shot: 0.42,
    spread_max: 4.6,
    spread_decay: 5.2,
    recoil: RecoilDef {
        pitch: 0.0125,
        yaw: 0.0032,
        kick_back: 0.012,
        kick_up: 0.0105,
        roll: 0.018,
        punch: 0.3,
        freq: 9.0,
        damping: 0.45,
        pattern_length: 17,
        pattern_seed: 0x1f77bc,
        climb_shape: &[1.0],
        drift: 1.2,
    },
    ads_time: 0.16,
    ads_fov: 0.86,
    view_fov: 0.92,
    reload_tac: 1.6,
    reload_empty: 2.2,
    inspect_time: 2.6,
    draw_time: 0.42,
    holster_time: 0.3,
    hip_pos: [0.115, -0.15, -0.34],
    hip_rot: [-0.05, 0.066, -0.115],
    ads_cant: [0.0, 0.0, 0.003],
    eye_relief: 0.34,
    sprint_pos: [0.09, -0.25, -0.28],
    sprint_rot: [-0.42, 0.5, 0.14],
    low_ready_pos: [0.1, -0.26, -0.32],
    low_ready_rot: [-0.44, 0.105, -0.07],
    sway_scale: 1.15,
    bob_scale: 1.1,
    mag_len: 0.108,
};

/// `WEAPON_DEFS`, `defs.js:18-273` — every weapon, in the source's
/// declaration order.
pub const WEAPON_DEFS: [&WeaponDef; 3] = [&RIFLE, &SMG, &PISTOL];

/// `WEAPON_DEFS[id]`. A missing id is `undefined` in the source; `None` here.
pub fn by_id(id: &str) -> Option<&'static WeaponDef> {
    WEAPON_DEFS.iter().copied().find(|def| def.id == id)
}

/// Generate the deterministic recoil pattern for a weapon.
/// `buildRecoilPattern(def, Rng)`, `defs.js:285-308`.
///
/// The shape is what a player learns: a strong vertical climb for the first
/// few shots, then the vertical settles while the muzzle starts to wander
/// sideways in a smooth, repeatable S. Everything comes from one fixed seed so
/// the same weapon always kicks the same way — including in capture mode.
///
/// Two shape changes from the source, both dictated by Rust rather than by
/// choice:
///
///   - The source takes `(def, Rng)` — the whole weapon def plus a generator
///     *class* to construct, because JS has no way to say "the concrete `Rng`
///     type" other than passing it as a value. This port only has one `Rng`
///     ([`crate::rng::Rng`]) and it is always the right one, so the signature
///     takes `&RecoilDef` directly (`def.recoil` in the source) and constructs
///     the generator itself.
///   - The source writes into a `Float32Array`, so every `out[i] = …` narrows
///     an `f64` computation down to `f32` on the spot. The port keeps that
///     narrowing explicit (`as f32` at the point of assignment) rather than
///     accumulating in `f64` and narrowing only the returned `Vec` — the
///     source's per-shot `sig`/`snake` terms are computed fresh each
///     iteration and never re-read a previously-narrowed value, so the two
///     orders agree, but the explicit narrowing is what a byte-for-byte
///     golden capture (which reads the JS `Float32Array` back through a
///     getter that widens each `f32` to `f64`) has to match.
///
/// Returns `n` `[pitch, yaw]` pairs in radians — the source's flat
/// `Float32Array` of length `n * 2`, reshaped into pairs because Rust has no
/// need to flatten what it is only ever going to index in twos.
pub fn build_recoil_pattern(recoil: &RecoilDef) -> Vec<[f32; 2]> {
    let n = recoil.pattern_length;
    let mut rng = Rng::new(recoil.pattern_seed);
    let mut out = vec![[0.0f32; 2]; n];

    // Two out-of-phase wanders make the horizontal read as a learnable snake
    // rather than as noise.
    let phase = rng.float() * std::f64::consts::PI * 2.0;
    let phase2 = rng.float() * std::f64::consts::PI * 2.0;
    let bias = rng.signed() * 0.35;

    (0..n).for_each(|shot| {
        let climb = recoil.climb_shape[shot.min(recoil.climb_shape.len() - 1)];
        // Vertical: strong early, tapering, with a per-shot signature bump.
        let sig = 0.88 + rng.float() * 0.24;
        out[shot][0] = (recoil.pitch * climb * sig) as f32;

        // Horizontal: a smooth snake plus a fixed per-shot signature.
        let t = shot as f64 / (n.saturating_sub(1)).max(1) as f64;
        let snake = (phase + t * std::f64::consts::PI * 2.6).sin() * 0.75
            + (phase2 + t * std::f64::consts::PI * 5.1).sin() * 0.35;
        out[shot][1] =
            (recoil.yaw * (snake * recoil.drift * 3.2 + bias + rng.signed() * 0.25)) as f32;
    });

    out
}

/// A movement/aim stance, and how much it scales the spread cone.
/// `SPREAD_MODS`, `defs.js:310-318`.
///
/// The source is a plain object keyed by string; the port is an enum with a
/// method, which gets the same "look up the mod for this stance" behaviour
/// with an exhaustive match instead of a possibly-missing property.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Stance {
    Crouch,
    Prone,
    Still,
    Walking,
    Sprinting,
    Airborne,
    Hipfire,
}

impl Stance {
    /// `SPREAD_MODS[stance]`.
    pub fn spread_mod(self) -> f64 {
        match self {
            Stance::Crouch => 0.78,
            Stance::Prone => 0.6,
            Stance::Still => 0.82,
            Stance::Walking => 1.15,
            Stance::Sprinting => 2.2,
            Stance::Airborne => 2.0,
            Stance::Hipfire => 1.0,
        }
    }
}
