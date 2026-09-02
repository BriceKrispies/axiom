//! Ported from Claude-of-Duty `src/fx/impacts.js:1-936` — the whole file.
//!
//! Per-surface impact recipes. Every one is built from the same vocabulary —
//! a sub-frame flash, ejecta on the reflected cone, a dust/aerosol puff that
//! expands and slows, something that lingers, and a decal — but the
//! timings, colours, masses and drag are picked per material so the frames
//! read differently: sparks skitter off steel, concrete coughs pale dust and
//! spall, wet dirt throws heavy clods, glass sprays glinting shards, flesh
//! atomises.
//!
//! Dispatch is on [`crate::world::palette::Surface`] — the same 12-entry
//! physics/audio surface enum, per the port recipe's instruction to reuse it
//! rather than define a second one; the source dispatches on a lower-case
//! string key (`IMPACTS[surface] ?? IMPACTS.concrete`, `impacts.js:934-936`)
//! which `Surface` replaces one-for-one, including the "unknown falls back
//! to concrete" default (here: exhaustive `match`, so there is no unknown
//! case to fall back from — `Surface` cannot name a value outside its own
//! twelve variants).

use crate::fx::atlas::{d, p};
use crate::fx::particles::reset_spawn;
use crate::fx::system::FxSystem;
use crate::fx::util::{blackbody, clamp_cone, cone, disc_on, reflect, toward_hemi, COS55};
use crate::physics::surfaces::mask;
use crate::world::palette::Surface;

const TWO_PI: f64 = std::f64::consts::PI * 2.0;

/// [`spark`]'s optional overrides, `impacts.js:44` (the `o` object).
#[derive(Debug, Clone, Copy, Default)]
pub struct SparkOpts {
    pub kelvin: Option<f64>,
    pub size: Option<f64>,
    pub life: Option<f64>,
    pub delay: Option<f64>,
    pub drag: Option<f64>,
    pub gravity: Option<f64>,
    pub intensity: Option<f64>,
    pub bounces: i32,
}

/// One incandescent spark. `spark(fx, x, y, z, dx, dy, dz, speed, o)`,
/// `impacts.js:44-111`.
#[allow(clippy::too_many_arguments)]
fn spark(fx: &mut FxSystem, x: f64, y: f64, z: f64, dx: f64, dy: f64, dz: f64, speed: f64, o: SparkOpts) {
    let kelvin_hot = o.kelvin.unwrap_or(2600.0);
    let (cr, cg, cb) = blackbody(kelvin_hot * fx.rng.range(0.92, 1.08));
    let (c2r, c2g, c2b) = blackbody(1200.0);
    let mut s = reset_spawn();
    s.x = x;
    s.y = y;
    s.z = z;
    s.vx = dx * speed;
    s.vy = dy * speed;
    s.vz = dz * speed;
    s.tile = p::STREAK as f64;
    s.size0 = o.size.unwrap_or_else(|| fx.rng.range(0.007, 0.016));
    s.size1 = s.size0 * 0.4;
    s.stretch = 0.4;
    s.life = o.life.unwrap_or_else(|| fx.rng.range(0.22, 0.55));
    s.delay = o.delay.unwrap_or(0.0);
    s.drag = o.drag.unwrap_or_else(|| fx.rng.range(1.4, 2.6));
    s.gravity = o.gravity.unwrap_or(-14.0);
    s.r0 = cr;
    s.g0 = cg;
    s.b0 = cb;
    s.i0 = o.intensity.unwrap_or(1.0) * fx.rng.range(6.0, 13.0);
    s.r1 = c2r;
    s.g1 = c2g;
    s.b1 = c2b;
    s.i1 = 0.2;
    s.flags = 1.0;
    s.alpha_curve = 0.45;
    s.soft = 0.05;
    s.seed = fx.rng.float();
    fx.emit_add(&s);

    if o.bounces <= 0 {
        return;
    }
    let Some(world) = fx.world.as_deref() else {
        return;
    };
    let (mut rdx, mut rdy, mut rdz) = (dx, dy - 0.35, dz);
    let l = {
        let h = (rdx * rdx + rdy * rdy + rdz * rdz).sqrt();
        if h == 0.0 {
            1.0
        } else {
            h
        }
    };
    rdx /= l;
    rdy /= l;
    rdz /= l;
    let reach = (speed * 0.22).min(3.2);
    // `ph.raycast(RAY_O, RAY_D, reach, ph.MASK?.WORLD ?? 0xffff)`
    // (`impacts.js:83`). `ph.MASK` is assigned unconditionally in the physics
    // constructor (`physics/index.js:190`), so the `?? 0xffff` arm is the
    // no-physics fallback and never runs with a world bound — and we only get
    // here at all because `fx.world` resolved. An earlier draft took the dead
    // arm, so ricochets bounced off actors, ragdolls, clip brushes, triggers
    // and foliage as well as the static world. `MASK.WORLD` is
    // `STATIC | PROP` (`physics/surfaces.js:132`) = 3. This is the same
    // mistake `FxSystem::add_decal` already names and fixes for the decal
    // projection mask.
    let Some(hit) = world.raycast((x, y, z), (rdx, rdy, rdz), reach, mask::WORLD) else {
        return;
    };
    let t = (hit.distance / speed.max(1.0)).max(0.02);
    let (mut vx, mut vy, mut vz) = reflect(rdx, rdy, rdz, hit.normal.0, hit.normal.1, hit.normal.2);
    vx += fx.rng.signed() * 0.25;
    vy = vy.abs() * 0.8 + 0.25;
    vz += fx.rng.signed() * 0.25;
    let bl = {
        let h = (vx * vx + vy * vy + vz * vz).sqrt();
        if h == 0.0 {
            1.0
        } else {
            h
        }
    };
    let next_speed = speed * fx.rng.range(0.22, 0.42);
    let next_life = fx.rng.range(0.18, 0.4);
    spark(
        fx,
        hit.point.0 + hit.normal.0 * 0.01,
        hit.point.1 + hit.normal.1 * 0.01,
        hit.point.2 + hit.normal.2 * 0.01,
        vx / bl,
        vy / bl,
        vz / bl,
        next_speed,
        SparkOpts {
            delay: Some(o.delay.unwrap_or(0.0) + t),
            life: Some(next_life),
            kelvin: Some(1700.0),
            intensity: Some(o.intensity.unwrap_or(1.0) * 0.55),
            size: Some(o.size.unwrap_or(0.01) * 0.8),
            bounces: o.bounces - 1,
            drag: Some(2.4),
            gravity: None,
        },
    );
}

