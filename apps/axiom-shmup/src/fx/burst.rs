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
use crate::fx::util::{blackbody, clamp_cone, cone, disc_on, reflect, toward_hemi};

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
    /// The incident direction — the way the bullet was travelling.
    IncidentX,
    IncidentY,
    IncidentZ,
    Energy,
    /// The index of the particle being emitted, `0..count`.
    ///
    /// The source uses it for phase: `i % 4 == 0` picks every fourth tile, a
    /// delay ramp spreads a burst over several frames. It draws nothing, so it
    /// costs the stream nothing to read.
    Index,
    /// How many particles this burst emits — the resolved count, so a recipe
    /// can normalise the index into `0..1` without knowing the quality scale.
    Count,
}

/// One instruction.
///
/// Each writes a fixed number of registers; an operand is the index of a
/// register written *earlier*. Variants that consume randomness say so, because
/// that is the property the whole format exists to keep honest.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Op {
    /// A value from the call site. Writes 1.
    Read(Input),
    /// `rng.range(lo, hi)`. Writes 1. **Draws once.**
    Range(f64, f64),
    /// `rng.float()`. Writes 1. **Draws once.**
    Unit,
    /// `rng.signed()`. Writes 1. **Draws once.**
    Signed,
    /// `cone(rng, dir, spread, power)`. Writes 3. **Draws twice.**
    Cone {
        dir: [u16; 3],
        spread: f64,
        power: f64,
    },
    /// A point on a disc of radius `r` in the plane through the origin with
    /// normal `n`. Writes 3. **Draws twice.**
    ///
    /// The source uses it to spread a plume's spawn points over the crater
    /// rather than stacking them on one point, so it is a *position* offset
    /// where [`Op::Cone`] is a direction.
    DiscOn { normal: [u16; 3], radius: f64 },
    /// The blackbody colour of a temperature in kelvin, normalised so its
    /// brightest channel is 1. Writes 3. Draws nothing.
    ///
    /// A pure function of one register, and the register is usually itself
    /// drawn — a spark's temperature jitters per particle — which is why the
    /// temperature is an operand rather than a parameter.
    Blackbody(u16),
    /// Force a direction into the hemisphere about an axis, mirroring whatever
    /// component points the wrong way and keeping a minimum forward bias.
    /// Writes 3. Draws nothing.
    TowardHemi {
        dir: [u16; 3],
        axis: [u16; 3],
        bias: f64,
    },
    /// Pull a direction back inside a cone about an axis, if it has strayed
    /// outside. Writes 3. Draws nothing.
    ClampCone {
        dir: [u16; 3],
        axis: [u16; 3],
        cos_max: f64,
    },
    /// `a * b`. Writes 1.
    Mul(u16, u16),
    /// `a * k`. Writes 1.
    Scale(u16, f64),
    /// `a + b`. Writes 1.
    Add(u16, u16),
    /// `a + k`. Writes 1.
    Offset(u16, f64),
    /// `a * k1 + k2`, the shape `px + n.x * off` takes everywhere in this
    /// corpus. One instruction rather than a `Scale` and an `Add` because the
    /// pair is the single most common thing a burst does, and three registers
    /// per axis adds up fast in a format read by eye.
    Mad(u16, f64, u16),
    /// `a % k`. Writes 1.
    ///
    /// Exact on the integers it is used with, which is what lets `i % 4 == 0`
    /// be a [`Op::SelectLt`] against `0.5` rather than an equality operator
    /// this format would otherwise need.
    Mod(u16, f64),
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
    SizeCurve,
    Life,
    Delay,
    Drag,
    Gravity,
    Rot,
    Spin,
    Stretch,
    R0,
    G0,
    B0,
    I0,
    R1,
    G1,
    B1,
    I1,
    Tile,
    Soft,
    Alpha,
    AlphaCurve,
    Turb,
    TurbFreq,
    Seed,
    Flags,
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

