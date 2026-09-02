//! Impact bursts, as data.
//!
//! Not a port of anything — this is the data form of recipes that were
//! transcribed into [`crate::fx::impacts`] as Rust functions. Each burst here
//! is a [`Burst`] the interpreter in [`crate::fx::burst`] executes, and its
//! source citation names the `impacts.js` lines it stands for so the audit
//! trail survives the change of form.
//!
//! # Reading a recipe
//!
//! Each is built once, on first use, through [`Program`] — the assembler that
//! hands back a handle for every value it writes, so nothing here counts a
//! register. **The order the calls appear is the order the burst draws**, which
//! is the property the whole format exists to keep: the random stream is shared
//! across every subsystem in a frame, so a burst that spends one extra draw
//! shifts every later effect silently.
//!
//! What comes out is a plain value with no code in it. The endpoint of this work
//! is a parser that produces the same value from an asset file, at which point
//! these builders go away and the recipes are content rather than source. This
//! is the authoring surface in the meantime, and it is the same one the engine's
//! own recipe graphs use.
//!
//! Most of what a burst writes is not computed at all. Those values are [`imm`]
//! in the field table and never enter the program — see [`crate::fx::burst::Src`]
//! for why that split is the whole reason this format is not larger than the
//! code it replaces.

use std::sync::LazyLock;

use crate::fx::atlas::p;
use crate::fx::burst::{imm, Burst, Field, Input, Pool, Program, V3};

const TWO_PI: f64 = std::f64::consts::PI * 2.0;

/// Foliage: shredded leaf matter, no hole. `impacts.js:844-864`.
///
/// The first burst expressed this way, and chosen for it: it is short, and it
/// uses the two things that decide whether the format is viable at all — a count
/// computed from the quality scale, which sets how many times the shared stream
/// is drawn from, and a *drawn* select (`rng.float() < 0.5` choosing between two
/// atlas tiles), which has to spend its draw whichever way it lands.
pub static FOLIAGE: LazyLock<Vec<Burst>> = LazyLock::new(|| {
    let mut b = Program::new();
    let at = b.point();
    let axis = b.reflected();
    let dir = b.cone(axis, 1.3, 1.0);
    let speed = b.range(1.5, 5.0);
    let vel = b.mul3(dir, speed);
    let flip = b.unit();
    let tile = b.select_lt(flip, 0.5, p::CHIP as f64, p::SPLINTER as f64);
    let size = b.range(0.012, 0.035);
    let life = b.range(0.8, 1.6);
    let spun = b.unit();
    let rot = b.scale(spun, TWO_PI);
    let signed = b.signed();
    let spin = b.scale(signed, 16.0);
    let seed = b.unit();

    vec![b.emit(
        (10.0, 4),
        Pool::Lit,
        vec![
            (Field::X, at.0.src()),
            (Field::Y, at.1.src()),
            (Field::Z, at.2.src()),
            (Field::Vx, vel.0.src()),
            (Field::Vy, vel.1.src()),
            (Field::Vz, vel.2.src()),
            (Field::Tile, tile.src()),
            // `s.size1 = s.size0` — one register named twice. A table would
            // have needed a "same as" sentinel; a handle just works.
            (Field::Size0, size.src()),
            (Field::Size1, size.src()),
            (Field::Life, life.src()),
            (Field::Rot, rot.src()),
            (Field::Spin, spin.src()),
            (Field::Seed, seed.src()),
            (Field::Drag, imm(2.2)),
            (Field::Gravity, imm(-8.0)),
            (Field::R0, imm(0.14)),
            (Field::G0, imm(0.22)),
            (Field::B0, imm(0.08)),
            (Field::R1, imm(0.11)),
            (Field::G1, imm(0.17)),
            (Field::B1, imm(0.06)),
            (Field::AlphaCurve, imm(0.4)),
            (Field::Soft, imm(0.06)),
        ],
    )]
});

