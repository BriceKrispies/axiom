//! The ported clip sampler, pinned against the JavaScript it came from.
//!
//! Every `expected` value below was captured by running the original
//! `C:/dev/Claude-of-Duty/src/weapons/clips.js` under Node (v24) —
//! `buildClips(nodes, def)` for a synthetic rifle-shaped rig (with a
//! `chargeRest`) and a synthetic pistol-shaped rig (without one, to exercise
//! the slide-rack `lhand` branch), then `Clip.sample(t, out)` at a fixed set
//! of times: exactly on keyframes, between them, before the track starts, and
//! past its end. They are golden values, not recomputations — see
//! `tests/core_port.rs` and `tests/weapons_mathx_port.rs` for the same
//! discipline.
//!
//! Every value here is asserted with **exact** `f64` equality, not a
//! tolerance: the whole sampler — `lerp`, `clamp01`, `smootherstep`,
//! `ease_out_cubic`, `ease_out_back` — is built only from `+ - * /` and
//! comparisons (no `sin`/`cos`/`ln`/`sqrt`/`exp` anywhere on this path), so
//! there is no libm cross-implementation risk to tolerate.

use axiom_claude_of_duty::weapons::clips::{
    build_clips, make_sample_result, AttachNodes, Clip, GripNode, Pose, PosNode,
};
use axiom_claude_of_duty::weapons::defs::{PISTOL, RIFLE};

/// Mirrors the synthetic `nodesWithCharge` rig used in the capture script —
/// the real rig (`viewmodel.js`/`models/rifle.js`) is not ported yet, so
/// these numbers only need to agree between the JS capture and this test.
fn rifle_nodes() -> AttachNodes {
    AttachNodes {
        grip_l: GripNode {
            pos: [-0.1, 0.0734, 0.0672],
            finger: Some([0.8977, -0.3267, -0.2955]),
            back: Some([-0.2784, -0.7648, 0.581]),
        },
        mag_seat: PosNode { pos: [0.0, 0.061, -0.09] },
        charge_rest: Some(PosNode { pos: [0.01, 0.045, 0.12] }),
    }
}

/// Mirrors the synthetic `nodesNoCharge` rig — no `chargeRest`, so
/// `build_clips` takes the pistol slide-rack `lhand` branch.
fn pistol_nodes() -> AttachNodes {
    AttachNodes {
        grip_l: GripNode {
            pos: [0.02, 0.05, 0.03],
            finger: Some([0.7, -0.2, 0.5]),
            back: Some([0.1, 0.9, 0.3]),
        },
        mag_seat: PosNode { pos: [0.0, 0.03, -0.06] },
        charge_rest: None,
    }
}

fn sample_at(clip: &Clip, t: f64) -> axiom_claude_of_duty::weapons::clips::SampleResult {
    let mut out = make_sample_result();
    clip.sample(t, &mut out);
    out
}

// ---------------------------------------------------------------------------
// event times — pure `t * scale` arithmetic, pinned per clip
// ---------------------------------------------------------------------------

#[test]
fn reload_tac_events_match_the_source() {
    let clips = build_clips(&rifle_nodes(), &RIFLE);
    let events: Vec<(&str, f64)> = clips
        .reload_tac
        .events
        .iter()
        .map(|e| (e.name, e.t))
        .collect();
    assert_eq!(
        events,
        vec![
            ("start", 0.042),
            ("magout", 0.42000000000000004),
            ("magdrop", 0.7140000000000001),
            ("magin", 1.7010000000000003),
            ("slap", 1.848),
            ("end", 2.0895),
        ]
    );
}

#[test]
fn reload_empty_events_match_the_source() {
    let clips = build_clips(&rifle_nodes(), &RIFLE);
    let events: Vec<(&str, f64)> = clips
        .reload_empty
        .events
        .iter()
        .map(|e| (e.name, e.t))
        .collect();
    assert_eq!(
        events,
        vec![
            ("start", 0.057999999999999996),
            ("magout", 0.46399999999999997),
            ("magdrop", 0.87),
            ("magin", 2.0589999999999997),
            ("charge", 2.61),
            ("boltrelease", 2.6593),
            ("end", 2.8855),
        ]
    );
}

