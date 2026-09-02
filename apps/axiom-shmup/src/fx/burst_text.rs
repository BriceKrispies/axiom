//! Reading a burst recipe from text.
//!
//! Not a port of anything. This is the step that makes the recipes *content*
//! rather than source: a `.burst` file is data an artist or an agent can read
//! and change without touching Rust, and what it parses to is exactly the
//! [`Burst`] the builder used to produce.
//!
//! # Why the recipes had to leave Rust
//!
//! Expressed as Rust builders, the ten converted impact recipes were about
//! twelve hundred lines against the seven hundred of code they replaced. The
//! behaviour was right and the size was not, and the reason is nothing to do
//! with the format: it is that Rust spends a whole line saying
//! `(Field::Drag, imm(2.2)),` where the thing being said is `drag 2.2`. A
//! recipe is a value with no code in it, so the syntax around it was pure
//! overhead.
//!
//! # The language
//!
//! Line-oriented, `#` to end of line is a comment, blank lines ignored.
//!
//! ```text
//! burst wood.splinters      # starts a burst; the previous one ends here
//! count 11 4                # round(11 * pscale) + 4 particles
//! pool lit                  # lit | add
//!
//! at   = point              # the program. every line names its result, and
//! n    = normal             # an argument is a name written earlier or a
//! from = mad3 n 0.01 at     # number.
//! dir  = cone axis 0.9 1.3
//!
//! gate is_near < 0.5        # the run up to `end` executes, and draws,
//!   delay = range_between lo hi
//! end                       # only when the probe passes
//!
//! fields                    # what the particle is made of
//! x     from.x              # a name, with .x/.y/.z for a vector
//! drag  0.8                 # or a literal
//!
//! companion 0.55 add        # a second emission from the same spawn,
//! gsize = range 0.01 0.02   # on a draw. more program, then its own
//! fields                    # fields, which overlay the parent's
//! size0 gsize
//! ```
//!
//! Draw order is line order, exactly as it is instruction order in the built
//! form — the property the whole format exists to preserve.

use std::collections::HashMap;

use crate::fx::atlas::p;
use crate::fx::burst::{Burst, Field, GateMark, Pool, Program, Reg, Src, V3};

/// A named value in a recipe: one register, or three.
#[derive(Debug, Clone, Copy)]
enum Val {
    Scalar(Reg),
    Vector(V3),
}

/// Parse every burst in a recipe file.
///
/// Errors name the line, because a recipe that fails to parse at startup is a
/// crash with no stack worth reading otherwise.
pub fn parse(src: &str) -> Result<Vec<Burst>, String> {
    let mut out = Vec::new();
    let mut cur: Option<Draft> = None;

    for (no, raw) in src.lines().enumerate() {
        let line = raw.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let at = no + 1;
        let words: Vec<&str> = line.split_whitespace().collect();

        if words[0] == "burst" {
            cur.take()
                .map(Draft::finish)
                .transpose()?
                .into_iter()
                .for_each(|b| out.push(b));
            cur = Some(Draft::new(words.get(1).unwrap_or(&"?").to_string()));
            continue;
        }
        let draft = cur
            .as_mut()
            .ok_or_else(|| format!("line {at}: `{line}` before any `burst`"))?;
        draft.line(&words, at)?;
    }
    cur.take()
        .map(Draft::finish)
        .transpose()?
        .into_iter()
        .for_each(|b| out.push(b));
    Ok(out)
}

/// Which part of a burst the parser is currently reading.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Section {
    Program,
    Fields,
    CompanionProgram,
    CompanionFields,
}

struct Draft {
    name: String,
    program: Program,
    names: HashMap<String, Val>,
    section: Section,
    count: (f64, i32),
    pool: Pool,
    fields: Vec<(Field, Src)>,
    companion: Option<(f64, Pool, Vec<(Field, Src)>)>,
    gates: Vec<GateMark>,
}

impl Draft {
    fn new(name: String) -> Self {
        Self {
            name,
            program: Program::new(),
            names: HashMap::new(),
            section: Section::Program,
            count: (0.0, 1),
            pool: Pool::Lit,
            fields: Vec::new(),
            companion: None,
            gates: Vec::new(),
        }
    }

