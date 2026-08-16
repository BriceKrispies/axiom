//! **CPU↔GPU parity for the vertex stage**, and the library graphs that prove
//! deformation is authored rather than built in.
//!
//! [`crate::surface_program::parity`] holds the fragment stage to
//! `axiom-field`'s evaluator, operator by operator, on a real GPU. This is the
//! same proof for the one **vertex-stage** channel: a displacement graph
//! evaluated at a set of sampled vertex positions, compared against the
//! generated `axiom_displace` running on the device at the same contexts, at the
//! same `1e-4` absolute tolerance and through the same harness — so what is
//! compared is the function the frame actually runs, not a restatement of it.
//!
//! ## Wind, ripple, bend and squash are **library graphs**
//!
//! None of them is an engine feature, and none of them needed a Rust operator.
//! Each is written below out of the closed algebra alone and driven through both
//! sides, which is the whole claim of this work stated as a test: *the engine
//! never needs a new function for a new effect*.
//!
//! **The honest gap.** A true twist — rotating each slice of a body about an
//! axis by an angle proportional to its height — needs `sin`/`cos`, and the
//! algebra deliberately excludes transcendentals because their CPU/GPU agreement
//! is not a budget this parity tolerance can carry. A twist is therefore
//! expressed as a `Transform` whose `Mat4` the app recomputes per frame from its
//! own simulation and uploads as a **parameter** — a uniform write, never a
//! recompile, because a parameter retune does not move a surface's digest. That
//! is demonstrated here too, so the workaround is a tested shape rather than a
//! sentence in a document.
//!
//! ## This runs only with a real GPU
//!
//! Same rule as the fragment parity: compiled only under `--features offscreen`,
//! and it **asserts** an adapter was acquired rather than skipping.

use axiom_field::{EvalContext, FieldGraph, FieldOp, FieldType, FieldValue};
use axiom_kernel::Seconds;
use axiom_math::{Vec2, Vec3, Vec4};
use axiom_recipe::{NodeId, Param, Scalar};
use axiom_surface::{Surface, SurfaceBuilder, SurfaceChannel};

use crate::surface_program::emit_vertex::displace_function;
use crate::surface_program::params::{pack, ParamLayout};
use crate::surface_program::parity::{
    assert_parity, builder, context_bytes, contexts, ParityGpu, PARITY_HARNESS_WGSL, SAMPLES,
    TOLERANCE,
};
use crate::surface_program::wgsl_template::{
    scene_shader, DEFAULT_DISPLACE_WGSL, DEFAULT_SURFACE_WGSL, SURFACE_PRELUDE_WGSL,
};

/// The surface one displacement graph becomes: the graph on `Displacement`,
/// nothing else bound.
fn surface_of(graph: &FieldGraph) -> Surface {
    SurfaceBuilder::new()
        .field(SurfaceChannel::Displacement, graph.clone())
        .build()
        .expect("every case here is a legal vec3 displacement")
}

/// Run one displacement graph on both sides and return `(cpu, gpu)` lane sets.
///
/// The fourth lane is zero on both sides: a displacement is a `Vec3`, and the
/// harness pads it so one comparison serves both stages.
fn compare(
    gpu: &ParityGpu,
    name: &str,
    graph: &FieldGraph,
    at: &[EvalContext],
) -> (Vec<[f32; 4]>, Vec<[f32; 4]>) {
    let surface = surface_of(graph);
    let flat = surface.flatten().expect("a flat surface flattens to itself");
    let program = displace_function(&surface).expect("every case emits");
    let module = gpu
        .compile(
            &[
                SURFACE_PRELUDE_WGSL,
                &program,
                DEFAULT_SURFACE_WGSL,
                PARITY_HARNESS_WGSL,
            ]
            .concat(),
            surface.digest().raw(),
            SurfaceChannel::Displacement.bit(),
        )
        .unwrap_or_else(|error| panic!("{name} must emit compiling WGSL: {error}"));
    let params = pack(ParamLayout::of(surface.requirements().param_count()), &flat);
    let rendered = gpu.render(
        &module,
        "parity_displace_fs",
        &context_bytes(at),
        &params,
        &vec![0_u8; 48 * 16],
    );
    let evaluated = at
        .iter()
        .map(|context| {
            let lanes = flat
                .binding(SurfaceChannel::Displacement)
                .as_graph()
                .evaluate(context)
                .unwrap_or_else(|error| panic!("{name} must evaluate on the CPU: {error:?}"))
                .as_vec4();
            [lanes.x, lanes.y, lanes.z, 0.0]
        })
        .collect();
    (evaluated, rendered)
}

