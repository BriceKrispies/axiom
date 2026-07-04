//! Render a posed kick frame as a side-view SVG of low-poly boxes.
//!
//! The kick swings in the Y–Z plane (a pitch about X moves the legs forward in
//! Z), so the side view projects `(z, y)` to screen. Each figure part is drawn
//! as its box: the four sagittal-plane corners of the box are rotated by the
//! part's world orientation and projected, so limbs tilt as they swing. Pure
//! string building over the posed parts the scene produces.

use axiom_figure::PosedPart;
use axiom_math::Vec3;

use crate::authoring::{self, KICK_PHASES};
use crate::scene::{FrameView, LabScene};

const WIDTH: f32 = 480.0;
const HEIGHT: f32 = 460.0;
const SCALE: f32 = 150.0;
const CX: f32 = 150.0;
const CY: f32 = 410.0;
const STRIP_X: f32 = 20.0;
const STRIP_Y: f32 = 16.0;
const STRIP_W: f32 = WIDTH - 40.0;
const STRIP_H: f32 = 14.0;

/// Project a world point to screen space using the side view `(z, y)`.
fn project(p: Vec3) -> (f32, f32) {
    (CX + p.z * SCALE, CY - p.y * SCALE)
}

/// A muted fill/stroke color per opaque render tag.
fn tag_colors(tag: u32) -> (&'static str, &'static str) {
    match tag {
        0 => ("#2f6bd8", "#1c3f80"), // jersey
        1 => ("#e6e6ea", "#9aa0aa"), // shorts
        2 => ("#d8a97e", "#9c7250"), // skin
        3 => ("#33384a", "#20222c"), // sock
        _ => ("#1a1c22", "#000000"), // boot
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
    push_parts(&mut s, &view);
    push_foot_markers(&mut s, &view);
    push_contact_marker(&mut s, &view);
    push_hud(&mut s, &view);

    s.push_str("</svg>\n");
    s
}

fn push_phase_strip(s: &mut String, scene: &LabScene, frame: u32) {
    let n = scene.frame_count().max(1);
    let cell = STRIP_W / n as f32;
    for f in 0..n {
        let x = STRIP_X + f as f32 * cell;
        let fill = scene.phase_of(f).map_or("#222222", phase_color);
        s.push_str(&format!(
            "<rect x=\"{x:.1}\" y=\"{STRIP_Y}\" width=\"{:.2}\" height=\"{STRIP_H}\" fill=\"{fill}\"/>\n",
            cell + 0.5
        ));
    }
    let cur = STRIP_X + frame as f32 * cell;
    s.push_str(&format!(
        "<rect x=\"{cur:.1}\" y=\"{STRIP_Y}\" width=\"{:.2}\" height=\"{STRIP_H}\" fill=\"none\" \
         stroke=\"#ffffff\" stroke-width=\"2\"/>\n",
        cell + 0.5
    ));
    let contact = STRIP_X + authoring::CONTACT_FRAME as f32 * cell + cell * 0.5;
    s.push_str(&format!(
        "<line x1=\"{contact:.1}\" y1=\"{:.1}\" x2=\"{contact:.1}\" y2=\"{:.1}\" \
         stroke=\"#ff5a5a\" stroke-width=\"2\"/>\n",
        STRIP_Y - 4.0,
        STRIP_Y + STRIP_H + 4.0
    ));
}

fn phase_color(code: u32) -> &'static str {
    ["#3a4a5a", "#3f5a6a", "#3f6a5a", "#5a6a3f", "#6a5a3f", "#8a3f3f", "#6a3f5a", "#4a4a4a"]
        .get(code as usize)
        .copied()
        .unwrap_or("#222222")
}

