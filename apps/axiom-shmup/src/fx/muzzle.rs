//! Ported from Claude-of-Duty `src/fx/muzzle.js:1-473` — the whole file.
//!
//! Muzzle flash: an incandescent core at the crown, an asymmetric set of
//! lobes thrown by whatever device is on the muzzle, a forward gas jet,
//! unburnt powder grains, and half a second of drifting smoke. Every shot
//! re-rolls lobe count, roll, scale, colour temperature and jet length.
//!
//! ## The camera seam
//!
//! [`screen_angle`] (`muzzle.js:38-45`) needs the camera's inverse-world
//! matrix to turn a world-space lobe direction into the *view-space* angle
//! the sprite rotation uses. No camera/view type exists in this port yet
//! (`player`/camera integration is a different, concurrently-developed
//! slice), so [`screen_angle`] takes the camera's view-space right/up basis
//! directly — `[[rx, ry, rz], [ux, uy, uz]]`, the two rows of
//! `matrixWorldInverse` the source actually reads (`m[0], m[4], m[8]` and
//! `m[1], m[5], m[9]`) — rather than a live camera object, and returns `0.0`
//! when `None`, exactly like the source's `if (!cam) return 0;`. Whatever
//! lands the camera/viewmodel integration supplies that basis.

use crate::fx::particles::reset_spawn;
use crate::fx::system::FxSystem;
use crate::fx::util::{basis, blackbody, clamp_cone, cone, toward_hemi, COS55};

const TWO_PI: f64 = std::f64::consts::PI * 2.0;

/// `screenAngle(fx, view, dx, dy, dz)`, `muzzle.js:38-45`. See the module
/// doc for the `camera_basis` seam.
pub fn screen_angle(camera_basis: Option<([f64; 3], [f64; 3])>, dx: f64, dy: f64, dz: f64) -> f64 {
    match camera_basis {
        None => 0.0,
        Some((right, up)) => {
            let vx = right[0] * dx + right[1] * dy + right[2] * dz;
            let vy = up[0] * dx + up[1] * dy + up[2] * dz;
            vy.atan2(vx)
        }
    }
}

/// One entry of `MUZZLE_PROFILES`, `muzzle.js:59-68`.
#[derive(Debug, Clone, Copy)]
pub struct MuzzleProfile {
    pub scale: f64,
    pub lobes: i32,
    pub jet: f64,
    pub light: f64,
    pub smoke: f64,
    pub temp: f64,
    pub sparks: i32,
}

macro_rules! profile {
    ($scale:expr, $lobes:expr, $jet:expr, $light:expr, $smoke:expr, $temp:expr, $sparks:expr) => {
        MuzzleProfile {
            scale: $scale,
            lobes: $lobes,
            jet: $jet,
            light: $light,
            smoke: $smoke,
            temp: $temp,
            sparks: $sparks,
        }
    };
}

pub const RIFLE: MuzzleProfile = profile!(1.0, 3, 1.0, 200.0, 1.0, 1.0, 8);
pub const CARBINE: MuzzleProfile = profile!(0.92, 3, 0.9, 175.0, 0.9, 1.02, 7);
pub const SMG: MuzzleProfile = profile!(0.78, 3, 0.75, 135.0, 0.7, 1.05, 6);
pub const PISTOL: MuzzleProfile = profile!(0.66, 2, 0.6, 100.0, 0.55, 1.06, 5);
pub const SHOTGUN: MuzzleProfile = profile!(1.5, 4, 1.35, 330.0, 1.8, 0.92, 14);
pub const SNIPER: MuzzleProfile = profile!(1.35, 3, 1.6, 290.0, 1.4, 0.95, 10);
pub const LMG: MuzzleProfile = profile!(1.15, 4, 1.1, 235.0, 1.2, 0.98, 9);
pub const SUPPRESSED: MuzzleProfile = profile!(0.34, 2, 0.4, 36.0, 1.6, 1.1, 2);

