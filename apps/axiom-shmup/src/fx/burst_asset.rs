//! Reading a burst recipe from its JSON asset.
//!
//! Not a port of anything. This is the seam that makes an effect *content*: a
//! `.burst.json` file is data anyone can read, diff and change without a
//! compiler, and what it deserialises to is an engine [`RecipeGraph`] plus the
//! table of fields it fills.
//!
//! # Why JSON and not a language
//!
//! An earlier draft of this was a hand-written parser for a bespoke
//! line-oriented syntax. That was a mistake: no schema, no tooling, no
//! validation but its own, and every future reader having to learn a format
//! nothing else in the world can open — all to save a few characters per line.
//! `serde_json` was already a dependency of this crate.
//!
//! The vocabulary is still a vocabulary — `cone`, `range`, `select` mean
//! something specific — but it is a *schema* over a standard encoding rather
//! than a grammar with a tokenizer behind it. The op names map one-to-one onto
//! [`crate::fx::burst::op`], so the asset and the evaluator share one list of
//! names that can be searched for.
//!
//! # The shape of a file
//!
//! ```json
//! [{
//!   "name": "wood.splinters",
//!   "count": { "factor": 11.0, "plus": 4 },
//!   "pool": "lit",
//!   "nodes": [
//!     { "op": "read", "input": "point" },
//!     { "op": "const", "value": 0.01 },
//!     { "op": "cone", "in": [0, 1, 1] }
//!   ],
//!   "fields": {
//!     "x": { "node": 2, "lane": 0 },
//!     "drag": { "imm": 0.8 }
//!   }
//! }]
//! ```
//!
//! A node's inputs are the indices of nodes **before** it, which is what makes
//! node order draw order. A field value is either a node reference or a number.

use std::collections::BTreeMap;

use axiom_recipe::{NodeId, Param, RecipeGraph, RecipeId};
use serde::{Deserialize, Deserializer};

use crate::fx::burst::{input, op, Burst, Count, Emission, Field, Pool, Src};

/// A number read through [`serde_json::Number::as_f64`].
///
/// **Not decoration.** `serde_json` 1.0.150 parses a decimal through `zmij`,
/// whose rounding is off by one ULP on some inputs — this crate already
/// measured it on an audio golden, where `0.20738044381141663` came back as
/// `…665`. With the `arbitrary_precision` feature a `Number` keeps its source
/// text and `as_f64` goes through `str::parse`, which is correctly rounded.
///
/// Every constant in a recipe has to be the exact `f64` the source had, because
/// a burst's constants feed a random draw and the frozen ledger compares the
/// result bit for bit. A one-ULP parse error is not a rounding difference here;
/// it is a different recipe.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Exact(pub f64);

impl<'de> Deserialize<'de> for Exact {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        serde_json::Number::deserialize(d).and_then(|n| {
            n.as_f64()
                .map(Exact)
                .ok_or_else(|| serde::de::Error::custom("not a finite number"))
        })
    }
}

/// One burst, as written in an asset.
#[derive(Debug, Deserialize)]
pub struct BurstAsset {
    /// For diagnostics only — the evaluator never reads it.
    #[serde(default)]
    pub name: String,
    pub count: CountAsset,
    pub pool: String,
    pub nodes: Vec<NodeAsset>,
    /// Ordered so a diff of two recipes lines up field by field.
    pub fields: BTreeMap<String, SrcAsset>,
    #[serde(default)]
    pub companion: Option<CompanionAsset>,
}

#[derive(Debug, Deserialize)]
pub struct CountAsset {
    pub factor: Exact,
    pub plus: i32,
}

/// A second emission from the same spawn, on a draw. Its nodes are its own —
/// it inherits the parent's *record*, not the parent's values.
#[derive(Debug, Deserialize)]
pub struct CompanionAsset {
    pub threshold: Exact,
    pub pool: String,
    pub nodes: Vec<NodeAsset>,
    pub fields: BTreeMap<String, SrcAsset>,
}

#[derive(Debug, Deserialize)]
pub struct NodeAsset {
    pub op: String,
    /// The nodes this one reads, by index. Must all be earlier.
    #[serde(default, rename = "in")]
    pub inputs: Vec<u32>,
    /// `const` only.
    #[serde(default)]
    pub value: Option<Exact>,
    /// `read` only.
    #[serde(default)]
    pub input: Option<String>,
    /// `lane` only.
    #[serde(default)]
    pub lane: Option<u32>,
    /// `gate` only: how many following nodes it covers.
    #[serde(default)]
    pub len: Option<u32>,
}

/// A field value: a node reference, or a literal.
///
/// Two named keys rather than an untagged enum, because serde's untagged
/// deserialization buffers the input through an intermediate that does not
/// preserve a number's source text — which would put the one-ULP parse back
/// exactly where [`Exact`] removed it.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SrcAsset {
    /// A literal value.
    #[serde(default)]
    pub imm: Option<Exact>,
    /// The node whose value this field takes.
    #[serde(default)]
    pub node: Option<u32>,
    /// Which component of that node, for a vector one.
    #[serde(default)]
    pub lane: u8,
}

/// Parse every burst in an asset.
pub fn parse(name: &str, src: &str) -> Result<Vec<Burst>, String> {
    let assets: Vec<BurstAsset> =
        serde_json::from_str(src).map_err(|e| format!("{name}: {e}"))?;
    assets.into_iter().map(build).collect()
}

