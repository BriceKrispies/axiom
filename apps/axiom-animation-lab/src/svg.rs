//! Render a solved kick frame as a side-view SVG stick figure.
//!
//! The kick swings in the Y–Z plane (a pitch about X moves the legs forward in
//! Z), so the side view projects `(z, y)` to screen space. Pure string building
//! over the neutral joint data the scene produces.

use crate::rig::KickPhase;
use crate::scene::{FrameView, LabScene, Segment};

const WIDTH: f32 = 320.0;
const HEIGHT: f32 = 360.0;
const SCALE: f32 = 110.0;
const ORIGIN_X: f32 = 150.0;
const GROUND_Y: f32 = 340.0;

/// Project a world `(z, y)` to SVG screen coordinates (Z forward → screen right,
/// Y up → screen up).
fn project(z: f32, y: f32) -> (f32, f32) {
    (ORIGIN_X + z * SCALE, GROUND_Y - y * SCALE)
}

/// Render frame `frame` of `scene` as a standalone SVG document.
pub fn render_frame(scene: &LabScene, frame: u32) -> String {
    let view = scene.view(frame);
    let mut svg = String::new();
    svg.push_str(&format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{WIDTH}\" height=\"{HEIGHT}\" \
         viewBox=\"0 0 {WIDTH} {HEIGHT}\">\n"
    ));
    svg.push_str(&format!(
        "  <rect width=\"{WIDTH}\" height=\"{HEIGHT}\" fill=\"#0e1116\"/>\n"
    ));
    // Ground line.
    svg.push_str(&format!(
        "  <line x1=\"0\" y1=\"{GROUND_Y}\" x2=\"{WIDTH}\" y2=\"{GROUND_Y}\" \
         stroke=\"#39414d\" stroke-width=\"1\"/>\n"
    ));
    for seg in &view.segments {
        svg.push_str(&segment_line(seg));
    }
    svg.push_str(&caption(&view));
    svg.push_str("</svg>\n");
    svg
}

/// One bone as a coloured line (kick leg highlighted).
fn segment_line(seg: &Segment) -> String {
    let (x1, y1) = project(seg.from.z, seg.from.y);
    let (x2, y2) = project(seg.to.z, seg.to.y);
    let colour = if seg.is_kick_leg { "#ff7043" } else { "#9ecbff" };
    format!(
        "  <line x1=\"{x1:.1}\" y1=\"{y1:.1}\" x2=\"{x2:.1}\" y2=\"{y2:.1}\" \
         stroke=\"{colour}\" stroke-width=\"4\" stroke-linecap=\"round\"/>\n"
    )
}

/// Frame index, phase name, and a contact marker.
fn caption(view: &FrameView) -> String {
    let phase = view.phase.map_or("-", KickPhase::name);
    let contact = if view.is_contact_frame { "  ● CONTACT" } else { "" };
    let (fx, fy) = project(view.right_foot.z, view.right_foot.y);
    let marker = if view.is_contact_frame {
        format!("  <circle cx=\"{fx:.1}\" cy=\"{fy:.1}\" r=\"7\" fill=\"#ffd54f\"/>\n")
    } else {
        String::new()
    };
    format!(
        "{marker}  <text x=\"12\" y=\"24\" fill=\"#e6edf3\" font-family=\"monospace\" \
         font-size=\"14\">frame {:02}  {phase}{contact}</text>\n",
        view.frame
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn svg_has_a_frame_and_segments() {
        let scene = LabScene::new();
        let svg = render_frame(&scene, 0);
        assert!(svg.starts_with("<svg"));
        assert!(svg.trim_end().ends_with("</svg>"));
        assert!(svg.contains("frame 00"));
        assert!(svg.contains("<line"));
    }

    #[test]
    fn contact_frame_draws_the_ball_marker() {
        let scene = LabScene::new();
        let strike = render_frame(&scene, crate::rig::KICK_STRIKE_FRAME);
        assert!(strike.contains("CONTACT"));
        assert!(strike.contains("#ffd54f"));
        let ready = render_frame(&scene, 0);
        assert!(!ready.contains("CONTACT"));
    }
}
