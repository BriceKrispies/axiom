//! `axiom-field-inspect` — dump a field graph or a surface, **as JSON**.
//!
//! ```text
//! cargo run -p axiom-field-inspect -- --help
//! cargo run -p axiom-field-inspect -- sample
//! cargo run -p axiom-field-inspect -- sample --text
//! cargo run -p axiom-field-inspect -- field   path/to/graph.field
//! cargo run -p axiom-field-inspect -- surface path/to/material.surface
//! ```
//!
//! **JSON is the point.** `tools/axiom-proc-inspect` prints its provenance chain
//! as prose, which makes it readable by a human and unusable to a program — an
//! agent that wants to know a node's type has to parse English. This tool emits a
//! structured document by default: every node's operator, derived type, raw
//! parameter words, inputs and dependents; the graph's structural digest and its
//! deterministic `explain()` lines; and, for a surface, every channel's binding,
//! the derived backend requirements, and the verdict of `supported_by` against
//! each backend capability profile. `--text` opts into the human form.
//!
//! Errors are JSON too (`{"error": {...}}`, exit code 1), carrying the engine's
//! own stable numeric code and the node it names — so a failing run is as
//! machine-readable as a succeeding one.
//!
//! The input is **bytes**, never text: there is no textual authoring format for a
//! field, deliberately, and this tool does not invent one. It reads what
//! `FieldGraph::serialize` / `Surface::serialize` wrote.
//!
//! Repo tooling: outside the engine dependency graph, outside the coverage gate,
//! outside the branchless gate. Nothing in `crates/`, `modules/` or `apps/` may
//! depend on it.

use std::env;
use std::process::ExitCode;

use axiom_field::{
    FieldBuilder, FieldGraph, FieldId, FieldOp, FieldType, FieldValue, Param, Scalar,
};
use axiom_host::{BackendCapabilityProfile, RenderCapability};
use axiom_surface::{supported_by, Surface, SurfaceBuilder, SurfaceChannel};

const USAGE: &str = "\
axiom-field-inspect — dump a field graph or a surface as JSON

USAGE:
    axiom-field-inspect <COMMAND> [--text]

COMMANDS:
    sample                 inspect a built-in sample field graph
    sample-surface         inspect a built-in sample surface
    field <PATH>           inspect the bytes FieldGraph::serialize wrote
    surface <PATH>         inspect the bytes Surface::serialize wrote
    --help, -h             print this message

OPTIONS:
    --text                 print the human-readable explanation instead of JSON

OUTPUT:
    JSON on stdout. Errors are JSON too, on stdout, with exit code 1.
";

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    let text = args.iter().any(|arg| arg == "--text");
    let positional: Vec<&str> = args
        .iter()
        .map(String::as_str)
        .filter(|arg| !arg.starts_with("--"))
        .collect();
    run(&args, &positional, text)
}

fn run(args: &[String], positional: &[&str], text: bool) -> ExitCode {
    if args.is_empty() || args.iter().any(|a| a == "--help" || a == "-h") {
        print!("{USAGE}");
        return ExitCode::SUCCESS;
    }
    match positional.first().copied() {
        Some("sample") => emit_field(Ok(sample_field()), text),
        Some("sample-surface") => emit_surface(Ok(sample_surface()), text),
        Some("field") => match positional.get(1) {
            Some(path) => emit_field(read_field(path), text),
            None => fail("field needs a path"),
        },
        Some("surface") => match positional.get(1) {
            Some(path) => emit_surface(read_surface(path), text),
            None => fail("surface needs a path"),
        },
        Some(other) => fail(&format!("unknown command `{other}`")),
        None => fail("no command given"),
    }
}

fn fail(message: &str) -> ExitCode {
    println!("{{\"error\":{{\"message\":{}}}}}", quote(message));
    ExitCode::FAILURE
}

// ---------------------------------------------------------------- reading

fn read_field(path: &str) -> Result<FieldGraph, String> {
    std::fs::read(path)
        .map_err(|e| format!("cannot read {path}: {e}"))
        .and_then(|bytes| {
            FieldGraph::deserialize(&bytes)
                .map_err(|e| format!("code {} at node {}: {}", e.code(), e.node().raw(), e.message()))
        })
}