/// The lattice-noise-driven **wind** graph, written out of the closed algebra
/// and nothing else:
///
/// `direction * (fbm(point + time * speed) * strength * heightMask)`
///
/// The mask is a `Smoothstep` on the point's y lane, so a trunk stays planted
/// while a canopy moves — the whole reason wind reads as wind rather than as the
/// object sliding. Every knob (direction, speed, strength) is a declared
/// parameter, so retuning the wind is a uniform write and not a recompile.
fn wind() -> FieldGraph {
    let (b, direction) = builder("v/wind").declare(
        "direction",
        FieldValue::vec3(Vec3::new(1.0, 0.0, 0.35)),
    );
    let (b, speed) = b.declare("speed", FieldValue::scalar(Scalar::new(0.6)));
    let (b, strength) = b.declare("strength", FieldValue::scalar(Scalar::new(0.4)));
    let (b, point) = b.push(FieldOp::Point, Vec::new(), Vec::new());
    let (b, clock) = b.push(FieldOp::Time, Vec::new(), Vec::new());
    let (b, speed_node) = b.push_param(speed, FieldType::Scalar);
    let (b, drift) = b.push(FieldOp::Mul, Vec::new(), vec![clock, speed_node]);
    let (b, sampled_at) = b.push(FieldOp::Add, Vec::new(), vec![point, drift]);
    let (b, gust) = b.push(
        FieldOp::Fbm,
        vec![
            Param::from_bits(0x00C0_FFEE),
            Param::from_bits(0x0000_0000),
            Param::int(3),
            Param::from_bits(0.9_f32.to_bits()),
            Param::from_bits(2.0_f32.to_bits()),
            Param::from_bits(0.5_f32.to_bits()),
        ],
        vec![sampled_at],
    );
    // The height mask: nothing below y = 0 moves, everything above y = 2 moves
    // fully, and the quintic ramp between is what keeps the canopy attached.
    let (b, low) = b.push_const(FieldValue::scalar(Scalar::new(0.0)));
    let (b, high) = b.push_const(FieldValue::scalar(Scalar::new(2.0)));
    let (b, height) = b.push(FieldOp::Component, vec![Param::int(1)], vec![point]);
    let (b, mask) = b.push(FieldOp::Smoothstep, Vec::new(), vec![low, high, height]);
    let (b, strength_node) = b.push_param(strength, FieldType::Scalar);
    let (b, amount) = b.push(FieldOp::Mul, Vec::new(), vec![gust, strength_node]);
    let (b, masked) = b.push(FieldOp::Mul, Vec::new(), vec![amount, mask]);
    let (b, dir_node) = b.push_param(direction, FieldType::Vec3);
    let (b, offset) = b.push(FieldOp::Mul, Vec::new(), vec![dir_node, masked]);
    b.build(offset)
}