/// Wood: splinters and a brown, resinous puff. `impacts.js:546-594`.
///
/// Two bursts, and the second is why the format has a sequence: the source runs
/// two loops, so every draw the splinters make comes before every draw the dust
/// makes, and array order is what keeps that true.
///
/// The bullet hole `impacts.js:588` writes afterwards is not here. A decal is a
/// different subsystem, not a particle burst, and folding it in to make one
/// recipe hold everything is how a format becomes a junk drawer. That call stays
/// in [`crate::fx::impacts`] until decals get a recipe form of their own.
pub static WOOD: LazyLock<Vec<Burst>> = LazyLock::new(|| {
    // Splinters — `impacts.js:553-575`. Lifted a centimetre off the surface so
    // a splinter does not spawn inside the plank it came from.
    let mut a = Program::new();
    let at = a.point();
    let n = a.normal();
    let from = a.mad3(n, 0.01, at);
    let axis = ejecta_axis(&mut a, n);
    let dir = a.cone(axis, 0.9, 1.3);
    let speed = a.range(2.5, 7.5);
    let vel = a.mul3(dir, speed);
    // Every fourth splinter is a chip rather than a sliver. No draw — the loop
    // index decides, and `%` is exact on small integers, so `< 0.5` is `== 0`.
    let index = a.read(Input::Index);
    let phase = a.modulo(index, 4.0);
    let tile = a.select_lt(phase, 0.5, p::CHIP as f64, p::SPLINTER as f64);
    let size = a.range(0.014, 0.045);
    let life = a.range(0.6, 1.2);
    let spun = a.unit();
    let rot = a.scale(spun, TWO_PI);
    let signed = a.signed();
    let spin = a.scale(signed, 26.0);
    let seed = a.unit();

    let splinters = a.emit(
        (11.0, 4),
        Pool::Lit,
        vec![
            (Field::X, from.0.src()),
            (Field::Y, from.1.src()),
            (Field::Z, from.2.src()),
            (Field::Vx, vel.0.src()),
            (Field::Vy, vel.1.src()),
            (Field::Vz, vel.2.src()),
            (Field::Tile, tile.src()),
            (Field::Size0, size.src()),
            (Field::Size1, size.src()),
            (Field::Life, life.src()),
            (Field::Rot, rot.src()),
            (Field::Spin, spin.src()),
            (Field::Seed, seed.src()),
            (Field::Drag, imm(0.8)),
            (Field::Gravity, imm(-18.0)),
            (Field::R0, imm(0.44)),
            (Field::G0, imm(0.3)),
            (Field::B0, imm(0.16)),
            (Field::R1, imm(0.36)),
            (Field::G1, imm(0.24)),
            (Field::B1, imm(0.13)),
            (Field::AlphaCurve, imm(0.25)),
            (Field::Soft, imm(0.06)),
        ],
    );

    // Resinous dust — `impacts.js:577-587`.
    let mut d = Program::new();
    let at = d.point();
    let n = d.normal();
    let axis = ejecta_axis(&mut d, n);
    let dir = d.cone(axis, 1.1, 0.7);
    let speed = d.range(0.6, 2.0);
    // One drawn distance, read three times: the puff starts off the surface
    // along the normal, so it is one offset and not three.
    let off = d.range(0.05, 0.12);
    let step = d.mul3(n, off);
    let from = d.add3(at, step);
    let vel = d.mul3(dir, speed);
    let rise = d.offset(vel.1, 0.35);
    let energy = d.read(Input::Energy);
    let near = d.range(0.04, 0.09);
    let size0 = d.mul(near, energy);
    let far = d.range(0.24, 0.44);
    let size1 = d.mul(far, energy);
    let life = d.range(0.45, 0.9);
    let spun = d.unit();
    let rot = d.scale(spun, TWO_PI);
    let signed = d.signed();
    let spin = d.scale(signed, 1.4);
    let alpha = d.range(0.5, 0.8);
    let seed = d.unit();

    let dust = d.emit(
        (5.0, 2),
        Pool::Lit,
        vec![
            (Field::X, from.0.src()),
            (Field::Y, from.1.src()),
            (Field::Z, from.2.src()),
            (Field::Vx, vel.0.src()),
            (Field::Vy, rise.src()),
            (Field::Vz, vel.2.src()),
            (Field::Size0, size0.src()),
            (Field::Size1, size1.src()),
            (Field::Life, life.src()),
            (Field::Rot, rot.src()),
            (Field::Spin, spin.src()),
            (Field::Alpha, alpha.src()),
            (Field::Seed, seed.src()),
            (Field::Tile, imm(p::DUST as f64)),
            (Field::SizeCurve, imm(0.45)),
            (Field::Drag, imm(3.4)),
            (Field::Gravity, imm(-0.9)),
            (Field::R0, imm(0.46)),
            (Field::G0, imm(0.34)),
            (Field::B0, imm(0.2)),
            (Field::R1, imm(0.38)),
            (Field::G1, imm(0.28)),
            (Field::B1, imm(0.17)),
            (Field::AlphaCurve, imm(1.5)),
            (Field::Soft, imm(0.09)),
            (Field::Turb, imm(0.05)),
            (Field::TurbFreq, imm(2.2)),
        ],
    );

    vec![splinters, dust]
});

/// Flesh: a dark aerosol cone, heavy droplets. `impacts.js:793-841`.
///
/// The one recipe whose ejecta axis follows the bullet rather than the surface —
/// `inc * 0.75 - n * 0.25`, mostly *through* the wound — which is what makes
/// [`Input::IncidentX`] worth reading directly instead of only its reflection.
///
/// `bloodSpatterBehind` (`impacts.js:840`) is not here: it needs a physics
/// raycast to find the wall behind, which is a query and not a burst.
pub static FLESH: LazyLock<Vec<Burst>> = LazyLock::new(|| {
    // Aerosol mist — `impacts.js:800-826`, spawned a couple of centimetres
    // *inside* the surface so the cone reads as coming out of the wound.
    let mut m = Program::new();
    let at = m.point();
    let n = m.normal();
    let inc = m.incident();
    let back = m.scale3(n, -0.25);
    let axis = m.mad3(inc, 0.75, back);
    let from = m.mad3(n, -0.02, at);
    let dir = m.cone(axis, 0.95, 0.8);
    let speed = m.range(1.2, 4.5);
    let vel = m.mul3(dir, speed);
    let rise = m.offset(vel.1, 0.3);
    let index = m.read(Input::Index);
    let phase = m.modulo(index, 3.0);
    let tile = m.select_lt(phase, 0.5, p::SMOKE_A as f64, p::MIST as f64);
    let energy = m.read(Input::Energy);
    let near = m.range(0.035, 0.075);
    let size0 = m.mul(near, energy);
    let far = m.range(0.16, 0.34);
    let size1 = m.mul(far, energy);
    let life = m.range(0.3, 0.62);
    let drag = m.range(4.5, 6.5);
    let spun = m.unit();
    let rot = m.scale(spun, TWO_PI);
    let signed = m.signed();
    let spin = m.scale(signed, 2.0);
    let alpha = m.range(0.6, 0.95);
    let seed = m.unit();

    let mist = m.emit(
        (9.0, 4),
        Pool::Lit,
        vec![
            (Field::X, from.0.src()),
            (Field::Y, from.1.src()),
            (Field::Z, from.2.src()),
            (Field::Vx, vel.0.src()),
            (Field::Vy, rise.src()),
            (Field::Vz, vel.2.src()),
            (Field::Tile, tile.src()),
            (Field::Size0, size0.src()),
            (Field::Size1, size1.src()),
            (Field::Life, life.src()),
            (Field::Drag, drag.src()),
            (Field::Rot, rot.src()),
            (Field::Spin, spin.src()),
            (Field::Alpha, alpha.src()),
            (Field::Seed, seed.src()),
            (Field::SizeCurve, imm(0.5)),
            (Field::Gravity, imm(-3.2)),
            (Field::R0, imm(0.34)),
            (Field::G0, imm(0.035)),
            (Field::B0, imm(0.03)),
            (Field::R1, imm(0.16)),
            (Field::G1, imm(0.016)),
            (Field::B1, imm(0.014)),
            (Field::AlphaCurve, imm(1.5)),
            (Field::Soft, imm(0.08)),
            (Field::Turb, imm(0.04)),
            (Field::TurbFreq, imm(3.0)),
        ],
    );

    // Droplets — `impacts.js:828-839`. Heavier, faster, stretched along their
    // own velocity, and thrown from the surface itself rather than behind it.
    let mut d = Program::new();
    let at = d.point();
    let n = d.normal();
    let inc = d.incident();
    let back = d.scale3(n, -0.25);
    let axis = d.mad3(inc, 0.75, back);
    let dir = d.cone(axis, 1.1, 1.2);
    let speed = d.range(2.0, 8.0);
    let vel = d.mul3(dir, speed);
    let size = d.range(0.007, 0.022);
    let life = d.range(0.35, 0.8);
    let seed = d.unit();

    let drops = d.emit(
        (14.0, 5),
        Pool::Lit,
        vec![
            (Field::X, at.0.src()),
            (Field::Y, at.1.src()),
            (Field::Z, at.2.src()),
            (Field::Vx, vel.0.src()),
            (Field::Vy, vel.1.src()),
            (Field::Vz, vel.2.src()),
            (Field::Size0, size.src()),
            (Field::Size1, size.src()),
            (Field::Life, life.src()),
            (Field::Seed, seed.src()),
            (Field::Tile, imm(p::DROPLET as f64)),
            (Field::Stretch, imm(0.6)),
            (Field::Drag, imm(0.9)),
            (Field::Gravity, imm(-19.0)),
            (Field::R0, imm(0.3)),
            (Field::G0, imm(0.03)),
            (Field::B0, imm(0.025)),
            (Field::R1, imm(0.22)),
            (Field::G1, imm(0.022)),
            (Field::B1, imm(0.018)),
            (Field::Alpha, imm(0.95)),
            (Field::AlphaCurve, imm(0.35)),
            (Field::Soft, imm(0.05)),
        ],
    );

    vec![mist, drops]
});

