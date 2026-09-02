//! A particle burst as an engine recipe graph, and the operator semantics for it.
//!
//! Not a port of anything. This is the shape the app is being moved toward: the
//! *behaviour* of an effect is data, and the thing that executes it belongs to
//! the engine.
//!
//! # What lives where
//!
//! The graph is an [`axiom_recipe::RecipeGraph`] — the engine's own recipe type,
//! with its validation, its canonical bytes and its content digest. Walking it
//! is [`axiom_proc_core::ProcCore::evaluate`], the engine's executor. What is
//! left in this file is the part that is genuinely this game's: what each
//! operator *means* for a particle, and which spawn field each result lands in.
//!
//! An earlier draft had a hand-written register machine and a hand-written
//! parser for a bespoke text format, both here in the app. That was wrong twice
//! over — a second private executor beside the engine's, and a language nothing
//! else could read. The engine already had the executor; it was missing only a
//! way to walk a graph against a randomness source the domain owns, which is
//! what `ProcCore::evaluate` now is.
//!
//! # Draw order is node order
//!
//! The random stream is shared across every subsystem in a frame, so a burst
//! that spends one extra draw shifts every later effect — silently, with the
//! frame still looking plausible. A recipe graph is evaluated in id order and
//! every input of node *i* is a node before *i*, so an operator that draws does
//! so when it is reached: **node order is draw order**, by construction.
//!
//! That is exactly the property an address-keyed per-node entropy stream cannot
//! express, and why `evaluate` exists beside `execute`.

use axiom_proc_core::{NodeStep, ProcCore};
use axiom_recipe::{Param, RecipeGraph};

use crate::fx::particles::ParticleSpawn;
use crate::fx::system::FxSystem;
use crate::fx::util::{blackbody, clamp_cone, cone, disc_on, reflect, toward_hemi};

/// What one node produces: a number, or three of them.
///
/// A cone yields a direction and a blackbody yields a colour, so a node's output
/// is not always a scalar. Keeping the vector whole is what removes the lane
/// bookkeeping an earlier register-file design forced on every recipe.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Value {
    Scalar(f64),
    Vec3(f64, f64, f64),
}

impl Value {
    /// One component. Lane 0 of a scalar is the scalar itself, so a field can
    /// name any node without knowing which kind it is.
    pub fn lane(self, lane: u8) -> f64 {
        match (self, lane) {
            (Value::Scalar(v), _) => v,
            (Value::Vec3(x, _, _), 0) => x,
            (Value::Vec3(_, y, _), 1) => y,
            (Value::Vec3(_, _, z), _) => z,
        }
    }

    fn triple(self) -> (f64, f64, f64) {
        match self {
            Value::Vec3(x, y, z) => (x, y, z),
            Value::Scalar(v) => (v, v, v),
        }
    }
}