/// The **ripple** graph: a radial falloff around a centre, pushing along the
/// surface normal.
///
/// `normal * (smoothstep(outer, inner, |point - centre|) * amplitude)`
///
/// No trigonometry — the edges are ordered outer-then-inner, so the smoothstep
/// runs *backwards* and the crest sits at the centre. That is the whole trick,
/// and it is why a ripple needs no `Sin`.
fn ripple() -> FieldGraph {
    let (b, centre) =
        builder("v/ripple").declare("centre", FieldValue::vec3(Vec3::new(0.0, 0.5, 0.0)));
    let (b, amplitude) = b.declare("amplitude", FieldValue::scalar(Scalar::new(0.3)));
    let (b, point) = b.push(FieldOp::Point, Vec::new(), Vec::new());
    let (b, centre_node) = b.push_param(centre, FieldType::Vec3);
    let (b, delta) = b.push(FieldOp::Sub, Vec::new(), vec![point, centre_node]);
    let (b, distance) = b.push(FieldOp::Length, Vec::new(), vec![delta]);
    let (b, outer) = b.push_const(FieldValue::scalar(Scalar::new(3.0)));
    let (b, inner) = b.push_const(FieldValue::scalar(Scalar::new(0.25)));
    let (b, falloff) = b.push(
        FieldOp::Smoothstep,
        Vec::new(),
        vec![outer, inner, distance],
    );
    let (b, amplitude_node) = b.push_param(amplitude, FieldType::Scalar);
    let (b, height) = b.push(FieldOp::Mul, Vec::new(), vec![falloff, amplitude_node]);
    let (b, normal) = b.push(FieldOp::Normal, Vec::new(), Vec::new());
    let (b, offset) = b.push(FieldOp::Mul, Vec::new(), vec![normal, height]);
    b.build(offset)
}

/// The **squash/stretch** graph: a component-wise `Mul` of the point against a
/// `Vec3` parameter, minus the point itself, so the result is an *offset* rather
/// than a position. Three numbers, no operator anyone had to add.
fn squash() -> FieldGraph {
    let (b, axes) = builder("v/squash").declare(
        "axes",
        FieldValue::vec3(Vec3::new(1.2, 0.75, 1.2)),
    );
    let (b, point) = b.push(FieldOp::Point, Vec::new(), Vec::new());
    let (b, axes_node) = b.push_param(axes, FieldType::Vec3);
    let (b, scaled) = b.push(FieldOp::Mul, Vec::new(), vec![point, axes_node]);
    let (b, offset) = b.push(FieldOp::Sub, Vec::new(), vec![scaled, point]);
    b.build(offset)
}

/// The **bend/twist** graph: `Transform(point) - point`, through a matrix whose
/// four columns are declared parameters, masked by the point's height.
///
/// This is the honest answer to the missing `Sin`. The rotation the twist needs
/// is computed by the *app*, per frame, from its own simulation, and uploaded
/// into these four slots. That costs one uniform write; it does not move the
/// surface's digest, so it cannot force a recompile.
fn twist(columns: [Vec4; 4]) -> FieldGraph {
    let (b, slots) = columns.iter().enumerate().fold(
        (builder("v/twist"), Vec::new()),
        |(b, mut slots), (index, column)| {
            let (b, slot) = b.declare(&format!("col{index}"), FieldValue::vec4(*column));
            slots.push(slot);
            (b, slots)
        },
    );
    let (b, point) = b.push(FieldOp::Point, Vec::new(), Vec::new());
    let (b, turned) = b.push(
        FieldOp::Transform,
        slots
            .iter()
            .map(|slot| Param::int(u32::from(slot.raw())))
            .collect(),
        vec![point],
    );
    let (b, offset) = b.push(FieldOp::Sub, Vec::new(), vec![turned, point]);
    let (b, low) = b.push_const(FieldValue::scalar(Scalar::new(0.0)));
    let (b, high) = b.push_const(FieldValue::scalar(Scalar::new(2.0)));
    let (b, height) = b.push(FieldOp::Component, vec![Param::int(1)], vec![point]);
    let (b, mask) = b.push(FieldOp::Smoothstep, Vec::new(), vec![low, high, height]);
    let (b, masked) = b.push(FieldOp::Mul, Vec::new(), vec![offset, mask]);
    b.build(masked)
}

/// `SAMPLES` contexts at one fixed vertex, sampled across `seconds` — the shape
/// a "does it move over time?" question needs, as opposed to `contexts()`, which
/// moves the point as well.
fn at_one_vertex(seconds: f32) -> Vec<EvalContext> {
    (0..SAMPLES)
        .map(|index| {
            EvalContext::new(
                Vec3::new(0.35, 1.5, -0.75),
                Vec2::new(0.5, 0.5),
                Vec3::new(0.0, 1.0, 0.0),
                Seconds::finite_or_zero(seconds + index as f32 * 0.0),
            )
        })
        .collect()
}