    fn line(&mut self, words: &[&str], at: usize) -> Result<(), String> {
        match words[0] {
            "count" => {
                self.count = (
                    num(words.get(1), at)?,
                    num(words.get(2), at)? as i32,
                );
                Ok(())
            }
            "pool" => {
                self.pool = pool(words.get(1), at)?;
                Ok(())
            }
            "fields" => {
                self.section = match self.section {
                    Section::CompanionProgram => Section::CompanionFields,
                    _ => Section::Fields,
                };
                Ok(())
            }
            "companion" => {
                let threshold = num(words.get(1), at)?;
                let pool = pool(words.get(2), at)?;
                self.program.companion_from(threshold);
                self.companion = Some((threshold, pool, Vec::new()));
                self.section = Section::CompanionProgram;
                Ok(())
            }
            "gate" => {
                // `gate <probe> < <threshold>`; the `<` is there so the sense
                // of the comparison is written down rather than remembered.
                let probe = self.scalar(words.get(1), at)?;
                let threshold = num(words.get(3), at)?;
                let mark = self.program.open_gate(probe, threshold);
                self.gates.push(mark);
                Ok(())
            }
            "end" => {
                let mark = self
                    .gates
                    .pop()
                    .ok_or_else(|| format!("line {at}: `end` with no open `gate`"))?;
                self.program.close_gate(mark);
                Ok(())
            }
            _ => self.statement(words, at),
        }
    }

    /// Either `name = op args…` in a program section, or `field value` in a
    /// fields section.
    fn statement(&mut self, words: &[&str], at: usize) -> Result<(), String> {
        let in_fields = matches!(self.section, Section::Fields | Section::CompanionFields);
        match in_fields {
            true => {
                let field = field(words[0], at)?;
                let src = self.src(words.get(1), at)?;
                match self.section {
                    Section::CompanionFields => self
                        .companion
                        .as_mut()
                        .map(|c| c.2.push((field, src)))
                        .ok_or_else(|| format!("line {at}: companion fields with no companion"))?,
                    _ => self.fields.push((field, src)),
                }
                Ok(())
            }
            false => {
                let name = words[0].to_string();
                (words.get(1) == Some(&"="))
                    .then_some(())
                    .ok_or_else(|| format!("line {at}: expected `{name} = <op> …`"))?;
                let val = self.op(&words[2..], at)?;
                self.names.insert(name, val);
                Ok(())
            }
        }
    }