fn read_surface(path: &str) -> Result<Surface, String> {
    std::fs::read(path)
        .map_err(|e| format!("cannot read {path}: {e}"))
        .and_then(|bytes| {
            Surface::deserialize(&bytes)
                .map_err(|e| format!("code {} at node {}: {}", e.code(), e.node().raw(), e.message()))
        })
}

// ---------------------------------------------------------------- field out

fn emit_field(graph: Result<FieldGraph, String>, text: bool) -> ExitCode {
    match graph {
        Err(why) => fail(&why),
        Ok(graph) => match graph.describe() {
            Err(e) => fail(&format!(
                "the graph does not type: code {} at node {}: {}",
                e.code(),
                e.node().raw(),
                e.message()
            )),
            Ok(described) => {
                if text {
                    println!("{}", described.explain().text());
                } else {
                    println!("{}", field_json(&graph, &described));
                }
                ExitCode::SUCCESS
            }
        },
    }
}

fn field_json(graph: &FieldGraph, described: &axiom_field::FieldDescription) -> String {
    let nodes: Vec<String> = described
        .nodes()
        .iter()
        .map(|row| node_json(graph, row))
        .collect();
    let params: Vec<String> = graph
        .params()
        .values()
        .iter()
        .enumerate()
        .map(|(slot, value)| {
            format!(
                "{{\"slot\":{slot},\"type\":{},\"lanes\":{}}}",
                quote(&format!("{:?}", value.ty())),
                lanes_json(*value)
            )
        })
        .collect();
    let lines: Vec<String> = described
        .explain()
        .lines()
        .iter()
        .map(|line| quote(line))
        .collect();
    format!(
        "{{\"kind\":\"field\",\"digest\":\"0x{:016x}\",\"node_count\":{},\"output\":{},\
         \"is_canonical\":{},\"param_count\":{},\"params\":[{}],\"nodes\":[{}],\"explain\":[{}]}}",
        described.digest().raw(),
        described.node_count(),
        described.output().raw(),
        graph.is_canonical(),
        graph.params().len(),
        params.join(","),
        nodes.join(","),
        lines.join(",")
    )
}

fn node_json(graph: &FieldGraph, row: &axiom_field::FieldNodeDescription) -> String {
    let words: Vec<String> = graph
        .recipe()
        .node(row.node())
        .map(|node| node.params().iter().map(|p| p.bits().to_string()).collect())
        .unwrap_or_default();
    let inputs: Vec<String> = row.inputs().iter().map(|i| i.raw().to_string()).collect();
    let dependents: Vec<String> = graph
        .dependents_of(row.node())
        .unwrap_or_default()
        .iter()
        .map(|i| i.raw().to_string())
        .collect();
    format!(
        "{{\"id\":{},\"op\":{},\"op_code\":{},\"type\":{},\"type_code\":{},\
         \"params\":[{}],\"inputs\":[{}],\"dependents\":[{}]}}",
        row.node().raw(),
        quote(&format!("{:?}", row.op())),
        row.op().code(),
        quote(&format!("{:?}", row.ty())),
        row.ty().code(),
        words.join(","),
        inputs.join(","),
        dependents.join(",")
    )
}

fn lanes_json(value: FieldValue) -> String {
    let lanes = value.as_vec4();
    let parts: Vec<String> = [lanes.x, lanes.y, lanes.z, lanes.w]
        .iter()
        .map(|lane| number(*lane))
        .collect();
    format!("[{}]", parts.join(","))
}

/// A finite lane, or JSON `null` — NaN and infinity have no JSON spelling, and
/// emitting one would produce a document no parser accepts.
fn number(value: f32) -> String {
    if value.is_finite() {
        format!("{value:?}")
    } else {
        String::from("null")
    }
}

// -------------------------------------------------------------- surface out

fn emit_surface(surface: Result<Surface, String>, text: bool) -> ExitCode {
    match surface {
        Err(why) => fail(&why),
        Ok(surface) => match surface.inspect() {
            Err(e) => fail(&format!(
                "channel {:?}, node {}: code {}: {}",
                e.channel(),
                e.node().raw(),
                e.code(),
                e.message()
            )),
            Ok(read) => {
                if text {
                    println!("{}", surface_text(&read));
                } else {
                    println!("{}", surface_json(&read));
                }
                ExitCode::SUCCESS
            }
        },
    }
}