/// How far apart two lane sets are at their widest.
fn spread(lanes: &[[f32; 4]]) -> f32 {
    lanes.iter().fold(0.0_f32, |widest, sample| {
        widest.max(sample[0].abs() + sample[1].abs() + sample[2].abs())
    })
}

/// The headline: a displacement graph, sampled at vertex positions, agrees with
/// the reference evaluator on a real GPU at the documented tolerance.
#[test]
fn a_displacement_graph_agrees_with_the_cpu_evaluator_at_every_sampled_vertex() {
    let gpu = ParityGpu::acquire();
    assert_ne!(
        gpu.backend,
        wgpu::Backend::Noop,
        "a vertex-stage parity proof is worthless unless a real backend ran it"
    );
    let at = contexts();
    [
        ("wind", wind()),
        ("ripple", ripple()),
        ("squash", squash()),
        (
            "twist",
            twist([
                Vec4::new(0.0, 0.0, -1.0, 0.0),
                Vec4::new(0.0, 1.0, 0.0, 0.0),
                Vec4::new(1.0, 0.0, 0.0, 0.0),
                Vec4::new(0.0, 0.0, 0.0, 1.0),
            ]),
        ),
    ]
    .iter()
    .for_each(|(name, graph)| {
        let (cpu, rendered) = compare(&gpu, name, graph, &at);
        assert_parity(name, &cpu, &rendered);
        // A displacement that is everywhere zero would pass a tolerance check
        // against a zero CPU side, so prove each graph actually moves something.
        assert!(
            spread(&cpu) > 0.01,
            "{name} must displace something, or the parity is vacuous"
        );
        assert_eq!(TOLERANCE, 1.0e-4);
    });
}

/// **Wind and ripple are authored graphs, not engine features.** Neither needed
/// a Rust operator: every node in both is one of the closed algebra's, and the
/// proof is that they emit, compile and agree with the evaluator above. What
/// this test adds is the *shape* claim — that their knobs are parameters, so
/// retuning a gust is a uniform write rather than a new program.
#[test]
fn wind_and_ripple_are_authored_graphs_whose_knobs_are_parameters() {
    [("wind", wind(), 3_usize), ("ripple", ripple(), 2)]
        .iter()
        .for_each(|(name, graph, knobs)| {
            assert_eq!(
                graph.params().len(),
                *knobs,
                "{name}'s knobs must be declared parameters"
            );
            let surface = surface_of(graph);
            assert!(surface.requirements().has_displacement());
            displace_function(&surface).unwrap_or_else(|error| panic!("{name}: {error}"));
        });
    // The whole surface is one program keyed by one digest: the vertex and
    // fragment halves cannot disagree about which material they belong to.
    let surface = surface_of(&wind());
    assert_eq!(
        crate::surface_program::plan::SurfaceProgramPlan::of(&surface).program_id(),
        surface.digest().raw()
    );
}

/// The determinism claim, on the device: a time-varying displacement is
/// **different** at a different frame time and **byte-identical** when the same
/// frame time is presented twice. That is what makes wind replayable.
#[test]
fn a_time_varying_displacement_moves_with_the_clock_and_replays_identically() {
    let gpu = ParityGpu::acquire();
    assert_ne!(gpu.backend, wgpu::Backend::Noop);
    let graph = wind();
    // One vertex, one tick.
    let tick = at_one_vertex(0.0);
    let (cpu_now, gpu_now) = compare(&gpu, "wind", &graph, &tick);
    // The same tick, presented again: identical, bit for bit.
    let (_cpu_again, gpu_again) = compare(&gpu, "wind", &graph, &tick);
    assert_eq!(gpu_now, gpu_again, "the same tick must replay exactly");
    assert_parity("wind", &cpu_now, &gpu_now);
    // Tick N + 60, at a 60 Hz step — a full second later.
    let later = at_one_vertex(1.0);
    let (cpu_later, gpu_later) = compare(&gpu, "wind", &graph, &later);
    assert_parity("wind", &cpu_later, &gpu_later);
    assert!(
        (cpu_now[0][0] - cpu_later[0][0]).abs() > TOLERANCE,
        "a clock-reading displacement must differ a second later: \
         {:?} vs {:?}",
        cpu_now[0],
        cpu_later[0]
    );
    assert_ne!(gpu_now[0], gpu_later[0]);
}