#[test]
fn inspect_draw_holster_events_match_the_source() {
    let clips = build_clips(&rifle_nodes(), &RIFLE);
    assert_eq!(clips.inspect.events[0].name, "end");
    assert_eq!(clips.inspect.events[0].t, 3.184);
    assert_eq!(clips.draw.events[0].t, 0.6169);
    // `0.995 * 0.4` — pinned as the literal Node capture printed it.
    assert_eq!(clips.holster.events[0].t, 0.398);
}

// ---------------------------------------------------------------------------
// reloadTac (rifle) — before start, on keys, mid-segment, the 'back' segment
// ---------------------------------------------------------------------------

#[test]
fn reload_tac_before_start_holds_the_first_keyframe() {
    // sampleTrack's forward scan can't go below index 0, so t < 0 still
    // brackets [key0, key1] with w clamped to 0 — the sampled value equals
    // key0's fields exactly. `clips.js:30-41`.
    let clips = build_clips(&rifle_nodes(), &RIFLE);
    let out = sample_at(&clips.reload_tac, -0.1);
    assert_eq!(out.pos, [0.0, 0.0, 0.0]);
    assert_eq!(out.rot, [0.0, 0.0, 0.0]);
    assert_eq!(out.lhand.pos, [-0.1, 0.0734, 0.0672]);
    assert_eq!(out.lhand.pose, Pose::Wrap);
    assert_eq!(out.parts.mag, 0.0);
    assert!(out.parts.mag_visible);
}

#[test]
fn reload_tac_at_t_zero_matches_the_first_keyframe() {
    let clips = build_clips(&rifle_nodes(), &RIFLE);
    let out = sample_at(&clips.reload_tac, 0.0);
    assert_eq!(out.pos, [0.0, 0.0, 0.0]);
    assert_eq!(out.lhand.pos, [-0.1, 0.0734, 0.0672]);
    assert_eq!(out.lhand.pose, Pose::Wrap);
}

#[test]
fn reload_tac_exactly_on_a_keyframe_matches_it_exactly() {
    let clips = build_clips(&rifle_nodes(), &RIFLE);
    let out = sample_at(&clips.reload_tac, 0.12 * 2.1);
    assert_eq!(out.pos, [0.014, -0.026, 0.03]);
    assert_eq!(out.rot, [-0.14, 0.3, 0.42]);
    assert_eq!(out.lhand.pos, [0.012, -0.070_44, -0.078]);
    assert_eq!(out.lhand.pose, Pose::Pinch);
}

#[test]
fn reload_tac_midway_between_keyframes_uses_smootherstep() {
    // 0.3*tac sits between the 0.12*tac and 0.5*tac weapon keys, and between
    // the 0.2*tac and 0.3*tac lhand keys (landing exactly on the latter).
    let clips = build_clips(&rifle_nodes(), &RIFLE);
    let out = sample_at(&clips.reload_tac, 0.3 * 2.1);
    assert_eq!(
        out.pos,
        [0.014901497880335157, -0.027802995760670312, 0.028197004239329686]
    );
    assert_eq!(
        out.rot,
        [-0.1219700423932969, 0.3180299576067031, 0.4560599152134062]
    );
    assert_eq!(out.lhand.pos, [0.05, -0.257, 0.0]);
    assert_eq!(out.parts.mag, 1.0);
}

#[test]
fn reload_tac_inside_the_back_eased_segment() {
    // 0.75*tac sits between the 0.72*tac key and the 0.78*tac ('back') key.
    let clips = build_clips(&rifle_nodes(), &RIFLE);
    let out = sample_at(&clips.reload_tac, 0.75 * 2.1);
    assert_eq!(
        out.pos,
        [0.007799999999999999, -0.007299999999999996, 0.013599999999999998]
    );
    assert_eq!(
        out.rot,
        [-0.046499999999999986, 0.176, 0.2929999999999999]
    );
}

// ---------------------------------------------------------------------------
// the source-quirk snap — literal `t: 1` outraces the tac-scaled keys
// ---------------------------------------------------------------------------