/// [`bullet_hole`]'s parameters, `impacts.js:126` (the `o` object).
struct BulletHole {
    tile: usize,
    min: f64,
    max: f64,
    e: f64,
    life: Option<f64>,
    halo: bool,
    soot: f64,
    halo_scale: f64,
    halo_life: Option<f64>,
    max_angle: Option<f64>,
}

impl Default for BulletHole {
    fn default() -> Self {
        BulletHole {
            tile: d::HOLE_CONCRETE,
            min: 0.05,
            max: 0.075,
            e: 1.0,
            life: None,
            halo: true,
            soot: 0.35,
            halo_scale: 1.0,
            halo_life: None,
            max_angle: None,
        }
    }
}

/// A bullet hole is two decals: the hole itself, and a much larger,
/// structureless dust/scorch wash around it. `bulletHole(fx, p, n, o)`,
/// `impacts.js:126-153`.
fn bullet_hole(fx: &mut FxSystem, point: (f64, f64, f64), normal: (f64, f64, f64), o: &BulletHole) {
    let size = fx.rng.range(o.min, o.max) * (0.9 + o.e * 0.1);
    let roll = fx.rng.float() * TWO_PI;
    let flip = fx.rng.float() < 0.5;
    fx.add_decal(
        point,
        normal,
        crate::fx::system::DecalOpts {
            tile: o.tile,
            size,
            life: o.life.or(Some(90.0)),
            roll: Some(roll),
            flip: Some(flip),
            depth: Some((size * 0.85).max(0.03)),
            max_angle: o.max_angle.or(Some(62.0)),
            fade: None,
            opacity: None,
        },
    );
    if !o.halo {
        return;
    }
    let sooty = fx.rng.float() < o.soot;
    let halo_size = fx.rng.range(0.18, 0.30) * o.halo_scale;
    let opacity = fx.rng.range(0.08, 0.15) * if sooty { 0.8 } else { 1.0 };
    let halo_roll = fx.rng.float() * TWO_PI;
    fx.add_decal(
        point,
        normal,
        crate::fx::system::DecalOpts {
            tile: if sooty { d::SCORCH } else { d::SMUDGE },
            size: halo_size,
            life: o.halo_life.or(Some(40.0)),
            opacity: Some(opacity),
            fade: Some(0.4),
            roll: Some(halo_roll),
            max_angle: Some(74.0),
            flip: None,
            depth: None,
        },
    );
}