/// The three profiles worth reporting: a hardware backend that attempts an
/// authored program, the software rasterizer's real profile, and an empty one.
const PROFILES: [&str; 3] = ["gpu", "canvas2d", "none"];

fn profile(name: &str) -> BackendCapabilityProfile {
    match name {
        "gpu" => BackendCapabilityProfile::all().with(RenderCapability::ProceduralSurface),
        "canvas2d" => BackendCapabilityProfile::canvas2d(),
        _ => BackendCapabilityProfile::none(),
    }
}

fn surface_json(read: &axiom_surface::SurfaceInspection) -> String {
    let reqs = read.requirements();
    let channels: Vec<String> = read
        .channels()
        .iter()
        .map(|row| {
            format!(
                "{{\"channel\":{},\"binding\":{},\"type\":{},\"node_count\":{},\
                 \"param_count\":{},\"graph_digest\":{},\"varies\":{}}}",
                quote(&format!("{:?}", row.channel())),
                quote(["field", "constant"][usize::from(row.is_constant())]),
                quote(&format!("{:?}", row.ty())),
                row.node_count(),
                row.param_count(),
                row.graph_digest()
                    .map_or_else(|| String::from("null"), |d| format!("\"0x{:016x}\"", d.raw())),
                reqs.varies(row.channel())
            )
        })
        .collect();
    let verdicts: Vec<String> = PROFILES
        .iter()
        .map(|name| format!("{}:{}", quote(name), supported_by(&reqs, profile(name))))
        .collect();
    let inputs: Vec<String> = input_names(read)
        .iter()
        .map(|name| quote(name))
        .collect();
    format!(
        "{{\"kind\":\"surface\",\"digest\":\"0x{:016x}\",\"lighting\":{},\"layer_count\":{},\
         \"requirements\":{{\"inputs\":[{}],\"varying_channels\":{},\"has_displacement\":{},\
         \"param_count\":{},\"node_count\":{},\"needs_program\":{}}},\
         \"supported_by\":{{{}}},\"channels\":[{}]}}",
        read.digest().raw(),
        quote(&format!("{:?}", read.lighting())),
        read.layer_count(),
        inputs.join(","),
        reqs.varying_channels(),
        reqs.has_displacement(),
        reqs.param_count(),
        reqs.node_count(),
        reqs.needs_program(),
        verdicts.join(","),
        channels.join(",")
    )
}

fn input_names(read: &axiom_surface::SurfaceInspection) -> Vec<&'static str> {
    let inputs = read.requirements().inputs();
    [
        (axiom_surface::SurfaceInput::POINT, "point"),
        (axiom_surface::SurfaceInput::UV, "uv"),
        (axiom_surface::SurfaceInput::NORMAL, "normal"),
        (axiom_surface::SurfaceInput::TIME, "time"),
    ]
    .iter()
    .filter(|(bit, _)| inputs.contains(*bit))
    .map(|(_, name)| *name)
    .collect()
}

fn surface_text(read: &axiom_surface::SurfaceInspection) -> String {
    let reqs = read.requirements();
    let head = format!(
        "surface 0x{:016x}  lighting={:?}  layers={}  nodes={}  params={}  needs_program={}",
        read.digest().raw(),
        read.lighting(),
        read.layer_count(),
        reqs.node_count(),
        reqs.param_count(),
        reqs.needs_program()
    );
    let channels: Vec<String> = read
        .channels()
        .iter()
        .map(|row| {
            format!(
                "  {:<13} {:<8} {:<6}  nodes={} params={}",
                format!("{:?}", row.channel()),
                ["field", "constant"][usize::from(row.is_constant())],
                format!("{:?}", row.ty()),
                row.node_count(),
                row.param_count()
            )
        })
        .collect();
    let verdicts: Vec<String> = PROFILES
        .iter()
        .map(|name| format!("  {name}: {}", supported_by(&reqs, profile(name))))
        .collect();
    format!(
        "{head}\ninputs: {}\nchannels:\n{}\nsupported_by:\n{}",
        input_names(read).join(", "),
        channels.join("\n"),
        verdicts.join("\n")
    )
}