#[test]
fn reload_tac_weapon_channel_snaps_early_because_the_final_key_ignores_the_time_scale() {
    // Source quirk (see `clips::build_clips`'s doc comment): the weapon
    // track's final keyframe is authored at the literal `t: 1`, not
    // `1 * tac`. For the rifle (tac = 2.1) that puts it *before* the
    // second-to-last ('back'-eased) key at `0.78 * tac` = 1.638. The moment
    // sampled time first reaches 1.638, `locate`'s forward scan also
    // satisfies `keys[last].t (== 1) <= t` and jumps straight to the final
    // (neutral) key — the 'back' overshoot is never actually interpolated
    // through; it pops from "approaching overshoot" to "exactly neutral" in
    // one sample.
    let clips = build_clips(&rifle_nodes(), &RIFLE);
    let boundary = 0.78 * 2.1;

    // Just before the boundary: still easing toward the 'back' key, NOT at
    // neutral.
    let just_before = sample_at(&clips.reload_tac, boundary - 1e-6);
    assert_eq!(
        just_before.pos,
        [0.007999999999647272, -0.00799999999876545, 0.013999999999294544]
    );
    assert_ne!(just_before.pos, [0.0, 0.0, 0.0]);

    // Exactly at the boundary: already snapped to neutral.
    let at_boundary = sample_at(&clips.reload_tac, boundary);
    assert_eq!(at_boundary.pos, [0.0, 0.0, 0.0]);
    assert_eq!(at_boundary.rot, [0.0, 0.0, 0.0]);

    // Just after, and for the rest of the nominal duration: stays neutral.
    let just_after = sample_at(&clips.reload_tac, boundary + 1e-6);
    assert_eq!(just_after.pos, [0.0, 0.0, 0.0]);
    let well_past = sample_at(&clips.reload_tac, 0.9 * 2.1);
    assert_eq!(well_past.pos, [0.0, 0.0, 0.0]);
    let past_duration = sample_at(&clips.reload_tac, 5.0);
    assert_eq!(past_duration.pos, [0.0, 0.0, 0.0]);
    assert_eq!(past_duration.lhand.pose, Pose::Wrap);
}

#[test]
fn reload_tac_at_the_nominal_t_equals_one_is_not_yet_the_quirk_boundary() {
    // t = 1.0 is well short of the weapon track's own 0.78*tac = 1.638
    // boundary (tac = 2.1), so this is ordinary mid-segment interpolation
    // between the 0.12*tac and 0.5*tac keys — not the snap.
    let clips = build_clips(&rifle_nodes(), &RIFLE);
    let out = sample_at(&clips.reload_tac, 1.0);
    assert_eq!(
        out.pos,
        [0.015995531164375354, -0.02999106232875071, 0.026008937671249288]
    );
    assert_eq!(
        out.rot,
        [-0.1000893767124929, 0.3399106232875071, 0.4998212465750142]
    );
    assert_eq!(out.lhand.pos, [0.11, -0.363, 0.07]);
    assert_eq!(out.lhand.pose, Pose::Open);
    assert_eq!(out.parts.mag, 1.0);
}

// ---------------------------------------------------------------------------
// lhand pose switching: `w < 0.5 ? a.pose : b.pose`
// ---------------------------------------------------------------------------

#[test]
fn lhand_pose_switches_at_the_segment_midpoint() {
    let clips = build_clips(&rifle_nodes(), &RIFLE);
    // 0.81*tac sits just past the 0.8*tac ('pinch') key, close to it, so
    // w < 0.5 and the pose is still the source key's ('pinch').
    let near_a = sample_at(&clips.reload_tac, 0.81 * 2.1);
    assert_eq!(near_a.lhand.pos, [0.0, -0.07086592592592594, -0.09]);
    assert_eq!(near_a.lhand.pose, Pose::Pinch);

    // 0.855*tac sits close to the 0.86*tac ('open') key, so w >= 0.5 and the
    // pose has already switched to the destination key's ('open').
    let near_b = sample_at(&clips.reload_tac, 0.855 * 2.1);
    assert_eq!(near_b.lhand.pos, [0.0, -0.08237894675925926, -0.09]);
    assert_eq!(near_b.lhand.pose, Pose::Open);
}

