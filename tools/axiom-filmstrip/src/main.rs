//! `axiom-filmstrip` — an agent-facing deterministic filmstrip capture tool.
//!
//! Runs a real Axiom app from a deterministic scenario, captures the rendered
//! frame at a list of ticks (or named animation markers), and composes them into
//! one labeled contact-sheet PNG (+ a sidecar metadata TOML) for human review.
//! It is a thin orchestrator over `axiom-shot` (see `replay_driver`); no renderer
//! is duplicated or mocked.
//!
//! ```text
//! cargo run -p axiom-filmstrip [--features offscreen] -- \
//!   --app <name> --scenario <name> --backend gpu|canvas2d \
//!   (--ticks "0,10,20" | --markers "a,b,c") [--out PATH] \
//!   [--viewport WxH] [--columns N] [--camera NAME] [--debug-overlays] \
//!   [--plan FILE]
//! ```

mod app_registry;
mod args;
mod capture_plan;
mod contact_sheet;
mod metadata;
mod replay_driver;

use std::fmt;
use std::process::ExitCode;

use capture_plan::{metadata_path, Backend, CapturePlan, PlanFile};

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    match run(&argv) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("axiom-filmstrip: {err}");
            ExitCode::FAILURE
        }
    }
}

fn run(argv: &[String]) -> Result<(), FilmstripError> {
    let raw = args::parse_argv(argv)?;
    let plan_file = raw.plan.as_deref().map(load_plan).transpose()?;
    let merged = args::merge(&raw, plan_file.as_ref())?;

    let app_name = merged
        .app
        .clone()
        .ok_or_else(|| FilmstripError::MissingApp {
            valid: app_registry::app_names()
                .iter()
                .map(|n| n.to_string())
                .collect(),
        })?;
    let app = app_registry::resolve_app(&app_name)?;
    let scenario = app.resolve_scenario(merged.scenario.as_deref())?;
    let camera = app.resolve_camera(merged.camera.as_deref())?;
    app.require_backend(merged.backend)?;
    merged
        .debug_overlays
        .then(|| app.require_debug())
        .transpose()?;
    let points = app.resolve_points(
        &scenario,
        merged.ticks.as_deref(),
        merged.markers.as_deref(),
    )?;

    let backend = effective_backend(merged.backend, &app)?;
    let out = merged
        .out
        .clone()
        .unwrap_or_else(|| format!("target/filmstrips/{}_{scenario}.png", app.name));

    let plan = CapturePlan {
        app: app.name.to_string(),
        scenario,
        backend,
        viewport: merged.viewport,
        columns: merged.columns,
        camera,
        debug_overlays: merged.debug_overlays,
        cinematic: app.cinematic,
        points,
        out,
    };

    println!(
        "axiom-filmstrip: capturing {} frame(s) — app={} scenario={} backend={} viewport={}x{}",
        plan.points.len(),
        plan.app,
        plan.scenario,
        plan.backend.name(),
        plan.viewport.width,
        plan.viewport.height,
    );

    let frames = replay_driver::capture(&plan, &app)?;
    let sheet = contact_sheet::compose(
        &frames,
        plan.columns,
        &plan.app,
        &plan.scenario,
        plan.backend.name(),
    );

    ensure_parent_dir(&plan.out)?;
    axiom_shot::capture::write_png(&plan.out, &sheet.rgba, sheet.width, sheet.height);

    let meta_path = metadata_path(&plan.out);
    let meta = metadata::build(&plan, &frames, argv);
    metadata::write(&meta_path, &meta)?;

    println!(
        "axiom-filmstrip: wrote {} ({}x{}) + {}",
        plan.out,
        sheet.width,
        sheet.height,
        meta_path.display()
    );
    Ok(())
}

/// Downgrade `--backend gpu` to canvas2d when the `offscreen` feature is off (the
/// GPU capture path is not compiled in), printing a clear notice so the metadata
/// reflects what actually rendered. Otherwise the requested backend stands.
fn effective_backend(
    requested: Backend,
    app: &app_registry::FilmstripApp,
) -> Result<Backend, FilmstripError> {
    match (requested, cfg!(feature = "offscreen")) {
        (Backend::Gpu, false) => {
            eprintln!(
                "axiom-filmstrip: --backend gpu needs `--features offscreen` (native wgpu is not \
                 compiled in); rendering canvas2d instead."
            );
            app.require_backend(Backend::Canvas2d)
                .map(|()| Backend::Canvas2d)
        }
        (backend, _) => Ok(backend),
    }
}