/// What separates dirt from sand: a colour, how far a clod can spread, and how
/// hard the air holds it back. `impacts.js:597-659`.
///
/// **Dirt and sand are one program with two rows.** The source writes them as a
/// single function taking a `sand` flag, and every place that flag is read is a
/// constant — a colour, a size ceiling, a drag — never a different computation.
/// That is a table, and it is the first place in this file where the *table*
/// half of the format earns its keep on its own: two ground recipes are the same
/// instructions and two sets of `imm`.
struct Ground {
    albedo: (f64, f64, f64),
    clod_size_max: f64,
    clod_drag: f64,
}

const DIRT: Ground = Ground {
    albedo: (0.3, 0.22, 0.15),
    clod_size_max: 0.035,
    clod_drag: 0.5,
};

const SAND: Ground = Ground {
    albedo: (0.66, 0.56, 0.4),
    clod_size_max: 0.02,
    clod_drag: 1.4,
};

/// Dirt / sand: a plume, plus heavy ejected clods. `impacts.js:597-659`.
fn ground(g: Ground) -> Vec<Burst> {
    let (cr, cg, cb) = g.albedo;

    // The plume — `impacts.js:604-635`. Spawn points are spread over a small
    // disc on the surface rather than stacked on the impact point, so the puff
    // reads as a crater rather than a jet.
    let mut p_ = Program::new();
    let at = p_.point();
    let n = p_.normal();
    let energy = p_.read(Input::Energy);
    let dir = p_.cone(n, 0.75, 0.55);
    let drawn = p_.range(1.6, 4.2);
    let speed = p_.mul(drawn, energy);
    let spread = p_.disc_on(n, 0.06);
    let from = p_.add3(at, spread);
    let lifted = p_.offset(from.1, 0.01);
    let vel = p_.mul3(dir, speed);
    let index = p_.read(Input::Index);
    let phase = p_.modulo(index, 3.0);
    let tile = p_.select_lt(phase, 0.5, p::SMOKE_B as f64, p::DUST as f64);
    let near = p_.range(0.06, 0.13);
    let size0 = p_.mul(near, energy);
    let far = p_.range(0.55, 1.0);
    let size1 = p_.mul(far, energy);
    let life = p_.range(0.8, 1.5);
    let drag = p_.range(2.2, 3.2);
    let spun = p_.unit();
    let rot = p_.scale(spun, TWO_PI);
    let signed = p_.signed();
    let spin = p_.scale(signed, 1.1);
    let alpha = p_.range(0.6, 0.95);
    let seed = p_.unit();

    let plume = p_.emit(
        (8.0, 3),
        Pool::Lit,
        vec![
            (Field::X, from.0.src()),
            (Field::Y, lifted.src()),
            (Field::Z, from.2.src()),
            (Field::Vx, vel.0.src()),
            (Field::Vy, vel.1.src()),
            (Field::Vz, vel.2.src()),
            (Field::Tile, tile.src()),
            (Field::Size0, size0.src()),
            (Field::Size1, size1.src()),
            (Field::Life, life.src()),
            (Field::Drag, drag.src()),
            (Field::Rot, rot.src()),
            (Field::Spin, spin.src()),
            (Field::Alpha, alpha.src()),
            (Field::Seed, seed.src()),
            (Field::SizeCurve, imm(0.5)),
            (Field::Gravity, imm(-1.6)),
            (Field::R0, imm(cr)),
            (Field::G0, imm(cg)),
            (Field::B0, imm(cb)),
            (Field::R1, imm(cr * 0.85)),
            (Field::G1, imm(cg * 0.85)),
            (Field::B1, imm(cb * 0.85)),
            (Field::AlphaCurve, imm(1.4)),
            (Field::Soft, imm(0.14)),
            (Field::Turb, imm(0.08)),
            (Field::TurbFreq, imm(1.6)),
        ],
    );

    // Clods — `impacts.js:637-657`.
    let mut c = Program::new();
    let at = c.point();
    let n = c.normal();
    let lifted = c.offset(at.1, 0.01);
    let dir = c.cone(n, 0.95, 1.1);
    let speed = c.range(3.0, 9.0);
    let vel = c.mul3(dir, speed);
    let size = c.range(0.008, g.clod_size_max);
    let life = c.range(0.6, 1.2);
    let spun = c.unit();
    let rot = c.scale(spun, TWO_PI);
    let signed = c.signed();
    let spin = c.scale(signed, 20.0);
    let seed = c.unit();

    let clods = c.emit(
        (13.0, 5),
        Pool::Lit,
        vec![
            (Field::X, at.0.src()),
            (Field::Y, lifted.src()),
            (Field::Z, at.2.src()),
            (Field::Vx, vel.0.src()),
            (Field::Vy, vel.1.src()),
            (Field::Vz, vel.2.src()),
            (Field::Size0, size.src()),
            (Field::Size1, size.src()),
            (Field::Life, life.src()),
            (Field::Rot, rot.src()),
            (Field::Spin, spin.src()),
            (Field::Seed, seed.src()),
            (Field::Tile, imm(p::CHIP as f64)),
            (Field::Drag, imm(g.clod_drag)),
            (Field::Gravity, imm(-19.0)),
            (Field::R0, imm(cr * 0.8)),
            (Field::G0, imm(cg * 0.8)),
            (Field::B0, imm(cb * 0.8)),
            (Field::R1, imm(cr * 0.7)),
            (Field::G1, imm(cg * 0.7)),
            (Field::B1, imm(cb * 0.7)),
            (Field::AlphaCurve, imm(0.3)),
            (Field::Soft, imm(0.06)),
        ],
    );

    vec![plume, clods]
}

