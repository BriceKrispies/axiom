//! The Animation Lab core: the isolated-actor harness poses one player through
//! every catalog clip via the real locomotion animator, deterministically. The
//! browser edge (`src/lab/web.rs`) is wasm-only and exercised by hand /
//! Playwright; this fixes the headless behavior the lab depends on.

use axiom_end_zone::lab::AnimLab;

fn index_of(lab: &AnimLab, label: &str) -> usize {
    lab.labels()
        .iter()
        .position(|l| *l == label)
        .unwrap_or_else(|| panic!("catalog has a `{label}` clip"))
}

#[test]
fn catalog_covers_every_anim_state() {
    // Fifteen AnimState variants → fifteen selectable clips.
    let lab = AnimLab::new();
    assert_eq!(lab.labels().len(), 15);
}

#[test]
fn every_clip_poses_exactly_one_finite_player() {
    let mut lab = AnimLab::new();
    let count = lab.labels().len();
    for i in 0..count {
        lab.select(i);
        let mut last = None;
        for _ in 0..40 {
            last = Some(lab.step());
        }
        let frame = last.expect("stepped at least once");
        assert_eq!(frame.poses.len(), 1, "clip {i} poses exactly the one actor");
        let pose = frame.poses[0].pose;
        assert!(
            pose.root_lift.is_finite() && pose.root_pitch.is_finite() && pose.root_roll.is_finite(),
            "clip {i} produces a finite root"
        );
        assert!(
            frame.camera.eye.x.is_finite() && frame.camera.target.y.is_finite(),
            "clip {i} frames a finite camera"
        );
        let sample = lab.sample().expect("a locomotion sample");
        assert!(
            sample.left_lock_error.is_finite() && sample.right_lock_error.is_finite(),
            "clip {i} has finite foot-lock diagnostics"
        );
    }
}

#[test]
fn a_moving_clip_actually_travels() {
    let mut lab = AnimLab::new();
    lab.select(index_of(&lab, "Sprint"));
    for _ in 0..90 {
        lab.step();
    }
    let sample = lab.sample().expect("sample");
    assert!(sample.distance_moved > 0.0, "the sprinter travels each tick");
    assert!(sample.speed > 5.0, "the sprinter carries sprint speed");
    assert!(!sample.overridden, "sprint is a locomotion state, not an override");
}

#[test]
fn a_still_clip_stays_put() {
    let mut lab = AnimLab::new();
    lab.select(index_of(&lab, "Idle"));
    let mut last = None;
    for _ in 0..60 {
        last = Some(lab.step());
    }
    let pos = last.expect("frame").snapshot.players[0].pos;
    assert!(
        pos.x.abs() < 1e-3 && pos.z.abs() < 1e-3,
        "an idle actor does not wander"
    );
}

#[test]
fn an_override_clip_reports_overridden() {
    let mut lab = AnimLab::new();
    lab.select(index_of(&lab, "Dive"));
    for _ in 0..20 {
        lab.step();
    }
    assert!(
        lab.sample().expect("sample").overridden,
        "Dive is posed by an override, not the gait"
    );
}

#[test]
fn switching_clips_re_anchors_without_a_skate_spike() {
    let mut lab = AnimLab::new();
    lab.select(index_of(&lab, "Sprint"));
    for _ in 0..80 {
        lab.step();
    }
    // Jump from a fast circle back to a still idle at the origin.
    lab.select(index_of(&lab, "Idle"));
    lab.step();
    let sample = lab.sample().expect("sample");
    assert!(
        sample.left_lock_error < 0.5 && sample.right_lock_error < 0.5,
        "the re-anchor absorbs the switch instead of registering it as a stride"
    );
}

#[test]
fn stepping_is_deterministic() {
    let mut a = AnimLab::new();
    let mut b = AnimLab::new();
    let clip = index_of(&a, "Jog");
    a.select(clip);
    b.select(clip);
    for _ in 0..120 {
        a.step();
        b.step();
    }
    let (sa, sb) = (a.sample().expect("a"), b.sample().expect("b"));
    assert_eq!(sa.gait_phase, sb.gait_phase);
    assert_eq!(sa.left_lock_error, sb.left_lock_error);
    assert_eq!(sa.stride_length, sb.stride_length);
}