#[test]
fn reload_tac_parts_hides_the_magazine_mid_segment() {
    let clips = build_clips(&rifle_nodes(), &RIFLE);
    let out = sample_at(&clips.reload_tac, 0.5 * 2.1);
    assert_eq!(out.pos, [0.016, -0.03, 0.026]);
    assert_eq!(out.rot, [-0.1, 0.34, 0.5]);
    assert_eq!(out.parts.mag, 1.0);
    assert!(out.parts.mag_visible);
}

// ---------------------------------------------------------------------------
// reloadEmpty (rifle, has a chargeRest) — the back-eased weapon segment and
// the 'back'-eased bolt-release parts key
// ---------------------------------------------------------------------------

#[test]
fn reload_empty_rifle_at_magin_matches_the_source() {
    let clips = build_clips(&rifle_nodes(), &RIFLE);
    let out = sample_at(&clips.reload_empty, 0.71 * 2.9);
    assert_eq!(
        out.pos,
        [0.009800000000000007, -0.013400000000000018, 0.017700000000000007]
    );
    assert_eq!(
        out.rot,
        [-0.05600000000000012, 0.2370000000000001, 0.37500000000000017]
    );
    assert_eq!(out.parts.bolt, 1.0);
    assert_eq!(out.parts.slide, 1.0);
}

#[test]
fn reload_empty_rifle_at_boltrelease_uses_back_easing() {
    let clips = build_clips(&rifle_nodes(), &RIFLE);
    let out = sample_at(&clips.reload_empty, 0.915 * 2.9);
    assert_eq!(
        out.pos,
        [0.004166666666666666, -0.006499999999999998, 0.021500000000000002]
    );
    assert_eq!(
        out.rot,
        [0.01666666666666668, 0.43833333333333335, 0.18333333333333332]
    );
    assert_eq!(out.lhand.pos, [-0.01, 0.053, 0.1762499999999999]);
    assert_eq!(out.lhand.finger, [0.55, 0.2, 0.81]);
    assert_eq!(out.lhand.pose, Pose::Open);
}

#[test]
fn reload_empty_rifle_at_t_one_matches_the_source() {
    let clips = build_clips(&rifle_nodes(), &RIFLE);
    let out = sample_at(&clips.reload_empty, 1.0);
    assert_eq!(
        out.pos,
        [0.017724902177608734, -0.03344980435521747, 0.02855019564478253]
    );
    assert_eq!(
        out.rot,
        [-0.1255019564478253, 0.3744980435521747, 0.5289960871043494]
    );
    assert_eq!(
        out.lhand.pos,
        [0.10835235879538486, -0.3600891672051799, 0.06807775192794902]
    );
    assert_eq!(out.parts.bolt, 1.0);
    assert_eq!(out.parts.slide, 1.0);
}

// ---------------------------------------------------------------------------
// reloadEmpty (pistol, no chargeRest) — the slide-rack lhand branch
// ---------------------------------------------------------------------------

#[test]
fn reload_empty_pistol_takes_the_slide_rack_branch() {
    let clips = build_clips(&pistol_nodes(), &PISTOL);
    let out = sample_at(&clips.reload_empty, 0.85 * 2.2);
    assert_eq!(
        out.pos,
        [0.006013060034509428, -0.012006530017254715, 0.016006530017254713]
    );
    assert_eq!(
        out.rot,
        [-0.020130600345094278, 0.4194122984470757, 0.2205224013803771]
    );
    assert_eq!(out.lhand.pos, [-0.02, 0.09, -0.095]);
    assert_eq!(out.lhand.finger, [0.7, -0.3, 0.65]);
    assert_eq!(out.lhand.back, [0.1, 0.94, 0.32]);
    assert_eq!(out.lhand.pose, Pose::Pinch);
    assert_eq!(out.parts.bolt, 1.0);
    assert_eq!(out.parts.slide, 1.0);
}

