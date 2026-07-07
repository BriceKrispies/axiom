//! The sidecar metadata TOML written beside the contact-sheet PNG.
//!
//! For `out.png` it records `out.toml`: the app/scenario/backend/viewport/columns/
//! camera/debug settings, the captured ticks and markers, the output path, a
//! per-frame FNV-1a hash of the RGBA (so a capture is reproducible/comparable),
//! and the exact command-line args used.

use serde::Serialize;

use crate::capture_plan::CapturePlan;
use crate::replay_driver::CapturedFrame;
use crate::FilmstripError;

#[derive(Debug, Serialize)]
pub struct Metadata {
    app: String,
    scenario: String,
    backend: String,
    camera: String,
    debug_overlays: bool,
    cinematic: bool,
    columns: u32,
    out: String,
    viewport: ViewportMeta,
    ticks: Vec<u64>,
    markers: Vec<String>,
    command: Vec<String>,
    frames: Vec<FrameMeta>,
}

#[derive(Debug, Serialize)]
struct ViewportMeta {
    width: u32,
    height: u32,
}

#[derive(Debug, Serialize)]
struct FrameMeta {
    tick: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    marker: Option<String>,
    width: u32,
    height: u32,
    hash: String,
}

/// Build the metadata record for a completed run.
pub fn build(plan: &CapturePlan, frames: &[CapturedFrame], command: &[String]) -> Metadata {
    Metadata {
        app: plan.app.clone(),
        scenario: plan.scenario.clone(),
        backend: plan.backend.name().to_string(),
        camera: plan.camera.clone(),
        debug_overlays: plan.debug_overlays,
        cinematic: plan.cinematic,
        columns: plan.columns,
        out: plan.out.clone(),
        viewport: ViewportMeta {
            width: plan.viewport.width,
            height: plan.viewport.height,
        },
        ticks: plan.points.iter().map(|p| p.tick).collect(),
        markers: plan
            .points
            .iter()
            .filter_map(|p| p.marker.clone())
            .collect(),
        command: command.to_vec(),
        frames: frames
            .iter()
            .map(|f| FrameMeta {
                tick: f.point.tick,
                marker: f.point.marker.clone(),
                width: f.width,
                height: f.height,
                hash: fnv1a(&f.rgba),
            })
            .collect(),
    }
}

/// Serialize `meta` to TOML and write it to `path`, creating parent directories.
pub fn write(path: &std::path::Path, meta: &Metadata) -> Result<(), FilmstripError> {
    let text = toml::to_string_pretty(meta)
        .map_err(|e| FilmstripError::Io(format!("serialize metadata: {e}")))?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| FilmstripError::Io(format!("create {}: {e}", parent.display())))?;
    }
    std::fs::write(path, text)
        .map_err(|e| FilmstripError::Io(format!("write {}: {e}", path.display())))
}

/// A stable FNV-1a 64-bit fingerprint of a byte buffer, hex-encoded — the same
/// scheme the browser/native agents use for frame hashes.
pub fn fnv1a(bytes: &[u8]) -> String {
    let hash = bytes.iter().fold(0xcbf2_9ce4_8422_2325_u64, |h, &b| {
        (h ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3)
    });
    format!("{hash:016x}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capture_plan::{Backend, CapturePoint, Viewport};
    use crate::replay_driver::CapturedFrame;

    fn plan() -> CapturePlan {
        CapturePlan {
            app: "soccer_penalty".into(),
            scenario: "default_penalty_kick".into(),
            backend: Backend::Canvas2d,
            viewport: Viewport {
                width: 1280,
                height: 720,
            },
            columns: 4,
            camera: "default".into(),
            debug_overlays: false,
            cinematic: true,
            points: vec![
                CapturePoint {
                    tick: 0,
                    marker: Some("kicker.runup.start".into()),
                },
                CapturePoint {
                    tick: 12,
                    marker: Some("kicker.foot.ball_contact".into()),
                },
            ],
            out: "target/filmstrips/x.png".into(),
        }
    }

    fn frame(tick: u64, marker: Option<&str>) -> CapturedFrame {
        CapturedFrame {
            rgba: vec![7u8; 16],
            width: 2,
            height: 2,
            point: CapturePoint {
                tick,
                marker: marker.map(str::to_string),
            },
        }
    }

    #[test]
    fn fnv1a_is_stable_and_distinguishes_inputs() {
        assert_eq!(fnv1a(&[1, 2, 3]), fnv1a(&[1, 2, 3]));
        assert_ne!(fnv1a(&[1, 2, 3]), fnv1a(&[3, 2, 1]));
        assert_eq!(fnv1a(&[1, 2, 3]).len(), 16);
    }

    #[test]
    fn build_captures_the_run_and_serializes_to_toml() {
        let p = plan();
        let frames = vec![
            frame(0, Some("kicker.runup.start")),
            frame(12, Some("kicker.foot.ball_contact")),
        ];
        let cmd = vec!["--app".to_string(), "soccer_penalty".to_string()];
        let meta = build(&p, &frames, &cmd);
        assert_eq!(meta.app, "soccer_penalty");
        assert_eq!(meta.backend, "canvas2d");
        assert_eq!(meta.ticks, vec![0, 12]);
        assert_eq!(
            meta.markers,
            vec!["kicker.runup.start", "kicker.foot.ball_contact"]
        );
        assert_eq!(meta.frames.len(), 2);
        let text = toml::to_string_pretty(&meta).unwrap();
        assert!(text.contains("app = \"soccer_penalty\""));
        assert!(text.contains("[[frames]]"));
        assert!(text.contains("hash = "));
    }
}
