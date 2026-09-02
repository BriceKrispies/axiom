//! A particle burst expressed as **data**, and the interpreter that runs it.
//!
//! Not a port of anything. This is the shape the app is being moved toward.
//!
//! # Why this exists, and why a table did not work
//!
//! An earlier attempt tried to express these bursts as tables — one row per
//! burst, columns for the values that vary. It failed on measurement: across the
//! 27 bursts in [`crate::fx::impacts`], `s.x` alone takes **twelve** distinct
//! shapes, not twelve distinct numbers. A table holds one shape and varying
//! values; these have varying *shape*.
//!
//! So they are not tables. They are small **programs**, and the vocabulary is
//! closed: about twenty operations cover all 1,352 lines. One interpreter,
//! written and tested once, then every burst is data. That is the leverage a
//! line-by-line rewrite never had — twenty-seven recipes over one evaluator
//! instead of twenty-seven hand-written functions.
//!
//! # Draw order is preserved by construction
//!
//! The random stream is shared across every subsystem in the frame, so a burst
//! that takes one extra draw shifts every later effect — silently. That is the
//! single hardest constraint on datafying this app.
//!
//! It dissolves here. A [`Burst`] is a **flat instruction list evaluated in
//! order**, each instruction writing registers that only later instructions may
//! read — the same single-assignment shape as `axiom_recipe::RecipeGraph`. An
//! instruction that draws does so when it is reached, so *program order is draw
//! order*. There is no scheduling decision left to get wrong.
//!
//! That is also why the engine's `ProcCore::execute` was widened from `Fn` to
//! `FnMut`: an evaluator that owns a shared random stream mutates as it walks.

use crate::fx::particles::ParticleSpawn;
use crate::fx::system::FxSystem;
use crate::fx::util::{cone, reflect};

/// A value the program can read that comes from the call site rather than from
/// an instruction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Input {
    PointX,
    PointY,
    PointZ,
    NormalX,
    NormalY,
    NormalZ,
    /// The incident direction reflected about the normal — `reflect(inc, n)`,
    /// precomputed because every burst that wants it wants all three components.
    ReflectX,
    ReflectY,
    ReflectZ,
    Energy,
}

/// One instruction.
///
/// Each writes a fixed number of registers; an operand is the index of a
/// register written *earlier*. Variants that consume randomness say so, because
/// that is the property the whole format exists to keep honest.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Op {
    /// A literal. Writes 1.
    Const(f64),
    /// A value from the call site. Writes 1.
    Read(Input),
    /// `rng.range(lo, hi)`. Writes 1. **Draws once.**
    Range(f64, f64),
    /// `rng.float()`. Writes 1. **Draws once.**
    Unit,
    /// `rng.signed()`. Writes 1. **Draws once.**
    Signed,
    /// `cone(rng, dir, spread, power)`. Writes 3. **Draws as `cone` does.**
    Cone {
        dir: [u16; 3],
        spread: f64,
        power: f64,
    },
    /// `a * b`. Writes 1.
    Mul(u16, u16),
    /// `a * k`. Writes 1.
    Scale(u16, f64),
    /// Pick `low` or `high` on `probe < threshold`. Writes 1.
    ///
    /// The comparison is an instruction rather than a branch in the interpreter
    /// so that a burst which selects on a *drawn* value — as the foliage chip /
    /// splinter choice does — still spends exactly one draw, in program order.
    SelectLt {
        probe: u16,
        threshold: f64,
        low: f64,
        high: f64,
    },
}

/// Which particle pool an emission lands in.
///
/// Five pools, and a burst that lands in the wrong one passes any assertion
/// that only inspects the additive buffer — so the pool is part of the data,
/// never a default.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pool {
    Additive,
    Lit,
}

/// A field of [`ParticleSpawn`] this format can write.
///
/// Deliberately an enum rather than an index: a burst that writes the wrong
/// slot of a 32-float record is a defect no test would name, and this makes it
/// a compile error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    X,
    Y,
    Z,
    Vx,
    Vy,
    Vz,
    Size0,
    Size1,
    Life,
    Drag,
    Gravity,
    Rot,
    Spin,
    R0,
    G0,
    B0,
    R1,
    G1,
    B1,
    Tile,
    AlphaCurve,
    Soft,
    Seed,
}

/// How many particles a burst emits.
///
/// The count is data because it is quality-dependent: the source computes it as
/// `round(factor * pscale) + plus`, and getting the rounding wrong changes the
/// number of draws and therefore every later effect.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Count {
    pub factor: f64,
    pub plus: i32,
}

/// A burst: a count, a program, and where its values go.
#[derive(Debug, Clone, PartialEq)]
pub struct Burst {
    pub count: Count,
    /// Evaluated once per particle, in order.
    pub ops: &'static [Op],
    pub pool: Pool,
    /// `(field, register)`. Order is irrelevant — every draw has already
    /// happened by the time these are read — but two fields may name the same
    /// register, which is how `s.size1 = s.size0` is expressed.
    pub fields: &'static [(Field, u16)],
}

/// Where the call site's values enter the program.
#[derive(Debug, Clone, Copy)]
pub struct Site {
    pub point: (f64, f64, f64),
    pub normal: (f64, f64, f64),
    pub incident: (f64, f64, f64),
    pub energy: f64,
}

impl Site {
    fn read(&self, input: Input, reflected: (f64, f64, f64)) -> f64 {
        match input {
            Input::PointX => self.point.0,
            Input::PointY => self.point.1,
            Input::PointZ => self.point.2,
            Input::NormalX => self.normal.0,
            Input::NormalY => self.normal.1,
            Input::NormalZ => self.normal.2,
            Input::ReflectX => reflected.0,
            Input::ReflectY => reflected.1,
            Input::ReflectZ => reflected.2,
            Input::Energy => self.energy,
        }
    }
}

