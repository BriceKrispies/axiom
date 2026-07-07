//! The resolved capture plan and its pure value types.
//!
//! A [`CapturePlan`] is the fully-resolved description of one filmstrip run:
//! which app/scenario, which backend, the viewport, the grid columns, the
//! camera, whether debug overlays are on, the ordered list of
//! [`CapturePoint`]s to shoot, and where to write. It is assembled in `main`
//! from the merged CLI+plan args ([`crate::args`]) and the app registry
//! ([`crate::app_registry`]), then consumed by the replay driver, the contact
//! sheet, and the metadata writer.
//!
//! Everything here is pure and unit-tested: backend parsing, the grid geometry
//! ([`contact_grid`]), and the sidecar-path derivation ([`metadata_path`]).

use std::path::PathBuf;

use serde::Deserialize;

use crate::FilmstripError;

/// The rendering backend a frame is captured through.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backend {
    /// Native off-screen wgpu (real GPU) — requires the `offscreen` feature.
    Gpu,
    /// Software Canvas 2D rasterizer — always available.
    Canvas2d,
}

impl Backend {
    /// Parse a backend name (`gpu`, `canvas2d`/`canvas`), or an error listing the
    /// valid names.
    pub fn parse(s: &str) -> Result<Backend, FilmstripError> {
        match s {
            "gpu" => Ok(Backend::Gpu),
            "canvas2d" | "canvas" => Ok(Backend::Canvas2d),
            other => Err(FilmstripError::UnsupportedBackend(other.to_string())),
        }
    }

    /// The canonical lowercase name (for labels + metadata).
    pub fn name(self) -> &'static str {
        match self {
            Backend::Gpu => "gpu",
            Backend::Canvas2d => "canvas2d",
        }
    }
}

/// A capture resolution (pixels). For a frame render this is fitted to the app's
/// authored aspect so nothing is ever stretched (see [`fit_viewport`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Viewport {
    pub width: u32,
    pub height: u32,
}

impl Viewport {
    /// Parse a `WIDTHxHEIGHT` string (e.g. `1280x720`), or an error.
    pub fn parse(s: &str) -> Result<Viewport, FilmstripError> {
        let bad = || FilmstripError::InvalidViewport(s.to_string());
        let (w, h) = s.split_once(['x', 'X']).ok_or_else(bad)?;
        let width: u32 = w.trim().parse().map_err(|_| bad())?;
        let height: u32 = h.trim().parse().map_err(|_| bad())?;
        (width > 0 && height > 0)
            .then_some(Viewport { width, height })
            .ok_or_else(bad)
    }
}

/// One named animation marker resolved to a tick of the deterministic scenario.
/// The tool-side marker traces (see [`crate::app_registry`]) are lists of these.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnimationMarker {
    pub name: String,
    pub tick: u64,
}

/// One point to capture: the tick to advance to, and the marker name that
/// resolved to it (when the run is in marker mode).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapturePoint {
    pub tick: u64,
    pub marker: Option<String>,
}

/// A fully-resolved filmstrip run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapturePlan {
    pub app: String,
    pub scenario: String,
    pub backend: Backend,
    pub viewport: Viewport,
    pub columns: u32,
    pub camera: String,
    pub debug_overlays: bool,
    /// Whether the GPU capture applies the app's cinematic grade (from the app
    /// registry). Recorded in metadata; consulted only by the GPU render path.
    pub cinematic: bool,
    pub points: Vec<CapturePoint>,
    pub out: String,
}

/// A `--plan` TOML file. Every field is optional; present fields seed the run and
/// explicit CLI args override them (see [`crate::args::merge`]). Mirrors the CLI.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlanFile {
    pub app: Option<String>,
    pub scenario: Option<String>,
    pub backend: Option<String>,
    pub viewport_width: Option<u32>,
    pub viewport_height: Option<u32>,
    pub columns: Option<u32>,
    pub ticks: Option<Vec<u64>>,
    pub markers: Option<Vec<String>>,
    pub camera: Option<String>,
    pub debug_overlays: Option<bool>,
    pub out: Option<String>,
}

impl PlanFile {
    /// Parse a plan file's TOML text.
    pub fn parse(text: &str) -> Result<PlanFile, FilmstripError> {
        toml::from_str(text).map_err(|e| FilmstripError::InvalidPlan(e.message().to_string()))
    }
}