/// The operator codes a burst graph uses.
///
/// Numbers rather than an enum because that is what a `RecipeGraph` node carries
/// — the engine's executor is deliberately ignorant of what an operator means.
/// The names live here so the asset schema and this evaluator agree on one
/// vocabulary that can be searched for.
pub mod op {
    /// A literal. `params: [f64 pair]`.
    pub const CONST: u16 = 0;
    /// A value from the call site. `params: [input id]`.
    pub const READ: u16 = 1;
    /// `rng.range(lo, hi)`. `inputs: [lo, hi]`. **Draws once.**
    pub const RANGE: u16 = 2;
    /// `rng.float()`. **Draws once.**
    pub const UNIT: u16 = 3;
    /// `rng.signed()`. **Draws once.**
    pub const SIGNED: u16 = 4;
    /// A direction in a cone. `inputs: [axis, spread, power]`. **Draws twice.**
    pub const CONE: u16 = 5;
    /// A point on a disc. `inputs: [normal, radius]`. **Draws twice.**
    pub const DISC_ON: u16 = 6;
    /// The colour of a temperature. `inputs: [kelvin]`.
    pub const BLACKBODY: u16 = 7;
    /// Fold a direction onto the near side of a surface.
    /// `inputs: [dir, axis, bias]`.
    pub const TOWARD_HEMI: u16 = 8;
    /// Pull a direction back inside a cone. `inputs: [dir, axis, cos_max]`.
    pub const CLAMP_CONE: u16 = 9;
    /// `a * b`, componentwise, with a scalar broadcasting across a vector.
    pub const MUL: u16 = 10;
    /// `a + b`.
    pub const ADD: u16 = 11;
    /// `a - b`.
    pub const SUB: u16 = 12;
    /// `a / b`.
    pub const DIV: u16 = 13;
    /// `a % b`.
    pub const MODULO: u16 = 14;
    /// `max(a, b)`.
    pub const MAX: u16 = 15;
    /// `cos(a)`.
    pub const COS: u16 = 16;
    /// `sin(a)`.
    pub const SIN: u16 = 17;
    /// `dot(a, b)`.
    pub const DOT: u16 = 18;
    /// `probe < threshold ? low : high`.
    /// `inputs: [probe, threshold, low, high]`.
    ///
    /// A node rather than a branch in the evaluator, so a select on a *drawn*
    /// value spends its draw exactly once whichever way it lands.
    pub const SELECT_LT: u16 = 19;
    /// Skip the next `len` nodes unless `probe < threshold`.
    /// `inputs: [probe, threshold]`, `params: [len]`.
    ///
    /// **A skipped node does not draw**, which is the point: plaster delays
    /// every particle except its first band and draws that delay only for the
    /// bands that have one. A skipped node still produces a value — zero — so a
    /// field naming it reads exactly what the source substitutes.
    pub const GATE: u16 = 20;
    /// One component of a vector. `inputs: [v]`, `params: [lane]`.
    pub const LANE: u16 = 21;
    /// Three scalars as a vector. `inputs: [x, y, z]`.
    pub const VEC3: u16 = 22;
}

/// The call-site values a `READ` node can name.
pub mod input {
    /// Where the event happened.
    pub const POINT: u32 = 0;
    /// The surface normal.
    pub const NORMAL: u32 = 1;
    /// The incident direction reflected about the normal.
    pub const REFLECTED: u32 = 2;
    /// The incident direction itself.
    pub const INCIDENT: u32 = 3;
    /// The world direction toward the sun.
    pub const SUN: u32 = 4;
    /// The axis an explosion throws along.
    pub const UP: u32 = 5;
    /// How hard the event was.
    pub const ENERGY: u32 = 6;
    /// How big it was, in metres.
    pub const RADIUS: u32 = 7;
    /// Which particle of the burst this is.
    pub const INDEX: u32 = 8;
    /// How many the burst emits.
    pub const COUNT: u32 = 9;
}

/// Which particle pool an emission lands in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pool {
    Additive,
    Lit,
}

/// A field of [`ParticleSpawn`] a burst can write.
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

/// Where a field's value comes from: a lane of a node the graph computed, or a
/// literal.
///
/// The literal arm is what keeps a recipe from being twice the size of the code
/// it replaces. Most of what a burst writes is not computed at all — drag,
/// gravity, the two ends of a colour ramp — and making each of those a node
/// bought nothing, because a constant has no dependencies and spends no draw.
/// So the field list is a **table** and the graph computes only what genuinely
/// derives from a draw, the site, or the index.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Src {
    /// Lane `lane` of node `node`.
    Node { node: u32, lane: u8 },
    /// A literal, written straight through.
    Imm(f64),
}

impl Src {
    fn read(self, cache: &[Value]) -> f64 {
        match self {
            Src::Imm(v) => v,
            Src::Node { node, lane } => cache
                .get(node as usize)
                .map_or(0.0, |value| value.lane(lane)),
        }
    }
}

/// How many particles a burst emits: `round(factor * pscale) + plus`.
///
/// Data because it is quality-dependent, and getting the rounding wrong changes
/// the number of draws and therefore every later effect in the frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Count {
    pub factor: f64,
    pub plus: i32,
}