/// Concrete / brick / stone: pale dust, spall chips, a wisp that hangs.
/// `impacts.js:156-329`.
fn concrete(fx: &mut FxSystem, point: (f64, f64, f64), n: (f64, f64, f64), inc: (f64, f64, f64), e: f64) {
    let q = fx.pscale;
    let (vx, vy, vz) = reflect(inc.0, inc.1, inc.2, n.0, n.1, n.2);
    let (rx, ry, rz) = ((vx + n.0) * 0.5, (vy + n.1) * 0.5, (vz + n.2) * 0.5);
    let (px, py, pz) = point;

    let mut s = reset_spawn();
    s.x = px + n.0 * 0.01;
    s.y = py + n.1 * 0.01;
    s.z = pz + n.2 * 0.01;
    s.tile = p::FLASH_CORE as f64;
    s.size0 = 0.045 * e;
    s.size1 = 0.19 * e;
    s.size_curve = 0.42;
    s.life = 0.07;
    s.drag = 6.0;
    s.r0 = 1.0;
    s.g0 = 0.72;
    s.b0 = 0.4;
    s.i0 = 10.0 * e;
    s.r1 = 1.0;
    s.g1 = 0.4;
    s.b1 = 0.12;
    s.i1 = 0.0;
    s.alpha_curve = 0.7;
    s.soft = 0.25;
    s.seed = fx.rng.float();
    fx.emit_add(&s);

    let n_dust = (9.0 * q).round() as i32 + 3;
    let sun = fx.sun_world();
    for i in 0..n_dust {
        let band = i % 3;
        let (mut vx2, mut vy2, mut vz2) = cone(&mut fx.rng, rx, ry, rz, if band == 0 { 0.85 } else { 1.25 }, 0.7);
        let th = toward_hemi(vx2, vy2, vz2, n.0, n.1, n.2, 0.05);
        vx2 = th.0;
        vy2 = th.1;
        vz2 = th.2;
        let sp = if band == 0 {
            fx.rng.range(1.8, 3.2)
        } else if band == 1 {
            fx.rng.range(0.9, 1.9)
        } else {
            fx.rng.range(0.4, 1.0)
        };
        let mut s = reset_spawn();
        let (dvx, dvy, dvz) = disc_on(&mut fx.rng, n.0, n.1, n.2, 0.09);
        let off = fx.rng.range(0.05, 0.16);
        s.x = px + dvx + n.0 * off;
        s.y = py + dvy + n.1 * off;
        s.z = pz + dvz + n.2 * off;
        s.vx = vx2 * sp;
        s.vy = vy2 * sp + 0.45;
        s.vz = vz2 * sp;
        s.tile = (if band == 2 { p::SMOKE_A } else { p::DUST }) as f64;
        s.size0 = fx.rng.range(0.045, 0.1) * e * if band == 0 { 0.8 } else { 1.0 };
        s.size1 = fx.rng.range(0.3, 0.62) * e * if band == 2 { 1.35 } else { 1.0 };
        s.size_curve = if band == 0 { 0.3 } else if band == 1 { 0.5 } else { 0.78 };
        s.delay = if band == 0 { 0.0 } else { fx.rng.range(0.02, if band == 1 { 0.09 } else { 0.2 }) };
        s.life = if band == 0 {
            fx.rng.range(0.22, 0.4)
        } else if band == 1 {
            fx.rng.range(0.5, 0.85)
        } else {
            fx.rng.range(1.1, 1.8)
        };
        s.drag = if band == 0 { fx.rng.range(5.0, 7.0) } else { fx.rng.range(2.6, 4.0) };
        s.gravity = -0.7;
        s.rot = fx.rng.float() * TWO_PI;
        s.spin = fx.rng.signed() * 1.5;
        let lit = 0.68 + 0.72 * (vx2 * sun.0 + vy2 * sun.1 + vz2 * sun.2).max(0.0);
        s.r0 = 0.56 * lit;
        s.g0 = 0.462 * lit;
        s.b0 = 0.33 * lit;
        s.i0 = 1.0;
        s.r1 = 0.45 * lit;
        s.g1 = 0.365 * lit;
        s.b1 = 0.26 * lit;
        s.i1 = 1.0;
        s.alpha = fx.rng.range(0.4, 0.72) * if band == 2 { 0.7 } else { 1.0 };
        s.alpha_curve = 1.5;
        s.soft = 0.09;
        s.turb = 0.05;
        s.turb_freq = 2.4;
        s.seed = fx.rng.float();
        fx.emit_lit(&s);
    }

    let n_jet = (5.0 * q).round() as i32 + 2;
    for _ in 0..n_jet {
        let (vx2, vy2, vz2) = cone(&mut fx.rng, rx, ry, rz, 0.34, 1.6);
        let sp = fx.rng.range(3.4, 7.5);
        let mut s = reset_spawn();
        s.x = px + n.0 * 0.02;
        s.y = py + n.1 * 0.02;
        s.z = pz + n.2 * 0.02;
        s.vx = vx2 * sp;
        s.vy = vy2 * sp + 0.2;
        s.vz = vz2 * sp;
        s.tile = p::DUST as f64;
        s.size0 = fx.rng.range(0.025, 0.05) * e;
        s.size1 = fx.rng.range(0.14, 0.26) * e;
        s.size_curve = 0.5;
        s.life = fx.rng.range(0.18, 0.32);
        s.drag = fx.rng.range(6.0, 9.0);
        s.gravity = -1.2;
        s.rot = fx.rng.float() * TWO_PI;
        s.spin = fx.rng.signed() * 2.2;
        s.r0 = 0.58;
        s.g0 = 0.475;
        s.b0 = 0.34;
        s.r1 = 0.47;
        s.g1 = 0.38;
        s.b1 = 0.27;
        s.alpha = fx.rng.range(0.35, 0.6);
        s.alpha_curve = 1.2;
        s.soft = 0.08;
        s.seed = fx.rng.float();
        fx.emit_lit(&s);
    }

    let n_chip = (9.0 * q).round() as i32 + 3;
    for _ in 0..n_chip {
        let (vx2, vy2, vz2) = cone(&mut fx.rng, rx, ry, rz, 0.85, 1.4);
        let sp = fx.rng.range(3.5, 9.5);
        let mut s = reset_spawn();
        s.x = px + n.0 * 0.01;
        s.y = py + n.1 * 0.01;
        s.z = pz + n.2 * 0.01;
        s.vx = vx2 * sp;
        s.vy = vy2 * sp;
        s.vz = vz2 * sp;
        s.tile = p::CHIP as f64;
        s.size0 = fx.rng.range(0.008, 0.026);
        s.size1 = s.size0 * 0.9;
        s.life = fx.rng.range(0.5, 1.0);
        s.drag = 0.35;
        s.gravity = -19.0;
        s.rot = fx.rng.float() * TWO_PI;
        s.spin = fx.rng.signed() * 22.0;
        s.r0 = 0.5;
        s.g0 = 0.48;
        s.b0 = 0.45;
        s.i0 = 1.0;
        s.r1 = 0.42;
        s.g1 = 0.4;
        s.b1 = 0.38;
        s.i1 = 1.0;
        s.alpha_curve = 0.25;
        s.soft = 0.06;
        s.seed = fx.rng.float();
        fx.emit_lit(&s);
    }

    let n_spark = (4.0 * q).round() as i32;
    for i in 0..n_spark {
        let (mut vx2, mut vy2, mut vz2) = cone(&mut fx.rng, rx, ry, rz, 0.85, 1.2);
        let th = toward_hemi(vx2, vy2, vz2, n.0, n.1, n.2, 0.12);
        vx2 = th.0;
        vy2 = th.1;
        vz2 = th.2;
        let cc = clamp_cone(vx2, vy2, vz2, n.0, n.1, n.2, COS55);
        vx2 = cc.0;
        vy2 = cc.1;
        vz2 = cc.2;
        let speed = fx.rng.range(3.0, 8.0);
        spark(
            fx,
            px + n.0 * 0.01,
            py + n.1 * 0.01,
            pz + n.2 * 0.01,
            vx2,
            vy2,
            vz2,
            speed,
            SparkOpts {
                intensity: Some(0.7),
                bounces: if i < 2 { 1 } else { 0 },
                ..Default::default()
            },
        );
    }

    let n_wisp = (2.0 * q).round() as i32 + 1;
    for i in 0..n_wisp {
        let (vx2, vy2, vz2) = cone(&mut fx.rng, n.0, n.1, n.2, 1.0, 0.6);
        let mut s = reset_spawn();
        s.x = px + n.0 * 0.16 + vx2 * 0.07;
        s.y = py + n.1 * 0.16 + vy2 * 0.07;
        s.z = pz + n.2 * 0.16 + vz2 * 0.07;
        s.vx = vx2 * 0.35;
        s.vy = 0.42 + fx.rng.range(0.0, 0.25);
        s.vz = vz2 * 0.35;
        s.tile = (if i % 2 == 1 { p::WISP } else { p::SMOKE_B }) as f64;
        s.size0 = fx.rng.range(0.12, 0.2) * e;
        s.size1 = fx.rng.range(0.7, 1.2) * e;
        s.size_curve = 0.75;
        s.life = fx.rng.range(1.7, 2.9);
        s.drag = 1.5;
        s.gravity = 0.28;
        s.rot = fx.rng.float() * TWO_PI;
        s.spin = fx.rng.signed() * 0.5;
        s.r0 = 0.55;
        s.g0 = 0.49;
        s.b0 = 0.40;
        s.r1 = 0.46;
        s.g1 = 0.41;
        s.b1 = 0.34;
        s.alpha = fx.rng.range(0.28, 0.46);
        s.alpha_curve = 1.35;
        s.soft = 0.14;
        s.turb = 0.14;
        s.turb_freq = 1.1;
        s.seed = fx.rng.float();
        fx.emit_lit(&s);
    }

    let hole_tile = if fx.rng.float() < 0.5 { d::HOLE_CONCRETE } else { d::HOLE_CONCRETE_B };
    bullet_hole(
        fx,
        point,
        n,
        &BulletHole {
            tile: hole_tile,
            min: 0.05,
            max: 0.075,
            e,
            soot: 0.4,
            halo_scale: 1.1,
            ..Default::default()
        },
    );
}

