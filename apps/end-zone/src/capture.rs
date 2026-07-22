//! Native capture builders for `axiom-shot`.
//!
//! This is the deterministic, headless "after the snap" screenshot harness. It
//! drives the ambient showcase — the app's own scripted play driver — to a
//! fixed post-snap tick, freezes it, and hands the posed [`RunningApp`] to
//! `axiom-shot`, which renders it to a PNG. No browser, no wall clock: the same
//! input every time yields the same frame, so the screenshot is reproducible
//! and usable as a visual-convergence reference/champion.

use axiom::prelude::RunningApp;
use axiom_input::KeyToken;

use crate::app::{EndZoneApp, TouchInput};
use crate::config::EndZoneConfig;

/// `axiom-shot` renders every registered slice at its own framebuffer size
/// (`registry::WIDTH`×`HEIGHT` = 960×600). Build the app to match so the baked
/// camera projection aspect is not stretched by the capture framebuffer.
const CAPTURE_WIDTH: u32 = 960;
const CAPTURE_HEIGHT: u32 = 600;

/// The sim tick to freeze on. The ambient showcase auto-starts the play at tick
/// `AUTO_START_DELAY` (100) and auto-snaps at `AUTO_START_DELAY + SNAP_DELAY`
/// (180); this lands ~0.5 s past the snap, with the line broken and the
/// quarterback still holding the ball (the one scripted throw is not until
/// `TRACE_THROW_TICK` = 258).
const POST_SNAP_TICK: u32 = 210;

/// Build End Zone frozen just after the snap, framed by the wide behind-the-
/// offense broadcast camera — the deterministic `end-zone-after-snap` slice.
pub fn build_end_zone_after_snap() -> RunningApp {
    let mut app = EndZoneApp::new_sized(EndZoneConfig::default(), CAPTURE_WIDTH, CAPTURE_HEIGHT);

    // Pin the wide broadcast camera. `Digit1` maps to `ForceFormationCamera`;
    // the director's snap-time switch changes only the *automatic* mode, so this
    // forced override survives the snap and keeps the behind-the-offense framing.
    let force_formation = [KeyToken::new("Digit1")];
    app.advance(&force_formation, TouchInput::default());

    // Step the showcase to just past the snap. Every other frame is hands-off:
    // the play, snap, blocking, and pursuit all emerge from the real systems.
    let idle: [KeyToken; 0] = [];
    for _ in 1..POST_SNAP_TICK {
        app.advance(&idle, TouchInput::default());
    }

    // Pose the frozen sim into the engine scene WITHOUT ticking the engine, then
    // hand it off: `axiom-shot` drives the single engine tick that renders this
    // frame, so the host frame sequence is advanced exactly once.
    app.pose_scene();
    app.into_running()
}