    fn op(&mut self, words: &[&str], at: usize) -> Result<Val, String> {
        let b = &mut self.program;
        let arg = |i: usize| words.get(i);
        match *words.first().ok_or_else(|| format!("line {at}: empty operation"))? {
            "point" => Ok(Val::Vector(b.point())),
            "normal" => Ok(Val::Vector(b.normal())),
            "reflected" => Ok(Val::Vector(b.reflected())),
            "incident" => Ok(Val::Vector(b.incident())),
            "sun" => Ok(Val::Vector(b.sun())),
            "index" => Ok(Val::Scalar(b.read(crate::fx::burst::Input::Index))),
            "particles" => Ok(Val::Scalar(b.read(crate::fx::burst::Input::Count))),
            "energy" => Ok(Val::Scalar(b.read(crate::fx::burst::Input::Energy))),
            "unit" => Ok(Val::Scalar(b.unit())),
            "signed" => Ok(Val::Scalar(b.signed())),
            "const" => Ok(Val::Scalar(b.push_const(num(arg(1), at)?))),
            "range" => Ok(Val::Scalar(b.range(num(arg(1), at)?, num(arg(2), at)?))),
            "cone" => {
                let dir = self.vector(arg(1), at)?;
                Ok(Val::Vector(self.program.cone(
                    dir,
                    num(arg(2), at)?,
                    num(arg(3), at)?,
                )))
            }
            "disc_on" => {
                let n = self.vector(arg(1), at)?;
                Ok(Val::Vector(self.program.disc_on(n, num(arg(2), at)?)))
            }
            "blackbody" => {
                let k = self.scalar(arg(1), at)?;
                Ok(Val::Vector(self.program.blackbody(k)))
            }
            "toward_hemi" => {
                let d = self.vector(arg(1), at)?;
                let a = self.vector(arg(2), at)?;
                Ok(Val::Vector(self.program.toward_hemi(d, a, num(arg(3), at)?)))
            }
            "clamp_cone" => {
                let d = self.vector(arg(1), at)?;
                let a = self.vector(arg(2), at)?;
                Ok(Val::Vector(self.program.clamp_cone(d, a, num(arg(3), at)?)))
            }
            "mul" => self.binary(words, at, Program::mul),
            "add" => self.binary(words, at, Program::add),
            "sub" => self.binary(words, at, Program::sub),
            "scale" => self.scaled(words, at, Program::scale),
            "offset" => self.scaled(words, at, Program::offset),
            "mod" => self.scaled(words, at, Program::modulo),
            "max" => self.scaled(words, at, Program::max),
            "mad" => {
                let a = self.scalar(arg(1), at)?;
                let k = num(arg(2), at)?;
                let c = self.scalar(arg(3), at)?;
                Ok(Val::Scalar(self.program.mad(a, k, c)))
            }
            "dot" => {
                let a = self.vector(arg(1), at)?;
                let c = self.vector(arg(2), at)?;
                Ok(Val::Scalar(self.program.dot(a, c)))
            }
            "range_between" => {
                let lo = self.scalar(arg(1), at)?;
                let hi = self.scalar(arg(2), at)?;
                Ok(Val::Scalar(self.program.range_between(lo, hi)))
            }
            "select" => {
                let probe = self.scalar(arg(1), at)?;
                let threshold = num(arg(2), at)?;
                let low = self.src(arg(3), at)?;
                let high = self.src(arg(4), at)?;
                Ok(Val::Scalar(self.program.pick(probe, threshold, low, high)))
            }
            "mul3" => {
                let v = self.vector(arg(1), at)?;
                let s = self.scalar(arg(2), at)?;
                Ok(Val::Vector(self.program.mul3(v, s)))
            }
            "scale3" => {
                let v = self.vector(arg(1), at)?;
                Ok(Val::Vector(self.program.scale3(v, num(arg(2), at)?)))
            }
            "add3" => {
                let a = self.vector(arg(1), at)?;
                let c = self.vector(arg(2), at)?;
                Ok(Val::Vector(self.program.add3(a, c)))
            }
            "mad3" => {
                let a = self.vector(arg(1), at)?;
                let k = num(arg(2), at)?;
                let c = self.vector(arg(3), at)?;
                Ok(Val::Vector(self.program.mad3(a, k, c)))
            }
            other => Err(format!("line {at}: unknown operation `{other}`")),
        }
    }

    fn binary(
        &mut self,
        words: &[&str],
        at: usize,
        f: fn(&mut Program, Reg, Reg) -> Reg,
    ) -> Result<Val, String> {
        let a = self.scalar(words.get(1), at)?;
        let b = self.scalar(words.get(2), at)?;
        Ok(Val::Scalar(f(&mut self.program, a, b)))
    }

    fn scaled(
        &mut self,
        words: &[&str],
        at: usize,
        f: fn(&mut Program, Reg, f64) -> Reg,
    ) -> Result<Val, String> {
        let a = self.scalar(words.get(1), at)?;
        let k = num(words.get(2), at)?;
        Ok(Val::Scalar(f(&mut self.program, a, k)))
    }

    /// Resolve a token to one register: a scalar name, or a `v.x` component.
    fn scalar(&self, word: Option<&&str>, at: usize) -> Result<Reg, String> {
        let w = *word.ok_or_else(|| format!("line {at}: missing operand"))?;
        let (base, part) = w.split_once('.').map_or((w, None), |(b, c)| (b, Some(c)));
        let val = self
            .names
            .get(base)
            .ok_or_else(|| format!("line {at}: `{base}` is not defined"))?;
        match (val, part) {
            (Val::Scalar(r), None) => Ok(*r),
            (Val::Vector(v), Some("x")) => Ok(v.0),
            (Val::Vector(v), Some("y")) => Ok(v.1),
            (Val::Vector(v), Some("z")) => Ok(v.2),
            _ => Err(format!("line {at}: `{w}` is not a single value")),
        }
    }