/// Where a field's value comes from: a register the program computed, or a
/// literal.
///
/// The literal arm is what keeps this format from being twice the size of the
/// code it replaces. Most of what a burst writes is not computed at all — drag,
/// gravity, the two ends of a colour ramp, an alpha curve. Making each of those
/// an instruction that pushes a constant onto a register file was pure
/// ceremony: it doubled the instruction count, it made the register numbering
/// harder to follow by eye, and it bought nothing, because a constant has no
/// dependencies and no draw.
///
/// So the field list is a **table** — field, value — and the program computes
/// only the values that genuinely derive from a draw, the site, or the index.
/// That is the split the earlier table-only analysis of this file missed: these
/// bursts are neither pure tables nor pure programs, they are a table of
/// constants with a small program feeding some of its cells.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Src {
    /// A register the program wrote.
    Reg(u16),
    /// A literal, written straight through.
    Imm(f64),
}

/// A burst: a count, a program, and where its values go.
#[derive(Debug, Clone, PartialEq)]
pub struct Burst {
    pub count: Count,
    /// Evaluated once per particle, in order. Only what a constant cannot say.
    pub ops: Vec<Op>,
    pub pool: Pool,
    /// `(field, value)`. Order is irrelevant — every draw has already happened
    /// by the time these are read — but two fields may name the same register,
    /// which is how `s.size1 = s.size0` is expressed.
    pub fields: Vec<(Field, Src)>,
}

/// A register the program has written. An opaque handle, not a number.
///
/// The number is what the first draft of this format made the author count, and
/// counting is exactly what an author is bad at: [`Op::Cone`] writes three
/// registers, so an instruction index and a register index diverge after the
/// first cone in a program, and a burst that names the wrong one still runs and
/// still emits plausible particles. Only the frozen ledger notices, and it
/// notices by moving a digest — which says *something* is wrong, not what.
///
/// A handle you receive from the instruction that wrote it cannot be
/// miscounted. That is the entire argument for [`Program`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Reg(u16);

impl Reg {
    /// This register as a field value.
    pub const fn src(self) -> Src {
        Src::Reg(self.0)
    }
}

/// Three registers holding a vector, so a burst can say `cone` once instead of
/// three times and keep the components in the order they were written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct V3(pub Reg, pub Reg, pub Reg);

/// Builds a burst's program, handing back a [`Reg`] for every value it writes.
///
/// The output is still data — a [`Burst`] is a plain value with no code in it,
/// and the endpoint of this work is a parser that produces the same value from
/// an asset file. This is the authoring surface, and it is the same one the
/// engine's own recipe graphs use: `RecipeGraph::add` likewise returns a handle
/// to the node it appended rather than asking the caller to know its index.
///
/// Instruction order is draw order, so **the order you call these is the order
/// the burst draws** — that is the property the whole format exists to keep, and
/// a builder does not weaken it.
#[derive(Debug, Default)]
pub struct Program {
    ops: Vec<Op>,
    written: u16,
}

impl Program {
    /// An empty program.
    pub fn new() -> Self {
        Self::default()
    }

    fn push(&mut self, op: Op) -> Reg {
        let first = self.written;
        self.written += op.writes() as u16;
        self.ops.push(op);
        Reg(first)
    }

    /// A value from the call site.
    pub fn read(&mut self, input: Input) -> Reg {
        self.push(Op::Read(input))
    }

    /// The impact point.
    pub fn point(&mut self) -> V3 {
        self.read3(Input::PointX, Input::PointY, Input::PointZ)
    }

    /// The surface normal.
    pub fn normal(&mut self) -> V3 {
        self.read3(Input::NormalX, Input::NormalY, Input::NormalZ)
    }

    /// The incident direction, reflected about the normal.
    pub fn reflected(&mut self) -> V3 {
        self.read3(Input::ReflectX, Input::ReflectY, Input::ReflectZ)
    }