fn load_plan(path: &str) -> Result<PlanFile, FilmstripError> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| FilmstripError::Io(format!("read plan {path}: {e}")))?;
    PlanFile::parse(&text)
}

fn ensure_parent_dir(out: &str) -> Result<(), FilmstripError> {
    match std::path::Path::new(out).parent() {
        Some(parent) if !parent.as_os_str().is_empty() => std::fs::create_dir_all(parent)
            .map_err(|e| FilmstripError::Io(format!("create {}: {e}", parent.display()))),
        _ => Ok(()),
    }
}

/// Every way a filmstrip run can fail, with an actionable message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FilmstripError {
    MissingApp {
        valid: Vec<String>,
    },
    UnknownApp {
        app: String,
        valid: Vec<String>,
    },
    UnknownScenario {
        app: String,
        scenario: String,
        valid: Vec<String>,
    },
    UnknownCamera {
        app: String,
        camera: String,
        valid: Vec<String>,
    },
    UnsupportedBackend(String),
    BackendUnsupportedByApp {
        app: String,
        backend: String,
    },
    DebugUnsupported {
        app: String,
    },
    InvalidTicks(String),
    InvalidColumns(String),
    InvalidViewport(String),
    UnknownMarker {
        app: String,
        scenario: String,
        marker: String,
        valid: Vec<String>,
    },
    ConflictingCaptureModes,
    NoCapturePoints,
    CaptureFailed {
        tick: u64,
        reason: String,
    },
    InvalidPlan(String),
    Io(String),
}

impl fmt::Display for FilmstripError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FilmstripError::MissingApp { valid } => {
                write!(f, "no --app given; known apps: {valid:?}")
            }
            FilmstripError::UnknownApp { app, valid } => {
                write!(f, "unknown --app '{app}'; known apps: {valid:?}")
            }
            FilmstripError::UnknownScenario {
                app,
                scenario,
                valid,
            } => {
                write!(
                    f,
                    "app '{app}' has no scenario '{scenario}'; valid: {valid:?}"
                )
            }
            FilmstripError::UnknownCamera { app, camera, valid } => {
                write!(f, "app '{app}' has no camera '{camera}'; valid: {valid:?}")
            }
            FilmstripError::UnsupportedBackend(b) => {
                write!(f, "unsupported --backend '{b}'; use 'gpu' or 'canvas2d'")
            }
            FilmstripError::BackendUnsupportedByApp { app, backend } => {
                write!(f, "app '{app}' does not support backend '{backend}'")
            }
            FilmstripError::DebugUnsupported { app } => {
                write!(
                    f,
                    "app '{app}' has no debug-overlay path; drop --debug-overlays"
                )
            }
            FilmstripError::InvalidTicks(s) => {
                write!(f, "invalid --ticks '{s}'; expected a comma-separated list of non-negative integers")
            }
            FilmstripError::InvalidColumns(s) => {
                write!(f, "invalid --columns '{s}'; expected a positive integer")
            }
            FilmstripError::InvalidViewport(s) => {
                write!(
                    f,
                    "invalid --viewport '{s}'; expected WIDTHxHEIGHT, e.g. 1280x720"
                )
            }
            FilmstripError::UnknownMarker {
                app,
                scenario,
                marker,
                valid,
            } => {
                write!(
                    f,
                    "app '{app}' scenario '{scenario}' has no marker '{marker}'; valid: {valid:?}"
                )
            }
            FilmstripError::ConflictingCaptureModes => {
                write!(f, "specify EITHER --ticks OR --markers, not both")
            }
            FilmstripError::NoCapturePoints => {
                write!(
                    f,
                    "no capture points; pass --ticks \"0,10,...\" or --markers \"name,...\""
                )
            }
            FilmstripError::CaptureFailed { tick, reason } => {
                write!(f, "failed to capture tick {tick}: {reason}")
            }
            FilmstripError::InvalidPlan(msg) => write!(f, "invalid --plan file: {msg}"),
            FilmstripError::Io(msg) => write!(f, "io error: {msg}"),
        }
    }
}

impl std::error::Error for FilmstripError {}