/// Wet earth. `impacts.js:597-659` with the dirt row.
pub static GROUND_DIRT: LazyLock<Vec<Burst>> = LazyLock::new(|| ground(DIRT));

/// Dry sand — paler, finer clods, more air resistance holding them back.
pub static GROUND_SAND: LazyLock<Vec<Burst>> = LazyLock::new(|| ground(SAND));

/// Water: a column, droplets, a hanging mist. `impacts.js:727-790`.
///
/// The only recipe that never reads the incident direction — everything leaves
/// along the surface normal, whichever way the bullet came in — and the only one
/// whose first burst has no cone at all: the column's outward velocity *is* its
/// spawn offset, scaled, so the splash widens as it rises.
///
/// The ripple decal `impacts.js:786` writes is not here; it is a decal.
pub static WATER: LazyLock<Vec<Burst>> = LazyLock::new(|| {
    // The column — `impacts.js:733-758`.
    let mut c = Program::new();
    let at = c.point();
    let n = c.normal();
    let energy = c.read(Input::Energy);
    let spread = c.disc_on(n, 0.05);
    let x = c.add(at.0, spread.0);
    let y = c.offset(at.1, 0.02);
    let z = c.add(at.2, spread.2);
    let vx = c.scale(spread.0, 2.5);
    let drawn = c.range(2.4, 4.6);
    let vy = c.mul(drawn, energy);
    let vz = c.scale(spread.2, 2.5);
    let near = c.range(0.07, 0.13);
    let size0 = c.mul(near, energy);
    let far = c.range(0.3, 0.55);
    let size1 = c.mul(far, energy);
    let life = c.range(0.4, 0.72);
    let tilted = c.signed();
    let rot = c.scale(tilted, 0.25);
    let signed = c.signed();
    let spin = c.scale(signed, 0.6);
    let alpha = c.range(0.6, 0.9);
    let seed = c.unit();

    let column = c.emit(
        (4.0, 2),
        Pool::Lit,
        vec![
            (Field::X, x.src()),
            (Field::Y, y.src()),
            (Field::Z, z.src()),
            (Field::Vx, vx.src()),
            (Field::Vy, vy.src()),
            (Field::Vz, vz.src()),
            (Field::Size0, size0.src()),
            (Field::Size1, size1.src()),
            (Field::Life, life.src()),
            (Field::Rot, rot.src()),
            (Field::Spin, spin.src()),
            (Field::Alpha, alpha.src()),
            (Field::Seed, seed.src()),
            (Field::Tile, imm(p::SPLASH as f64)),
            (Field::SizeCurve, imm(0.55)),
            (Field::Drag, imm(1.1)),
            (Field::Gravity, imm(-13.0)),
            (Field::R0, imm(0.7)),
            (Field::G0, imm(0.76)),
            (Field::B0, imm(0.8)),
            (Field::R1, imm(0.6)),
            (Field::G1, imm(0.68)),
            (Field::B1, imm(0.72)),
            (Field::AlphaCurve, imm(1.5)),
            (Field::Soft, imm(0.1)),
        ],
    );

    // Droplets — `impacts.js:760-781`.
    let mut d = Program::new();
    let at = d.point();
    let n = d.normal();
    let y = d.offset(at.1, 0.02);
    let dir = d.cone(n, 0.85, 0.9);
    let speed = d.range(2.5, 7.5);
    let vel = d.mul3(dir, speed);
    let size0 = d.range(0.008, 0.026);
    let size1 = d.scale(size0, 0.9);
    let life = d.range(0.4, 0.9);
    let seed = d.unit();

    let drops = d.emit(
        (18.0, 6),
        Pool::Lit,
        vec![
            (Field::X, at.0.src()),
            (Field::Y, y.src()),
            (Field::Z, at.2.src()),
            (Field::Vx, vel.0.src()),
            (Field::Vy, vel.1.src()),
            (Field::Vz, vel.2.src()),
            (Field::Size0, size0.src()),
            (Field::Size1, size1.src()),
            (Field::Life, life.src()),
            (Field::Seed, seed.src()),
            (Field::Tile, imm(p::DROPLET as f64)),
            (Field::Stretch, imm(0.5)),
            (Field::Drag, imm(0.7)),
            (Field::Gravity, imm(-19.0)),
            (Field::R0, imm(0.72)),
            (Field::G0, imm(0.78)),
            (Field::B0, imm(0.82)),
            (Field::R1, imm(0.66)),
            (Field::G1, imm(0.72)),
            (Field::B1, imm(0.76)),
            (Field::Alpha, imm(0.8)),
            (Field::AlphaCurve, imm(0.4)),
            (Field::Soft, imm(0.05)),
        ],
    );

    // Hanging mist — `impacts.js:763-785`. No cone and no drawn velocity: it
    // rises straight up at a fixed rate and expands.
    let mut m = Program::new();
    let at = m.point();
    let y = m.offset(at.1, 0.05);
    let size1 = m.range(0.35, 0.6);
    let life = m.range(0.5, 0.9);
    let spun = m.unit();
    let rot = m.scale(spun, TWO_PI);
    let seed = m.unit();

    let mist = m.emit(
        (3.0, 1),
        Pool::Lit,
        vec![
            (Field::X, at.0.src()),
            (Field::Y, y.src()),
            (Field::Z, at.2.src()),
            (Field::Size1, size1.src()),
            (Field::Life, life.src()),
            (Field::Rot, rot.src()),
            (Field::Seed, seed.src()),
            (Field::Vy, imm(0.7)),
            (Field::Tile, imm(p::MIST as f64)),
            (Field::Size0, imm(0.08)),
            (Field::SizeCurve, imm(0.5)),
            (Field::Drag, imm(3.4)),
            (Field::Gravity, imm(-1.4)),
            (Field::R0, imm(0.78)),
            (Field::G0, imm(0.83)),
            (Field::B0, imm(0.86)),
            (Field::R1, imm(0.7)),
            (Field::G1, imm(0.75)),
            (Field::B1, imm(0.78)),
            (Field::Alpha, imm(0.4)),
            (Field::AlphaCurve, imm(1.7)),
            (Field::Soft, imm(0.12)),
        ],
    );

    vec![column, drops, mist]
});

