//! Impact bursts, as data.
//!
//! Not a port of anything — this is the data form of recipes that were
//! transcribed into [`crate::fx::impacts`] as Rust functions. Each constant
//! here is a [`Burst`] the interpreter in [`crate::fx::burst`] executes, and
//! its source citation names the `impacts.js` lines it stands for so the audit
//! trail survives the change of form.
//!
//! # Reading a recipe
//!
//! Registers are numbered in the order instructions *write* them, and
//! [`Op::Cone`] writes three. So a register index is not an instruction index
//! after the first cone in a program — the one genuinely error-prone thing
//! about hand-authoring this format. Every recipe is checked by
//! [`Burst::operands_resolve`] (an operand must name an earlier register) and
//! then by the frozen fingerprint ledger, which is what says a *correct*
//! earlier register was named.
//!
//! Instructions appear in **draw order**, and the non-drawing ones
//! ([`Op::Read`], [`Op::Mul`], [`Op::Scale`]) sit where the source computes
//! them, so a recipe diffs against `impacts.js` by eye.
//!
//! Most of what a burst writes is not computed at all. Those values are
//! [`Src::Imm`] in the field table and never enter the program — see [`Src`]
//! for why that split is the whole reason this format is not larger than the
//! code it replaces.

use crate::fx::atlas::p;
use crate::fx::burst::{Burst, Count, Field, Input, Op, Pool, Src};

const TWO_PI: f64 = std::f64::consts::PI * 2.0;

/// Foliage: shredded leaf matter, no hole. `impacts.js:844-864`.
///
/// The first burst expressed this way, and chosen for it: it is short, and it
/// uses the two operations that decide whether the format is viable at all — a
/// count computed from the quality scale (which sets how many times the shared
/// random stream is drawn from) and a *drawn* select (`rng.float() < 0.5`
/// picking one of two atlas tiles). Both had to spend their draws in program
/// order or every later effect in the frame shifts.
pub static FOLIAGE: Burst = Burst {
    count: Count {
        factor: 10.0,
        plus: 4,
    },
    pool: Pool::Lit,
    ops: &[
        // regs 0..2 — the impact point.
        Op::Read(Input::PointX),
        Op::Read(Input::PointY),
        Op::Read(Input::PointZ),
        // regs 3..5 — `reflect(inc, n)`, the ejecta axis.
        Op::Read(Input::ReflectX),
        Op::Read(Input::ReflectY),
        Op::Read(Input::ReflectZ),
        // regs 6..8 — a direction in a wide cone about it. Draws twice.
        Op::Cone {
            dir: [3, 4, 5],
            spread: 1.3,
            power: 1.0,
        },
        // reg 9 — `sp`, the speed the direction is scaled by.
        Op::Range(1.5, 5.0),
        // regs 10..12 — velocity.
        Op::Mul(6, 9),
        Op::Mul(7, 9),
        Op::Mul(8, 9),
        // regs 13, 14 — a coin flip, then the tile it selects. Two
        // instructions because the draw must happen whichever way it lands.
        Op::Unit,
        Op::SelectLt {
            probe: 13,
            threshold: 0.5,
            low: p::CHIP as f64,
            high: p::SPLINTER as f64,
        },
        // reg 15 — size, written to both ends of the ramp (see `fields`).
        Op::Range(0.012, 0.035),
        // reg 16 — life.
        Op::Range(0.8, 1.6),
        // regs 17, 18 — roll.
        Op::Unit,
        Op::Scale(17, TWO_PI),
        // regs 19, 20 — spin, signed.
        Op::Signed,
        Op::Scale(19, 16.0),
        // reg 21 — the per-particle seed. The burst's last draw.
        Op::Unit,
    ],
    fields: &[
        (Field::X, Src::Reg(0)),
        (Field::Y, Src::Reg(1)),
        (Field::Z, Src::Reg(2)),
        (Field::Vx, Src::Reg(10)),
        (Field::Vy, Src::Reg(11)),
        (Field::Vz, Src::Reg(12)),
        (Field::Tile, Src::Reg(14)),
        // `s.size1 = s.size0` — one register, read twice. A table would have
        // needed a "same as" sentinel here; naming a register just works.
        (Field::Size0, Src::Reg(15)),
        (Field::Size1, Src::Reg(15)),
        (Field::Life, Src::Reg(16)),
        (Field::Drag, Src::Imm(2.2)),
        (Field::Gravity, Src::Imm(-8.0)),
        (Field::Rot, Src::Reg(18)),
        (Field::Spin, Src::Reg(20)),
        (Field::R0, Src::Imm(0.14)),
        (Field::G0, Src::Imm(0.22)),
        (Field::B0, Src::Imm(0.08)),
        (Field::R1, Src::Imm(0.11)),
        (Field::G1, Src::Imm(0.17)),
        (Field::B1, Src::Imm(0.06)),
        (Field::AlphaCurve, Src::Imm(0.4)),
        (Field::Soft, Src::Imm(0.06)),
        (Field::Seed, Src::Reg(21)),
    ],
};