#[test]
fn reload_empty_pistol_charge_key_fires_without_a_charge_handle() {
    // Even though the pistol has no `chargeRest`, the `parts.charge` field
    // still animates (it drives the striker/slide-release cue, not the
    // charging-handle hand path) — `emptyParts`, `clips.js:230`.
    let clips = build_clips(&pistol_nodes(), &PISTOL);
    let out = sample_at(&clips.reload_empty, 0.9 * 2.2);
    assert_eq!(
        out.pos,
        [0.004666666666666669, -0.008000000000000007, 0.019999999999999993]
    );
    assert_eq!(out.lhand.pos, [-0.02, 0.09, -0.11499999999999996]);
    assert_eq!(out.lhand.pose, Pose::Open);
    assert_eq!(out.parts.charge, 1.0);
}

// ---------------------------------------------------------------------------
// inspect / draw / holster — draw and holster have durations < 1s, so their
// literal `t: 1` final keys are NOT affected by the source quirk above.
// ---------------------------------------------------------------------------

#[test]
fn inspect_matches_the_source_mid_clip() {
    let clips = build_clips(&rifle_nodes(), &RIFLE);
    let out = sample_at(&clips.inspect, 0.2 * 3.2);
    assert_eq!(
        out.pos,
        [-0.02969435553523345, -0.011541533302850175, 0.07576411116191638]
    );
    assert_eq!(
        out.rot,
        [0.08853833257125436, -0.632225778590662, -0.35222577859066206]
    );
    assert_eq!(out.lhand.pose, Pose::Clamp);
}

#[test]
fn draw_matches_the_source_at_start_mid_and_end() {
    let clips = build_clips(&rifle_nodes(), &RIFLE);

    let start = sample_at(&clips.draw, 0.0);
    assert_eq!(start.pos, [0.05, -0.3, 0.14]);
    assert_eq!(start.rot, [-0.85, 0.5, 0.55]);
    assert_eq!(start.lhand.pose, Pose::Open);

    let mid = sample_at(&clips.draw, 0.55 * 0.62);
    assert_eq!(mid.pos, [0.01, -0.03, 0.02]);
    assert_eq!(mid.rot, [-0.1, 0.06, 0.06]);
    assert_eq!(mid.lhand.pose, Pose::Wrap);

    // draw_time (0.62) < 1, so the literal `t: 1` final key IS the largest
    // keyframe time in this track — no source-quirk snap here.
    let end = sample_at(&clips.draw, 1.0);
    assert_eq!(end.pos, [0.0, 0.0, 0.0]);
    assert_eq!(end.lhand.pos, [-0.1, 0.0734, 0.0672]);
    assert_eq!(end.lhand.pose, Pose::Wrap);
}

#[test]
fn holster_matches_the_source_mid_and_end() {
    let clips = build_clips(&rifle_nodes(), &RIFLE);

    let mid = sample_at(&clips.holster, 0.25 * 0.4);
    assert_eq!(mid.pos, [0.004, 0.014, -0.01]);
    assert_eq!(mid.rot, [0.08, -0.04, -0.05]);

    let end = sample_at(&clips.holster, 1.0);
    assert_eq!(end.pos, [0.05, -0.32, 0.15]);
    assert_eq!(end.rot, [-0.9, 0.55, 0.6]);
    assert_eq!(end.lhand.pose, Pose::Open);
}

// ---------------------------------------------------------------------------
// make_sample_result / SampleResult defaults
// ---------------------------------------------------------------------------

#[test]
fn make_sample_result_matches_the_source_defaults() {
    // `makeSampleResult()`, `clips.js:100-108`.
    let out = make_sample_result();
    assert!(!out.active);
    assert_eq!(out.pos, [0.0, 0.0, 0.0]);
    assert_eq!(out.rot, [0.0, 0.0, 0.0]);
    assert_eq!(out.lhand.pos, [0.0, 0.0, 0.0]);
    assert_eq!(out.lhand.pose, Pose::Wrap);
    assert_eq!(out.lhand.weight, 0.0);
    assert_eq!(out.parts.mag, 0.0);
    assert!(out.parts.mag_visible);
}

#[test]
fn sample_sets_active_true() {
    let clips = build_clips(&rifle_nodes(), &RIFLE);
    let out = sample_at(&clips.draw, 0.0);
    assert!(out.active);
}