/// `MUZZLE_PROFILES`, `muzzle.js:70-79`, keyed the same order the source's
/// `for (const name in MUZZLE_PROFILES)` scan visits (JS preserves
/// string-key insertion order for non-integer keys).
pub const MUZZLE_PROFILES: &[(&str, MuzzleProfile)] = &[
    ("rifle", RIFLE),
    ("carbine", CARBINE),
    ("smg", SMG),
    ("pistol", PISTOL),
    ("shotgun", SHOTGUN),
    ("sniper", SNIPER),
    ("lmg", LMG),
    ("suppressed", SUPPRESSED),
];

/// `REF_LIGHT`, `muzzle.js:82`.
pub const REF_LIGHT: f64 = RIFLE.light;

/// `profileFor(weapon)`, `muzzle.js:84-91`. `weapon` is `None` for "no
/// weapon key given" (`!weapon`); a caller with a class/kind/name string
/// passes it lower-cased already, matching `String(key).toLowerCase()`.
pub fn profile_for(weapon: Option<&str>) -> MuzzleProfile {
    let Some(key) = weapon else {
        return RIFLE;
    };
    let k = key.to_lowercase();
    for (name, prof) in MUZZLE_PROFILES {
        if k.contains(name) {
            return *prof;
        }
    }
    RIFLE
}

/// `muzzleFlash(fx, o)`'s parameters, `muzzle.js:93-107`.
pub struct MuzzleFlashOpts<'a> {
    pub position: (f64, f64, f64),
    pub direction: (f64, f64, f64),
    pub weapon: Option<&'a str>,
    pub intensity: Option<f64>,
    pub scale: Option<f64>,
    pub light: Option<f64>,
    pub view: bool,
    pub light_pos: Option<(f64, f64, f64)>,
    pub camera_basis: Option<([f64; 3], [f64; 3])>,
}