/// Wood: splinters and a brown, resinous puff. `impacts.js:546-594`.
///
/// Two bursts, and the second one is why the format has a sequence at all: the
/// source runs two loops, so every draw the splinters make comes before every
/// draw the dust makes, and array order is what keeps that true.
///
/// It also carries the three shapes foliage did not have. The splinter tile is
/// selected on the *loop index* (`i % 4 == 0`, `impacts.js:563`) rather than on
/// a draw — [`Op::Mod`] is exact on small integers, so `i % 4 < 0.5` is `i % 4
/// == 0` and no equality operator is needed. Positions are offset along the
/// surface normal, which is what [`Op::Mad`] exists for. And the dust's size
/// scales with the impact energy, read from the site.
///
/// The bullet hole `impacts.js:588` writes afterwards is not here: a decal is a
/// different subsystem, not a particle burst, and pretending otherwise to make
/// one recipe hold everything is how a format becomes a junk drawer. The call
/// stays in [`crate::fx::impacts`] until decals get a recipe form of their own.
pub static WOOD: [Burst; 2] = [
    // Splinters — `impacts.js:553-575`.
    Burst {
        count: Count {
            factor: 11.0,
            plus: 4,
        },
        pool: Pool::Lit,
        ops: &[
            // regs 0..2 — the point.
            Op::Read(Input::PointX),
            Op::Read(Input::PointY),
            Op::Read(Input::PointZ),
            // regs 3..5 — the normal.
            Op::Read(Input::NormalX),
            Op::Read(Input::NormalY),
            Op::Read(Input::NormalZ),
            // regs 6..8 — the point lifted 1 cm off the surface, so a splinter
            // does not spawn inside the plank it came from.
            Op::Mad(3, 0.01, 0),
            Op::Mad(4, 0.01, 1),
            Op::Mad(5, 0.01, 2),
            // regs 9..11 — `r`, the ejecta axis: the reflection and the normal,
            // averaged. Splinters follow the bullet more than they follow the
            // surface, but not entirely.
            Op::Read(Input::ReflectX),
            Op::Read(Input::ReflectY),
            Op::Read(Input::ReflectZ),
            Op::Add(9, 3),
            Op::Add(10, 4),
            Op::Add(11, 5),
            Op::Scale(12, 0.5),
            Op::Scale(13, 0.5),
            Op::Scale(14, 0.5),
            // regs 18..20 — a tight, axis-biased cone. Draws twice.
            Op::Cone {
                dir: [15, 16, 17],
                spread: 0.9,
                power: 1.3,
            },
            // reg 21 — speed; regs 22..24 — velocity.
            Op::Range(2.5, 7.5),
            Op::Mul(18, 21),
            Op::Mul(19, 21),
            Op::Mul(20, 21),
            // regs 25..27 — every fourth splinter is a chip rather than a
            // sliver. No draw: the index decides.
            Op::Read(Input::Index),
            Op::Mod(25, 4.0),
            Op::SelectLt {
                probe: 26,
                threshold: 0.5,
                low: p::CHIP as f64,
                high: p::SPLINTER as f64,
            },
            // reg 28 — size, both ends.
            Op::Range(0.014, 0.045),
            // reg 29 — life.
            Op::Range(0.6, 1.2),
            // regs 30, 31 — roll.
            Op::Unit,
            Op::Scale(30, TWO_PI),
            // regs 32, 33 — spin.
            Op::Signed,
            Op::Scale(32, 26.0),
            // reg 34 — seed, the burst's last draw.
            Op::Unit,
        ],
        fields: &[
            (Field::X, Src::Reg(6)),
            (Field::Y, Src::Reg(7)),
            (Field::Z, Src::Reg(8)),
            (Field::Vx, Src::Reg(22)),
            (Field::Vy, Src::Reg(23)),
            (Field::Vz, Src::Reg(24)),
            (Field::Tile, Src::Reg(27)),
            (Field::Size0, Src::Reg(28)),
            (Field::Size1, Src::Reg(28)),
            (Field::Life, Src::Reg(29)),
            (Field::Drag, Src::Imm(0.8)),
            (Field::Gravity, Src::Imm(-18.0)),
            (Field::Rot, Src::Reg(31)),
            (Field::Spin, Src::Reg(33)),
            (Field::R0, Src::Imm(0.44)),
            (Field::G0, Src::Imm(0.3)),
            (Field::B0, Src::Imm(0.16)),
            (Field::R1, Src::Imm(0.36)),
            (Field::G1, Src::Imm(0.24)),
            (Field::B1, Src::Imm(0.13)),
            (Field::AlphaCurve, Src::Imm(0.25)),
            (Field::Soft, Src::Imm(0.06)),
            (Field::Seed, Src::Reg(34)),
        ],
    },
    // Resinous dust — `impacts.js:577-587`.
    Burst {
        count: Count {
            factor: 5.0,
            plus: 2,
        },
        pool: Pool::Lit,
        ops: &[
            // regs 0..2 — the point; regs 3..5 — the normal.
            Op::Read(Input::PointX),
            Op::Read(Input::PointY),
            Op::Read(Input::PointZ),
            Op::Read(Input::NormalX),
            Op::Read(Input::NormalY),
            Op::Read(Input::NormalZ),
            // regs 6..8 — `r`, as above.
            Op::Read(Input::ReflectX),
            Op::Read(Input::ReflectY),
            Op::Read(Input::ReflectZ),
            Op::Add(6, 3),
            Op::Add(7, 4),
            Op::Add(8, 5),
            Op::Scale(9, 0.5),
            Op::Scale(10, 0.5),
            Op::Scale(11, 0.5),
            // regs 15..17 — a wider, flatter cone. Draws twice.
            Op::Cone {
                dir: [12, 13, 14],
                spread: 1.1,
                power: 0.7,
            },
            // reg 18 — speed. Drawn before the offset, as the source does.
            Op::Range(0.6, 2.0),
            // reg 19 — how far off the surface the puff starts. One draw, read
            // three times: the offset is along the normal, so it is one
            // distance, not three. A table would have needed three columns and
            // a rule saying they are always equal.
            Op::Range(0.05, 0.12),
            // regs 20..25 — the offset applied. `Mad`'s multiplier is a
            // constant, so a *drawn* scale is a multiply and an add.
            Op::Mul(3, 19),
            Op::Mul(4, 19),
            Op::Mul(5, 19),
            Op::Add(0, 20),
            Op::Add(1, 21),
            Op::Add(2, 22),
            // regs 26..29 — velocity, with the puff drifting upward.
            Op::Mul(15, 18),
            Op::Mul(16, 18),
            Op::Mul(17, 18),
            Op::Offset(27, 0.35),
            // reg 30 — energy, which both size ends scale by.
            Op::Read(Input::Energy),
            // regs 31..34 — the two ends of the size ramp.
            Op::Range(0.04, 0.09),
            Op::Mul(31, 30),
            Op::Range(0.24, 0.44),
            Op::Mul(33, 30),
            // reg 35 — life.
            Op::Range(0.45, 0.9),
            // regs 36, 37 — roll.
            Op::Unit,
            Op::Scale(36, TWO_PI),
            // regs 38, 39 — spin.
            Op::Signed,
            Op::Scale(38, 1.4),
            // reg 40 — alpha.
            Op::Range(0.5, 0.8),
            // reg 41 — seed, the burst's last draw.
            Op::Unit,
        ],
        fields: &[
            (Field::X, Src::Reg(23)),
            (Field::Y, Src::Reg(24)),
            (Field::Z, Src::Reg(25)),
            (Field::Vx, Src::Reg(26)),
            (Field::Vy, Src::Reg(29)),
            (Field::Vz, Src::Reg(28)),
            (Field::Tile, Src::Imm(p::DUST as f64)),
            (Field::Size0, Src::Reg(32)),
            (Field::Size1, Src::Reg(34)),
            (Field::SizeCurve, Src::Imm(0.45)),
            (Field::Life, Src::Reg(35)),
            (Field::Drag, Src::Imm(3.4)),
            (Field::Gravity, Src::Imm(-0.9)),
            (Field::Rot, Src::Reg(37)),
            (Field::Spin, Src::Reg(39)),
            (Field::R0, Src::Imm(0.46)),
            (Field::G0, Src::Imm(0.34)),
            (Field::B0, Src::Imm(0.2)),
            (Field::R1, Src::Imm(0.38)),
            (Field::G1, Src::Imm(0.28)),
            (Field::B1, Src::Imm(0.17)),
            (Field::Alpha, Src::Reg(40)),
            (Field::AlphaCurve, Src::Imm(1.5)),
            (Field::Soft, Src::Imm(0.09)),
            (Field::Turb, Src::Imm(0.05)),
            (Field::TurbFreq, Src::Imm(2.2)),
            (Field::Seed, Src::Reg(41)),
        ],
    },
];