/// The grid geometry for `n` frames at `columns` columns: `(rows, cols)`. Columns
/// shrink to the frame count when fewer frames than columns are captured, and
/// rows are `ceil(n / cols)`. `n == 0` yields `(0, 0)`.
pub fn contact_grid(n: usize, columns: u32) -> (u32, u32) {
    match n {
        0 => (0, 0),
        _ => {
            let cols = columns.max(1).min(n as u32);
            let rows = n.div_ceil(cols as usize) as u32;
            (rows, cols)
        }
    }
}

/// The sidecar metadata path for an output PNG: the same path with a `.toml`
/// extension (`a/b/kick.png` -> `a/b/kick.toml`). A path with no extension gains
/// `.toml`.
pub fn metadata_path(out_png: &str) -> PathBuf {
    PathBuf::from(out_png).with_extension("toml")
}

/// Fit `native` (authored w,h) inside `viewport` preserving aspect, so a frame is
/// captured at the largest size that fits without distortion. Never upscales past
/// the viewport; both dimensions stay >= 1.
pub fn fit_viewport(native: (u32, u32), viewport: Viewport) -> (u32, u32) {
    let (nw, nh) = (native.0.max(1) as f64, native.1.max(1) as f64);
    let scale = (viewport.width as f64 / nw).min(viewport.height as f64 / nh);
    let w = ((nw * scale).round() as u32).max(1);
    let h = ((nh * scale).round() as u32).max(1);
    (w, h)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backend_parses_known_names_and_rejects_others() {
        assert_eq!(Backend::parse("gpu").unwrap(), Backend::Gpu);
        assert_eq!(Backend::parse("canvas2d").unwrap(), Backend::Canvas2d);
        assert_eq!(Backend::parse("canvas").unwrap(), Backend::Canvas2d);
        assert_eq!(Backend::Gpu.name(), "gpu");
        assert_eq!(Backend::Canvas2d.name(), "canvas2d");
        assert!(matches!(
            Backend::parse("vulkan"),
            Err(FilmstripError::UnsupportedBackend(b)) if b == "vulkan"
        ));
    }

    #[test]
    fn viewport_parses_and_rejects() {
        assert_eq!(
            Viewport::parse("1280x720").unwrap(),
            Viewport {
                width: 1280,
                height: 720
            }
        );
        assert_eq!(
            Viewport::parse("640X480").unwrap(),
            Viewport {
                width: 640,
                height: 480
            }
        );
        assert!(Viewport::parse("1280").is_err());
        assert!(Viewport::parse("axb").is_err());
        assert!(Viewport::parse("0x720").is_err());
    }

    #[test]
    fn contact_grid_computes_rows_and_columns() {
        assert_eq!(contact_grid(13, 4), (4, 4));
        assert_eq!(contact_grid(8, 4), (2, 4));
        assert_eq!(contact_grid(4, 4), (1, 4));
        // Fewer frames than columns shrink the columns.
        assert_eq!(contact_grid(2, 4), (1, 2));
        assert_eq!(contact_grid(1, 4), (1, 1));
        assert_eq!(contact_grid(0, 4), (0, 0));
        // A zero column request is treated as one column.
        assert_eq!(contact_grid(3, 0), (3, 1));
    }

    #[test]
    fn metadata_path_swaps_extension_to_toml() {
        assert_eq!(
            metadata_path("target/filmstrips/kick.png"),
            PathBuf::from("target/filmstrips/kick.toml")
        );
        assert_eq!(metadata_path("kick.png"), PathBuf::from("kick.toml"));
        assert_eq!(metadata_path("noext"), PathBuf::from("noext.toml"));
    }

    #[test]
    fn fit_viewport_preserves_aspect_without_upscaling_past_the_box() {
        // 8:5 native into a 16:9 box is height-limited (no horizontal stretch).
        assert_eq!(
            fit_viewport(
                (960, 600),
                Viewport {
                    width: 1280,
                    height: 720
                }
            ),
            (1152, 720)
        );
        // Exact-aspect box fills both dimensions.
        assert_eq!(
            fit_viewport(
                (960, 600),
                Viewport {
                    width: 480,
                    height: 300
                }
            ),
            (480, 300)
        );
    }

    #[test]
    fn plan_file_parses_partial_and_rejects_unknown_fields() {
        let p = PlanFile::parse("app = \"soccer_penalty\"\ncolumns = 3\n").unwrap();
        assert_eq!(p.app.as_deref(), Some("soccer_penalty"));
        assert_eq!(p.columns, Some(3));
        assert!(p.ticks.is_none());
        assert!(PlanFile::parse("mystery = true").is_err());
    }
}