    /// The incident direction itself.
    pub fn incident(&mut self) -> V3 {
        self.read3(Input::IncidentX, Input::IncidentY, Input::IncidentZ)
    }

    fn read3(&mut self, x: Input, y: Input, z: Input) -> V3 {
        V3(self.read(x), self.read(y), self.read(z))
    }

    /// `rng.range(lo, hi)`. **Draws once.**
    pub fn range(&mut self, lo: f64, hi: f64) -> Reg {
        self.push(Op::Range(lo, hi))
    }

    /// `rng.float()`. **Draws once.**
    pub fn unit(&mut self) -> Reg {
        self.push(Op::Unit)
    }

    /// `rng.signed()`. **Draws once.**
    pub fn signed(&mut self) -> Reg {
        self.push(Op::Signed)
    }

    /// A direction in a cone about `dir`. **Draws twice.**
    pub fn cone(&mut self, dir: V3, spread: f64, power: f64) -> V3 {
        self.triple(Op::Cone {
            dir: [dir.0 .0, dir.1 .0, dir.2 .0],
            spread,
            power,
        })
    }

    /// Append an instruction that writes three registers, handing back all
    /// three. The one place the `+ 1`/`+ 2` is written, so a recipe never
    /// touches a raw register number even for the instructions that break the
    /// one-instruction-one-register rule.
    fn triple(&mut self, op: Op) -> V3 {
        let first = self.push(op);
        V3(first, Reg(first.0 + 1), Reg(first.0 + 2))
    }

    /// A point on a disc of radius `r` about `normal`. **Draws twice.**
    pub fn disc_on(&mut self, normal: V3, radius: f64) -> V3 {
        self.triple(Op::DiscOn {
            normal: [normal.0 .0, normal.1 .0, normal.2 .0],
            radius,
        })
    }

    /// The blackbody colour of a temperature register.
    pub fn blackbody(&mut self, kelvin: Reg) -> V3 {
        self.triple(Op::Blackbody(kelvin.0))
    }

    /// Force `dir` into the hemisphere about `axis`, keeping `bias` forward.
    pub fn toward_hemi(&mut self, dir: V3, axis: V3, bias: f64) -> V3 {
        self.triple(Op::TowardHemi {
            dir: [dir.0 .0, dir.1 .0, dir.2 .0],
            axis: [axis.0 .0, axis.1 .0, axis.2 .0],
            bias,
        })
    }

    /// Pull `dir` back inside a cone about `axis`.
    pub fn clamp_cone(&mut self, dir: V3, axis: V3, cos_max: f64) -> V3 {
        self.triple(Op::ClampCone {
            dir: [dir.0 .0, dir.1 .0, dir.2 .0],
            axis: [axis.0 .0, axis.1 .0, axis.2 .0],
            cos_max,
        })
    }

    /// `a * b`.
    pub fn mul(&mut self, a: Reg, b: Reg) -> Reg {
        self.push(Op::Mul(a.0, b.0))
    }

    /// `a * k`.
    pub fn scale(&mut self, a: Reg, k: f64) -> Reg {
        self.push(Op::Scale(a.0, k))
    }

    /// `a + b`.
    pub fn add(&mut self, a: Reg, b: Reg) -> Reg {
        self.push(Op::Add(a.0, b.0))
    }

    /// `a + k`.
    pub fn offset(&mut self, a: Reg, k: f64) -> Reg {
        self.push(Op::Offset(a.0, k))
    }

    /// `a * k + b`.
    pub fn mad(&mut self, a: Reg, k: f64, b: Reg) -> Reg {
        self.push(Op::Mad(a.0, k, b.0))
    }

    /// `a % k`.
    pub fn modulo(&mut self, a: Reg, k: f64) -> Reg {
        self.push(Op::Mod(a.0, k))
    }

    /// `probe < threshold ? low : high`.
    pub fn select_lt(&mut self, probe: Reg, threshold: f64, low: f64, high: f64) -> Reg {
        self.push(Op::SelectLt {
            probe: probe.0,
            threshold,
            low,
            high,
        })
    }