/// Plaster / drywall: white powder, crumbs, no sparks. `impacts.js:332-410`.
fn plaster(fx: &mut FxSystem, point: (f64, f64, f64), n: (f64, f64, f64), inc: (f64, f64, f64), e: f64) {
    let q = fx.pscale;
    let (vx, vy, vz) = reflect(inc.0, inc.1, inc.2, n.0, n.1, n.2);
    let (rx, ry, rz) = ((vx + n.0 * 1.3) * 0.5, (vy + n.1 * 1.3) * 0.5, (vz + n.2 * 1.3) * 0.5);
    let (px, py, pz) = point;
    let sun = fx.sun_world();

    let n_dust = (8.0 * q).round() as i32 + 3;
    for i in 0..n_dust {
        let (mut vx2, mut vy2, mut vz2) = cone(&mut fx.rng, rx, ry, rz, 1.3, 0.6);
        let th = toward_hemi(vx2, vy2, vz2, n.0, n.1, n.2, 0.05);
        vx2 = th.0;
        vy2 = th.1;
        vz2 = th.2;
        let sp = fx.rng.range(0.6, 2.2);
        let mut s = reset_spawn();
        let off = fx.rng.range(0.05, 0.14);
        s.x = px + n.0 * off;
        s.y = py + n.1 * off;
        s.z = pz + n.2 * off;
        s.vx = vx2 * sp;
        s.vy = vy2 * sp + 0.4;
        s.vz = vz2 * sp;
        s.tile = (if i % 2 == 1 { p::DUST } else { p::MIST }) as f64;
        let band = i % 3;
        s.size0 = fx.rng.range(0.05, 0.11) * e * if band == 0 { 0.8 } else { 1.0 };
        s.size1 = fx.rng.range(0.34, 0.62) * e * if band == 2 { 1.3 } else { 1.0 };
        s.size_curve = if band == 0 { 0.3 } else if band == 1 { 0.48 } else { 0.78 };
        s.delay = if band == 0 { 0.0 } else { fx.rng.range(0.02, if band == 1 { 0.1 } else { 0.22 }) };
        s.life = if band == 0 {
            fx.rng.range(0.25, 0.45)
        } else if band == 1 {
            fx.rng.range(0.7, 1.2)
        } else {
            fx.rng.range(1.4, 2.2)
        };
        s.drag = if band == 0 { fx.rng.range(5.0, 7.0) } else { fx.rng.range(2.6, 3.8) };
        s.gravity = -0.55;
        s.rot = fx.rng.float() * TWO_PI;
        s.spin = fx.rng.signed() * 1.2;
        let lit = 0.68 + 0.74 * (vx2 * sun.0 + vy2 * sun.1 + vz2 * sun.2).max(0.0);
        s.r0 = 0.74 * lit;
        s.g0 = 0.63 * lit;
        s.b0 = 0.465 * lit;
        s.r1 = 0.63 * lit;
        s.g1 = 0.53 * lit;
        s.b1 = 0.385 * lit;
        s.alpha = fx.rng.range(0.42, 0.72) * if band == 2 { 0.7 } else { 1.0 };
        s.alpha_curve = 1.6;
        s.soft = 0.09;
        s.turb = 0.06;
        s.turb_freq = 2.0;
        s.seed = fx.rng.float();
        fx.emit_lit(&s);
    }
    let n_chip = (7.0 * q).round() as i32 + 2;
    for _ in 0..n_chip {
        let (vx2, vy2, vz2) = cone(&mut fx.rng, rx, ry, rz, 0.95, 1.3);
        let sp = fx.rng.range(2.0, 6.0);
        let mut s = reset_spawn();
        s.x = px + n.0 * 0.01;
        s.y = py + n.1 * 0.01;
        s.z = pz + n.2 * 0.01;
        s.vx = vx2 * sp;
        s.vy = vy2 * sp;
        s.vz = vz2 * sp;
        s.tile = p::CHIP as f64;
        s.size0 = fx.rng.range(0.007, 0.02);
        s.size1 = s.size0;
        s.life = fx.rng.range(0.5, 0.9);
        s.drag = 0.5;
        s.gravity = -19.0;
        s.rot = fx.rng.float() * TWO_PI;
        s.spin = fx.rng.signed() * 18.0;
        s.r0 = 0.72;
        s.g0 = 0.65;
        s.b0 = 0.52;
        s.r1 = 0.64;
        s.g1 = 0.575;
        s.b1 = 0.46;
        s.alpha_curve = 0.25;
        s.soft = 0.06;
        s.seed = fx.rng.float();
        fx.emit_lit(&s);
    }
    let n_ej = (4.0 * q).round() as i32 + 2;
    for _ in 0..n_ej {
        let (vx2, vy2, vz2) = cone(&mut fx.rng, rx, ry, rz, 0.32, 1.6);
        let sp = fx.rng.range(3.0, 6.5);
        let mut s = reset_spawn();
        s.x = px + n.0 * 0.02;
        s.y = py + n.1 * 0.02;
        s.z = pz + n.2 * 0.02;
        s.vx = vx2 * sp;
        s.vy = vy2 * sp + 0.2;
        s.vz = vz2 * sp;
        s.tile = p::DUST as f64;
        s.size0 = fx.rng.range(0.022, 0.045) * e;
        s.size1 = fx.rng.range(0.12, 0.24) * e;
        s.size_curve = 0.5;
        s.life = fx.rng.range(0.18, 0.3);
        s.drag = fx.rng.range(6.0, 9.0);
        s.gravity = -1.1;
        s.rot = fx.rng.float() * TWO_PI;
        s.spin = fx.rng.signed() * 2.2;
        s.r0 = 0.77;
        s.g0 = 0.66;
        s.b0 = 0.485;
        s.r1 = 0.65;
        s.g1 = 0.55;
        s.b1 = 0.40;
        s.alpha = fx.rng.range(0.35, 0.6);
        s.alpha_curve = 1.2;
        s.soft = 0.08;
        s.seed = fx.rng.float();
        fx.emit_lit(&s);
    }
    bullet_hole(
        fx,
        point,
        n,
        &BulletHole {
            tile: d::HOLE_PLASTER,
            min: 0.045,
            max: 0.07,
            e,
            soot: 0.15,
            ..Default::default()
        },
    );
}