/// Every recipe in this file, so a test can sweep them.
pub const ALL: &[(&str, &Burst)] = &[
    ("foliage", &FOLIAGE),
    ("wood.splinters", &WOOD[0]),
    ("wood.dust", &WOOD[1]),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_recipe_reads_only_registers_written_before_it() {
        ALL.iter().for_each(|(name, burst)| {
            assert!(burst.operands_resolve(), "{name}");
        });
    }

    /// The register count is what `run` sizes its register file to; if a recipe
    /// disagrees with the interpreter about it, the file reallocates mid-burst
    /// — harmless, but it means one of the two is miscounting cones.
    #[test]
    fn the_foliage_program_writes_twenty_two_registers() {
        assert_eq!(FOLIAGE.register_count(), 22);
    }

    /// Every recipe's program is shorter than the table it fills.
    ///
    /// This is the format's central claim, so it is asserted rather than
    /// described. A burst writes twenty-odd fields and computes about half of
    /// them; when constants were instructions the program was the longer half,
    /// which is what made the data form bigger than the code it replaced.
    #[test]
    fn a_recipe_computes_less_than_it_states() {
        ALL.iter().for_each(|(name, burst)| {
            let computed = burst
                .fields
                .iter()
                .filter(|(_, src)| matches!(src, Src::Reg(_)))
                .count();
            assert!(
                computed < burst.fields.len(),
                "{name}: every field is computed, so nothing is a constant"
            );
            assert!(burst.ops.len() < burst.fields.len() * 2, "{name}");
        });
    }
}