// ------------------------------------------------------------------ samples

/// A sample field: `mix(noise(point * 3), uv.x, tint)` — a literal, a context
/// source, a spatial sampler, a lane extract and a parameter knob, so every
/// column of the dump has something in it.
fn sample_field() -> FieldGraph {
    let (build, point) = FieldBuilder::new(FieldId::of_name("sample/marbled"), 1).push(
        FieldOp::Point,
        Vec::new(),
        Vec::new(),
    );
    let (build, three) = build.push_const(FieldValue::scalar(Scalar::new(3.0)));
    let (build, scaled) = build.push(FieldOp::Mul, Vec::new(), vec![point, three]);
    let (build, grain) = build.push_noise(0x0BAD_F00D, scaled);
    let (build, uv) = build.push(FieldOp::Uv, Vec::new(), Vec::new());
    let (build, lane) = build.push(FieldOp::Component, vec![Param::int(0)], vec![uv]);
    let (build, slot) = build.declare("tint", FieldValue::scalar(Scalar::new(0.25)));
    let (build, tint) = build.push_param(slot, FieldType::Scalar);
    let (build, mixed) = build.push(FieldOp::Mix, Vec::new(), vec![grain, lane, tint]);
    build.build(mixed)
}

/// A sample surface: the sample field driving roughness, everything else at its
/// default — the smallest thing that is not all-constant.
fn sample_surface() -> Surface {
    SurfaceBuilder::new()
        .field(SurfaceChannel::Roughness, sample_field())
        .build()
        .expect("a scalar field is a legal roughness")
}

// --------------------------------------------------------------------- JSON

/// One JSON string literal, with the six escapes the grammar requires plus a
/// `\u` escape for every other control character.
fn quote(text: &str) -> String {
    let body: String = text
        .chars()
        .map(|c| match c {
            '"' => String::from("\\\""),
            '\\' => String::from("\\\\"),
            '\n' => String::from("\\n"),
            '\r' => String::from("\\r"),
            '\t' => String::from("\\t"),
            c if (c as u32) < 0x20 => format!("\\u{:04x}", c as u32),
            c => c.to_string(),
        })
        .collect();
    format!("\"{body}\"")
}

/// A round-trip smoke test: the tool's own sample must describe, explain and
/// evaluate, so `--help` is not the only thing it is known to survive.
#[cfg(test)]
mod tests {
    use super::*;
    use axiom_field::EvalContext;

    #[test]
    fn the_sample_field_describes_explains_and_evaluates() {
        let graph = sample_field();
        assert_eq!(graph.validate(), Ok(()));
        let described = graph.describe().expect("the sample types");
        assert_eq!(described.node_count(), 8);
        let json = field_json(&graph, &described);
        assert!(json.starts_with("{\"kind\":\"field\""));
        assert!(json.contains("\"op\":\"Noise\""));
        assert!(json.contains("\"dependents\""));
        assert!(graph
            .evaluate(&EvalContext::at(
                axiom_math::Vec3::ZERO,
                axiom_math::Vec2::new(0.5, 0.0),
                axiom_math::Vec3::UNIT_Y
            ))
            .is_ok());
    }

    #[test]
    fn the_sample_surface_inspects_and_reports_a_verdict_per_profile() {
        let read = sample_surface().inspect().expect("the sample types");
        let json = surface_json(&read);
        assert!(json.contains("\"kind\":\"surface\""));
        assert!(json.contains("\"needs_program\":true"));
        assert!(json.contains("\"gpu\":true"));
        assert!(surface_text(&read).contains("supported_by"));
    }

    #[test]
    fn strings_are_escaped_so_the_document_stays_parseable() {
        assert_eq!(quote("a\"b\\c\nd\te\r"), "\"a\\\"b\\\\c\\nd\\te\\r\"");
        assert_eq!(quote("\u{1}"), "\"\\u0001\"");
        assert_eq!(number(f32::NAN), "null");
        assert_eq!(number(0.5), "0.5");
    }
}
