//! End-to-end behavioral proofs for the humanoid kick, exercised through the
//! public [`AnimationApi`] facade — the properties the animation lab depends on.

use axiom_animation::{AnimationApi, EventKind, HumanoidPrefab, PhaseKind, Pose, SkeletonError};
use axiom_math::Vec3;

/// The default humanoid skeleton validates.
#[test]
fn default_humanoid_skeleton_validates() {
    let api = AnimationApi::new();
    let prefab = api.default_humanoid();
    assert_eq!(api.validate_skeleton(&prefab.skeleton), Ok(()));
}

/// An invalid parent reference fails validation.
#[test]
fn invalid_parent_reference_fails() {
    let api = AnimationApi::new();
    let mut prefab = api.default_humanoid();
    // Point a bone at a parent that comes after it (forward reference).
    prefab.skeleton.bones[2].parent = Some(9);
    assert_eq!(
        api.validate_skeleton(&prefab.skeleton),
        Err(SkeletonError::BadParent { bone: 2 })
    );
}

/// Sampling the same clip at the same frame yields the same pose — determinism.
#[test]
fn clip_sampling_is_deterministic() {
    let api = AnimationApi::new();
    let prefab = api.default_humanoid();
    let clip = &prefab.clips[0];
    let n = prefab.skeleton.bone_count();
    (0..clip.frame_count).for_each(|f| {
        assert_eq!(api.sample(clip, n, f), api.sample(clip, n, f));
    });
}

/// The kick_right clip declares its eight phases in the authored order.
#[test]
fn kick_has_expected_phase_order() {
    let clip = HumanoidPrefab::kick_right_clip();
    assert_eq!(
        clip.phase_kinds(),
        vec![
            PhaseKind::Ready,
            PhaseKind::LeanForward,
            PhaseKind::Approach,
            PhaseKind::Plant,
            PhaseKind::Backswing,
            PhaseKind::Strike,
            PhaseKind::FollowThrough,
            PhaseKind::Recover,
        ]
    );
}

/// The KickContact event occurs on exactly the configured strike frame and
/// nowhere else, targeting the right foot.
#[test]
fn kick_contact_is_exactly_on_the_strike_frame() {
    let api = AnimationApi::new();
    let clip = HumanoidPrefab::kick_right_clip();
    let contact_frames: Vec<u32> = (0..clip.frame_count)
        .filter(|&f| {
            api.events_at(&clip, f)
                .iter()
                .any(|e| e.kind == EventKind::KickContact)
        })
        .collect();
    assert_eq!(contact_frames, vec![HumanoidPrefab::KICK_STRIKE_FRAME]);

    let contact = api.events_at(&clip, HumanoidPrefab::KICK_STRIKE_FRAME);
    assert_eq!(contact[0].kind, EventKind::KickContact);
    assert_eq!(contact[0].target_bone, HumanoidPrefab::RIGHT_FOOT_BONE);
}

/// The pose solver prevents backward knee and elbow bends: a pose driving the
/// right knee and right elbow into hyperextension is clamped to their limits.
#[test]
fn pose_solver_blocks_backward_knee_and_elbow() {
    let api = AnimationApi::new();
    let prefab = api.default_humanoid();
    let knee = prefab.skeleton.bone_index("right_lower_leg").unwrap();
    let elbow = prefab.skeleton.bone_index("right_lower_arm").unwrap();

    let mut eulers = vec![Vec3::ZERO; prefab.skeleton.bone_count()];
    eulers[knee] = Vec3::new(-1.2, 0.0, 0.0); // backward knee bend
    eulers[elbow] = Vec3::new(-1.2, 0.0, 0.0); // backward elbow bend
    let bad = Pose::new(eulers);

    assert!(!api.is_pose_legal(&prefab.joint_limits, &bad));
    let solved = api.solve(&prefab.joint_limits, &bad);
    assert!(api.is_pose_legal(&prefab.joint_limits, &solved));
    assert!(solved.joint_eulers[knee].x >= 0.0);
    assert!(solved.joint_eulers[elbow].x >= 0.0);
}

/// Every sampled frame of the kick over its full duration solves to a legal
/// pose — and, in fact, the authored clip is already within limits, so solving
/// never distorts it.
#[test]
fn every_frame_solves_to_a_legal_pose() {
    let api = AnimationApi::new();
    let prefab = api.default_humanoid();
    let clip = &prefab.clips[0];
    let n = prefab.skeleton.bone_count();
    (0..clip.frame_count).for_each(|f| {
        let raw = api.sample(clip, n, f);
        let solved = api.solve(&prefab.joint_limits, &raw);
        assert!(
            api.is_pose_legal(&prefab.joint_limits, &solved),
            "solved pose illegal at frame {f}"
        );
        // The authored kick respects the joint limits as-authored.
        assert!(
            api.is_pose_legal(&prefab.joint_limits, &raw),
            "authored pose illegal at frame {f}"
        );
    });
}

/// `lib.rs` publicly exports exactly one behavioral facade — `AnimationApi` —
/// alongside the `ids` value vocabulary (Module Law #8).
#[test]
fn animation_api_is_the_only_public_facade() {
    let lib_rs = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/lib.rs")).unwrap();
    let facade_exports: Vec<&str> = lib_rs
        .lines()
        .map(str::trim)
        .filter(|l| l.starts_with("pub use "))
        .filter(|l| !l.contains("ids::"))
        .collect();
    assert_eq!(
        facade_exports,
        vec!["pub use animation_api::AnimationApi;"],
        "exactly one non-ids facade export expected"
    );
}