    /// `v * s`, componentwise against one scalar.
    pub fn mul3(&mut self, v: V3, s: Reg) -> V3 {
        V3(self.mul(v.0, s), self.mul(v.1, s), self.mul(v.2, s))
    }

    /// `v * k`.
    pub fn scale3(&mut self, v: V3, k: f64) -> V3 {
        V3(
            self.scale(v.0, k),
            self.scale(v.1, k),
            self.scale(v.2, k),
        )
    }

    /// `a + b`.
    pub fn add3(&mut self, a: V3, b: V3) -> V3 {
        V3(self.add(a.0, b.0), self.add(a.1, b.1), self.add(a.2, b.2))
    }

    /// `a * k + b`, the shape a position offset along a normal takes.
    pub fn mad3(&mut self, a: V3, k: f64, b: V3) -> V3 {
        V3(
            self.mad(a.0, k, b.0),
            self.mad(a.1, k, b.1),
            self.mad(a.2, k, b.2),
        )
    }

    /// Close the program into a burst.
    ///
    /// `count` is `round(factor * pscale) + plus`, which is what decides how
    /// many times the burst draws and therefore what every later effect in the
    /// frame sees.
    pub fn emit(
        self,
        count: (f64, i32),
        pool: Pool,
        fields: Vec<(Field, Src)>,
    ) -> Burst {
        Burst {
            count: Count {
                factor: count.0,
                plus: count.1,
            },
            ops: self.ops,
            pool,
            fields,
        }
    }
}

/// A literal field value, so a recipe's constant table reads as a table.
pub const fn imm(v: f64) -> Src {
    Src::Imm(v)
}

/// Where the call site's values enter the program.
#[derive(Debug, Clone, Copy)]
pub struct Site {
    pub point: (f64, f64, f64),
    pub normal: (f64, f64, f64),
    pub incident: (f64, f64, f64),
    pub energy: f64,
}

/// The per-particle values [`Input`] can reach that are not part of the site:
/// where in the burst this particle is, and how long the burst is.
#[derive(Debug, Clone, Copy)]
struct Position {
    index: f64,
    count: f64,
}

impl Site {
    fn read(&self, input: Input, reflected: (f64, f64, f64), at: Position) -> f64 {
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
            Input::IncidentX => self.incident.0,
            Input::IncidentY => self.incident.1,
            Input::IncidentZ => self.incident.2,
            Input::Energy => self.energy,
            Input::Index => at.index,
            Input::Count => at.count,
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
        let three = matches!(
            self,
            Op::Cone { .. }
                | Op::DiscOn { .. }
                | Op::Blackbody(_)
                | Op::TowardHemi { .. }
                | Op::ClampCone { .. }
        );
        [1, 3][usize::from(three)]
    }