fn build(asset: BurstAsset) -> Result<Burst, String> {
    let label = asset.name.clone();
    let main = emission(&label, &asset.pool, asset.nodes, &asset.fields)?;
    let companion = asset
        .companion
        .map(|c| {
            emission(&label, &c.pool, c.nodes, &c.fields).map(|e| (c.threshold.0, e))
        })
        .transpose()?;
    Ok(Burst {
        count: Count {
            factor: asset.count.factor.0,
            plus: asset.count.plus,
        },
        main,
        companion,
    })
}

fn emission(
    label: &str,
    pool_name: &str,
    nodes: Vec<NodeAsset>,
    fields: &BTreeMap<String, SrcAsset>,
) -> Result<Emission, String> {
    let mut program = RecipeGraph::new(RecipeId::from_raw(1), 1);
    nodes.into_iter().enumerate().try_for_each(|(index, n)| {
        let (code, params) = operator(label, index, &n)?;
        let inputs = n.inputs.iter().map(|i| NodeId::from_raw(*i)).collect();
        program.add(code, params, inputs);
        Ok::<(), String>(())
    })?;
    program
        .validate()
        .map_err(|e| format!("{label}: invalid graph ({:?})", e.kind()))?;

    let table = fields
        .iter()
        .map(|(name, src)| {
            field(label, name).and_then(|f| source(label, name, src).map(|s| (f, s)))
        })
        .collect::<Result<Vec<_>, String>>()?;

    Ok(Emission {
        program,
        pool: pool(label, pool_name)?,
        fields: table,
    })
}

fn source(label: &str, name: &str, src: &SrcAsset) -> Result<Src, String> {
    match (src.imm, src.node) {
        (Some(Exact(v)), None) => Ok(Src::Imm(v)),
        (None, Some(node)) => Ok(Src::Node {
            node,
            lane: src.lane,
        }),
        _ => Err(format!(
            "{label}: field `{name}` needs exactly one of `imm` or `node`"
        )),
    }
}

/// Map an operator name to its code and parameter words.
///
/// The f64-carrying parameters go through [`Param::pair`], the engine's named
/// widening boundary: a recipe word is 32 bits and 70% of this corpus's
/// constants are not representable in `f32`.
fn operator(label: &str, index: usize, n: &NodeAsset) -> Result<(u16, Vec<Param>), String> {
    let at = || format!("{label}: node {index} (`{}`)", n.op);
    match n.op.as_str() {
        "const" => n
            .value
            .map(|v| (op::CONST, Param::pair(v.0).to_vec()))
            .ok_or_else(|| format!("{}: needs a `value`", at())),
        "read" => n
            .input
            .as_deref()
            .and_then(input_id)
            .map(|id| (op::READ, vec![Param::int(id)]))
            .ok_or_else(|| format!("{}: needs a known `input`", at())),
        "lane" => n
            .lane
            .map(|l| (op::LANE, vec![Param::int(l)]))
            .ok_or_else(|| format!("{}: needs a `lane`", at())),
        "gate" => n
            .len
            .map(|l| (op::GATE, vec![Param::int(l)]))
            .ok_or_else(|| format!("{}: needs a `len`", at())),
        "range" => Ok((op::RANGE, Vec::new())),
        "unit" => Ok((op::UNIT, Vec::new())),
        "signed" => Ok((op::SIGNED, Vec::new())),
        "cone" => Ok((op::CONE, Vec::new())),
        "disc_on" => Ok((op::DISC_ON, Vec::new())),
        "blackbody" => Ok((op::BLACKBODY, Vec::new())),
        "toward_hemi" => Ok((op::TOWARD_HEMI, Vec::new())),
        "clamp_cone" => Ok((op::CLAMP_CONE, Vec::new())),
        "mul" => Ok((op::MUL, Vec::new())),
        "add" => Ok((op::ADD, Vec::new())),
        "sub" => Ok((op::SUB, Vec::new())),
        "div" => Ok((op::DIV, Vec::new())),
        "mod" => Ok((op::MODULO, Vec::new())),
        "max" => Ok((op::MAX, Vec::new())),
        "cos" => Ok((op::COS, Vec::new())),
        "sin" => Ok((op::SIN, Vec::new())),
        "dot" => Ok((op::DOT, Vec::new())),
        "select" => Ok((op::SELECT_LT, Vec::new())),
        "vec3" => Ok((op::VEC3, Vec::new())),
        other => Err(format!("{label}: node {index}: unknown op `{other}`")),
    }
}

fn input_id(name: &str) -> Option<u32> {
    match name {
        "point" => Some(input::POINT),
        "normal" => Some(input::NORMAL),
        "reflected" => Some(input::REFLECTED),
        "incident" => Some(input::INCIDENT),
        "sun" => Some(input::SUN),
        "up" => Some(input::UP),
        "energy" => Some(input::ENERGY),
        "radius" => Some(input::RADIUS),
        "index" => Some(input::INDEX),
        "count" => Some(input::COUNT),
        _ => None,
    }
}

fn pool(label: &str, name: &str) -> Result<Pool, String> {
    match name {
        "lit" => Ok(Pool::Lit),
        "add" => Ok(Pool::Additive),
        other => Err(format!("{label}: pool is `lit` or `add`, not `{other}`")),
    }
}

fn field(label: &str, name: &str) -> Result<Field, String> {
    match name {
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
        other => Err(format!("{label}: unknown field `{other}`")),
    }
}