/// A displacement graph spliced into the **real main pass** compiles — which is
/// what proves the emitted vertex-stage text is valid where `vs` actually calls
/// it, and not merely valid in the parity harness.
#[test]
fn the_main_pass_shader_compiles_with_a_generated_displacement_program_spliced_in() {
    let gpu = ParityGpu::acquire();
    let surface = surface_of(&wind());
    let program = displace_function(&surface).expect("wind emits");
    let source = scene_shader(
        crate::scene_wgsl::SCENE_WGSL_PREFIX,
        &program,
        DEFAULT_SURFACE_WGSL,
        crate::scene_wgsl::SCENE_WGSL_SUFFIX,
    );
    gpu.compile(
        &source,
        surface.digest().raw(),
        SurfaceChannel::Displacement.bit(),
    )
    .expect("the main pass must compile with a generated displacement program");
    // `vs` calls it, with the arguments it already had in hand — no new vertex
    // attribute, the frame's own surface time, and the program's BOUND parameter
    // region (group 3, binding 1) rather than the zero value the pass used to
    // hand every program.
    assert!(source.contains(
        "let displaced = position + axiom_displace(position, normal, uv, lights.camera.w, \
         surface_params);"
    ));
    assert!(source.contains("@group(3) @binding(1) var<uniform> surface_params: SurfaceParams;"));
    assert!(source.contains("out.clip = mvp * vec4<f32>(displaced, 1.0);"));
    // And `vs_skinned` does NOT: the 16-attribute ceiling, stated in the shader.
    let skinned_at = source.find("fn vs_skinned").expect("the skinned stage");
    let shadow_at = source.find("fn shadow_factor").expect("the next function");
    assert!(!source[skinned_at..shadow_at].contains("axiom_displace("));
}

/// **A surface that displaces nothing costs nothing in the vertex stage.** The
/// pass splices the default program, whose body is a literal zero — so the
/// vertex it transforms is the vertex it was handed, and every existing frame is
/// unchanged.
#[test]
fn a_surface_that_displaces_nothing_emits_no_vertex_stage_arithmetic() {
    let plain = SurfaceBuilder::new()
        .constant(
            SurfaceChannel::BaseColor,
            FieldValue::vec4(Vec4::new(0.2, 0.4, 0.6, 1.0)),
        )
        .build()
        .expect("a vec4 constant is a legal base colour");
    assert!(!plain.requirements().has_displacement());
    // The default the pass actually splices for it: a literal zero, and no
    // reference to any context lane at all.
    assert!(DEFAULT_DISPLACE_WGSL.contains("return vec3<f32>(0.0, 0.0, 0.0);"));
    ["in.object_pos", "in.uv", "in.object_normal", "in.time", "params.slots"]
        .iter()
        .for_each(|lane| {
            assert!(
                !DEFAULT_DISPLACE_WGSL.contains(lane),
                "the default vertex program must read {lane} not at all"
            );
        });
    // A generated one for the same surface is the zero constant too — it just
    // never runs, because there is nothing to bind it for.
    let text = displace_function(&plain).expect("flattens");
    assert!(text.contains("let c6_n0 = vec4<f32>(0.0, 0.0, 0.0, 0.0);"));
    // And the node id the graph exists at is the only one: no arithmetic.
    assert!(!text.contains("c6_n1"));
    assert_eq!(NodeId::from_raw(0).raw(), 0);
}