/// One emission: a program, a pool, and the table it fills.
#[derive(Debug)]
pub struct Emission {
    pub program: RecipeGraph,
    pub pool: Pool,
    pub fields: Vec<(Field, Src)>,
}

/// A burst: a count, an emission, and sometimes a second one.
#[derive(Debug)]
pub struct Burst {
    pub count: Count,
    pub main: Emission,
    /// A second emission from the same spawn, on a draw.
    ///
    /// Glass is what this is for: a shard is thrown, and about half the time a
    /// bright glint rides it — the same position, velocity, life and spin, with
    /// a different tile and colour, into the additive pool instead of the lit
    /// one. The source expresses that by mutating the spawn record after the
    /// first emit and emitting it again, so the companion's fields are an
    /// **overlay** on the parent's, not a fresh record.
    ///
    /// Its gate is drawn after the parent emits, and its own program runs only
    /// when the gate opens — the one place a burst's draw count varies per
    /// particle, and why the gate belongs to the burst rather than the caller.
    pub companion: Option<(f64, Emission)>,
}

/// Where the call site's values enter a program.
#[derive(Debug, Clone, Copy)]
pub struct Site {
    pub point: (f64, f64, f64),
    pub normal: (f64, f64, f64),
    pub incident: (f64, f64, f64),
    pub energy: f64,
    /// The world direction toward the sun. Read only by plaster, which shades
    /// its dust by which way each particle leaves.
    pub sun: (f64, f64, f64),
    /// The axis an explosion throws along. Straight up for an impact, which
    /// never reads it.
    pub up: (f64, f64, f64),
    /// The size of the event. One for an impact, which never reads it.
    pub radius: f64,
}

impl Site {
    /// The site of one impact, reading the scene-level values from `fx`.
    pub fn at(
        fx: &FxSystem,
        point: (f64, f64, f64),
        normal: (f64, f64, f64),
        incident: (f64, f64, f64),
        energy: f64,
    ) -> Self {
        Self {
            point,
            normal,
            incident,
            energy,
            sun: fx.sun_world(),
            up: (0.0, 1.0, 0.0),
            radius: 1.0,
        }
    }

    /// The site of an explosion: a point, the axis it throws along, and how big
    /// it is. No surface, so no normal and no incident direction.
    pub fn blast(
        fx: &FxSystem,
        point: (f64, f64, f64),
        up: (f64, f64, f64),
        radius: f64,
    ) -> Self {
        Self {
            point,
            normal: up,
            incident: (0.0, 0.0, 0.0),
            energy: 1.0,
            sun: fx.sun_world(),
            up,
            radius,
        }
    }

    fn read(&self, id: u32, reflected: (f64, f64, f64), at: Position) -> Option<Value> {
        let vector = |(x, y, z): (f64, f64, f64)| Some(Value::Vec3(x, y, z));
        match id {
            input::POINT => vector(self.point),
            input::NORMAL => vector(self.normal),
            input::REFLECTED => vector(reflected),
            input::INCIDENT => vector(self.incident),
            input::SUN => vector(self.sun),
            input::UP => vector(self.up),
            input::ENERGY => Some(Value::Scalar(self.energy)),
            input::RADIUS => Some(Value::Scalar(self.radius)),
            input::INDEX => Some(Value::Scalar(at.index)),
            input::COUNT => Some(Value::Scalar(at.count)),
            _ => None,
        }
    }
}

/// Where in its burst a particle is. Not part of the site: it changes per
/// particle, and the source uses it for phase — every fourth tile, a delay ramp.
#[derive(Debug, Clone, Copy)]
struct Position {
    index: f64,
    count: f64,
}