/// Steel: the sparks are the whole story. `impacts.js:413-543`.
fn metal(fx: &mut FxSystem, point: (f64, f64, f64), n: (f64, f64, f64), inc: (f64, f64, f64), e: f64) {
    let q = fx.pscale;
    let (vx, vy, vz) = reflect(inc.0, inc.1, inc.2, n.0, n.1, n.2);
    let grazing = 1.0 - (inc.0 * n.0 + inc.1 * n.1 + inc.2 * n.2).abs();
    let (rx, ry, rz) = (vx * 0.8 + n.0 * 0.2, vy * 0.8 + n.1 * 0.2, vz * 0.8 + n.2 * 0.2);
    let (px, py, pz) = point;

    let mut s = reset_spawn();
    let rlen = 0.5 * e * 0.3;
    s.x = px + n.0 * 0.012 + rx * rlen;
    s.y = py + n.1 * 0.012 + ry * rlen;
    s.z = pz + n.2 * 0.012 + rz * rlen;
    s.tile = p::FLASH_LOBE as f64;
    s.size0 = 0.08 * e;
    s.size1 = 0.3 * e;
    s.size_curve = 0.38;
    s.life = 0.07;
    s.drag = 8.0;
    // `s.rot = screenAngle( fx, false, rx, ry, rz ) + rng.signed() * 0.2`
    // (`impacts.js:437`). `FLASH_LOBE` is rooted at its -X edge, so the tongue
    // only points along the reflected ray if it is rolled to that ray's
    // *screen* angle. An earlier draft passed `None` here, which is the
    // source's no-camera arm and returns 0.0 — so every metal ricochet's
    // brightest sprite pointed screen-right whatever direction the round came
    // from. `fx.camera_basis` is `ctx.camera`, which the source has here.
    s.rot = crate::fx::muzzle::screen_angle(fx.camera_basis, rx, ry, rz) + fx.rng.signed() * 0.2;
    s.r0 = 1.0;
    s.g0 = 0.6;
    s.b0 = 0.26;
    s.i0 = 9.0 * e;
    s.r1 = 1.0;
    s.g1 = 0.5;
    s.b1 = 0.16;
    s.i1 = 0.0;
    s.alpha_curve = 0.6;
    s.soft = 0.2;
    s.seed = fx.rng.float();
    fx.emit_add(&s);

    let mut s = reset_spawn();
    s.x = px + n.0 * 0.012;
    s.y = py + n.1 * 0.012;
    s.z = pz + n.2 * 0.012;
    s.tile = p::FLASH_CORE as f64;
    s.size0 = 0.03 * e;
    s.size1 = 0.11 * e;
    s.size_curve = 0.4;
    s.life = 0.075;
    s.drag = 8.0;
    s.r0 = 1.0;
    s.g0 = 0.97;
    s.b0 = 0.92;
    s.i0 = 24.0 * e;
    s.r1 = 1.0;
    s.g1 = 0.55;
    s.b1 = 0.2;
    s.i1 = 0.0;
    s.alpha_curve = 0.5;
    s.soft = 0.2;
    s.seed = fx.rng.float();
    fx.emit_add(&s);

    let n_spark = (20.0 * q).round() as i32 + 6;
    for i in 0..n_spark {
        let flier = fx.rng.float() < 0.22;
        let (mut vx2, mut vy2, mut vz2) = cone(
            &mut fx.rng,
            rx,
            ry,
            rz,
            if grazing > 0.55 { 0.5 } else { 0.9 },
            if flier { 2.4 } else { 1.1 },
        );
        let th = toward_hemi(vx2, vy2, vz2, n.0, n.1, n.2, 0.1);
        vx2 = th.0;
        vy2 = th.1;
        vz2 = th.2;
        let cc = clamp_cone(vx2, vy2, vz2, n.0, n.1, n.2, COS55);
        vx2 = cc.0;
        vy2 = cc.1;
        vz2 = cc.2;
        let speed = if flier { fx.rng.range(11.0, 17.0) } else { fx.rng.range(4.5, 11.0) };
        let spark_size = fx.rng.range(0.008, 0.017);
        let spark_life = if flier { fx.rng.range(0.4, 0.72) } else { fx.rng.range(0.2, 0.5) };
        spark(
            fx,
            px + n.0 * 0.012,
            py + n.1 * 0.012,
            pz + n.2 * 0.012,
            vx2,
            vy2,
            vz2,
            speed,
            SparkOpts {
                size: Some(spark_size),
                life: Some(spark_life),
                intensity: Some(1.1),
                kelvin: Some(2500.0),
                bounces: if flier { 2 } else if i < 3 { 1 } else { 0 },
                ..Default::default()
            },
        );
    }
    let n_ember = (5.0 * q).round() as i32;
    for _ in 0..n_ember {
        let (mut vx2, mut vy2, mut vz2) = cone(&mut fx.rng, rx, ry, rz, 1.0, 1.0);
        let th = toward_hemi(vx2, vy2, vz2, n.0, n.1, n.2, 0.1);
        vx2 = th.0;
        vy2 = th.1;
        vz2 = th.2;
        let cc = clamp_cone(vx2, vy2, vz2, n.0, n.1, n.2, COS55);
        vx2 = cc.0;
        vy2 = cc.1;
        vz2 = cc.2;
        let sp = fx.rng.range(2.0, 6.0);
        let mut s = reset_spawn();
        s.x = px;
        s.y = py;
        s.z = pz;
        s.vx = vx2 * sp;
        s.vy = vy2 * sp;
        s.vz = vz2 * sp;
        s.tile = p::SPARK as f64;
        s.size0 = fx.rng.range(0.008, 0.016);
        s.size1 = s.size0 * 0.6;
        s.life = fx.rng.range(0.5, 1.0);
        s.drag = 1.6;
        s.gravity = -13.0;
        let (cr, cg, cb) = blackbody(2100.0);
        let (c2r, c2g, c2b) = blackbody(1150.0);
        s.r0 = cr;
        s.g0 = cg;
        s.b0 = cb;
        s.i0 = fx.rng.range(5.0, 10.0);
        s.r1 = c2r;
        s.g1 = c2g;
        s.b1 = c2b;
        s.i1 = 0.1;
        s.alpha_curve = 0.7;
        s.flags = 1.0;
        s.soft = 0.05;
        s.seed = fx.rng.float();
        fx.emit_add(&s);
    }

    let n_sm = (2.0 * q).round() as i32 + 1;
    for _ in 0..n_sm {
        let (vx2, vy2, vz2) = cone(&mut fx.rng, n.0, n.1, n.2, 1.1, 0.7);
        let mut s = reset_spawn();
        s.x = px + n.0 * 0.1;
        s.y = py + n.1 * 0.1;
        s.z = pz + n.2 * 0.1;
        s.vx = vx2 * 0.7;
        s.vy = vy2 * 0.7 + 0.5;
        s.vz = vz2 * 0.7;
        s.tile = p::WISP as f64;
        s.size0 = 0.05;
        s.size1 = fx.rng.range(0.3, 0.5);
        s.size_curve = 0.6;
        s.life = fx.rng.range(0.6, 1.1);
        s.drag = 2.4;
        s.gravity = 0.4;
        s.rot = fx.rng.float() * TWO_PI;
        s.spin = fx.rng.signed() * 1.4;
        s.r0 = 0.2;
        s.g0 = 0.19;
        s.b0 = 0.18;
        s.r1 = 0.26;
        s.g1 = 0.25;
        s.b1 = 0.24;
        s.alpha = fx.rng.range(0.3, 0.5);
        s.alpha_curve = 1.7;
        s.soft = 0.1;
        s.turb = 0.09;
        s.turb_freq = 1.8;
        s.seed = fx.rng.float();
        fx.emit_lit(&s);
    }

    fx.haze(px + n.0 * 0.06, py + n.1 * 0.06, pz + n.2 * 0.06, 0.15, 2.6, 0.13, 0.45, p::SMOKE_A);

    if grazing > 0.6 {
        let scrape_size = fx.rng.range(0.16, 0.28);
        let scrape_roll = fx.rng.float() * TWO_PI;
        fx.add_decal(
            point,
            n,
            crate::fx::system::DecalOpts {
                tile: d::SCRAPE,
                size: scrape_size,
                life: Some(70.0),
                roll: Some(scrape_roll),
                flip: None,
                depth: None,
                max_angle: None,
                fade: None,
                opacity: None,
            },
        );
    } else {
        bullet_hole(
            fx,
            point,
            n,
            &BulletHole {
                tile: d::HOLE_METAL,
                min: 0.045,
                max: 0.07,
                e,
                soot: 0.85,
                halo_scale: 0.8,
                halo_life: Some(55.0),
                ..Default::default()
            },
        );
    }
}