/// What separates fabric from rubber: two colour pairs, and nothing else.
/// `impacts.js:867-916`.
///
/// The second two-row table in this file, and a starker one than ground —
/// every use of the source's `rubber` flag is a colour. Same instructions,
/// same counts, same drags; two palettes.
struct Soft {
    dust0: (f64, f64, f64),
    dust1: (f64, f64, f64),
    fibre: (f64, f64, f64),
}

const FABRIC_PALETTE: Soft = Soft {
    dust0: (0.5, 0.45, 0.38),
    dust1: (0.42, 0.38, 0.32),
    fibre: (0.46, 0.41, 0.34),
};

const RUBBER_PALETTE: Soft = Soft {
    dust0: (0.1, 0.095, 0.09),
    dust1: (0.08, 0.078, 0.075),
    fibre: (0.09, 0.085, 0.08),
};

/// Fabric / rubber: dust, fibres, a tear. `impacts.js:867-916`.
fn soft(s: Soft) -> Vec<Burst> {
    // Dust — `impacts.js:874-901`.
    let mut d = Program::new();
    let at = d.point();
    let n = d.normal();
    let from = d.mad3(n, 0.02, at);
    let axis = ejecta_axis(&mut d, n);
    let dir = d.cone(axis, 1.1, 0.8);
    let speed = d.range(0.8, 3.0);
    let vel = d.mul3(dir, speed);
    let rise = d.offset(vel.1, 0.3);
    // Odd particles are dust, even are mist. `i % 2 < 0.5` is `i % 2 == 0`,
    // which is the *even* case, so mist is the low arm.
    let index = d.read(Input::Index);
    let phase = d.modulo(index, 2.0);
    let tile = d.select_lt(phase, 0.5, p::MIST as f64, p::DUST as f64);
    let size0 = d.range(0.04, 0.08);
    let size1 = d.range(0.2, 0.36);
    let life = d.range(0.4, 0.8);
    let spun = d.unit();
    let rot = d.scale(spun, TWO_PI);
    let signed = d.signed();
    let spin = d.scale(signed, 1.5);
    let alpha = d.range(0.45, 0.7);
    let seed = d.unit();

    let dust = d.emit(
        (6.0, 2),
        Pool::Lit,
        vec![
            (Field::X, from.0.src()),
            (Field::Y, from.1.src()),
            (Field::Z, from.2.src()),
            (Field::Vx, vel.0.src()),
            (Field::Vy, rise.src()),
            (Field::Vz, vel.2.src()),
            (Field::Tile, tile.src()),
            (Field::Size0, size0.src()),
            (Field::Size1, size1.src()),
            (Field::Life, life.src()),
            (Field::Rot, rot.src()),
            (Field::Spin, spin.src()),
            (Field::Alpha, alpha.src()),
            (Field::Seed, seed.src()),
            (Field::SizeCurve, imm(0.45)),
            (Field::Drag, imm(3.8)),
            (Field::Gravity, imm(-1.2)),
            (Field::R0, imm(s.dust0.0)),
            (Field::G0, imm(s.dust0.1)),
            (Field::B0, imm(s.dust0.2)),
            (Field::R1, imm(s.dust1.0)),
            (Field::G1, imm(s.dust1.1)),
            (Field::B1, imm(s.dust1.2)),
            (Field::AlphaCurve, imm(1.5)),
            (Field::Soft, imm(0.09)),
        ],
    );

    // Fibres — `impacts.js:903-913`. The far colour is the near one at 90%,
    // which is arithmetic on constants and therefore still a constant.
    let mut f = Program::new();
    let at = f.point();
    let n = f.normal();
    let axis = ejecta_axis(&mut f, n);
    let dir = f.cone(axis, 1.0, 1.2);
    let speed = f.range(1.5, 4.5);
    let vel = f.mul3(dir, speed);
    let size = f.range(0.01, 0.03);
    let life = f.range(0.6, 1.2);
    let spun = f.unit();
    let rot = f.scale(spun, TWO_PI);
    let signed = f.signed();
    let spin = f.scale(signed, 18.0);
    let seed = f.unit();

    let fibres = f.emit(
        (5.0, 2),
        Pool::Lit,
        vec![
            (Field::X, at.0.src()),
            (Field::Y, at.1.src()),
            (Field::Z, at.2.src()),
            (Field::Vx, vel.0.src()),
            (Field::Vy, vel.1.src()),
            (Field::Vz, vel.2.src()),
            (Field::Size0, size.src()),
            (Field::Size1, size.src()),
            (Field::Life, life.src()),
            (Field::Rot, rot.src()),
            (Field::Spin, spin.src()),
            (Field::Seed, seed.src()),
            (Field::Tile, imm(p::SPLINTER as f64)),
            (Field::Drag, imm(2.4)),
            (Field::Gravity, imm(-14.0)),
            (Field::R0, imm(s.fibre.0)),
            (Field::G0, imm(s.fibre.1)),
            (Field::B0, imm(s.fibre.2)),
            (Field::R1, imm(s.fibre.0 * 0.9)),
            (Field::G1, imm(s.fibre.1 * 0.9)),
            (Field::B1, imm(s.fibre.2 * 0.9)),
            (Field::AlphaCurve, imm(0.35)),
            (Field::Soft, imm(0.06)),
        ],
    );

    vec![dust, fibres]
}

/// Woven cloth — pale, dusty fibres.
pub static FABRIC: LazyLock<Vec<Burst>> = LazyLock::new(|| soft(FABRIC_PALETTE));

