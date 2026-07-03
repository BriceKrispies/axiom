//! Deterministic SVG debug view of one kick frame.
//!
//! A side view (looking down the rig's local -X, so the kick swings across the
//! frame) with a ground plane, the ball, the skeleton drawn as bone lines with
//! joint markers, distinct right-foot and plant-foot markers, a phase timeline
//! strip with the KickContact frame ticked, and a text HUD showing the current
//! frame and phase. Pure string building — no platform, no randomness — so the
//! same frame always produces byte-identical SVG.

use axiom_animation::{HumanoidPrefab, PhaseKind};
use axiom_math::Vec3;

use crate::scene::{phase_name, LabScene};

const WIDTH: f32 = 820.0;
const HEIGHT: f32 = 480.0;
const SCALE: f32 = 190.0;
const CX: f32 = 200.0;
const CY: f32 = 430.0;
const STRIP_X: f32 = 20.0;
const STRIP_Y: f32 = 18.0;
const STRIP_W: f32 = WIDTH - 40.0;
const STRIP_H: f32 = 16.0;

/// Project a world point to screen space using the side view `(z, y)`.
fn project(p: Vec3) -> (f32, f32) {
    (CX + p.z * SCALE, CY - p.y * SCALE)
}

/// A muted color per phase for the timeline strip.
fn phase_color(phase: Option<PhaseKind>) -> &'static str {
    match phase {
        Some(PhaseKind::Ready) => "#3a4a5a",
        Some(PhaseKind::LeanForward) => "#3f5a6a",
        Some(PhaseKind::Approach) => "#3f6a5a",
        Some(PhaseKind::Plant) => "#5a6a3f",
        Some(PhaseKind::Backswing) => "#6a5a3f",
        Some(PhaseKind::Strike) => "#8a3f3f",
        Some(PhaseKind::FollowThrough) => "#6a3f5a",
        Some(PhaseKind::Recover) => "#4a4a4a",
        None => "#222222",
    }
}

/// Render the full debug SVG for `frame`.
pub fn render_frame(scene: &LabScene, frame: u32) -> String {
    let view = scene.view(frame);
    let mut s = String::new();
    s.push_str(&format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{WIDTH}\" height=\"{HEIGHT}\" \
         viewBox=\"0 0 {WIDTH} {HEIGHT}\" font-family=\"monospace\">\n"
    ));
    s.push_str(&format!(
        "<rect x=\"0\" y=\"0\" width=\"{WIDTH}\" height=\"{HEIGHT}\" fill=\"#12151b\"/>\n"
    ));

    push_phase_strip(&mut s, scene, frame);
    push_ground_and_ball(&mut s);
    push_bones(&mut s, &view);
    push_joints(&mut s, &view);
    push_foot_markers(&mut s, &view);
    push_contact_marker(&mut s, &view);
    push_hud(&mut s, &view, scene.frame_count());

    s.push_str("</svg>\n");
    s
}

fn push_phase_strip(s: &mut String, scene: &LabScene, frame: u32) {
    let n = scene.frame_count().max(1);
    let cell = STRIP_W / n as f32;
    (0..n).for_each(|f| {
        let x = STRIP_X + f as f32 * cell;
        s.push_str(&format!(
            "<rect x=\"{x:.1}\" y=\"{STRIP_Y}\" width=\"{:.2}\" height=\"{STRIP_H}\" fill=\"{}\"/>\n",
            cell + 0.5,
            phase_color(scene.phase_of(f))
        ));
    });
    // Current-frame cursor.
    let cur = STRIP_X + frame as f32 * cell;
    s.push_str(&format!(
        "<rect x=\"{cur:.1}\" y=\"{STRIP_Y}\" width=\"{:.2}\" height=\"{STRIP_H}\" \
         fill=\"none\" stroke=\"#ffffff\" stroke-width=\"2\"/>\n",
        cell + 0.5
    ));
    // KickContact frame tick.
    let contact = STRIP_X + HumanoidPrefab::KICK_STRIKE_FRAME as f32 * cell + cell * 0.5;
    s.push_str(&format!(
        "<line x1=\"{contact:.1}\" y1=\"{:.1}\" x2=\"{contact:.1}\" y2=\"{:.1}\" \
         stroke=\"#ff5a5a\" stroke-width=\"2\"/>\n",
        STRIP_Y - 4.0,
        STRIP_Y + STRIP_H + 4.0
    ));
    s.push_str(&format!(
        "<text x=\"{contact:.1}\" y=\"{:.1}\" fill=\"#ff9a9a\" font-size=\"10\" \
         text-anchor=\"middle\">contact</text>\n",
        STRIP_Y + STRIP_H + 16.0
    ));
}