/// `muzzleFlash(fx, o)`, `muzzle.js:109-338`.
pub fn muzzle_flash(fx: &mut FxSystem, o: &MuzzleFlashOpts) -> MuzzleProfile {
    let prof = profile_for(o.weapon);
    let p = o.position;
    let d = o.direction;
    let gain = o.intensity.unwrap_or(1.0) * (0.86 + fx.rng.float() * 0.28);
    let light_gain = gain * o.light.unwrap_or(1.0);
    let ember_gain = 0.22 + 0.78 * gain.min(1.0);
    let sc = prof.scale * o.scale.unwrap_or_else(|| o.intensity.unwrap_or(1.0)) * (0.88 + fx.rng.float() * 0.24);
    let view = o.view;
    let temp = prof.temp * (0.96 + fx.rng.float() * 0.09);
    let core_k = 3800.0 + fx.rng.float() * 600.0;
    let (cr, cg, cb) = blackbody(core_k * temp);
    let (gr, gg, gb) = blackbody(core_k * 0.62 * temp);
    let (tr, tg, tb) = blackbody(2200.0);

    // --- incandescent core at the crown ------------------------------------
    let mut s = reset_spawn();
    s.x = p.0;
    s.y = p.1;
    s.z = p.2;
    s.tile = crate::fx::atlas::p::FLASH_CORE as f64;
    s.size0 = 0.085 * sc;
    s.size1 = 0.135 * sc;
    s.size_curve = 0.35;
    s.life = 0.062;
    s.drag = 10.0;
    s.r0 = gr;
    s.g0 = gg;
    s.b0 = gb;
    s.i0 = 17.0 * gain;
    s.r1 = tr;
    s.g1 = tg;
    s.b1 = tb;
    s.i1 = 4.5 * gain;
    s.alpha_curve = 0.7;
    s.soft = 0.15;
    s.rot = fx.rng.float() * TWO_PI;
    s.spin = fx.rng.signed() * 3.0;
    s.seed = fx.rng.float();
    fx.emit_add_view(view, &s);

    let mut s = reset_spawn();
    s.x = p.0;
    s.y = p.1;
    s.z = p.2;
    s.tile = crate::fx::atlas::p::FLASH_CORE as f64;
    s.size0 = 0.036 * sc;
    s.size1 = 0.056 * sc;
    s.size_curve = 0.4;
    s.life = 0.046;
    s.drag = 12.0;
    s.r0 = cr;
    s.g0 = cg;
    s.b0 = cb;
    s.i0 = 38.0 * gain;
    s.r1 = gr;
    s.g1 = gg;
    s.b1 = gb;
    s.i1 = 6.0 * gain;
    s.alpha_curve = 0.6;
    s.soft = 0.12;
    s.rot = fx.rng.float() * TWO_PI;
    s.spin = fx.rng.signed() * 4.0;
    s.seed = fx.rng.float();
    fx.emit_add_view(view, &s);

    // --- petals --------------------------------------------------------------
    let (btx, bty, btz, bbx, bby, bbz) = basis(d.0, d.1, d.2);
    let ports = ((prof.lobes + i32::from(fx.rng.float() < 0.45)).max(3)).min(4);
    let roll_base = fx.rng.float() * TWO_PI;
    let weak = fx.rng.int(0, ports - 1);
    let big = fx.rng.int(0, ports - 1);
    for i in 0..ports {
        let roll = roll_base + (f64::from(i) / f64::from(ports)) * TWO_PI + fx.rng.signed() * 0.55;
        let pitch = fx.rng.range(0.40, 0.85);
        let cp = pitch.cos();
        let sp = pitch.sin();
        let rc = roll.cos() * sp;
        let rs = roll.sin() * sp;
        let lobe = (
            d.0 * cp + btx * rc + bbx * rs,
            d.1 * cp + bty * rc + bby * rs,
            d.2 * cp + btz * rc + bbz * rs,
        );
        let choke = if i == weak {
            fx.rng.range(0.4, 0.62)
        } else if i == big {
            fx.rng.range(1.1, 1.35)
        } else {
            fx.rng.range(0.78, 1.0)
        };
        let push = fx.rng.range(0.6, 2.2) * choke;
        let mut s = reset_spawn();
        s.tile = crate::fx::atlas::p::FLASH_LOBE as f64;
        s.size0 = (0.06 + 0.05 * fx.rng.float()) * sc * choke;
        s.size1 = (0.18 + 0.14 * fx.rng.float().powf(1.5)) * sc * choke;
        let off = 0.34 * s.size1;
        s.x = p.0 + lobe.0 * off;
        s.y = p.1 + lobe.1 * off;
        s.z = p.2 + lobe.2 * off;
        s.vx = lobe.0 * push;
        s.vy = lobe.1 * push;
        s.vz = lobe.2 * push;
        s.size_curve = 0.35;
        s.life = 0.040 + fx.rng.float() * 0.020;
        s.drag = 12.0;
        s.rot = screen_angle(o.camera_basis, lobe.0, lobe.1, lobe.2) + fx.rng.signed() * 0.1;
        s.spin = fx.rng.signed() * 1.6;
        s.r0 = gr;
        s.g0 = gg;
        s.b0 = gb;
        s.i0 = (8.0 + fx.rng.float() * 6.0) * gain * choke;
        s.r1 = tr;
        s.g1 = tg;
        s.b1 = tb;
        s.i1 = 1.6 * gain * choke;
        s.alpha_curve = 0.75;
        s.soft = 0.15;
        s.seed = fx.rng.float();
        fx.emit_add_view(view, &s);
    }

    // --- forward gas jet -----------------------------------------------------
    let mut s = reset_spawn();
    s.x = p.0 + d.0 * 0.04;
    s.y = p.1 + d.1 * 0.04;
    s.z = p.2 + d.2 * 0.04;
    s.vx = d.0 * 7.0;
    s.vy = d.1 * 7.0;
    s.vz = d.2 * 7.0;
    s.tile = crate::fx::atlas::p::STREAK as f64;
    s.size0 = 0.045 * sc * prof.jet;
    s.size1 = 0.018 * sc * prof.jet;
    s.stretch = 0.28 * prof.jet;
    s.life = 0.044;
    s.drag = 14.0;
    s.r0 = 1.0;
    s.g0 = 0.8;
    s.b0 = 0.5;
    s.i0 = 9.0 * gain;
    s.r1 = 1.0;
    s.g1 = 0.4;
    s.b1 = 0.1;
    s.i1 = 0.0;
    s.alpha_curve = 0.6;
    s.soft = 0.12;
    s.seed = fx.rng.float();
    fx.emit_add_view(view, &s);

    // --- unburnt powder --------------------------------------------------------
    let n_spark = (f64::from(prof.sparks) * fx.pscale).round() as i32;
    let (kr, kg, kb) = blackbody(2600.0 * temp);
    let (jr, jg, jb) = blackbody(1200.0);
    for _ in 0..n_spark {
        let (mut vx, mut vy, mut vz) = cone(&mut fx.rng, d.0, d.1, d.2, 0.55, 1.4);
        let th = toward_hemi(vx, vy, vz, d.0, d.1, d.2, 0.25);
        vx = th.0;
        vy = th.1;
        vz = th.2;
        let cc = clamp_cone(vx, vy, vz, d.0, d.1, d.2, COS55);
        vx = cc.0;
        vy = cc.1;
        vz = cc.2;
        let sp = fx.rng.range(2.5, 8.5);
        let mut s = reset_spawn();
        s.x = p.0 + d.0 * 0.012;
        s.y = p.1 + d.1 * 0.012;
        s.z = p.2 + d.2 * 0.012;
        s.vx = vx * sp;
        s.vy = vy * sp;
        s.vz = vz * sp;
        s.tile = crate::fx::atlas::p::STREAK as f64;
        s.size0 = fx.rng.range(0.005, 0.011);
        s.size1 = s.size0 * 0.4;
        s.stretch = 0.16 + sp * 0.045;
        s.life = fx.rng.range(0.1, 0.4);
        s.drag = 3.2;
        s.gravity = -11.0;
        s.r0 = kr;
        s.g0 = kg;
        s.b0 = kb;
        s.i0 = fx.rng.range(5.0, 11.0) * ember_gain;
        s.r1 = jr;
        s.g1 = jg;
        s.b1 = jb;
        s.i1 = 0.15;
        s.flags = 1.0;
        s.alpha_curve = 0.5;
        s.soft = 0.05;
        s.seed = fx.rng.float();
        fx.emit_add_view(view, &s);
    }

    // --- crown puff ------------------------------------------------------------
    let n_puff = (3.0 * fx.pscale).round().max(2.0) as i32;
    for i in 0..n_puff {
        let (mut vx, mut vy, mut vz) = cone(&mut fx.rng, d.0, d.1, d.2, 0.7, 0.9);
        let th = toward_hemi(vx, vy, vz, d.0, d.1, d.2, 0.1);
        vx = th.0;
        vy = th.1;
        vz = th.2;
        let sp = fx.rng.range(0.6, 1.9);
        let px = p.0 + d.0 * 0.018 + vx * 0.008;
        let py = p.1 + d.1 * 0.018 + vy * 0.008;
        let pz = p.2 + d.2 * 0.018 + vz * 0.008;
        let vvx = vx * sp;
        let vvy = vy * sp + 0.12;
        let vvz = vz * sp;
        let size0 = (0.018 + 0.016 * fx.rng.float()) * sc;
        let size1 = (0.075 + 0.07 * fx.rng.float()) * sc * (0.7 + prof.smoke * 0.5);
        let tile = if i % 2 == 1 { crate::fx::atlas::p::SMOKE_B } else { crate::fx::atlas::p::WISP };
        let rot = fx.rng.float() * TWO_PI;
        let spin = fx.rng.signed() * 2.2;
        let seed = fx.rng.float();
        let delay = 0.012 + fx.rng.float() * 0.022;

        let mut s = reset_spawn();
        s.x = px;
        s.y = py;
        s.z = pz;
        s.vx = vvx;
        s.vy = vvy;
        s.vz = vvz;
        s.tile = tile as f64;
        s.size0 = size0 * 0.9;
        s.size1 = size1 * 0.75;
        s.size_curve = 0.5;
        s.life = 0.05 + fx.rng.float() * 0.03;
        s.drag = 7.0;
        s.rot = rot;
        s.spin = spin;
        s.r0 = 1.0;
        s.g0 = 0.5;
        s.b0 = 0.17;
        s.i0 = (0.5 + fx.rng.float() * 0.7) * gain;
        s.r1 = 1.0;
        s.g1 = 0.26;
        s.b1 = 0.05;
        s.i1 = 0.0;
        s.alpha_curve = 1.1;
        s.soft = 0.2;
        s.seed = seed;
        fx.emit_add_view(view, &s);

        let mut s = reset_spawn();
        s.x = px;
        s.y = py;
        s.z = pz;
        s.vx = vvx;
        s.vy = vvy;
        s.vz = vvz;
        s.tile = tile as f64;
        s.size0 = size0;
        s.size1 = size1;
        s.size_curve = 0.5;
        s.life = fx.rng.range(0.3, 0.55) * (0.7 + prof.smoke * 0.5);
        s.delay = delay;
        s.drag = 7.0;
        s.gravity = 0.5;
        s.rot = rot;
        s.spin = spin;
        s.r0 = 0.62;
        s.g0 = 0.57;
        s.b0 = 0.52;
        s.r1 = 0.44;
        s.g1 = 0.44;
        s.b1 = 0.44;
        s.alpha = fx.rng.range(0.26, 0.44) * (0.7 + prof.smoke * 0.4) * ember_gain;
        s.alpha_curve = 1.35;
        s.soft = 0.22;
        s.turb = 0.05;
        s.turb_freq = 2.4;
        s.seed = seed;
        fx.emit_lit_view(view, &s);
    }

    // --- barrel smoke ------------------------------------------------------------
    let n_smoke = ((2.0 + prof.smoke * 3.0) * fx.pscale * (0.4 + 0.6 * gain.min(1.0))).round() as i32;
    for i in 0..n_smoke {
        let (vx, vy, vz) = cone(&mut fx.rng, d.0, d.1, d.2, 1.0, 0.7);
        let sp = fx.rng.range(0.4, 2.2) * prof.smoke;
        let mut s = reset_spawn();
        s.x = p.0 + d.0 * fx.rng.range(0.0, 0.09);
        s.y = p.1 + d.1 * fx.rng.range(0.0, 0.09);
        s.z = p.2 + d.2 * fx.rng.range(0.0, 0.09);
        s.vx = vx * sp;
        s.vy = vy * sp + 0.28;
        s.vz = vz * sp;
        s.tile = if i % 2 == 1 { crate::fx::atlas::p::WISP } else { crate::fx::atlas::p::SMOKE_A } as f64;
        s.size0 = fx.rng.range(0.03, 0.055) * prof.smoke;
        s.size1 = fx.rng.range(0.16, 0.32) * prof.smoke;
        s.size_curve = 0.55;
        s.life = fx.rng.range(0.5, 1.15) * (0.6 + prof.smoke * 0.6);
        s.delay = fx.rng.range(0.0, 0.05);
        s.drag = 2.6;
        s.gravity = 0.36;
        s.rot = fx.rng.float() * TWO_PI;
        s.spin = fx.rng.signed() * 1.6;
        s.r0 = 0.5;
        s.g0 = 0.49;
        s.b0 = 0.47;
        s.r1 = 0.42;
        s.g1 = 0.41;
        s.b1 = 0.4;
        s.alpha = fx.rng.range(0.10, 0.20) * (0.6 + prof.smoke * 0.5) * ember_gain;
        s.alpha_curve = 1.8;
        s.soft = 0.25;
        s.turb = 0.06;
        s.turb_freq = 1.6;
        s.seed = fx.rng.float();
        fx.emit_lit_view(view, &s);
    }

    // --- muzzle light --------------------------------------------------------
    let lp = o.light_pos.unwrap_or(p);
    fx.lights.flash(
        lp.0 + d.0 * 0.1,
        lp.1 + d.1 * 0.1,
        lp.2 + d.2 * 0.1,
        cr,
        cg,
        cb,
        prof.light * light_gain * 0.18,
        0.09,
        16.0,
        5.0 * sc,
        2.0,
    );

    // --- the same flash, mirrored into the viewmodel scene --------------------
    if view {
        fx.view_flash(
            p.0 + d.0 * 0.03,
            p.1 + d.1 * 0.03,
            p.2 + d.2 * 0.03,
            cr,
            cg,
            cb,
            light_gain * (prof.light / REF_LIGHT),
        );
    }

    // --- hot gas refraction ----------------------------------------------------
    fx.haze(
        p.0 + d.0 * 0.1,
        p.1 + d.1 * 0.1,
        p.2 + d.2 * 0.1,
        0.1 * sc,
        3.0,
        0.1,
        0.7 * sc,
        crate::fx::atlas::p::SMOKE_B,
    );

    prof
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profile_for_matches_by_substring() {
        assert_eq!(profile_for(Some("M4 Carbine")).light, CARBINE.light);
        assert_eq!(profile_for(Some("Suppressed")).light, SUPPRESSED.light);
        assert_eq!(profile_for(None).light, RIFLE.light);
        assert_eq!(profile_for(Some("unknown thing")).light, RIFLE.light);
    }

    /// `for (const name in MUZZLE_PROFILES) if (k.includes(name)) return ...`
    /// (`muzzle.js:87-88`) matches the *first* key (in `MUZZLE_PROFILES`'s
    /// declaration order) whose name is a substring — so a weapon id that
    /// happens to contain an earlier key's name matches that one, even if a
    /// later key would seem like the "more specific" match. `"suppressed_
    /// pistol"` contains `"pistol"` (index 3) which is scanned before
    /// `"suppressed"` (index 7), so it resolves to the pistol profile.
    #[test]
    fn profile_for_matches_the_first_substring_in_declaration_order() {
        assert_eq!(profile_for(Some("suppressed_pistol")).light, PISTOL.light);
    }

    #[test]
    fn screen_angle_is_zero_without_a_camera() {
        assert_eq!(screen_angle(None, 1.0, 0.0, 0.0), 0.0);
    }

    #[test]
    fn screen_angle_matches_atan2_of_the_view_space_direction() {
        let right = [1.0, 0.0, 0.0];
        let up = [0.0, 1.0, 0.0];
        let a = screen_angle(Some((right, up)), 1.0, 1.0, 0.0);
        assert!((a - std::f64::consts::FRAC_PI_4).abs() < 1e-12);
    }

    #[test]
    fn muzzle_flash_falls_back_to_the_rifle_profile_signature() {
        let mut fx = crate::fx::system::FxSystem::test_instance(1);
        let opts = MuzzleFlashOpts {
            position: (0.0, 1.6, 0.0),
            direction: (0.0, 0.0, -1.0),
            weapon: None,
            intensity: None,
            scale: None,
            light: None,
            view: false,
            light_pos: None,
            camera_basis: None,
        };
        let prof = muzzle_flash(&mut fx, &opts);
        assert_eq!(prof.light, RIFLE.light);
        assert!(fx.add.spawned() > 0);
    }
}