impl Op {
    /// How many registers this instruction writes.
    ///
    /// Only [`Op::Cone`] writes more than one, and that single exception is
    /// why a register index is not an instruction index. It is the one thing
    /// about this format that is easy to get wrong by hand, so it is stated as
    /// a function rather than left implicit in the interpreter, and
    /// [`Burst::operands_resolve`] checks every recipe against it.
    pub fn writes(self) -> usize {
        [1, 3][usize::from(matches!(self, Op::Cone { .. }))]
    }

    /// The registers this instruction reads. At most three.
    pub fn operands(self) -> Vec<u16> {
        match self {
            Op::Const(_) | Op::Read(_) | Op::Range(..) | Op::Unit | Op::Signed => Vec::new(),
            Op::Cone { dir, .. } => dir.to_vec(),
            Op::Mul(a, b) => vec![a, b],
            Op::Scale(a, _) => vec![a],
            Op::SelectLt { probe, .. } => vec![probe],
        }
    }
}

impl Burst {
    /// The number of registers a run of this burst produces.
    pub fn register_count(&self) -> usize {
        self.ops.iter().map(|op| op.writes()).sum()
    }

    /// Every operand and every field reads a register written *earlier*.
    ///
    /// A recipe that fails this is reading uninitialised state or running off
    /// the end — a panic at best, and at worst a silently wrong particle. It is
    /// cheap to check and there is no reason for a recipe not to be checked, so
    /// the recipe table's own test runs it over every recipe.
    pub fn operands_resolve(&self) -> bool {
        let ops_ok = self
            .ops
            .iter()
            .scan(0usize, |written, op| {
                let ok = op.operands().iter().all(|r| usize::from(*r) < *written);
                *written += op.writes();
                Some(ok)
            })
            .all(|ok| ok);
        let total = self.register_count();
        let fields_ok = self.fields.iter().all(|(_, r)| usize::from(*r) < total);
        ops_ok & fields_ok
    }
}

/// Run a burst.
///
/// The loop bound, the program and the emission all consume the caller's shared
/// random stream in the order written, so replacing a hand-written burst with
/// its data form is a byte-identical change or it is a bug — there is no third
/// outcome, and `fx.ledger` is what says which.
pub fn run(fx: &mut FxSystem, burst: &Burst, site: Site) {
    let reflected = reflect(
        site.incident.0,
        site.incident.1,
        site.incident.2,
        site.normal.0,
        site.normal.1,
        site.normal.2,
    );
    let count = (burst.count.factor * fx.pscale).round() as i32 + burst.count.plus;

    // One register file for the whole burst, cleared between particles. The
    // source allocates nothing per particle either — `resetSpawn()` writes into
    // a reused record — and a burst is emitted inside a hit reaction, so this is
    // not a place to hand the allocator work.
    let mut regs: Vec<f64> = Vec::with_capacity(burst.register_count());

    for _ in 0..count {
        regs.clear();
        for op in burst.ops {
            match *op {
                Op::Const(v) => regs.push(v),
                Op::Read(input) => regs.push(site.read(input, reflected)),
                Op::Range(lo, hi) => {
                    let v = fx.rng.range(lo, hi);
                    regs.push(v);
                }
                Op::Unit => {
                    let v = fx.rng.float();
                    regs.push(v);
                }
                Op::Signed => {
                    let v = fx.rng.signed();
                    regs.push(v);
                }
                Op::Cone { dir, spread, power } => {
                    let (x, y, z) = cone(
                        &mut fx.rng,
                        regs[dir[0] as usize],
                        regs[dir[1] as usize],
                        regs[dir[2] as usize],
                        spread,
                        power,
                    );
                    regs.extend([x, y, z]);
                }
                Op::Mul(a, b) => regs.push(regs[a as usize] * regs[b as usize]),
                Op::Scale(a, k) => regs.push(regs[a as usize] * k),
                Op::SelectLt {
                    probe,
                    threshold,
                    low,
                    high,
                } => {
                    let picked = [high, low][usize::from(regs[probe as usize] < threshold)];
                    regs.push(picked);
                }
            }
        }

        let mut s = crate::fx::particles::reset_spawn();
        for (field, reg) in burst.fields {
            write(&mut s, *field, regs[*reg as usize]);
        }
        match burst.pool {
            Pool::Additive => fx.emit_add(&s),
            Pool::Lit => fx.emit_lit(&s),
        };
    }
}

fn write(s: &mut ParticleSpawn, field: Field, v: f64) {
    match field {
        Field::X => s.x = v,
        Field::Y => s.y = v,
        Field::Z => s.z = v,
        Field::Vx => s.vx = v,
        Field::Vy => s.vy = v,
        Field::Vz => s.vz = v,
        Field::Size0 => s.size0 = v,
        Field::Size1 => s.size1 = v,
        Field::Life => s.life = v,
        Field::Drag => s.drag = v,
        Field::Gravity => s.gravity = v,
        Field::Rot => s.rot = v,
        Field::Spin => s.spin = v,
        Field::R0 => s.r0 = v,
        Field::G0 => s.g0 = v,
        Field::B0 => s.b0 = v,
        Field::R1 => s.r1 = v,
        Field::G1 => s.g1 = v,
        Field::B1 => s.b1 = v,
        Field::Tile => s.tile = v,
        Field::AlphaCurve => s.alpha_curve = v,
        Field::Soft => s.soft = v,
        Field::Seed => s.seed = v,
    }
}