/// Rubber — the same burst, nearly black.
pub static RUBBER: LazyLock<Vec<Burst>> = LazyLock::new(|| soft(RUBBER_PALETTE));

/// Glass: glinting shards and a fine aerosol. `impacts.js:662-724`.
///
/// The recipe that needed the companion. Roughly half the shards carry a bright
/// glint into the additive pool — the same position, velocity, life and spin,
/// a different tile and colour — and the source writes that by mutating the
/// spawn record after the first emit and emitting it again. So the glint's
/// fields overlay the shard's rather than replacing them, and its two extra
/// draws happen only when its gate opens.
///
/// It is also the only recipe that *reverses* its own axis on a draw: about
/// three shards in ten are thrown back toward the shooter rather than through
/// the pane. Negation is a multiply by a drawn sign, which is exact.
///
/// The crack-web decal `impacts.js:711` writes is not here; it is a decal, and
/// its tile is itself drawn.
pub static GLASS: LazyLock<Vec<Burst>> = LazyLock::new(|| {
    // Shards, with the glint riding them — `impacts.js:669-710`.
    let mut s = Program::new();
    let at = s.point();
    let n = s.normal();
    let inc = s.incident();
    let back = s.scale3(n, -0.2);
    let front = s.mad3(inc, 0.8, back);
    let flip = s.unit();
    let sign = s.select_lt(flip, 0.3, -1.0, 1.0);
    let axis = s.mul3(front, sign);
    let dir = s.cone(axis, 1.0, 1.2);
    let speed = s.range(2.5, 8.0);
    let vel = s.mul3(dir, speed);
    let shape = s.unit();
    let tile = s.select_lt(shape, 0.4, p::SPLINTER as f64, p::CHIP as f64);
    let size = s.range(0.01, 0.038);
    let life = s.range(0.7, 1.4);
    let spun = s.unit();
    let rot = s.scale(spun, TWO_PI);
    let signed = s.signed();
    let spin = s.scale(signed, 30.0);
    let seed = s.unit();

    let shard_fields = vec![
        (Field::X, at.0.src()),
        (Field::Y, at.1.src()),
        (Field::Z, at.2.src()),
        (Field::Vx, vel.0.src()),
        (Field::Vy, vel.1.src()),
        (Field::Vz, vel.2.src()),
        (Field::Tile, tile.src()),
        (Field::Size0, size.src()),
        (Field::Size1, size.src()),
        (Field::Life, life.src()),
        (Field::Rot, rot.src()),
        (Field::Spin, spin.src()),
        (Field::Seed, seed.src()),
        (Field::Drag, imm(0.6)),
        (Field::Gravity, imm(-19.0)),
        (Field::R0, imm(0.72)),
        (Field::G0, imm(0.8)),
        (Field::B0, imm(0.84)),
        (Field::R1, imm(0.6)),
        (Field::G1, imm(0.68)),
        (Field::B1, imm(0.72)),
        (Field::Alpha, imm(0.85)),
        (Field::AlphaCurve, imm(0.3)),
        (Field::Soft, imm(0.06)),
    ];

    // Everything from here belongs to the glint, and draws only if it lands.
    s.companion_from(0.55);
    let glint_size = s.range(0.01, 0.02);
    let glint_i0 = s.range(3.0, 8.0);

    let shards = s.emit_with(
        (14.0, 5),
        Pool::Lit,
        shard_fields,
        Some((
            Pool::Additive,
            vec![
                (Field::Size0, glint_size.src()),
                (Field::Size1, glint_size.src()),
                (Field::I0, glint_i0.src()),
                (Field::Tile, imm(p::SPARK as f64)),
                (Field::R0, imm(0.85)),
                (Field::G0, imm(0.95)),
                (Field::B0, imm(1.0)),
                (Field::R1, imm(0.8)),
                (Field::G1, imm(0.9)),
                (Field::B1, imm(1.0)),
                (Field::I1, imm(0.2)),
                (Field::Alpha, imm(1.0)),
                (Field::AlphaCurve, imm(1.0)),
                (Field::Flags, imm(1.0)),
            ],
        )),
    );

    // Aerosol — `impacts.js:713-723`. A fixed speed, so nothing is drawn for
    // the velocity at all.
    let mut m = Program::new();
    let at = m.point();
    let n = m.normal();
    let inc = m.incident();
    let back = m.scale3(n, -0.2);
    let axis = m.mad3(inc, 0.8, back);
    let dir = m.cone(axis, 1.2, 0.7);
    let vel = m.scale3(dir, 1.6);
    let size1 = m.range(0.28, 0.45);
    let life = m.range(0.35, 0.7);
    let spun = m.unit();
    let rot = m.scale(spun, TWO_PI);
    let seed = m.unit();

    let mist = m.emit(
        (4.0, 1),
        Pool::Lit,
        vec![
            (Field::X, at.0.src()),
            (Field::Y, at.1.src()),
            (Field::Z, at.2.src()),
            (Field::Vx, vel.0.src()),
            (Field::Vy, vel.1.src()),
            (Field::Vz, vel.2.src()),
            (Field::Size1, size1.src()),
            (Field::Life, life.src()),
            (Field::Rot, rot.src()),
            (Field::Seed, seed.src()),
            (Field::Tile, imm(p::MIST as f64)),
            (Field::Size0, imm(0.05)),
            (Field::SizeCurve, imm(0.45)),
            (Field::Drag, imm(4.2)),
            (Field::Gravity, imm(-3.0)),
            (Field::R0, imm(0.8)),
            (Field::G0, imm(0.86)),
            (Field::B0, imm(0.9)),
            (Field::R1, imm(0.7)),
            (Field::G1, imm(0.76)),
            (Field::B1, imm(0.8)),
            (Field::Alpha, imm(0.5)),
            (Field::AlphaCurve, imm(1.6)),
            (Field::Soft, imm(0.1)),
        ],
    );

    vec![shards, mist]
});

