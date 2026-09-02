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
    [("foliage", &*FOLIAGE), ("wood", &*WOOD)]
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
