//! Produce an *artifact of evidence* that the Layer-05 introspection surface
//! (`axiom-introspect`) works correctly against the real running engine.
//!
//! It drives the headless rotating-cube engine for a deterministic run,
//! interrogates `IntrospectApi` exactly as an agent would, and writes a plain
//! log of what it observed plus pass/fail checks to
//! `target/introspection-evidence.log` (also echoed to stdout). The process
//! exits non-zero if any check fails, so the artifact can be trusted and wired
//! into CI.
//!
//! ```sh
//! cargo run -p axiom-demo-rotating-cube --example introspection_evidence
//! ```
//!
//! Checks:
//! 1. Live capture — one report per tick, monotonic engine frame index.
//! 2. Query by index — describe_frame returns the matching report.
//! 3. Snapshot channel — snapshot_bytes() decodes back to an equal report.
//! 4. Deterministic — two independent runs yield byte-identical snapshots.
//! 5. Per-system report — a frame whose step failed carries the system's name +
//!    error code through serialization.

use std::fmt::Write as _;
use std::fs;

use axiom_demo_rotating_cube::DemoRotatingCubeApi;
use axiom_ecs::ComponentColumn;
use axiom_frame::FrameBuilder;
use axiom_host::{
    HostBoundaryConfig, HostFrameInput, HostLifecycleSignal, HostStepDriver, HostViewport,
};
use axiom_introspect::{FrameReport, IntrospectApi};
use axiom_kernel::{BinaryReader, BinaryWriter, EntityId, HandleId, Ratio};
use axiom_math::{Transform, Vec3};
use axiom_runtime::{
    Runtime, RuntimeConfig, RuntimeContext, RuntimeError, RuntimeErrorCode, RuntimeResult,
    RuntimeSystem,
};

const TICKS: u64 = 120;
const OUT_PATH: &str = "target/introspection-evidence.log";

fn main() {
    let mut log = String::new();
    let mut checks = 0u32;
    let mut passed = 0u32;

    macro_rules! info {
        ($($arg:tt)*) => {{ let _ = writeln!(log, "INFO {}", format_args!($($arg)*)); }};
    }
    macro_rules! check {
        ($label:expr, $pass:expr) => {{
            checks += 1;
            let pass = $pass;
            if pass {
                passed += 1;
            }
            let _ = writeln!(log, "{} {}", if pass { "PASS" } else { "FAIL" }, $label);
        }};
    }

    // --- 1. Live capture over a deterministic run. ---
    let mut demo = DemoRotatingCubeApi::new();
    let mut indices = Vec::new();
    for tick in 0..=TICKS {
        indices.push(demo.run_tick(tick).engine_frame_index);
    }
    let recent = demo.recent_frames((TICKS + 1) as usize);
    info!("capture ticks=0..={TICKS} retained={}", recent.len());
    for r in recent {
        info!(
            "frame idx={} host_seq={} steps={} lifecycle={:?} viewport={}x{} systems=[{}] metrics=[{}]",
            r.engine_frame_index(),
            r.host_frame_sequence(),
            r.runtime_step_count(),
            r.lifecycle(),
            r.viewport_width(),
            r.viewport_height(),
            fmt_systems(r),
            fmt_metrics(r),
        );
    }
    let monotonic = indices.windows(2).all(|w| w[0] < w[1]);
    check!(
        "one report retained per tick",
        recent.len() == (TICKS + 1) as usize
    );
    check!("engine frame index strictly monotonic", monotonic);

    // The whole point: a metric that actually changes frame to frame, plus a
    // diff between two frames an agent would compare.
    let angle = |idx: usize| metric_f32(&recent[idx], "cube.angle_rad");
    info!(
        "diff idx {}->{}: cube.angle_rad {:.4} -> {:.4}  (delta {:.4})",
        recent[0].engine_frame_index(),
        recent[60].engine_frame_index(),
        angle(0),
        angle(60),
        angle(60) - angle(0),
    );
    check!(
        "cube.angle_rad changes between frame 0 and 60",
        angle(0) != angle(60)
    );
    check!(
        "every frame carries the cube.angle_rad metric",
        recent
            .iter()
            .all(|r| r.metrics().iter().any(|m| m.name() == "cube.angle_rad"))
    );

    // --- 2. Query by index. ---
    let probe = indices[60];
    let described_ok = demo
        .describe_frame(probe)
        .map(FrameReport::engine_frame_index)
        == Some(probe);
    let missing_ok = demo.describe_frame(u64::MAX).is_none();
    info!("query describe_frame({probe})={described_ok} describe_frame(MAX)_miss={missing_ok}");
    check!("describe_frame returns the matching report", described_ok);
    check!("describe_frame misses an unobserved index", missing_ok);

    // --- 3. Serialized snapshot channel round-trips. ---
    let snapshot = demo.introspection_snapshot().expect("a tick has run");
    let decoded = FrameReport::from_bytes(&snapshot).expect("snapshot decodes");
    let round_trips = Some(&decoded) == demo.recent_frames(1).first();
    info!(
        "snapshot bytes={} roundtrip={round_trips} hex={}",
        snapshot.len(),
        hex(&snapshot)
    );
    check!("snapshot decodes back to an equal report", round_trips);

    // --- 4. Deterministic replay. ---
    let run = |ticks: u64| {
        let mut d = DemoRotatingCubeApi::new();
        for t in 0..=ticks {
            d.run_tick(t);
        }
        d.introspection_snapshot().expect("a tick has run")
    };
    let snap_a = run(TICKS);
    let snap_b = run(TICKS);
    let identical = snap_a == snap_b;
    info!("replay runs=2 bytes={} identical={identical}", snap_a.len());
    check!(
        "independent runs are byte-identical (replayable)",
        identical
    );

    // --- 5. Per-system introspection via a failing system. ---
    let frame = failing_system_frame();
    let mut api = IntrospectApi::new(4);
    api.observe(&frame);
    let report = api.latest().expect("observed one frame");
    let sys = &report.systems()[0];
    info!(
        "system id={} name={} order={} succeeded={} error_code={:?}",
        sys.system_id(),
        sys.name(),
        sys.order(),
        sys.succeeded(),
        sys.error_code(),
    );
    let sys_bytes = api.snapshot_bytes().expect("observed one frame");
    let sys_decoded = FrameReport::from_bytes(&sys_bytes).expect("decodes");
    check!("failing system captured by name", sys.name() == "physics");
    check!(
        "failure carries a non-empty error code",
        sys.error_code().is_some()
    );
    check!(
        "frame-with-system serializes and round-trips",
        sys_decoded.systems().len() == 1 && &sys_decoded == report
    );

    // --- 6. Reflection: the world describes its components, and a component
    //        column round-trips as bytes (the world is now data). ---
    for schema in demo.component_schemas() {
        let fields: Vec<String> = schema
            .fields()
            .iter()
            .map(|f| format!("{}: {}", f.name(), f.type_name()))
            .collect();
        info!("schema {} {{ {} }}", schema.name(), fields.join(", "));
    }
    check!(
        "the world exposes component schemas",
        demo.component_schemas().len() >= 4
    );

    let mut column: ComponentColumn<Transform> = ComponentColumn::new();
    column.insert(EntityId::from_raw(1), Transform::IDENTITY);
    column.insert(
        EntityId::from_raw(2),
        Transform::from_translation(Vec3::new(1.0, 2.0, 3.0)),
    );
    let mut w = BinaryWriter::new();
    column.reflect_write(&mut w);
    let bytes = w.into_bytes();
    let decoded = ComponentColumn::<Transform>::reflect_read(&mut BinaryReader::new(&bytes))
        .expect("decodes");
    let column_roundtrips = decoded.len() == 2 && decoded.get(EntityId::from_raw(2)).is_some();
    info!(
        "column Transform entries=2 bytes={} roundtrip={column_roundtrips}",
        bytes.len()
    );
    check!(
        "a Transform component column round-trips as bytes",
        column_roundtrips
    );

    let all_pass = passed == checks;
    let _ = writeln!(
        log,
        "RESULT {} checks={passed}/{checks}",
        if all_pass { "PASS" } else { "FAIL" }
    );

    fs::write(OUT_PATH, &log).expect("write evidence artifact");
    print!("{log}");
    eprintln!("wrote {OUT_PATH}");
    if !all_pass {
        std::process::exit(1);
    }
}