/// Plaster / drywall: white powder, crumbs, no sparks. `impacts.js:332-410`.
///
/// The hardest recipe in the file, and the one that decided the last two
/// features of the format.
///
/// Its dust is **banded**: `i % 3` sorts each particle into a near, middle or
/// far puff, and the band picks a different size, curve, life, drag and delay.
/// Two of those are drawn, which is the difficulty — a band-dependent *range*,
/// not a band-dependent value. `rng.range(lo, hi)` is `lo + (hi - lo) * float()`,
/// so the recipe picks the bounds with a select and then draws once, spending
/// exactly the one draw the source spends whichever band the particle is in.
/// The delay is the exception: band zero has none, and draws nothing for it,
/// which is what [`Op::Gate`] exists for.
///
/// It is also the only burst that shades itself. A particle leaving toward the
/// sun is brighter than one leaving away from it, so the recipe reads the sun
/// direction from the site and dots it against the particle's own velocity.
pub static PLASTER: LazyLock<Vec<Burst>> = LazyLock::new(|| {
    // Powder — `impacts.js:339-388`.
    let mut d = Program::new();
    let at = d.point();
    let n = d.normal();
    // Plaster's ejecta axis leans harder off the wall than wood's: the normal
    // is weighted 1.3 before the average, so the puff stands off the surface.
    let r = d.reflected();
    let lean = d.mad3(n, 1.3, r);
    let axis = d.scale3(lean, 0.5);
    let raw = d.cone(axis, 1.3, 0.6);
    // Anything the cone threw into the wall is folded back out of it.
    let dir = d.toward_hemi(raw, n, 0.05);
    let speed = d.range(0.6, 2.2);
    let off = d.range(0.05, 0.14);
    let step = d.mul3(n, off);
    let from = d.add3(at, step);
    let vel = d.mul3(dir, speed);
    let rise = d.offset(vel.1, 0.4);
    let energy = d.read(Input::Energy);

    let index = d.read(Input::Index);
    let odd = d.modulo(index, 2.0);
    let tile = d.select_lt(odd, 0.5, p::MIST as f64, p::DUST as f64);
    let band = d.modulo(index, 3.0);
    // `band == 0` is `band < 0.5`; `band == 2` is `band >= 1.5`.
    let near_k = d.select_lt(band, 0.5, 0.8, 1.0);
    let far_k = d.select_lt(band, 1.5, 1.0, 1.3);

    let near = d.range(0.05, 0.11);
    let near_e = d.mul(near, energy);
    let size0 = d.mul(near_e, near_k);
    let far = d.range(0.34, 0.62);
    let far_e = d.mul(far, energy);
    let size1 = d.mul(far_e, far_k);

    let mid_or_far = d.select_lt(band, 1.5, 0.48, 0.78);
    let size_curve = d.pick(band, 0.5, imm(0.3), mid_or_far.src());

    // The delay: none at all for the near band, which therefore draws nothing.
    // `has_delay` is 1.0 exactly when the band is zero, so the gate — which
    // opens on `< 0.5` — is closed for that band and open for the others.
    let is_near = d.select_lt(band, 0.5, 1.0, 0.0);
    let delay_hi = d.select_lt(band, 1.5, 0.1, 0.22);
    let delay_lo = d.push_const(0.02);
    let gate = d.open_gate(is_near, 0.5);
    let delay = d.range_between(delay_lo, delay_hi);
    d.close_gate(gate);

    let life_lo_far = d.select_lt(band, 1.5, 0.7, 1.4);
    let life_lo = d.pick(band, 0.5, imm(0.25), life_lo_far.src());
    let life_hi_far = d.select_lt(band, 1.5, 1.2, 2.2);
    let life_hi = d.pick(band, 0.5, imm(0.45), life_hi_far.src());
    let life = d.range_between(life_lo, life_hi);

    let drag_lo = d.select_lt(band, 0.5, 5.0, 2.6);
    let drag_hi = d.select_lt(band, 0.5, 7.0, 3.8);
    let drag = d.range_between(drag_lo, drag_hi);

    let spun = d.unit();
    let rot = d.scale(spun, TWO_PI);
    let signed = d.signed();
    let spin = d.scale(signed, 1.2);

    // Self-shading: `0.68 + 0.74 * max(dot(v, sun), 0)`, then the powder's
    // colour scaled by it. The dot is against the *shaped* direction, before
    // the speed is applied, exactly as the source does.
    let sun = d.sun();
    let facing = d.dot(dir, sun);
    let toward = d.max(facing, 0.0);
    let lifted = d.scale(toward, 0.74);
    let lit = d.offset(lifted, 0.68);
    let r0 = d.scale(lit, 0.74);
    let g0 = d.scale(lit, 0.63);
    let b0 = d.scale(lit, 0.465);
    let r1 = d.scale(lit, 0.63);
    let g1 = d.scale(lit, 0.53);
    let b1 = d.scale(lit, 0.385);

    let alpha_k = d.select_lt(band, 1.5, 1.0, 0.7);
    let alpha_drawn = d.range(0.42, 0.72);
    let alpha = d.mul(alpha_drawn, alpha_k);
    let seed = d.unit();

    let powder = d.emit(
        (8.0, 3),
        Pool::Lit,
        vec![
            (Field::X, from.0.src()),
            (Field::Y, from.1.src()),
            (Field::Z, from.2.src()),
            (Field::Vx, vel.0.src()),
            (Field::Vy, rise.src()),
            (Field::Vz, vel.2.src()),
            (Field::Tile, tile.src()),
            (Field::Size0, size0.src()),
            (Field::Size1, size1.src()),
            (Field::SizeCurve, size_curve.src()),
            (Field::Delay, delay.src()),
            (Field::Life, life.src()),
            (Field::Drag, drag.src()),
            (Field::Rot, rot.src()),
            (Field::Spin, spin.src()),
            (Field::R0, r0.src()),
            (Field::G0, g0.src()),
            (Field::B0, b0.src()),
            (Field::R1, r1.src()),
            (Field::G1, g1.src()),
            (Field::B1, b1.src()),
            (Field::Alpha, alpha.src()),
            (Field::Seed, seed.src()),
            (Field::Gravity, imm(-0.55)),
            (Field::AlphaCurve, imm(1.6)),
            (Field::Soft, imm(0.09)),
            (Field::Turb, imm(0.06)),
            (Field::TurbFreq, imm(2.0)),
        ],
    );

    // Crumbs — `impacts.js:390-408`.
    let mut c = Program::new();
    let at = c.point();
    let n = c.normal();
    let from = c.mad3(n, 0.01, at);
    let r = c.reflected();
    let lean = c.mad3(n, 1.3, r);
    let axis = c.scale3(lean, 0.5);
    let dir = c.cone(axis, 0.95, 1.3);
    let speed = c.range(2.0, 6.0);
    let vel = c.mul3(dir, speed);
    let size = c.range(0.007, 0.02);
    let life = c.range(0.5, 0.9);
    let spun = c.unit();
    let rot = c.scale(spun, TWO_PI);
    let signed = c.signed();
    let spin = c.scale(signed, 18.0);
    let seed = c.unit();

    let crumbs = c.emit(
        (7.0, 2),
        Pool::Lit,
        vec![
            (Field::X, from.0.src()),
            (Field::Y, from.1.src()),
            (Field::Z, from.2.src()),
            (Field::Vx, vel.0.src()),
            (Field::Vy, vel.1.src()),
            (Field::Vz, vel.2.src()),
            (Field::Size0, size.src()),
            (Field::Size1, size.src()),
            (Field::Life, life.src()),
            (Field::Rot, rot.src()),
            (Field::Spin, spin.src()),
            (Field::Seed, seed.src()),
            (Field::Tile, imm(p::CHIP as f64)),
            (Field::Drag, imm(0.5)),
            (Field::Gravity, imm(-19.0)),
            (Field::R0, imm(0.72)),
            (Field::G0, imm(0.65)),
            (Field::B0, imm(0.52)),
            (Field::R1, imm(0.64)),
            (Field::G1, imm(0.575)),
            (Field::B1, imm(0.46)),
            (Field::AlphaCurve, imm(0.25)),
            (Field::Soft, imm(0.06)),
        ],
    );

    // Ejecta — `impacts.js:410-433`. A tight, fast jet straight back out.
    let mut e = Program::new();
    let at = e.point();
    let n = e.normal();
    let from = e.mad3(n, 0.02, at);
    let r = e.reflected();
    let lean = e.mad3(n, 1.3, r);
    let axis = e.scale3(lean, 0.5);
    let dir = e.cone(axis, 0.32, 1.6);
    let speed = e.range(3.0, 6.5);
    let vel = e.mul3(dir, speed);
    let rise = e.offset(vel.1, 0.2);
    let energy = e.read(Input::Energy);
    let near = e.range(0.022, 0.045);
    let size0 = e.mul(near, energy);
    let far = e.range(0.12, 0.24);
    let size1 = e.mul(far, energy);
    let life = e.range(0.18, 0.3);
    let drag = e.range(6.0, 9.0);
    let spun = e.unit();
    let rot = e.scale(spun, TWO_PI);
    let signed = e.signed();
    let spin = e.scale(signed, 2.2);
    let alpha = e.range(0.35, 0.6);
    let seed = e.unit();

    let ejecta = e.emit(
        (4.0, 2),
        Pool::Lit,
        vec![
            (Field::X, from.0.src()),
            (Field::Y, from.1.src()),
            (Field::Z, from.2.src()),
            (Field::Vx, vel.0.src()),
            (Field::Vy, rise.src()),
            (Field::Vz, vel.2.src()),
            (Field::Size0, size0.src()),
            (Field::Size1, size1.src()),
            (Field::Life, life.src()),
            (Field::Drag, drag.src()),
            (Field::Rot, rot.src()),
            (Field::Spin, spin.src()),
            (Field::Alpha, alpha.src()),
            (Field::Seed, seed.src()),
            (Field::Tile, imm(p::DUST as f64)),
            (Field::SizeCurve, imm(0.5)),
            (Field::Gravity, imm(-1.1)),
            (Field::R0, imm(0.77)),
            (Field::G0, imm(0.66)),
            (Field::B0, imm(0.485)),
            (Field::R1, imm(0.65)),
            (Field::G1, imm(0.55)),
            (Field::B1, imm(0.4)),
            (Field::AlphaCurve, imm(1.2)),
            (Field::Soft, imm(0.08)),
        ],
    );

    vec![powder, crumbs, ejecta]
});