fn push_ground_and_ball(s: &mut String) {
    s.push_str(&format!(
        "<line x1=\"0\" y1=\"{CY}\" x2=\"{WIDTH}\" y2=\"{CY}\" stroke=\"#44506a\" stroke-width=\"2\"/>\n"
    ));
    let (bx, by) = project(Vec3::new(0.0, 0.11, 0.62));
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

/// Draw each posed part as its projected box (four sagittal corners as a filled
/// polygon), rear parts first so the near leg reads on top.
fn push_parts(s: &mut String, view: &FrameView) {
    view.parts.iter().for_each(|part| push_box(s, part));
}

fn push_box(s: &mut String, part: &PosedPart) {
    let rot = part.transform.rotation;
    let center = part.transform.translation;
    let half_y = rot.rotate(Vec3::UNIT_Y).mul_scalar(part.box_size.y * 0.5);
    let half_z = rot.rotate(Vec3::UNIT_Z).mul_scalar(part.box_size.z * 0.5);
    let corners = [
        center.add(half_y).add(half_z),
        center.add(half_y).subtract(half_z),
        center.subtract(half_y).subtract(half_z),
        center.subtract(half_y).add(half_z),
    ];
    let points: String = corners
        .iter()
        .map(|c| {
            let (x, y) = project(*c);
            format!("{x:.1},{y:.1} ")
        })
        .collect();
    let (fill, stroke) = tag_colors(part.tag);
    s.push_str(&format!(
        "<polygon points=\"{points}\" fill=\"{fill}\" stroke=\"{stroke}\" stroke-width=\"1.5\" \
         stroke-linejoin=\"round\"/>\n"
    ));
}

fn push_foot_markers(s: &mut String, view: &FrameView) {
    let (rx, ry) = project(view.right_foot);
    s.push_str(&format!(
        "<circle cx=\"{rx:.1}\" cy=\"{ry:.1}\" r=\"7\" fill=\"none\" stroke=\"#ff7a3a\" stroke-width=\"2.5\"/>\n"
    ));
    s.push_str(&format!(
        "<text x=\"{:.1}\" y=\"{ry:.1}\" fill=\"#ffb07a\" font-size=\"11\">R foot</text>\n",
        rx + 11.0
    ));
    let (px, py) = project(view.plant_foot);
    s.push_str(&format!(
        "<circle cx=\"{px:.1}\" cy=\"{py:.1}\" r=\"7\" fill=\"none\" stroke=\"#5ad0ff\" stroke-width=\"2.5\"/>\n"
    ));
    s.push_str(&format!(
        "<text x=\"{:.1}\" y=\"{py:.1}\" fill=\"#9ae0ff\" font-size=\"11\">plant</text>\n",
        px + 11.0
    ));
}

fn push_contact_marker(s: &mut String, view: &FrameView) {
    if view.is_contact_frame {
        let (x, y) = project(view.right_foot);
        s.push_str(&format!(
            "<circle cx=\"{x:.1}\" cy=\"{y:.1}\" r=\"15\" fill=\"none\" stroke=\"#ff3a3a\" stroke-width=\"3\"/>\n"
        ));
        s.push_str(&format!(
            "<text x=\"{x:.1}\" y=\"{:.1}\" fill=\"#ff5a5a\" font-size=\"12\" text-anchor=\"middle\" \
             font-weight=\"bold\">KICK CONTACT</text>\n",
            y - 20.0
        ));
    }
}

fn push_hud(s: &mut String, view: &FrameView) {
    let phase = view.phase.map_or("-", authoring::phase_name);
    s.push_str(&format!(
        "<text x=\"20\" y=\"{:.1}\" fill=\"#eaeef5\" font-size=\"15\">frame {} / {}</text>\n",
        HEIGHT - 34.0,
        view.frame,
        KICK_PHASES.len() * 6 - 1
    ));
    s.push_str(&format!(
        "<text x=\"20\" y=\"{:.1}\" fill=\"#ffcf5a\" font-size=\"15\">phase: {phase}</text>\n",
        HEIGHT - 14.0
    ));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn svg_has_core_debug_elements() {
        let scene = LabScene::new();
        let svg = render_frame(&scene, authoring::CONTACT_FRAME);
        assert!(svg.starts_with("<svg"));
        assert!(svg.ends_with("</svg>\n"));
        assert!(svg.contains("<polygon"));
        assert!(svg.contains("R foot"));
        assert!(svg.contains("plant"));
        assert!(svg.contains("ball"));
        assert!(svg.contains("phase: strike"));
        assert!(svg.contains("KICK CONTACT"));
    }

    #[test]
    fn non_contact_frame_omits_the_burst() {
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
