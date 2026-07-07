//! The filmstrip app registry: the table mapping a filmstrip `--app` name to how
//! it is captured — the `axiom-shot` slice it builds, its scenarios, cameras,
//! supported backends, authored render size, cinematic-grade flag, and the
//! per-scenario animation-marker trace.
//!
//! Adding an app is adding one row to [`registry`]. Today there is one entry
//! (`soccer_penalty`), which resolves to axiom-shot's already-registered
//! `"soccer-penalty"` slice — so no app rendering code is duplicated.
//!
//! The marker trace is *static scenario metadata* (a name -> tick map for the
//! deterministic shot), not a general animation system; it exists so the agent
//! can capture "the moment the foot meets the ball" without knowing its tick.

use crate::capture_plan::{AnimationMarker, Backend, CapturePoint};
use crate::FilmstripError;

/// One capturable app: its filmstrip name and everything needed to drive
/// axiom-shot for it.
#[derive(Debug, Clone, Copy)]
pub struct FilmstripApp {
    /// The `--app` name (e.g. `soccer_penalty`).
    pub name: &'static str,
    /// The axiom-shot registry slice name this builds (e.g. `soccer-penalty`).
    pub shot_name: &'static str,
    /// The scenarios this app understands (first is the default).
    pub scenarios: &'static [&'static str],
    /// The named cameras this app offers (first is the default).
    pub cameras: &'static [&'static str],
    /// The backends this app can be captured through.
    pub backends: &'static [Backend],
    /// The authored render size (w, h); captures fit this aspect (no distortion).
    pub native: (u32, u32),
    /// Whether `--debug-overlays` is supported (routed through the axiom-shot seam).
    pub supports_debug: bool,
    /// Whether the GPU capture applies the soccer cinematic grade (retro-32-bit
    /// quantize + exposure/contrast). Recorded in metadata; consulted only by the
    /// GPU (`offscreen`) render path.
    pub cinematic: bool,
    /// The animation-marker trace for a given scenario (name -> tick).
    pub markers: fn(&str) -> Vec<AnimationMarker>,
}

impl FilmstripApp {
    fn default_scenario(&self) -> String {
        self.scenarios
            .first()
            .copied()
            .unwrap_or_default()
            .to_string()
    }

    /// Resolve the requested scenario (or the default), erroring on an unknown one.
    pub fn resolve_scenario(&self, requested: Option<&str>) -> Result<String, FilmstripError> {
        match requested {
            None => Ok(self.default_scenario()),
            Some(s) if self.scenarios.contains(&s) => Ok(s.to_string()),
            Some(s) => Err(FilmstripError::UnknownScenario {
                app: self.name.to_string(),
                scenario: s.to_string(),
                valid: self.scenarios.iter().map(|c| c.to_string()).collect(),
            }),
        }
    }

    /// Resolve the requested camera (or the default), erroring on an unknown one.
    pub fn resolve_camera(&self, requested: Option<&str>) -> Result<String, FilmstripError> {
        let default = self.cameras.first().copied().unwrap_or("default");
        match requested {
            None => Ok(default.to_string()),
            Some(c) if self.cameras.contains(&c) => Ok(c.to_string()),
            Some(c) => Err(FilmstripError::UnknownCamera {
                app: self.name.to_string(),
                camera: c.to_string(),
                valid: self.cameras.iter().map(|c| c.to_string()).collect(),
            }),
        }
    }

    /// Confirm this app supports `backend`.
    pub fn require_backend(&self, backend: Backend) -> Result<(), FilmstripError> {
        self.backends
            .contains(&backend)
            .then_some(())
            .ok_or_else(|| FilmstripError::BackendUnsupportedByApp {
                app: self.name.to_string(),
                backend: backend.name().to_string(),
            })
    }

    /// Confirm this app supports debug overlays (for `--debug-overlays`).
    pub fn require_debug(&self) -> Result<(), FilmstripError> {
        self.supports_debug
            .then_some(())
            .ok_or_else(|| FilmstripError::DebugUnsupported {
                app: self.name.to_string(),
            })
    }

    /// Resolve the capture points from exactly one of `ticks` or `markers`.
    /// Exactly one must be non-empty; markers are resolved to ticks through this
    /// app's static trace for `scenario`.
    pub fn resolve_points(
        &self,
        scenario: &str,
        ticks: Option<&[u64]>,
        markers: Option<&[String]>,
    ) -> Result<Vec<CapturePoint>, FilmstripError> {
        let has_ticks = ticks.is_some_and(|t| !t.is_empty());
        let has_markers = markers.is_some_and(|m| !m.is_empty());
        match (has_ticks, has_markers) {
            (true, true) => Err(FilmstripError::ConflictingCaptureModes),
            (false, false) => Err(FilmstripError::NoCapturePoints),
            (true, false) => Ok(ticks
                .unwrap_or_default()
                .iter()
                .map(|&tick| CapturePoint { tick, marker: None })
                .collect()),
            (false, true) => {
                let trace = (self.markers)(scenario);
                markers
                    .unwrap_or_default()
                    .iter()
                    .map(|name| self.resolve_marker(scenario, name, &trace))
                    .collect()
            }
        }
    }

    fn resolve_marker(
        &self,
        scenario: &str,
        name: &str,
        trace: &[AnimationMarker],
    ) -> Result<CapturePoint, FilmstripError> {
        trace
            .iter()
            .find(|m| m.name == name)
            .map(|m| CapturePoint {
                tick: m.tick,
                marker: Some(m.name.clone()),
            })
            .ok_or_else(|| FilmstripError::UnknownMarker {
                app: self.name.to_string(),
                scenario: scenario.to_string(),
                marker: name.to_string(),
                valid: trace.iter().map(|m| m.name.clone()).collect(),
            })
    }
}