/// Wood: splinters and a brown, resinous puff. `impacts.js:546-594`.
fn wood(fx: &mut FxSystem, point: (f64, f64, f64), n: (f64, f64, f64), inc: (f64, f64, f64), e: f64) {
    crate::fx::burst::run_all(
        fx,
        &crate::fx::recipes::WOOD,
        crate::fx::burst::Site {
            point,
            normal: n,
            incident: inc,
            energy: e,
        },
    );
    bullet_hole(
        fx,
        point,
        n,
        &BulletHole {
            tile: d::HOLE_WOOD,
            min: 0.05,
            max: 0.075,
            e,
            soot: 0.5,
            halo_scale: 0.85,
            ..Default::default()
        },
    );
}

/// Dirt / sand: a plume, plus heavy ejected clods. `impacts.js:597-659`.
fn ground(fx: &mut FxSystem, point: (f64, f64, f64), n: (f64, f64, f64), e: f64, sand: bool) {
    crate::fx::burst::run_all(
        fx,
        [&crate::fx::recipes::GROUND_DIRT, &crate::fx::recipes::GROUND_SAND][usize::from(sand)],
        crate::fx::burst::Site {
            point,
            normal: n,
            // Ground never reads the incident direction: the plume and the
            // clods both leave along the surface normal, whichever way the
            // bullet came in.
            incident: (0.0, 0.0, 0.0),
            energy: e,
        },
    );
    bullet_hole(
        fx,
        point,
        n,
        &BulletHole {
            tile: if sand { d::IMPACT_SAND } else { d::IMPACT_DIRT },
            min: 0.085,
            max: 0.15,
            e,
            life: Some(60.0),
            max_angle: Some(78.0),
            soot: 0.0,
            halo_scale: if sand { 1.15 } else { 1.0 },
            halo_life: Some(30.0),
            ..Default::default()
        },
    );
}

