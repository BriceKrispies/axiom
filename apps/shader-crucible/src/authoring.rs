//! The small graph-authoring vocabulary every station writes against.
//!
//! [`axiom_field::FieldBuilder`] is deliberately raw: `push(op, params, inputs)`
//! and nothing else, because the layer that owns the algebra must not grow a
//! convenience surface per consumer. What an *app* wants is to read
//! `mul(b, a, c)` rather than `b.push(FieldOp::Mul, Vec::new(), vec![a, c])`
//! nine hundred times, so the sugar lives here, at the app tier, where it costs
//! the engine nothing.
//!
//! Every function here is a one-line spelling of one operator. **Nothing here
//! computes anything** — there is no shading maths in this file and none
//! anywhere else in this app. That is the point of the whole system: a visual
//! effect is an authored graph, never a new Rust function.
//!
//! The builder is threaded by value (`(builder, node)` in, `(builder, node)`
//! out) because `FieldBuilder` is an append-only immutable value; that is the
//! same idiom `apps/burnt-rubber/src/render/asphalt_field.rs` uses.

use axiom_field::{FieldBuilder, FieldOp, FieldType, FieldValue, NodeId, Param, Scalar};
use axiom_math::{Vec3, Vec4};

/// A scalar `Const` node.
pub fn konst(builder: FieldBuilder, value: f32) -> (FieldBuilder, NodeId) {
    builder.push_const(FieldValue::scalar(Scalar::new(value)))
}

/// A `Vec3` `Const` node.
pub fn konst3(builder: FieldBuilder, x: f32, y: f32, z: f32) -> (FieldBuilder, NodeId) {
    builder.push_const(FieldValue::vec3(Vec3::new(x, y, z)))
}

/// A `Vec4` `Const` node.
pub fn konst4(builder: FieldBuilder, x: f32, y: f32, z: f32, w: f32) -> (FieldBuilder, NodeId) {
    builder.push_const(FieldValue::vec4(Vec4::new(x, y, z, w)))
}

/// The evaluation context's object-space point.
pub fn point(builder: FieldBuilder) -> (FieldBuilder, NodeId) {
    builder.push(FieldOp::Point, Vec::new(), Vec::new())
}

/// The evaluation context's texture coordinate.
pub fn uv(builder: FieldBuilder) -> (FieldBuilder, NodeId) {
    builder.push(FieldOp::Uv, Vec::new(), Vec::new())
}

/// The evaluation context's surface normal.
pub fn normal(builder: FieldBuilder) -> (FieldBuilder, NodeId) {
    builder.push(FieldOp::Normal, Vec::new(), Vec::new())
}

/// The evaluation context's engine time. **Never a wall clock** — the frame
/// supplies it, so a replayed tick replays the same appearance.
pub fn time(builder: FieldBuilder) -> (FieldBuilder, NodeId) {
    builder.push(FieldOp::Time, Vec::new(), Vec::new())
}

/// Lane `lane` of `input`, as a `Scalar`.
pub fn component(builder: FieldBuilder, input: NodeId, lane: u32) -> (FieldBuilder, NodeId) {
    builder.push(FieldOp::Component, vec![Param::int(lane)], vec![input])
}

/// A `Vec3` from the first lane of each of three inputs.
pub fn compose3(
    builder: FieldBuilder,
    x: NodeId,
    y: NodeId,
    z: NodeId,
) -> (FieldBuilder, NodeId) {
    builder.push(FieldOp::Compose, vec![Param::int(3)], vec![x, y, z])
}

/// A `Vec4` from the first lane of each of four inputs.
pub fn compose4(
    builder: FieldBuilder,
    x: NodeId,
    y: NodeId,
    z: NodeId,
    w: NodeId,
) -> (FieldBuilder, NodeId) {
    builder.push(FieldOp::Compose, vec![Param::int(4)], vec![x, y, z, w])
}

/// A binary operator over two nodes, with no parameter words.
fn binary(
    builder: FieldBuilder,
    op: FieldOp,
    a: NodeId,
    b: NodeId,
) -> (FieldBuilder, NodeId) {
    builder.push(op, Vec::new(), vec![a, b])
}

/// A unary operator over one node, with no parameter words.
fn unary(builder: FieldBuilder, op: FieldOp, a: NodeId) -> (FieldBuilder, NodeId) {
    builder.push(op, Vec::new(), vec![a])
}

/// `a + b`, component-wise, a scalar broadcasting.
pub fn add(builder: FieldBuilder, a: NodeId, b: NodeId) -> (FieldBuilder, NodeId) {
    binary(builder, FieldOp::Add, a, b)
}

/// `a - b`.
pub fn sub(builder: FieldBuilder, a: NodeId, b: NodeId) -> (FieldBuilder, NodeId) {
    binary(builder, FieldOp::Sub, a, b)
}

/// `a * b`.
pub fn mul(builder: FieldBuilder, a: NodeId, b: NodeId) -> (FieldBuilder, NodeId) {
    binary(builder, FieldOp::Mul, a, b)
}

/// `min(a, b)`.
pub fn min(builder: FieldBuilder, a: NodeId, b: NodeId) -> (FieldBuilder, NodeId) {
    binary(builder, FieldOp::Min, a, b)
}