/// Run a burst.
///
/// The loop bound, every program and every emission consume the caller's shared
/// random stream in node order, so replacing a hand-written burst with its data
/// form is a byte-identical change or it is a bug. `tests/golden/fx.ledger` is
/// what says which — every pool, plus the state of the stream afterwards.
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

    // One cache for the whole burst. `ProcCore::evaluate` clears and refills it,
    // so the per-particle cost is no allocation at all — which is why that entry
    // point exists rather than `execute`, whose per-node address and cloned
    // input buffer would be two allocations a node on a path that runs on every
    // bullet impact.
    let mut cache: Vec<Value> = Vec::new();

    (0..count).for_each(|index| {
        let at = Position {
            index: f64::from(index),
            count: f64::from(count),
        };
        let mut spawn = crate::fx::particles::reset_spawn();

        evaluate(fx, &burst.main.program, &mut cache, site, reflected, at);
        apply(&mut spawn, &burst.main.fields, &cache);
        emit(fx, burst.main.pool, &spawn);

        burst.companion.iter().for_each(|(threshold, companion)| {
            let gate = fx.rng.float();
            (gate < *threshold).then(|| {
                evaluate(fx, &companion.program, &mut cache, site, reflected, at);
                apply(&mut spawn, &companion.fields, &cache);
                emit(fx, companion.pool, &spawn);
            });
        });
    });
}

/// Run a sequence of bursts, in order.
///
/// Almost every recipe is more than one burst — wood throws splinters and then a
/// resinous puff, an explosion has seven. They are a sequence rather than one
/// long program because each has its own count, pool and fields, and because the
/// source runs them as separate loops: every draw of the second comes after
/// every draw of the first, and array order is what keeps that true.
pub fn run_all(fx: &mut FxSystem, bursts: &[Burst], site: Site) {
    bursts.iter().for_each(|burst| run(fx, burst, site));
}

/// Walk one program, filling `cache` with every node's value.
///
/// The evaluator holds `skip`, the count of nodes a closed gate is still
/// swallowing. That is why `ProcCore::evaluate` takes `FnMut`: conditional
/// evaluation is domain state, not something the executor should know about.
fn evaluate(
    fx: &mut FxSystem,
    program: &RecipeGraph,
    cache: &mut Vec<Value>,
    site: Site,
    reflected: (f64, f64, f64),
    at: Position,
) {
    let mut skip = 0usize;
    let outcome = ProcCore::new().evaluate(program, cache, |step| {
        let closed = skip > 0;
        skip = skip.saturating_sub(1);
        match closed {
            true => Some(Value::Scalar(0.0)),
            false => node(fx, &step, site, reflected, at, &mut skip),
        }
    });
    debug_assert!(outcome.is_ok(), "a burst program failed to evaluate");
}