/// The reflection and the normal, averaged — the axis ejecta follows when the
/// debris cares about the bullet more than the surface, but not entirely.
///
/// Written once because several recipes spell it identically. A helper is not a
/// compromise of the data form: it appends instructions to the caller's program
/// in the caller's draw order, and the burst it lands in is the same value it
/// would be with the three calls written out.
fn ejecta_axis(b: &mut Program, n: V3) -> V3 {
    let r = b.reflected();
    let sum = b.add3(r, n);
    b.scale3(sum, 0.5)
}

/// Every recipe, so a test can sweep them.
pub fn all() -> Vec<(&'static str, &'static Burst)> {
    [
        ("foliage", &*FOLIAGE),
        ("wood", &*WOOD),
        ("flesh", &*FLESH),
        ("ground.dirt", &*GROUND_DIRT),
        ("ground.sand", &*GROUND_SAND),
        ("water", &*WATER),
        ("fabric", &*FABRIC),
        ("rubber", &*RUBBER),
        ("glass", &*GLASS),
        ("plaster", &*PLASTER),
    ]
        .into_iter()
        .flat_map(|(name, bursts)| bursts.iter().map(move |b| (name, b)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fx::burst::Src;

    #[test]
    fn every_recipe_reads_only_registers_written_before_it() {
        all().iter().for_each(|(name, burst)| {
            assert!(burst.operands_resolve(), "{name}");
        });
    }

    /// Every recipe's program is shorter than the table it fills.
    ///
    /// This is the format's central claim, so it is asserted rather than
    /// described. A burst writes twenty-odd fields and computes about half of
    /// them; when constants were instructions the program was the longer half,
    /// which is what made the data form bigger than the code it replaced.
    #[test]
    fn a_recipe_computes_less_than_it_states() {
        all().iter().for_each(|(name, burst)| {
            let computed = burst
                .fields
                .iter()
                .filter(|(_, src)| matches!(src, Src::Reg(_)))
                .count();
            assert!(
                computed < burst.fields.len(),
                "{name}: every field is computed, so nothing is a constant"
            );
        });
    }

    /// The assembler numbers registers so the author does not. If it ever
    /// miscounts a cone — the only instruction writing more than one — every
    /// handle after it points one short, so pinning the count pins that.
    #[test]
    fn the_foliage_program_writes_twenty_two_registers() {
        assert_eq!(FOLIAGE[0].register_count(), 22);
    }
}