    fn vector(&self, word: Option<&&str>, at: usize) -> Result<V3, String> {
        let w = *word.ok_or_else(|| format!("line {at}: missing operand"))?;
        match self.names.get(w) {
            Some(Val::Vector(v)) => Ok(*v),
            _ => Err(format!("line {at}: `{w}` is not a vector")),
        }
    }

    /// A field value or a select arm: a number, a named tile, or a register.
    fn src(&self, word: Option<&&str>, at: usize) -> Result<Src, String> {
        let w = *word.ok_or_else(|| format!("line {at}: missing value"))?;
        match literal(w) {
            Some(v) => Ok(Src::Imm(v)),
            None => self.scalar(Some(&w), at).map(Reg::src),
        }
    }

    fn finish(mut self) -> Result<Burst, String> {
        self.gates
            .is_empty()
            .then_some(())
            .ok_or_else(|| format!("burst `{}`: a gate was never closed", self.name))?;
        let companion = self.companion.take().map(|(_, pool, fields)| (pool, fields));
        Ok(self
            .program
            .emit_with(self.count, self.pool, self.fields, companion))
    }
}

/// A number, or one of the atlas tile names.
///
/// Tiles are spelt by name so a recipe stays greppable — a `9` in a file is a
/// number nobody can search for, and `SPLINTER` leads straight back to the
/// atlas that defines it.
fn literal(word: &str) -> Option<f64> {
    word.parse::<f64>().ok().or_else(|| match word {
        "TAU" => Some(std::f64::consts::PI * 2.0),
        "SPARK" => Some(p::SPARK as f64),
        "STREAK" => Some(p::STREAK as f64),
        "DUST" => Some(p::DUST as f64),
        "SMOKE_A" => Some(p::SMOKE_A as f64),
        "SMOKE_B" => Some(p::SMOKE_B as f64),
        "MIST" => Some(p::MIST as f64),
        "CHIP" => Some(p::CHIP as f64),
        "SPLINTER" => Some(p::SPLINTER as f64),
        "DROPLET" => Some(p::DROPLET as f64),
        "SPLASH" => Some(p::SPLASH as f64),
        _ => None,
    })
}

fn num(word: Option<&&str>, at: usize) -> Result<f64, String> {
    let w = *word.ok_or_else(|| format!("line {at}: missing number"))?;
    literal(w).ok_or_else(|| format!("line {at}: `{w}` is not a number"))
}

fn pool(word: Option<&&str>, at: usize) -> Result<Pool, String> {
    match word.copied() {
        Some("lit") => Ok(Pool::Lit),
        Some("add") => Ok(Pool::Additive),
        other => Err(format!("line {at}: pool is `lit` or `add`, not `{other:?}`")),
    }
}

fn field(word: &str, at: usize) -> Result<Field, String> {
    match word {
        "x" => Ok(Field::X),
        "y" => Ok(Field::Y),
        "z" => Ok(Field::Z),
        "vx" => Ok(Field::Vx),
        "vy" => Ok(Field::Vy),
        "vz" => Ok(Field::Vz),
        "size0" => Ok(Field::Size0),
        "size1" => Ok(Field::Size1),
        "size_curve" => Ok(Field::SizeCurve),
        "life" => Ok(Field::Life),
        "delay" => Ok(Field::Delay),
        "drag" => Ok(Field::Drag),
        "gravity" => Ok(Field::Gravity),
        "rot" => Ok(Field::Rot),
        "spin" => Ok(Field::Spin),
        "stretch" => Ok(Field::Stretch),
        "r0" => Ok(Field::R0),
        "g0" => Ok(Field::G0),
        "b0" => Ok(Field::B0),
        "i0" => Ok(Field::I0),
        "r1" => Ok(Field::R1),
        "g1" => Ok(Field::G1),
        "b1" => Ok(Field::B1),
        "i1" => Ok(Field::I1),
        "tile" => Ok(Field::Tile),
        "soft" => Ok(Field::Soft),
        "alpha" => Ok(Field::Alpha),
        "alpha_curve" => Ok(Field::AlphaCurve),
        "turb" => Ok(Field::Turb),
        "turb_freq" => Ok(Field::TurbFreq),
        "seed" => Ok(Field::Seed),
        "flags" => Ok(Field::Flags),
        other => Err(format!("line {at}: unknown field `{other}`")),
    }
}