/// What one operator means for a particle.
fn node(
    fx: &mut FxSystem,
    step: &NodeStep<'_, Value>,
    site: Site,
    reflected: (f64, f64, f64),
    at: Position,
    skip: &mut usize,
) -> Option<Value> {
    let scalar = |slot: usize| step.input(slot).map(|v| v.lane(0));
    let vector = |slot: usize| step.input(slot).map(|v| v.triple());

    match step.op() {
        op::CONST => step
            .params()
            .get(0..2)
            .map(|w| Value::Scalar(Param::from_pair([w[0], w[1]]))),
        op::READ => step
            .params()
            .first()
            .and_then(|p| site.read(p.as_int(), reflected, at)),
        op::RANGE => scalar(0)
            .zip(scalar(1))
            .map(|(lo, hi)| Value::Scalar(fx.rng.range(lo, hi))),
        op::UNIT => Some(Value::Scalar(fx.rng.float())),
        op::SIGNED => Some(Value::Scalar(fx.rng.signed())),
        op::CONE => vector(0).zip(scalar(1).zip(scalar(2))).map(|(a, (s, p))| {
            let (x, y, z) = cone(&mut fx.rng, a.0, a.1, a.2, s, p);
            Value::Vec3(x, y, z)
        }),
        op::DISC_ON => vector(0).zip(scalar(1)).map(|(n, r)| {
            let (x, y, z) = disc_on(&mut fx.rng, n.0, n.1, n.2, r);
            Value::Vec3(x, y, z)
        }),
        op::BLACKBODY => scalar(0).map(|k| {
            let (r, g, b) = blackbody(k);
            Value::Vec3(r, g, b)
        }),
        op::TOWARD_HEMI => vector(0)
            .zip(vector(1).zip(scalar(2)))
            .map(|(d, (a, bias))| {
                let (x, y, z) = toward_hemi(d.0, d.1, d.2, a.0, a.1, a.2, bias);
                Value::Vec3(x, y, z)
            }),
        op::CLAMP_CONE => vector(0)
            .zip(vector(1).zip(scalar(2)))
            .map(|(d, (a, cos_max))| {
                let (x, y, z) = clamp_cone(d.0, d.1, d.2, a.0, a.1, a.2, cos_max);
                Value::Vec3(x, y, z)
            }),
        op::MUL => arithmetic(step, |a, b| a * b),
        op::ADD => arithmetic(step, |a, b| a + b),
        op::SUB => arithmetic(step, |a, b| a - b),
        op::DIV => arithmetic(step, |a, b| a / b),
        op::MODULO => arithmetic(step, |a, b| a % b),
        op::MAX => arithmetic(step, f64::max),
        op::COS => scalar(0).map(|a| Value::Scalar(a.cos())),
        op::SIN => scalar(0).map(|a| Value::Scalar(a.sin())),
        op::DOT => vector(0)
            .zip(vector(1))
            .map(|(a, b)| Value::Scalar(a.0 * b.0 + a.1 * b.1 + a.2 * b.2)),
        op::SELECT_LT => scalar(0)
            .zip(scalar(1))
            .zip(scalar(2).zip(scalar(3)))
            .map(|((probe, threshold), (low, high))| {
                Value::Scalar([high, low][usize::from(probe < threshold)])
            }),
        op::GATE => scalar(0).zip(scalar(1)).map(|(probe, threshold)| {
            let len = step.params().first().map_or(0, |p| p.as_int()) as usize;
            *skip = [0, len][usize::from(probe >= threshold)];
            Value::Scalar(0.0)
        }),
        op::LANE => step
            .input(0)
            .zip(step.params().first())
            .map(|(v, p)| Value::Scalar(v.lane(p.as_int() as u8))),
        op::VEC3 => scalar(0)
            .zip(scalar(1).zip(scalar(2)))
            .map(|(x, (y, z))| Value::Vec3(x, y, z)),
        _ => None,
    }
}

/// A binary operator, componentwise, with a scalar broadcasting across a vector.
///
/// `dir * speed` and `a + b` are the same operator at different arities in the
/// source, so they are one operator here rather than two.
fn arithmetic(step: &NodeStep<'_, Value>, f: fn(f64, f64) -> f64) -> Option<Value> {
    step.input(0)
        .copied()
        .zip(step.input(1).copied())
        .map(|(a, b)| match (a, b) {
            (Value::Scalar(x), Value::Scalar(y)) => Value::Scalar(f(x, y)),
            _ => {
                let (ax, ay, az) = a.triple();
                let (bx, by, bz) = b.triple();
                Value::Vec3(f(ax, bx), f(ay, by), f(az, bz))
            }
        })
}

/// Write a field list onto a spawn record. A companion's list overlays the
/// parent's rather than replacing it, which is what the source does when it
/// mutates the record in place and emits a second time.
fn apply(spawn: &mut ParticleSpawn, fields: &[(Field, Src)], cache: &[Value]) {
    fields
        .iter()
        .for_each(|(field, src)| write(spawn, *field, src.read(cache)));
}

fn emit(fx: &mut FxSystem, pool: Pool, spawn: &ParticleSpawn) {
    match pool {
        Pool::Additive => fx.emit_add(spawn),
        Pool::Lit => fx.emit_lit(spawn),
    };
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