/// Every capturable app.
pub fn registry() -> Vec<FilmstripApp> {
    vec![FilmstripApp {
        name: "soccer_penalty",
        shot_name: "soccer-penalty",
        scenarios: &["default_penalty_kick"],
        cameras: &["default"],
        backends: &[Backend::Gpu, Backend::Canvas2d],
        native: (960, 600),
        supports_debug: true,
        cinematic: true,
        markers: soccer_markers,
    }]
}

/// The names of every registered app (for `--app`-unknown error messages).
pub fn app_names() -> Vec<&'static str> {
    registry().into_iter().map(|a| a.name).collect()
}

/// Resolve an app by name, erroring (with the valid list) on an unknown one.
pub fn resolve_app(name: &str) -> Result<FilmstripApp, FilmstripError> {
    registry()
        .into_iter()
        .find(|a| a.name == name)
        .ok_or_else(|| FilmstripError::UnknownApp {
            app: name.to_string(),
            valid: app_names().iter().map(|n| n.to_string()).collect(),
        })
}

/// The static marker trace for the soccer penalty scenarios. Tick values follow
/// the deterministic shot (`soccer_shot_state`: charge for 8 ticks, release at 8,
/// then ball flight and result freeze). Non-soccer scenarios have no markers.
fn soccer_markers(scenario: &str) -> Vec<AnimationMarker> {
    let mark = |name: &str, tick: u64| AnimationMarker {
        name: name.to_string(),
        tick,
    };
    match scenario {
        // Ticks are grounded in the rendered shot (charge for 8 ticks, release at
        // 8, ball leaves the foot ~tick 9, crosses the line ~tick 22, settles ~34).
        "default_penalty_kick" => vec![
            mark("kicker.runup.start", 0),
            mark("kicker.stride.1", 2),
            mark("kicker.stride.2", 4),
            mark("kicker.left_foot.plant", 6),
            mark("kicker.hip.twist.peak", 7),
            mark("kicker.right_leg.swing.apex", 8),
            mark("kicker.foot.ball_contact", 9),
            mark("kicker.followthrough.peak", 13),
            mark("goalie.dive.commit", 16),
            mark("ball.goal_line.cross", 22),
            mark("result.freeze", 40),
        ],
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn soccer() -> FilmstripApp {
        resolve_app("soccer_penalty").unwrap()
    }

    #[test]
    fn unknown_app_lists_the_valid_names() {
        assert!(matches!(
            resolve_app("halo"),
            Err(FilmstripError::UnknownApp { app, valid })
                if app == "halo" && valid == vec!["soccer_penalty".to_string()]
        ));
        assert_eq!(app_names(), vec!["soccer_penalty"]);
    }

    #[test]
    fn scenario_and_camera_default_and_validate() {
        let app = soccer();
        assert_eq!(app.resolve_scenario(None).unwrap(), "default_penalty_kick");
        assert_eq!(
            app.resolve_scenario(Some("default_penalty_kick")).unwrap(),
            "default_penalty_kick"
        );
        assert!(matches!(
            app.resolve_scenario(Some("shootout")),
            Err(FilmstripError::UnknownScenario { .. })
        ));
        assert_eq!(app.resolve_camera(None).unwrap(), "default");
        assert!(matches!(
            app.resolve_camera(Some("drone")),
            Err(FilmstripError::UnknownCamera { .. })
        ));
    }

    #[test]
    fn backend_and_debug_support() {
        let app = soccer();
        assert!(app.require_backend(Backend::Gpu).is_ok());
        assert!(app.require_backend(Backend::Canvas2d).is_ok());
        assert!(app.require_debug().is_ok());
    }

    #[test]
    fn points_from_ticks() {
        let app = soccer();
        let pts = app
            .resolve_points("default_penalty_kick", Some(&[0, 10, 20]), None)
            .unwrap();
        assert_eq!(pts.len(), 3);
        assert_eq!(
            pts[1],
            CapturePoint {
                tick: 10,
                marker: None
            }
        );
    }

    #[test]
    fn points_from_markers_resolve_to_ticks() {
        let app = soccer();
        let names = vec![
            "kicker.foot.ball_contact".to_string(),
            "result.freeze".to_string(),
        ];
        let pts = app
            .resolve_points("default_penalty_kick", None, Some(&names))
            .unwrap();
        assert_eq!(
            pts[0],
            CapturePoint {
                tick: 9,
                marker: Some("kicker.foot.ball_contact".into())
            }
        );
        assert_eq!(pts[1].tick, 40);
    }

    #[test]
    fn unknown_marker_is_an_error_with_valid_names() {
        let app = soccer();
        let names = vec!["kicker.moonwalk".to_string()];
        assert!(matches!(
            app.resolve_points("default_penalty_kick", None, Some(&names)),
            Err(FilmstripError::UnknownMarker { marker, valid, .. })
                if marker == "kicker.moonwalk" && valid.contains(&"result.freeze".to_string())
        ));
    }

    #[test]
    fn conflicting_and_empty_capture_modes_error() {
        let app = soccer();
        assert!(matches!(
            app.resolve_points(
                "default_penalty_kick",
                Some(&[0]),
                Some(&["result.freeze".to_string()])
            ),
            Err(FilmstripError::ConflictingCaptureModes)
        ));
        assert!(matches!(
            app.resolve_points("default_penalty_kick", None, None),
            Err(FilmstripError::NoCapturePoints)
        ));
    }
}