fn push_ground_and_ball(s: &mut String) {
    s.push_str(&format!(
        "<line x1=\"0\" y1=\"{CY}\" x2=\"{WIDTH}\" y2=\"{CY}\" stroke=\"#44506a\" stroke-width=\"2\"/>\n"
    ));
    let (bx, by) = project(Vec3::new(0.0, 0.11, 0.55));
    s.push_str(&format!(
        "<circle cx=\"{bx:.1}\" cy=\"{by:.1}\" r=\"{:.1}\" fill=\"#e8e8e8\" stroke=\"#101010\" \
         stroke-width=\"1.5\"/>\n",
        0.11 * SCALE
    ));
    s.push_str(&format!(
        "<text x=\"{bx:.1}\" y=\"{:.1}\" fill=\"#9aa0aa\" font-size=\"10\" text-anchor=\"middle\">ball</text>\n",
        by + 0.11 * SCALE + 12.0
    ));
}

fn push_bones(s: &mut String, view: &crate::scene::FrameView) {
    view.bones.iter().for_each(|seg| {
        let (x1, y1) = project(seg.from);
        let (x2, y2) = project(seg.to);
        let (color, width) = if seg.is_kick_leg {
            ("#ffcf5a", 5.0)
        } else {
            ("#7fb0ff", 3.5)
        };
        s.push_str(&format!(
            "<line x1=\"{x1:.1}\" y1=\"{y1:.1}\" x2=\"{x2:.1}\" y2=\"{y2:.1}\" \
             stroke=\"{color}\" stroke-width=\"{width}\" stroke-linecap=\"round\"/>\n"
        ));
    });
}

fn push_joints(s: &mut String, view: &crate::scene::FrameView) {
    view.joints.iter().for_each(|j| {
        let (x, y) = project(*j);
        s.push_str(&format!(
            "<circle cx=\"{x:.1}\" cy=\"{y:.1}\" r=\"3\" fill=\"#dfe6ef\"/>\n"
        ));
    });
}

fn push_foot_markers(s: &mut String, view: &crate::scene::FrameView) {
    let (rx, ry) = project(view.right_foot);
    s.push_str(&format!(
        "<circle cx=\"{rx:.1}\" cy=\"{ry:.1}\" r=\"8\" fill=\"none\" stroke=\"#ff7a3a\" \
         stroke-width=\"2.5\"/>\n"
    ));
    s.push_str(&format!(
        "<text x=\"{:.1}\" y=\"{ry:.1}\" fill=\"#ffb07a\" font-size=\"11\">R foot</text>\n",
        rx + 12.0
    ));
    let (px, py) = project(view.plant_foot);
    s.push_str(&format!(
        "<circle cx=\"{px:.1}\" cy=\"{py:.1}\" r=\"8\" fill=\"none\" stroke=\"#5ad0ff\" \
         stroke-width=\"2.5\"/>\n"
    ));
    s.push_str(&format!(
        "<text x=\"{:.1}\" y=\"{py:.1}\" fill=\"#9ae0ff\" font-size=\"11\">plant</text>\n",
        px + 12.0
    ));
}

fn push_contact_marker(s: &mut String, view: &crate::scene::FrameView) {
    if view.is_contact_frame {
        let (x, y) = project(view.right_foot);
        s.push_str(&format!(
            "<circle cx=\"{x:.1}\" cy=\"{y:.1}\" r=\"16\" fill=\"none\" stroke=\"#ff3a3a\" \
             stroke-width=\"3\"/>\n"
        ));
        s.push_str(&format!(
            "<text x=\"{x:.1}\" y=\"{:.1}\" fill=\"#ff5a5a\" font-size=\"13\" \
             text-anchor=\"middle\" font-weight=\"bold\">KICK CONTACT</text>\n",
            y - 22.0
        ));
    }
}

fn push_hud(s: &mut String, view: &crate::scene::FrameView, frame_count: u32) {
    s.push_str(&format!(
        "<text x=\"20\" y=\"{:.1}\" fill=\"#eaeef5\" font-size=\"16\">frame {} / {}</text>\n",
        HEIGHT - 40.0,
        view.frame,
        frame_count - 1
    ));
    s.push_str(&format!(
        "<text x=\"20\" y=\"{:.1}\" fill=\"#ffcf5a\" font-size=\"16\">phase: {}</text>\n",
        HEIGHT - 18.0,
        phase_name(view.phase)
    ));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn svg_has_core_debug_elements() {
        let scene = LabScene::new();
        let svg = render_frame(&scene, HumanoidPrefab::KICK_STRIKE_FRAME);
        assert!(svg.starts_with("<svg"));
        assert!(svg.ends_with("</svg>\n"));
        assert!(svg.contains("R foot"));
        assert!(svg.contains("plant"));
        assert!(svg.contains("ball"));
        assert!(svg.contains("phase: strike"));
        assert!(svg.contains("KICK CONTACT"));
        assert!(svg.contains("contact"));
    }

    #[test]
    fn non_contact_frame_omits_contact_burst() {
        let scene = LabScene::new();
        let svg = render_frame(&scene, 0);
        assert!(!svg.contains("KICK CONTACT"));
        assert!(svg.contains("phase: ready"));
    }

    #[test]
    fn render_is_deterministic() {
        let scene = LabScene::new();
        assert_eq!(render_frame(&scene, 20), render_frame(&scene, 20));
    }
}