/// Glass: glinting shards, fine aerosol, a crack web. `impacts.js:662-724`.
fn glass(fx: &mut FxSystem, point: (f64, f64, f64), n: (f64, f64, f64), inc: (f64, f64, f64), e: f64) {
    // Glass ignores impact energy — a pane either breaks or it does not.
    let _ = e;
    crate::fx::burst::run_all(
        fx,
        &crate::fx::recipes::GLASS,
        crate::fx::burst::Site {
            point,
            normal: n,
            incident: inc,
            energy: 1.0,
        },
    );
    let glass_tile = if fx.rng.float() < 0.5 { d::GLASS_CRACK } else { d::HOLE_GLASS };
    let glass_size = fx.rng.range(0.3, 0.55);
    let glass_roll = fx.rng.float() * TWO_PI;
    fx.add_decal(
        point,
        n,
        crate::fx::system::DecalOpts {
            tile: glass_tile,
            size: glass_size,
            life: Some(120.0),
            roll: Some(glass_roll),
            max_angle: Some(40.0),
            flip: None,
            depth: None,
            fade: None,
            opacity: None,
        },
    );
}

/// Water: a column, droplets, an expanding ripple. `impacts.js:727-790`.
fn water(fx: &mut FxSystem, point: (f64, f64, f64), n: (f64, f64, f64), e: f64) {
    crate::fx::burst::run_all(
        fx,
        &crate::fx::recipes::WATER,
        crate::fx::burst::Site {
            point,
            normal: n,
            // Water never reads it: the column, the droplets and the mist all
            // leave along the normal, whichever way the bullet came in.
            incident: (0.0, 0.0, 0.0),
            energy: e,
        },
    );
    let ripple_size = fx.rng.range(0.45, 0.7);
    fx.add_decal(
        point,
        n,
        crate::fx::system::DecalOpts {
            tile: d::RIPPLE,
            size: ripple_size,
            life: Some(2.6),
            fade: Some(0.15),
            opacity: Some(0.8),
            max_angle: Some(80.0),
            roll: None,
            flip: None,
            depth: None,
        },
    );
}

/// Flesh: a dark aerosol cone, heavy droplets, spatter behind.
/// `impacts.js:793-841`.
fn flesh(fx: &mut FxSystem, point: (f64, f64, f64), n: (f64, f64, f64), inc: (f64, f64, f64), e: f64) {
    crate::fx::burst::run_all(
        fx,
        &crate::fx::recipes::FLESH,
        crate::fx::burst::Site {
            point,
            normal: n,
            incident: inc,
            energy: e,
        },
    );
    fx.blood_spatter_behind(point, inc);
}

/// Foliage: shredded leaf matter, no hole. `impacts.js:844-864`.
///
/// **The first burst in this file that is data rather than code.** The recipe
/// is [`crate::fx::recipes::FOLIAGE`] and the interpreter is
/// [`crate::fx::burst`]; this function is now the call that binds the impact
/// site to it. The two forms were proved byte-identical — particles and shared
/// random-stream state alike — before the swap, and the test that proves it is
/// still in this file's `tests` module.
fn foliage(fx: &mut FxSystem, point: (f64, f64, f64), n: (f64, f64, f64), inc: (f64, f64, f64)) {
    crate::fx::burst::run_all(
        fx,
        &crate::fx::recipes::FOLIAGE,
        crate::fx::burst::Site {
            point,
            normal: n,
            incident: inc,
            energy: 1.0,
        },
    );
}

/// Fabric / rubber: dust, fibres, a tear. `impacts.js:867-916`.
fn soft(fx: &mut FxSystem, point: (f64, f64, f64), n: (f64, f64, f64), inc: (f64, f64, f64), rubber: bool) {
    crate::fx::burst::run_all(
        fx,
        [&crate::fx::recipes::FABRIC, &crate::fx::recipes::RUBBER][usize::from(rubber)],
        crate::fx::burst::Site {
            point,
            normal: n,
            incident: inc,
            energy: 1.0,
        },
    );
    let tear_size = fx.rng.range(0.09, 0.15);
    let tear_roll = fx.rng.float() * TWO_PI;
    fx.add_decal(
        point,
        n,
        crate::fx::system::DecalOpts {
            tile: d::TEAR,
            size: tear_size,
            life: Some(80.0),
            roll: Some(tear_roll),
            flip: None,
            depth: None,
            max_angle: None,
            fade: None,
            opacity: None,
        },
    );
}