    /// The registers this instruction reads. At most three.
    pub fn operands(self) -> Vec<u16> {
        match self {
            Op::Read(_) | Op::Range(..) | Op::Unit | Op::Signed => Vec::new(),
            Op::Cone { dir, .. } => dir.to_vec(),
            Op::DiscOn { normal, .. } => normal.to_vec(),
            Op::Blackbody(k) => vec![k],
            Op::TowardHemi { dir, axis, .. } | Op::ClampCone { dir, axis, .. } => {
                dir.iter().chain(axis.iter()).copied().collect()
            }
            Op::Mul(a, b) | Op::Add(a, b) => vec![a, b],
            Op::Mad(a, _, b) => vec![a, b],
            Op::Scale(a, _) | Op::Offset(a, _) | Op::Mod(a, _) => vec![a],
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
        let fields_ok = self.fields.iter().all(|(_, src)| match src {
            Src::Reg(r) => usize::from(*r) < total,
            Src::Imm(_) => true,
        });
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

    (0..count).for_each(|index| {
        let at = Position {
            index: f64::from(index),
            count: f64::from(count),
        };
        regs.clear();
        for op in &burst.ops {
            match *op {
                Op::Read(input) => regs.push(site.read(input, reflected, at)),
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
                Op::DiscOn { normal, radius } => {
                    let (x, y, z) = disc_on(
                        &mut fx.rng,
                        regs[normal[0] as usize],
                        regs[normal[1] as usize],
                        regs[normal[2] as usize],
                        radius,
                    );
                    regs.extend([x, y, z]);
                }
                Op::Blackbody(k) => {
                    let (r, g, b) = blackbody(regs[k as usize]);
                    regs.extend([r, g, b]);
                }
                Op::TowardHemi { dir, axis, bias } => {
                    let (x, y, z) = toward_hemi(
                        regs[dir[0] as usize],
                        regs[dir[1] as usize],
                        regs[dir[2] as usize],
                        regs[axis[0] as usize],
                        regs[axis[1] as usize],
                        regs[axis[2] as usize],
                        bias,
                    );
                    regs.extend([x, y, z]);
                }
                Op::ClampCone { dir, axis, cos_max } => {
                    let (x, y, z) = clamp_cone(
                        regs[dir[0] as usize],
                        regs[dir[1] as usize],
                        regs[dir[2] as usize],
                        regs[axis[0] as usize],
                        regs[axis[1] as usize],
                        regs[axis[2] as usize],
                        cos_max,
                    );
                    regs.extend([x, y, z]);
                }
                Op::Mul(a, b) => regs.push(regs[a as usize] * regs[b as usize]),
                Op::Scale(a, k) => regs.push(regs[a as usize] * k),
                Op::Add(a, b) => regs.push(regs[a as usize] + regs[b as usize]),
                Op::Offset(a, k) => regs.push(regs[a as usize] + k),
                Op::Mad(a, k, b) => regs.push(regs[a as usize] * k + regs[b as usize]),
                Op::Mod(a, k) => regs.push(regs[a as usize] % k),
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
        for (field, src) in &burst.fields {
            let v = match src {
                Src::Reg(r) => regs[*r as usize],
                Src::Imm(k) => *k,
            };
            write(&mut s, *field, v);
        }
        match burst.pool {
            Pool::Additive => fx.emit_add(&s),
            Pool::Lit => fx.emit_lit(&s),
        };
    });
}

/// Run a sequence of bursts, in order.
///
/// Almost every recipe in this corpus is more than one burst — wood throws
/// splinters and then a resinous puff, concrete has five. They are a sequence
/// rather than one burst with a longer program because each has its own count,
/// its own pool and its own fields, and because the source runs them as
/// separate loops: the second burst's draws all come after the first burst's,
/// and the array order is what keeps that true.
pub fn run_all(fx: &mut FxSystem, bursts: &[Burst], site: Site) {
    bursts.iter().for_each(|burst| run(fx, burst, site));
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
        Field::SizeCurve => s.size_curve = v,
        Field::Life => s.life = v,
        Field::Delay => s.delay = v,
        Field::Drag => s.drag = v,
        Field::Gravity => s.gravity = v,
        Field::Rot => s.rot = v,
        Field::Spin => s.spin = v,
        Field::Stretch => s.stretch = v,
        Field::R0 => s.r0 = v,
        Field::G0 => s.g0 = v,
        Field::B0 => s.b0 = v,
        Field::I0 => s.i0 = v,
        Field::R1 => s.r1 = v,
        Field::G1 => s.g1 = v,
        Field::B1 => s.b1 = v,
        Field::I1 => s.i1 = v,
        Field::Tile => s.tile = v,
        Field::Soft => s.soft = v,
        Field::Alpha => s.alpha = v,
        Field::AlphaCurve => s.alpha_curve = v,
        Field::Turb => s.turb = v,
        Field::TurbFreq => s.turb_freq = v,
        Field::Seed => s.seed = v,
        Field::Flags => s.flags = v,
    }
}