/// Build one engine frame whose single runtime step ran a failing system, so
/// its frame report carries a populated per-system report with an error code.
fn failing_system_frame() -> axiom_frame::EngineFrame {
    const STEP: u64 = 1_000;
    struct AlwaysFail;
    impl RuntimeSystem for AlwaysFail {
        fn run(&mut self, _: &mut RuntimeContext<'_>) -> RuntimeResult<()> {
            Err(RuntimeError::new(RuntimeErrorCode::SystemFailed, "boom"))
        }
    }
    let viewport = HostViewport::new(320, 200, Ratio::new(1.0).unwrap()).unwrap();
    let mut driver = HostStepDriver::new(HostBoundaryConfig::new(STEP, 5).unwrap());
    driver.apply_lifecycle_signal(HostLifecycleSignal::Started);
    let mut runtime =
        Runtime::new(RuntimeConfig::new(STEP).with_fail_on_system_error(false)).unwrap();
    runtime.initialize().unwrap();
    runtime.start().unwrap();
    runtime
        .scheduler_mut()
        .register(HandleId::from_raw(1), "physics", 1, Box::new(AlwaysFail))
        .unwrap();
    let report = driver
        .drive(&mut runtime, HostFrameInput::new(1, STEP, viewport))
        .unwrap();
    FrameBuilder::new(STEP).build(&report, Vec::new()).unwrap()
}

/// `name=value` for each metric on a frame, space-separated.
fn fmt_metrics(report: &FrameReport) -> String {
    let mut s = String::new();
    for (i, m) in report.metrics().iter().enumerate() {
        if i > 0 {
            s.push(' ');
        }
        match m.value().as_integer() {
            Some(v) => {
                let _ = write!(s, "{}={v}", m.name());
            }
            None => {
                let _ = write!(s, "{}={:.3}", m.name(), m.value().as_float().unwrap_or(0.0));
            }
        }
    }
    s
}

/// Comma-separated system names on a frame.
fn fmt_systems(report: &FrameReport) -> String {
    report
        .systems()
        .iter()
        .map(axiom_introspect::SystemReport::name)
        .collect::<Vec<_>>()
        .join(",")
}

/// The float value of the named metric on a frame, or 0.0 if absent.
fn metric_f32(report: &FrameReport, name: &str) -> f32 {
    report
        .metrics()
        .iter()
        .find(|m| m.name() == name)
        .and_then(|m| m.value().as_float())
        .unwrap_or(0.0)
}

/// Lowercase hex of a byte slice.
fn hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        let _ = write!(s, "{b:02x}");
    }
    s
}
