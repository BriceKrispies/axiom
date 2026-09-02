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
//! ([`Op::Const`], [`Op::Read`], [`Op::Mul`], [`Op::Scale`]) sit where the
//! source computes them, so a recipe diffs against `impacts.js` by eye.

use crate::fx::atlas::p;
use crate::fx::burst::{Burst, Count, Field, Input, Op, Pool};

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
        // regs 22.. — the constants: drag, gravity, the two ends of the leaf
        // colour ramp, and the alpha/soft shaping.
        Op::Const(2.2),
        Op::Const(-8.0),
        Op::Const(0.14),
        Op::Const(0.22),
        Op::Const(0.08),
        Op::Const(0.11),
        Op::Const(0.17),
        Op::Const(0.06),
        Op::Const(0.4),
        Op::Const(0.06),
    ],
    fields: &[
        (Field::X, 0),
        (Field::Y, 1),
        (Field::Z, 2),
        (Field::Vx, 10),
        (Field::Vy, 11),
        (Field::Vz, 12),
        (Field::Tile, 14),
        // `s.size1 = s.size0` — one register, read twice. A table would have
        // needed a "same as" sentinel here; naming a register just works.
        (Field::Size0, 15),
        (Field::Size1, 15),
        (Field::Life, 16),
        (Field::Drag, 22),
        (Field::Gravity, 23),
        (Field::Rot, 18),
        (Field::Spin, 20),
        (Field::R0, 24),
        (Field::G0, 25),
        (Field::B0, 26),
        (Field::R1, 27),
        (Field::G1, 28),
        (Field::B1, 29),
        (Field::AlphaCurve, 30),
        (Field::Soft, 31),
        (Field::Seed, 21),
    ],
};

/// Every recipe in this file, so a test can sweep them.
pub const ALL: &[(&str, &Burst)] = &[("foliage", &FOLIAGE)];

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
    fn the_foliage_program_writes_thirty_two_registers() {
        assert_eq!(FOLIAGE.register_count(), 32);
    }
}