/// `max(a, b)`.
pub fn max(builder: FieldBuilder, a: NodeId, b: NodeId) -> (FieldBuilder, NodeId) {
    binary(builder, FieldOp::Max, a, b)
}

/// `|a|`.
pub fn abs(builder: FieldBuilder, a: NodeId) -> (FieldBuilder, NodeId) {
    unary(builder, FieldOp::Abs, a)
}

/// `sin(a)`, radians.
pub fn sin(builder: FieldBuilder, a: NodeId) -> (FieldBuilder, NodeId) {
    unary(builder, FieldOp::Sin, a)
}

/// `cos(a)`, radians.
pub fn cos(builder: FieldBuilder, a: NodeId) -> (FieldBuilder, NodeId) {
    unary(builder, FieldOp::Cos, a)
}

/// `exp(a)`.
pub fn exp(builder: FieldBuilder, a: NodeId) -> (FieldBuilder, NodeId) {
    unary(builder, FieldOp::Exp, a)
}

/// `length(a)`.
pub fn length(builder: FieldBuilder, a: NodeId) -> (FieldBuilder, NodeId) {
    unary(builder, FieldOp::Length, a)
}

/// `pow(a, b)` — `powf` where `a > 0`, and **exactly `0.0` for every base at or
/// below zero**. That rule is why a square is [`mul`]`(x, x)` here and never
/// `pow(x, 2)`: the latter is zero across the whole negative half.
pub fn pow(builder: FieldBuilder, a: NodeId, b: NodeId) -> (FieldBuilder, NodeId) {
    binary(builder, FieldOp::Pow, a, b)
}

/// `a + (b - a) * t`, component-wise, `t` unclamped. The spelling is exact: the
/// algebraically equal `a*(1-t) + b*t` differs in the last `f32` bit and would
/// break the CPU/GPU parity contract the backends are written against.
pub fn mix(builder: FieldBuilder, a: NodeId, b: NodeId, t: NodeId) -> (FieldBuilder, NodeId) {
    builder.push(FieldOp::Mix, Vec::new(), vec![a, b, t])
}

/// `clamp(x, lo, hi)` — `max(min(x, hi), lo)`, so an inverted range yields `lo`.
pub fn clamp(
    builder: FieldBuilder,
    x: NodeId,
    lo: NodeId,
    hi: NodeId,
) -> (FieldBuilder, NodeId) {
    builder.push(FieldOp::Clamp, Vec::new(), vec![x, lo, hi])
}

/// `smoothstep(e0, e1, x)`; equal edges yield `0`.
pub fn smoothstep(
    builder: FieldBuilder,
    e0: NodeId,
    e1: NodeId,
    x: NodeId,
) -> (FieldBuilder, NodeId) {
    builder.push(FieldOp::Smoothstep, Vec::new(), vec![e0, e1, x])
}

/// `clamp(x, 0, 1)`.
pub fn clamp_unit(builder: FieldBuilder, x: NodeId) -> (FieldBuilder, NodeId) {
    let (builder, lo) = konst(builder, 0.0);
    let (builder, hi) = konst(builder, 1.0);
    clamp(builder, x, lo, hi)
}

/// `smoothstep` against two authored literal edges.
pub fn smoothstep_at(
    builder: FieldBuilder,
    e0: f32,
    e1: f32,
    x: NodeId,
) -> (FieldBuilder, NodeId) {
    let (builder, lo) = konst(builder, e0);
    let (builder, hi) = konst(builder, e1);
    smoothstep(builder, lo, hi, x)
}

/// `x * scale`, against an authored literal.
pub fn scale(builder: FieldBuilder, x: NodeId, factor: f32) -> (FieldBuilder, NodeId) {
    let (builder, k) = konst(builder, factor);
    mul(builder, x, k)
}

/// `x + offset`, against an authored literal.
pub fn offset(builder: FieldBuilder, x: NodeId, amount: f32) -> (FieldBuilder, NodeId) {
    let (builder, k) = konst(builder, amount);
    add(builder, x, k)
}

/// A signed `[-1, 1]` noise-like value remapped onto `[0, 1]`.
pub fn remap01(builder: FieldBuilder, signed: NodeId) -> (FieldBuilder, NodeId) {
    let (builder, half) = konst(builder, 0.5);
    let (builder, scaled) = mul(builder, signed, half);
    add(builder, scaled, half)
}

/// The object-space point with each axis multiplied by its own frequency — the
/// domain warp every pattern in this app is sampled over.
pub fn frequency_point(
    builder: FieldBuilder,
    fx: f32,
    fy: f32,
    fz: f32,
) -> (FieldBuilder, NodeId) {
    let (builder, p) = point(builder);
    let (builder, f) = konst3(builder, fx, fy, fz);
    mul(builder, p, f)
}

/// Declare a named scalar parameter slot holding `value`, and immediately read
/// it as a node.
///
/// **The value is deliberately outside the graph's structural digest.** That is
/// what station 4 exists to demonstrate: retuning any of these is a uniform
/// write, never a shader recompile.
pub fn knob(builder: FieldBuilder, name: &str, value: f32) -> (FieldBuilder, NodeId) {
    let (builder, slot) = builder.declare(name, FieldValue::scalar(Scalar::new(value)));
    builder.push_param(slot, FieldType::Scalar)
}
