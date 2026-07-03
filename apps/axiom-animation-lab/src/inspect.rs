//! Terminal inspection: a deterministic text table for stepping through the kick
//! frame by frame without a renderer.
//!
//! This is the "scrub in the terminal" surface — one line per frame showing the
//! frame index, its phase, the world-space right-foot and plant-foot positions,
//! and any events. A default run prints the five inspection frames the kick is
//! meant to be read at (ready, plant, backswing, strike, follow-through).

use axiom_animation::{EventKind, HumanoidPrefab};

use crate::scene::{phase_name, LabScene};

/// The canonical frames to inspect the kick at.
pub const INSPECTION_FRAMES: [(u32, &str); 5] = [
    (0, "ready"),
    (18, "plant"),
    (24, "backswing"),
    (HumanoidPrefab::KICK_STRIKE_FRAME, "strike"),
    (36, "follow_through"),
];

/// A one-line summary of a single frame.
pub fn frame_line(scene: &LabScene, frame: u32) -> String {
    let view = scene.view(frame);
    let rf = view.right_foot;
    let pf = view.plant_foot;
    let events: Vec<String> = view
        .events
        .iter()
        .map(|e| {
            let kind = match e.kind {
                EventKind::KickContact => "KickContact",
                EventKind::FootPlant => "FootPlant",
            };
            format!("{kind}->bone{}", e.target_bone)
        })
        .collect();
    let events = if events.is_empty() {
        String::new()
    } else {
        format!("  [{}]", events.join(", "))
    };
    format!(
        "frame {:>2}/{}  phase {:<14}  Rfoot(z={:+.2}, y={:+.2})  plant(z={:+.2}, y={:+.2}){}",
        frame,
        scene.frame_count() - 1,
        phase_name(view.phase),
        rf.z,
        rf.y,
        pf.z,
        pf.y,
        events
    )
}

/// The default inspection table: a header plus one line per canonical frame.
pub fn inspection_table(scene: &LabScene) -> String {
    let mut out = String::from("Axiom Animation Lab — kick_right inspection\n");
    out.push_str("  (side view; Rfoot = kicking right foot, plant = support left foot)\n\n");
    INSPECTION_FRAMES.iter().for_each(|(frame, _label)| {
        out.push_str(&frame_line(scene, *frame));
        out.push('\n');
    });
    out
}

/// Every frame, one line each — a full scrub of the clip.
pub fn full_table(scene: &LabScene) -> String {
    let mut out = String::new();
    (0..scene.frame_count()).for_each(|f| {
        out.push_str(&frame_line(scene, f));
        out.push('\n');
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_lists_the_five_inspection_frames() {
        let scene = LabScene::new();
        let table = inspection_table(&scene);
        assert!(table.contains("phase ready"));
        assert!(table.contains("phase plant"));
        assert!(table.contains("phase backswing"));
        assert!(table.contains("phase strike"));
        assert!(table.contains("phase follow_through"));
    }

    #[test]
    fn strike_line_shows_kick_contact_event() {
        let scene = LabScene::new();
        let line = frame_line(&scene, HumanoidPrefab::KICK_STRIKE_FRAME);
        assert!(line.contains("KickContact"));
    }

    #[test]
    fn full_table_has_a_line_per_frame() {
        let scene = LabScene::new();
        let table = full_table(&scene);
        assert_eq!(table.lines().count(), scene.frame_count() as usize);
    }
}