/// Dispatch on [`Surface`]; the source's "unknown surface falls back to
/// concrete" (`impacts.js:934-936`) has no counterpart here — see the
/// module doc. `spawnImpact(fx, point, normal, incident, surface, energy)`.
pub fn spawn_impact(
    fx: &mut FxSystem,
    point: (f64, f64, f64),
    normal: (f64, f64, f64),
    incident: (f64, f64, f64),
    surface: Surface,
    energy: f64,
) {
    match surface {
        Surface::Concrete => concrete(fx, point, normal, incident, energy),
        Surface::Plaster => plaster(fx, point, normal, incident, energy),
        Surface::Metal => metal(fx, point, normal, incident, energy),
        Surface::Wood => wood(fx, point, normal, incident, energy),
        Surface::Dirt => ground(fx, point, normal, energy, false),
        Surface::Sand => ground(fx, point, normal, energy, true),
        Surface::Glass => glass(fx, point, normal, incident, energy),
        Surface::Water => water(fx, point, normal, energy),
        Surface::Flesh => flesh(fx, point, normal, incident, energy),
        Surface::Foliage => foliage(fx, point, normal, incident),
        Surface::Fabric => soft(fx, point, normal, incident, false),
        Surface::Rubber => soft(fx, point, normal, incident, true),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fx::system::FxSystem;

    /// The data form of a burst emits exactly what the hand-written form did.
    ///
    /// This is the proof that [`crate::fx::burst`] is a real substitution and
    /// not an approximation, and it is checked at the only level that can tell:
    /// the raw interleaved particle buffers, byte for byte, **and** the state of
    /// the shared random stream afterwards. The second half is the load-bearing
    /// one — a burst that produces identical particles while spending a
    /// different number of draws shifts every later effect in the frame, and no
    /// amount of looking at the particles reveals it.
    ///
    /// Kept as a live test rather than deleted after the conversion, because it
    /// is the only place the two forms are compared directly; once `foliage`'s
    /// body is the interpreter call, the frozen ledger can only say that
    /// *something* changed.
    #[test]
    fn the_foliage_recipe_and_the_hand_written_burst_agree_exactly() {
        let point = (1.5, 2.25, -3.75);
        let n = (0.0, 1.0, 0.0);
        let inc = (0.30, -0.90, 0.30);

        let mut hand = FxSystem::test_instance(0x5eed);
        foliage_hand_written(&mut hand, point, n, inc);

        let mut data = FxSystem::test_instance(0x5eed);
        crate::fx::burst::run_all(
            &mut data,
            &crate::fx::recipes::FOLIAGE,
            crate::fx::burst::Site {
                point,
                normal: n,
                incident: inc,
                energy: 1.0,
            },
        );

        assert_eq!(hand.lit.spawned(), data.lit.spawned(), "particle count");
        assert!(hand.lit.spawned() > 0, "the case emitted nothing");
        assert_eq!(hand.lit.raw(), data.lit.raw(), "particle buffer");
        assert_eq!(hand.add.raw(), data.add.raw(), "wrong pool");
        assert_eq!(
            hand.rng.float(),
            data.rng.float(),
            "the two forms spent a different number of draws"
        );
    }

    /// `foliage` as it was transcribed, kept only so the test above has
    /// something to compare against. Deleted with the last hand-written burst.
    fn foliage_hand_written(
        fx: &mut FxSystem,
        point: (f64, f64, f64),
        n: (f64, f64, f64),
        inc: (f64, f64, f64),
    ) {
        let q = fx.pscale;
        let (vx, vy, vz) = reflect(inc.0, inc.1, inc.2, n.0, n.1, n.2);
        let (px, py, pz) = point;
        let count = (10.0 * q).round() as i32 + 4;
        for _ in 0..count {
            let (vx2, vy2, vz2) = cone(&mut fx.rng, vx, vy, vz, 1.3, 1.0);
            let sp = fx.rng.range(1.5, 5.0);
            let mut s = reset_spawn();
            s.x = px;
            s.y = py;
            s.z = pz;
            s.vx = vx2 * sp;
            s.vy = vy2 * sp;
            s.vz = vz2 * sp;
            s.tile = (if fx.rng.float() < 0.5 { p::CHIP } else { p::SPLINTER }) as f64;
            s.size0 = fx.rng.range(0.012, 0.035);
            s.size1 = s.size0;
            s.life = fx.rng.range(0.8, 1.6);
            s.drag = 2.2;
            s.gravity = -8.0;
            s.rot = fx.rng.float() * TWO_PI;
            s.spin = fx.rng.signed() * 16.0;
            s.r0 = 0.14;
            s.g0 = 0.22;
            s.b0 = 0.08;
            s.r1 = 0.11;
            s.g1 = 0.17;
            s.b1 = 0.06;
            s.alpha_curve = 0.4;
            s.soft = 0.06;
            s.seed = fx.rng.float();
            fx.emit_lit(&s);
        }
    }

    #[test]
    fn every_surface_spawns_something() {
        for surface in Surface::ALL {
            let mut fx = FxSystem::test_instance(surface.index() as u32 + 1);
            let before_add = fx.add.spawned();
            let before_lit = fx.lit.spawned();
            spawn_impact(
                &mut fx,
                (0.0, 1.0, 0.0),
                (0.0, 1.0, 0.0),
                (0.0, -1.0, 0.0),
                surface,
                1.0,
            );
            assert!(
                fx.add.spawned() > before_add || fx.lit.spawned() > before_lit,
                "{surface:?} spawned nothing"
            );
        }
    }

    #[test]
    fn every_surface_but_foliage_writes_a_decal() {
        // Flesh is excluded here: its decal comes from `bloodSpatterBehind`
        // (`impacts.js:840`), which needs a physics raycast hit (`fx.physics
        // ?.raycast`, `index.js:568-580`) — see `flesh_spatters_behind_when_
        // physics_is_bound` below for that path with a world double.
        for surface in Surface::ALL {
            if surface == Surface::Foliage || surface == Surface::Flesh {
                continue;
            }
            let mut fx = FxSystem::test_instance(surface.index() as u32 + 100);
            let before = fx.decals.count;
            spawn_impact(&mut fx, (0.0, 1.0, 0.0), (0.0, 1.0, 0.0), (0.0, -1.0, 0.0), surface, 1.0);
            assert!(fx.decals.count > before, "{surface:?} wrote no decal");
        }
    }

    /// A world that hits a plane 1 metre along whatever ray it is asked to
    /// cast, so `bloodSpatterBehind`'s raycast always succeeds.
    struct WallBehind;
    impl crate::fx::decals::DecalWorld for WallBehind {
        fn tri_count(&self) -> usize {
            0
        }
        fn query_aabb(&self, _min: [f64; 3], _max: [f64; 3], _mask: u16) -> Vec<u32> {
            vec![]
        }
        fn triangle(&self, _tri: u32) -> ([[f64; 3]; 3], [f64; 3]) {
            ([[0.0; 3]; 3], [0.0, 1.0, 0.0])
        }
    }
    impl crate::fx::world::FxWorld for WallBehind {
        fn raycast(
            &self,
            origin: (f64, f64, f64),
            dir: (f64, f64, f64),
            _max_dist: f64,
            _mask: u16,
        ) -> Option<crate::fx::world::FxHit> {
            Some(crate::fx::world::FxHit {
                point: (origin.0 + dir.0, origin.1 + dir.1, origin.2 + dir.2),
                normal: (-dir.0, -dir.1, -dir.2),
                distance: 1.0,
                surface: Surface::Concrete,
            })
        }
        fn ground_height(&self, _x: f64, _z: f64, _from_y: f64) -> Option<f64> {
            None
        }
    }

    #[test]
    fn flesh_spatters_behind_when_physics_is_bound() {
        let mut fx = FxSystem::test_instance(1);
        fx.world = Some(Box::new(WallBehind));
        let before = fx.decals.count;
        spawn_impact(
            &mut fx,
            (0.0, 1.0, 0.0),
            (0.0, 1.0, 0.0),
            (0.0, -1.0, 0.0),
            Surface::Flesh,
            1.0,
        );
        assert!(fx.decals.count > before);
    }
}
